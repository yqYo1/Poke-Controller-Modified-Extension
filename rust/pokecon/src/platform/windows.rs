use super::{PlatformAdapter, PlatformKind};

/// Windows implementation boundary for native services.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsAdapter;

impl PlatformAdapter for WindowsAdapter {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn supports_native_notifications(&self) -> bool {
        true
    }
}
