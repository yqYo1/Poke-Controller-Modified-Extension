//! App-managed `CPython` selection and non-Nix python-build-standalone setup.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::process::Command;

use crate::lock::{LockError, LockManager};
use crate::roots::EffectiveRoots;
use crate::uv::{ManagedUv, UvChildEnvironment};

/// Exact python-build-standalone interpreter shipped by release artifacts.
pub const PORTABLE_PYTHON_VERSION: &str = "3.14.3";
const PACKAGED_RUNTIME_MARKER: &str = ".pokecon-runtime.sha256";

#[derive(Deserialize)]
struct ResourceManifestIdentity {
    content_sha256: String,
}

/// One prepared `CPython` interpreter and its fingerprint identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedPython {
    pub path: PathBuf,
    pub build_id: String,
}

/// Selects Nix `CPython` or prepares python-build-standalone under Data.
///
/// Packaged release resources take priority over the compile-time Nix path so
/// a Tauri package cannot accidentally use a build host store path. If a
/// non-Nix build has no bundled distribution, pinned uv downloads the exact
/// python-build-standalone version into the effective Data root.
///
/// # Errors
///
/// Returns an error when a packaged runtime is malformed, installation cannot
/// be committed, or pinned uv cannot install the fallback distribution.
pub async fn prepare_managed_python(
    resource_root: &Path,
    roots: &EffectiveRoots,
    managed_uv: &ManagedUv,
    environment: &UvChildEnvironment,
    build_python: Option<&Path>,
) -> Result<ManagedPython, PythonError> {
    if let Some(source) = packaged_source(resource_root)? {
        let roots = roots.clone();
        return tokio::task::spawn_blocking(move || materialize_packaged(&source, &roots))
            .await
            .map_err(|_| PythonError::Task)?;
    }
    if let Some(path) = build_python.filter(|path| path.is_file()) {
        return Ok(ManagedPython {
            path: path.to_path_buf(),
            build_id: format!("cpython-3.14-nix:{}", path.display()),
        });
    }
    install_with_uv(roots, managed_uv, environment).await
}

#[derive(Clone, Debug)]
struct PackagedSource {
    root: PathBuf,
    identity: String,
}

fn packaged_source(resource_root: &Path) -> Result<Option<PackagedSource>, PythonError> {
    let root = resource_root.join("python");
    match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(PythonError::InvalidPackagedRuntime(root)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(PythonError::Io { path: root, source }),
    }
    python_executable(&root)?;
    let manifest_path = resource_root.join("resource-manifest.json");
    let source = fs::read_to_string(&manifest_path).map_err(|source| PythonError::Io {
        path: manifest_path,
        source,
    })?;
    let manifest: ResourceManifestIdentity =
        serde_json::from_str(&source).map_err(PythonError::ResourceManifest)?;
    if manifest.content_sha256.len() != 64
        || !manifest
            .content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PythonError::InvalidResourceIdentity);
    }
    Ok(Some(PackagedSource {
        root,
        identity: manifest.content_sha256,
    }))
}

fn materialize_packaged(
    source: &PackagedSource,
    roots: &EffectiveRoots,
) -> Result<ManagedPython, PythonError> {
    let parent = roots.data.join("python");
    let destination = parent.join(format!("cpython-{PORTABLE_PYTHON_VERSION}"));
    fs::create_dir_all(&parent).map_err(|source| PythonError::Io {
        path: parent.clone(),
        source,
    })?;
    let locks = LockManager::new(roots);
    let _guard = locks.runtime(&destination)?;
    let marker = destination.join(PACKAGED_RUNTIME_MARKER);
    if fs::read_to_string(&marker).is_ok_and(|identity| identity.trim() == source.identity)
        && python_executable(&destination).is_ok()
    {
        return managed_python(&destination, &source.identity);
    }

    let staging = tempfile::Builder::new()
        .prefix(".python-stage-")
        .tempdir_in(&parent)
        .map_err(|source| PythonError::Io {
            path: parent.clone(),
            source,
        })?;
    let staged = staging.path().join("runtime");
    copy_directory(&source.root, &staged)?;
    fs::write(staged.join(PACKAGED_RUNTIME_MARKER), &source.identity).map_err(|source| {
        PythonError::Io {
            path: staged.join(PACKAGED_RUNTIME_MARKER),
            source,
        }
    })?;
    commit_runtime(staging, &staged, &destination)?;
    managed_python(&destination, &source.identity)
}

