use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::io;
#[cfg(target_os = "linux")]
use std::path::Path;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Camera selector retained in its original integer or UTF-8 string form.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
pub enum CameraSelector {
    Index(u32),
    Native(String),
}

impl CameraSelector {
    /// Creates an opaque native selector without case, path, or symlink
    /// normalization.
    ///
    /// # Errors
    ///
    /// Rejects empty strings and embedded NUL bytes.
    pub fn native(raw: impl Into<String>) -> Result<Self, CameraSelectorError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(CameraSelectorError::Empty);
        }
        if raw.contains('\0') {
            return Err(CameraSelectorError::ContainsNul);
        }
        Ok(Self::Native(raw))
    }

    #[must_use]
    pub const fn as_index(&self) -> Option<u32> {
        match self {
            Self::Index(index) => Some(*index),
            Self::Native(_) => None,
        }
    }

    #[must_use]
    pub fn as_native(&self) -> Option<&str> {
        match self {
            Self::Index(_) => None,
            Self::Native(selector) => Some(selector),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CameraSelectorWire {
    Index(u32),
    Native(String),
}

impl<'de> Deserialize<'de> for CameraSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match CameraSelectorWire::deserialize(deserializer)? {
            CameraSelectorWire::Index(index) => Ok(Self::Index(index)),
            CameraSelectorWire::Native(raw) => Self::native(raw).map_err(D::Error::custom),
        }
    }
}

/// REST-safe camera enumeration item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraDevice {
    pub selector: CameraSelector,
    pub label: String,
    pub available: bool,
}

impl CameraDevice {
    #[must_use]
    pub fn available(selector: CameraSelector, label: impl Into<String>) -> Self {
        Self {
            selector,
            label: label.into(),
            available: true,
        }
    }

    #[must_use]
    pub fn unavailable(selector: CameraSelector) -> Self {
        Self {
            label: selector_label(&selector),
            selector,
            available: false,
        }
    }
}

/// Native Windows descriptor used by both Media Foundation enumeration and
/// the platform-independent mock adapter tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsCameraDescriptor {
    pub index: u32,
    pub native_id: String,
    pub label: String,
}

/// Maps Media Foundation descriptors to opaque string selectors. An exact
/// configured selector remains visible as unavailable when not enumerated.
#[must_use]
pub fn select_windows_candidates(
    descriptors: Vec<WindowsCameraDescriptor>,
    configured: Option<&CameraSelector>,
) -> Vec<CameraDevice> {
    let mut seen = BTreeSet::new();
    let mut devices = descriptors
        .into_iter()
        .filter_map(|descriptor| {
            let selector = if descriptor.native_id.is_empty() {
                CameraSelector::Index(descriptor.index)
            } else {
                CameraSelector::native(descriptor.native_id).ok()?
            };
            seen.insert(selector.clone()).then(|| {
                CameraDevice::available(
                    selector,
                    nonempty_label(descriptor.label, descriptor.index),
                )
            })
        })
        .collect::<Vec<_>>();
    append_configured_if_missing(&mut devices, configured, false);
    devices
}

