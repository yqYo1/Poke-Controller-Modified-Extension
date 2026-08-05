#[cfg(feature = "tauri-shell")]
use std::ffi::OsStr;
#[cfg(feature = "tauri-shell")]
use std::fs::{File, Metadata, OpenOptions};
#[cfg(feature = "tauri-shell")]
use std::io::{Read, Write};
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::num::TryFromIntError;
#[cfg(feature = "tauri-shell")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "tauri-shell")]
use std::collections::BTreeMap;
#[cfg(feature = "tauri-shell")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "tauri-shell")]
use std::sync::{Arc, mpsc};

#[cfg(all(feature = "tauri-shell", unix))]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(all(feature = "tauri-shell", target_os = "linux"))]
use std::os::unix::process::CommandExt;
#[cfg(all(feature = "tauri-shell", windows))]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

use crate::desktop::{CloseBehavior, DesktopError, DesktopRuntimeSettings};
#[cfg(feature = "tauri-shell")]
use crate::desktop::{DesktopLifecycle, DesktopShellConfig, run_tauri_shell};
use crate::diagnostics::{TracingInitError, init_tracing};
use crate::dynamic_runtime::bootstrap_dynamic;
use crate::runtime::ShutdownCoordinator;
#[cfg(feature = "tauri-shell")]
use crate::runtime::ShutdownReason;
use crate::settings::pipeline::{LoadedSettings, PipelineError, PipelineRequest, SettingsPipeline};
use crate::settings::scaffold::{ScaffoldError, ScaffoldManager};
use crate::{AppError, AppOptions, RunControl, UiMode, run_configured_controlled};
use clap::{Parser, ValueEnum};
#[cfg(feature = "tauri-shell")]
use sha2::{Digest, Sha256};
#[cfg(feature = "tauri-shell")]
use tempfile::TempDir;
use thiserror::Error;

#[cfg(all(feature = "tauri-shell", target_os = "linux"))]
const COMPOSITING_REEXEC_MARKER: &str = "PCME_DESKTOP_COMPOSITING_CONFIGURED";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum UiArgument {
    Web,
    Desktop,
}

