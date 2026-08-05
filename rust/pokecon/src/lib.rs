//! Top-level process orchestration for web and desktop modes.

mod application_backend;
#[path = "camera/facade.rs"]
mod camera;
mod command_service;
#[path = "contracts/facade.rs"]
mod contracts;
mod desktop;
#[path = "device/facade.rs"]
mod device;
#[doc(hidden)]
pub mod diagnostics;
#[allow(
    dead_code,
    reason = "engine-only helpers are consumed by the worker binary's private copy"
)]
#[doc(hidden)]
pub mod dynamic;
pub(crate) use dynamic as dynamic_domain;
mod dynamic_host;
mod dynamic_runtime;
mod entrypoint;
#[doc(hidden)]
pub mod platform;
mod production;
mod profile_service;
#[doc(hidden)]
pub mod runtime;
mod script_host;
mod script_runtime;
#[allow(dead_code, reason = "retained internal server and OpenAPI contracts")]
#[allow(clippy::option_option, reason = "wire patch fields are three-state")]
mod server;
#[path = "settings/facade.rs"]
mod settings;
mod settings_runtime;
#[doc(hidden)]
pub mod worker;

#[doc(hidden)]
pub use diagnostics::{
    APP_STARTING, APP_STOPPED, SHUTDOWN_REQUESTED, SIGNAL_HANDLER_FAILED, TracingInitError,
    init_tracing, init_tracing_to_stderr,
};
pub use entrypoint::{MainError, run_cli};
#[doc(hidden)]
pub use runtime::{
    OsSignal, RuntimeContext, ShutdownCoordinator, ShutdownReason, install_os_signal_forwarder,
};

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use crate::camera::ScreenshotMode;
use crate::desktop::DesktopRuntimeSettings;
use crate::server::BoundServer;
use crate::server::router::public_router;
use crate::server::security::RequestSecurity;
use crate::server::static_files::{StaticFiles, StaticRootError};
use crate::settings::pipeline::{LoadedSettings, PipelineRequest};
use axum::Router;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::dynamic_host::StartupDynamicHost;
use crate::dynamic_runtime::DynamicRuntime;
use crate::production::ProductionRuntime;

/// Builds the canonical `OpenAPI` document for the generator binary.
///
/// # Errors
///
/// Returns an error when the API schema or canonical contract registry cannot
/// be serialized.
#[cfg(feature = "contract-generator")]
#[doc(hidden)]
pub fn generate_openapi_document_json() -> Result<String, Box<dyn std::error::Error>> {
    server::openapi::document_json().map_err(Into::into)
}

const SERVER_STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// User-interface mode selected for the single application binary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UiMode {
    /// Start axum without creating a desktop window.
    Web,
    /// Prepare the Tauri lifecycle boundary alongside axum.
    Desktop,
}

/// Capabilities that differ between the primary Web UI and its desktop adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UiCapabilities {
    /// Whether requests originating from the colocated Tauri `WebView` are accepted.
    allow_tauri_origin: bool,
    /// Whether API destinations may name an absolute native filesystem path.
    screenshot_mode: ScreenshotMode,
}

impl UiMode {
    fn capabilities(self) -> UiCapabilities {
        let capabilities = match self {
            Self::Web => UiCapabilities {
                allow_tauri_origin: false,
                screenshot_mode: ScreenshotMode::Web,
            },
            Self::Desktop => UiCapabilities {
                allow_tauri_origin: true,
                screenshot_mode: ScreenshotMode::Desktop,
            },
        };
        debug_assert_eq!(capabilities.allow_tauri_origin, self == Self::Desktop);
        capabilities
    }
}

/// Composes the Web-primary router shared by browser and desktop display modes.
fn ui_router(
    api: Router,
    static_files: StaticFiles,
    listen_address: SocketAddr,
    ui: UiCapabilities,
) -> Router {
    let security = RequestSecurity::new(listen_address, ui.allow_tauri_origin);
    public_router(api, static_files, security)
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
    /// The axum server stopped without an accepted shutdown request.
    #[error("axum server stopped before shutdown was requested")]
    ServerStopped,
    /// The operating-system signal task stopped without accepting shutdown.
    #[error("operating-system signal task stopped before shutdown was requested")]
    SignalStopped,
    /// A managed Tokio task panicked or was cancelled unexpectedly.
    #[error("managed runtime task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    /// Production service composition failed before the listener became ready.
    #[error("application runtime initialization failed: {0}")]
    Runtime(String),
}

