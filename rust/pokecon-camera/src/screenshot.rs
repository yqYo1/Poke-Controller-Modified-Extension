use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use atomic_write_file::AtomicWriteFile;
use chrono::Local;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder as _};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::frame::{BgrFrame, FrameError, NormalizedRegion};
use crate::media::LatestFrameSource;

/// Closed screenshot encoding formats.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    #[default]
    Png,
    Jpeg,
}

impl ScreenshotFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }
}

impl FromStr for ScreenshotFormat {
    type Err = ScreenshotError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("png") {
            Ok(Self::Png)
        } else if value.eq_ignore_ascii_case("jpeg") {
            Ok(Self::Jpeg)
        } else {
            Err(ScreenshotError::InvalidFormat)
        }
    }
}

impl<'de> Deserialize<'de> for ScreenshotFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

/// Whether native server-host paths may be accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenshotMode {
    Web,
    Desktop,
}

/// Destination-specific screenshot API union.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "destination", rename_all = "lowercase")]
pub enum ScreenshotDestination {
    Captures {
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        format: Option<ScreenshotFormat>,
        #[serde(default)]
        overwrite: bool,
    },
    Path {
        path: String,
        format: ScreenshotFormat,
        #[serde(default)]
        overwrite: bool,
    },
    Download {
        #[serde(default)]
        filename: Option<String>,
        format: ScreenshotFormat,
    },
}

/// Screenshot request with an optional normalized crop.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotRequest {
    #[serde(default)]
    pub region: Option<NormalizedRegion>,
    #[serde(flatten)]
    pub destination: ScreenshotDestination,
}

#[derive(Deserialize)]
#[serde(tag = "destination", rename_all = "lowercase", deny_unknown_fields)]
enum ScreenshotRequestWire {
    Captures {
        #[serde(default)]
        region: Option<NormalizedRegion>,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        format: Option<ScreenshotFormat>,
        #[serde(default)]
        overwrite: bool,
    },
    Path {
        #[serde(default)]
        region: Option<NormalizedRegion>,
        path: String,
        format: ScreenshotFormat,
        #[serde(default)]
        overwrite: bool,
    },
    Download {
        #[serde(default)]
        region: Option<NormalizedRegion>,
        #[serde(default)]
        filename: Option<String>,
        format: ScreenshotFormat,
    },
}

impl<'de> Deserialize<'de> for ScreenshotRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (region, destination) = match ScreenshotRequestWire::deserialize(deserializer)? {
            ScreenshotRequestWire::Captures {
                region,
                filename,
                format,
                overwrite,
            } => (
                region,
                ScreenshotDestination::Captures {
                    filename,
                    format,
                    overwrite,
                },
            ),
            ScreenshotRequestWire::Path {
                region,
                path,
                format,
                overwrite,
            } => (
                region,
                ScreenshotDestination::Path {
                    path,
                    format,
                    overwrite,
                },
            ),
            ScreenshotRequestWire::Download {
                region,
                filename,
                format,
            } => (region, ScreenshotDestination::Download { filename, format }),
        };
        Ok(Self {
            region,
            destination,
        })
    }
}

/// Saved-file response. Captures destinations expose only a relative display
/// path; explicit desktop paths repeat the user-selected path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SavedScreenshot {
    pub display_path: String,
    pub format: ScreenshotFormat,
}

/// Download response metadata and bytes for the HTTP layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadScreenshot {
    pub filename: String,
    pub format: ScreenshotFormat,
    pub content_type: &'static str,
    pub content_disposition: String,
    pub bytes: Vec<u8>,
}

/// Screenshot operation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScreenshotResult {
    Saved(SavedScreenshot),
    Download(DownloadScreenshot),
}

/// Shared immediately-applied screenshot defaults.
#[derive(Clone, Debug)]
pub struct ScreenshotRuntimeSettings {
    format: Arc<AtomicU8>,
    jpeg_quality: Arc<AtomicU8>,
}

