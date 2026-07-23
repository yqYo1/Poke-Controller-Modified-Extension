//! Production adapter for profile-scoped user-script venv and worker
//! lifecycle.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pokecon_settings::package::PythonWorker;
use pokecon_settings::pipeline::{LoadedSettings, PipelineRequest, SettingSource};
use pokecon_settings::python::{ManagedPython, prepare_managed_python};
use pokecon_settings::uv::{
    ManagedUv, ManagedUvSource, PackagedWheelhouse, UvChildEnvironment, packaged_wheelhouse,
};
use pokecon_settings::venv::{
    CommandUvExecutor, VenvManager, VenvOwnership, VenvPreparationRequest,
};
use pokecon_worker::WorkerKind;
use pokecon_worker::ipc::{LogLevel, LogPayload, ResourceSafety};
use pokecon_worker::script::protocol::{
    PYTHON_SITE_PACKAGES_ENV, ScriptDiscoveryResult, ScriptExecuteRequest, ScriptExecutionResult,
    ScriptInitializeRequest, ScriptPauseResult, ScriptPointerEvent, ScriptStopResult,
    ScriptTkEvent,
};
use pokecon_worker::script::{ScriptHost, ScriptWorkerClient};
use pokecon_worker::supervisor::{ManagedWorker, StopPurpose, WorkerSupervisor};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::command_service::{
    CommandBackendError, ScriptSessionStop, UserScriptFactory, UserScriptSession,
};
use crate::dynamic_runtime::{
    build_python, optional_path_setting, packaged_worker, python_site_packages,
    python_worker_launch,
};

const FAILED_STARTUP_STOP_TIMEOUT: Duration = Duration::from_secs(2);
const RECEIVER_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);

/// Rust-main host and transport-loss cleanup created afresh for each script
/// worker generation.
pub struct ScriptGenerationResources {
    pub host: Arc<dyn ScriptHost>,
    pub safety: Arc<dyn ResourceSafety>,
}

impl std::fmt::Debug for ScriptGenerationResources {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptGenerationResources")
            .finish_non_exhaustive()
    }
}

/// Creates profile/generation-specific host resources.
pub trait ScriptGenerationHostFactory: Send + Sync {
    /// # Errors
    ///
    /// Returns a stable backend error when Rust-owned resources cannot be
    /// allocated for the generation.
    fn create(
        &self,
        settings: &LoadedSettings,
    ) -> Result<ScriptGenerationResources, CommandBackendError>;
}

/// Exact-sync worker factory used by [`crate::command_service::CommandService`].
pub struct ManagedUserScriptFactory {
    request: PipelineRequest,
    command_root: PathBuf,
    venvs: VenvManager,
    supervisor: Arc<WorkerSupervisor>,
    hosts: Arc<dyn ScriptGenerationHostFactory>,
}

impl std::fmt::Debug for ManagedUserScriptFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedUserScriptFactory")
            .field("command_root", &self.command_root)
            .field("venvs", &self.venvs)
            .field("supervisor", &self.supervisor)
            .finish_non_exhaustive()
    }
}

impl ManagedUserScriptFactory {
    #[must_use]
    pub fn new(
        request: PipelineRequest,
        initial_settings: &LoadedSettings,
        command_root: PathBuf,
        hosts: Arc<dyn ScriptGenerationHostFactory>,
    ) -> Self {
        Self {
            request,
            command_root,
            venvs: VenvManager::new(initial_settings.roots.clone(), Arc::new(CommandUvExecutor)),
            supervisor: Arc::new(WorkerSupervisor::new()),
            hosts,
        }
    }

    #[must_use]
    pub fn supervisor(&self) -> Arc<WorkerSupervisor> {
        Arc::clone(&self.supervisor)
    }

    async fn prepare(
        &self,
        settings: &LoadedSettings,
    ) -> Result<PreparedScriptEnvironment, CommandBackendError> {
        if !self.command_root.is_dir() {
            return Err(CommandBackendError::new(
                "CommandRootUnavailable",
                "user command root is not a directory",
            ));
        }
        let worker_program =
            packaged_worker(&self.request.resource_root).map_err(runtime_environment_error)?;
        let managed_uv_source = ManagedUvSource::bundled_at(&self.request.resource_root)
            .map_err(runtime_environment_error)?
            .ok_or_else(|| {
                CommandBackendError::new(
                    "MissingManagedUv",
                    "packaged application is missing its managed uv executable",
                )
            })?;
        let managed_uv = ManagedUv::prepare(&settings.roots, &managed_uv_source)
            .map_err(runtime_environment_error)?;
        let base_uv_environment = UvChildEnvironment::build(&self.request.environment)
            .map_err(runtime_environment_error)?;
        let managed_python = prepare_managed_python(
            &self.request.resource_root,
            &settings.roots,
            &managed_uv,
            &base_uv_environment,
            build_python().as_deref(),
        )
        .await
        .map_err(runtime_environment_error)?;
        let wheelhouse =
            packaged_wheelhouse(&self.request.resource_root).map_err(runtime_environment_error)?;
        let (venv, preparation) = script_venv_preparation(
            settings,
            managed_uv,
            managed_python,
            wheelhouse,
            base_uv_environment,
        )?;
        let result = self
            .venvs
            .prepare(preparation)
            .await
            .map_err(runtime_environment_error)?;
        tracing::info!(
            worker_kind = "script",
            profile = settings.active_profile.as_str(),
            disposition = ?result.disposition,
            "user-script Python environment is synchronized"
        );
        let site_packages = python_site_packages(&venv);
        if !site_packages.is_dir() {
            return Err(CommandBackendError::new(
                "MissingSitePackages",
                "prepared user-script environment has no site-packages directory",
            ));
        }
        Ok(PreparedScriptEnvironment {
            worker_program,
            venv,
            site_packages,
        })
    }
}