impl From<UiArgument> for UiMode {
    fn from(value: UiArgument) -> Self {
        match value {
            UiArgument::Web => Self::Web,
            UiArgument::Desktop => Self::Desktop,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about = "PokeCon Rust runtime")]
struct Cli {
    /// Selects the web-only or desktop lifecycle mode.
    #[arg(long, value_enum, default_value_t = UiArgument::Web)]
    ui: UiArgument,
    /// Exit successfully after the runtime boundaries have started.
    #[arg(long)]
    exit_after_startup: bool,
}

#[derive(Debug, Error)]
pub enum MainError {
    #[error(transparent)]
    Tracing(#[from] TracingInitError),
    #[error(transparent)]
    App(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Desktop(#[from] DesktopError),
    #[error(transparent)]
    Settings(#[from] PipelineError),
    #[error(transparent)]
    Scaffold(#[from] ScaffoldError),
    #[error("canonical server.bind_address is not a numeric IP literal")]
    BindAddress(#[from] AddrParseError),
    #[error("canonical server.port is outside the u16 range")]
    Port(#[from] TryFromIntError),
    #[cfg(feature = "tauri-shell")]
    #[error("desktop backend task failed: {0}")]
    BackendTask(#[source] tokio::task::JoinError),
    #[cfg(feature = "tauri-shell")]
    #[error("the primary desktop instance did not start its backend")]
    BackendNotStarted,
    #[cfg(feature = "tauri-shell")]
    #[error(transparent)]
    ResourceManifest(#[from] ResourceManifestError),
    #[cfg(not(feature = "tauri-shell"))]
    #[error("desktop mode is unavailable in this binary; rebuild with feature `tauri-shell`")]
    DesktopUnavailable,
    #[cfg(all(feature = "tauri-shell", target_os = "linux"))]
    #[error("failed to relaunch with Linux WebView compositing disabled: {0}")]
    CompositingRelaunch(#[source] std::io::Error),
}

impl From<AppError> for MainError {
    fn from(error: AppError) -> Self {
        Self::App(Box::new(error))
    }
}

/// Runs the canonical command-line and desktop application lifecycle.
///
/// # Errors
///
/// Returns an error if settings, tracing, scaffolding, desktop startup, address
/// parsing, or application runtime execution fails.
pub async fn run_cli() -> Result<(), MainError> {
    let request = PipelineRequest::current()?;
    let before_dynamic = SettingsPipeline::new(request.clone()).load_before_dynamic()?;
    let cli = Cli::parse_from(&before_dynamic.remaining_arguments);

    #[cfg(all(feature = "tauri-shell", target_os = "linux"))]
    {
        let disable_compositing =
            before_dynamic.pre_dynamic_final_boolean_with_cli("ui.desktop.disable_compositing")?;
        let already_reexecuted = std::env::var_os(COMPOSITING_REEXEC_MARKER).is_some();
        if should_reexec_for_linux_compositing(
            cli.ui,
            cli.exit_after_startup,
            disable_compositing,
            already_reexecuted,
        ) {
            drop((cli, before_dynamic, request));
            return reexec_for_linux_compositing();
        }
    }

    init_tracing("info")?;
    ScaffoldManager::new(before_dynamic.roots.clone())
        .ensure(before_dynamic.active_profile.as_str())?;

    if cli.ui == UiArgument::Desktop && !cli.exit_after_startup {
        #[cfg(feature = "tauri-shell")]
        return run_desktop(request, before_dynamic).await;
        #[cfg(not(feature = "tauri-shell"))]
        return Err(MainError::DesktopUnavailable);
    }

    #[cfg(feature = "tauri-shell")]
    return run_packaged_backend(
        request,
        before_dynamic,
        cli.ui.into(),
        cli.exit_after_startup,
        RunControl::new(ShutdownCoordinator::new()),
        None,
    )
    .await;

    #[cfg(not(feature = "tauri-shell"))]
    run_backend(
        request,
        before_dynamic,
        cli.ui.into(),
        cli.exit_after_startup,
        RunControl::new(ShutdownCoordinator::new()),
        None,
    )
    .await
}

#[cfg(feature = "tauri-shell")]
fn packaged_resource_root(current: &Path) -> Result<SelectedResourceRoot, ResourceManifestError> {
    let provenance = compiled_resource_provenance()?;
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let origin_platform = ResourceOriginPlatform::current();
    let platform = origin_platform.resource_platform()?;
    select_resource_root_for_provenance(
        current,
        &context.package_info().name,
        origin_platform,
        provenance,
        has_exact_nix_resource_layout(current),
        platform == ResourcePlatform::Unix && has_exact_cargo_output_layout(current, current),
    )
}

#[cfg(feature = "tauri-shell")]
#[derive(Debug, Error)]
pub enum ResourceManifestError {
    #[error("compiled resource provenance is invalid")]
    InvalidProvenance,
    #[error("compiled resource provenance does not permit the detected {origin} resource origin")]
    OriginMismatch { origin: &'static str },
    #[error(
        "packaged resource manifest identity does not match compiled resource provenance at {path}"
    )]
    IdentityMismatch { path: PathBuf },
    #[error("packaged resource origins are unsupported on {platform}")]
    UnsupportedOrigin { platform: &'static str },
    #[error("packaged resource origin {path} is malformed for {platform}: {reason}")]
    MalformedOrigin {
        path: PathBuf,
        platform: &'static str,
        reason: &'static str,
    },
    #[error("packaged resource manifest is missing at {path}")]
    Missing { path: PathBuf },
    #[error("failed to {operation} packaged resource path {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("packaged resource manifest {path} is not valid JSON: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("packaged resource manifest {path} is invalid: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error("failed to create a private verified resource snapshot: {source}")]
    Snapshot {
        #[source]
        source: std::io::Error,
    },
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourcePlatform {
    Unix,
    Windows,
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceProvenance<'a> {
    Development,
    NixExact,
    Packaged(&'a str),
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceOrigin {
    Cargo,
    NixExact,
    Packaged,
}

#[cfg(feature = "tauri-shell")]
impl ResourceOrigin {
    const fn name(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo",
            Self::NixExact => "Nix",
            Self::Packaged => "packaged",
        }
    }
}

#[cfg(feature = "tauri-shell")]
fn parse_resource_provenance(value: &str) -> Result<ResourceProvenance<'_>, ResourceManifestError> {
    match value {
        "development" => Ok(ResourceProvenance::Development),
        "nix-exact" => Ok(ResourceProvenance::NixExact),
        _ => value
            .strip_prefix("packaged:")
            .filter(|digest| is_lowercase_sha256(digest))
            .map(ResourceProvenance::Packaged)
            .ok_or(ResourceManifestError::InvalidProvenance),
    }
}

#[cfg(feature = "tauri-shell")]
fn compiled_resource_provenance() -> Result<ResourceProvenance<'static>, ResourceManifestError> {
    parse_resource_provenance(env!("POKECON_RESOURCE_PROVENANCE"))
}

#[cfg(feature = "tauri-shell")]
const fn provenance_accepts_origin(
    provenance: ResourceProvenance<'_>,
    origin: ResourceOrigin,
) -> bool {
    matches!(
        (provenance, origin),
        (ResourceProvenance::Development, ResourceOrigin::Cargo)
            | (ResourceProvenance::NixExact, ResourceOrigin::NixExact)
            | (ResourceProvenance::Packaged(_), ResourceOrigin::Packaged)
    )
}

#[cfg(feature = "tauri-shell")]
fn require_resource_origin(
    provenance: ResourceProvenance<'_>,
    origin: ResourceOrigin,
) -> Result<(), ResourceManifestError> {
    if provenance_accepts_origin(provenance, origin) {
        Ok(())
    } else {
        Err(ResourceManifestError::OriginMismatch {
            origin: origin.name(),
        })
    }
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceOriginPlatform {
    Linux,
    Macos,
    Windows,
    Unsupported,
}

#[cfg(feature = "tauri-shell")]
impl ResourceOriginPlatform {
    const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unsupported
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Unsupported => std::env::consts::OS,
        }
    }

    const fn resource_platform(self) -> Result<ResourcePlatform, ResourceManifestError> {
        match self {
            Self::Linux | Self::Macos => Ok(ResourcePlatform::Unix),
            Self::Windows => Ok(ResourcePlatform::Windows),
            Self::Unsupported => Err(ResourceManifestError::UnsupportedOrigin {
                platform: std::env::consts::OS,
            }),
        }
    }

    fn is_packager_owned_top_level_file(self, relative: &str) -> bool {
        match self {
            Self::Macos => relative == "icon.icns",
            Self::Windows => matches!(relative, "pokecon.exe" | "uninstall.exe"),
            Self::Linux | Self::Unsupported => false,
        }
    }
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MissingManifestPolicy {
    Reject,
    DevelopmentFallback,
}

#[cfg(feature = "tauri-shell")]
impl MissingManifestPolicy {
    #[cfg(test)]
    const fn runtime(platform: ResourcePlatform, development_build: bool) -> Self {
        if matches!(platform, ResourcePlatform::Unix) && development_build {
            Self::DevelopmentFallback
        } else {
            Self::Reject
        }
    }
}

#[cfg(feature = "tauri-shell")]
impl ResourcePlatform {
    #[cfg(test)]
    const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Unix => "unix",
            Self::Windows => "windows",
        }
    }

    const fn required_files(self) -> &'static [&'static str] {
        match self {
            Self::Unix => &[
                "pokecon-worker",
                "python-wheels/requirements.lock",
                "python-wheels/wheelhouse-manifest.json",
                "python/bin/python3.14",
                "uv/uv",
                "web/dist/index.html",
            ],
            Self::Windows => &[
                "pokecon-worker.exe",
                "python-wheels/requirements.lock",
                "python-wheels/wheelhouse-manifest.json",
                "python/python.exe",
                "uv/uv.exe",
                "web/dist/index.html",
            ],
        }
    }

    fn requires_executable(self, relative: &str) -> bool {
        matches!(self, Self::Unix)
            && matches!(
                relative,
                "pokecon-worker" | "python/bin/python3.14" | "uv/uv"
            )
    }
}

#[cfg(feature = "tauri-shell")]
struct ResourceSnapshot {
    _container: TempDir,
    root: PathBuf,
}

#[cfg(feature = "tauri-shell")]
impl Drop for ResourceSnapshot {
    fn drop(&mut self) {
        let _result = make_snapshot_tree_writable(&self.root);
    }
}

#[cfg(feature = "tauri-shell")]
struct SelectedResourceRoot {
    path: PathBuf,
    _snapshot: Option<ResourceSnapshot>,
}

#[cfg(feature = "tauri-shell")]
impl SelectedResourceRoot {
    fn borrowed(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            _snapshot: None,
        }
    }

    fn snapshot(snapshot: ResourceSnapshot) -> Self {
        Self {
            path: snapshot.root.clone(),
            _snapshot: Some(snapshot),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct StableMetadata {
    length: u64,
    #[cfg(unix)]
    platform: UnixStableMetadata,
    #[cfg(windows)]
    platform: WindowsStableMetadata,
    #[cfg(not(any(unix, windows)))]
    platform: OtherStableMetadata,
}

#[cfg(all(feature = "tauri-shell", unix))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct UnixStableMetadata {
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    group: u32,
    links: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[cfg(all(feature = "tauri-shell", windows))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct WindowsStableMetadata {
    attributes: u64,
    created: Option<u64>,
    modified: Option<u64>,
    volume: u64,
    file_index: u64,
    links: u64,
}

#[cfg(all(feature = "tauri-shell", not(any(unix, windows))))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct OtherStableMetadata {
    readonly: bool,
    modified: Option<std::time::SystemTime>,
}

#[cfg(feature = "tauri-shell")]
struct VerifiedOpenFile {
    path: PathBuf,
    file: File,
    identity: StableMetadata,
    contents: Vec<u8>,
    executable: bool,
}

#[cfg(feature = "tauri-shell")]
struct ObservedDirectory {
    path: PathBuf,
    handle: File,
    identity: StableMetadata,
}

#[cfg(feature = "tauri-shell")]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifest {
    schema_version: u32,
    layout_version: u32,
    platform: String,
    files: Vec<ResourceManifestEntry>,
    content_sha256: String,
}

#[cfg(feature = "tauri-shell")]
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifestEntry {
    path: String,
    sha256: String,
    size: u64,
}

#[cfg(feature = "tauri-shell")]
struct ExpectedResourceManifest {
    files: BTreeMap<String, (String, u64)>,
    content_sha256: String,
}

#[cfg(feature = "tauri-shell")]
fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(feature = "tauri-shell")]
fn is_safe_manifest_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

#[cfg(feature = "tauri-shell")]
fn invalid_manifest(manifest_path: &Path, reason: impl Into<String>) -> ResourceManifestError {
    ResourceManifestError::Invalid {
        path: manifest_path.to_path_buf(),
        reason: reason.into(),
    }
}

#[cfg(feature = "tauri-shell")]
fn resource_io(
    operation: &'static str,
    path: &Path,
    source: std::io::Error,
) -> ResourceManifestError {
    ResourceManifestError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(feature = "tauri-shell")]
fn lowercase_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(feature = "tauri-shell")]
fn sha256_bytes(bytes: &[u8]) -> String {
    lowercase_hex(&Sha256::digest(bytes))
}

#[cfg(all(feature = "tauri-shell", unix))]
fn stable_metadata(_file: &File, metadata: &Metadata, _path: &Path) -> StableMetadata {
    StableMetadata {
        length: metadata.len(),
        platform: UnixStableMetadata {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            owner: metadata.uid(),
            group: metadata.gid(),
            links: metadata.nlink(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        },
    }
}

#[cfg(all(feature = "tauri-shell", windows))]
fn stable_metadata(
    file: &File,
    _metadata: &Metadata,
    path: &Path,
) -> Result<StableMetadata, ResourceManifestError> {
    let information = winapi_util::file::information(file)
        .map_err(|error| resource_io("query open file information", path, error))?;
    Ok(StableMetadata {
        length: information.file_size(),
        platform: WindowsStableMetadata {
            attributes: information.file_attributes(),
            created: information.creation_time(),
            modified: information.last_write_time(),
            volume: information.volume_serial_number(),
            file_index: information.file_index(),
            links: information.number_of_links(),
        },
    })
}

#[cfg(all(feature = "tauri-shell", not(any(unix, windows))))]
fn stable_metadata(_file: &File, metadata: &Metadata, _path: &Path) -> StableMetadata {
    StableMetadata {
        length: metadata.len(),
        platform: OtherStableMetadata {
            readonly: metadata.permissions().readonly(),
            modified: metadata.modified().ok(),
        },
    }
}

#[cfg(feature = "tauri-shell")]
impl StableMetadata {
    fn has_single_link(&self) -> bool {
        #[cfg(any(unix, windows))]
        {
            self.platform.links == 1
        }
        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    }
}

#[cfg(all(feature = "tauri-shell", any(windows, test)))]
const fn windows_attributes_include_reparse_point(attributes: u32) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(feature = "tauri-shell")]
fn metadata_is_link_or_reparse_point(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        windows_attributes_include_reparse_point(metadata.file_attributes())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(feature = "tauri-shell")]
fn metadata_is_executable(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        metadata.mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

#[cfg(all(feature = "tauri-shell", unix))]
fn open_no_follow(path: &Path, directory: bool) -> Result<File, ResourceManifestError> {
    use nix::fcntl::{OFlag, open};
    use nix::sys::stat::Mode;

    let mut flags = OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW;
    if directory {
        flags |= OFlag::O_DIRECTORY;
    }
    let descriptor = open(path, flags, Mode::empty()).map_err(|error| {
        resource_io(
            "open without following links",
            path,
            std::io::Error::from(error),
        )
    })?;
    Ok(File::from(descriptor))
}

#[cfg(all(feature = "tauri-shell", windows))]
fn open_no_follow(path: &Path, directory: bool) -> Result<File, ResourceManifestError> {
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    let mut options = OpenOptions::new();
    options.read(true);
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS;
    }
    options.custom_flags(flags);
    options
        .open(path)
        .map_err(|error| resource_io("open without following links", path, error))
}

#[cfg(all(feature = "tauri-shell", not(any(unix, windows))))]
fn open_no_follow(path: &Path, _directory: bool) -> Result<File, ResourceManifestError> {
    OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| resource_io("open resource", path, error))
}

#[cfg(feature = "tauri-shell")]
fn ensure_regular_file(
    metadata: &Metadata,
    path: &Path,
    manifest_path: &Path,
) -> Result<(), ResourceManifestError> {
    if metadata_is_link_or_reparse_point(metadata) || !metadata.file_type().is_file() {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "resource is not a non-link, non-reparse regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(feature = "tauri-shell")]
fn inspect_open_regular_file(
    file: &File,
    path: &Path,
    manifest_path: &Path,
) -> Result<(StableMetadata, bool), ResourceManifestError> {
    let metadata = file
        .metadata()
        .map_err(|error| resource_io("inspect open", path, error))?;
    ensure_regular_file(&metadata, path, manifest_path)?;
    #[cfg(windows)]
    let identity = stable_metadata(file, &metadata, path)?;
    #[cfg(not(windows))]
    let identity = stable_metadata(file, &metadata, path);
    if !identity.has_single_link() {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "resource file does not have exactly one hard link: {}",
                path.display()
            ),
        ));
    }
    Ok((identity, metadata_is_executable(&metadata)))
}

#[cfg(feature = "tauri-shell")]
impl VerifiedOpenFile {
    fn verify_stable(&self, manifest_path: &Path) -> Result<(), ResourceManifestError> {
        let (handle_identity, _executable) =
            inspect_open_regular_file(&self.file, &self.path, manifest_path)?;
        let reopened = open_no_follow(&self.path, false)?;
        let (path_identity, _executable) =
            inspect_open_regular_file(&reopened, &self.path, manifest_path)?;
        if handle_identity != self.identity || path_identity != self.identity {
            return Err(invalid_manifest(
                manifest_path,
                format!(
                    "resource file identity or metadata changed while it was verified: {}",
                    self.path.display()
                ),
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "tauri-shell")]
fn read_stable_resource_file_with<F>(
    path: &Path,
    manifest_path: &Path,
    after_read: F,
) -> Result<VerifiedOpenFile, ResourceManifestError>
where
    F: FnOnce(),
{
    let path_metadata =
        std::fs::symlink_metadata(path).map_err(|error| resource_io("inspect", path, error))?;
    ensure_regular_file(&path_metadata, path, manifest_path)?;
    let mut file = open_no_follow(path, false)?;
    let (identity, executable) = inspect_open_regular_file(&file, path, manifest_path)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .map_err(|error| resource_io("read open", path, error))?;
    after_read();
    let verified = VerifiedOpenFile {
        path: path.to_path_buf(),
        file,
        identity,
        contents,
        executable,
    };
    verified.verify_stable(manifest_path)?;
    if u64::try_from(verified.contents.len()).ok() != Some(verified.identity.length) {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "resource file length changed while it was read: {}",
                path.display()
            ),
        ));
    }
    Ok(verified)
}

#[cfg(feature = "tauri-shell")]
fn read_stable_resource_file(
    path: &Path,
    manifest_path: &Path,
) -> Result<VerifiedOpenFile, ResourceManifestError> {
    read_stable_resource_file_with(path, manifest_path, || {})
}

#[cfg(feature = "tauri-shell")]
fn inspect_open_directory(
    file: &File,
    path: &Path,
    manifest_path: &Path,
) -> Result<StableMetadata, ResourceManifestError> {
    let metadata = file
        .metadata()
        .map_err(|error| resource_io("inspect open directory", path, error))?;
    if metadata_is_link_or_reparse_point(&metadata) || !metadata.file_type().is_dir() {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "opened resource path is not a non-link, non-reparse directory: {}",
                path.display()
            ),
        ));
    }
    #[cfg(windows)]
    {
        stable_metadata(file, &metadata, path)
    }
    #[cfg(not(windows))]
    {
        Ok(stable_metadata(file, &metadata, path))
    }
}

#[cfg(feature = "tauri-shell")]
fn observe_directory(
    path: &Path,
    manifest_path: &Path,
) -> Result<ObservedDirectory, ResourceManifestError> {
    let path_metadata =
        std::fs::symlink_metadata(path).map_err(|error| resource_io("inspect", path, error))?;
    if metadata_is_link_or_reparse_point(&path_metadata) || !path_metadata.file_type().is_dir() {
        return Err(invalid_manifest(
            manifest_path,
            format!(
                "resource directory is not a non-link, non-reparse directory: {}",
                path.display()
            ),
        ));
    }
    let handle = open_no_follow(path, true)?;
    let identity = inspect_open_directory(&handle, path, manifest_path)?;
    Ok(ObservedDirectory {
        path: path.to_path_buf(),
        handle,
        identity,
    })
}

#[cfg(feature = "tauri-shell")]
impl ObservedDirectory {
    fn verify_stable(&self, manifest_path: &Path) -> Result<(), ResourceManifestError> {
        let handle_identity = inspect_open_directory(&self.handle, &self.path, manifest_path)?;
        let reopened = open_no_follow(&self.path, true)?;
        let path_identity = inspect_open_directory(&reopened, &self.path, manifest_path)?;
        if handle_identity != self.identity || path_identity != self.identity {
            return Err(invalid_manifest(
                manifest_path,
                format!(
                    "resource directory identity or metadata changed while it was inventoried: {}",
                    self.path.display()
                ),
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "tauri-shell")]
fn relative_resource_path(
    resource_root: &Path,
    path: &Path,
    manifest_path: &Path,
) -> Result<String, ResourceManifestError> {
    let relative = path.strip_prefix(resource_root).map_err(|_error| {
        invalid_manifest(
            manifest_path,
            format!("resource path escapes its lexical root: {}", path.display()),
        )
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(invalid_manifest(
                manifest_path,
                format!("resource path is not canonical: {}", path.display()),
            ));
        };
        let Some(component) = component.to_str() else {
            return Err(invalid_manifest(
                manifest_path,
                format!("resource path is not UTF-8: {}", path.display()),
            ));
        };
        components.push(component);
    }
    let relative = components.join("/");
    if !is_safe_manifest_path(&relative) {
        return Err(invalid_manifest(
            manifest_path,
            format!("resource path is not a safe relative POSIX path: {relative:?}"),
        ));
    }
    Ok(relative)
}

#[cfg(feature = "tauri-shell")]
fn expected_resource_files(
    contents: &[u8],
    manifest_path: &Path,
    platform: ResourcePlatform,
) -> Result<ExpectedResourceManifest, ResourceManifestError> {
    let manifest = serde_json::from_slice::<ResourceManifest>(contents).map_err(|source| {
        ResourceManifestError::Json {
            path: manifest_path.to_path_buf(),
            source,
        }
    })?;
    if manifest.schema_version != 1
        || manifest.layout_version != 1
        || manifest.platform != platform.name()
        || manifest.files.is_empty()
        || !is_lowercase_sha256(&manifest.content_sha256)
    {
        return Err(invalid_manifest(
            manifest_path,
            "schema, layout, platform, files, or content identity is invalid",
        ));
    }
    let mut expected = BTreeMap::new();
    let mut previous_path: Option<&str> = None;
    for entry in &manifest.files {
        if !is_safe_manifest_path(&entry.path) || !is_lowercase_sha256(&entry.sha256) {
            return Err(invalid_manifest(
                manifest_path,
                format!("manifest entry is invalid: {:?}", entry.path),
            ));
        }
        if previous_path.is_some_and(|previous| previous >= entry.path.as_str()) {
            return Err(invalid_manifest(
                manifest_path,
                "manifest file entries are not in canonical path order",
            ));
        }
        previous_path = Some(&entry.path);
        expected.insert(entry.path.clone(), (entry.sha256.clone(), entry.size));
    }
    for required in platform.required_files() {
        if !expected.contains_key(*required) {
            return Err(invalid_manifest(
                manifest_path,
                format!("manifest omits required resource: {required}"),
            ));
        }
    }
    let canonical_files =
        serde_json::to_vec(&manifest.files).map_err(|source| ResourceManifestError::Json {
            path: manifest_path.to_path_buf(),
            source,
        })?;
    if sha256_bytes(&canonical_files) != manifest.content_sha256 {
        return Err(invalid_manifest(
            manifest_path,
            "content identity does not match the canonical file inventory",
        ));
    }
    Ok(ExpectedResourceManifest {
        files: expected,
        content_sha256: manifest.content_sha256,
    })
}

#[cfg(feature = "tauri-shell")]
fn require_packaged_resource_identity(
    actual: &str,
    required: Option<&str>,
    manifest_path: &Path,
) -> Result<(), ResourceManifestError> {
    if required.is_none_or(|required| actual == required) {
        Ok(())
    } else {
        Err(ResourceManifestError::IdentityMismatch {
            path: manifest_path.to_path_buf(),
        })
    }
}

#[cfg(feature = "tauri-shell")]
fn set_snapshot_permissions(
    path: &Path,
    directory: bool,
    executable: bool,
    writable: bool,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mode = if writable {
            if directory { 0o700 } else { 0o600 }
        } else if directory || executable {
            0o500
        } else {
            0o400
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (directory, executable);
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_readonly(!writable);
        std::fs::set_permissions(path, permissions)
    }
}

#[cfg(feature = "tauri-shell")]
fn make_snapshot_tree_writable(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata_is_link_or_reparse_point(&metadata) {
        return Err(std::io::Error::other(
            "private resource snapshot unexpectedly contains a link or reparse point",
        ));
    }
    if metadata.file_type().is_dir() {
        set_snapshot_permissions(path, true, false, true)?;
        for entry in std::fs::read_dir(path)? {
            make_snapshot_tree_writable(&entry?.path())?;
        }
    } else {
        set_snapshot_permissions(path, false, false, true)?;
    }
    Ok(())
}

#[cfg(feature = "tauri-shell")]
fn seal_snapshot_directories(path: &Path) -> Result<(), ResourceManifestError> {
    for entry in std::fs::read_dir(path)
        .map_err(|error| resource_io("read snapshot directory", path, error))?
    {
        let entry = entry.map_err(|error| resource_io("read snapshot directory", path, error))?;
        let child = entry.path();
        let metadata = std::fs::symlink_metadata(&child)
            .map_err(|error| resource_io("inspect snapshot", &child, error))?;
        if metadata_is_link_or_reparse_point(&metadata) {
            return Err(invalid_manifest(
                &path.join("resource-manifest.json"),
                format!(
                    "private resource snapshot contains a link or reparse point: {}",
                    child.display()
                ),
            ));
        }
        if metadata.file_type().is_dir() {
            seal_snapshot_directories(&child)?;
        }
    }
    set_snapshot_permissions(path, true, false, false)
        .map_err(|error| resource_io("seal snapshot directory", path, error))
}

#[cfg(feature = "tauri-shell")]
impl ResourceSnapshot {
    fn create() -> Result<Self, ResourceManifestError> {
        let container = tempfile::Builder::new()
            .prefix("pokecon-verified-resources-")
            .tempdir()
            .map_err(|source| ResourceManifestError::Snapshot { source })?;
        set_snapshot_permissions(container.path(), true, false, true)
            .map_err(|error| resource_io("protect snapshot container", container.path(), error))?;
        let root = container.path().join("resources");
        std::fs::create_dir(&root)
            .map_err(|error| resource_io("create snapshot root", &root, error))?;
        set_snapshot_permissions(&root, true, false, true)
            .map_err(|error| resource_io("protect snapshot root", &root, error))?;
        Ok(Self {
            _container: container,
            root,
        })
    }
}

#[cfg(feature = "tauri-shell")]
fn write_snapshot_file(
    snapshot_root: &Path,
    relative: &str,
    contents: &[u8],
    executable: bool,
    manifest_path: &Path,
) -> Result<(), ResourceManifestError> {
    let destination = snapshot_root.join(relative);
    let parent = destination.parent().ok_or_else(|| {
        invalid_manifest(
            manifest_path,
            format!("snapshot resource has no parent: {relative:?}"),
        )
    })?;
    std::fs::create_dir_all(parent)
        .map_err(|error| resource_io("create snapshot directory", parent, error))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)
        .map_err(|error| resource_io("create snapshot resource", &destination, error))?;
    output
        .write_all(contents)
        .map_err(|error| resource_io("write snapshot resource", &destination, error))?;
    output
        .sync_all()
        .map_err(|error| resource_io("sync snapshot resource", &destination, error))?;
    drop(output);
    set_snapshot_permissions(&destination, false, executable, false)
        .map_err(|error| resource_io("seal snapshot resource", &destination, error))
}

#[cfg(feature = "tauri-shell")]
fn ensure_canonical_resource_containment(
    path: &Path,
    canonical_root: &Path,
    manifest_path: &Path,
) -> Result<(), ResourceManifestError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| resource_io("canonicalize", path, error))?;
    if canonical.starts_with(canonical_root) {
        Ok(())
    } else {
        Err(invalid_manifest(
            manifest_path,
            format!(
                "resource path escapes its canonical root: {}",
                path.display()
            ),
        ))
    }
}

#[cfg(feature = "tauri-shell")]
fn inventory_resource_files(
    resource_root: &Path,
    canonical_root: &Path,
    manifest_path: &Path,
    platform: ResourcePlatform,
    packaged_origin: Option<ResourceOriginPlatform>,
    expected: &BTreeMap<String, (String, u64)>,
    snapshot_root: Option<&Path>,
) -> Result<BTreeMap<String, (String, u64)>, ResourceManifestError> {
    let mut inventory = BTreeMap::new();
    let mut pending = vec![resource_root.to_path_buf()];
    let mut observed_directories = Vec::new();
    while let Some(directory) = pending.pop() {
        let observed = observe_directory(&directory, manifest_path)?;
        ensure_canonical_resource_containment(&directory, canonical_root, manifest_path)?;
        let mut entries = std::fs::read_dir(&directory)
            .map_err(|error| resource_io("read directory", &directory, error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| resource_io("read directory", &directory, error))?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| resource_io("inspect", &path, error))?;
            if metadata_is_link_or_reparse_point(&metadata) {
                return Err(invalid_manifest(
                    manifest_path,
                    format!(
                        "resource entry is a link or reparse point: {}",
                        path.display()
                    ),
                ));
            }
            ensure_canonical_resource_containment(&path, canonical_root, manifest_path)?;
            let relative = relative_resource_path(resource_root, &path, manifest_path)?;
            let packager_owned = packaged_origin
                .is_some_and(|origin| origin.is_packager_owned_top_level_file(&relative));
            if packager_owned && !metadata.file_type().is_file() {
                return Err(invalid_manifest(
                    manifest_path,
                    format!(
                        "packager-owned resource entry is not a regular file: {}",
                        path.display()
                    ),
                ));
            }
            if metadata.file_type().is_dir() {
                pending.push(path);
            } else if metadata.file_type().is_file() {
                if relative == "resource-manifest.json" {
                    continue;
                }
                let verified = read_stable_resource_file(&path, manifest_path)?;
                if packager_owned {
                    continue;
                }
                if platform.requires_executable(&relative) && !verified.executable {
                    return Err(invalid_manifest(
                        manifest_path,
                        format!("required resource is not executable: {relative:?}"),
                    ));
                }
                let actual = (
                    sha256_bytes(&verified.contents),
                    u64::try_from(verified.contents.len()).map_err(|_error| {
                        invalid_manifest(manifest_path, "resource length exceeds u64")
                    })?,
                );
                if inventory.insert(relative.clone(), actual).is_some() {
                    return Err(invalid_manifest(
                        manifest_path,
                        format!("resource inventory repeats path: {relative:?}"),
                    ));
                }
                if let Some(snapshot_root) = snapshot_root
                    && expected.contains_key(&relative)
                {
                    write_snapshot_file(
                        snapshot_root,
                        &relative,
                        &verified.contents,
                        platform.requires_executable(&relative),
                        manifest_path,
                    )?;
                }
            } else {
                return Err(invalid_manifest(
                    manifest_path,
                    format!(
                        "resource entry is not a regular file or directory: {}",
                        path.display()
                    ),
                ));
            }
        }
        observed_directories.push(observed);
    }
    for directory in &observed_directories {
        directory.verify_stable(manifest_path)?;
    }
    Ok(inventory)
}

#[cfg(feature = "tauri-shell")]
fn ensure_resource_inventory(
    expected: &BTreeMap<String, (String, u64)>,
    actual: &BTreeMap<String, (String, u64)>,
    manifest_path: &Path,
) -> Result<(), ResourceManifestError> {
    if actual == expected {
        return Ok(());
    }
    let missing = expected
        .keys()
        .filter(|path| !actual.contains_key(*path))
        .collect::<Vec<_>>();
    let extra = actual
        .keys()
        .filter(|path| !expected.contains_key(*path))
        .collect::<Vec<_>>();
    let changed = expected
        .keys()
        .filter(|path| {
            actual
                .get(*path)
                .is_some_and(|value| value != &expected[*path])
        })
        .collect::<Vec<_>>();
    Err(invalid_manifest(
        manifest_path,
        format!(
            "resource inventory mismatch: missing={missing:?}, extra={extra:?}, changed={changed:?}"
        ),
    ))
}

#[cfg(feature = "tauri-shell")]
fn verify_materialized_snapshot(
    snapshot_root: &Path,
    manifest_contents: &[u8],
    expected: &ExpectedResourceManifest,
    platform: ResourcePlatform,
    required_content_sha256: Option<&str>,
) -> Result<(), ResourceManifestError> {
    let manifest_path = snapshot_root.join("resource-manifest.json");
    let manifest = read_stable_resource_file(&manifest_path, &manifest_path)?;
    if manifest.contents != manifest_contents {
        return Err(invalid_manifest(
            &manifest_path,
            "private snapshot manifest bytes differ from the verified source manifest",
        ));
    }
    let snapshot_expected = expected_resource_files(&manifest.contents, &manifest_path, platform)?;
    if snapshot_expected.files != expected.files
        || snapshot_expected.content_sha256 != expected.content_sha256
    {
        return Err(invalid_manifest(
            &manifest_path,
            "private snapshot manifest inventory differs from the verified source manifest",
        ));
    }
    let canonical_root = snapshot_root
        .canonicalize()
        .map_err(|error| resource_io("canonicalize snapshot root", snapshot_root, error))?;
    let actual = inventory_resource_files(
        snapshot_root,
        &canonical_root,
        &manifest_path,
        platform,
        None,
        &expected.files,
        None,
    )?;
    ensure_resource_inventory(&expected.files, &actual, &manifest_path)?;
    require_packaged_resource_identity(
        &snapshot_expected.content_sha256,
        required_content_sha256,
        &manifest_path,
    )?;
    manifest.verify_stable(&manifest_path)
}

#[cfg(feature = "tauri-shell")]
fn materialize_resource_manifest(
    resource_root: &Path,
    origin_platform: ResourceOriginPlatform,
    required_content_sha256: Option<&str>,
) -> Result<Option<SelectedResourceRoot>, ResourceManifestError> {
    let platform = origin_platform.resource_platform()?;
    let manifest_path = resource_root.join("resource-manifest.json");
    match std::fs::symlink_metadata(&manifest_path) {
        Ok(_metadata) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(resource_io("inspect", &manifest_path, error)),
    }
    let root_metadata = std::fs::symlink_metadata(resource_root)
        .map_err(|error| resource_io("inspect", resource_root, error))?;
    if metadata_is_link_or_reparse_point(&root_metadata) || !root_metadata.file_type().is_dir() {
        return Err(invalid_manifest(
            &manifest_path,
            "resource root is not a non-link, non-reparse directory",
        ));
    }
    let canonical_root = resource_root
        .canonicalize()
        .map_err(|error| resource_io("canonicalize", resource_root, error))?;
    let canonical_manifest = manifest_path
        .canonicalize()
        .map_err(|error| resource_io("canonicalize", &manifest_path, error))?;
    if canonical_manifest.parent() != Some(canonical_root.as_path()) {
        return Err(invalid_manifest(
            &manifest_path,
            "manifest is not directly contained by the canonical resource root",
        ));
    }
    let manifest = read_stable_resource_file(&manifest_path, &manifest_path)?;
    let expected = expected_resource_files(&manifest.contents, &manifest_path, platform)?;
    require_packaged_resource_identity(
        &expected.content_sha256,
        required_content_sha256,
        &manifest_path,
    )?;
    let snapshot = ResourceSnapshot::create()?;
    let actual = inventory_resource_files(
        resource_root,
        &canonical_root,
        &manifest_path,
        platform,
        Some(origin_platform),
        &expected.files,
        Some(&snapshot.root),
    )?;
    ensure_resource_inventory(&expected.files, &actual, &manifest_path)?;
    manifest.verify_stable(&manifest_path)?;
    write_snapshot_file(
        &snapshot.root,
        "resource-manifest.json",
        &manifest.contents,
        false,
        &manifest_path,
    )?;
    verify_materialized_snapshot(
        &snapshot.root,
        &manifest.contents,
        &expected,
        platform,
        required_content_sha256,
    )?;
    seal_snapshot_directories(&snapshot.root)?;
    Ok(Some(SelectedResourceRoot::snapshot(snapshot)))
}

#[cfg(feature = "tauri-shell")]
fn malformed_resource_origin(
    current: &Path,
    platform: ResourceOriginPlatform,
    reason: &'static str,
) -> ResourceManifestError {
    ResourceManifestError::MalformedOrigin {
        path: current.to_path_buf(),
        platform: platform.name(),
        reason,
    }
}

#[cfg(feature = "tauri-shell")]
fn select_resource_root_for_provenance(
    current: &Path,
    product_name: &str,
    origin_platform: ResourceOriginPlatform,
    provenance: ResourceProvenance<'_>,
    exact_nix_layout: bool,
    exact_cargo_layout: bool,
) -> Result<SelectedResourceRoot, ResourceManifestError> {
    let platform = origin_platform.resource_platform()?;
    if platform == ResourcePlatform::Unix && exact_nix_layout {
        require_resource_origin(provenance, ResourceOrigin::NixExact)?;
        return Ok(SelectedResourceRoot::borrowed(current));
    }
    if platform == ResourcePlatform::Unix && exact_cargo_layout {
        require_resource_origin(provenance, ResourceOrigin::Cargo)?;
        return Ok(SelectedResourceRoot::borrowed(current));
    }
    require_resource_origin(provenance, ResourceOrigin::Packaged)?;
    let ResourceProvenance::Packaged(required_content_sha256) = provenance else {
        return Err(ResourceManifestError::InvalidProvenance);
    };
    let candidate = packaged_resource_candidate(
        current,
        product_name,
        origin_platform,
        MissingManifestPolicy::Reject,
    )?;
    materialize_resource_manifest(&candidate, origin_platform, Some(required_content_sha256))?
        .ok_or_else(|| ResourceManifestError::Missing {
            path: candidate.join("resource-manifest.json"),
        })
}

#[cfg(feature = "tauri-shell")]
fn packaged_resource_candidate(
    current: &Path,
    product_name: &str,
    origin_platform: ResourceOriginPlatform,
    missing_policy: MissingManifestPolicy,
) -> Result<PathBuf, ResourceManifestError> {
    if !current.is_absolute() {
        return Err(malformed_resource_origin(
            current,
            origin_platform,
            "expected an absolute executable directory",
        ));
    }
    let resource_platform = origin_platform.resource_platform()?;
    if resource_platform == ResourcePlatform::Unix
        && missing_policy == MissingManifestPolicy::DevelopmentFallback
        && has_exact_cargo_output_layout(current, current)
    {
        return Ok(current.to_path_buf());
    }

    match origin_platform {
        ResourceOriginPlatform::Windows => Ok(current.to_path_buf()),
        ResourceOriginPlatform::Linux => {
            if current.file_name() != Some(OsStr::new("bin")) {
                return Err(malformed_resource_origin(
                    current,
                    origin_platform,
                    "Linux packaged executable directory basename is not bin",
                ));
            }
            let mut components = Path::new(product_name).components();
            if !matches!(
                (components.next(), components.next()),
                (Some(std::path::Component::Normal(component)), None)
                    if component == OsStr::new(product_name)
            ) {
                return Err(malformed_resource_origin(
                    current,
                    origin_platform,
                    "Tauri product name is not one non-empty path component",
                ));
            }
            let parent = current.parent().ok_or_else(|| {
                malformed_resource_origin(
                    current,
                    origin_platform,
                    "executable directory has no installation prefix",
                )
            })?;
            Ok(parent.join("lib").join(product_name))
        }
        ResourceOriginPlatform::Macos => {
            if current.file_name() != Some(OsStr::new("MacOS")) {
                return Err(malformed_resource_origin(
                    current,
                    origin_platform,
                    "macOS packaged executable directory basename is not MacOS",
                ));
            }
            let contents = current.parent().ok_or_else(|| {
                malformed_resource_origin(
                    current,
                    origin_platform,
                    "executable directory has no application Contents directory",
                )
            })?;
            if contents.file_name() != Some(OsStr::new("Contents")) {
                return Err(malformed_resource_origin(
                    current,
                    origin_platform,
                    "macOS packaged executable directory parent basename is not Contents",
                ));
            }
            Ok(contents.join("Resources"))
        }
        ResourceOriginPlatform::Unsupported => Err(ResourceManifestError::UnsupportedOrigin {
            platform: std::env::consts::OS,
        }),
    }
}

#[cfg(all(feature = "tauri-shell", test))]
fn select_tauri_resource_root(
    current: &Path,
    candidate: &Path,
) -> Result<SelectedResourceRoot, ResourceManifestError> {
    select_tauri_resource_root_for_platform_with_policy(
        current,
        candidate,
        ResourcePlatform::current(),
        MissingManifestPolicy::Reject,
    )
}

#[cfg(all(feature = "tauri-shell", test))]
fn select_tauri_resource_root_for_platform(
    current: &Path,
    candidate: &Path,
    platform: ResourcePlatform,
    development_build: bool,
) -> Result<SelectedResourceRoot, ResourceManifestError> {
    select_tauri_resource_root_for_platform_with_policy(
        current,
        candidate,
        platform,
        MissingManifestPolicy::runtime(platform, development_build),
    )
}

#[cfg(all(feature = "tauri-shell", test))]
fn select_tauri_resource_root_for_platform_with_policy(
    current: &Path,
    candidate: &Path,
    platform: ResourcePlatform,
    missing_policy: MissingManifestPolicy,
) -> Result<SelectedResourceRoot, ResourceManifestError> {
    let origin_platform = match platform {
        ResourcePlatform::Unix => ResourceOriginPlatform::Linux,
        ResourcePlatform::Windows => ResourceOriginPlatform::Windows,
    };
    if let Some(selected) = materialize_resource_manifest(candidate, origin_platform, None)? {
        return Ok(selected);
    }
    if platform == ResourcePlatform::Unix
        && missing_policy == MissingManifestPolicy::DevelopmentFallback
        && has_exact_cargo_output_layout(current, candidate)
    {
        return Ok(SelectedResourceRoot::borrowed(current));
    }
    Err(ResourceManifestError::Missing {
        path: candidate.join("resource-manifest.json"),
    })
}

#[cfg(feature = "tauri-shell")]
fn has_exact_cargo_output_layout(current: &Path, candidate: &Path) -> bool {
    if current != candidate || current.file_name().is_none_or(OsStr::is_empty) {
        return false;
    }

    let Some(profile_parent) = current.parent() else {
        return false;
    };
    let target_root = if profile_parent.file_name() == Some(OsStr::new("target")) {
        profile_parent
    } else {
        if profile_parent.file_name().is_none_or(OsStr::is_empty) {
            return false;
        }
        let Some(target_root) = profile_parent.parent() else {
            return false;
        };
        if target_root.file_name() != Some(OsStr::new("target")) {
            return false;
        }
        target_root
    };

    let Ok(canonical_current) = current.canonicalize() else {
        return false;
    };
    if canonical_current != current || !target_root.is_dir() {
        return false;
    }

    let marker = current.join(".cargo-lock");
    let Ok(marker_metadata) = std::fs::symlink_metadata(&marker) else {
        return false;
    };
    !metadata_is_link_or_reparse_point(&marker_metadata) && marker_metadata.file_type().is_file()
}

#[cfg(feature = "tauri-shell")]
fn is_valid_nix_store_package_basename(value: &OsStr) -> bool {
    const NIX_BASE32: &str = "0123456789abcdfghijklmnpqrsvwxyz";

    let Some(value) = value.to_str() else {
        return false;
    };
    let Some((hash, name)) = value.split_once('-') else {
        return false;
    };
    hash.len() == 32
        && !name.is_empty()
        && hash
            .bytes()
            .all(|byte| NIX_BASE32.as_bytes().contains(&byte))
}

#[cfg(feature = "tauri-shell")]
fn has_exact_nix_resource_layout(current: &Path) -> bool {
    if !cfg!(unix) || current.file_name() != Some(OsStr::new("bin")) {
        return false;
    }
    let Some(package_root) = current.parent() else {
        return false;
    };
    if package_root.parent() != Some(Path::new("/nix/store"))
        || !package_root
            .file_name()
            .is_some_and(is_valid_nix_store_package_basename)
    {
        return false;
    }
    for directory in [package_root, current] {
        let Ok(metadata) = std::fs::symlink_metadata(directory) else {
            return false;
        };
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return false;
        }
    }
    let web_link = current.join("web");
    let Ok(web_link_metadata) = std::fs::symlink_metadata(&web_link) else {
        return false;
    };
    if !web_link_metadata.file_type().is_symlink()
        || std::fs::read_link(&web_link).ok().as_deref() != Some(Path::new("../web"))
    {
        return false;
    }
    let package_web = package_root.join("web");
    let package_dist = package_web.join("dist");
    let package_entrypoint = package_dist.join("index.html");
    for directory in [&package_web, &package_dist] {
        let Ok(metadata) = std::fs::symlink_metadata(directory) else {
            return false;
        };
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return false;
        }
    }
    let Ok(entrypoint_metadata) = std::fs::symlink_metadata(&package_entrypoint) else {
        return false;
    };
    if entrypoint_metadata.file_type().is_symlink() || !entrypoint_metadata.file_type().is_file() {
        return false;
    }
    let Ok(canonical_package_root) = package_root.canonicalize() else {
        return false;
    };
    let Ok(canonical_current) = current.canonicalize() else {
        return false;
    };
    let Ok(canonical_package_web) = package_web.canonicalize() else {
        return false;
    };
    let Ok(canonical_web_link) = web_link.canonicalize() else {
        return false;
    };
    let Ok(canonical_entrypoint) = package_entrypoint.canonicalize() else {
        return false;
    };
    canonical_package_root == package_root
        && canonical_current == current
        && canonical_current.parent() == Some(canonical_package_root.as_path())
        && canonical_package_web == package_web
        && canonical_web_link == canonical_package_web
        && canonical_entrypoint == package_entrypoint
        && canonical_entrypoint.starts_with(&canonical_package_root)
}

