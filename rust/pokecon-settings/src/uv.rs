use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use atomic_write_file::OpenOptions;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::MANAGED_UV_SOURCE_JSON;
use crate::roots::{EffectiveRoots, RootEnvironment};

/// Verified source for the application-managed pinned uv binary.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedUvSource {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
}

impl ManagedUvSource {
    /// Loads the build-time pinned uv identity.
    ///
    /// Compile-only environments may omit the bundle and return `None`; a
    /// packaged application treats that as a startup error.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded build artifact is malformed.
    pub fn bundled() -> Result<Option<Self>, UvError> {
        serde_json::from_str(MANAGED_UV_SOURCE_JSON).map_err(UvError::EmbeddedMetadata)
    }
}

/// Prepared managed uv identity under the effective Data root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedUv {
    pub path: PathBuf,
    pub build_id: String,
    pub sha256: String,
}

impl ManagedUv {
    /// Copies and verifies pinned uv under Data using atomic replacement.
    ///
    /// # Errors
    ///
    /// Returns an error if the source is missing/non-regular, its digest does
    /// not match, or the managed copy cannot be committed.
    pub fn prepare(roots: &EffectiveRoots, source: &ManagedUvSource) -> Result<Self, UvError> {
        if !source.path.is_file() {
            return Err(UvError::MissingManagedUv(source.path.clone()));
        }
        let source_digest = digest_file(&source.path)?;
        if !source_digest.eq_ignore_ascii_case(&source.sha256) {
            return Err(UvError::IntegrityMismatch);
        }
        let executable_name = if cfg!(target_os = "windows") {
            "uv.exe"
        } else {
            "uv"
        };
        let destination = roots
            .data
            .join("uv")
            .join(&source.version)
            .join(executable_name);
        if destination.is_file() && digest_file(&destination)? == source_digest {
            return Ok(Self {
                path: destination,
                build_id: source.version.clone(),
                sha256: source_digest,
            });
        }
        let parent = destination
            .parent()
            .ok_or_else(|| UvError::InvalidManagedPath(destination.clone()))?;
        fs::create_dir_all(parent).map_err(|source| UvError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let mut options = OpenOptions::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o755);
        }
        let mut output = options.open(&destination).map_err(|source| UvError::Io {
            path: destination.clone(),
            source,
        })?;
        let mut input = fs::File::open(&source.path).map_err(|error| UvError::Io {
            path: source.path.clone(),
            source: error,
        })?;
        std::io::copy(&mut input, &mut output).map_err(|source| UvError::Io {
            path: destination.clone(),
            source,
        })?;
        output.commit().map_err(|source| UvError::Io {
            path: destination.clone(),
            source,
        })?;
        if digest_file(&destination)? != source_digest {
            return Err(UvError::IntegrityMismatch);
        }
        Ok(Self {
            path: destination,
            build_id: source.version.clone(),
            sha256: source_digest,
        })
    }
}

