//! Top-level process orchestration for web and desktop modes.

pub mod command_service;
pub mod dynamic_host;
pub mod dynamic_runtime;
pub mod profile_service;
pub mod script_runtime;

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownReason, install_os_signal_forwarder,
};
use pokecon_desktop::DesktopLifecycle;
use pokecon_server::BoundServer;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::dynamic_runtime::DynamicRuntime;

const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(2);

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
    run_with_dynamic(options, None).await
}

/// Starts the common runtime with an optional persistent dynamic worker and
/// exits through the shutdown coordinator.
///
/// Dynamic startup-post runs only after the server and desktop boundaries are
/// ready. Dynamic shutdown-pre completes or times out before those services
/// are cancelled.
///
/// # Errors
///
/// Returns an error if the server cannot bind, serve, or join cleanly.
pub async fn run_with_dynamic(
    options: AppOptions,
    mut dynamic: Option<DynamicRuntime>,
) -> Result<RunSummary, AppError> {
    let context = RuntimeContext::native();
    let shutdown = context.shutdown().clone();
    let signal_task = install_os_signal_forwarder(shutdown.clone()).await;
    let server = match BoundServer::bind(options.listen_address).await {
        Ok(server) => server,
        Err(error) => {
            signal_task.abort();
            let _aborted = signal_task.await;
            if let Some(runtime) = dynamic.take() {
                runtime.shutdown().await;
            }
            return Err(AppError::Bind(error));
        }
    };
    let listen_address = server.local_addr();
    let server_shutdown = CancellationToken::new();
    let server_task = tokio::spawn(server.serve(server_shutdown.clone()));

    let _desktop_lifecycle =
        (options.ui_mode == UiMode::Desktop).then(|| DesktopLifecycle::new(shutdown.clone()));
    if let Some(runtime) = dynamic.as_ref()
        && let Err(error) = runtime.emit_startup_post().await
    {
        tracing::error!(
            event = "AppStartupPost",
            error = %error,
            "dynamic startup-post event failed"
        );
    }
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
    if let Some(runtime) = dynamic.take() {
        runtime.shutdown().await;
    }
    server_shutdown.cancel();
    finish_server_task(server_task).await?;
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

async fn finish_server_task(mut task: JoinHandle<io::Result<()>>) -> Result<(), AppError> {
    if let Ok(result) = timeout(SERVER_STOP_TIMEOUT, &mut task).await {
        result?.map_err(AppError::Serve)
    } else {
        tracing::error!(
            timeout_ms = SERVER_STOP_TIMEOUT.as_millis(),
            "axum server shutdown timed out; aborting its task"
        );
        task.abort();
        match task.await {
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(AppError::Task(error)),
            Ok(result) => result.map_err(AppError::Serve),
        }
    }
}
