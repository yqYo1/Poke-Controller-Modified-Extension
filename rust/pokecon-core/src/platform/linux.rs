use super::{PlatformAdapter, PlatformKind};

/// Linux implementation boundary for native services.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxAdapter;

impl PlatformAdapter for LinuxAdapter {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Linux
    }

    fn supports_native_notifications(&self) -> bool {
        true
    }
}
