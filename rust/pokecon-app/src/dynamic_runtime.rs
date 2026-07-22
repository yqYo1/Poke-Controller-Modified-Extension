//! Application-owned lifecycle for the persistent dynamic configuration
//! worker.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pokecon_dynamic::protocol::{DynamicInitializeRequest, PYTHON_SITE_PACKAGES_ENV};
use pokecon_dynamic::{DynamicConfigLanguage, DynamicHostError};
use pokecon_settings::package::PythonWorker;
use pokecon_settings::pipeline::{LoadedSettings, PipelineError, PipelineRequest, SettingSource};
use pokecon_settings::roots::RootEnvironment;
use pokecon_settings::uv::{ManagedUv, ManagedUvSource, UvChildEnvironment, UvError};
use pokecon_settings::venv::{
    CommandUvExecutor, VenvError, VenvManager, VenvOwnership, VenvPreparationRequest,
};
use pokecon_worker::WorkerKind;
use pokecon_worker::dynamic::{DynamicClientError, DynamicWorkerClient};
use pokecon_worker::ipc::{LogLevel, LogPayload};
use pokecon_worker::supervisor::{
    ManagedWorker, StopPurpose, SupervisorError, WorkerLaunch, WorkerSupervisor,
};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::dynamic_host::StartupDynamicHost;

const DYNAMIC_EVENT_TIMEOUT: Duration = Duration::from_secs(2);
const DYNAMIC_STOP_TIMEOUT: Duration = Duration::from_secs(2);
const RECEIVER_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
pub(crate) const PYTHON_VERSION: &str = "3.14";

pub(crate) const SAFE_WORKER_ENVIRONMENT: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "TMPDIR",
    "TEMP",
    "TMP",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "RUST_LOG",
];

/// Startup result after the dynamic layer has either committed or failed
/// softly back to the ordinary settings pipeline.
#[derive(Debug)]
pub struct DynamicBootstrap {
    pub loaded: LoadedSettings,
    pub host: Arc<StartupDynamicHost>,
    pub runtime: Option<DynamicRuntime>,
    pub startup_failure: Option<DynamicStartupError>,
}

