//! Desktop lifecycle boundary shared by the Tauri shell and headless tests.

use pokecon_core::{ShutdownCoordinator, ShutdownReason};

/// Connects desktop window lifecycle requests to process-wide shutdown.
#[derive(Clone, Debug)]
pub struct DesktopLifecycle {
    shutdown: ShutdownCoordinator,
}

impl DesktopLifecycle {
    /// Creates a desktop lifecycle adapter.
    #[must_use]
    pub const fn new(shutdown: ShutdownCoordinator) -> Self {
        Self { shutdown }
    }

    /// Requests the same complete shutdown used by operating-system signals.
    pub fn request_exit(&self) -> bool {
        self.shutdown.request(ShutdownReason::DesktopExit)
    }
}

/// Installs the Tauri close-request bridge on an application builder.
///
/// The full window construction and close-behavior policy are implemented in
/// phase twelve. This boundary is compiled now so Tauri requests cannot grow a
/// separate shutdown path.
#[cfg(feature = "tauri-shell")]
#[must_use]
pub fn configure_tauri_builder<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    lifecycle: DesktopLifecycle,
) -> tauri::Builder<R> {
    builder.on_window_event(move |_window, event| {
        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            lifecycle.request_exit();
        }
    })
}

#[cfg(test)]
mod tests {
    use pokecon_core::{ShutdownCoordinator, ShutdownReason};

    use super::DesktopLifecycle;

    #[tokio::test]
    async fn desktop_exit_uses_the_common_coordinator() {
        let shutdown = ShutdownCoordinator::new();
        let lifecycle = DesktopLifecycle::new(shutdown.clone());
        assert!(lifecycle.request_exit());
        assert_eq!(shutdown.cancelled().await, ShutdownReason::DesktopExit);
    }
}