fn managed_python(root: &Path, identity: &str) -> Result<ManagedPython, PythonError> {
    Ok(ManagedPython {
        path: python_executable(root)?,
        build_id: format!(
            "cpython-{PORTABLE_PYTHON_VERSION}:{}",
            identity.to_ascii_lowercase()
        ),
    })
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), PythonError> {
    fs::create_dir(destination).map_err(|source| PythonError::Io {
        path: destination.to_path_buf(),
        source,
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|source_error| PythonError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source_error| PythonError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|source| PythonError::Io {
            path: source_path.clone(),
            source,
        })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(PythonError::InvalidPackagedRuntime(source_path));
        }
        if file_type.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|source| PythonError::Io {
                path: destination_path.clone(),
                source,
            })?;
            fs::set_permissions(&destination_path, metadata.permissions()).map_err(|source| {
                PythonError::Io {
                    path: destination_path,
                    source,
                }
            })?;
        } else {
            return Err(PythonError::InvalidPackagedRuntime(source_path));
        }
    }
    Ok(())
}

fn commit_runtime(staging: TempDir, staged: &Path, destination: &Path) -> Result<(), PythonError> {
    if !destination.exists() {
        fs::rename(staged, destination).map_err(|source| PythonError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
        drop(staging);
        return Ok(());
    }
    let previous = staging.path().join("previous");
    fs::rename(destination, &previous).map_err(|source| PythonError::Io {
        path: destination.to_path_buf(),
        source,
    })?;
    if let Err(source) = fs::rename(staged, destination) {
        if fs::rename(&previous, destination).is_err() {
            let _preserved = staging.keep();
            return Err(PythonError::ConcurrentMutation(destination.to_path_buf()));
        }
        return Err(PythonError::Io {
            path: destination.to_path_buf(),
            source,
        });
    }
    drop(staging);
    Ok(())
}

async fn install_with_uv(
    roots: &EffectiveRoots,
    managed_uv: &ManagedUv,
    environment: &UvChildEnvironment,
) -> Result<ManagedPython, PythonError> {
    let install_root = roots.data.join("python/downloads");
    fs::create_dir_all(&install_root).map_err(|source| PythonError::Io {
        path: install_root.clone(),
        source,
    })?;
    let mut command = Command::new(&managed_uv.path);
    command
        .args([
            "--no-config",
            "python",
            "install",
            PORTABLE_PYTHON_VERSION,
            "--managed-python",
            "--no-bin",
            "--install-dir",
        ])
        .arg(&install_root)
        .env_clear()
        .envs(environment.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command
        .output()
        .await
        .map_err(|source| PythonError::UvSpawn {
            path: managed_uv.path.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(PythonError::UvFailed);
    }
    let mut candidates = fs::read_dir(&install_root)
        .map_err(|source| PythonError::Io {
            path: install_root.clone(),
            source,
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("cpython-3.14.3-"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    let Some(root) = candidates
        .into_iter()
        .find(|path| python_executable(path).is_ok())
    else {
        return Err(PythonError::UvMissingInstallation(install_root));
    };
    let path = python_executable(&root)?;
    let name = root
        .file_name()
        .map_or_else(|| "unknown".into(), |name| name.to_string_lossy());
    Ok(ManagedPython {
        path,
        build_id: format!("{name}:uv-{}", managed_uv.build_id),
    })
}

fn python_executable(root: &Path) -> Result<PathBuf, PythonError> {
    #[cfg(windows)]
    let candidates = [root.join("python.exe"), root.join("bin/python.exe")];
    #[cfg(not(windows))]
    let candidates = [
        root.join("bin/python3.14"),
        root.join("bin/python3"),
        root.join("bin/python"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| PythonError::MissingExecutable(root.to_path_buf()))
}

/// Managed `CPython` preparation failures.
#[derive(Debug, Error)]
pub enum PythonError {
    #[error("packaged CPython runtime is invalid: {0}")]
    InvalidPackagedRuntime(PathBuf),
    #[error("packaged resource manifest is invalid")]
    ResourceManifest(#[source] serde_json::Error),
    #[error("packaged resource manifest has an invalid content identity")]
    InvalidResourceIdentity,
    #[error("managed CPython runtime has no executable below {0}")]
    MissingExecutable(PathBuf),
    #[error("managed CPython installation changed concurrently: {0}")]
    ConcurrentMutation(PathBuf),
    #[error("managed CPython task failed")]
    Task,
    #[error("managed uv could not be started for CPython installation: {path}: {source}")]
    UvSpawn {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("managed uv failed to install CPython")]
    UvFailed,
    #[error("managed uv did not produce a CPython installation below {0}")]
    UvMissingInstallation(PathBuf),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error("managed CPython I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::prepare_managed_python;
    use crate::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};
    use crate::uv::{ManagedUv, UvChildEnvironment};

    fn roots(temp: &TempDir) -> EffectiveRoots {
        let base = temp.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("bases must resolve");
        EffectiveRoots::from_bases(
            SafeComponent::new("pokecon").expect("name must be safe"),
            &bases,
        )
    }

    fn uv_environment() -> UvChildEnvironment {
        UvChildEnvironment::build(&RootEnvironment::from_values([("PATH", "/bin")]))
            .expect("uv environment must build")
    }

    fn uv() -> ManagedUv {
        ManagedUv {
            path: PathBuf::from("/missing/uv"),
            build_id: "test".to_owned(),
            sha256: "00".repeat(32),
        }
    }

    #[tokio::test]
    async fn packaged_runtime_is_materialized_under_data_before_build_python() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must exist");
        let resources = temp.path().join("resources");
        let source_python = if cfg!(windows) {
            resources.join("python/python.exe")
        } else {
            resources.join("python/bin/python3.14")
        };
        fs::create_dir_all(source_python.parent().expect("Python must have a parent"))
            .expect("Python parent must exist");
        fs::write(&source_python, b"portable-python").expect("Python fixture must exist");
        fs::write(
            resources.join("resource-manifest.json"),
            format!(
                "{{\"content_sha256\":\"{}\"}}",
                "0123456789abcdef".repeat(4)
            ),
        )
        .expect("resource manifest must exist");
        let build_python = temp.path().join("build-python");
        fs::write(&build_python, b"nix-python").expect("build Python must exist");

        let managed = prepare_managed_python(
            &resources,
            &roots,
            &uv(),
            &uv_environment(),
            Some(&build_python),
        )
        .await
        .expect("packaged Python must prepare");
        assert!(managed.path.starts_with(&roots.data));
        assert_eq!(
            fs::read(managed.path).expect("managed Python must be readable"),
            b"portable-python"
        );
    }

    #[tokio::test]
    async fn nix_build_python_is_used_when_no_package_runtime_exists() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must exist");
        let build_python = temp.path().join("build-python");
        fs::write(&build_python, b"nix-python").expect("build Python must exist");
        let managed = prepare_managed_python(
            &temp.path().join("resources"),
            &roots,
            &uv(),
            &uv_environment(),
            Some(&build_python),
        )
        .await
        .expect("build Python must be selected");
        assert_eq!(managed.path, build_python);
    }
}