enum EarlyRuntimeTaskError {
    Server(AppError),
    Signal(AppError),
}

struct RuntimeTaskResult {
    early_task_error: Option<EarlyRuntimeTaskError>,
    server_task_consumed: bool,
    signal_task_consumed: bool,
}

struct ServerReadinessRequest {
    prepared: oneshot::Sender<()>,
    published: oneshot::Receiver<()>,
}

struct ServerReadinessPermit {
    published: oneshot::Sender<()>,
}

impl ServerReadinessPermit {
    fn publish(self) {
        let _published = self.published.send(());
    }
}

async fn fail_startup(
    dynamic: &mut Option<DynamicRuntime>,
    error: AppError,
) -> Result<(), AppError> {
    if let Some(runtime) = dynamic.take() {
        runtime.shutdown().await;
    }
    Err(error)
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
    let (shutdown, ready) = (control.shutdown, control.ready);
    let desktop_settings = control.desktop_settings;
    let ui = options.ui_mode.capabilities();
    let static_files = match StaticFiles::new(&options.web_root) {
        Ok(static_files) => static_files,
        Err(error) => return fail_startup(&mut dynamic, AppError::Static(error)).await,
    };
    let dynamic_client = dynamic.as_ref().and_then(DynamicRuntime::command_bridge);
    let mut production = match ProductionRuntime::build(
        request,
        loaded,
        host,
        dynamic_client,
        ui.screenshot_mode,
        desktop_settings,
    )
    .await
    {
        Ok(production) => production,
        Err(error) => return fail_startup(&mut dynamic, AppError::Runtime(error)).await,
    };
    let platform_kind: crate::platform::PlatformKind = RuntimeContext::native().platform().kind();
    let mut signal_task = install_os_signal_forwarder(shutdown.clone()).await;
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
    let app = ui_router(production.router(), static_files, listen_address, ui);
    let server = server.with_router(app);
    let server_shutdown = CancellationToken::new();
    let (server_readiness_sender, server_readiness_receiver) = oneshot::channel();
    let mut server_task = tokio::spawn(serve_with_readiness_barrier(
        server.serve(server_shutdown.clone()),
        server_readiness_receiver,
    ));

    emit_startup_post(dynamic.as_ref()).await;
    let task_result = match prepare_server_readiness(
        &shutdown,
        &mut server_task,
        server_readiness_sender,
    )
    .await
    {
        Ok(readiness_permit) => {
            tracing::info!(
                diagnostic_id = APP_STARTING,
                ?listen_address,
                ui_mode = ?options.ui_mode,
                platform = ?platform_kind,
                "PokeCon production runtime is ready"
            );
            if let Some(ready) = ready
                && ready.send(listen_address).is_err()
            {
                shutdown.request(ShutdownReason::FatalError(
                    "desktop shell stopped before backend readiness".to_owned(),
                ));
            }
            readiness_permit.publish();

            if options.exit_after_startup {
                shutdown.request(ShutdownReason::StartupProbe);
            }
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task).await
        }
        Err(task_result) => task_result,
    };
    let shutdown_reason = shutdown.cancelled().await;
    shutdown_production(&mut production, dynamic.take()).await;
    server_shutdown.cancel();
    let server_result = if task_result.server_task_consumed {
        Ok(())
    } else {
        finish_server_task(server_task).await
    };
    let signal_result = if task_result.signal_task_consumed {
        Ok(())
    } else {
        signal_task.await.map_err(AppError::Task)
    };
    if let Some(error) = task_result.early_task_error {
        return Err(match error {
            EarlyRuntimeTaskError::Server(error) | EarlyRuntimeTaskError::Signal(error) => error,
        });
    }
    server_result?;
    signal_result?;
    tracing::info!(
        diagnostic_id = APP_STOPPED,
        ?shutdown_reason,
        "PokeCon stopped cleanly"
    );
    Ok(())
}