fn script_venv_preparation(
    settings: &LoadedSettings,
    managed_uv: ManagedUv,
    managed_python: ManagedPython,
    wheelhouse: Option<PackagedWheelhouse>,
    base_uv_environment: UvChildEnvironment,
) -> Result<(PathBuf, VenvPreparationRequest), CommandBackendError> {
    let venv = PathBuf::from(
        settings
            .settings
            .string("python.script.venv")
            .map_err(runtime_environment_error)?,
    );
    let ownership = match settings
        .settings
        .get("python.script.venv")
        .ok_or_else(|| CommandBackendError::new("InvalidSetting", "python.script.venv is missing"))?
        .source
    {
        SettingSource::Default => VenvOwnership::AppManaged,
        _ => VenvOwnership::UserSpecified,
    };
    let packages = settings
        .settings
        .resolve_packages(PythonWorker::Script)
        .map_err(runtime_environment_error)?;
    let package_defaults_only = settings
        .settings
        .package_sources("python.script.packages.list")
        .is_empty();
    let offline_wheelhouse = wheelhouse.is_some() && package_defaults_only;
    let preparation = VenvPreparationRequest {
        worker: PythonWorker::Script,
        venv: venv.clone(),
        ownership,
        managed_uv,
        python: managed_python.path,
        python_build_id: managed_python.build_id,
        packages,
        override_application_constraints: setting_bool(
            settings,
            "python.script.packages.override_application_constraints",
        )?,
        override_package_metadata_constraints: setting_bool(
            settings,
            "python.script.packages.override_package_metadata_constraints",
        )?,
        uv_config: optional_path_setting(settings, "python.script.packages.uv_config")
            .map_err(runtime_environment_error)?,
        find_links: wheelhouse.as_ref().map(|source| source.path.clone()),
        no_index: offline_wheelhouse,
        package_source_build_id: wheelhouse.map(|source| source.content_sha256),
        uv_environment: base_uv_environment.with_offline_default(offline_wheelhouse),
        revalidate_mutable_sources: setting_bool(
            settings,
            "python.script.packages.revalidate_mutable_sources",
        )?,
        application_build_id: format!("pokecon-{}", env!("CARGO_PKG_VERSION")),
        kernel_id: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
    };
    Ok((venv, preparation))
}

#[async_trait]
impl UserScriptFactory for ManagedUserScriptFactory {
    async fn spawn(
        &self,
        settings: LoadedSettings,
    ) -> Result<Arc<dyn UserScriptSession>, CommandBackendError> {
        let prepared = self.prepare(&settings).await?;
        let resources = self.hosts.create(&settings)?;
        let launch = python_worker_launch(
            prepared.worker_program,
            WorkerKind::Script,
            &self.request.environment,
            &self.command_root,
            &prepared.venv,
            &prepared.site_packages,
            PYTHON_SITE_PACKAGES_ENV,
        );
        let worker = self
            .supervisor
            .spawn(launch, resources.safety)
            .await
            .map_err(runtime_environment_error)?;
        let diagnostic_task = worker.take_diagnostics().map(spawn_diagnostic_receiver);
        let client = match ScriptWorkerClient::attach(worker.clone(), resources.host) {
            Ok(client) => Arc::new(client),
            Err(error) => {
                let _stop = worker
                    .stop(StopPurpose::ProfileSwitch, FAILED_STARTUP_STOP_TIMEOUT)
                    .await;
                finish_receiver(diagnostic_task).await;
                return Err(runtime_environment_error(error));
            }
        };
        let log_task = client.take_logs().map(spawn_log_receiver);
        let session = Arc::new(ManagedUserScriptSession {
            worker,
            client,
            log_task: Mutex::new(log_task),
            diagnostic_task: Mutex::new(diagnostic_task),
        });
        let initialize = session
            .client
            .initialize(&ScriptInitializeRequest {
                profile: settings.active_profile.as_str().to_owned(),
                command_root: self.command_root.clone(),
                data_root: settings.roots.data.clone(),
            })
            .await;
        if let Err(error) = initialize {
            session.begin_stopping();
            let _stop = session.shutdown(FAILED_STARTUP_STOP_TIMEOUT).await;
            return Err(runtime_environment_error(error));
        }
        Ok(session)
    }
}