/// Secret-safe failure while preparing or initializing the optional dynamic
/// runtime.
#[derive(Debug, Error)]
pub enum DynamicStartupError {
    #[error("packaged application is missing its pinned managed uv executable")]
    MissingManagedUv,
    #[error("packaged application is missing its CPython executable: {0}")]
    MissingPython(PathBuf),
    #[error("packaged application is missing its dynamic worker executable: {0}")]
    MissingWorker(PathBuf),
    #[error("prepared dynamic Python environment has no site-packages directory: {0}")]
    MissingSitePackages(PathBuf),
    #[error("current executable is unavailable: {0}")]
    CurrentExecutable(#[source] io::Error),
    #[error("current executable has no parent directory")]
    MissingExecutableParent,
    #[error(transparent)]
    Settings(#[from] PipelineError),
    #[error(transparent)]
    Uv(#[from] UvError),
    #[error(transparent)]
    Venv(#[from] VenvError),
    #[error(transparent)]
    Host(#[from] DynamicHostError),
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
    #[error(transparent)]
    Client(#[from] DynamicClientError),
}

/// Live handles for the sole persistent dynamic worker generation.
pub struct DynamicRuntime {
    _supervisor: Arc<WorkerSupervisor>,
    worker: Arc<ManagedWorker>,
    host: Arc<StartupDynamicHost>,
    client: Option<Arc<DynamicWorkerClient>>,
    log_task: Option<JoinHandle<()>>,
    diagnostic_task: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for DynamicRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DynamicRuntime")
            .field("worker", &self.worker)
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

impl DynamicRuntime {
    /// Returns the Rust-owned state/settings host shared with command and
    /// profile services.
    #[must_use]
    pub fn host(&self) -> Arc<StartupDynamicHost> {
        Arc::clone(&self.host)
    }

    /// Returns the persistent worker as the command callback/event bridge.
    #[must_use]
    pub fn command_bridge(&self) -> Option<Arc<DynamicWorkerClient>> {
        self.client.clone()
    }

    /// Emits the non-cancellable event after the server and desktop boundaries
    /// are ready.
    ///
    /// # Errors
    ///
    /// Returns a transport or dynamic-engine error from the worker.
    pub async fn emit_startup_post(&self) -> Result<(), DynamicClientError> {
        let Some(client) = self.client.as_ref() else {
            return Ok(());
        };
        let result = client.emit("AppStartupPost").await?;
        if result.cancelled {
            tracing::warn!(
                event = "AppStartupPost",
                "non-cancellable dynamic event reported cancellation"
            );
        }
        Ok(())
    }

    /// Runs the shutdown-pre event while application services remain alive,
    /// then closes host mutations and releases dynamic controller ownership.
    pub async fn prepare_shutdown(&self) {
        if let Some(client) = self.client.as_ref() {
            match timeout(DYNAMIC_EVENT_TIMEOUT, client.emit("AppShutdownPre")).await {
                Ok(Ok(result)) if result.cancelled => tracing::warn!(
                    event = "AppShutdownPre",
                    "non-cancellable dynamic event reported cancellation"
                ),
                Ok(Ok(_result)) => {}
                Ok(Err(error)) => tracing::error!(
                    event = "AppShutdownPre",
                    error = %error,
                    "dynamic shutdown-pre event failed"
                ),
                Err(_) => tracing::error!(
                    event = "AppShutdownPre",
                    timeout_ms = DYNAMIC_EVENT_TIMEOUT.as_millis(),
                    "dynamic shutdown-pre event timed out"
                ),
            }
        }

        self.host.begin_stopping();
    }

    /// Reaps the already-stopping dynamic worker by the fixed deadline.
    pub async fn shutdown_worker(mut self) {
        log_stop_result(
            self.worker
                .stop(StopPurpose::ApplicationShutdown, DYNAMIC_STOP_TIMEOUT)
                .await,
        );
        finish_worker_receivers(
            &self.worker,
            self.client.take(),
            self.log_task.take(),
            self.diagnostic_task.take(),
        )
        .await;
    }

    /// Performs both shutdown phases for startup-failure and legacy callers.
    pub async fn shutdown(self) {
        self.prepare_shutdown().await;
        self.shutdown_worker().await;
    }
}

/// Resolves settings with an optional dynamic layer. Dynamic preparation and
/// evaluation are fail-soft: a canonical static snapshot is returned whenever
/// the full non-dynamic pipeline remains valid.
///
/// # Errors
///
/// Returns a settings error when the selected language is invalid or the
/// static fallback cannot be resolved.
pub async fn bootstrap_dynamic(
    request: PipelineRequest,
    before_dynamic: LoadedSettings,
) -> Result<DynamicBootstrap, PipelineError> {
    let Some(primary) = selected_language(&before_dynamic)? else {
        let host = Arc::new(
            StartupDynamicHost::new(request, before_dynamic)
                .map_err(|error| host_pipeline_error(&error))?,
        );
        let loaded = host
            .finish_startup()
            .map_err(|error| host_pipeline_error(&error))?;
        return Ok(DynamicBootstrap {
            loaded,
            host,
            runtime: None,
            startup_failure: None,
        });
    };

    match start_dynamic(request.clone(), before_dynamic.clone(), primary).await {
        Ok((loaded, runtime)) => {
            let host = runtime.host();
            Ok(DynamicBootstrap {
                loaded,
                host,
                runtime: Some(runtime),
                startup_failure: None,
            })
        }
        Err(error) => {
            let host = Arc::new(
                StartupDynamicHost::new(request, before_dynamic)
                    .map_err(|error| host_pipeline_error(&error))?,
            );
            let loaded = host
                .finish_startup()
                .map_err(|error| host_pipeline_error(&error))?;
            Ok(DynamicBootstrap {
                loaded,
                host,
                runtime: None,
                startup_failure: Some(error),
            })
        }
    }
}

fn host_pipeline_error(error: &DynamicHostError) -> PipelineError {
    PipelineError::CrossSetting(format!(
        "runtime host initialization failed ({})",
        error.code
    ))
}

fn selected_language(
    before_dynamic: &LoadedSettings,
) -> Result<Option<DynamicConfigLanguage>, PipelineError> {
    let value = before_dynamic.settings.string("dynamic_config_language")?;
    if value.eq_ignore_ascii_case("python") {
        Ok(Some(DynamicConfigLanguage::Python))
    } else if value.eq_ignore_ascii_case("lua") {
        Ok(Some(DynamicConfigLanguage::Lua))
    } else if value.eq_ignore_ascii_case("none") {
        Ok(None)
    } else {
        Err(PipelineError::TypedLookup(
            "dynamic_config_language".to_owned(),
        ))
    }
}

async fn start_dynamic(
    request: PipelineRequest,
    before_dynamic: LoadedSettings,
    primary: DynamicConfigLanguage,
) -> Result<(LoadedSettings, DynamicRuntime), DynamicStartupError> {
    let prepared = prepare_dynamic_environment(&request, &before_dynamic).await?;
    let host = Arc::new(StartupDynamicHost::new(
        request.clone(),
        before_dynamic.clone(),
    )?);
    let supervisor = Arc::new(WorkerSupervisor::new());
    let launch = python_worker_launch(
        prepared.worker_program,
        WorkerKind::Dynamic,
        &request.environment,
        &before_dynamic.roots.config,
        &prepared.venv,
        &prepared.site_packages,
        PYTHON_SITE_PACKAGES_ENV,
    );
    let worker = supervisor.spawn(launch, host.controller_safety()).await?;
    let diagnostic_task = worker.take_diagnostics().map(spawn_diagnostic_receiver);
    let client = match DynamicWorkerClient::attach(worker.clone(), host.clone()) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            stop_failed_startup(&worker, &host, None, None, diagnostic_task).await;
            return Err(error.into());
        }
    };
    let log_task = client.take_logs().map(spawn_log_receiver);

    let initialized = match client
        .initialize(&DynamicInitializeRequest {
            config_root: before_dynamic.roots.config.clone(),
            home: dynamic_home(&request.environment),
            primary,
        })
        .await
    {
        Ok(initialized) => initialized,
        Err(error) => {
            stop_failed_startup(&worker, &host, Some(client), log_task, diagnostic_task).await;
            return Err(error.into());
        }
    };
    if let Some(load) = initialized.startup_load.as_ref()
        && !load.loaded
    {
        tracing::warn!(
            path = %load.display_path,
            language = ?load.language,
            "dynamic startup configuration was rejected"
        );
    }

    let loaded = match host.finish_startup() {
        Ok(loaded) => loaded,
        Err(error) => {
            stop_failed_startup(&worker, &host, Some(client), log_task, diagnostic_task).await;
            return Err(error.into());
        }
    };
    Ok((
        loaded,
        DynamicRuntime {
            _supervisor: supervisor,
            worker,
            host,
            client: Some(client),
            log_task,
            diagnostic_task,
        },
    ))
}

struct PreparedDynamicEnvironment {
    worker_program: PathBuf,
    venv: PathBuf,
    site_packages: PathBuf,
}

async fn prepare_dynamic_environment(
    request: &PipelineRequest,
    before_dynamic: &LoadedSettings,
) -> Result<PreparedDynamicEnvironment, DynamicStartupError> {
    let python = packaged_python()?;
    let worker_program = packaged_worker()?;
    let managed_uv_source =
        ManagedUvSource::bundled()?.ok_or(DynamicStartupError::MissingManagedUv)?;
    let managed_uv = ManagedUv::prepare(&before_dynamic.roots, &managed_uv_source)?;
    let venv = PathBuf::from(before_dynamic.settings.string("python.dynamic.venv")?);
    let ownership = match before_dynamic
        .settings
        .get("python.dynamic.venv")
        .ok_or_else(|| PipelineError::TypedLookup("python.dynamic.venv".to_owned()))?
        .source
    {
        SettingSource::Default => VenvOwnership::AppManaged,
        _ => VenvOwnership::UserSpecified,
    };
    let uv_config = optional_path_setting(before_dynamic, "python.dynamic.packages.uv_config")?;
    let packages = before_dynamic
        .settings
        .resolve_packages(PythonWorker::Dynamic)?;
    let uv_environment = UvChildEnvironment::build(&request.environment)?;
    let preparation = VenvPreparationRequest {
        worker: PythonWorker::Dynamic,
        venv: venv.clone(),
        ownership,
        managed_uv,
        python: python.clone(),
        python_build_id: format!("cpython-{PYTHON_VERSION}:{}", python.display()),
        packages,
        override_application_constraints: before_dynamic
            .settings
            .boolean("python.dynamic.packages.override_application_constraints")?,
        override_package_metadata_constraints: before_dynamic
            .settings
            .boolean("python.dynamic.packages.override_package_metadata_constraints")?,
        uv_config,
        uv_environment,
        revalidate_mutable_sources: before_dynamic
            .settings
            .boolean("python.dynamic.packages.revalidate_mutable_sources")?,
        application_build_id: format!("pokecon-{}", env!("CARGO_PKG_VERSION")),
        kernel_id: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
    };
    let manager = VenvManager::new(before_dynamic.roots.clone(), Arc::new(CommandUvExecutor));
    let preparation_result = manager.prepare(preparation).await?;
    tracing::info!(
        worker_kind = "dynamic",
        disposition = ?preparation_result.disposition,
        "dynamic Python environment is synchronized"
    );

    let site_packages = python_site_packages(&venv);
    if !site_packages.is_dir() {
        return Err(DynamicStartupError::MissingSitePackages(site_packages));
    }

    Ok(PreparedDynamicEnvironment {
        worker_program,
        venv,
        site_packages,
    })
}

pub(crate) fn packaged_python() -> Result<PathBuf, DynamicStartupError> {
    let path = option_env!("PYO3_PYTHON")
        .filter(|value| !value.is_empty())
        .map_or_else(PathBuf::new, PathBuf::from);
    if path.is_file() {
        Ok(path)
    } else {
        Err(DynamicStartupError::MissingPython(path))
    }
}

pub(crate) fn packaged_worker() -> Result<PathBuf, DynamicStartupError> {
    let executable = std::env::current_exe().map_err(DynamicStartupError::CurrentExecutable)?;
    let directory = executable
        .parent()
        .ok_or(DynamicStartupError::MissingExecutableParent)?;
    let worker = directory.join(format!("pokecon-worker{}", std::env::consts::EXE_SUFFIX));
    if worker.is_file() {
        Ok(worker)
    } else {
        Err(DynamicStartupError::MissingWorker(worker))
    }
}

pub(crate) fn optional_path_setting(
    loaded: &LoadedSettings,
    id: &str,
) -> Result<Option<PathBuf>, PipelineError> {
    match &loaded
        .settings
        .get(id)
        .ok_or_else(|| PipelineError::TypedLookup(id.to_owned()))?
        .value
    {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(path) => Ok(Some(PathBuf::from(path))),
        _ => Err(PipelineError::TypedLookup(id.to_owned())),
    }
}

pub(crate) fn python_site_packages(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Lib").join("site-packages")
    } else {
        venv.join("lib")
            .join(format!("python{PYTHON_VERSION}"))
            .join("site-packages")
    }
}

fn dynamic_home(environment: &RootEnvironment) -> Option<PathBuf> {
    ["HOME", "USERPROFILE"].into_iter().find_map(|name| {
        environment
            .get(name)
            .filter(|value| value.to_str().is_some())
            .map(PathBuf::from)
    })
}

pub(crate) fn python_worker_launch(
    worker_program: PathBuf,
    kind: WorkerKind,
    environment: &RootEnvironment,
    working_directory: &Path,
    venv: &Path,
    site_packages: &Path,
    site_packages_environment: &str,
) -> WorkerLaunch {
    let mut launch = WorkerLaunch::managed(worker_program, kind)
        .clear_environment()
        .current_directory(working_directory)
        .environment("VIRTUAL_ENV", venv)
        .environment("PYTHONNOUSERSITE", "1")
        .environment("PYTHONUTF8", "1")
        .environment(site_packages_environment, site_packages);
    for name in SAFE_WORKER_ENVIRONMENT {
        if let Some(value) = environment.get(name) {
            launch = launch.environment(OsStr::new(name), value);
        }
    }
    launch
}

fn spawn_log_receiver(mut receiver: tokio::sync::mpsc::Receiver<LogPayload>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(payload) = receiver.recv().await {
            log_dynamic_payload(&payload);
        }
    })
}

fn log_dynamic_payload(payload: &LogPayload) {
    match payload.level {
        LogLevel::Debug => tracing::debug!(
            target: "pokecon.dynamic",
            log_target = ?payload.target,
            message = %payload.message,
            "dynamic worker output"
        ),
        LogLevel::Info => tracing::info!(
            target: "pokecon.dynamic",
            log_target = ?payload.target,
            message = %payload.message,
            "dynamic worker output"
        ),
        LogLevel::Warning => tracing::warn!(
            target: "pokecon.dynamic",
            log_target = ?payload.target,
            message = %payload.message,
            "dynamic worker output"
        ),
        LogLevel::Error | LogLevel::Critical => tracing::error!(
            target: "pokecon.dynamic",
            log_target = ?payload.target,
            message = %payload.message,
            "dynamic worker output"
        ),
    }
}

fn spawn_diagnostic_receiver(
    mut receiver: tokio::sync::mpsc::Receiver<pokecon_worker::supervisor::OobDiagnostic>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(diagnostic) = receiver.recv().await {
            tracing::debug!(
                worker_kind = "dynamic",
                byte_count = diagnostic.bytes.len(),
                "captured out-of-band worker diagnostic"
            );
        }
    })
}

