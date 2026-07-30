//! Shared runtime primitives for every `PokeCon` process.

#[path = "../../pokecon/src/diagnostics/mod.rs"]
mod diagnostics;
#[path = "../../pokecon/src/platform/mod.rs"]
pub mod platform;
#[path = "../../pokecon/src/runtime/mod.rs"]
mod runtime;

pub use diagnostics::{
    APP_STARTING, APP_STOPPED, SHUTDOWN_REQUESTED, SIGNAL_HANDLER_FAILED, TracingInitError,
    init_tracing, init_tracing_to_stderr,
};
pub use runtime::{
    OsSignal, RuntimeContext, ShutdownCoordinator, ShutdownReason, install_os_signal_forwarder,
};
