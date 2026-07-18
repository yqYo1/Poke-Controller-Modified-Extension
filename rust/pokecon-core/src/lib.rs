//! Shared runtime primitives for every `PokeCon` process.

mod diagnostics;
pub mod platform;
mod shutdown;

use std::sync::Arc;

pub use diagnostics::{
    APP_STARTING, APP_STOPPED, SHUTDOWN_REQUESTED, SIGNAL_HANDLER_FAILED, TracingInitError,
    init_tracing, init_tracing_to_stderr,
};
pub use shutdown::{OsSignal, ShutdownCoordinator, ShutdownReason, install_os_signal_forwarder};

use crate::platform::PlatformAdapter;

/// Process-wide services shared by the application and worker runtimes.
#[derive(Clone, Debug)]
pub struct RuntimeContext {
    shutdown: ShutdownCoordinator,
    platform: Arc<dyn PlatformAdapter>,
}

impl RuntimeContext {
    /// Creates a context for the native target platform.
    #[must_use]
    pub fn native() -> Self {
        Self {
            shutdown: ShutdownCoordinator::new(),
            platform: Arc::from(platform::native_adapter()),
        }
    }

    /// Returns the shared shutdown coordinator.
    #[must_use]
    pub const fn shutdown(&self) -> &ShutdownCoordinator {
        &self.shutdown
    }

    /// Returns the selected platform adapter.
    #[must_use]
    pub fn platform(&self) -> &dyn PlatformAdapter {
        self.platform.as_ref()
    }
}

impl Default for RuntimeContext {
    fn default() -> Self {
        Self::native()
    }
}