impl ScreenshotRuntimeSettings {
    /// Creates validated defaults.
    ///
    /// # Errors
    ///
    /// Rejects JPEG quality outside 1..=100.
    pub fn new(format: ScreenshotFormat, jpeg_quality: u8) -> Result<Self, ScreenshotError> {
        validate_quality(jpeg_quality)?;
        Ok(Self {
            format: Arc::new(AtomicU8::new(encode_format(format))),
            jpeg_quality: Arc::new(AtomicU8::new(jpeg_quality)),
        })
    }

    #[must_use]
    pub fn format(&self) -> ScreenshotFormat {
        decode_format(self.format.load(Ordering::Acquire))
    }

    pub fn set_format(&self, format: ScreenshotFormat) {
        self.format.store(encode_format(format), Ordering::Release);
    }

    #[must_use]
    pub fn jpeg_quality(&self) -> u8 {
        self.jpeg_quality.load(Ordering::Acquire)
    }

    /// Updates JPEG and Motion JPEG quality.
    ///
    /// # Errors
    ///
    /// Rejects values outside 1..=100.
    pub fn set_jpeg_quality(&self, quality: u8) -> Result<(), ScreenshotError> {
        validate_quality(quality)?;
        self.jpeg_quality.store(quality, Ordering::Release);
        Ok(())
    }
}

impl Default for ScreenshotRuntimeSettings {
    fn default() -> Self {
        Self {
            format: Arc::new(AtomicU8::new(encode_format(ScreenshotFormat::Png))),
            jpeg_quality: Arc::new(AtomicU8::new(85)),
        }
    }
}

