//! Platform-specific behavior is isolated behind this module.

mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::fmt::Debug;

pub use linux::LinuxAdapter;
#[cfg(target_os = "windows")]
pub use windows::WindowsAdapter;

/// Supported native target families.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformKind {
    /// Linux desktop or server.
    Linux,
    /// Windows desktop or server.
    #[cfg(target_os = "windows")]
    Windows,
    /// A build-only target outside the supported runtime matrix.
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    Unsupported,
}

/// Boundary for behavior that differs between Linux and Windows.
pub trait PlatformAdapter: Debug + Send + Sync {
    /// Returns the adapter's platform family.
    fn kind(&self) -> PlatformKind;

    /// Indicates whether native desktop notifications can be implemented.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "notification capability is asserted by target tests"
        )
    )]
    fn supports_native_notifications(&self) -> bool;
}

/// Selects the adapter for the current compilation target.
#[must_use]
pub fn native_adapter() -> Box<dyn PlatformAdapter> {
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxAdapter)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsAdapter)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Box::new(UnsupportedAdapter)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
#[derive(Debug)]
struct UnsupportedAdapter;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl PlatformAdapter for UnsupportedAdapter {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Unsupported
    }

    fn supports_native_notifications(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{PlatformKind, native_adapter};

    #[test]
    fn native_adapter_matches_the_build_target() {
        let adapter = native_adapter();
        #[cfg(target_os = "linux")]
        {
            assert_eq!(adapter.kind(), PlatformKind::Linux);
            assert!(!adapter.supports_native_notifications());
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(adapter.kind(), PlatformKind::Windows);
            assert!(adapter.supports_native_notifications());
        }
    }
}
