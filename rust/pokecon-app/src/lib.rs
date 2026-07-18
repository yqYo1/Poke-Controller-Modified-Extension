//! Top-level process orchestration for web and desktop modes.

use std::io;
use std::net::SocketAddr;

use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownReason, install_os_signal_forwarder,
};
use pokecon_desktop::DesktopLifecycle;
use pokecon_server::BoundServer;
use thiserror::Error;

/// User-interface mode selected for the single application binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiMode {
    /// Start axum without creating a desktop window.
    Web,
    /// Prepare the Tauri lifecycle boundary alongside axum.
    Desktop,
}

/// Startup values required by the phase-two process skeleton.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppOptions {
    /// HTTP listener address.
    pub listen_address: SocketAddr,
    /// Web or desktop lifecycle mode.
    pub ui_mode: UiMode,
    /// Request a clean exit immediately after all startup boundaries are ready.
    pub exit_after_startup: bool,
}

/// Observable result of a clean application run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSummary {
    /// Actual HTTP listener address.
    pub listen_address: SocketAddr,
    /// First accepted shutdown reason.
    pub shutdown_reason: ShutdownReason,
}

/// Failure while binding, serving, or joining the main process tasks.
#[derive(Debug, Error)]
pub enum AppError {
    /// The axum listener could not be bound.
    #[error("failed to bind the axum listener: {0}")]
    Bind(#[source] io::Error),
    /// The axum server returned an I/O error.
    #[error("axum server failed: {0}")]
    Serve(#[source] io::Error),
    /// A managed Tokio task panicked or was cancelled unexpectedly.
    #[error("managed runtime task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// Starts the common runtime and exits through the shutdown coordinator.
///
/// # Errors
///
/// Returns an error if the server cannot bind, serve, or join cleanly.
pub async fn run(options: AppOptions) -> Result<RunSummary, AppError> {
    let context = RuntimeContext::native();
    let shutdown = context.shutdown().clone();
    let server = BoundServer::bind(options.listen_address)
        .await
        .map_err(AppError::Bind)?;
    let listen_address = server.local_addr();
    let signal_task = install_os_signal_forwarder(shutdown.clone()).await;
    let server_task = tokio::spawn(server.serve(shutdown.cancellation_token()));

    let _desktop_lifecycle =
        (options.ui_mode == UiMode::Desktop).then(|| DesktopLifecycle::new(shutdown.clone()));
    tracing::info!(
        diagnostic_id = APP_STARTING,
        ?listen_address,
        ui_mode = ?options.ui_mode,
        platform = ?context.platform().kind(),
        "PokeCon runtime boundaries are ready"
    );

    if options.exit_after_startup {
        shutdown.request(ShutdownReason::StartupProbe);
    }

    let shutdown_reason = shutdown.cancelled().await;
    server_task.await?.map_err(AppError::Serve)?;
    signal_task.await?;
    tracing::info!(
        diagnostic_id = APP_STOPPED,
        ?shutdown_reason,
        "PokeCon stopped cleanly"
    );

    Ok(RunSummary {
        listen_address,
        shutdown_reason,
    })
}