/// Injectable local timestamp source for deterministic collision tests.
pub trait ScreenshotClock: Send + Sync {
    /// Returns the fixed-baseline `YYYY-MM-DD_HH-MM-SS` base name.
    ///
    /// # Errors
    ///
    /// Returns a fixed clock failure without falling back to another name.
    fn local_basename(&self) -> Result<String, ScreenshotError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemScreenshotClock;

impl ScreenshotClock for SystemScreenshotClock {
    fn local_basename(&self) -> Result<String, ScreenshotError> {
        Ok(Local::now().format("%Y-%m-%d_%H-%M-%S").to_string())
    }
}

/// Current-frame crop/encode/save service.
#[derive(Clone)]
pub struct ScreenshotService {
    frames: LatestFrameSource,
    captures_root: PathBuf,
    mode: ScreenshotMode,
    settings: ScreenshotRuntimeSettings,
    clock: Arc<dyn ScreenshotClock>,
}

impl std::fmt::Debug for ScreenshotService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScreenshotService")
            .field("captures_root", &self.captures_root)
            .field("mode", &self.mode)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl ScreenshotService {
    #[must_use]
    pub fn new(
        frames: LatestFrameSource,
        data_root: impl Into<PathBuf>,
        mode: ScreenshotMode,
        settings: ScreenshotRuntimeSettings,
    ) -> Self {
        Self::with_clock(
            frames,
            data_root,
            mode,
            settings,
            Arc::new(SystemScreenshotClock),
        )
    }

    #[must_use]
    pub fn with_clock(
        frames: LatestFrameSource,
        data_root: impl Into<PathBuf>,
        mode: ScreenshotMode,
        settings: ScreenshotRuntimeSettings,
        clock: Arc<dyn ScreenshotClock>,
    ) -> Self {
        Self {
            frames,
            captures_root: data_root.into().join("Captures"),
            mode,
            settings,
            clock,
        }
    }

    #[must_use]
    pub const fn settings(&self) -> &ScreenshotRuntimeSettings {
        &self.settings
    }

    /// Copies the current published frame, crops it, and fulfills exactly one
    /// destination variant.
    ///
    /// # Errors
    ///
    /// Returns fixed no-frame, validation, conflict, encoding, or I/O errors.
    pub fn capture(
        &self,
        request: &ScreenshotRequest,
    ) -> Result<ScreenshotResult, ScreenshotError> {
        let snapshot = self
            .frames
            .latest()
            .ok_or(ScreenshotError::NoPublishedFrame)?;
        let mut frame = snapshot.frame.as_ref().clone();
        if let Some(region) = request.region {
            frame = frame.crop(region.to_pixels(frame.size()))?;
        }
        match &request.destination {
            ScreenshotDestination::Captures {
                filename,
                format,
                overwrite,
            } => {
                let format = format.unwrap_or_else(|| self.settings.format());
                let bytes = encode_frame(&frame, format, self.settings.jpeg_quality())?;
                let saved = self.save_captures(filename.as_deref(), format, *overwrite, &bytes)?;
                Ok(ScreenshotResult::Saved(saved))
            }
            ScreenshotDestination::Path {
                path,
                format,
                overwrite,
            } => {
                if self.mode != ScreenshotMode::Desktop {
                    return Err(ScreenshotError::PathDestinationUnavailable);
                }
                let target = normalize_absolute_path(path, *format)?;
                let bytes = encode_frame(&frame, *format, self.settings.jpeg_quality())?;
                save_explicit(&target, *overwrite, &bytes)?;
                Ok(ScreenshotResult::Saved(SavedScreenshot {
                    display_path: target.to_string_lossy().into_owned(),
                    format: *format,
                }))
            }
            ScreenshotDestination::Download { filename, format } => {
                let filename = self.download_filename(filename.as_deref(), *format)?;
                let bytes = encode_frame(&frame, *format, self.settings.jpeg_quality())?;
                Ok(ScreenshotResult::Download(DownloadScreenshot {
                    content_type: format.content_type(),
                    content_disposition: content_disposition(&filename),
                    filename,
                    format: *format,
                    bytes,
                }))
            }
        }
    }

    /// Fixed-baseline script compatibility: automatic names never overwrite;
    /// explicit relative names replace an existing regular file.
    ///
    /// # Errors
    ///
    /// Uses the same confinement, format, and file-type checks as REST saves.
    pub fn save_compatibility_frame(
        &self,
        frame: &BgrFrame,
        filename: Option<&str>,
        format: Option<ScreenshotFormat>,
    ) -> Result<SavedScreenshot, ScreenshotError> {
        let format = format.unwrap_or_else(|| self.settings.format());
        let bytes = encode_frame(frame, format, self.settings.jpeg_quality())?;
        let explicit = filename.is_some_and(|value| !value.is_empty());
        self.save_captures(filename, format, explicit, &bytes)
    }

    fn save_captures(
        &self,
        filename: Option<&str>,
        format: ScreenshotFormat,
        overwrite: bool,
        bytes: &[u8],
    ) -> Result<SavedScreenshot, ScreenshotError> {
        let root = prepare_captures_root(&self.captures_root)?;
        let automatic = filename.is_none_or(str::is_empty);
        let relative = if automatic {
            PathBuf::from(with_extension(&self.clock.local_basename()?, format))
        } else {
            normalize_relative_path(filename.unwrap_or_default(), format)?
        };
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let canonical_parent = prepare_confined_parent(&root, parent)?;
        let leaf_name = relative.file_name().ok_or(ScreenshotError::UnsafePath)?;
        let target = canonical_parent.join(leaf_name);
        let saved_target = if automatic {
            save_with_collision_suffix(&target, bytes)?
        } else {
            validate_confined_target(&root, &target)?;
            save_explicit(&target, overwrite, bytes)?;
            target
        };
        let display = saved_target
            .strip_prefix(&root)
            .map_err(|_| ScreenshotError::UnsafePath)?;
        Ok(SavedScreenshot {
            display_path: display_path(display),
            format,
        })
    }

    fn download_filename(
        &self,
        filename: Option<&str>,
        format: ScreenshotFormat,
    ) -> Result<String, ScreenshotError> {
        let base = if filename.is_none_or(str::is_empty) {
            self.clock.local_basename()?
        } else {
            let filename = filename.unwrap_or_default();
            validate_download_basename(filename)?;
            filename.to_owned()
        };
        Ok(with_extension(&base, format))
    }
}

fn encode_frame(
    frame: &BgrFrame,
    format: ScreenshotFormat,
    jpeg_quality: u8,
) -> Result<Vec<u8>, ScreenshotError> {
    validate_quality(jpeg_quality)?;
    let mut rgb = frame.pixels().to_vec();
    for pixel in rgb.chunks_exact_mut(3) {
        pixel.swap(0, 2);
    }
    let size = frame.size();
    let mut output = Vec::new();
    match format {
        ScreenshotFormat::Png => PngEncoder::new(&mut output)
            .write_image(&rgb, size.width(), size.height(), ColorType::Rgb8.into())
            .map_err(|_| ScreenshotError::EncodingFailed)?,
        ScreenshotFormat::Jpeg => JpegEncoder::new_with_quality(&mut output, jpeg_quality)
            .encode(&rgb, size.width(), size.height(), ColorType::Rgb8.into())
            .map_err(|_| ScreenshotError::EncodingFailed)?,
    }
    Ok(output)
}

fn prepare_captures_root(path: &Path) -> Result<PathBuf, ScreenshotError> {
    fs::create_dir_all(path).map_err(|_| ScreenshotError::IoFailed)?;
    fs::canonicalize(path).map_err(|_| ScreenshotError::IoFailed)
}

fn prepare_confined_parent(root: &Path, relative: &Path) -> Result<PathBuf, ScreenshotError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(ScreenshotError::UnsafePath);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
                    return Err(ScreenshotError::UnsafeTargetType);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| ScreenshotError::IoFailed)?;
            }
            Err(_) => return Err(ScreenshotError::IoFailed),
        }
        current = fs::canonicalize(&current).map_err(|_| ScreenshotError::IoFailed)?;
        if !current.starts_with(root) {
            return Err(ScreenshotError::UnsafePath);
        }
    }
    Ok(current)
}