fn digest_file(path: &Path) -> Result<String, UvError> {
    let mut file = fs::File::open(path).map_err(|source| UvError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| UvError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Secret-aware uv child environment built from an empty base.
#[derive(Clone, Eq, PartialEq)]
pub struct UvChildEnvironment {
    values: BTreeMap<OsString, OsString>,
    bridged_secret_values: BTreeMap<String, Vec<u8>>,
}

impl UvChildEnvironment {
    /// Builds the deterministic inheritance/bridge sequence.
    ///
    /// Ambient `UV_*` and original `POKECON_UV_*` variables never survive.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty `POKECON_UV_` suffix or a non-Unicode
    /// bridge name.
    pub fn build(environment: &RootEnvironment) -> Result<Self, UvError> {
        const MINIMUM: &[&str] = &[
            "PATH",
            "HOME",
            "USERPROFILE",
            "TMPDIR",
            "TEMP",
            "TMP",
            "SYSTEMROOT",
            "COMSPEC",
            "PATHEXT",
        ];
        const NETWORK: &[&str] = &[
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
        ];
        let mut values = BTreeMap::new();
        for name in MINIMUM.iter().chain(NETWORK) {
            if let Some(value) = environment.get(name) {
                values.insert(OsString::from(name), value.to_os_string());
            }
        }
        values.retain(|name, _| !name.to_string_lossy().starts_with("UV_"));
        let mut bridged_secret_values = BTreeMap::new();
        for (name, value) in environment.iter() {
            let Some(suffix) = name.strip_prefix("POKECON_UV_") else {
                continue;
            };
            if suffix.is_empty() {
                return Err(UvError::EmptyBridgeSuffix);
            }
            let child_name = format!("UV_{suffix}");
            values.insert(OsString::from(&child_name), value.to_os_string());
            bridged_secret_values.insert(child_name, os_bytes(value));
        }
        Ok(Self {
            values,
            bridged_secret_values,
        })
    }

    /// Iterates the exact child environment.
    pub fn iter(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }

    /// Returns keyed-fingerprint inputs. Callers must never log the values.
    #[must_use]
    pub const fn bridged_secret_values(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.bridged_secret_values
    }

    /// Looks up a child variable for contract tests and process construction.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.values.get(OsStr::new(name)).map(OsString::as_os_str)
    }
}

impl fmt::Debug for UvChildEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UvChildEnvironment")
            .field("names", &self.values.keys().collect::<Vec<_>>())
            .field("values", &"<redacted>")
            .field("bridged_secret_values", &"<redacted>")
            .finish()
    }
}

/// One direct, shell-free managed uv invocation.
#[derive(Clone, Eq, PartialEq)]
pub struct UvInvocation {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment: UvChildEnvironment,
}

impl fmt::Debug for UvInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UvInvocation")
            .field("program", &self.program)
            .field("arguments", &"<redacted>")
            .field("environment", &self.environment)
            .finish()
    }
}

/// Inputs for construction of exact-sync commands.
#[derive(Clone, Debug)]
pub struct UvPlanInput {
    pub managed_uv: ManagedUv,
    pub python: PathBuf,
    pub venv: PathBuf,
    pub cache: PathBuf,
    pub requirements: PathBuf,
    pub compiled: PathBuf,
    pub constraints: Option<PathBuf>,
    pub overrides: Option<PathBuf>,
    pub uv_config: Option<PathBuf>,
    pub environment: UvChildEnvironment,
}

/// Constructs invocations with app invariants expressed as final CLI inputs.
#[derive(Clone, Debug)]
pub struct UvPlan {
    pub create_venv: UvInvocation,
    pub compile: UvInvocation,
    pub sync: UvInvocation,
    pub check: UvInvocation,
    pub inventory: UvInvocation,
}

impl UvPlan {
    #[must_use]
    pub fn new(input: &UvPlanInput) -> Self {
        let global = |arguments: &mut Vec<OsString>| {
            if let Some(config) = &input.uv_config {
                arguments.push(OsString::from("--config-file"));
                arguments.push(config.as_os_str().to_os_string());
            } else {
                arguments.push(OsString::from("--no-config"));
            }
        };
        let invocation = |arguments: Vec<OsString>| UvInvocation {
            program: input.managed_uv.path.clone(),
            arguments,
            environment: input.environment.clone(),
        };

        let mut create = Vec::new();
        global(&mut create);
        create.extend([
            OsString::from("venv"),
            input.venv.as_os_str().to_os_string(),
            OsString::from("--python"),
            input.python.as_os_str().to_os_string(),
            OsString::from("--cache-dir"),
            input.cache.as_os_str().to_os_string(),
        ]);

        let mut compile = Vec::new();
        global(&mut compile);
        compile.extend([
            OsString::from("pip"),
            OsString::from("compile"),
            input.requirements.as_os_str().to_os_string(),
        ]);
        if let Some(constraints) = &input.constraints {
            compile.extend([
                OsString::from("--constraints"),
                constraints.as_os_str().to_os_string(),
            ]);
        }
        if let Some(overrides) = &input.overrides {
            compile.extend([
                OsString::from("--overrides"),
                overrides.as_os_str().to_os_string(),
            ]);
        }
        compile.extend([
            OsString::from("--python"),
            input.python.as_os_str().to_os_string(),
            OsString::from("--cache-dir"),
            input.cache.as_os_str().to_os_string(),
            OsString::from("--output-file"),
            input.compiled.as_os_str().to_os_string(),
        ]);

        let venv_python = venv_python(&input.venv);
        let mut sync = Vec::new();
        global(&mut sync);
        sync.extend([
            OsString::from("pip"),
            OsString::from("sync"),
            input.compiled.as_os_str().to_os_string(),
            OsString::from("--python"),
            venv_python.as_os_str().to_os_string(),
            OsString::from("--cache-dir"),
            input.cache.as_os_str().to_os_string(),
            OsString::from("--strict"),
        ]);

        let mut check = Vec::new();
        global(&mut check);
        check.extend([
            OsString::from("pip"),
            OsString::from("check"),
            OsString::from("--python"),
            venv_python.as_os_str().to_os_string(),
        ]);
        let mut inventory = Vec::new();
        global(&mut inventory);
        inventory.extend([
            OsString::from("pip"),
            OsString::from("list"),
            OsString::from("--format"),
            OsString::from("json"),
            OsString::from("--python"),
            venv_python.as_os_str().to_os_string(),
        ]);
        Self {
            create_venv: invocation(create),
            compile: invocation(compile),
            sync: invocation(sync),
            check: invocation(check),
            inventory: invocation(inventory),
        }
    }
}