async fn run_backend(
    request: PipelineRequest,
    before_dynamic: LoadedSettings,
    ui_mode: UiMode,
    exit_after_startup: bool,
    control: RunControl,
    desktop_settings: Option<DesktopRuntimeSettings>,
) -> Result<(), MainError> {
    let bootstrap = bootstrap_dynamic(request.clone(), before_dynamic).await?;
    if let Some(error) = bootstrap.startup_failure.as_ref() {
        tracing::error!(
            error = %error,
            "dynamic configuration is unavailable; continuing with static settings"
        );
    }
    let loaded = bootstrap.loaded;
    if let Some(settings) = desktop_settings {
        settings.set_close_behavior(
            loaded
                .settings
                .string("ui.desktop.close_behavior")?
                .parse::<CloseBehavior>()?,
        );
    }
    let bind_address = loaded
        .settings
        .string("server.bind_address")?
        .parse::<IpAddr>()?;
    let port = u16::try_from(loaded.settings.integer("server.port")?)?;
    let web_root = PathBuf::from(loaded.settings.string("server.web_dir")?);
    run_configured_controlled(
        AppOptions {
            listen_address: SocketAddr::new(bind_address, port),
            ui_mode,
            web_root,
            exit_after_startup,
        },
        request,
        loaded,
        bootstrap.host,
        bootstrap.runtime,
        control,
    )
    .await?;
    Ok(())
}

