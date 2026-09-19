use thiserror::Error;
use tracing_subscriber::{
    EnvFilter,
    layer::SubscriberExt,
    util::{SubscriberInitExt, TryInitError},
};

/// Diagnostic emitted when a managed process begins startup.
pub const APP_STARTING: &str = "POKECON-RUNTIME-0001";
/// Diagnostic emitted after a managed process has stopped cleanly.
pub const APP_STOPPED: &str = "POKECON-RUNTIME-0002";
/// Diagnostic emitted when the common shutdown coordinator accepts a request.
pub const SHUTDOWN_REQUESTED: &str = "POKECON-RUNTIME-0003";
/// Diagnostic emitted if an operating-system signal handler cannot be installed.
pub const SIGNAL_HANDLER_FAILED: &str = "POKECON-RUNTIME-0004";

/// Error returned when the global tracing subscriber cannot be installed.
#[derive(Debug, Error)]
#[error("failed to install the global tracing subscriber: {0}")]
pub struct TracingInitError(#[source] TryInitError);

/// Installs structured JSON logging with an environment-filter override.
///
/// # Errors
///
/// Returns an error when another global tracing subscriber was already installed.
pub fn init_tracing(default_filter: &str) -> Result<(), TracingInitError> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json())
        .try_init()
        .map_err(TracingInitError)
}

/// Installs structured JSON logging on stderr.
///
/// Worker stdout is reserved for the framed IPC protocol, so managed worker
/// processes must use this initializer instead of [`init_tracing`].
///
/// # Errors
///
/// Returns an error when another global tracing subscriber was already installed.
pub fn init_tracing_to_stderr(default_filter: &str) -> Result<(), TracingInitError> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr),
        )
        .try_init()
        .map_err(TracingInitError)
}