async fn serve_with_readiness_barrier<F>(
    serve: F,
    readiness: oneshot::Receiver<ServerReadinessRequest>,
) -> io::Result<()>
where
    F: Future<Output = io::Result<()>>,
{
    tokio::pin!(serve);
    let request = tokio::select! {
        biased;
        result = &mut serve => return result,
        request = readiness => request,
    };
    let Ok(request) = request else {
        return serve.await;
    };
    if request.prepared.send(()).is_err() {
        return serve.await;
    }
    let _published = request.published.await;
    serve.await
}

async fn prepare_server_readiness(
    shutdown: &ShutdownCoordinator,
    server_task: &mut JoinHandle<io::Result<()>>,
    readiness: oneshot::Sender<ServerReadinessRequest>,
) -> Result<ServerReadinessPermit, RuntimeTaskResult> {
    let (prepared_sender, prepared_receiver) = oneshot::channel();
    let (published_sender, published_receiver) = oneshot::channel();
    let request = ServerReadinessRequest {
        prepared: prepared_sender,
        published: published_receiver,
    };
    if readiness.send(request).is_err() {
        let result = (&mut *server_task).await;
        return Err(completed_server_task_result(shutdown, result));
    }
    if prepared_receiver.await.is_err() {
        let result = (&mut *server_task).await;
        return Err(completed_server_task_result(shutdown, result));
    }
    Ok(ServerReadinessPermit {
        published: published_sender,
    })
}

async fn wait_for_shutdown_or_runtime_task(
    shutdown: &ShutdownCoordinator,
    server_task: &mut JoinHandle<io::Result<()>>,
    signal_task: &mut JoinHandle<()>,
) -> RuntimeTaskResult {
    tokio::select! {
        biased;
        _reason = shutdown.cancelled() => RuntimeTaskResult {
            early_task_error: None,
            server_task_consumed: false,
            signal_task_consumed: false,
        },
        result = &mut *server_task => completed_server_task_result(shutdown, result),
        result = &mut *signal_task => completed_signal_task_result(shutdown, result),
    }
}

fn completed_server_task_result(
    shutdown: &ShutdownCoordinator,
    result: Result<io::Result<()>, tokio::task::JoinError>,
) -> RuntimeTaskResult {
    let error = early_server_task_error(result);
    shutdown.request(ShutdownReason::FatalError(error.to_string()));
    RuntimeTaskResult {
        early_task_error: Some(EarlyRuntimeTaskError::Server(error)),
        server_task_consumed: true,
        signal_task_consumed: false,
    }
}

fn early_server_task_error(result: Result<io::Result<()>, tokio::task::JoinError>) -> AppError {
    match result {
        Ok(Ok(())) => AppError::ServerStopped,
        Ok(Err(error)) => AppError::Serve(error),
        Err(error) => AppError::Task(error),
    }
}

fn completed_signal_task_result(
    shutdown: &ShutdownCoordinator,
    result: Result<(), tokio::task::JoinError>,
) -> RuntimeTaskResult {
    let early_task_error = match result {
        Ok(()) => {
            let error = AppError::SignalStopped;
            let fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            fatal_claimed.then_some(EarlyRuntimeTaskError::Signal(error))
        }
        Err(error) => {
            let error = AppError::Task(error);
            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Some(EarlyRuntimeTaskError::Signal(error))
        }
    };
    RuntimeTaskResult {
        early_task_error,
        server_task_consumed: false,
        signal_task_consumed: true,
    }
}

