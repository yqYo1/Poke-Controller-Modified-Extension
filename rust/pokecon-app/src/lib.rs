//! Top-level process orchestration for web and desktop modes.

mod application_backend;
pub mod command_service;
pub mod dynamic_host;
pub mod dynamic_runtime;
mod production;
pub mod profile_service;
mod script_host;
pub mod script_runtime;
mod settings_runtime;

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use axum::Router;
use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownCoordinator, ShutdownReason,
    install_os_signal_forwarder,
};
use pokecon_desktop::{DesktopLifecycle, DesktopRuntimeSettings};
use pokecon_server::BoundServer;
use pokecon_server::router::public_router;
use pokecon_server::security::RequestSecurity;
use pokecon_server::static_files::{StaticFiles, StaticRootError};
use pokecon_settings::pipeline::{LoadedSettings, PipelineRequest};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::dynamic_host::StartupDynamicHost;
use crate::dynamic_runtime::DynamicRuntime;
use crate::production::ProductionRuntime;

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOptions {
    /// HTTP listener address.
    pub listen_address: SocketAddr,
    /// Web or desktop lifecycle mode.
    pub ui_mode: UiMode,
    /// Canonical startup-only directory containing the static SPA bundle.
    pub web_root: PathBuf,
    /// Request a clean exit immediately after all startup boundaries are ready.
    pub exit_after_startup: bool,
}

/// Process-wide controls supplied by the native shell or a headless caller.
#[derive(Debug)]
pub struct RunControl {
    shutdown: ShutdownCoordinator,
    ready: Option<SyncSender<SocketAddr>>,
    desktop_settings: Option<DesktopRuntimeSettings>,
}

impl RunControl {
    /// Creates controls that share the supplied first-writer-wins shutdown path.
    #[must_use]
    pub const fn new(shutdown: ShutdownCoordinator) -> Self {
        Self {
            shutdown,
            ready: None,
            desktop_settings: None,
        }
    }

    /// Publishes the actual listener address after all production services are ready.
    #[must_use]
    pub fn with_ready_sender(mut self, ready: SyncSender<SocketAddr>) -> Self {
        self.ready = Some(ready);
        self
    }

    /// Connects runtime-immediate desktop settings to the settings transaction path.
    #[must_use]
    pub fn with_desktop_settings(mut self, settings: DesktopRuntimeSettings) -> Self {
        self.desktop_settings = Some(settings);
        self
    }
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
    /// The configured static SPA root could not be opened safely.
    #[error("failed to open the static SPA root: {0}")]
    Static(#[from] StaticRootError),
    /// The axum listener could not be bound.
    #[error("failed to bind the axum listener: {0}")]
    Bind(#[source] io::Error),
    /// The axum server returned an I/O error.
    #[error("axum server failed: {0}")]
    Serve(#[source] io::Error),
    /// A managed Tokio task panicked or was cancelled unexpectedly.
    #[error("managed runtime task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    /// Production service composition failed before the listener became ready.
    #[error("application runtime initialization failed: {0}")]
    Runtime(String),
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
    let static_files = match StaticFiles::new(&options.web_root) {
        Ok(static_files) => static_files,
        Err(error) => {
            if let Some(runtime) = dynamic.take() {
                runtime.shutdown().await;
            }
            return Err(AppError::Static(error));
        }
    };
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
    let security = RequestSecurity::new(listen_address, options.ui_mode == UiMode::Desktop);
    let server = server.with_router(public_router(Router::new(), static_files, security));
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

/// Starts the fully connected production runtime from the final canonical
/// settings and dynamic-worker bootstrap result.
///
/// # Errors
///
/// Returns an error if service construction, binding, serving, or bounded task
/// shutdown fails.
pub async fn run_configured(
    options: AppOptions,
    request: PipelineRequest,
    loaded: LoadedSettings,
    host: std::sync::Arc<StartupDynamicHost>,
    dynamic: Option<DynamicRuntime>,
) -> Result<RunSummary, AppError> {
    let shutdown = RuntimeContext::native().shutdown().clone();
    run_configured_controlled(
        options,
        request,
        loaded,
        host,
        dynamic,
        RunControl::new(shutdown),
    )
    .await
}

/// Starts the production runtime with controls owned by the colocated desktop
/// shell. This keeps Tauri, operating-system signals, and backend failures on
/// one shutdown coordinator.
///
/// # Errors
///
/// Returns an error if service construction, binding, serving, or bounded task
/// shutdown fails.
pub async fn run_configured_controlled(
    options: AppOptions,
    request: PipelineRequest,
    loaded: LoadedSettings,
    host: std::sync::Arc<StartupDynamicHost>,
    mut dynamic: Option<DynamicRuntime>,
    control: RunControl,
) -> Result<RunSummary, AppError> {
    let RunControl {
        shutdown,
        ready,
        desktop_settings,
    } = control;
    let static_files = match StaticFiles::new(&options.web_root) {
        Ok(static_files) => static_files,
        Err(error) => {
            if let Some(runtime) = dynamic.take() {
                runtime.shutdown().await;
            }
            return Err(AppError::Static(error));
        }
    };
    let dynamic_client = dynamic.as_ref().and_then(DynamicRuntime::command_bridge);
    let mut production = match ProductionRuntime::build(
        request,
        loaded,
        host,
        dynamic_client,
        options.ui_mode,
        desktop_settings,
    )
    .await
    {
        Ok(production) => production,
        Err(error) => {
            if let Some(runtime) = dynamic.take() {
                runtime.shutdown().await;
            }
            return Err(AppError::Runtime(error));
        }
    };
    let context = RuntimeContext::native();
    let signal_task = install_os_signal_forwarder(shutdown.clone()).await;
    let server = match BoundServer::bind(options.listen_address).await {
        Ok(server) => server,
        Err(error) => {
            signal_task.abort();
            let _aborted = signal_task.await;
            shutdown_production(&mut production, dynamic.take()).await;
            return Err(AppError::Bind(error));
        }
    };
    let listen_address = server.local_addr();
    let security = RequestSecurity::new(listen_address, options.ui_mode == UiMode::Desktop);
    let server = server.with_router(public_router(production.router(), static_files, security));
    let server_shutdown = CancellationToken::new();
    let server_task = tokio::spawn(server.serve(server_shutdown.clone()));

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
        "PokeCon production runtime is ready"
    );
    if let Some(ready) = ready
        && ready.send(listen_address).is_err()
    {
        shutdown.request(ShutdownReason::FatalError(
            "desktop shell stopped before backend readiness".to_owned(),
        ));
    }

    if options.exit_after_startup {
        shutdown.request(ShutdownReason::StartupProbe);
    }
    let shutdown_reason = shutdown.cancelled().await;
    shutdown_production(&mut production, dynamic.take()).await;
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

async fn shutdown_production(production: &mut ProductionRuntime, dynamic: Option<DynamicRuntime>) {
    if let Some(runtime) = dynamic.as_ref() {
        runtime.prepare_shutdown().await;
    }
    production.stop_inputs_camera_and_scripts().await;
    if let Some(runtime) = dynamic {
        runtime.shutdown_worker().await;
    }
    production.stop_serial().await;
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