struct PreparedScriptEnvironment {
    worker_program: PathBuf,
    venv: PathBuf,
    site_packages: PathBuf,
}

struct ManagedUserScriptSession {
    worker: Arc<ManagedWorker>,
    client: Arc<ScriptWorkerClient>,
    log_task: Mutex<Option<JoinHandle<()>>>,
    diagnostic_task: Mutex<Option<JoinHandle<()>>>,
}

#[async_trait]
impl UserScriptSession for ManagedUserScriptSession {
    async fn discover(&self) -> Result<ScriptDiscoveryResult, CommandBackendError> {
        self.client
            .discover()
            .await
            .map_err(runtime_environment_error)
    }

    async fn execute(
        &self,
        request: ScriptExecuteRequest,
    ) -> Result<ScriptExecutionResult, CommandBackendError> {
        self.client
            .execute(&request)
            .await
            .map_err(runtime_environment_error)
    }

    async fn pause(&self) -> Result<ScriptPauseResult, CommandBackendError> {
        self.client.pause().await.map_err(runtime_environment_error)
    }

    async fn resume(&self) -> Result<ScriptPauseResult, CommandBackendError> {
        self.client
            .resume()
            .await
            .map_err(runtime_environment_error)
    }

    async fn stop_command(&self) -> Result<ScriptStopResult, CommandBackendError> {
        self.client.stop().await.map_err(runtime_environment_error)
    }

    async fn tk_event(&self, event: &ScriptTkEvent) -> Result<(), CommandBackendError> {
        self.client
            .tk_event(event)
            .await
            .map_err(runtime_environment_error)
    }

    async fn pointer_event(&self, event: &ScriptPointerEvent) -> Result<(), CommandBackendError> {
        self.client
            .pointer_event(event)
            .await
            .map_err(runtime_environment_error)
    }

    async fn request_profile_stop(&self) -> Result<(), CommandBackendError> {
        self.client
            .request_profile_stop()
            .await
            .map_err(runtime_environment_error)
    }

    fn begin_stopping(&self) {
        self.worker.generation().begin_stopping();
        self.worker.connection().force_release_resources();
    }

    async fn shutdown(&self, deadline: Duration) -> Result<ScriptSessionStop, CommandBackendError> {
        let report = self
            .worker
            .stop(StopPurpose::ProfileSwitch, deadline)
            .await
            .map_err(runtime_environment_error)?;
        finish_receiver(take_task(&self.log_task)).await;
        finish_receiver(take_task(&self.diagnostic_task)).await;
        Ok(ScriptSessionStop {
            forced: report.forced,
        })
    }
}

fn setting_bool(settings: &LoadedSettings, id: &str) -> Result<bool, CommandBackendError> {
    settings
        .settings
        .boolean(id)
        .map_err(runtime_environment_error)
}

fn runtime_environment_error(error: impl std::fmt::Display) -> CommandBackendError {
    CommandBackendError::new("ScriptWorkerUnavailable", error.to_string())
}

fn take_task(task: &Mutex<Option<JoinHandle<()>>>) -> Option<JoinHandle<()>> {
    task.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

fn spawn_log_receiver(mut receiver: tokio::sync::mpsc::Receiver<LogPayload>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(payload) = receiver.recv().await {
            match payload.level {
                LogLevel::Debug => tracing::debug!(
                    target: "pokecon.script",
                    log_target = ?payload.target,
                    message = %payload.message,
                    "user-script worker output"
                ),
                LogLevel::Info => tracing::info!(
                    target: "pokecon.script",
                    log_target = ?payload.target,
                    message = %payload.message,
                    "user-script worker output"
                ),
                LogLevel::Warning => tracing::warn!(
                    target: "pokecon.script",
                    log_target = ?payload.target,
                    message = %payload.message,
                    "user-script worker output"
                ),
                LogLevel::Error | LogLevel::Critical => tracing::error!(
                    target: "pokecon.script",
                    log_target = ?payload.target,
                    message = %payload.message,
                    "user-script worker output"
                ),
            }
        }
    })
}

fn spawn_diagnostic_receiver(
    mut receiver: tokio::sync::mpsc::Receiver<pokecon_worker::supervisor::OobDiagnostic>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(diagnostic) = receiver.recv().await {
            tracing::debug!(
                worker_kind = "script",
                byte_count = diagnostic.bytes.len(),
                "captured out-of-band user-script diagnostic"
            );
        }
    })
}

async fn finish_receiver(task: Option<JoinHandle<()>>) {
    let Some(mut task) = task else {
        return;
    };
    if timeout(RECEIVER_DRAIN_TIMEOUT, &mut task).await.is_err() {
        task.abort();
        let _result = task.await;
    }
}
