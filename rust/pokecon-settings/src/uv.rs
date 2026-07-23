use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use atomic_write_file::OpenOptions;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::MANAGED_UV_SOURCE_JSON;
use crate::roots::{EffectiveRoots, RootEnvironment};

const PACKAGED_UV_CACHE_DIRECTORY: &str = "uv-cache";
const PACKAGED_UV_CACHE_MARKER: &str = ".pokecon-packaged-cache.sha256";
const PACKAGED_WHEELHOUSE_DIRECTORY: &str = "python-wheels";

#[derive(Deserialize)]
struct ResourceManifestIdentity {
    content_sha256: String,
}

/// Installer-provided, immutable Python package source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagedWheelhouse {
    pub path: PathBuf,
    pub content_sha256: String,
}

/// Locates and validates the flat wheelhouse staged by the release pipeline.
///
/// Returns `None` for Nix/developer layouts which intentionally do not carry
/// non-Nix wheels.
///
/// # Errors
///
/// Returns an error for a present but malformed or symlinked wheelhouse.
pub fn packaged_wheelhouse(resource_root: &Path) -> Result<Option<PackagedWheelhouse>, UvError> {
    let path = resource_root.join(PACKAGED_WHEELHOUSE_DIRECTORY);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(UvError::InvalidPackagedWheelhouse(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(UvError::Io { path, source });
        }
    }
    let manifest_path = resource_root.join("resource-manifest.json");
    let manifest_source = fs::read_to_string(&manifest_path).map_err(|source| UvError::Io {
        path: manifest_path,
        source,
    })?;
    let manifest: ResourceManifestIdentity =
        serde_json::from_str(&manifest_source).map_err(UvError::ResourceManifest)?;
    validate_resource_identity(&manifest.content_sha256)?;

    let required = ["requirements.lock", "wheelhouse-manifest.json"];
    for name in required {
        let required_path = path.join(name);
        let metadata = fs::symlink_metadata(&required_path).map_err(|source| UvError::Io {
            path: required_path.clone(),
            source,
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(UvError::InvalidPackagedWheelhouse(required_path));
        }
    }
    let mut wheel_count = 0_usize;
    for entry in fs::read_dir(&path).map_err(|source| UvError::Io {
        path: path.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| UvError::Io {
            path: path.clone(),
            source,
        })?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path).map_err(|source| UvError::Io {
            path: entry_path.clone(),
            source,
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(UvError::InvalidPackagedWheelhouse(entry_path));
        }
        if entry_path.extension() == Some(OsStr::new("whl")) {
            wheel_count += 1;
        }
    }
    if wheel_count == 0 {
        return Err(UvError::InvalidPackagedWheelhouse(path));
    }
    Ok(Some(PackagedWheelhouse {
        path,
        content_sha256: manifest.content_sha256,
    }))
}

fn validate_resource_identity(identity: &str) -> Result<(), UvError> {
    if identity.len() == 64 && identity.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(UvError::InvalidResourceIdentity)
    }
}

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
        let executable = std::env::current_exe().map_err(UvError::CurrentExecutable)?;
        let resource_root = executable.parent().ok_or(UvError::MissingResourceRoot)?;
        Self::bundled_at(resource_root)
    }

    /// Loads the build-time identity and resolves its packaged path against
    /// the runtime resource directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded build artifact is malformed.
    pub fn bundled_at(resource_root: &Path) -> Result<Option<Self>, UvError> {
        let mut source: Option<Self> =
            serde_json::from_str(MANAGED_UV_SOURCE_JSON).map_err(UvError::EmbeddedMetadata)?;
        if let Some(source) = source.as_mut()
            && source.path.is_relative()
        {
            source.path = resource_root.join(&source.path);
        }
        Ok(source)
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
        #[cfg(unix)]
        let options = {
            use std::os::unix::fs::OpenOptionsExt;

            let mut options = OpenOptions::new();
            options.mode(0o755);
            options
        };
        #[cfg(not(unix))]
        let options = OpenOptions::new();
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

/// Copies the installer-provided uv cache into the exact per-user cache used
/// by [`UvPlan`]. A content marker makes repeated application starts cheap;
/// interrupted copies are retried because the marker is committed last.
///
/// Returns `false` when the current package has no offline cache (for example,
/// a developer or Nix-store build) and `true` after a packaged cache is ready.
///
/// # Errors
///
/// Returns an error when a present cache or its resource manifest is invalid,
/// or when cache materialization fails.
pub fn seed_packaged_uv_cache(
    resource_root: &Path,
    roots: &EffectiveRoots,
) -> Result<bool, UvError> {
    let source = resource_root.join(PACKAGED_UV_CACHE_DIRECTORY);
    match fs::symlink_metadata(&source) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(UvError::InvalidPackagedCache(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source_error) => {
            return Err(UvError::Io {
                path: source,
                source: source_error,
            });
        }
    }

    let manifest_path = resource_root.join("resource-manifest.json");
    let manifest_source = fs::read_to_string(&manifest_path).map_err(|source| UvError::Io {
        path: manifest_path,
        source,
    })?;
    let manifest: ResourceManifestIdentity =
        serde_json::from_str(&manifest_source).map_err(UvError::ResourceManifest)?;
    validate_resource_identity(&manifest.content_sha256)?;

    let destination = roots.cache.join("uv");
    fs::create_dir_all(&destination).map_err(|source| UvError::Io {
        path: destination.clone(),
        source,
    })?;
    let marker = destination.join(PACKAGED_UV_CACHE_MARKER);
    match fs::read_to_string(&marker) {
        Ok(identity) if identity.trim() == manifest.content_sha256 => return Ok(true),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(UvError::Io {
                path: marker,
                source,
            });
        }
    }

    copy_packaged_cache_directory(&source, &destination)?;
    atomic_write(&marker, manifest.content_sha256.as_bytes(), None)?;
    Ok(true)
}