fn validate_confined_target(root: &Path, target: &Path) -> Result<(), ScreenshotError> {
    let parent = target.parent().ok_or(ScreenshotError::UnsafePath)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| ScreenshotError::IoFailed)?;
    if !canonical_parent.starts_with(root) {
        return Err(ScreenshotError::UnsafePath);
    }
    if let Ok(metadata) = fs::symlink_metadata(target) {
        if metadata.file_type().is_symlink() {
            return Err(ScreenshotError::UnsafeTargetType);
        }
        if !metadata.is_file() {
            return Err(ScreenshotError::UnsafeTargetType);
        }
        let canonical = fs::canonicalize(target).map_err(|_| ScreenshotError::IoFailed)?;
        if !canonical.starts_with(root) {
            return Err(ScreenshotError::UnsafePath);
        }
    }
    Ok(())
}

fn normalize_relative_path(
    raw: &str,
    format: ScreenshotFormat,
) -> Result<PathBuf, ScreenshotError> {
    if raw.is_empty()
        || raw.starts_with(['/', '\\'])
        || looks_like_windows_drive(raw)
        || raw.chars().any(char::is_control)
    {
        return Err(ScreenshotError::UnsafePath);
    }
    let mut path = PathBuf::new();
    for component in raw.split(['/', '\\']) {
        if component.is_empty() || component == "." || component == ".." || component.contains(':')
        {
            return Err(ScreenshotError::UnsafePath);
        }
        path.push(component);
    }
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(ScreenshotError::UnsafePath)?;
    let adjusted = with_extension(file_name, format);
    path.set_file_name(adjusted);
    Ok(path)
}

fn normalize_absolute_path(
    raw: &str,
    format: ScreenshotFormat,
) -> Result<PathBuf, ScreenshotError> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return Err(ScreenshotError::UnsafePath);
    }
    let mut path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(ScreenshotError::UnsafePath);
    }
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(ScreenshotError::UnsafePath)?;
    path.set_file_name(with_extension(file_name, format));
    Ok(path)
}

fn validate_download_basename(value: &str) -> Result<(), ScreenshotError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\\', '\0', '\r', '\n'])
        || value.chars().any(char::is_control)
    {
        return Err(ScreenshotError::UnsafePath);
    }
    Ok(())
}