#[cfg(feature = "tauri-shell")]
async fn run_packaged_backend(
    mut request: PipelineRequest,
    before_dynamic: LoadedSettings,
    ui_mode: UiMode,
    exit_after_startup: bool,
    control: RunControl,
    desktop_settings: Option<DesktopRuntimeSettings>,
) -> Result<(), MainError> {
    let resource_root_guard = packaged_resource_root(&request.resource_root)?;
    request.resource_root = resource_root_guard.path().to_path_buf();
    let before_dynamic = before_dynamic.with_resource_root(request.resource_root.clone())?;
    let result = run_backend(
        request,
        before_dynamic,
        ui_mode,
        exit_after_startup,
        control,
        desktop_settings,
    )
    .await;
    drop(resource_root_guard);
    result
}

#[cfg(feature = "tauri-shell")]
async fn supervise_desktop_backend_task(
    inner_task: tokio::task::JoinHandle<Result<(), MainError>>,
    shutdown: ShutdownCoordinator,
) -> Result<(), MainError> {
    match inner_task.await {
        Ok(result) => {
            if let Err(error) = result.as_ref() {
                let _fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
            }
            result
        }
        Err(error) => {
            let error = MainError::BackendTask(error);
            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Err(error)
        }
    }
}

#[cfg(feature = "tauri-shell")]
async fn supervise_desktop_backend_startup<ReadinessGuard>(
    inner_task: tokio::task::JoinHandle<Result<(), MainError>>,
    shutdown: ShutdownCoordinator,
    readiness_guard: ReadinessGuard,
) -> Result<(), MainError> {
    let result = supervise_desktop_backend_task(inner_task, shutdown).await;
    drop(readiness_guard);
    result
}

#[cfg(feature = "tauri-shell")]
async fn run_desktop(
    request: PipelineRequest,
    before_dynamic: LoadedSettings,
) -> Result<(), MainError> {
    let runtime_settings = DesktopRuntimeSettings::new(
        before_dynamic
            .settings
            .string("ui.desktop.close_behavior")?
            .parse::<CloseBehavior>()?,
    );
    let disable_compositing =
        before_dynamic.pre_dynamic_final_boolean_with_cli("ui.desktop.disable_compositing")?;
    let shell_config = DesktopShellConfig {
        config_directory: before_dynamic.roots.config.clone(),
        runtime_settings: runtime_settings.clone(),
        disable_compositing,
    };
    let shutdown = ShutdownCoordinator::new();
    let lifecycle = DesktopLifecycle::new(shutdown.clone());
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let readiness_guard = ready_sender.clone();
    let backend_stopped_before_readiness = Arc::new(AtomicBool::new(false));
    let startup_failure_marker = Arc::clone(&backend_stopped_before_readiness);
    let (task_sender, task_receiver) =
        mpsc::sync_channel::<tokio::task::JoinHandle<Result<(), MainError>>>(1);
    let runtime = tokio::runtime::Handle::current();
    let task_shutdown = shutdown.clone();
    let control = RunControl::new(shutdown.clone())
        .with_ready_sender(ready_sender)
        .with_desktop_settings(runtime_settings.clone());

    let shell_result = tokio::task::block_in_place(|| {
        run_tauri_shell(
            tauri::generate_context!(),
            shell_config,
            &lifecycle,
            move || {
                let inner_task = runtime.spawn(async move {
                    run_packaged_backend(
                        request,
                        before_dynamic,
                        UiMode::Desktop,
                        false,
                        control,
                        Some(runtime_settings),
                    )
                    .await
                });
                let supervisor_task = runtime.spawn(supervise_desktop_backend_startup(
                    inner_task,
                    task_shutdown,
                    readiness_guard,
                ));
                task_sender.send(supervisor_task).map_err(|_error| {
                    DesktopError::BackendStartup("backend task receiver was dropped".to_owned())
                })?;
                let actual_address = ready_receiver.recv().map_err(|_error| {
                    startup_failure_marker.store(true, Ordering::Release);
                    DesktopError::BackendAddressUnavailable
                })?;
                Ok(actual_address)
            },
        )
    });

    finish_desktop_run(
        shell_result,
        task_receiver,
        &shutdown,
        backend_stopped_before_readiness.load(Ordering::Acquire),
    )
    .await
}

#[cfg(feature = "tauri-shell")]
async fn finish_desktop_run(
    shell_result: Result<(), DesktopError>,
    task_receiver: mpsc::Receiver<tokio::task::JoinHandle<Result<(), MainError>>>,
    shutdown: &ShutdownCoordinator,
    backend_stopped_before_readiness: bool,
) -> Result<(), MainError> {
    if !backend_stopped_before_readiness && let Err(error) = shell_result.as_ref() {
        shutdown.request(ShutdownReason::FatalError(error.to_string()));
    }
    let backend_task = task_receiver.try_recv().ok();
    let backend_result = if let Some(task) = backend_task {
        match task.await {
            Ok(result) => result,
            Err(error) => Err(MainError::BackendTask(error)),
        }
    } else {
        Err(MainError::BackendNotStarted)
    };
    if backend_stopped_before_readiness {
        return if let Err(error) = backend_result {
            Err(error)
        } else {
            let error = shell_result
                .err()
                .unwrap_or(DesktopError::BackendAddressUnavailable);
            shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Err(MainError::Desktop(error))
        };
    }
    shell_result?;
    backend_result
}