async fn emit_startup_post(runtime: Option<&DynamicRuntime>) {
    if let Some(runtime) = runtime
        && let Err(error) = runtime.emit_startup_post().await
    {
        tracing::error!(
            event = "AppStartupPost",
            error = %error,
            "dynamic startup-post event failed"
        );
    }
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

#[cfg(test)]
mod tests {
    mod ui_boundary_acceptance;

    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::OsSignal;

    use super::*;

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);

    async fn abort_and_join<T>(task: JoinHandle<T>) {
        task.abort();
        let Err(error) = task.await else {
            panic!("pending test task must be cancelled during cleanup");
        };
        assert!(error.is_cancelled());
    }

    struct DropMarker(Arc<AtomicBool>);

    impl Drop for DropMarker {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    async fn failed_server_readiness<F>(serve: F) -> (RuntimeTaskResult, ShutdownCoordinator)
    where
        F: Future<Output = io::Result<()>> + Send + 'static,
    {
        let shutdown = ShutdownCoordinator::new();
        let (readiness_sender, readiness_receiver) = oneshot::channel();
        let mut server_task = tokio::spawn(serve_with_readiness_barrier(serve, readiness_receiver));

        let preparation = timeout(
            TEST_TIMEOUT,
            prepare_server_readiness(&shutdown, &mut server_task, readiness_sender),
        )
        .await
        .expect("terminal listener task must be observed before the deadline");
        let Err(result) = preparation else {
            panic!("terminal listener task must not grant a readiness permit");
        };
        assert!(result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        (result, shutdown)
    }

    #[tokio::test]
    async fn pre_readiness_listener_io_failure_is_classified_without_a_permit() {
        let (result, shutdown) = failed_server_readiness(async {
            Err(io::Error::other("injected pre-readiness listener failure"))
        })
        .await;

        let Some(EarlyRuntimeTaskError::Server(AppError::Serve(error))) = result.early_task_error
        else {
            panic!("pre-readiness listener I/O failure must retain its typed error");
        };
        assert_eq!(error.to_string(), "injected pre-readiness listener failure");
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "axum server failed: injected pre-readiness listener failure".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn pre_readiness_clean_listener_completion_is_classified_without_a_permit() {
        let (result, shutdown) = failed_server_readiness(async { Ok(()) }).await;

        assert!(matches!(
            result.early_task_error,
            Some(EarlyRuntimeTaskError::Server(AppError::ServerStopped))
        ));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "axum server stopped before shutdown was requested".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn pre_readiness_listener_panic_is_classified_without_a_permit() {
        let (result, shutdown) = failed_server_readiness(async {
            panic!("injected pre-readiness listener panic");
        })
        .await;

        let Some(EarlyRuntimeTaskError::Server(AppError::Task(error))) = result.early_task_error
        else {
            panic!("pre-readiness listener panic must retain its join error");
        };
        assert!(error.is_panic());
        assert!(matches!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(reason))
                if reason.contains("injected pre-readiness listener panic")
        ));
    }

    #[tokio::test]
    async fn pre_readiness_listener_cancellation_is_classified_without_a_permit() {
        let shutdown = ShutdownCoordinator::new();
        let (readiness_sender, readiness_receiver) = oneshot::channel();
        let mut server_task = tokio::spawn(serve_with_readiness_barrier(
            pending::<io::Result<()>>(),
            readiness_receiver,
        ));
        server_task.abort();

        let preparation = timeout(
            TEST_TIMEOUT,
            prepare_server_readiness(&shutdown, &mut server_task, readiness_sender),
        )
        .await
        .expect("listener cancellation must be observed before the deadline");
        let Err(result) = preparation else {
            panic!("cancelled listener task must not grant a readiness permit");
        };
        assert!(result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Server(AppError::Task(error))) = result.early_task_error
        else {
            panic!("pre-readiness listener cancellation must retain its join error");
        };
        assert!(error.is_cancelled());
        assert!(matches!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(reason))
                if reason.starts_with("managed runtime task failed: task ")
                    && reason.ends_with(" was cancelled")
        ));
    }

    #[tokio::test]
    async fn readiness_permit_pauses_terminal_server_until_address_publication() {
        let shutdown = ShutdownCoordinator::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let task_dropped = Arc::clone(&dropped);
        let (serve_polled_sender, serve_polled_receiver) = oneshot::channel();
        let (completion_sender, completion_receiver) = oneshot::channel();
        let serve = async move {
            let _drop_marker = DropMarker(task_dropped);
            let _serve_polled = serve_polled_sender.send(());
            completion_receiver
                .await
                .expect("test controls the listener completion")
        };
        let (readiness_sender, readiness_receiver) = oneshot::channel();
        let mut server_task = tokio::spawn(serve_with_readiness_barrier(serve, readiness_receiver));

        timeout(TEST_TIMEOUT, serve_polled_receiver)
            .await
            .expect("listener must run during the startup-post interval")
            .expect("listener polling observation must remain connected");
        let permit = timeout(
            TEST_TIMEOUT,
            prepare_server_readiness(&shutdown, &mut server_task, readiness_sender),
        )
        .await
        .expect("live listener must prepare readiness before the deadline")
        .unwrap_or_else(|_| panic!("live listener must grant a readiness permit"));

        completion_sender
            .send(Ok(()))
            .expect("paused listener completion receiver must remain alive");
        tokio::task::yield_now().await;
        assert!(!server_task.is_finished());
        assert!(!dropped.load(Ordering::Acquire));

        permit.publish();
        timeout(TEST_TIMEOUT, &mut server_task)
            .await
            .expect("published listener must resume before the deadline")
            .expect("listener task must not panic")
            .expect("injected listener completion must remain successful");
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn listener_io_failure_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task =
            tokio::spawn(async { Err(io::Error::other("injected listener failure")) });
        let mut signal_task = tokio::spawn(pending::<()>());

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("listener failure must be observed before the deadline");

        assert!(result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Server(AppError::Serve(error))) = result.early_task_error
        else {
            panic!("listener I/O failure must retain its typed error");
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "injected listener failure");
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "axum server failed: injected listener failure".to_owned()
            ))
        );
        abort_and_join(signal_task).await;
    }

    #[tokio::test]
    async fn unexpected_clean_listener_completion_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task = tokio::spawn(async { Ok(()) });
        let mut signal_task = tokio::spawn(pending::<()>());

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("listener completion must be observed before the deadline");

        assert!(matches!(
            result.early_task_error,
            Some(EarlyRuntimeTaskError::Server(AppError::ServerStopped))
        ));
        assert!(result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "axum server stopped before shutdown was requested".to_owned()
            ))
        );
        abort_and_join(signal_task).await;
    }

    #[tokio::test]
    async fn panicked_listener_task_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task: JoinHandle<io::Result<()>> =
            tokio::spawn(async { panic!("injected listener panic") });
        let mut signal_task = tokio::spawn(pending::<()>());

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("listener panic must be observed before the deadline");

        assert!(result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Server(AppError::Task(error))) = result.early_task_error
        else {
            panic!("listener panic must retain its join error");
        };
        assert!(error.is_panic());
        let Some(ShutdownReason::FatalError(reason)) = shutdown.reason() else {
            panic!("listener panic must request fatal shutdown");
        };
        assert!(reason.contains("injected listener panic"));
        abort_and_join(signal_task).await;
    }

    #[tokio::test]
    async fn cancelled_listener_task_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        server_task.abort();
        let mut signal_task = tokio::spawn(pending::<()>());

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("listener cancellation must be observed before the deadline");

        assert!(result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Server(AppError::Task(error))) = result.early_task_error
        else {
            panic!("listener cancellation must retain its join error");
        };
        assert!(error.is_cancelled());
        assert!(matches!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(reason))
                if reason.starts_with("managed runtime task failed: task ")
                    && reason.ends_with(" was cancelled")
        ));
        abort_and_join(signal_task).await;
    }

    #[tokio::test]
    async fn unexpected_clean_signal_task_completion_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        let mut signal_task = tokio::spawn(async {});

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("signal task completion must be observed before the deadline");

        assert!(matches!(
            result.early_task_error,
            Some(EarlyRuntimeTaskError::Signal(AppError::SignalStopped))
        ));
        assert!(!result.server_task_consumed);
        assert!(result.signal_task_consumed);
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "operating-system signal task stopped before shutdown was requested".to_owned()
            ))
        );
        abort_and_join(server_task).await;
    }

    #[tokio::test]
    async fn panicked_signal_task_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        let mut signal_task = tokio::spawn(async { panic!("injected signal task panic") });

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("signal task panic must be observed before the deadline");

        assert!(!result.server_task_consumed);
        assert!(result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Signal(AppError::Task(error))) = result.early_task_error
        else {
            panic!("signal task panic must retain its join error");
        };
        assert!(error.is_panic());
        let Some(ShutdownReason::FatalError(reason)) = shutdown.reason() else {
            panic!("signal task panic must request fatal shutdown");
        };
        assert!(reason.contains("injected signal task panic"));
        abort_and_join(server_task).await;
    }

    #[tokio::test]
    async fn cancelled_signal_task_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        let mut signal_task = tokio::spawn(pending::<()>());
        signal_task.abort();

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("signal task cancellation must be observed before the deadline");

        assert!(!result.server_task_consumed);
        assert!(result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Signal(AppError::Task(error))) = result.early_task_error
        else {
            panic!("signal task cancellation must retain its join error");
        };
        assert!(error.is_cancelled());
        assert!(matches!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(reason))
                if reason.starts_with("managed runtime task failed: task ")
                    && reason.ends_with(" was cancelled")
        ));
        abort_and_join(server_task).await;
    }

    #[tokio::test]
    async fn self_coordinating_signal_task_completion_remains_clean() {
        let shutdown = ShutdownCoordinator::new();
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        let signal_shutdown = shutdown.clone();
        let mut signal_task = tokio::spawn(async move {
            assert!(signal_shutdown.request(ShutdownReason::Signal(OsSignal::Interrupt)));
        });

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("coordinated signal completion must be observed before the deadline");

        assert!(result.early_task_error.is_none());
        assert!(!result.server_task_consumed);
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::Signal(OsSignal::Interrupt))
        );
        if !result.signal_task_consumed {
            signal_task
                .await
                .expect("unconsumed coordinated signal task must retain its clean join result");
        }
        abort_and_join(server_task).await;
    }

    #[tokio::test]
    async fn preaccepted_shutdown_wins_over_completed_signal_task() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        let mut signal_task = tokio::spawn(async {});

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("accepted shutdown must win before the deadline");

        assert!(result.early_task_error.is_none());
        assert!(!result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
        signal_task
            .await
            .expect("biased shutdown branch must leave the signal handle joinable");
        abort_and_join(server_task).await;
    }

    #[tokio::test]
    async fn completed_signal_task_after_preaccepted_shutdown_is_consumed_cleanly() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let signal_result = tokio::spawn(async {}).await;

        let result = completed_signal_task_result(&shutdown, signal_result);

        assert!(result.early_task_error.is_none());
        assert!(!result.server_task_consumed);
        assert!(result.signal_task_consumed);
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[tokio::test]
    async fn cancelled_signal_task_after_preaccepted_shutdown_retains_typed_error() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let signal_task = tokio::spawn(pending::<()>());
        signal_task.abort();
        let signal_result = signal_task.await;

        let result = completed_signal_task_result(&shutdown, signal_result);

        assert!(!result.server_task_consumed);
        assert!(result.signal_task_consumed);
        let Some(EarlyRuntimeTaskError::Signal(AppError::Task(error))) = result.early_task_error
        else {
            panic!("signal task cancellation must retain its join error");
        };
        assert!(error.is_cancelled());
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[tokio::test]
    async fn preaccepted_shutdown_does_not_create_listener_fatal_error() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let mut server_task = tokio::spawn(pending::<io::Result<()>>());
        let mut signal_task = tokio::spawn(pending::<()>());

        let result = timeout(
            TEST_TIMEOUT,
            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task),
        )
        .await
        .expect("accepted shutdown must win before the deadline");

        assert!(result.early_task_error.is_none());
        assert!(!result.server_task_consumed);
        assert!(!result.signal_task_consumed);
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
        abort_and_join(server_task).await;
        abort_and_join(signal_task).await;
    }
}