fn with_extension(name: &str, format: ScreenshotFormat) -> String {
    let path = Path::new(name);
    let recognized = path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            ["png", "jpg", "jpeg"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        });
    if recognized {
        let mut adjusted = path.to_path_buf();
        adjusted.set_extension(format.extension());
        adjusted.to_string_lossy().into_owned()
    } else {
        format!("{name}.{}", format.extension())
    }
}

fn save_with_collision_suffix(base: &Path, bytes: &[u8]) -> Result<PathBuf, ScreenshotError> {
    for suffix in 0..=u32::MAX {
        let target = if suffix == 0 {
            base.to_path_buf()
        } else {
            suffixed_path(base, suffix)?
        };
        match create_new_file(&target, bytes) {
            Ok(()) => return Ok(target),
            Err(ScreenshotError::Conflict) => {}
            Err(error) => return Err(error),
        }
    }
    Err(ScreenshotError::IoFailed)
}

fn suffixed_path(base: &Path, suffix: u32) -> Result<PathBuf, ScreenshotError> {
    let stem = base
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or(ScreenshotError::UnsafePath)?;
    let extension = base
        .extension()
        .and_then(OsStr::to_str)
        .ok_or(ScreenshotError::UnsafePath)?;
    Ok(base.with_file_name(format!("{stem}_{suffix}.{extension}")))
}

fn save_explicit(target: &Path, overwrite: bool, bytes: &[u8]) -> Result<(), ScreenshotError> {
    if let Ok(metadata) = fs::symlink_metadata(target) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ScreenshotError::UnsafeTargetType);
        }
        if !overwrite {
            return Err(ScreenshotError::Conflict);
        }
    }
    if overwrite {
        let mut file = AtomicWriteFile::open(target).map_err(|_| ScreenshotError::IoFailed)?;
        file.write_all(bytes)
            .map_err(|_| ScreenshotError::IoFailed)?;
        file.commit().map_err(|_| ScreenshotError::IoFailed)
    } else {
        create_new_file(target, bytes)
    }
}

fn create_new_file(target: &Path, bytes: &[u8]) -> Result<(), ScreenshotError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(target) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ScreenshotError::Conflict);
        }
        Err(_) => return Err(ScreenshotError::IoFailed),
    };
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(target);
        return Err(ScreenshotError::IoFailed);
    }
    Ok(())
}

fn content_disposition(filename: &str) -> String {
    let fallback = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
        rfc5987(filename)
    )
}