#[cfg(all(feature = "tauri-shell", target_os = "linux"))]
fn should_reexec_for_linux_compositing(
    ui: UiArgument,
    exit_after_startup: bool,
    disable_compositing: bool,
    already_reexecuted: bool,
) -> bool {
    ui == UiArgument::Desktop && !exit_after_startup && disable_compositing && !already_reexecuted
}

#[cfg(all(feature = "tauri-shell", target_os = "linux"))]
fn reexec_for_linux_compositing() -> Result<(), MainError> {
    let executable = std::env::current_exe().map_err(MainError::CompositingRelaunch)?;
    let error = std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .env(COMPOSITING_REEXEC_MARKER, "1")
        .env("WEBKIT_DISABLE_COMPOSITING_MODE", "1")
        .exec();
    Err(MainError::CompositingRelaunch(error))
}

#[cfg(all(test, feature = "tauri-shell"))]
mod tests {
    use std::fs;
    use std::future::pending;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use serde_json::json;
    use tempfile::TempDir;
    use tokio::sync::oneshot;
    use tokio::time::timeout;

    use crate::desktop::DesktopError;
    use crate::runtime::{ShutdownCoordinator, ShutdownReason};

    #[cfg(unix)]
    use super::read_stable_resource_file_with;
    use super::{
        MissingManifestPolicy, ResourceManifestEntry, ResourceManifestError, ResourceOrigin,
        ResourceOriginPlatform, ResourcePlatform, ResourceProvenance, expected_resource_files,
        finish_desktop_run, has_exact_cargo_output_layout, is_valid_nix_store_package_basename,
        packaged_resource_candidate, parse_resource_provenance, provenance_accepts_origin,
        select_resource_root_for_provenance, select_tauri_resource_root,
        select_tauri_resource_root_for_platform,
        select_tauri_resource_root_for_platform_with_policy, sha256_bytes,
        supervise_desktop_backend_startup, supervise_desktop_backend_task,
        verify_materialized_snapshot, windows_attributes_include_reparse_point,
    };

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn ar_11_11_web_is_the_default_and_desktop_is_an_explicit_mode() {
        use clap::Parser as _;

        let default = super::Cli::try_parse_from(["pokecon"]).expect("default CLI");
        assert_eq!(default.ui, super::UiArgument::Web);
        assert_eq!(crate::UiMode::from(default.ui), crate::UiMode::Web);

        let desktop =
            super::Cli::try_parse_from(["pokecon", "--ui", "desktop"]).expect("desktop CLI");
        assert_eq!(desktop.ui, super::UiArgument::Desktop);
        assert_eq!(crate::UiMode::from(desktop.ui), crate::UiMode::Desktop);
    }

    struct FatalBeforeReadinessDropGuard {
        shutdown: ShutdownCoordinator,
        _readiness_sender: mpsc::SyncSender<super::SocketAddr>,
    }