#[must_use]
pub fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        venv.join("Scripts/python.exe")
    } else {
        venv.join("bin/python")
    }
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(not(any(unix, windows)))]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

/// Managed uv preparation/environment failures. Secret values are never
/// included.
#[derive(Debug, Error)]
pub enum UvError {
    #[error("embedded managed uv metadata is invalid")]
    EmbeddedMetadata(#[source] serde_json::Error),
    #[error("managed uv source is missing or not a regular file: {0}")]
    MissingManagedUv(PathBuf),
    #[error("managed uv integrity verification failed")]
    IntegrityMismatch,
    #[error("managed uv destination path is invalid: {0}")]
    InvalidManagedPath(PathBuf),
    #[error("POKECON_UV_ bridge variable has an empty suffix")]
    EmptyBridgeSuffix,
    #[error("managed uv I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use super::{ManagedUv, ManagedUvSource, UvChildEnvironment};
    use crate::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};

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

    #[test]
    fn pinned_uv_is_verified_and_copied_under_data() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must exist");
        let source = temp.path().join("bundled-uv");
        fs::write(&source, b"pinned uv fixture").expect("fixture must be written");
        let digest = hex::encode(Sha256::digest(b"pinned uv fixture"));
        let managed = ManagedUv::prepare(
            &roots,
            &ManagedUvSource {
                path: source,
                version: "test-build".to_owned(),
                sha256: digest.clone(),
            },
        )
        .expect("managed uv must be prepared");
        assert!(managed.path.starts_with(&roots.data));
        assert_eq!(managed.sha256, digest);
    }

    #[test]
    fn child_environment_removes_ambient_uv_and_bridges_only_prefixed_values() {
        let token = "https://user:token@example.invalid/simple";
        let environment = RootEnvironment::from_values([
            ("PATH", "/bin"),
            ("UV_INDEX_URL", "ambient"),
            ("POKECON_UV_INDEX_URL", token),
            ("UNRELATED", "ignored"),
            ("HTTPS_PROXY", "proxy-secret"),
        ]);
        let child = UvChildEnvironment::build(&environment).expect("environment must build");
        assert_eq!(child.get("UV_INDEX_URL"), Some(std::ffi::OsStr::new(token)));
        assert!(child.get("POKECON_UV_INDEX_URL").is_none());
        assert!(child.get("UNRELATED").is_none());
        assert!(!format!("{child:?}").contains(token));
        assert!(!format!("{child:?}").contains("proxy-secret"));
    }

    #[test]
    fn empty_uv_bridge_suffix_is_a_startup_error() {
        let environment = RootEnvironment::from_values([("POKECON_UV_", "secret")]);
        assert!(UvChildEnvironment::build(&environment).is_err());
    }
}