/// Re-enumerates native cameras while retaining the configured spelling.
///
/// # Errors
///
/// Returns a fixed error when the OS enumerator cannot complete.
pub fn enumerate_native_cameras(
    configured: Option<&CameraSelector>,
) -> Result<Vec<CameraDevice>, CameraEnumerationError> {
    #[cfg(target_os = "linux")]
    {
        enumerate_linux(Path::new("/dev"), configured).map_err(|_| CameraEnumerationError::Failed)
    }
    #[cfg(target_os = "windows")]
    {
        use nokhwa::utils::ApiBackend;

        let descriptors = nokhwa::query(ApiBackend::MediaFoundation)
            .map_err(|_| CameraEnumerationError::Failed)?
            .into_iter()
            .map(|info| WindowsCameraDescriptor {
                index: info.index().as_index().unwrap_or_default(),
                native_id: info.misc(),
                label: info.human_name(),
            })
            .collect();
        Ok(select_windows_candidates(descriptors, configured))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let mut devices = Vec::new();
        append_configured_if_missing(&mut devices, configured, false);
        Ok(devices)
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LinuxSelectorClass {
    ById,
    ByPath,
    Index,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
struct LinuxCandidate {
    device: CameraDevice,
    identity: String,
    class: LinuxSelectorClass,
}

#[cfg(target_os = "linux")]
fn enumerate_linux(
    dev_root: &Path,
    configured: Option<&CameraSelector>,
) -> io::Result<Vec<CameraDevice>> {
    let mut candidates = Vec::new();
    enumerate_linux_aliases(
        &dev_root.join("v4l/by-id"),
        LinuxSelectorClass::ById,
        &mut candidates,
    )?;
    enumerate_linux_aliases(
        &dev_root.join("v4l/by-path"),
        LinuxSelectorClass::ByPath,
        &mut candidates,
    )?;
    let entries = std::fs::read_dir(dev_root)?;
    for entry in entries {
        let path = entry?.path();
        let Some(index) = video_index(&path) else {
            continue;
        };
        let Some(identity) = character_device_identity(&path) else {
            continue;
        };
        candidates.push(LinuxCandidate {
            device: CameraDevice::available(
                CameraSelector::Index(index),
                format!("Camera {index}"),
            ),
            identity,
            class: LinuxSelectorClass::Index,
        });
    }

    candidates.sort_by(|left, right| {
        left.class
            .cmp(&right.class)
            .then_with(|| left.device.selector.cmp(&right.device.selector))
    });
    let configured_candidate = configured.and_then(|selector| {
        candidates
            .iter()
            .find(|candidate| &candidate.device.selector == selector)
            .map(|candidate| candidate.device.clone())
    });
    let mut identities = BTreeSet::new();
    let mut devices = candidates
        .into_iter()
        .filter(|candidate| identities.insert(candidate.identity.clone()))
        .map(|candidate| candidate.device)
        .collect::<Vec<_>>();
    if let Some(candidate) = configured_candidate {
        if !devices
            .iter()
            .any(|device| device.selector == candidate.selector)
        {
            devices.push(candidate);
        }
    } else {
        append_configured_if_missing(&mut devices, configured, false);
    }
    Ok(devices)
}

#[cfg(target_os = "linux")]
fn enumerate_linux_aliases(
    directory: &Path,
    class: LinuxSelectorClass,
    output: &mut Vec<LinuxCandidate>,
) -> io::Result<()> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        let Some(identity) = character_device_identity(&path) else {
            continue;
        };
        let raw = path.to_string_lossy().into_owned();
        let Ok(selector) = CameraSelector::native(raw) else {
            continue;
        };
        let label = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("Camera")
            .to_owned();
        output.push(LinuxCandidate {
            device: CameraDevice::available(selector, label),
            identity,
            class,
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn video_index(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix("video")?.parse().ok()
}

#[cfg(target_os = "linux")]
fn character_device_identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};

    let metadata = path.canonicalize().ok()?.metadata().ok()?;
    metadata
        .file_type()
        .is_char_device()
        .then(|| format!("{}:{}", metadata.dev(), metadata.rdev()))
}

fn append_configured_if_missing(
    devices: &mut Vec<CameraDevice>,
    configured: Option<&CameraSelector>,
    available: bool,
) {
    if let Some(configured) = configured
        && !devices.iter().any(|device| &device.selector == configured)
    {
        let mut device = CameraDevice::unavailable(configured.clone());
        device.available = available;
        devices.push(device);
    }
}

fn selector_label(selector: &CameraSelector) -> String {
    match selector {
        CameraSelector::Index(index) => format!("Camera {index}"),
        CameraSelector::Native(native) => native.clone(),
    }
}

fn nonempty_label(label: String, index: u32) -> String {
    if label.is_empty() {
        format!("Camera {index}")
    } else {
        label
    }
}

/// Invalid raw camera selector.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CameraSelectorError {
    #[error("camera selector must not be empty")]
    Empty,
    #[error("camera selector must not contain NUL")]
    ContainsNul,
}

/// Fixed native camera enumeration failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CameraEnumerationError {
    #[error("native camera enumeration failed")]
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{CameraDevice, CameraSelector, WindowsCameraDescriptor, select_windows_candidates};

    #[test]
    fn selector_wire_type_and_raw_native_spelling_are_preserved() {
        assert_eq!(
            serde_json::from_str::<CameraSelector>("3").unwrap(),
            CameraSelector::Index(3)
        );
        let raw = r"\\?\usb#VID_0001";
        assert_eq!(
            serde_json::from_value::<CameraSelector>(serde_json::json!(raw)).unwrap(),
            CameraSelector::Native(raw.to_owned())
        );
        assert!(serde_json::from_str::<CameraSelector>("-1").is_err());
        assert!(serde_json::from_str::<CameraSelector>("\"\"").is_err());
        assert!(serde_json::from_str::<CameraSelector>("\"bad\\u0000id\"").is_err());
    }

    #[test]
    fn windows_mock_uses_native_id_and_keeps_missing_configured_selector() {
        let configured = CameraSelector::native("missing-native-id").unwrap();
        let devices = select_windows_candidates(
            vec![WindowsCameraDescriptor {
                index: 2,
                native_id: "opaque-native-id".to_owned(),
                label: "Capture Card".to_owned(),
            }],
            Some(&configured),
        );
        assert_eq!(
            devices,
            vec![
                CameraDevice::available(
                    CameraSelector::native("opaque-native-id").unwrap(),
                    "Capture Card"
                ),
                CameraDevice::unavailable(configured),
            ]
        );
    }
}
