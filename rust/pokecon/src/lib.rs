//! Top-level process orchestration for web and desktop modes.

mod application_backend;
#[path = "camera/facade.rs"]
mod camera;
mod command_service;
#[path = "contracts/facade.rs"]
mod contracts;
#[path = "desktop/facade.rs"]
mod desktop;
#[path = "device/facade.rs"]
mod device;
#[path = "diagnostics/facade.rs"]
mod diagnostics;
#[path = "dynamic/facade.rs"]
mod dynamic;
mod dynamic_host;
mod dynamic_runtime;
mod entrypoint;
#[path = "platform/facade.rs"]
mod platform;
mod production;
mod profile_service;
#[path = "runtime/facade.rs"]
mod runtime;
mod script_host;
mod script_runtime;
#[path = "server/facade.rs"]
mod server;
#[path = "settings/facade.rs"]
mod settings;
mod settings_runtime;
#[path = "worker/facade.rs"]
mod worker;

pub use entrypoint::{MainError, run_cli};

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use crate::desktop::DesktopRuntimeSettings;
use crate::server::BoundServer;
use crate::server::router::public_router;
use crate::server::security::RequestSecurity;
use crate::server::static_files::{StaticFiles, StaticRootError};
use crate::settings::pipeline::{LoadedSettings, PipelineRequest};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::diagnostics::{APP_STARTING, APP_STOPPED};
use crate::dynamic_host::StartupDynamicHost;
use crate::dynamic_runtime::DynamicRuntime;
use crate::production::ProductionRuntime;
use crate::runtime::{
    RuntimeContext, ShutdownCoordinator, ShutdownReason, install_os_signal_forwarder,
};

const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// User-interface mode selected for the single application binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UiMode {
    /// Start axum without creating a desktop window.
    Web,
    /// Prepare the Tauri lifecycle boundary alongside axum.
    Desktop,
}

/// Startup values required by the phase-two process skeleton.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AppOptions {
    /// HTTP listener address.
    listen_address: SocketAddr,
    /// Web or desktop lifecycle mode.
    ui_mode: UiMode,
    /// Canonical startup-only directory containing the static SPA bundle.
    web_root: PathBuf,
    /// Request a clean exit immediately after all startup boundaries are ready.
    exit_after_startup: bool,
}

/// Process-wide controls supplied by the native shell or a headless caller.
#[derive(Debug)]
struct RunControl {
    shutdown: ShutdownCoordinator,
    ready: Option<SyncSender<SocketAddr>>,
    desktop_settings: Option<DesktopRuntimeSettings>,
}

impl RunControl {
    /// Creates controls that share the supplied first-writer-wins shutdown path.
    #[must_use]
    const fn new(shutdown: ShutdownCoordinator) -> Self {
        Self {
            shutdown,
            ready: None,
            desktop_settings: None,
        }
    }

    /// Publishes the actual listener address after all production services are ready.
    #[must_use]
    fn with_ready_sender(mut self, ready: SyncSender<SocketAddr>) -> Self {
        self.ready = Some(ready);
        self
    }

    /// Connects runtime-immediate desktop settings to the settings transaction path.
    #[must_use]
    fn with_desktop_settings(mut self, settings: DesktopRuntimeSettings) -> Self {
        self.desktop_settings = Some(settings);
        self
    }
}

/// Failure while binding, serving, or joining the main process tasks.
#[derive(Debug, Error)]
enum AppError {
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

/// Starts the production runtime with controls owned by the colocated desktop
/// shell. This keeps Tauri, operating-system signals, and backend failures on
/// one shutdown coordinator.
///
/// # Errors
///
/// Returns an error if service construction, binding, serving, or bounded task
/// shutdown fails.
async fn run_configured_controlled(
    options: AppOptions,
    request: PipelineRequest,
    loaded: LoadedSettings,
    host: std::sync::Arc<StartupDynamicHost>,
    mut dynamic: Option<DynamicRuntime>,
    control: RunControl,
) -> Result<(), AppError> {
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
    let platform: &dyn crate::platform::PlatformAdapter = context.platform();
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
        platform = ?platform.kind(),
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
    Ok(())
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