fn rfc5987(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
        {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn display_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn looks_like_windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

const fn encode_format(format: ScreenshotFormat) -> u8 {
    match format {
        ScreenshotFormat::Png => 0,
        ScreenshotFormat::Jpeg => 1,
    }
}

const fn decode_format(value: u8) -> ScreenshotFormat {
    if value == 1 {
        ScreenshotFormat::Jpeg
    } else {
        ScreenshotFormat::Png
    }
}

const fn validate_quality(quality: u8) -> Result<(), ScreenshotError> {
    if quality == 0 || quality > 100 {
        Err(ScreenshotError::InvalidJpegQuality)
    } else {
        Ok(())
    }
}

/// Fixed screenshot failure suitable for REST status classification.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ScreenshotError {
    #[error("screenshot format is invalid")]
    InvalidFormat,
    #[error("JPEG quality must be in 1..=100")]
    InvalidJpegQuality,
    #[error("no complete camera frame is published")]
    NoPublishedFrame,
    #[error("screenshot crop is invalid")]
    InvalidRegion,
    #[error("screenshot path is unsafe")]
    UnsafePath,
    #[error("screenshot target is a symlink or non-regular file")]
    UnsafeTargetType,
    #[error("screenshot target already exists")]
    Conflict,
    #[error("native path screenshots are unavailable in web mode")]
    PathDestinationUnavailable,
    #[error("screenshot encoding failed")]
    EncodingFailed,
    #[error("screenshot filesystem operation failed")]
    IoFailed,
    #[error("local screenshot timestamp acquisition failed")]
    ClockFailed,
}

impl From<FrameError> for ScreenshotError {
    fn from(_value: FrameError) -> Self {
        Self::InvalidRegion
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::GenericImageView as _;
    use tempfile::TempDir;

    use super::{
        ScreenshotClock, ScreenshotDestination, ScreenshotError, ScreenshotFormat, ScreenshotMode,
        ScreenshotRequest, ScreenshotResult, ScreenshotRuntimeSettings, ScreenshotService,
    };
    use crate::frame::{BgrFrame, CaptureResolution, NormalizedRegion};
    use crate::media::LatestFrameSource;

    #[derive(Debug)]
    struct FixedClock;

    impl ScreenshotClock for FixedClock {
        fn local_basename(&self) -> Result<String, ScreenshotError> {
            Ok("2026-07-19_12-34-56".to_owned())
        }
    }

    fn fixture(mode: ScreenshotMode) -> (TempDir, LatestFrameSource, ScreenshotService) {
        let data = tempfile::tempdir().unwrap();
        let frames = LatestFrameSource::new();
        frames.publish(
            1,
            BgrFrame::solid(CaptureResolution::R640x360, [10, 20, 30]),
        );
        let service = ScreenshotService::with_clock(
            frames.clone(),
            data.path(),
            mode,
            ScreenshotRuntimeSettings::default(),
            Arc::new(FixedClock),
        );
        (data, frames, service)
    }

    fn captures_request(
        filename: Option<&str>,
        format: Option<ScreenshotFormat>,
        overwrite: bool,
    ) -> ScreenshotRequest {
        ScreenshotRequest {
            region: None,
            destination: ScreenshotDestination::Captures {
                filename: filename.map(str::to_owned),
                format,
                overwrite,
            },
        }
    }

    #[test]
    fn download_crop_encodes_rgb_and_sanitizes_content_disposition() {
        let (data, _ring, service) = fixture(ScreenshotMode::Web);
        let request = ScreenshotRequest {
            region: Some(NormalizedRegion::new(0.0, 0.0, 0.5, 1.0).unwrap()),
            destination: ScreenshotDestination::Download {
                filename: Some("名前 \"capture\".PNG".to_owned()),
                format: ScreenshotFormat::Jpeg,
            },
        };
        let ScreenshotResult::Download(download) = service.capture(&request).unwrap() else {
            panic!("download variant expected");
        };
        assert_eq!(download.filename, "名前 \"capture\".jpg");
        assert_eq!(download.content_type, "image/jpeg");
        assert!(download.content_disposition.contains("filename*=UTF-8''"));
        assert!(download.content_disposition.contains("%22"));
        assert!(!download.content_disposition.contains(['\r', '\n']));
        let decoded = image::load_from_memory(&download.bytes).unwrap();
        assert_eq!(decoded.dimensions(), (320, 360));
        assert_eq!(decoded.to_rgb8().get_pixel(0, 0).0, [30, 20, 10]);
        assert!(!data.path().join("Captures").exists());
    }

    #[test]
    fn automatic_names_use_exclusive_smallest_collision_suffixes() {
        let (data, _ring, service) = fixture(ScreenshotMode::Web);
        let request = captures_request(None, None, false);
        let workers = (0..8)
            .map(|_| {
                let service = service.clone();
                let request = request.clone();
                std::thread::spawn(move || service.capture(&request).unwrap())
            })
            .collect::<Vec<_>>();
        let mut names = workers
            .into_iter()
            .map(|worker| match worker.join().unwrap() {
                ScreenshotResult::Saved(saved) => saved.display_path,
                ScreenshotResult::Download(_) => panic!("saved variant expected"),
            })
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names.len(), 8);
        names.dedup();
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"2026-07-19_12-34-56.png".to_owned()));
        assert!(names.contains(&"2026-07-19_12-34-56_1.png".to_owned()));
        assert_eq!(
            std::fs::read_dir(data.path().join("Captures"))
                .unwrap()
                .count(),
            8
        );
    }

    #[test]
    fn explicit_extension_conflict_and_overwrite_rules_are_consistent() {
        let (data, frames, service) = fixture(ScreenshotMode::Web);
        let request = captures_request(
            Some("nested/photo.PNG"),
            Some(ScreenshotFormat::Jpeg),
            false,
        );
        let ScreenshotResult::Saved(saved) = service.capture(&request).unwrap() else {
            panic!("saved variant expected");
        };
        assert_eq!(saved.display_path, "nested/photo.jpg");
        let original_bytes = std::fs::read(data.path().join("Captures/nested/photo.jpg")).unwrap();
        assert_eq!(service.capture(&request), Err(ScreenshotError::Conflict));
        frames.publish(2, BgrFrame::solid(CaptureResolution::R640x360, [1, 2, 3]));
        let overwrite = captures_request(
            Some("nested/photo.jpeg"),
            Some(ScreenshotFormat::Jpeg),
            true,
        );
        service.capture(&overwrite).unwrap();
        let bytes = std::fs::read(data.path().join("Captures/nested/photo.jpg")).unwrap();
        assert_ne!(bytes, original_bytes);
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().dimensions(),
            (640, 360)
        );
    }

    #[test]
    fn captures_rejects_cross_platform_traversal_and_outside_symlinks() {
        let (data, _ring, service) = fixture(ScreenshotMode::Web);
        for unsafe_name in [
            "../outside",
            "sub/../../outside",
            "/absolute",
            r"C:\absolute",
            r"\\server\share",
            "line\nfeed",
        ] {
            assert_eq!(
                service.capture(&captures_request(Some(unsafe_name), None, false)),
                Err(ScreenshotError::UnsafePath),
                "{unsafe_name}"
            );
        }

        #[cfg(not(unix))]
        drop(data);

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let captures = data.path().join("Captures");
            let outside = data.path().join("outside");
            std::fs::create_dir_all(&captures).unwrap();
            std::fs::create_dir_all(&outside).unwrap();
            symlink(&outside, captures.join("escape")).unwrap();
            assert_eq!(
                service.capture(&captures_request(Some("escape/pwn"), None, false)),
                Err(ScreenshotError::UnsafePath)
            );
        }
    }

    #[test]
    fn native_path_is_desktop_only_and_requires_an_absolute_regular_target() {
        let (data, _ring, web) = fixture(ScreenshotMode::Web);
        let target = data.path().join("chosen.PNG");
        let request = ScreenshotRequest {
            region: None,
            destination: ScreenshotDestination::Path {
                path: target.to_string_lossy().into_owned(),
                format: ScreenshotFormat::Jpeg,
                overwrite: false,
            },
        };
        assert_eq!(
            web.capture(&request),
            Err(ScreenshotError::PathDestinationUnavailable)
        );
        let desktop = ScreenshotService::with_clock(
            web.frames.clone(),
            data.path(),
            ScreenshotMode::Desktop,
            ScreenshotRuntimeSettings::default(),
            Arc::new(FixedClock),
        );
        let ScreenshotResult::Saved(saved) = desktop.capture(&request).unwrap() else {
            panic!("saved variant expected");
        };
        assert!(saved.display_path.ends_with("chosen.jpg"));
        assert!(data.path().join("chosen.jpg").is_file());
    }

    #[test]
    fn invalid_or_absent_publication_never_falls_back_to_an_image() {
        let data = tempfile::tempdir().unwrap();
        let frames = LatestFrameSource::new();
        let service = ScreenshotService::with_clock(
            frames,
            data.path(),
            ScreenshotMode::Web,
            ScreenshotRuntimeSettings::default(),
            Arc::new(FixedClock),
        );
        assert_eq!(
            service.capture(&captures_request(None, None, false)),
            Err(ScreenshotError::NoPublishedFrame)
        );
    }

    #[test]
    fn screenshot_wire_union_rejects_unknown_fields_and_invalid_formats() {
        assert!(
            serde_json::from_value::<ScreenshotRequest>(serde_json::json!({
                "destination": "captures",
                "filename": null,
                "format": "PNG",
                "overwrite": false
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<ScreenshotRequest>(serde_json::json!({
                "destination": "download",
                "filename": null,
                "format": "gif"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ScreenshotRequest>(serde_json::json!({
                "destination": "captures",
                "filename": null,
                "format": null,
                "overwrite": false,
                "unexpected": true
            }))
            .is_err()
        );
    }
}