    impl Drop for FatalBeforeReadinessDropGuard {
        fn drop(&mut self) {
            let reason = self.shutdown.reason();
            assert!(
                matches!(&reason, Some(ShutdownReason::FatalError(_))),
                "desktop backend terminal state must be classified before readiness closes, got {reason:?}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_compositing_reexec_predicate_is_exact() {
        for ui in [super::UiArgument::Web, super::UiArgument::Desktop] {
            for exit_after_startup in [false, true] {
                for disable_compositing in [false, true] {
                    for already_reexecuted in [false, true] {
                        let expected = ui == super::UiArgument::Desktop
                            && !exit_after_startup
                            && disable_compositing
                            && !already_reexecuted;

                        assert_eq!(
                            super::should_reexec_for_linux_compositing(
                                ui,
                                exit_after_startup,
                                disable_compositing,
                                already_reexecuted,
                            ),
                            expected,
                            "unexpected reexec decision for {ui:?}, exit_after_startup={exit_after_startup}, disable_compositing={disable_compositing}, already_reexecuted={already_reexecuted}",
                        );
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resource_snapshot_guard_drop_removes_the_pre_exec_tree() {
        let snapshot = super::ResourceSnapshot::create().expect("resource snapshot");
        let root = snapshot.root.clone();
        let container = root
            .parent()
            .expect("resource snapshot root must have a container")
            .to_path_buf();
        let guard = super::SelectedResourceRoot::snapshot(snapshot);

        assert!(container.is_dir());
        assert!(root.is_dir());
        drop(guard);
        assert!(!container.exists());
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn desktop_backend_supervisor_clean_completion_stays_clean() {
        let shutdown = ShutdownCoordinator::new();
        let inner_task = tokio::spawn(async { Ok::<(), super::MainError>(()) });
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));

        let result = timeout(TEST_TIMEOUT, supervisor_task)
            .await
            .expect("desktop backend supervisor must finish before the deadline")
            .expect("desktop backend supervisor task must not panic");

        assert!(result.is_ok());
        assert_eq!(shutdown.reason(), None);
    }

    #[tokio::test]
    async fn desktop_backend_supervisor_returned_error_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let inner_task = tokio::spawn(async { Err(super::MainError::BackendNotStarted) });
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));

        let shutdown_reason = timeout(TEST_TIMEOUT, shutdown.cancelled())
            .await
            .expect("returned backend error must cancel the shell before the deadline");
        let result = timeout(TEST_TIMEOUT, supervisor_task)
            .await
            .expect("desktop backend supervisor must finish before the deadline")
            .expect("desktop backend supervisor task must not panic");

        assert!(matches!(result, Err(super::MainError::BackendNotStarted)));
        assert_eq!(
            shutdown_reason,
            ShutdownReason::FatalError(
                "the primary desktop instance did not start its backend".to_owned()
            )
        );
    }

    #[tokio::test]
    async fn desktop_backend_supervisor_post_readiness_panic_exits_shell_and_retains_join_error() {
        let shutdown = ShutdownCoordinator::new();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let (panic_sender, panic_receiver) = oneshot::channel();
        let inner_task: tokio::task::JoinHandle<Result<(), super::MainError>> =
            tokio::spawn(async move {
                ready_sender
                    .send(())
                    .expect("simulated backend readiness receiver");
                panic_receiver.await.expect("post-readiness panic trigger");
                panic!("injected post-readiness backend panic");
            });
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));

        timeout(TEST_TIMEOUT, ready_receiver)
            .await
            .expect("simulated backend must publish readiness before the deadline")
            .expect("simulated backend readiness sender");
        panic_sender
            .send(())
            .expect("post-readiness backend panic trigger receiver");
        let shutdown_reason = timeout(TEST_TIMEOUT, shutdown.cancelled())
            .await
            .expect("backend panic must cancel the shell before the deadline");

        let late_shell_shutdown = shutdown.clone();
        let simulated_shell_completion =
            tokio::spawn(async move { late_shell_shutdown.cancelled().await });
        let late_shell_reason = timeout(TEST_TIMEOUT, simulated_shell_completion)
            .await
            .expect("late shell cancellation waiter must finish before the deadline")
            .expect("simulated shell completion task must not panic");
        assert_eq!(late_shell_reason, shutdown_reason);

        let result = timeout(TEST_TIMEOUT, supervisor_task)
            .await
            .expect("desktop backend supervisor must finish before the deadline")
            .expect("desktop backend supervisor task must not panic");
        let Err(super::MainError::BackendTask(error)) = result else {
            panic!("post-readiness backend panic must retain its typed join error");
        };
        assert!(error.is_panic());
        let payload = error.into_panic();
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"injected post-readiness backend panic")
        );
    }

    #[tokio::test]
    async fn desktop_backend_supervisor_cancellation_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let inner_task = tokio::spawn(pending::<Result<(), super::MainError>>());
        inner_task.abort();
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));

        let shutdown_reason = timeout(TEST_TIMEOUT, shutdown.cancelled())
            .await
            .expect("backend cancellation must cancel the shell before the deadline");
        let result = timeout(TEST_TIMEOUT, supervisor_task)
            .await
            .expect("desktop backend supervisor must finish before the deadline")
            .expect("desktop backend supervisor task must not panic");

        let Err(super::MainError::BackendTask(error)) = result else {
            panic!("backend cancellation must retain its typed join error");
        };
        assert!(error.is_cancelled());
        assert!(matches!(
            shutdown_reason,
            ShutdownReason::FatalError(reason)
                if reason.starts_with("desktop backend task failed: task ")
                    && reason.ends_with(" was cancelled")
        ));
    }

    #[tokio::test]
    async fn desktop_backend_supervisor_preaccepted_shutdown_retains_cancellation_error() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let inner_task = tokio::spawn(pending::<Result<(), super::MainError>>());
        inner_task.abort();
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));

        let result = timeout(TEST_TIMEOUT, supervisor_task)
            .await
            .expect("desktop backend supervisor must finish before the deadline")
            .expect("desktop backend supervisor task must not panic");

        let Err(super::MainError::BackendTask(error)) = result else {
            panic!("preaccepted shutdown must retain the backend cancellation join error");
        };
        assert!(error.is_cancelled());
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_readiness_backend_error_is_classified_before_shell_and_returned() {
        let shutdown = ShutdownCoordinator::new();
        let (ready_sender, ready_receiver) = mpsc::sync_channel::<super::SocketAddr>(1);
        let readiness_guard = FatalBeforeReadinessDropGuard {
            shutdown: shutdown.clone(),
            _readiness_sender: ready_sender.clone(),
        };
        let inner_task = tokio::spawn(async move {
            drop(ready_sender);
            Err(super::MainError::Desktop(DesktopError::BackendStartup(
                "injected pre-readiness backend failure".to_owned(),
            )))
        });
        let supervisor_task = tokio::spawn(supervise_desktop_backend_startup(
            inner_task,
            shutdown.clone(),
            readiness_guard,
        ));
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(supervisor_task)
            .expect("published backend supervisor task");
        drop(task_sender);

        let receiver_shutdown = shutdown.clone();
        let receive_task = tokio::task::spawn_blocking(move || {
            ready_receiver
                .recv()
                .expect_err("backend must stop before publishing readiness");
            receiver_shutdown.reason()
        });
        let observed_reason = timeout(TEST_TIMEOUT, receive_task)
            .await
            .expect("readiness receiver must close before the deadline")
            .expect("readiness observer must not panic");
        let expected_reason = ShutdownReason::FatalError(
            "desktop backend startup failed: injected pre-readiness backend failure".to_owned(),
        );
        assert_eq!(observed_reason, Some(expected_reason.clone()));
        assert!(!shutdown.request(ShutdownReason::FatalError(
            "Tauri desktop shell panicked: derived setup failure".to_owned(),
        )));

        let result = finish_desktop_run(
            Err(DesktopError::TauriPanic("derived setup failure".to_owned())),
            task_receiver,
            &shutdown,
            true,
        )
        .await;

        assert!(matches!(
            result,
            Err(super::MainError::Desktop(DesktopError::BackendStartup(message)))
                if message == "injected pre-readiness backend failure"
        ));
        assert_eq!(shutdown.reason(), Some(expected_reason));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_readiness_backend_panic_is_classified_before_shell_and_returned() {
        let shutdown = ShutdownCoordinator::new();
        let (ready_sender, ready_receiver) = mpsc::sync_channel::<super::SocketAddr>(1);
        let readiness_guard = FatalBeforeReadinessDropGuard {
            shutdown: shutdown.clone(),
            _readiness_sender: ready_sender.clone(),
        };
        let inner_task: tokio::task::JoinHandle<Result<(), super::MainError>> =
            tokio::spawn(async move {
                drop(ready_sender);
                panic!("injected pre-readiness backend panic");
            });
        let supervisor_task = tokio::spawn(supervise_desktop_backend_startup(
            inner_task,
            shutdown.clone(),
            readiness_guard,
        ));
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(supervisor_task)
            .expect("published backend supervisor task");
        drop(task_sender);

        let receiver_shutdown = shutdown.clone();
        let receive_task = tokio::task::spawn_blocking(move || {
            ready_receiver
                .recv()
                .expect_err("backend must panic before publishing readiness");
            receiver_shutdown.reason()
        });
        let observed_reason = timeout(TEST_TIMEOUT, receive_task)
            .await
            .expect("readiness receiver must close before the deadline")
            .expect("readiness observer must not panic")
            .expect("backend panic must be classified before readiness closes");
        assert!(matches!(
            &observed_reason,
            ShutdownReason::FatalError(reason)
                if reason.starts_with("desktop backend task failed: task ")
                    && reason.contains(" panicked with message ")
                    && reason.contains("injected pre-readiness backend panic")
        ));
        assert!(!shutdown.request(ShutdownReason::FatalError(
            "Tauri desktop shell panicked: derived setup failure".to_owned(),
        )));

        let result = finish_desktop_run(
            Err(DesktopError::TauriPanic("derived setup failure".to_owned())),
            task_receiver,
            &shutdown,
            true,
        )
        .await;

        let Err(super::MainError::BackendTask(error)) = result else {
            panic!("pre-readiness backend panic must retain its typed join error");
        };
        assert!(error.is_panic());
        let payload = error.into_panic();
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"injected pre-readiness backend panic")
        );
        assert_eq!(shutdown.reason(), Some(observed_reason));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_readiness_backend_cancellation_is_classified_before_shell_and_returned() {
        let shutdown = ShutdownCoordinator::new();
        let (ready_sender, ready_receiver) = mpsc::sync_channel::<super::SocketAddr>(1);
        let readiness_guard = FatalBeforeReadinessDropGuard {
            shutdown: shutdown.clone(),
            _readiness_sender: ready_sender.clone(),
        };
        let inner_task = tokio::spawn(async move {
            let _ready_sender = ready_sender;
            pending::<Result<(), super::MainError>>().await
        });
        inner_task.abort();
        let supervisor_task = tokio::spawn(supervise_desktop_backend_startup(
            inner_task,
            shutdown.clone(),
            readiness_guard,
        ));
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(supervisor_task)
            .expect("published backend supervisor task");
        drop(task_sender);

        let receiver_shutdown = shutdown.clone();
        let receive_task = tokio::task::spawn_blocking(move || {
            ready_receiver
                .recv()
                .expect_err("backend must be cancelled before publishing readiness");
            receiver_shutdown.reason()
        });
        let observed_reason = timeout(TEST_TIMEOUT, receive_task)
            .await
            .expect("readiness receiver must close before the deadline")
            .expect("readiness observer must not panic")
            .expect("backend cancellation must be classified before readiness closes");
        assert!(matches!(
            &observed_reason,
            ShutdownReason::FatalError(reason)
                if reason.starts_with("desktop backend task failed: task ")
                    && reason.ends_with(" was cancelled")
        ));
        assert!(!shutdown.request(ShutdownReason::FatalError(
            "Tauri desktop shell panicked: derived setup failure".to_owned(),
        )));

        let result = finish_desktop_run(
            Err(DesktopError::TauriPanic("derived setup failure".to_owned())),
            task_receiver,
            &shutdown,
            true,
        )
        .await;

        let Err(super::MainError::BackendTask(error)) = result else {
            panic!("pre-readiness backend cancellation must retain its typed join error");
        };
        assert!(error.is_cancelled());
        assert_eq!(shutdown.reason(), Some(observed_reason));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pre_readiness_clean_backend_stop_retains_the_shell_error() {
        let shutdown = ShutdownCoordinator::new();
        let (ready_sender, ready_receiver) = mpsc::sync_channel::<super::SocketAddr>(1);
        let readiness_guard = ready_sender.clone();
        let inner_task = tokio::spawn(async move {
            drop(ready_sender);
            Ok::<(), super::MainError>(())
        });
        let supervisor_task = tokio::spawn(supervise_desktop_backend_startup(
            inner_task,
            shutdown.clone(),
            readiness_guard,
        ));
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(supervisor_task)
            .expect("published backend supervisor task");
        drop(task_sender);

        timeout(
            TEST_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                ready_receiver
                    .recv()
                    .expect_err("backend must stop before publishing readiness");
            }),
        )
        .await
        .expect("readiness receiver must close before the deadline")
        .expect("readiness observer must not panic");
        assert_eq!(shutdown.reason(), None);

        let result = finish_desktop_run(
            Err(DesktopError::BackendAddressUnavailable),
            task_receiver,
            &shutdown,
            true,
        )
        .await;

        assert!(matches!(
            result,
            Err(super::MainError::Desktop(
                DesktopError::BackendAddressUnavailable
            ))
        ));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "desktop backend listener was not published".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn caught_shell_panic_cancels_and_joins_the_published_backend() {
        let shutdown = ShutdownCoordinator::new();
        let cancellation = shutdown.cancellation_token();
        let cleanup_completed = Arc::new(AtomicBool::new(false));
        let task_cleanup_completed = Arc::clone(&cleanup_completed);
        let backend_task = tokio::spawn(async move {
            cancellation.cancelled().await;
            task_cleanup_completed.store(true, Ordering::Release);
            Ok::<(), super::MainError>(())
        });
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(backend_task)
            .expect("published backend task");
        drop(task_sender);

        let result = finish_desktop_run(
            Err(DesktopError::TauriPanic("setup failed".to_owned())),
            task_receiver,
            &shutdown,
            false,
        )
        .await;

        assert!(matches!(
            result,
            Err(super::MainError::Desktop(DesktopError::TauriPanic(message)))
                if message == "setup failed"
        ));
        assert!(cleanup_completed.load(Ordering::Acquire));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "Tauri desktop shell panicked: setup failed".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn shell_error_remains_primary_after_backend_cleanup_error() {
        let shutdown = ShutdownCoordinator::new();
        let cancellation = shutdown.cancellation_token();
        let cleanup_completed = Arc::new(AtomicBool::new(false));
        let task_cleanup_completed = Arc::clone(&cleanup_completed);
        let inner_task = tokio::spawn(async move {
            cancellation.cancelled().await;
            task_cleanup_completed.store(true, Ordering::Release);
            Err(super::MainError::BackendNotStarted)
        });
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(supervisor_task)
            .expect("published backend supervisor task");
        drop(task_sender);

        let result = finish_desktop_run(
            Err(DesktopError::TauriPanic("shell failed first".to_owned())),
            task_receiver,
            &shutdown,
            false,
        )
        .await;

        assert!(matches!(
            result,
            Err(super::MainError::Desktop(DesktopError::TauriPanic(message)))
                if message == "shell failed first"
        ));
        assert!(cleanup_completed.load(Ordering::Acquire));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "Tauri desktop shell panicked: shell failed first".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn shell_error_remains_primary_after_backend_cleanup_panic() {
        let shutdown = ShutdownCoordinator::new();
        let cancellation = shutdown.cancellation_token();
        let cleanup_completed = Arc::new(AtomicBool::new(false));
        let task_cleanup_completed = Arc::clone(&cleanup_completed);
        let inner_task: tokio::task::JoinHandle<Result<(), super::MainError>> =
            tokio::spawn(async move {
                cancellation.cancelled().await;
                task_cleanup_completed.store(true, Ordering::Release);
                panic!("injected backend cleanup panic");
            });
        let supervisor_task =
            tokio::spawn(supervise_desktop_backend_task(inner_task, shutdown.clone()));
        let (task_sender, task_receiver) = mpsc::sync_channel(1);
        task_sender
            .send(supervisor_task)
            .expect("published backend supervisor task");
        drop(task_sender);

        let result = finish_desktop_run(
            Err(DesktopError::TauriPanic("shell failed first".to_owned())),
            task_receiver,
            &shutdown,
            false,
        )
        .await;

        assert!(matches!(
            result,
            Err(super::MainError::Desktop(DesktopError::TauriPanic(message)))
                if message == "shell failed first"
        ));
        assert!(cleanup_completed.load(Ordering::Acquire));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "Tauri desktop shell panicked: shell failed first".to_owned()
            ))
        );
    }

    fn write_web_entrypoint(root: &Path) {
        let resource = root.join("web/dist/index.html");
        fs::create_dir_all(resource.parent().expect("resource parent"))
            .expect("resource directories");
        fs::write(&resource, b"fixture\n").expect("resource file");
    }

    fn write_cargo_output_marker(root: &Path) {
        fs::create_dir_all(root).expect("Cargo output root");
        fs::write(root.join(".cargo-lock"), b"").expect("Cargo output marker");
    }

    fn write_manifest(root: &Path, platform: ResourcePlatform, files: &[ResourceManifestEntry]) {
        let canonical = serde_json::to_vec(files).expect("canonical manifest file inventory");
        let manifest = json!({
            "schema_version": 1,
            "layout_version": 1,
            "platform": platform.name(),
            "files": files,
            "content_sha256": sha256_bytes(&canonical),
        });
        fs::write(
            root.join("resource-manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("resource manifest");
    }

    fn write_manifest_fixture(
        root: &Path,
        platform: ResourcePlatform,
        paths: &[&str],
    ) -> Vec<ResourceManifestEntry> {
        let mut files = Vec::new();
        for path in paths {
            let contents = format!("fixture resource: {path}\n").into_bytes();
            let destination = root.join(path);
            fs::create_dir_all(destination.parent().expect("resource parent"))
                .expect("resource directories");
            fs::write(&destination, &contents).expect("resource file");
            #[cfg(unix)]
            if platform.requires_executable(path) {
                use std::os::unix::fs::PermissionsExt;

                fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
                    .expect("executable resource permissions");
            }
            files.push(ResourceManifestEntry {
                path: (*path).to_owned(),
                sha256: sha256_bytes(&contents),
                size: u64::try_from(contents.len()).expect("fixture size"),
            });
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        write_manifest(root, platform, &files);
        files
    }

    fn write_valid_manifest_for(root: &Path, platform: ResourcePlatform) {
        write_manifest_fixture(root, platform, platform.required_files());
    }

    fn write_valid_manifest(root: &Path) {
        write_valid_manifest_for(root, ResourcePlatform::current());
    }

    fn manifest_content_sha256(root: &Path) -> String {
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(root.join("resource-manifest.json")).expect("resource manifest"),
        )
        .expect("resource manifest JSON");
        manifest["content_sha256"]
            .as_str()
            .expect("resource manifest content identity")
            .to_owned()
    }

    #[test]
    fn resource_provenance_parser_accepts_only_the_exact_grammar() {
        let digest = "0123456789abcdef".repeat(4);
        assert_eq!(
            parse_resource_provenance("development").expect("development provenance"),
            ResourceProvenance::Development
        );
        assert_eq!(
            parse_resource_provenance("nix-exact").expect("exact Nix provenance"),
            ResourceProvenance::NixExact
        );
        assert_eq!(
            parse_resource_provenance(&format!("packaged:{digest}")).expect("packaged provenance"),
            ResourceProvenance::Packaged(&digest)
        );

        for invalid in [
            String::new(),
            "development ".to_owned(),
            " development".to_owned(),
            "Development".to_owned(),
            "nix_exact".to_owned(),
            "nix-exact\n".to_owned(),
            "packaged:".to_owned(),
            format!("packaged:{}", "0".repeat(63)),
            format!("packaged:{}", "0".repeat(65)),
            format!("packaged:{}A", "0".repeat(63)),
            format!("packaged:{}g", "0".repeat(63)),
            format!("packaged:{}é", "0".repeat(63)),
        ] {
            assert!(
                matches!(
                    parse_resource_provenance(&invalid),
                    Err(ResourceManifestError::InvalidProvenance)
                ),
                "invalid provenance was accepted"
            );
        }
    }

    #[test]
    fn resource_provenance_origin_matrix_is_strictly_diagonal() {
        let digest = "0123456789abcdef".repeat(4);
        let provenances = [
            ResourceProvenance::Development,
            ResourceProvenance::NixExact,
            ResourceProvenance::Packaged(&digest),
        ];
        let origins = [
            ResourceOrigin::Cargo,
            ResourceOrigin::NixExact,
            ResourceOrigin::Packaged,
        ];

        for (provenance_index, provenance) in provenances.into_iter().enumerate() {
            for (origin_index, origin) in origins.into_iter().enumerate() {
                assert_eq!(
                    provenance_accepts_origin(provenance, origin),
                    provenance_index == origin_index,
                    "resource provenance/origin matrix is not diagonal"
                );
            }
        }
    }

    #[test]
    fn development_provenance_accepts_verified_cargo_layout_independent_of_profile() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("target/release");
        write_cargo_output_marker(&current);
        let exact_cargo_layout = has_exact_cargo_output_layout(&current, &current);
        assert!(exact_cargo_layout);

        let selected = select_resource_root_for_provenance(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            ResourceProvenance::Development,
            false,
            exact_cargo_layout,
        )
        .expect("verified release-profile Cargo origin");

        assert_eq!(selected.path(), current);
        assert!(matches!(
            select_resource_root_for_provenance(
                &current,
                "PokeCon Controller",
                ResourceOriginPlatform::Linux,
                ResourceProvenance::NixExact,
                false,
                exact_cargo_layout,
            ),
            Err(ResourceManifestError::OriginMismatch { origin: "Cargo" })
        ));
    }

    #[test]
    fn development_and_nix_provenance_reject_a_valid_packaged_origin() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("installation/bin");
        let candidate = temporary.path().join("installation/lib/PokeCon Controller");
        fs::create_dir_all(&current).expect("application directory");
        write_valid_manifest(&candidate);

        for provenance in [
            ResourceProvenance::Development,
            ResourceProvenance::NixExact,
        ] {
            assert!(matches!(
                select_resource_root_for_provenance(
                    &current,
                    "PokeCon Controller",
                    ResourceOriginPlatform::Linux,
                    provenance,
                    false,
                    false,
                ),
                Err(ResourceManifestError::OriginMismatch { origin: "packaged" })
            ));
        }
    }

    #[test]
    fn packaged_provenance_requires_the_matching_manifest_identity() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("installation/bin");
        let candidate = temporary.path().join("installation/lib/PokeCon Controller");
        fs::create_dir_all(&current).expect("application directory");
        write_valid_manifest(&candidate);
        let digest = manifest_content_sha256(&candidate);

        let selected = select_resource_root_for_provenance(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            ResourceProvenance::Packaged(&digest),
            false,
            false,
        )
        .expect("matching packaged resource identity");
        assert_ne!(selected.path(), candidate);
        drop(selected);

        let wrong_digest = if digest == "0".repeat(64) {
            "1".repeat(64)
        } else {
            "0".repeat(64)
        };
        assert!(matches!(
            select_resource_root_for_provenance(
                &current,
                "PokeCon Controller",
                ResourceOriginPlatform::Linux,
                ResourceProvenance::Packaged(&wrong_digest),
                false,
                false,
            ),
            Err(ResourceManifestError::IdentityMismatch { path })
                if path == candidate.join("resource-manifest.json")
        ));
    }

    #[test]
    fn private_snapshot_revalidates_the_compiled_packaged_identity() {
        let temporary = TempDir::new().expect("temporary directory");
        let snapshot = temporary.path().join("snapshot");
        write_valid_manifest(&snapshot);
        let manifest_path = snapshot.join("resource-manifest.json");
        let manifest_contents = fs::read(&manifest_path).expect("resource manifest");
        let expected = expected_resource_files(
            &manifest_contents,
            &manifest_path,
            ResourcePlatform::current(),
        )
        .expect("verified manifest");
        let wrong_digest = if expected.content_sha256 == "0".repeat(64) {
            "1".repeat(64)
        } else {
            "0".repeat(64)
        };

        assert!(matches!(
            verify_materialized_snapshot(
                &snapshot,
                &manifest_contents,
                &expected,
                ResourcePlatform::current(),
                Some(&wrong_digest),
            ),
            Err(ResourceManifestError::IdentityMismatch { path }) if path == manifest_path
        ));
    }

    #[test]
    fn valid_manifest_is_materialized_as_a_read_only_private_snapshot() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        fs::create_dir_all(&current).expect("current root");
        write_valid_manifest(&candidate);
        let source_entrypoint = candidate.join("web/dist/index.html");
        let verified_contents = fs::read(&source_entrypoint).expect("source entrypoint");
        let manifest_contents =
            fs::read(candidate.join("resource-manifest.json")).expect("source manifest");

        let selected =
            select_tauri_resource_root(&current, &candidate).expect("valid candidate snapshot");
        let snapshot_root = selected.path().to_path_buf();
        assert_ne!(snapshot_root, candidate);
        assert_eq!(
            fs::read(snapshot_root.join("web/dist/index.html")).expect("snapshot entrypoint"),
            verified_contents
        );
        assert_eq!(
            fs::read(snapshot_root.join("resource-manifest.json")).expect("snapshot manifest"),
            manifest_contents
        );

        fs::write(&source_entrypoint, b"mutated after verification\n")
            .expect("mutate source entrypoint");
        assert_eq!(
            fs::read(snapshot_root.join("web/dist/index.html")).expect("snapshot entrypoint"),
            verified_contents
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                fs::metadata(&snapshot_root)
                    .expect("snapshot root metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o500
            );
            assert_eq!(
                fs::metadata(snapshot_root.join("web/dist/index.html"))
                    .expect("snapshot data metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o400
            );
            assert_eq!(
                fs::metadata(snapshot_root.join("pokecon-worker"))
                    .expect("snapshot executable metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o500
            );
            assert_eq!(
                fs::metadata(snapshot_root.parent().expect("snapshot container"))
                    .expect("snapshot container metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }

        drop(selected);
        assert!(!snapshot_root.exists());
    }

    #[test]
    fn ordinary_current_web_root_does_not_bypass_valid_tauri_candidate() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_web_entrypoint(&current);
        write_valid_manifest(&candidate);

        let selected = select_tauri_resource_root(&current, &candidate).expect("valid candidate");
        assert_ne!(selected.path(), candidate);
        assert_eq!(
            fs::read(selected.path().join("web/dist/index.html")).expect("snapshot entrypoint"),
            fs::read(candidate.join("web/dist/index.html")).expect("candidate entrypoint")
        );
    }

    #[cfg(unix)]
    #[test]
    fn copied_nix_layout_lookalike_does_not_bypass_a_corrupt_candidate() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory");
        let package = temporary
            .path()
            .join("00000000000000000000000000000000-pokecon");
        let current = package.join("bin");
        let candidate = temporary.path().join("candidate");
        fs::create_dir_all(&current).expect("package bin");
        write_web_entrypoint(&package);
        symlink("../web", current.join("web")).expect("package Web symlink");
        fs::create_dir_all(&candidate).expect("candidate root");
        fs::write(candidate.join("resource-manifest.json"), b"not JSON")
            .expect("invalid candidate manifest");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn outside_bin_web_symlink_falls_back_to_valid_tauri_candidate() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("package/bin");
        let outside = temporary.path().join("outside");
        let candidate = temporary.path().join("candidate");
        fs::create_dir_all(&current).expect("package bin");
        write_web_entrypoint(&outside);
        symlink(outside.join("web"), current.join("web")).expect("outside Web symlink");
        write_valid_manifest(&candidate);

        let selected = select_tauri_resource_root(&current, &candidate).expect("valid candidate");
        assert_ne!(selected.path(), candidate);
    }

    #[test]
    fn linux_debian_candidate_uses_the_exact_tauri_product_name() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("usr/bin");
        let candidate = packaged_resource_candidate(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            MissingManifestPolicy::Reject,
        )
        .expect("Linux Debian resource candidate");

        assert_eq!(
            candidate,
            temporary.path().join("usr/lib/PokeCon Controller")
        );
    }

    #[test]
    fn windows_candidate_is_colocated_with_the_executable() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("Program Files/PokeCon Controller");
        let candidate = packaged_resource_candidate(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Windows,
            MissingManifestPolicy::Reject,
        )
        .expect("Windows resource candidate");

        assert_eq!(candidate, current);
    }

    #[test]
    fn windows_candidate_accepts_the_absolute_filesystem_root() {
        let temporary = TempDir::new().expect("temporary directory");
        let root = temporary
            .path()
            .ancestors()
            .last()
            .expect("absolute filesystem root");
        let candidate = packaged_resource_candidate(
            root,
            "PokeCon Controller",
            ResourceOriginPlatform::Windows,
            MissingManifestPolicy::Reject,
        )
        .expect("Windows root resource candidate");

        assert_eq!(candidate, root);
    }

    #[test]
    fn macos_candidate_is_the_sibling_contents_resources_directory() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary
            .path()
            .join("Applications/PokeCon Controller.app/Contents/MacOS");
        let candidate = packaged_resource_candidate(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Macos,
            MissingManifestPolicy::Reject,
        )
        .expect("macOS resource candidate");

        assert_eq!(
            candidate,
            temporary
                .path()
                .join("Applications/PokeCon Controller.app/Contents/Resources")
        );
    }

    #[test]
    fn macos_packaged_origin_excludes_only_the_tauri_icon_from_its_private_snapshot() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("Foo.app/Contents/MacOS");
        let candidate = temporary.path().join("Foo.app/Contents/Resources");
        fs::create_dir_all(&current).expect("macOS executable directory");
        write_valid_manifest_for(&candidate, ResourcePlatform::Unix);
        fs::write(candidate.join("icon.icns"), b"Tauri application icon\n")
            .expect("Tauri application icon");
        let digest = manifest_content_sha256(&candidate);

        let selected = select_resource_root_for_provenance(
            &current,
            "Foo",
            ResourceOriginPlatform::Macos,
            ResourceProvenance::Packaged(&digest),
            false,
            false,
        )
        .expect("macOS packaged resources with the Tauri-owned icon");

        assert_ne!(selected.path(), candidate);
        assert!(selected.path().join("web/dist/index.html").is_file());
        assert!(!selected.path().join("icon.icns").exists());
        assert_eq!(
            fs::read(selected.path().join("resource-manifest.json")).expect("snapshot manifest"),
            fs::read(candidate.join("resource-manifest.json")).expect("source manifest")
        );
    }

    #[test]
    fn linux_packaged_origin_rejects_the_same_undeclared_icon() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("installation/bin");
        let candidate = temporary.path().join("installation/lib/Foo");
        fs::create_dir_all(&current).expect("Linux executable directory");
        write_valid_manifest_for(&candidate, ResourcePlatform::Unix);
        fs::write(candidate.join("icon.icns"), b"undeclared icon\n").expect("undeclared icon");
        let digest = manifest_content_sha256(&candidate);

        assert!(
            select_resource_root_for_provenance(
                &current,
                "Foo",
                ResourceOriginPlatform::Linux,
                ResourceProvenance::Packaged(&digest),
                false,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn macos_packaged_origin_rejects_other_undeclared_icon_artifacts() {
        for relative in [
            "Assets.car",
            "nested/icon.icns",
            "alternate.icns",
            "ApplicationIcon.icon",
        ] {
            let temporary = TempDir::new().expect("temporary directory");
            let current = temporary.path().join("Foo.app/Contents/MacOS");
            let candidate = temporary.path().join("Foo.app/Contents/Resources");
            fs::create_dir_all(&current).expect("macOS executable directory");
            write_valid_manifest_for(&candidate, ResourcePlatform::Unix);
            let extra = candidate.join(relative);
            fs::create_dir_all(extra.parent().expect("extra resource parent"))
                .expect("extra resource directory");
            fs::write(&extra, b"undeclared artifact\n").expect("undeclared artifact");
            let digest = manifest_content_sha256(&candidate);

            assert!(
                select_resource_root_for_provenance(
                    &current,
                    "Foo",
                    ResourceOriginPlatform::Macos,
                    ResourceProvenance::Packaged(&digest),
                    false,
                    false,
                )
                .is_err(),
                "macOS accepted undeclared artifact {relative:?}"
            );
        }
    }

    #[test]
    fn macos_packager_owned_icon_name_must_be_a_regular_file() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("Foo.app/Contents/MacOS");
        let candidate = temporary.path().join("Foo.app/Contents/Resources");
        fs::create_dir_all(&current).expect("macOS executable directory");
        write_valid_manifest_for(&candidate, ResourcePlatform::Unix);
        fs::create_dir(candidate.join("icon.icns")).expect("icon-named directory");
        let digest = manifest_content_sha256(&candidate);

        assert!(
            select_resource_root_for_provenance(
                &current,
                "Foo",
                ResourceOriginPlatform::Macos,
                ResourceProvenance::Packaged(&digest),
                false,
                false,
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn macos_packager_owned_icon_must_have_exactly_one_hard_link() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("Foo.app/Contents/MacOS");
        let candidate = temporary.path().join("Foo.app/Contents/Resources");
        fs::create_dir_all(&current).expect("macOS executable directory");
        write_valid_manifest_for(&candidate, ResourcePlatform::Unix);
        let icon_source = temporary.path().join("icon-source.icns");
        fs::write(&icon_source, b"hard-linked Tauri icon\n").expect("icon source");
        fs::hard_link(&icon_source, candidate.join("icon.icns")).expect("hard-linked Tauri icon");
        let digest = manifest_content_sha256(&candidate);

        assert!(
            select_resource_root_for_provenance(
                &current,
                "Foo",
                ResourceOriginPlatform::Macos,
                ResourceProvenance::Packaged(&digest),
                false,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn macos_packager_owned_icon_cannot_be_declared_in_the_manifest() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("Foo.app/Contents/MacOS");
        let candidate = temporary.path().join("Foo.app/Contents/Resources");
        fs::create_dir_all(&current).expect("macOS executable directory");
        let mut paths = ResourcePlatform::Unix.required_files().to_vec();
        paths.push("icon.icns");
        write_manifest_fixture(&candidate, ResourcePlatform::Unix, &paths);
        let digest = manifest_content_sha256(&candidate);

        assert!(
            select_resource_root_for_provenance(
                &current,
                "Foo",
                ResourceOriginPlatform::Macos,
                ResourceProvenance::Packaged(&digest),
                false,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn macos_exact_nix_provenance_bypasses_packaged_suffix_validation() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary
            .path()
            .join("nix/store/00000000000000000000000000000000-pokecon/bin");

        let selected = select_resource_root_for_provenance(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Macos,
            ResourceProvenance::NixExact,
            true,
            false,
        )
        .expect("exact Nix resource root");
        assert_eq!(selected.path(), current);
        let packaged_digest = "0".repeat(64);
        for provenance in [
            ResourceProvenance::Development,
            ResourceProvenance::Packaged(&packaged_digest),
        ] {
            assert!(matches!(
                select_resource_root_for_provenance(
                    &current,
                    "PokeCon Controller",
                    ResourceOriginPlatform::Macos,
                    provenance,
                    true,
                    false,
                ),
                Err(ResourceManifestError::OriginMismatch { origin: "Nix" })
            ));
        }
        assert!(matches!(
            packaged_resource_candidate(
                &current,
                "PokeCon Controller",
                ResourceOriginPlatform::Macos,
                MissingManifestPolicy::Reject,
            ),
            Err(ResourceManifestError::MalformedOrigin { path, platform: "macos", .. })
                if path == current
        ));
    }

    #[test]
    fn linux_packaged_candidate_rejects_a_non_bin_executable_directory() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("usr/sbin");
        let result = packaged_resource_candidate(
            &current,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            MissingManifestPolicy::Reject,
        );

        assert!(matches!(
            result,
            Err(ResourceManifestError::MalformedOrigin { path, platform: "linux", .. })
                if path == current
        ));
    }

    #[test]
    fn macos_packaged_candidate_requires_the_exact_contents_macos_suffix() {
        let temporary = TempDir::new().expect("temporary directory");
        for current in [
            temporary
                .path()
                .join("Applications/PokeCon Controller.app/Contents/Binaries"),
            temporary
                .path()
                .join("Applications/PokeCon Controller.app/Bundle/MacOS"),
        ] {
            let result = packaged_resource_candidate(
                &current,
                "PokeCon Controller",
                ResourceOriginPlatform::Macos,
                MissingManifestPolicy::Reject,
            );

            assert!(matches!(
                result,
                Err(ResourceManifestError::MalformedOrigin { path, platform: "macos", .. })
                    if path == current
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn development_origin_uses_current_for_both_exact_cargo_layouts() {
        let temporary = TempDir::new().expect("temporary directory");
        for relative in ["target/debug", "target/nix-tasks/release"] {
            let current = temporary.path().join(relative);
            write_cargo_output_marker(&current);

            let candidate = packaged_resource_candidate(
                &current,
                "PokeCon Controller",
                ResourceOriginPlatform::Linux,
                MissingManifestPolicy::DevelopmentFallback,
            )
            .expect("development Cargo resource candidate");
            assert_eq!(candidate, current);
        }
    }

    #[cfg(unix)]
    #[test]
    fn release_cargo_is_rejected_and_non_cargo_bin_uses_the_linux_mapping() {
        let temporary = TempDir::new().expect("temporary directory");
        let cargo_current = temporary.path().join("target/debug");
        write_cargo_output_marker(&cargo_current);
        let non_cargo_current = temporary.path().join("installation/bin");
        fs::create_dir_all(&non_cargo_current).expect("non-Cargo executable directory");

        let release_result = packaged_resource_candidate(
            &cargo_current,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            MissingManifestPolicy::Reject,
        );
        let non_cargo_candidate = packaged_resource_candidate(
            &non_cargo_current,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            MissingManifestPolicy::DevelopmentFallback,
        )
        .expect("non-Cargo resource candidate");

        assert!(matches!(
            release_result,
            Err(ResourceManifestError::MalformedOrigin { path, platform: "linux", .. })
                if path == cargo_current
        ));
        assert_eq!(
            non_cargo_candidate,
            temporary.path().join("installation/lib/PokeCon Controller")
        );
    }

    #[test]
    fn malformed_or_unsupported_resource_origins_fail_typed() {
        let temporary = TempDir::new().expect("temporary directory");
        let root = temporary
            .path()
            .ancestors()
            .last()
            .expect("absolute filesystem root");
        let malformed = packaged_resource_candidate(
            root,
            "PokeCon Controller",
            ResourceOriginPlatform::Linux,
            MissingManifestPolicy::Reject,
        );
        assert!(matches!(
            malformed,
            Err(ResourceManifestError::MalformedOrigin { path, platform: "linux", .. })
                if path == root
        ));

        let unsupported = packaged_resource_candidate(
            &temporary.path().join("usr/bin"),
            "PokeCon Controller",
            ResourceOriginPlatform::Unsupported,
            MissingManifestPolicy::Reject,
        );
        assert!(matches!(
            unsupported,
            Err(ResourceManifestError::UnsupportedOrigin { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_policy_allows_exact_cargo_output_roots() {
        let temporary = TempDir::new().expect("temporary directory");
        for relative in ["target/debug", "target/nix-tasks/release"] {
            let current = temporary.path().join(relative);
            write_cargo_output_marker(&current);

            let selected = select_tauri_resource_root_for_platform(
                &current,
                &current,
                ResourcePlatform::Unix,
                true,
            )
            .expect("Cargo output fallback");
            assert_eq!(selected.path(), current);
        }
    }

    #[cfg(unix)]
    #[test]
    fn release_runtime_policy_rejects_exact_cargo_output_roots() {
        let temporary = TempDir::new().expect("temporary directory");
        for relative in ["target/debug", "target/nix-tasks/release"] {
            let current = temporary.path().join(relative);
            write_cargo_output_marker(&current);

            let result = select_tauri_resource_root_for_platform(
                &current,
                &current,
                ResourcePlatform::Unix,
                false,
            );
            assert!(matches!(
                result,
                Err(ResourceManifestError::Missing { path })
                    if path == current.join("resource-manifest.json")
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn runtime_policy_rejects_same_root_without_a_cargo_marker() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("target/debug");
        fs::create_dir_all(&current).expect("Cargo output root");

        let result = select_tauri_resource_root_for_platform(
            &current,
            &current,
            ResourcePlatform::Unix,
            true,
        );
        assert!(matches!(
            result,
            Err(ResourceManifestError::Missing { path })
                if path == current.join("resource-manifest.json")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn runtime_policy_rejects_noncanonical_or_inexact_cargo_output_ancestry() {
        let temporary = TempDir::new().expect("temporary directory");
        for relative in [
            "build/debug",
            "target/nix-tasks/custom/release",
            "target/../target/debug",
        ] {
            let current = temporary.path().join(relative);
            write_cargo_output_marker(&current);

            let result = select_tauri_resource_root_for_platform(
                &current,
                &current,
                ResourcePlatform::Unix,
                true,
            );
            assert!(matches!(result, Err(ResourceManifestError::Missing { .. })));
        }
    }

    #[cfg(unix)]
    #[test]
    fn cargo_output_marker_must_be_a_non_link_regular_file() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::UnixListener;

        let temporary = TempDir::new().expect("temporary directory");

        let symlink_root = temporary.path().join("symlink/target/debug");
        fs::create_dir_all(&symlink_root).expect("symlink Cargo output root");
        let marker_target = temporary.path().join("marker-target");
        fs::write(&marker_target, b"").expect("marker target");
        symlink(&marker_target, symlink_root.join(".cargo-lock")).expect("marker symlink");
        assert!(
            select_tauri_resource_root_for_platform(
                &symlink_root,
                &symlink_root,
                ResourcePlatform::Unix,
                true,
            )
            .is_err()
        );

        let special_root = temporary.path().join("special/target/debug");
        fs::create_dir_all(&special_root).expect("special Cargo output root");
        let _socket =
            UnixListener::bind(special_root.join(".cargo-lock")).expect("special marker socket");
        assert!(
            select_tauri_resource_root_for_platform(
                &special_root,
                &special_root,
                ResourcePlatform::Unix,
                true,
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_policy_rejects_a_different_cargo_output_candidate() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("target/debug");
        let candidate = temporary.path().join("target/release");
        write_cargo_output_marker(&current);
        write_cargo_output_marker(&candidate);

        let result = select_tauri_resource_root_for_platform(
            &current,
            &candidate,
            ResourcePlatform::Unix,
            true,
        );
        assert!(matches!(
            result,
            Err(ResourceManifestError::Missing { path })
                if path == candidate.join("resource-manifest.json")
        ));
    }

    #[test]
    fn production_policy_rejects_a_missing_manifest() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        fs::create_dir_all(&candidate).expect("candidate root");

        let result = select_tauri_resource_root_for_platform_with_policy(
            &current,
            &candidate,
            ResourcePlatform::Unix,
            MissingManifestPolicy::Reject,
        );
        assert!(matches!(
            result,
            Err(ResourceManifestError::Missing { path })
                if path == candidate.join("resource-manifest.json")
        ));
    }

    #[test]
    fn windows_candidate_equal_to_current_still_requires_a_manifest() {
        let temporary = TempDir::new().expect("temporary directory");
        let install_root = temporary.path().join("target/debug");
        write_cargo_output_marker(&install_root);

        let result = select_tauri_resource_root_for_platform_with_policy(
            &install_root,
            &install_root,
            ResourcePlatform::Windows,
            MissingManifestPolicy::DevelopmentFallback,
        );
        assert!(matches!(
            result,
            Err(ResourceManifestError::Missing { path })
                if path == install_root.join("resource-manifest.json")
        ));
    }

    #[test]
    fn windows_reparse_point_attribute_is_rejected_by_the_portable_predicate() {
        assert!(windows_attributes_include_reparse_point(0x0000_0400));
        assert!(windows_attributes_include_reparse_point(0x0000_0420));
        assert!(!windows_attributes_include_reparse_point(0x0000_0020));
    }

    #[test]
    fn existing_invalid_tauri_manifests_are_errors() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        fs::create_dir_all(&candidate).expect("candidate root");
        let manifest = candidate.join("resource-manifest.json");

        fs::write(&manifest, b"not JSON").expect("invalid manifest fixture");
        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_tauri_manifest_is_an_error() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        let manifest = candidate.join("resource-manifest.json");
        let target = temporary.path().join("manifest-target.json");
        fs::rename(&manifest, &target).expect("manifest target");
        symlink(&target, &manifest).expect("manifest symlink");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[test]
    fn same_size_content_corruption_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        let entrypoint = candidate.join("web/dist/index.html");
        let mut contents = fs::read(&entrypoint).expect("Web fixture");
        contents[0] ^= 1;
        fs::write(&entrypoint, contents).expect("same-size corruption");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[test]
    fn wrong_content_identity_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        let manifest_path = candidate.join("resource-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).expect("resource manifest"))
                .expect("manifest JSON");
        manifest["content_sha256"] = json!("0".repeat(64));
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("wrong manifest identity");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[test]
    fn extra_undeclared_resource_file_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        fs::write(candidate.join("extra-resource"), b"extra\n").expect("extra resource");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[test]
    fn internally_consistent_manifest_missing_required_layout_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        let platform = ResourcePlatform::current();
        let paths = platform
            .required_files()
            .iter()
            .copied()
            .filter(|path| *path != "web/dist/index.html")
            .collect::<Vec<_>>();
        write_manifest_fixture(&candidate, platform, &paths);

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[test]
    fn noncanonical_manifest_file_order_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        let platform = ResourcePlatform::current();
        let mut files = write_manifest_fixture(&candidate, platform, platform.required_files());
        files.reverse();
        write_manifest(&candidate, platform, &files);

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[test]
    fn corrupt_same_directory_windows_candidate_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let install_root = temporary.path().join("windows-install");
        write_valid_manifest_for(&install_root, ResourcePlatform::Windows);
        fs::write(install_root.join("pokecon.exe"), b"application\n")
            .expect("application executable");
        fs::write(install_root.join("uninstall.exe"), b"uninstaller\n")
            .expect("uninstaller executable");
        fs::write(install_root.join("resource-manifest.json"), b"not JSON")
            .expect("corrupt resource manifest");

        assert!(
            select_tauri_resource_root_for_platform(
                &install_root,
                &install_root,
                ResourcePlatform::Windows,
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn windows_inventory_excludes_only_installer_owned_top_level_files() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("windows-install");
        let mut paths = ResourcePlatform::Windows.required_files().to_vec();
        paths.extend([
            "python/include/Python.h",
            "python/Lib/site.py",
            "python/DLLs/_ssl.pyd",
        ]);
        let files = write_manifest_fixture(&candidate, ResourcePlatform::Windows, &paths);
        assert_eq!(
            files
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            [
                "pokecon-worker.exe",
                "python-wheels/requirements.lock",
                "python-wheels/wheelhouse-manifest.json",
                "python/DLLs/_ssl.pyd",
                "python/Lib/site.py",
                "python/include/Python.h",
                "python/python.exe",
                "uv/uv.exe",
                "web/dist/index.html",
            ]
        );
        fs::write(candidate.join("pokecon.exe"), b"application\n").expect("application executable");
        fs::write(candidate.join("uninstall.exe"), b"uninstaller\n")
            .expect("uninstaller executable");

        let selected = select_tauri_resource_root_for_platform(
            &current,
            &candidate,
            ResourcePlatform::Windows,
            true,
        )
        .expect("Windows installer-owned files");
        assert_ne!(selected.path(), candidate);
        assert!(selected.path().join("pokecon-worker.exe").is_file());
        assert!(!selected.path().join("pokecon.exe").exists());
        assert!(!selected.path().join("uninstall.exe").exists());
        assert_eq!(
            fs::read(selected.path().join("resource-manifest.json")).expect("snapshot manifest"),
            fs::read(candidate.join("resource-manifest.json")).expect("source manifest")
        );

        fs::write(candidate.join("installer-extra.exe"), b"extra\n")
            .expect("arbitrary installer extra");
        assert!(
            select_tauri_resource_root_for_platform(
                &current,
                &candidate,
                ResourcePlatform::Windows,
                true,
            )
            .is_err()
        );
        fs::remove_file(candidate.join("installer-extra.exe"))
            .expect("remove arbitrary installer extra");
        fs::write(candidate.join("icon.icns"), b"macOS icon\n").expect("macOS icon");
        assert!(
            select_tauri_resource_root_for_platform(
                &current,
                &candidate,
                ResourcePlatform::Windows,
                true,
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_governed_resource_is_an_error() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        let entrypoint = candidate.join("web/dist/index.html");
        let target = temporary.path().join("index-target.html");
        fs::rename(&entrypoint, &target).expect("entrypoint target");
        symlink(&target, &entrypoint).expect("entrypoint symlink");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn special_resource_entry_is_an_error() {
        use std::os::unix::net::UnixListener;

        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        let _socket =
            UnixListener::bind(candidate.join("unexpected.sock")).expect("special resource socket");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn hard_linked_governed_resource_is_an_error() {
        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        fs::hard_link(
            candidate.join("web/dist/index.html"),
            temporary.path().join("outside-hard-link.html"),
        )
        .expect("governed resource hard link");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn required_unix_executable_without_execute_permission_is_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = TempDir::new().expect("temporary directory");
        let current = temporary.path().join("current");
        let candidate = temporary.path().join("candidate");
        write_valid_manifest(&candidate);
        fs::set_permissions(
            candidate.join("pokecon-worker"),
            fs::Permissions::from_mode(0o600),
        )
        .expect("remove executable permission");

        assert!(select_tauri_resource_root(&current, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn metadata_mutation_during_handle_read_is_an_error() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = TempDir::new().expect("temporary directory");
        let resource = temporary.path().join("resource");
        let manifest = temporary.path().join("resource-manifest.json");
        fs::write(&resource, b"stable bytes\n").expect("resource fixture");
        fs::set_permissions(&resource, fs::Permissions::from_mode(0o600))
            .expect("initial resource permissions");

        let result = read_stable_resource_file_with(&resource, &manifest, || {
            fs::set_permissions(&resource, fs::Permissions::from_mode(0o400))
                .expect("mutated resource permissions");
        });
        assert!(matches!(result, Err(ResourceManifestError::Invalid { .. })));
    }

    #[test]
    fn nix_store_package_basename_requires_exact_hash_and_nonempty_name() {
        use std::ffi::OsStr;

        assert!(is_valid_nix_store_package_basename(OsStr::new(
            "00000000000000000000000000000000-pokecon"
        )));
        for malformed in [
            "0000000000000000000000000000000-pokecon",
            "000000000000000000000000000000000-pokecon",
            "00000000000000000000000000000000-",
            "00000000000000000000000000000000",
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-pokecon",
            "0000000000000000000000000000000_-pokecon",
        ] {
            assert!(!is_valid_nix_store_package_basename(OsStr::new(malformed)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn equivalent_nonliteral_nix_web_symlink_does_not_get_priority() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory");
        let package = temporary.path().join("package");
        let current = package.join("bin");
        let candidate = temporary.path().join("candidate");
        fs::create_dir_all(&current).expect("package bin");
        write_web_entrypoint(&package);
        symlink(package.join("web"), current.join("web")).expect("absolute Web symlink");
        write_valid_manifest(&candidate);

        let selected = select_tauri_resource_root(&current, &candidate).expect("valid candidate");
        assert_ne!(selected.path(), candidate);
    }
}