async fn stop_failed_startup(
    worker: &Arc<ManagedWorker>,
    host: &StartupDynamicHost,
    client: Option<Arc<DynamicWorkerClient>>,
    log_task: Option<JoinHandle<()>>,
    diagnostic_task: Option<JoinHandle<()>>,
) {
    host.begin_stopping();
    log_stop_result(
        worker
            .stop(StopPurpose::ApplicationShutdown, DYNAMIC_STOP_TIMEOUT)
            .await,
    );
    finish_worker_receivers(worker, client, log_task, diagnostic_task).await;
}

fn log_stop_result(result: Result<pokecon_worker::supervisor::StopReport, SupervisorError>) {
    match result {
        Ok(report) if report.forced => tracing::warn!(
            worker_kind = "dynamic",
            cooperative_acknowledged = report.cooperative_acknowledged,
            exit_success = report.exit.success,
            exit_code = report.exit.code,
            "dynamic worker required forced termination"
        ),
        Ok(report) => tracing::info!(
            worker_kind = "dynamic",
            cooperative_acknowledged = report.cooperative_acknowledged,
            exit_success = report.exit.success,
            exit_code = report.exit.code,
            "dynamic worker stopped"
        ),
        Err(error) => tracing::error!(
            worker_kind = "dynamic",
            error = %error,
            "dynamic worker could not be stopped cleanly"
        ),
    }
}

async fn finish_worker_receivers(
    worker: &ManagedWorker,
    client: Option<Arc<DynamicWorkerClient>>,
    log_task: Option<JoinHandle<()>>,
    diagnostic_task: Option<JoinHandle<()>>,
) {
    let dropped_logs = client
        .as_ref()
        .map_or(0, |client| client.dropped_log_count());
    let dropped_diagnostics = worker.dropped_diagnostic_count();
    drop(client);
    finish_receiver_task(log_task).await;
    finish_receiver_task(diagnostic_task).await;
    if dropped_logs > 0 || dropped_diagnostics > 0 {
        tracing::warn!(
            worker_kind = "dynamic",
            dropped_logs,
            dropped_diagnostics,
            "bounded worker diagnostic queues dropped messages"
        );
    }
}

async fn finish_receiver_task(task: Option<JoinHandle<()>>) {
    let Some(mut task) = task else {
        return;
    };
    if timeout(RECEIVER_DRAIN_TIMEOUT, &mut task).await.is_err() {
        task.abort();
        let _result = task.await;
    }
}
