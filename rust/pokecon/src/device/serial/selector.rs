use std::collections::BTreeSet;
use std::io;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// User-supplied serial selector retained byte-for-byte as UTF-8 text.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PortSelector(String);

impl PortSelector {
    /// Builds a raw selector without case, slash, symlink, or COM normalization.
    ///
    /// # Errors
    ///
    /// Rejects only an empty selector.
    pub fn new(raw: impl Into<String>) -> Result<Self, SelectorError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(SelectorError::Empty);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Linux selector stability class, in preferred display order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SelectorClass {
    #[cfg(any(target_os = "linux", test))]
    ById,
    #[cfg(any(target_os = "linux", test))]
    ByPath,
    Direct,
}

/// One enumerated spelling and the underlying character-device identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortCandidate {
    pub selector: PortSelector,
    pub canonical_device_id: String,
    pub class: SelectorClass,
}

impl PortCandidate {
    #[must_use]
    pub fn new(
        selector: PortSelector,
        canonical_device_id: impl Into<String>,
        class: SelectorClass,
    ) -> Self {
        Self {
            selector,
            canonical_device_id: canonical_device_id.into(),
            class,
        }
    }
}

/// Deduplicates aliases by device identity, preferring by-id, then by-path,
/// then a direct tty spelling. The result is stable across enumeration order.
#[must_use]
#[cfg(any(target_os = "linux", test))]
pub fn select_linux_candidates(mut candidates: Vec<PortCandidate>) -> Vec<PortCandidate> {
    candidates.sort_by(|left, right| {
        left.class
            .cmp(&right.class)
            .then_with(|| left.selector.cmp(&right.selector))
    });
    let mut identities = BTreeSet::new();
    candidates.retain(|candidate| identities.insert(candidate.canonical_device_id.clone()));
    candidates
}

/// Resolves only an exact configured spelling. A missing selection stays
/// unavailable and never falls through to another detected port.
#[must_use]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "configured-port selection is exercised by serial unit tests"
    )
)]
pub fn choose_configured<'a>(
    configured: &PortSelector,
    candidates: &'a [PortCandidate],
) -> Option<&'a PortCandidate> {
    candidates
        .iter()
        .find(|candidate| &candidate.selector == configured)
}

/// Enumerates selectors for the current OS while preserving their raw names.
///
/// # Errors
///
/// Returns an I/O error if the platform serial enumerator fails.
pub fn enumerate_native_ports() -> io::Result<Vec<PortCandidate>> {
    #[cfg(target_os = "linux")]
    {
        enumerate_linux(Path::new("/dev"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        tokio_serial::available_ports()
            .map(|ports| {
                ports
                    .into_iter()
                    .filter_map(|port| {
                        PortSelector::new(port.port_name.clone())
                            .ok()
                            .map(|selector| {
                                PortCandidate::new(selector, port.port_name, SelectorClass::Direct)
                            })
                    })
                    .collect()
            })
            .map_err(io::Error::other)
    }
}

#[cfg(target_os = "linux")]
fn enumerate_linux(dev_root: &Path) -> io::Result<Vec<PortCandidate>> {
    let mut candidates = Vec::new();
    enumerate_alias_directory(
        &dev_root.join("serial/by-id"),
        SelectorClass::ById,
        &mut candidates,
    )?;
    enumerate_alias_directory(
        &dev_root.join("serial/by-path"),
        SelectorClass::ByPath,
        &mut candidates,
    )?;
    for port in tokio_serial::available_ports().map_err(io::Error::other)? {
        let path = PathBuf::from(&port.port_name);
        if let (Ok(selector), Some(identity)) =
            (PortSelector::new(port.port_name), device_identity(&path))
        {
            candidates.push(PortCandidate::new(
                selector,
                identity,
                SelectorClass::Direct,
            ));
        }
    }
    Ok(select_linux_candidates(candidates))
}

#[cfg(target_os = "linux")]
fn enumerate_alias_directory(
    directory: &Path,
    class: SelectorClass,
    output: &mut Vec<PortCandidate>,
) -> io::Result<()> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if let Some(identity) = device_identity(&path)
            && let Ok(selector) = PortSelector::new(path.to_string_lossy().into_owned())
        {
            output.push(PortCandidate::new(selector, identity, class));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn device_identity(path: &Path) -> Option<String> {
    use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};

    let canonical = path.canonicalize().ok()?;
    let metadata = canonical.metadata().ok()?;
    metadata
        .file_type()
        .is_char_device()
        .then(|| format!("{}:{}", metadata.dev(), metadata.rdev()))
}

/// Invalid raw selector.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SelectorError {
    #[error("serial selector must not be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::{
        PortCandidate, PortSelector, SelectorClass, choose_configured, select_linux_candidates,
    };

    fn candidate(raw: &str, identity: &str, class: SelectorClass) -> PortCandidate {
        PortCandidate::new(PortSelector::new(raw).unwrap(), identity.to_owned(), class)
    }

    #[test]
    fn linux_dedup_prefers_stable_selector_classes() {
        let selected = select_linux_candidates(vec![
            candidate("/dev/ttyACM0", "1:2", SelectorClass::Direct),
            candidate("/dev/serial/by-path/pci-1", "1:2", SelectorClass::ByPath),
            candidate("/dev/serial/by-id/controller", "1:2", SelectorClass::ById),
            candidate("/dev/ttyUSB0", "1:3", SelectorClass::Direct),
        ]);
        assert_eq!(selected.len(), 2);
        assert_eq!(
            selected[0].selector.as_str(),
            "/dev/serial/by-id/controller"
        );
    }

    #[test]
    fn configured_value_is_exact_and_never_auto_replaced() {
        let candidates = vec![candidate("COM7", "COM7", SelectorClass::Direct)];
        let configured = PortSelector::new("com7").unwrap();
        assert!(choose_configured(&configured, &candidates).is_none());
        assert_eq!(configured.as_str(), "com7");
    }
}