fn copy_packaged_cache_directory(source: &Path, destination: &Path) -> Result<(), UvError> {
    let mut entries = fs::read_dir(source)
        .map_err(|source_error| UvError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source_error| UvError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|source| UvError::Io {
            path: source_path.clone(),
            source,
        })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(UvError::UnsupportedPackagedCacheEntry(source_path));
        }
        if file_type.is_dir() {
            match fs::symlink_metadata(&destination_path) {
                Ok(destination_metadata)
                    if destination_metadata.file_type().is_dir()
                        && !destination_metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(UvError::UnsupportedPackagedCacheEntry(destination_path));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&destination_path).map_err(|source| UvError::Io {
                        path: destination_path.clone(),
                        source,
                    })?;
                }
                Err(source) => {
                    return Err(UvError::Io {
                        path: destination_path,
                        source,
                    });
                }
            }
            copy_packaged_cache_directory(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                Some(metadata.permissions().mode() & 0o777)
            };
            #[cfg(not(unix))]
            let mode = None;
            atomic_copy(&source_path, &destination_path, mode)?;
        } else {
            return Err(UvError::UnsupportedPackagedCacheEntry(source_path));
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, contents: &[u8], mode: Option<u32>) -> Result<(), UvError> {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut output = options.open(path).map_err(|source| UvError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    output.write_all(contents).map_err(|source| UvError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    output.commit().map_err(|source| UvError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn atomic_copy(
    source_path: &Path,
    destination_path: &Path,
    mode: Option<u32>,
) -> Result<(), UvError> {
    let mut input = fs::File::open(source_path).map_err(|source| UvError::Io {
        path: source_path.to_path_buf(),
        source,
    })?;
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut output = options
        .open(destination_path)
        .map_err(|source| UvError::Io {
            path: destination_path.to_path_buf(),
            source,
        })?;
    std::io::copy(&mut input, &mut output).map_err(|source| UvError::Io {
        path: destination_path.to_path_buf(),
        source,
    })?;
    output.commit().map_err(|source| UvError::Io {
        path: destination_path.to_path_buf(),
        source,
    })
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

    /// Enables offline resolution as a package default without overriding an
    /// explicit `POKECON_UV_OFFLINE` bridge supplied by the user.
    #[must_use]
    pub fn with_offline_default(mut self, enabled: bool) -> Self {
        if enabled {
            self.values
                .entry(OsString::from("UV_OFFLINE"))
                .or_insert_with(|| OsString::from("1"));
        }
        self
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
    pub find_links: Option<PathBuf>,
    pub no_index: bool,
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
        add_package_source(&mut compile, input);

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
        add_package_source(&mut sync, input);

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

fn add_package_source(arguments: &mut Vec<OsString>, input: &UvPlanInput) {
    if input.no_index {
        arguments.push(OsString::from("--no-index"));
    }
    if let Some(find_links) = &input.find_links {
        arguments.push(OsString::from("--find-links"));
        arguments.push(find_links.as_os_str().to_os_string());
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
    #[error("current executable is unavailable while resolving managed uv: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("current executable has no resource directory")]
    MissingResourceRoot,
    #[error("managed uv source is missing or not a regular file: {0}")]
    MissingManagedUv(PathBuf),
    #[error("managed uv integrity verification failed")]
    IntegrityMismatch,
    #[error("managed uv destination path is invalid: {0}")]
    InvalidManagedPath(PathBuf),
    #[error("packaged uv cache is invalid: {0}")]
    InvalidPackagedCache(PathBuf),
    #[error("packaged uv cache contains an unsupported entry: {0}")]
    UnsupportedPackagedCacheEntry(PathBuf),
    #[error("packaged Python wheelhouse is invalid: {0}")]
    InvalidPackagedWheelhouse(PathBuf),
    #[error("packaged resource manifest is invalid")]
    ResourceManifest(#[source] serde_json::Error),
    #[error("packaged resource manifest has an invalid content identity")]
    InvalidResourceIdentity,
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

    use super::{
        ManagedUv, ManagedUvSource, UvChildEnvironment, packaged_wheelhouse, seed_packaged_uv_cache,
    };
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
    fn packaged_cache_is_seeded_once_and_enables_an_overridable_offline_default() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must exist");
        let resources = temp.path().join("resources");
        let cache = resources.join("uv-cache/archive-v0");
        fs::create_dir_all(&cache).expect("cache directory must exist");
        fs::write(cache.join("fixture.whl"), b"cached wheel").expect("cache file must exist");
        fs::write(
            resources.join("resource-manifest.json"),
            format!(
                "{{\"content_sha256\":\"{}\"}}",
                "0123456789abcdef".repeat(4)
            ),
        )
        .expect("manifest must exist");

        assert!(seed_packaged_uv_cache(&resources, &roots).expect("cache must seed"));
        assert_eq!(
            fs::read(roots.cache.join("uv/archive-v0/fixture.whl"))
                .expect("seeded file must exist"),
            b"cached wheel"
        );
        fs::write(cache.join("fixture.whl"), b"changed after staging")
            .expect("fixture must change");
        assert!(seed_packaged_uv_cache(&resources, &roots).expect("marker must be reusable"));
        assert_eq!(
            fs::read(roots.cache.join("uv/archive-v0/fixture.whl"))
                .expect("seeded file must remain"),
            b"cached wheel"
        );

        let default = UvChildEnvironment::build(&RootEnvironment::default())
            .expect("environment must build")
            .with_offline_default(true);
        assert_eq!(default.get("UV_OFFLINE"), Some(std::ffi::OsStr::new("1")));
        let explicit =
            UvChildEnvironment::build(&RootEnvironment::from_values([("POKECON_UV_OFFLINE", "0")]))
                .expect("environment must build")
                .with_offline_default(true);
        assert_eq!(explicit.get("UV_OFFLINE"), Some(std::ffi::OsStr::new("0")));
    }

    #[test]
    fn packaged_wheelhouse_requires_a_manifest_lock_and_regular_wheel() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let resources = temp.path().join("resources");
        let wheelhouse = resources.join("python-wheels");
        fs::create_dir_all(&wheelhouse).expect("wheelhouse must exist");
        fs::write(wheelhouse.join("requirements.lock"), b"fixture==1\n").expect("lock must exist");
        fs::write(wheelhouse.join("wheelhouse-manifest.json"), b"{}\n")
            .expect("wheelhouse manifest must exist");
        fs::write(wheelhouse.join("fixture-1-py3-none-any.whl"), b"wheel")
            .expect("wheel must exist");
        let identity = "0123456789abcdef".repeat(4);
        fs::write(
            resources.join("resource-manifest.json"),
            format!("{{\"content_sha256\":\"{identity}\"}}"),
        )
        .expect("resource manifest must exist");

        let packaged = packaged_wheelhouse(&resources)
            .expect("wheelhouse must validate")
            .expect("wheelhouse must be present");
        assert_eq!(packaged.path, wheelhouse);
        assert_eq!(packaged.content_sha256, identity);
    }

    #[test]
    fn empty_uv_bridge_suffix_is_a_startup_error() {
        let environment = RootEnvironment::from_values([("POKECON_UV_", "secret")]);
        assert!(UvChildEnvironment::build(&environment).is_err());
    }
}
