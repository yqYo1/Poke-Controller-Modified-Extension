use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::JoinHandle;

use pokecon_camera::{
    BgrFrame, CaptureResolution, LatestFrameSource, RingReader, ScreenshotFormat, ScreenshotMode,
    ScreenshotRuntimeSettings, ScreenshotService, SharedFrameRing,
};
use pokecon_contracts::PROTOCOL_REGISTRY_JSON;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule, PyModuleMethods};
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::ipc::{ConnectionError, IpcConnection, deserialize_value, serialize_value};

use super::protocol::{
    self, HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostSerialWriteRequest, HostSerialWriteRowRequest, HostTkRequest, HostTkResult,
    ScriptCommandKind, ScriptControl, ScriptDiscoveredCommand, ScriptExecutionOutcome,
    ScriptExecutionResult, ScriptInputAction, ScriptOutputMode, ScriptOutputTarget,
    ScriptPointerEvent, ScriptTkEvent, ScriptWorkerStatus,
};

#[derive(Clone, Debug)]
pub(super) struct PythonActorError {
    pub(super) code: &'static str,
    pub(super) message: String,
}

impl PythonActorError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone)]
pub(super) struct PythonActorConfig {
    pub(super) profile: String,
    pub(super) command_root: PathBuf,
    pub(super) data_root: PathBuf,
    pub(super) connection: IpcConnection,
    pub(super) runtime_handle: Handle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum StopCause {
    None = 0,
    Requested = 1,
    Finish = 2,
}

impl StopCause {
    fn decode(value: u8) -> Self {
        match value {
            1 => Self::Requested,
            2 => Self::Finish,
            _ => Self::None,
        }
    }
}

#[derive(Debug)]
struct ExecutionState {
    next_id: AtomicU64,
    active_id: AtomicU64,
    alive: AtomicBool,
    stop_cause: AtomicU8,
    paused: AtomicBool,
    pause_gate: Mutex<()>,
    resumed: Condvar,
}

impl ExecutionState {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            active_id: AtomicU64::new(0),
            alive: AtomicBool::new(false),
            stop_cause: AtomicU8::new(StopCause::None as u8),
            paused: AtomicBool::new(false),
            pause_gate: Mutex::new(()),
            resumed: Condvar::new(),
        }
    }

    fn lock_pause_gate(&self) -> std::sync::MutexGuard<'_, ()> {
        self.pause_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn clear_pause(&self) {
        let _gate = self.lock_pause_gate();
        self.paused.store(false, Ordering::Release);
        self.resumed.notify_all();
    }

    fn begin(&self) -> Result<u64, PythonActorError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if id == 0 {
            return Err(PythonActorError::new(
                "ExecutionIdExhausted",
                "script execution identity space is exhausted",
            ));
        }
        self.active_id
            .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|active| {
                PythonActorError::new(
                    "ScriptBusy",
                    format!("script execution {active} is already running"),
                )
            })?;
        self.stop_cause
            .store(StopCause::None as u8, Ordering::Release);
        self.clear_pause();
        self.alive.store(true, Ordering::Release);
        Ok(id)
    }

    fn complete(&self, id: u64) {
        self.alive.store(false, Ordering::Release);
        let _result = self
            .active_id
            .compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire);
        self.stop_cause
            .store(StopCause::None as u8, Ordering::Release);
        self.clear_pause();
    }

    fn request_stop(&self) -> bool {
        if self.active_id.load(Ordering::Acquire) == 0 {
            return false;
        }
        let _result = self.stop_cause.compare_exchange(
            StopCause::None as u8,
            StopCause::Requested as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.alive.store(false, Ordering::Release);
        self.clear_pause();
        true
    }

    fn request_finish(&self) {
        if self.active_id.load(Ordering::Acquire) != 0 {
            self.stop_cause
                .store(StopCause::Finish as u8, Ordering::Release);
            self.alive.store(false, Ordering::Release);
            self.clear_pause();
        }
    }

    fn pause(&self) -> bool {
        if self.active_id.load(Ordering::Acquire) == 0 {
            return false;
        }
        let _gate = self.lock_pause_gate();
        !self.paused.swap(true, Ordering::AcqRel)
    }

    fn resume(&self) -> bool {
        let _gate = self.lock_pause_gate();
        let changed = self.paused.swap(false, Ordering::AcqRel);
        if changed {
            self.resumed.notify_all();
        }
        changed
    }

    fn checkpoint(&self) -> bool {
        if !self.paused.load(Ordering::Acquire) {
            return self.alive.load(Ordering::Acquire);
        }
        let mut gate = self.lock_pause_gate();
        while self.paused.load(Ordering::Acquire) && self.alive.load(Ordering::Acquire) {
            gate = self
                .resumed
                .wait(gate)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        self.alive.load(Ordering::Acquire)
    }

    fn status(&self, profile: &str) -> ScriptWorkerStatus {
        let active = self.active_id.load(Ordering::Acquire);
        ScriptWorkerStatus {
            initialized: true,
            profile: Some(profile.to_owned()),
            running: active != 0,
            paused: active != 0 && self.paused.load(Ordering::Acquire),
            execution_id: (active != 0).then_some(active),
        }
    }

    fn cause(&self) -> StopCause {
        StopCause::decode(self.stop_cause.load(Ordering::Acquire))
    }
}

struct ExecuteCommand {
    id: u64,
    path: PathBuf,
    class_name: String,
    tags: Vec<String>,
    response: oneshot::Sender<ScriptExecutionResult>,
}

#[derive(Debug)]
pub(super) struct DiscoverSource {
    pub(super) path: PathBuf,
    pub(super) relative_path: PathBuf,
    pub(super) module_path: String,
    pub(super) automatic_tags: Vec<String>,
}

struct DiscoverCommand {
    sources: Vec<DiscoverSource>,
    response: oneshot::Sender<Result<Vec<ScriptDiscoveredCommand>, PythonActorError>>,
}

enum ActorCommand {
    Discover(DiscoverCommand),
    Execute(ExecuteCommand),
    Shutdown(oneshot::Sender<()>),
}

pub(super) struct PythonActor {
    profile: String,
    state: Arc<ExecutionState>,
    commands: Option<mpsc::Sender<ActorCommand>>,
    thread: Option<JoinHandle<()>>,
}

impl PythonActor {
    pub(super) async fn spawn(config: PythonActorConfig) -> Result<Self, PythonActorError> {
        let profile = config.profile.clone();
        let state = Arc::new(ExecutionState::new());
        let actor_state = state.clone();
        let (command_sender, command_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name(format!("pokecon-script-{profile}"))
            .spawn(move || actor_main(config, actor_state, command_receiver, ready_sender))
            .map_err(|error| PythonActorError::new("ScriptThreadStartFailed", error.to_string()))?;
        match ready_receiver.await {
            Ok(Ok(())) => Ok(Self {
                profile,
                state,
                commands: Some(command_sender),
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _join_result = thread.join();
                Err(error)
            }
            Err(error) => {
                let _join_result = thread.join();
                Err(PythonActorError::new(
                    "ScriptThreadStartFailed",
                    error.to_string(),
                ))
            }
        }
    }

    pub(super) fn status(&self) -> ScriptWorkerStatus {
        self.state.status(&self.profile)
    }

    pub(super) fn execute(
        &self,
        path: PathBuf,
        class_name: String,
        tags: Vec<String>,
    ) -> Result<(u64, oneshot::Receiver<ScriptExecutionResult>), PythonActorError> {
        let id = self.state.begin()?;
        let (response_sender, response_receiver) = oneshot::channel();
        let command = ActorCommand::Execute(ExecuteCommand {
            id,
            path,
            class_name,
            tags,
            response: response_sender,
        });
        if self
            .commands
            .as_ref()
            .is_none_or(|commands| commands.send(command).is_err())
        {
            self.state.complete(id);
            return Err(PythonActorError::new(
                "ScriptThreadStopped",
                "script execution thread is unavailable",
            ));
        }
        Ok((id, response_receiver))
    }

    pub(super) async fn discover(
        &self,
        sources: Vec<DiscoverSource>,
    ) -> Result<Vec<ScriptDiscoveredCommand>, PythonActorError> {
        if self.state.active_id.load(Ordering::Acquire) != 0 {
            return Err(PythonActorError::new(
                "ScriptBusy",
                "commands cannot be discovered while a script is running",
            ));
        }
        let (response, result) = oneshot::channel();
        self.commands
            .as_ref()
            .ok_or_else(|| {
                PythonActorError::new(
                    "ScriptThreadStopped",
                    "script execution thread is unavailable",
                )
            })?
            .send(ActorCommand::Discover(DiscoverCommand {
                sources,
                response,
            }))
            .map_err(|_| {
                PythonActorError::new(
                    "ScriptThreadStopped",
                    "script execution thread is unavailable",
                )
            })?;
        result
            .await
            .map_err(|error| PythonActorError::new("ScriptThreadStopped", error.to_string()))?
    }

    pub(super) fn pause(&self) -> bool {
        self.state.pause()
    }

    pub(super) fn resume(&self) -> bool {
        self.state.resume()
    }

    pub(super) fn request_stop(&self) -> bool {
        self.state.request_stop()
    }

    pub(super) fn tk_event(&self, event: &ScriptTkEvent) -> Result<(), PythonActorError> {
        if self.commands.is_none() {
            return Err(PythonActorError::new(
                "ScriptThreadStopped",
                "script execution thread is unavailable",
            ));
        }
        let encoded = serde_json::to_string(event)
            .map_err(|error| PythonActorError::new("TkEventError", error.to_string()))?;
        Python::attach(|py| -> PyResult<()> {
            py.import("_pokecon_script")?
                .getattr("_tk_event")?
                .call1((encoded,))?;
            Ok(())
        })
        .map_err(|error| PythonActorError::new("TkEventError", error.to_string()))
    }

    pub(super) fn pointer_event(&self, event: &ScriptPointerEvent) -> Result<(), PythonActorError> {
        if self.commands.is_none() {
            return Err(PythonActorError::new(
                "ScriptThreadStopped",
                "script execution thread is unavailable",
            ));
        }
        let encoded = serde_json::to_string(event)
            .map_err(|error| PythonActorError::new("PointerEventError", error.to_string()))?;
        Python::attach(|py| -> PyResult<()> {
            py.import("_pokecon_script")?
                .getattr("_pointer_event")?
                .call1((encoded,))?;
            Ok(())
        })
        .map_err(|error| PythonActorError::new("PointerEventError", error.to_string()))
    }

    pub(super) async fn shutdown(&mut self) {
        self.state.request_stop();
        if let Some(commands) = self.commands.take() {
            let (sender, receiver) = oneshot::channel();
            if commands.send(ActorCommand::Shutdown(sender)).is_ok() {
                let _result = receiver.await;
            }
        }
        if let Some(thread) = self.thread.take() {
            let _result = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}

impl Drop for PythonActor {
    fn drop(&mut self) {
        self.state.request_stop();
        self.commands.take();
    }
}

fn actor_main(
    config: PythonActorConfig,
    state: Arc<ExecutionState>,
    commands: mpsc::Receiver<ActorCommand>,
    ready: oneshot::Sender<Result<(), PythonActorError>>,
) {
    PythonThread {
        config,
        state,
        commands,
    }
    .run(ready);
}

struct PythonThread {
    config: PythonActorConfig,
    state: Arc<ExecutionState>,
    commands: mpsc::Receiver<ActorCommand>,
}

impl PythonThread {
    fn run(self, ready: oneshot::Sender<Result<(), PythonActorError>>) {
        if let Err(error) = initialize_python(&self.config, self.state.clone()) {
            let _result = ready.send(Err(error));
            return;
        }
        if ready.send(Ok(())).is_err() {
            return;
        }
        while let Ok(command) = self.commands.recv() {
            match command {
                ActorCommand::Discover(command) => {
                    let result = discover_commands(&command.sources);
                    let _result = command.response.send(result);
                }
                ActorCommand::Execute(command) => {
                    execute_command(&self.config, &self.state, command);
                }
                ActorCommand::Shutdown(response) => {
                    let _result = response.send(());
                    break;
                }
            }
        }
        shutdown_python();
    }
}

fn shutdown_python() {
    Python::attach(|py| {
        if let Ok(module) = py.import("_pokecon_script") {
            let _result = module
                .getattr("_shutdown_tk")
                .and_then(|function| function.call0());
        }
    });
}

fn initialize_python(
    config: &PythonActorConfig,
    state: Arc<ExecutionState>,
) -> Result<(), PythonActorError> {
    let camera = request_host::<_, HostCameraInitializeResult>(
        &config.connection,
        &config.runtime_handle,
        protocol::HOST_CAMERA_INITIALIZE,
        &(),
    )
    .map_err(|error| PythonActorError::new("CameraInitializeError", error))?;
    let camera_reader = camera
        .mapping
        .map(SharedFrameRing::open)
        .transpose()
        .map_err(|error| PythonActorError::new("CameraMappingError", error.to_string()))?
        .map(RingReader::new);
    let screenshot = ScreenshotService::new(
        LatestFrameSource::default(),
        &config.data_root,
        ScreenshotMode::Web,
        ScreenshotRuntimeSettings::default(),
    );
    Python::initialize();
    Python::attach(|py| -> PyResult<()> {
        add_python_paths(py, &config.command_root)?;
        let module = PyModule::new(py, "_pokecon_script")?;
        module.add(
            "_api",
            Py::new(
                py,
                PyApi {
                    connection: config.connection.clone(),
                    runtime_handle: config.runtime_handle.clone(),
                    state,
                    profile: config.profile.clone(),
                    command_root: config.command_root.clone(),
                    data_root: config.data_root.clone(),
                    camera_reader,
                    screenshot,
                },
            )?,
        )?;
        let bootstrap = CString::new(PYTHON_BOOTSTRAP)
            .map_err(|_| PyValueError::new_err("script bootstrap contains NUL"))?;
        py.run(
            bootstrap.as_c_str(),
            Some(&module.dict()),
            Some(&module.dict()),
        )?;
        py.import("sys")?
            .getattr("modules")?
            .set_item("_pokecon_script", module)?;
        validate_runtime_surface(py)?;
        Ok(())
    })
    .map_err(|error| PythonActorError::new("PythonBootstrapError", error.to_string()))
}

fn add_python_paths(py: Python<'_>, command_root: &Path) -> PyResult<()> {
    if let Some(site_packages) = std::env::var_os(protocol::PYTHON_SITE_PACKAGES_ENV) {
        let site_packages = PathBuf::from(site_packages);
        if !site_packages.is_absolute() || !site_packages.is_dir() {
            return Err(PyRuntimeError::new_err(
                "script worker site-packages path is unavailable",
            ));
        }
        py.import("site")?
            .call_method1("addsitedir", (site_packages.to_string_lossy().as_ref(),))?;
    }
    py.import("sys")?
        .getattr("path")?
        .call_method1("insert", (0, command_root.to_string_lossy().as_ref()))?;
    Ok(())
}

fn execute_command(config: &PythonActorConfig, state: &ExecutionState, command: ExecuteCommand) {
    let mut outcome = match invoke_command(&command) {
        Ok(()) => ScriptExecutionOutcome::Completed,
        Err(error) => Python::attach(|py| {
            let stopped = py
                .import("_pokecon_script")
                .and_then(|module| module.getattr("StopThread"))
                .is_ok_and(|stop| error.is_instance(py, &stop));
            if stopped {
                match state.cause() {
                    StopCause::Finish => ScriptExecutionOutcome::Finished,
                    StopCause::None | StopCause::Requested => ScriptExecutionOutcome::Stopped,
                }
            } else {
                ScriptExecutionOutcome::Failed {
                    message: error.to_string(),
                }
            }
        }),
    };
    Python::attach(|py| {
        if let Ok(module) = py.import("_pokecon_script") {
            let _result = module
                .getattr("_reset_tk")
                .and_then(|function| function.call0());
        }
    });
    if let Err(error) = request_host::<_, ()>(
        &config.connection,
        &config.runtime_handle,
        protocol::HOST_DIALOG_CLOSE_ALL,
        &(),
    ) && matches!(outcome, ScriptExecutionOutcome::Completed)
    {
        outcome = ScriptExecutionOutcome::Failed {
            message: format!("dialog cleanup failed: {error}"),
        };
    }
    if let Err(error) = request_host::<_, HostTkResult>(
        &config.connection,
        &config.runtime_handle,
        protocol::HOST_TK,
        &HostTkRequest::Cleanup,
    ) && matches!(outcome, ScriptExecutionOutcome::Completed)
    {
        outcome = ScriptExecutionOutcome::Failed {
            message: format!("Tk compatibility cleanup failed: {error}"),
        };
    }
    if let Err(error) = request_host::<_, HostNetworkResult>(
        &config.connection,
        &config.runtime_handle,
        protocol::HOST_NETWORK,
        &HostNetworkRequest::Cleanup,
    ) && matches!(outcome, ScriptExecutionOutcome::Completed)
    {
        outcome = ScriptExecutionOutcome::Failed {
            message: format!("network cleanup failed: {error}"),
        };
    }
    if let Err(error) = request_host::<_, ()>(
        &config.connection,
        &config.runtime_handle,
        protocol::HOST_OVERLAY,
        &HostOverlayRequest::Cleanup,
    ) && matches!(outcome, ScriptExecutionOutcome::Completed)
    {
        outcome = ScriptExecutionOutcome::Failed {
            message: format!("overlay cleanup failed: {error}"),
        };
    }
    if let Err(error) = request_host::<_, ()>(
        &config.connection,
        &config.runtime_handle,
        protocol::HOST_CONTROLLER_NEUTRAL,
        &(),
    ) && matches!(outcome, ScriptExecutionOutcome::Completed)
    {
        outcome = ScriptExecutionOutcome::Failed {
            message: format!("controller neutralization failed: {error}"),
        };
    }
    state.complete(command.id);
    let _result = command.response.send(ScriptExecutionResult {
        execution_id: command.id,
        outcome,
    });
}

fn invoke_command(command: &ExecuteCommand) -> PyResult<()> {
    let source = std::fs::read_to_string(&command.path)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    Python::attach(|py| {
        let module = py.import("_pokecon_script")?;
        module.getattr("_run_source")?.call1((
            source,
            command.path.to_string_lossy().as_ref(),
            &command.class_name,
            serde_json::to_string(&command.tags)
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?,
        ))?;
        Ok(())
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonDiscoveredClass {
    name: String,
    class_name: String,
    manual_tags: Vec<String>,
    kind: ScriptCommandKind,
}

fn discover_commands(
    sources: &[DiscoverSource],
) -> Result<Vec<ScriptDiscoveredCommand>, PythonActorError> {
    let mut commands = Vec::new();
    for source in sources {
        let content = std::fs::read_to_string(&source.path)
            .map_err(|error| PythonActorError::new("ScriptReadError", error.to_string()))?;
        let encoded = Python::attach(|py| -> PyResult<String> {
            py.import("_pokecon_script")?
                .getattr("_discover_source")?
                .call1((
                    content,
                    source.path.to_string_lossy().as_ref(),
                    &source.module_path,
                ))?
                .extract()
        })
        .map_err(|error| PythonActorError::new("ScriptDiscoveryError", error.to_string()))?;
        let discovered = serde_json::from_str::<Vec<PythonDiscoveredClass>>(&encoded)
            .map_err(|error| PythonActorError::new("ScriptDiscoveryError", error.to_string()))?;
        commands.extend(
            discovered
                .into_iter()
                .map(|discovered| ScriptDiscoveredCommand {
                    command: pokecon_dynamic::CommandInfo {
                        name: discovered.name,
                        module_path: source.module_path.clone(),
                        class_name: discovered.class_name,
                        tags: source.automatic_tags.clone(),
                    },
                    relative_path: source.relative_path.clone(),
                    manual_tags: discovered.manual_tags,
                    kind: discovered.kind,
                }),
        );
    }
    Ok(commands)
}

fn validate_runtime_surface(py: Python<'_>) -> PyResult<()> {
    let registry = serde_json::from_str::<serde_json::Value>(PROTOCOL_REGISTRY_JSON)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let surfaces = registry["public_surfaces"]
        .as_array()
        .ok_or_else(|| PyRuntimeError::new_err("protocol public_surfaces is unavailable"))?;
    let surface = surfaces
        .iter()
        .find(|surface| surface["worker"] == "user_script")
        .ok_or_else(|| PyRuntimeError::new_err("user_script public surface is unavailable"))?;

    for required in surface["required_imports"]
        .as_array()
        .ok_or_else(|| PyRuntimeError::new_err("required_imports is unavailable"))?
    {
        let module_name = required["module"]
            .as_str()
            .ok_or_else(|| PyRuntimeError::new_err("required import module is invalid"))?;
        let module = py.import(module_name)?;
        for symbol in required["symbols"]
            .as_array()
            .ok_or_else(|| PyRuntimeError::new_err("required import symbols are invalid"))?
        {
            let symbol = symbol
                .as_str()
                .ok_or_else(|| PyRuntimeError::new_err("required import symbol is invalid"))?;
            module.getattr(symbol).map_err(|_| {
                PyRuntimeError::new_err(format!("runtime is missing {module_name}.{symbol}"))
            })?;
        }
    }

    let class_modules = [
        ("CommandMeta", "Commands._meta"),
        ("Command", "Commands.CommandBase"),
        ("PythonCommand", "Commands.PythonCommandBase"),
        ("ImageProcPythonCommand", "Commands.PythonCommandBase"),
        ("Camera", "Commands.PythonCommandBase"),
        ("CaptureArea", "Commands.PythonCommandBase"),
        ("McuCommand", "Commands.McuCommandBase"),
        ("KeyPress", "Commands.Keys"),
        ("Sender", "Commands.Sender"),
        ("Widget", "Commands.dialogue"),
    ];
    let class_members = surface["class_members"]
        .as_object()
        .ok_or_else(|| PyRuntimeError::new_err("class_members is unavailable"))?;
    for (class_name, module_name) in class_modules {
        let class = py.import(module_name)?.getattr(class_name)?;
        for member in class_members[class_name]
            .as_array()
            .ok_or_else(|| PyRuntimeError::new_err("class member list is invalid"))?
        {
            let member = member
                .as_str()
                .ok_or_else(|| PyRuntimeError::new_err("class member is invalid"))?;
            class.getattr(member).map_err(|_| {
                PyRuntimeError::new_err(format!("runtime is missing {class_name}.{member}"))
            })?;
        }
    }

    for (module_name, functions) in surface["module_functions"]
        .as_object()
        .ok_or_else(|| PyRuntimeError::new_err("module_functions is unavailable"))?
    {
        let module = py.import(module_name)?;
        for function in functions
            .as_array()
            .ok_or_else(|| PyRuntimeError::new_err("module function list is invalid"))?
        {
            let function = function
                .as_str()
                .ok_or_else(|| PyRuntimeError::new_err("module function is invalid"))?;
            module.getattr(function).map_err(|_| {
                PyRuntimeError::new_err(format!("runtime is missing {module_name}.{function}"))
            })?;
        }
    }
    Ok(())
}

#[pyclass]
struct PyApi {
    connection: IpcConnection,
    runtime_handle: Handle,
    state: Arc<ExecutionState>,
    profile: String,
    command_root: PathBuf,
    data_root: PathBuf,
    camera_reader: Option<RingReader>,
    screenshot: ScreenshotService,
}

impl PyApi {
    fn request<Request, Response>(
        &self,
        operation: &'static str,
        request: &Request,
    ) -> PyResult<Response>
    where
        Request: serde::Serialize + Sync,
        Response: serde::de::DeserializeOwned + Send,
    {
        Python::attach(|py| {
            py.detach(|| request_host(&self.connection, &self.runtime_handle, operation, request))
        })
        .map_err(PyRuntimeError::new_err)
    }
}

#[pymethods]
impl PyApi {
    fn is_executing(&self) -> bool {
        self.state.active_id.load(Ordering::Acquire) != 0
    }

    fn is_alive(&self) -> bool {
        self.state.alive.load(Ordering::Acquire)
    }

    fn execution_checkpoint(&self, py: Python<'_>) -> bool {
        if self.state.paused.load(Ordering::Acquire) {
            py.detach(|| self.state.checkpoint())
        } else {
            self.state.alive.load(Ordering::Acquire)
        }
    }

    fn finish(&self) {
        self.state.request_finish();
    }

    fn abort(&self) {
        self.state.request_stop();
    }

    fn profile_name(&self) -> &str {
        &self.profile
    }

    fn data_root(&self) -> String {
        self.data_root.to_string_lossy().into_owned()
    }

    fn command_root(&self) -> String {
        self.command_root.to_string_lossy().into_owned()
    }

    fn camera_state(&self) -> PyResult<String> {
        let state = self.request::<_, HostCameraState>(
            protocol::HOST_CAMERA_CONTROL,
            &HostCameraControlRequest::State,
        )?;
        serde_json::to_string(&state).map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn camera_control(&self, request_json: &str) -> PyResult<String> {
        let request = serde_json::from_str::<HostCameraControlRequest>(request_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let state = self.request::<_, HostCameraState>(protocol::HOST_CAMERA_CONTROL, &request)?;
        serde_json::to_string(&state).map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn camera_frame(&self, py: Python<'_>, resolution: &str) -> PyResult<(u32, u32, Py<PyBytes>)> {
        let resolution = CaptureResolution::from_str(resolution)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let frame = self.camera_reader.as_ref().map_or_else(
            || Ok(BgrFrame::zero(resolution)),
            |reader| reader.read(resolution),
        );
        let frame = frame.map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        let size = frame.size();
        Ok((
            size.width(),
            size.height(),
            PyBytes::new(py, frame.pixels()).unbind(),
        ))
    }

    fn save_image(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        filename: Option<&str>,
        format: &str,
    ) -> PyResult<String> {
        let frame = BgrFrame::new(width, height, pixels)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let format = ScreenshotFormat::from_str(format)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        self.screenshot
            .save_compatibility_frame(&frame, filename, Some(format))
            .map(|saved| saved.display_path)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn overlay(&self, request_json: &str) -> PyResult<()> {
        let request = serde_json::from_str::<HostOverlayRequest>(request_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        self.request(protocol::HOST_OVERLAY, &request)
    }

    fn popup_image(&self, title: String, content_type: String, encoded: Vec<u8>) -> PyResult<()> {
        if encoded.len() > protocol::MAX_POPUP_IMAGE_BYTES {
            return Err(PyValueError::new_err(
                "compressed popup image exceeds the bounded payload limit",
            ));
        }
        self.request(
            protocol::HOST_POPUP_IMAGE,
            &HostPopupImageRequest {
                title,
                content_type,
                encoded,
            },
        )
    }

    fn tk(&self, request_json: &str) -> PyResult<String> {
        let request = serde_json::from_str::<HostTkRequest>(request_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let result = self.request::<_, HostTkResult>(protocol::HOST_TK, &request)?;
        serde_json::to_string(&result).map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn controller_input(
        &self,
        action: &str,
        controls_json: &str,
        unset_hat: bool,
        unset_touchscreen: bool,
    ) -> PyResult<()> {
        let action = match action {
            "press" => ScriptInputAction::Press,
            "release" => ScriptInputAction::Release,
            _ => return Err(PyValueError::new_err("unknown controller input action")),
        };
        let controls = serde_json::from_str::<Vec<ScriptControl>>(controls_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        self.request(
            protocol::HOST_CONTROLLER_INPUT,
            &HostControllerInputRequest {
                action,
                controls,
                unset_hat,
                unset_touchscreen,
            },
        )
    }

    fn controller_neutral(&self) -> PyResult<()> {
        self.request(protocol::HOST_CONTROLLER_NEUTRAL, &())
    }

    fn serial_write(&self, data: Vec<u8>) -> PyResult<()> {
        self.request(
            protocol::HOST_SERIAL_WRITE,
            &HostSerialWriteRequest { data },
        )
    }

    fn serial_write_row(&self, row: String) -> PyResult<()> {
        self.request(
            protocol::HOST_SERIAL_WRITE_ROW,
            &HostSerialWriteRowRequest { row },
        )
    }

    fn serial_reload(&self) -> PyResult<()> {
        self.request(protocol::HOST_SERIAL_RELOAD, &())
    }

    fn output(&self, target: &str, mode: Option<&str>, message: String) -> PyResult<()> {
        let target = match target {
            "stdout" => ScriptOutputTarget::Stdout,
            "panel1" => ScriptOutputTarget::Panel1,
            "panel2" => ScriptOutputTarget::Panel2,
            "alternate" => ScriptOutputTarget::Alternate,
            _ => return Err(PyValueError::new_err("unknown script output target")),
        };
        let mode = match mode {
            None => None,
            Some("w") => Some(ScriptOutputMode::Write),
            Some("a") => Some(ScriptOutputMode::Append),
            Some("d") => Some(ScriptOutputMode::Delete),
            Some(_) => return Err(PyValueError::new_err("unknown script output mode")),
        };
        self.request(
            protocol::HOST_OUTPUT,
            &HostOutputRequest {
                target,
                mode,
                message,
            },
        )
    }

    fn dialog_open(
        &self,
        title: String,
        description: Option<String>,
        widgets_json: &str,
    ) -> PyResult<u64> {
        let widgets = serde_json::from_str(widgets_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let result = self.request::<_, HostDialogOpenResult>(
            protocol::HOST_DIALOG_OPEN,
            &HostDialogOpenRequest {
                title,
                description,
                widgets,
            },
        )?;
        if result.dialog_id == 0 {
            return Err(PyRuntimeError::new_err(
                "script dialog host returned reserved dialog ID zero",
            ));
        }
        Ok(result.dialog_id)
    }

    fn dialog_status(&self, dialog_id: u64) -> PyResult<String> {
        let result = self.request::<_, HostDialogStatusResult>(
            protocol::HOST_DIALOG_STATUS,
            &HostDialogStatusRequest { dialog_id },
        )?;
        serde_json::to_string(&result).map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn dialog_close_all(&self) -> PyResult<()> {
        self.request(protocol::HOST_DIALOG_CLOSE_ALL, &())
    }

    fn network(&self, request_json: &str) -> PyResult<String> {
        let request = serde_json::from_str::<HostNetworkRequest>(request_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let result = self.request::<_, HostNetworkResult>(protocol::HOST_NETWORK, &request)?;
        serde_json::to_string(&result).map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn notification(&self, request_json: &str) -> PyResult<()> {
        let request = serde_json::from_str::<HostNotificationRequest>(request_json)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        self.request(protocol::HOST_NOTIFICATION, &request)
    }
}

fn request_host<Request, Response>(
    connection: &IpcConnection,
    runtime_handle: &Handle,
    operation: &'static str,
    request: &Request,
) -> Result<Response, String>
where
    Request: serde::Serialize,
    Response: serde::de::DeserializeOwned,
{
    let payload = serialize_value(request).map_err(|error| error.to_string())?;
    let future = connection.request(operation, payload);
    let response = if Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| runtime_handle.block_on(future))
    } else {
        runtime_handle.block_on(future)
    }
    .map_err(connection_error)?;
    deserialize_value(&response).map_err(|error| error.to_string())
}

fn connection_error(error: ConnectionError) -> String {
    match error {
        ConnectionError::Remote { code, message } => format!("{code}: {message}"),
        error => error.to_string(),
    }
}

const PYTHON_BOOTSTRAP: &str = r#"
import enum as _enum
import inspect as _inspect
import json as _json
import logging as _logging
import math as _math
import os as _os
import queue as _queue
import sys as _sys
import threading as _threading
import time as _time
import traceback as _traceback
import types as _types


class StopThread(Exception):
    pass


def _abstract_command_method(function):
    function.__pokecon_abstract_command_method__ = True
    return function


class CommandMeta(type):
    def __call__(cls, *args, **kwargs):
        if getattr(cls, "__requires_do__", False):
            implementation = _inspect.getattr_static(cls, "do", None)
            if isinstance(implementation, (classmethod, staticmethod)):
                implementation = implementation.__func__
            if implementation is None or getattr(
                implementation, "__pokecon_abstract_command_method__", False
            ):
                raise TypeError(
                    f"cannot instantiate abstract command {cls.__name__!r} "
                    "without implementing do()"
                )
        return super().__call__(*args, **kwargs)


class Command(metaclass=CommandMeta):
    isRunning = False

    def __init__(self):
        self.isRunning = False


def _monitor_stop(_code, _offset):
    if _api.is_executing() and not _api.execution_checkpoint():
        current = globals().get("_current")
        command = None if current is None else getattr(current, "command", None)
        if command is not None:
            cleanup = getattr(command, "_stop_cleanup", None)
            if cleanup is not None:
                cleanup()
        raise StopThread()


_tool_id = _sys.monitoring.DEBUGGER_ID
try:
    _sys.monitoring.free_tool_id(_tool_id)
except ValueError:
    pass
_sys.monitoring.use_tool_id(_tool_id, "pokecon.script.stop")
_sys.monitoring.register_callback(
    _tool_id, _sys.monitoring.events.INSTRUCTION, _monitor_stop
)
_sys.monitoring.set_events(_tool_id, _sys.monitoring.events.INSTRUCTION)


class _ScriptStdout:
    def write(self, text):
        text = str(text)
        if text:
            _api.output("stdout", None, text)
        return len(text)

    def flush(self):
        return None

    def isatty(self):
        return False


_sys.stdout = _ScriptStdout()


class _ScriptLogHandler(_logging.Handler):
    def emit(self, record):
        try:
            _api.output("stdout", None, self.format(record) + "\n")
        except Exception:
            self.handleError(record)


def _make_logger():
    logger = _logging.getLogger(f"pokecon.user.{id(_threading.current_thread())}")
    logger.handlers.clear()
    logger.addHandler(_ScriptLogHandler())
    logger.setLevel(_logging.DEBUG)
    logger.propagate = False
    return logger


class Button(_enum.IntFlag):
    Y = 1
    B = 2
    A = 4
    X = 8
    L = 16
    R = 32
    ZL = 64
    ZR = 128
    MINUS = 256
    PLUS = 512
    LCLICK = 1024
    RCLICK = 2048
    HOME = 4096
    CAPTURE = 8192
    SELECT = MINUS
    START = PLUS
    POWER = LCLICK
    WIRELESS = RCLICK


class Hat(_enum.IntEnum):
    TOP = 0
    TOP_RIGHT = 1
    RIGHT = 2
    BTM_RIGHT = 3
    BTM = 4
    BTM_LEFT = 5
    LEFT = 6
    TOP_LEFT = 7
    CENTER = 8


class Stick(_enum.Enum):
    LEFT = 1
    RIGHT = 2


class Direction:
    def __init__(
        self, stick, angle, magnification=1.0, isDegree=True, showName=None
    ):
        if not isinstance(stick, Stick):
            raise TypeError("stick must be Stick")
        self.stick = stick
        self.angle_for_show = angle
        self.showName = showName
        self.mag = min(1.0, max(0.0, float(magnification)))
        if isinstance(angle, tuple):
            if len(angle) != 2:
                raise ValueError("stick coordinate tuple must contain two values")
            self.x = int(angle[0])
            self.y = int(angle[1])
            self.showName = f"({self.x}, {self.y})"
        else:
            radians = _math.radians(angle) if isDegree else angle
            self.x = _math.ceil(127.5 * _math.cos(radians) * self.mag + 127.5)
            self.y = _math.floor(127.5 * _math.sin(radians) * self.mag + 127.5)
        self.x = min(255, max(0, self.x))
        self.y = min(255, max(0, self.y))

    @property
    def name(self):
        return repr(self)

    def __repr__(self):
        shown = self.showName or f"{self.angle_for_show}[deg]"
        return f"<{self.stick}, {shown}>"

    def __eq__(self, other):
        return isinstance(other, Direction) and (
            self.stick,
            self.angle_for_show,
        ) == (other.stick, other.angle_for_show)

    def __hash__(self):
        return hash((self.x, self.y, self.stick))


Direction.UP = Direction(Stick.LEFT, 90, showName="UP")
Direction.RIGHT = Direction(Stick.LEFT, 0, showName="RIGHT")
Direction.DOWN = Direction(Stick.LEFT, -90, showName="DOWN")
Direction.LEFT = Direction(Stick.LEFT, -180, showName="LEFT")
Direction.UP_RIGHT = Direction(Stick.LEFT, 45, showName="UP_RIGHT")
Direction.DOWN_RIGHT = Direction(Stick.LEFT, -45, showName="DOWN_RIGHT")
Direction.DOWN_LEFT = Direction(Stick.LEFT, -135, showName="DOWN_LEFT")
Direction.UP_LEFT = Direction(Stick.LEFT, 135, showName="UP_LEFT")
Direction.R_UP = Direction(Stick.RIGHT, 90, showName="UP")
Direction.R_RIGHT = Direction(Stick.RIGHT, 0, showName="RIGHT")
Direction.R_DOWN = Direction(Stick.RIGHT, -90, showName="DOWN")
Direction.R_LEFT = Direction(Stick.RIGHT, -180, showName="LEFT")
Direction.R_UP_RIGHT = Direction(Stick.RIGHT, 45, showName="UP_RIGHT")
Direction.R_DOWN_RIGHT = Direction(Stick.RIGHT, -45, showName="DOWN_RIGHT")
Direction.R_DOWN_LEFT = Direction(Stick.RIGHT, -135, showName="DOWN_LEFT")
Direction.R_UP_LEFT = Direction(Stick.RIGHT, 135, showName="UP_LEFT")


class Touchscreen:
    def __init__(self, x, y):
        self.x = int(x)
        self.y = int(y)

    @property
    def name(self):
        return f"<Touchscreen, ({self.x}, {self.y})>"

    def __eq__(self, other):
        return isinstance(other, Touchscreen) and (self.x, self.y) == (
            other.x,
            other.y,
        )

    def __hash__(self):
        return hash((self.x, self.y))


_button_names = [
    (Button.Y, "Y"),
    (Button.B, "B"),
    (Button.A, "A"),
    (Button.X, "X"),
    (Button.L, "L"),
    (Button.R, "R"),
    (Button.ZL, "ZL"),
    (Button.ZR, "ZR"),
    (Button.MINUS, "MINUS"),
    (Button.PLUS, "PLUS"),
    (Button.LCLICK, "LCLICK"),
    (Button.RCLICK, "RCLICK"),
    (Button.HOME, "HOME"),
    (Button.CAPTURE, "CAPTURE"),
]
_button_mask = sum(int(button) for button, _name in _button_names)
_hat_names = {
    Hat.TOP: "top",
    Hat.TOP_RIGHT: "top_right",
    Hat.RIGHT: "right",
    Hat.BTM_RIGHT: "bottom_right",
    Hat.BTM: "bottom",
    Hat.BTM_LEFT: "bottom_left",
    Hat.LEFT: "left",
    Hat.TOP_LEFT: "top_left",
    Hat.CENTER: "center",
}


def _controls(value):
    values = value if isinstance(value, list) else [value]
    result = []
    for item in values:
        if isinstance(item, Button):
            raw = int(item)
            if raw == 0 or raw & ~_button_mask:
                raise ValueError("button mask contains unsupported bits")
            result.extend(
                {"kind": "button", "button": name}
                for button, name in _button_names
                if item & button
            )
        elif isinstance(item, Hat):
            result.append({"kind": "hat", "direction": _hat_names[item]})
        elif isinstance(item, Direction):
            result.append(
                {
                    "kind": "stick",
                    "stick": item.stick.name.lower(),
                    "x": item.x,
                    "y": item.y,
                }
            )
        elif isinstance(item, Touchscreen):
            result.append({"kind": "touchscreen", "x": item.x, "y": item.y})
        else:
            raise TypeError(f"unsupported controller input {type(item).__name__}")
    return _json.dumps(result, separators=(",", ":"))


class Sender:
    def writeRow(self, row):
        if not isinstance(row, str):
            raise TypeError("serial row must be str")
        _api.serial_write_row(row)

    def write(self, data):
        if isinstance(data, str):
            raise TypeError("unicode strings are not supported, please encode to bytes")
        if isinstance(data, bytes):
            raw = data
        elif isinstance(data, (bytearray, memoryview)):
            raw = bytes(data)
        else:
            raw = bytes(bytearray(data))
        _api.serial_write(list(raw))


class KeyPress:
    ser = None

    def __init__(self, ser):
        self.ser = ser

    def input(self, btns, ifPrint=True):
        _api.controller_input("press", _controls(btns), True, True)

    def inputEnd(
        self,
        btns,
        ifPrint=True,
        unset_hat=True,
        unset_Touchscreen=True,
    ):
        _api.controller_input(
            "release", _controls(btns), bool(unset_hat), bool(unset_Touchscreen)
        )

    def hold(self, btns):
        self.input(btns)

    def holdEnd(self, btns):
        self.inputEnd(btns)

    def neutral(self):
        _api.controller_neutral()

    def end(self):
        self.ser.writeRow("end")


class Widget:
    value = None

    def __init__(self, widget_type, *args, **kwargs):
        kinds = {
            "entry": "Entry",
            "check": "Check",
            "combo": "Combo",
            "radio": "Radio",
            "spin": "Spin",
            "scale": "Scale",
            "next": "Next",
        }
        normalized = str(widget_type).casefold()
        if normalized not in kinds:
            raise ValueError(f"unsupported widget type {widget_type!r}")
        self.widget_type = kinds[normalized]
        self._has_result = False
        self._active_dialog = None
        self.label = None
        self.options = []
        self.minimum = None
        self.maximum = None
        self.precision = None
        if self.widget_type == "Next":
            if args or kwargs:
                raise TypeError("Next widget accepts no additional arguments")
            self.value = None
        elif self.widget_type in {"Entry", "Check"}:
            self.label = _widget_argument(args, kwargs, 0, "label")
            self.value = _widget_argument(args, kwargs, 1, "default")
            _reject_widget_arguments(args, kwargs, 2, {"label", "default"})
        elif self.widget_type in {"Combo", "Radio"}:
            self.label = _widget_argument(args, kwargs, 0, "label")
            self.options = list(_widget_argument(args, kwargs, 1, "options"))
            self.value = _widget_argument(args, kwargs, 2, "default")
            _reject_widget_arguments(
                args, kwargs, 3, {"label", "options", "default"}
            )
            if self.value not in self.options:
                raise ValueError("widget default must be present in options")
        elif self.widget_type == "Spin" and (
            "options" in kwargs or (len(args) == 3 and "min" not in kwargs)
        ):
            self.label = _widget_argument(args, kwargs, 0, "label")
            self.options = list(_widget_argument(args, kwargs, 1, "options"))
            self.value = _widget_argument(args, kwargs, 2, "default")
            _reject_widget_arguments(
                args, kwargs, 3, {"label", "options", "default"}
            )
            if self.value not in self.options:
                raise ValueError("widget default must be present in options")
        else:
            self.label = _widget_argument(args, kwargs, 0, "label")
            self.minimum = float(_widget_argument(args, kwargs, 1, "min"))
            self.maximum = float(_widget_argument(args, kwargs, 2, "max"))
            self.value = _widget_argument(args, kwargs, 3, "default")
            self.precision = int(kwargs.pop("precision", kwargs.pop("digit", 0)))
            _reject_widget_arguments(
                args, kwargs, 4, {"label", "min", "max", "default"}
            )
            if self.minimum > self.maximum:
                raise ValueError("widget minimum must not exceed maximum")
            numeric = float(self.value)
            if not self.minimum <= numeric <= self.maximum:
                raise ValueError("widget default is outside its range")
        if self.label is not None and not isinstance(self.label, str):
            raise TypeError("widget label must be str")
        _encode_dialog_value(self.value)
        for option in self.options:
            _encode_dialog_value(option)

    @property
    def has_result(self):
        return self._has_result

    def _payload(self):
        return {
            "kind": self.widget_type.casefold(),
            "label": self.label,
            "value": _encode_dialog_value(self.value),
            "options": [_encode_dialog_value(value) for value in self.options],
            "minimum": self.minimum,
            "maximum": self.maximum,
            "precision": self.precision,
        }


def _widget_argument(args, kwargs, index, name):
    if index < len(args):
        if name in kwargs:
            raise TypeError(f"multiple values for widget argument {name!r}")
        return args[index]
    if name in kwargs:
        return kwargs.pop(name)
    raise TypeError(f"missing required widget argument {name!r}")


def _reject_widget_arguments(args, kwargs, positional, allowed):
    if len(args) > positional:
        raise TypeError("too many positional widget arguments")
    unknown = set(kwargs) - allowed
    if unknown:
        name = sorted(unknown)[0]
        raise TypeError(f"unexpected widget argument {name!r}")


def _encode_dialog_value(value):
    if value is None:
        return {"type": "none"}
    if isinstance(value, bool):
        return {"type": "bool", "value": value}
    if isinstance(value, int):
        if not -(2**63) <= value < 2**63:
            raise OverflowError("dialog integer must fit signed 64 bits")
        return {"type": "integer", "value": value}
    if isinstance(value, float):
        if not _math.isfinite(value):
            raise ValueError("dialog float must be finite")
        return {"type": "float", "value": value}
    if isinstance(value, str):
        return {"type": "string", "value": value}
    raise TypeError(f"unsupported dialog value {type(value).__name__}")


def _decode_dialog_value(encoded):
    kind = encoded.get("type")
    if kind == "none":
        return None
    if kind in {"string", "bool", "integer", "float"}:
        return encoded["value"]
    raise RuntimeError(f"dialog host returned unknown value type {kind!r}")


_dialogs = {}
_closed_dialogs = set()


def _reset_dialogs():
    for widgets in _dialogs.values():
        for widget in widgets:
            widget._active_dialog = None
    _dialogs.clear()
    _closed_dialogs.clear()


def _poll_dialog(dialog_id):
    if dialog_id in _closed_dialogs:
        return True
    widgets = _dialogs.get(dialog_id)
    if widgets is None:
        return False
    response = _json.loads(_api.dialog_status(dialog_id))
    state = response["state"]
    if state["state"] == "open":
        return False
    if state["state"] == "aborted":
        for widget in widgets:
            widget._active_dialog = None
        del _dialogs[dialog_id]
        _api.abort()
        raise StopThread()
    values = state["values"]
    if len(values) != len(widgets):
        raise RuntimeError("dialog result count differs from widget count")
    for widget, value in zip(widgets, values, strict=True):
        decoded = _decode_dialog_value(value)
        if widget.widget_type == "Next" and decoded is not None:
            raise RuntimeError("Next dialog widget returned a value")
        widget.value = decoded
        widget._has_result = True
        widget._active_dialog = None
    del _dialogs[dialog_id]
    _closed_dialogs.add(dialog_id)
    return True


def _show_dialog(command, title, widgets, blocking=True, description=None):
    if isinstance(widgets, Widget):
        widgets = [widgets]
    elif isinstance(widgets, list):
        widgets = list(widgets)
    else:
        raise TypeError("widgets must be a Widget or list[Widget]")
    if not widgets or any(not isinstance(widget, Widget) for widget in widgets):
        raise TypeError("dialog requires one or more Widget instances")
    if len({id(widget) for widget in widgets}) != len(widgets):
        raise ValueError("the same Widget cannot occur twice in one dialog")
    for widget in widgets:
        if widget._active_dialog is not None:
            raise RuntimeError("Widget already belongs to an open dialog")
        widget._has_result = False
    payload = _json.dumps(
        [widget._payload() for widget in widgets],
        ensure_ascii=False,
        separators=(",", ":"),
        allow_nan=False,
    )
    dialog_id = _api.dialog_open(str(title), description, payload)
    if dialog_id in _dialogs or dialog_id in _closed_dialogs:
        _api.dialog_close_all()
        raise RuntimeError("dialog host reused an active dialog ID")
    for widget in widgets:
        widget._active_dialog = dialog_id
    _dialogs[dialog_id] = widgets
    if not bool(blocking):
        return dialog_id
    _wait_dialog(command, dialog_id)
    return 0


def _wait_dialog(command, dialog_id):
    while not _poll_dialog(dialog_id):
        command.checkIfAlive()
        _time.sleep(0.02)
    return 0


def _legacy_widgets(dialogue_list):
    if not isinstance(dialogue_list, list):
        raise TypeError("dialogue_list must be list")
    widgets = []
    labels = set()
    for definition in dialogue_list:
        if not isinstance(definition, list) or not definition:
            raise TypeError("each legacy dialog widget must be a non-empty list")
        kind = str(definition[0]).casefold()
        if kind == "next":
            if len(definition) != 1:
                raise TypeError("Next legacy widget accepts no other values")
            widgets.append(Widget("Next"))
            continue
        if len(definition) < 3:
            raise TypeError("legacy dialog widget is missing required values")
        label = definition[1]
        if label in labels:
            raise ValueError("legacy dialog widget names must be unique")
        labels.add(label)
        shown_label = str(label)
        if kind == "entry":
            widget = Widget("Entry", shown_label, str(definition[2]))
        elif kind == "check":
            default = definition[2]
            if isinstance(default, str):
                default = default.casefold() in {"true", "1", "yes", "on"}
            widget = Widget("Check", shown_label, bool(default))
        elif kind in {"combo", "radio", "spin"}:
            if len(definition) < 4:
                raise TypeError("legacy selection widget is missing its default")
            widget = Widget(
                kind.title(), shown_label, list(definition[2]), definition[3]
            )
        elif kind == "scale":
            if len(definition) < 5:
                raise TypeError("legacy Scale widget is missing range values")
            precision = int(definition[5]) if len(definition) > 5 else 0
            widget = Widget(
                "Scale",
                shown_label,
                definition[2],
                definition[3],
                definition[4],
                precision=precision,
            )
        else:
            raise ValueError(f"unsupported legacy widget type {definition[0]!r}")
        widget._legacy_label = label
        widgets.append(widget)
    return widgets


def _legacy_result(widgets, need):
    values = [widget for widget in widgets if widget.widget_type != "Next"]
    if need is dict:
        return {
            getattr(widget, "_legacy_label", widget.label): widget.value
            for widget in values
        }
    return [widget.value for widget in values]


def _load_legacy_settings(filename):
    try:
        with open(filename, encoding="utf-8") as settings_file:
            value = _json.load(settings_file)
        return value if isinstance(value, dict) else {}
    except (OSError, ValueError):
        return {}


def _save_legacy_settings(filename, values):
    parent = _os.path.dirname(_os.path.abspath(filename))
    _os.makedirs(parent, exist_ok=True)
    temporary = f"{filename}.tmp-{_os.getpid()}-{_threading.get_ident()}"
    try:
        with open(temporary, "w", encoding="utf-8") as settings_file:
            _json.dump(values, settings_file, indent=4, ensure_ascii=False)
            settings_file.flush()
            _os.fsync(settings_file.fileno())
        _os.replace(temporary, filename)
    finally:
        try:
            _os.unlink(temporary)
        except FileNotFoundError:
            pass


def _apply_legacy_settings(widgets, settings):
    for widget in widgets:
        label = getattr(widget, "_legacy_label", None)
        if label in settings and widget.widget_type != "Next":
            value = settings[label]
            _encode_dialog_value(value)
            widget.value = value


def _not_implemented(name):
    raise NotImplementedError(f"{name} is not available in this compatibility layer yet")


def _network(operation, **values):
    request = {"operation": operation, **values}
    response = _json.loads(
        _api.network(
            _json.dumps(
                request,
                ensure_ascii=False,
                separators=(",", ":"),
                allow_nan=False,
            )
        )
    )
    return response["message"]


def _notification(request):
    _api.notification(
        _json.dumps(
            request,
            ensure_ascii=False,
            separators=(",", ":"),
            allow_nan=False,
        )
    )


class PythonCommand(Command):
    __requires_do__ = True
    _logger = None
    keys = None
    postProcess = None

    def __init__(self):
        super().__init__()
        self._logger = _make_logger()
        self.keys = KeyPress(Sender())
        self.postProcess = None
        self._stop_cleanup_done = False

    @property
    def alive(self):
        return _api.is_alive()

    @_abstract_command_method
    def do(self):
        raise NotImplementedError("PythonCommand.do() must be overridden")

    def finish(self):
        _api.finish()
        self._stop_cleanup()
        raise StopThread()

    def checkIfAlive(self):
        if not self.alive:
            self._stop_cleanup()
            raise StopThread()
        return True

    def _stop_cleanup(self):
        if self._stop_cleanup_done:
            return
        self._stop_cleanup_done = True
        self.isRunning = False
        try:
            self.keys.end()
        except Exception:
            self._logger.exception("failed to release command keys during stop")
        post_process = self.postProcess
        self.postProcess = None
        if post_process is not None:
            try:
                post_process()
            except Exception:
                self._logger.exception("command postProcess callback failed")

    def press(self, buttons, duration=0.1, wait=0.1):
        self.keys.input(buttons)
        self.wait(duration)
        self.keys.inputEnd(buttons)
        self.wait(wait)

    def pressRep(self, buttons, repeat, duration=0.1, interval=0.1, wait=0.1):
        for index in range(repeat):
            self.press(buttons, duration, interval if index + 1 < repeat else wait)

    def hold(self, buttons, wait=0.1):
        self.keys.hold(buttons)
        self.wait(wait)

    def holdEnd(self, buttons):
        self.keys.holdEnd(buttons)

    def wait(self, wait):
        wait = float(wait)
        if wait > 0.1:
            deadline = _time.perf_counter() + wait
            while True:
                self.checkIfAlive()
                remaining = deadline - _time.perf_counter()
                if remaining <= 0:
                    break
                _time.sleep(min(remaining, 0.05))
        else:
            self.short_wait(wait)

    def short_wait(self, wait):
        deadline = _time.perf_counter() + float(wait)
        while _time.perf_counter() < deadline:
            self.checkIfAlive()
        self.checkIfAlive()

    def direct_serial(self, commands, waittimes):
        for waittime, command in zip(waittimes, commands, strict=False):
            self.wait(waittime)
            self.keys.ser.writeRow(str(command).replace("\r", "").replace("\n", ""))

    def reload_com_port(self):
        _api.serial_reload()

    def _print(self, target, mode, objects, sep, end):
        _api.output(target, mode, sep.join(str(item) for item in objects) + end)

    def print_t1(self, *objects, sep=" ", end="\n"):
        self._print("panel1", None, objects, sep, end)

    def print_t2(self, *objects, sep=" ", end="\n"):
        self._print("panel2", None, objects, sep, end)

    def print_t(self, *objects, sep=" ", end="\n"):
        self._print("alternate", None, objects, sep, end)

    def print_s(self, *objects, sep=" ", end="\n"):
        self._print("stdout", None, objects, sep, end)

    print_ts = print_s

    def print_t1b(self, mode, *objects, sep=" ", end="\n"):
        self._print("panel1", mode, objects, sep, end)

    def print_t2b(self, mode, *objects, sep=" ", end="\n"):
        self._print("panel2", mode, objects, sep, end)

    def print_tb(self, mode, *objects, sep=" ", end="\n"):
        self._print("alternate", mode, objects, sep, end)

    def print_tbs(self, mode, *objects, sep=" ", end="\n"):
        self._print("stdout", mode, objects, sep, end)

    def show_var(self):
        internal = {
            "_logger",
            "_stop_cleanup_done",
            "isRunning",
            "keys",
            "postProcess",
        }
        values = {
            name: value
            for name, value in vars(self).items()
            if name not in internal and not name.startswith("__")
        }
        self.print_s(values)

    def show_dialog(self, title, widgets, blocking=True):
        return _show_dialog(self, title, widgets, blocking)

    def is_dialog_closed(self, dialog_id):
        return _poll_dialog(int(dialog_id))

    def wait_dialog(self, dialog_id):
        return _wait_dialog(self, int(dialog_id))

    def dialogue6widget(self, title, dialogue_list, desc=None, need=list):
        widgets = _legacy_widgets(dialogue_list)
        _show_dialog(self, title, widgets, True, desc)
        return _legacy_result(widgets, need)

    def dialogue6widget_save_settings(
        self, title, dialogue_list, filename, desc=None, need=list
    ):
        widgets = _legacy_widgets(dialogue_list)
        _apply_legacy_settings(widgets, _load_legacy_settings(filename))
        _show_dialog(self, title, widgets, True, desc)
        result = _legacy_result(widgets, need)
        _save_legacy_settings(
            filename, _legacy_result(widgets, dict)
        )
        return result

    def dialogue6widget_select_settings(
        self, title, dialogue_list, dirname, desc=None, need=list
    ):
        _os.makedirs(dirname, exist_ok=True)
        choices = []
        for root, _directories, files in _os.walk(dirname):
            for filename in files:
                if filename.endswith(".json") and not filename.startswith("_"):
                    relative = _os.path.relpath(
                        _os.path.join(root, filename), dirname
                    )
                    choices.append(relative[:-5])
        choices.sort()
        selection = self.dialogue6widget(
            "Select Preset",
            [["Combo", "---設定ファイル選択---", choices + ["(選択して下さい)"], "(選択して下さい)"]],
        )[0]
        selected = _os.path.join(dirname, f"{selection}.json")
        widgets = _legacy_widgets(dialogue_list)
        if _os.path.isfile(selected):
            _apply_legacy_settings(widgets, _load_legacy_settings(selected))
        preset_name = Widget("Entry", "[PokeCon]設定ファイル名", "")
        preset_name._legacy_label = "[PokeCon]設定ファイル名"
        save_preset = Widget("Check", "[PokeCon]設定を保存", False)
        save_preset._legacy_label = "[PokeCon]設定を保存"
        complete = widgets + [preset_name, save_preset]
        _show_dialog(self, title, complete, True, desc)
        if save_preset.value and preset_name.value:
            _save_legacy_settings(
                _os.path.join(dirname, f"{preset_name.value}.json"),
                _legacy_result(widgets, dict),
            )
        _save_legacy_settings(
            _os.path.join(dirname, "前回の設定.json"),
            _legacy_result(widgets, dict),
        )
        return _legacy_result(widgets, need)

    def dialogue(self, title, message, desc=None, need=list):
        messages = message if isinstance(message, list) else [message]
        widgets = []
        for label in messages:
            widget = Widget("Entry", str(label), "")
            widget._legacy_label = label
            widgets.append(widget)
        _show_dialog(self, title, widgets, True, desc)
        return _legacy_result(widgets, need)

    def socket_connect(self):
        _network("socket_connect")

    def socket_disconnect(self):
        _network("socket_disconnect")

    def socket_transmit_message(self, message):
        if not isinstance(message, str):
            raise TypeError("socket message must be str")
        _network("socket_transmit", message=message)
        self.checkIfAlive()

    def socket_receive_message(self, header, show_msg=False):
        result = _network(
            "socket_receive",
            headers=[str(header)],
            show_message=bool(show_msg),
        )
        self.checkIfAlive()
        return result

    def socket_receive_message2(self, headerlist, show_msg=False):
        if not isinstance(headerlist, list) or any(
            not isinstance(header, str) for header in headerlist
        ):
            raise TypeError("socket headers must be list[str]")
        result = _network(
            "socket_receive",
            headers=headerlist,
            show_message=bool(show_msg),
        )
        self.checkIfAlive()
        return result

    def socket_change_ipaddr(self, addr):
        if not isinstance(addr, str):
            raise TypeError("socket address must be str")
        _network("socket_change_address", address=addr)

    def socket_change_port(self, port):
        if isinstance(port, bool) or not isinstance(port, int):
            raise TypeError("socket port must be int")
        if not 0 <= port <= 65535:
            raise ValueError("socket port must be between 0 and 65535")
        _network("socket_change_port", port=port)

    def socket_change_alive(self, flag):
        if not isinstance(flag, bool):
            raise TypeError("socket alive flag must be bool")
        _network("socket_change_alive", alive=flag)

    def mqtt_transmit_message(self, roomid, message):
        if not isinstance(roomid, str) or not isinstance(message, str):
            raise TypeError("MQTT room ID and message must be str")
        try:
            _network("mqtt_transmit", room_id=roomid, message=message)
        except Exception:
            self._logger.warning("MQTT transmit failed")
        self.checkIfAlive()

    def mqtt_receive_message(self, roomid, header, show_msg=False):
        if not isinstance(roomid, str) or not isinstance(header, str):
            raise TypeError("MQTT room ID and header must be str")
        try:
            result = _network(
                "mqtt_receive",
                room_id=roomid,
                headers=[header],
                show_message=bool(show_msg),
            )
        except Exception:
            self._logger.warning("MQTT receive failed")
            result = ""
        self.checkIfAlive()
        return result

    def mqtt_receive_message2(self, roomid, headerlist, show_msg=False):
        if not isinstance(roomid, str):
            raise TypeError("MQTT room ID must be str")
        if not isinstance(headerlist, list) or any(
            not isinstance(header, str) for header in headerlist
        ):
            raise TypeError("MQTT headers must be list[str]")
        try:
            result = _network(
                "mqtt_receive",
                room_id=roomid,
                headers=headerlist,
                show_message=bool(show_msg),
            )
        except Exception:
            self._logger.warning("MQTT receive failed")
            result = ""
        self.checkIfAlive()
        return result

    def mqtt_change_broker_address(self, broker_address):
        if not isinstance(broker_address, str):
            raise TypeError("MQTT broker address must be str")
        _network("mqtt_change_broker_address", broker_address=broker_address)

    def mqtt_change_id(self, mqtt_id):
        if not isinstance(mqtt_id, str):
            raise TypeError("MQTT ID must be str")
        _network("mqtt_change_id", mqtt_id=mqtt_id)

    def mqtt_change_clientId(self, clientId):
        if not isinstance(clientId, str):
            raise TypeError("MQTT client ID must be str")
        _network("mqtt_change_client_id", client_id=clientId)

    def mqtt_change_pub_token(self, pub_token):
        if not isinstance(pub_token, str):
            raise TypeError("MQTT publish token must be str")
        _network("mqtt_change_publish_token", token=pub_token)

    def mqtt_change_sub_token(self, sub_token):
        if not isinstance(sub_token, str):
            raise TypeError("MQTT subscribe token must be str")
        _network("mqtt_change_subscribe_token", token=sub_token)

    def LINE_text(self, txt, token=""):
        self._logger.warning("LINE_text is unavailable because LINE Notify has ended")

    def discord_text(self, content="", index=0, keys="DISCORD_WEBHOOK"):
        settings_key = f"DISCORD_WEBHOOK{index}" if index != 0 else str(keys)
        try:
            _notification(
                {
                    "kind": "discord_text",
                    "content": str(content),
                    "settings_key": settings_key,
                }
            )
        except Exception:
            self._logger.warning("discord_text delivery failed")


_CROP_FORMATS = {"", "1", "2", "3", "4", "11", "12", "13", "14"}
_POPUP_TARGET_BYTES = 240_000


def _json_request(operation, **values):
    return _json.dumps({"operation": operation, **values}, separators=(",", ":"))


def _require_int(value, name):
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{name} must be int")
    return value


def _convert_crop(crop_fmt="", crop=None):
    normalized = str(crop_fmt)
    if normalized not in _CROP_FORMATS:
        raise ValueError(f"unsupported crop format {crop_fmt!r}")
    if normalized == "" or crop is None or crop == []:
        return None, None
    if not isinstance(crop, (list, tuple)) or len(crop) != 4:
        raise ValueError("crop must contain exactly four integers")
    values = [_require_int(value, "crop coordinate") for value in crop]
    if normalized == "1":
        x1, y1, x2, y2 = values
    elif normalized == "2":
        x1, y1, width, height = values
        x2, y2 = x1 + width, y1 + height
    elif normalized == "3":
        x1, x2, y1, y2 = values
    elif normalized == "4":
        x1, width, y1, height = values
        x2, y2 = x1 + width, y1 + height
    elif normalized == "11":
        y1, x1, y2, x2 = values
    elif normalized == "12":
        y1, x1, height, width = values
        y2, x2 = y1 + height, x1 + width
    elif normalized == "13":
        y1, y2, x1, x2 = values
    else:
        y1, height, x1, width = values
        y2, x2 = y1 + height, x1 + width
    return (y1, y2, x1, x2), (x1, y1, x2, y2)


def _crop_image(image, crop_fmt="", crop=None):
    import numpy as np

    bounds, _ = _convert_crop(crop_fmt, crop)
    if bounds is None:
        return np.ascontiguousarray(image).copy()
    y1, y2, x1, x2 = bounds
    return np.ascontiguousarray(image[y1:y2, x1:x2]).copy()


def _validate_bgr(image):
    import numpy as np

    array = np.asarray(image)
    if array.dtype != np.uint8:
        raise TypeError("image dtype must be uint8")
    if array.ndim != 3 or array.shape[2] != 3:
        raise ValueError("image must have shape (height, width, 3)")
    if array.shape[0] == 0 or array.shape[1] == 0:
        raise ValueError("image must be non-empty")
    return np.ascontiguousarray(array).copy()


def _encode_popup(image):
    import cv2
    import numpy as np

    frame = np.ascontiguousarray(image)
    if frame.ndim == 2:
        frame = cv2.cvtColor(frame, cv2.COLOR_GRAY2BGR)
    quality = 85
    while True:
        ok, encoded = cv2.imencode(
            ".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, quality]
        )
        if not ok:
            raise RuntimeError("OpenCV could not encode popup image")
        payload = encoded.tobytes()
        if len(payload) <= _POPUP_TARGET_BYTES:
            return payload
        if frame.shape[0] <= 64 or frame.shape[1] <= 64:
            if quality > 35:
                quality -= 10
                continue
            raise ValueError("popup image cannot fit the bounded IPC payload")
        frame = cv2.resize(
            frame,
            (
                max(64, int(frame.shape[1] * 0.75)),
                max(64, int(frame.shape[0] * 0.75)),
            ),
            interpolation=cv2.INTER_AREA,
        )


def _popup_array(image, title):
    _api.popup_image(str(title), "image/jpeg", _encode_popup(image))


class Camera:
    def __init__(self, fps=45):
        _require_int(fps, "fps")
        self._fps = fps
        self._capture_size = (1280, 720)
        self._capture_resolution = "1280x720"
        self._flip = False
        self._flip_mode = 0
        self._opened = False
        self._screenshot_format = "png"
        self._refresh()

    def _apply_state(self, encoded):
        state = _json.loads(encoded)
        resolution = state["capture_resolution"]
        width, height = (int(value) for value in resolution.split("x", 1))
        mode = state["flip_mode"]
        modes = {
            "none": (False, self._flip_mode),
            "vertical": (True, 0),
            "horizontal": (True, 1),
            "both": (True, -1),
        }
        self._opened = bool(state["opened"])
        self._fps = int(state["fps"])
        self._capture_size = (width, height)
        self._capture_resolution = resolution
        self._flip, self._flip_mode = modes[mode]
        self._screenshot_format = state["screenshot_format"]
        return self

    def _refresh(self):
        return self._apply_state(_api.camera_state())

    def _control(self, operation, **values):
        return self._apply_state(
            _api.camera_control(_json_request(operation, **values))
        )

    @property
    def image_bgr(self):
        import numpy as np

        self._refresh()
        width, height, pixels = _api.camera_frame(self._capture_resolution)
        return (
            np.frombuffer(pixels, dtype=np.uint8)
            .reshape((height, width, 3))
            .copy()
        )

    def readFrame(self):
        return self.image_bgr

    def isOpened(self):
        self._refresh()
        return self._opened

    @property
    def fps(self):
        self._refresh()
        return self._fps

    @fps.setter
    def fps(self, value):
        value = _require_int(value, "fps")
        if value <= 0:
            raise ValueError("fps must be positive")
        self._control("set_fps", fps=value)

    @property
    def capture_size(self):
        self._refresh()
        return self._capture_size

    @property
    def flip(self):
        self._refresh()
        return self._flip

    @property
    def flip_mode(self):
        self._refresh()
        return self._flip_mode

    def set_flip(self, value):
        if not isinstance(value, str):
            raise TypeError("flip must be str")
        normalized = value.casefold()
        if normalized not in {"none", "vertical", "horizontal", "both"}:
            raise ValueError("flip must be None, Vertical, Horizontal, or Both")
        self._control("set_flip", mode=normalized)

    def saveCapture(
        self, filename=None, crop=None, crop_ax=None, img=None, format=None
    ):
        source = self.readFrame() if img is None else _validate_bgr(img)
        if crop_ax is None:
            crop_ax = [0, 0, source.shape[1], source.shape[0]]
        if crop is None:
            image = source
        elif crop in {1, "1"}:
            image = _crop_image(source, "1", crop_ax)
        elif crop in {2, "2"}:
            image = _crop_image(source, "2", crop_ax)
        else:
            raise ValueError("camera crop must be None, 1, or 2")
        if filename is not None and not isinstance(filename, str):
            raise TypeError("filename must be str or None")
        if format is None:
            self._refresh()
            format = self._screenshot_format
        normalized_format = str(format).casefold()
        if normalized_format not in {"png", "jpeg"}:
            raise ValueError("format must be png or jpeg")
        image = _validate_bgr(image)
        saved = _api.save_image(
            image.shape[1],
            image.shape[0],
            image.tobytes(),
            filename or None,
            normalized_format,
        )
        print(f"capture succeeded: {saved}")

    def openCamera(self, cameraId):
        if isinstance(cameraId, bool) or not isinstance(cameraId, (int, str)):
            raise TypeError("cameraId must be int or str")
        if isinstance(cameraId, int) and cameraId < 0:
            raise ValueError("camera index must be non-negative")
        if isinstance(cameraId, str) and not cameraId:
            raise ValueError("camera selector must be non-empty")
        self._control("open", selector=cameraId)

    def destroy(self):
        self._control("destroy")

    def camera_thread_start(self):
        self._control("thread_start")

    def camera_thread_stop(self):
        self._control("thread_stop")

    def camera_update(self):
        self._control("update")


class CaptureArea:
    def __init__(self, camera=None):
        self.camera = camera if camera is not None else Camera()
        self._show_size = (720, 1280)
        self._is_show_var = True
        self._rectangles = {}
        self._texts = {}
        self._fps = 45
        self.right_mouse_mode = "Default"
        self.touchscreen_area = (0.0, 0.0, 1.0, 1.0)
        self._range_start = None
        self._range_end = None
        self._pointer_radius = 60.0
        self._left_origin = None
        self._right_origin = None

    @property
    def show_size(self):
        return self._show_size

    @property
    def is_show_var(self):
        return self._is_show_var

    def ImgRect(self, x1, y1, x2, y2, outline, tag, ms, flag=True):
        values = [_require_int(value, "rectangle coordinate") for value in (x1, y1, x2, y2)]
        duration = _require_int(ms, "ms")
        if duration < 0:
            raise ValueError("ms must be non-negative")
        request = {
            "operation": "rectangle",
            "x1": values[0],
            "y1": values[1],
            "x2": values[2],
            "y2": values[3],
            "outline": str(outline),
            "tag": str(tag),
            "expires_ms": duration if bool(flag) else None,
        }
        _api.overlay(_json.dumps(request, separators=(",", ":")))
        self._rectangles[str(tag)] = request

    def ImgText(
        self,
        x1,
        y1,
        txt,
        tag,
        ms,
        ft=("UD デジタル 教科書体 NP-B", 20),
        color="black",
        flag=True,
    ):
        x1 = _require_int(x1, "text x")
        y1 = _require_int(y1, "text y")
        duration = _require_int(ms, "ms")
        if duration < 0:
            raise ValueError("ms must be non-negative")
        if not isinstance(ft, tuple) or len(ft) != 2:
            raise TypeError("font must be a (name, size) tuple")
        font_size = _require_int(ft[1], "font size")
        if font_size <= 0:
            raise ValueError("font size must be positive")
        request = {
            "operation": "text",
            "x": x1,
            "y": y1,
            "text": str(txt),
            "tag": str(tag),
            "expires_ms": duration if bool(flag) else None,
            "font": str(ft[0]),
            "font_size": font_size,
            "color": str(color),
        }
        _api.overlay(_json.dumps(request, separators=(",", ":")))
        self._texts[str(tag)] = request

    def deleteImageRect(self, tag):
        _api.overlay(_json_request("delete_rectangle", tag=str(tag)))
        self._rectangles.pop(str(tag), None)

    def deleteImageText(self, tag):
        _api.overlay(_json_request("delete_text", tag=str(tag)))
        self._texts.pop(str(tag), None)

    def setFps(self, fps):
        if isinstance(fps, bool) or not isinstance(fps, (str, int)):
            raise TypeError("fps must be str or int")
        try:
            value = int(fps)
        except ValueError as error:
            raise ValueError("fps must be an integer") from error
        if value <= 0:
            raise ValueError("fps must be positive")
        _api.overlay(_json_request("set_fps", fps=value))
        self._fps = value

    def setShowsize(self, show_height, show_width):
        height = _require_int(show_height, "show_height")
        width = _require_int(show_width, "show_width")
        if height <= 0 or width <= 0:
            raise ValueError("show size must be positive")
        _api.overlay(_json_request("set_show_size", height=height, width=width))
        self._show_size = (height, width)

    def changeRightMouseMode(self, mode):
        if not isinstance(mode, str):
            raise TypeError("right mouse mode must be str")
        _api.overlay(_json_request("set_right_mouse_mode", mode=mode))
        self.right_mouse_mode = mode

    def setTouchscreenArea(self, x1, y1, x2, y2):
        values = [_require_int(value, "touchscreen coordinate") for value in (x1, y1, x2, y2)]
        height, width = self._show_size
        if height <= 0 or width <= 0:
            raise ValueError("show size must be positive")
        left, right = sorted((max(0, min(width, values[0])), max(0, min(width, values[2]))))
        top, bottom = sorted((max(0, min(height, values[1])), max(0, min(height, values[3]))))
        if left == right or top == bottom:
            raise ValueError("touchscreen area must be non-degenerate")
        normalized = (left / width, top / height, right / width, bottom / height)
        _api.overlay(
            _json_request(
                "set_touchscreen_area",
                left=normalized[0],
                top=normalized[1],
                right=normalized[2],
                bottom=normalized[3],
            )
        )
        self.touchscreen_area = normalized

    def saveCapture(self):
        self.camera.saveCapture()

    def update(self):
        _api.overlay(_json_request("update"))

    def _binding(self, button, enabled):
        _api.overlay(
            _json_request("set_binding", button=button, enabled=bool(enabled))
        )

    def BindLeftClick(self):
        self._binding("left", True)

    def BindRightClick(self):
        self._binding("right", True)

    def UnbindLeftClick(self):
        self._binding("left", False)

    def UnbindRightClick(self):
        self._binding("right", False)

    def mouseCtrlLeftPress(self, event):
        x, y = _require_int(event.x, "event.x"), _require_int(event.y, "event.y")
        height, width = self._show_size
        capture_width, capture_height = self.camera.capture_size
        source_x = max(0, min(capture_width - 1, int(x * capture_width / width)))
        source_y = max(0, min(capture_height - 1, int(y * capture_height / height)))
        b, g, r = self.camera.image_bgr[source_y, source_x]
        print(f"Color [R: {int(r)}, G: {int(g)}, B: {int(b)}]")

    def mouseLeftPress(self, event, keys_):
        self._left_origin = (
            _require_int(event.x, "event.x"),
            _require_int(event.y, "event.y"),
        )

    def mouseLeftPressing(self, event, keys_):
        if self._left_origin is None:
            self.mouseLeftPress(event, keys_)
        keys_.input(self._pointer_stick(event, self._left_origin, Stick.LEFT))

    def mouseLeftRelease(self, keys_):
        self._left_origin = None
        keys_.input(Direction(Stick.LEFT, (128, 128)))

    def mouseRightPress(self, event, keys_):
        if self.right_mouse_mode == "Qingpi":
            self._pointer_touch(event, keys_)
            return
        self._right_origin = (
            _require_int(event.x, "event.x"),
            _require_int(event.y, "event.y"),
        )

    def mouseRightPressing(self, event, keys_):
        if self.right_mouse_mode == "Qingpi":
            self._pointer_touch(event, keys_)
            return
        if self._right_origin is None:
            self.mouseRightPress(event, keys_)
        keys_.input(self._pointer_stick(event, self._right_origin, Stick.RIGHT))

    def mouseRightRelease(self, keys_):
        if self.right_mouse_mode == "Qingpi":
            keys_.inputEnd(Touchscreen(0, 0))
        else:
            keys_.input(Direction(Stick.RIGHT, (128, 128)))
        self._right_origin = None

    def _pointer_stick(self, event, origin, stick):
        x = _require_int(event.x, "event.x")
        y = _require_int(event.y, "event.y")
        dx, dy = x - origin[0], y - origin[1]
        distance = _math.hypot(dx, dy)
        scale = 1.0 if distance <= self._pointer_radius else self._pointer_radius / distance
        return Direction(
            stick,
            (
                round(128 + dx * scale * 127 / self._pointer_radius),
                round(128 - dy * scale * 127 / self._pointer_radius),
            ),
        )

    def _pointer_touch(self, event, keys_):
        x = _require_int(event.x, "event.x")
        y = _require_int(event.y, "event.y")
        left, top, right, bottom = self.touchscreen_area
        height, width = self._show_size
        normalized_x = x / max(1, width)
        normalized_y = y / max(1, height)
        if not (left <= normalized_x <= right and top <= normalized_y <= bottom):
            return
        touch_x = min(319, max(0, int(320 * (normalized_x - left) / (right - left))))
        touch_y = min(239, max(0, int(240 * (normalized_y - top) / (bottom - top))))
        keys_.input(Touchscreen(touch_x, touch_y))

    def StartRangeSS(self, event):
        self._range_start = (_require_int(event.x, "event.x"), _require_int(event.y, "event.y"))
        self._range_end = self._range_start

    def MotionRangeSS(self, event):
        height, width = self._show_size
        self._range_end = (
            max(0, min(width, _require_int(event.x, "event.x"))),
            max(0, min(height, _require_int(event.y, "event.y"))),
        )

    def ReleaseRangeSS(self, event):
        self.MotionRangeSS(event)
        if self._range_start is None or self._range_end is None:
            raise RuntimeError("range screenshot has not started")
        height, width = self._show_size
        capture_width, capture_height = self.camera.capture_size
        left, right = sorted((self._range_start[0], self._range_end[0]))
        top, bottom = sorted((self._range_start[1], self._range_end[1]))
        if left == right or top == bottom:
            raise ValueError("screenshot range must be non-degenerate")
        self.camera.saveCapture(
            crop=1,
            crop_ax=[
                int(left * capture_width / width),
                int(top * capture_height / height),
                int(right * capture_width / width),
                int(bottom * capture_height / height),
            ],
        )


_tk_id_lock = _threading.Lock()
_tk_next_id = 1
_tk_objects = {}
_tk_windows = {}
_tk_callback_queue = _queue.Queue()
_tk_callback_generation = 0
_tk_callback_shutdown = False


def _tk_id():
    global _tk_next_id
    with _tk_id_lock:
        value = _tk_next_id
        _tk_next_id += 1
    return value


def _tk_request(operation, **values):
    response = _api.tk(_json_request(operation, **values))
    return _json.loads(response)


def _tk_reject_options(owner, options):
    if options:
        name = sorted(options)[0]
        raise NotImplementedError(f"tkinter.{owner} option {name!r} is not implemented")


def _tk_require_window(master):
    if not isinstance(master, _TkToplevel) or master._destroyed:
        raise TypeError("Tk widget parent must be a live tkinter.Toplevel")
    return master


def _tk_callback_main():
    while True:
        item = _tk_callback_queue.get()
        if item is None:
            return
        generation, callback, arguments = item
        if generation != _tk_callback_generation:
            continue
        try:
            callback(*arguments)
        except BaseException:
            _traceback.print_exc(file=_sys.stderr)


_tk_callback_thread = _threading.Thread(
    target=_tk_callback_main,
    name="pokecon-tk-callback",
    daemon=True,
)
_tk_callback_thread.start()


def _tk_queue_callback(callback, *arguments):
    if callback is None or _tk_callback_shutdown:
        return
    _tk_callback_queue.put((_tk_callback_generation, callback, arguments))


class _TkObject:
    def __init__(self, master):
        self.master = _tk_require_window(master)
        self._id = _tk_id()
        self._destroyed = False
        self._packed = False
        _tk_objects[self._id] = self
        self.master._children.add(self._id)

    def _ensure_live(self):
        if self._destroyed or self.master._destroyed:
            raise RuntimeError("Tk compatibility object has been destroyed")

    def pack(self, **options):
        self._ensure_live()
        _tk_reject_options(type(self).__name__.removeprefix("_Tk"), set(options) - {"pady"})
        pady = options.get("pady")
        if pady is not None:
            pady = _require_int(pady, "pack pady")
        _tk_request("pack", widget_id=self._id, pady=pady)
        self._packed = True

    def __getattr__(self, name):
        raise NotImplementedError(
            f"tkinter.{type(self).__name__.removeprefix('_Tk')}.{name} is not implemented"
        )


class _TkToplevel:
    def __init__(self, master=None, **options):
        _tk_reject_options("Toplevel", options)
        if not isinstance(master, CaptureArea):
            raise TypeError("tkinter.Toplevel parent must be CaptureArea")
        self.master = master
        self._id = _tk_id()
        self._destroyed = False
        self._children = set()
        _tk_windows[self._id] = self
        _tk_request("create_toplevel", window_id=self._id)

    def _ensure_live(self):
        if self._destroyed:
            raise RuntimeError("Tk compatibility window has been destroyed")

    def title(self, value):
        self._ensure_live()
        if not isinstance(value, str):
            raise TypeError("Toplevel title must be str")
        _tk_request("set_title", window_id=self._id, title=value)

    def geometry(self, value):
        self._ensure_live()
        if not isinstance(value, str):
            raise TypeError("Toplevel geometry must be str")
        _tk_request("set_geometry", window_id=self._id, geometry=value)

    def destroy(self):
        if self._destroyed:
            return
        _tk_request("destroy_window", window_id=self._id)
        _tk_destroy_window(self._id)

    def __getattr__(self, name):
        raise NotImplementedError(f"tkinter.Toplevel.{name} is not implemented")


class _TkScale(_TkObject):
    def __init__(
        self,
        master,
        from_=0,
        to=100,
        orient="vertical",
        label=None,
        command=None,
        **options,
    ):
        _tk_reject_options("Scale", options)
        super().__init__(master)
        if isinstance(from_, bool) or not isinstance(from_, (int, float)):
            raise TypeError("Scale from_ must be numeric")
        if isinstance(to, bool) or not isinstance(to, (int, float)):
            raise TypeError("Scale to must be numeric")
        if orient not in {"horizontal", "vertical"}:
            raise ValueError("Scale orient must be horizontal or vertical")
        if label is not None and not isinstance(label, str):
            raise TypeError("Scale label must be str or None")
        if command is not None and not callable(command):
            raise TypeError("Scale command must be callable or None")
        self._from = float(from_)
        self._to = float(to)
        self._value = self._from
        self._command = command
        _tk_request(
            "create_scale",
            window_id=self.master._id,
            widget_id=self._id,
            from_value=self._from,
            to_value=self._to,
            orient=orient,
            label=label,
        )

    def _normalize(self, value):
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise TypeError("Scale value must be numeric")
        low, high = sorted((self._from, self._to))
        return max(low, min(high, float(value)))

    def set(self, value):
        self._ensure_live()
        self._value = self._normalize(value)
        _tk_request("set_scale", widget_id=self._id, value=self._value)

    def get(self):
        self._ensure_live()
        result = _tk_request("get_scale", widget_id=self._id)
        if result["value"] is None:
            raise RuntimeError("Tk host returned no Scale value")
        self._value = self._normalize(result["value"])
        return self._value


class _TkButton(_TkObject):
    def __init__(self, master, text="", command=None, **options):
        _tk_reject_options("Button", options)
        super().__init__(master)
        if not isinstance(text, str):
            raise TypeError("Button text must be str")
        if command is not None and not callable(command):
            raise TypeError("Button command must be callable or None")
        self._command = command
        _tk_request(
            "create_button",
            window_id=self.master._id,
            widget_id=self._id,
            text=text,
        )

    def invoke(self):
        self._ensure_live()
        _tk_queue_callback(self._command)


class _TkLabel(_TkObject):
    def __init__(
        self,
        master,
        text="",
        width=None,
        height=None,
        relief=None,
        bg=None,
        **options,
    ):
        _tk_reject_options("Label", options)
        super().__init__(master)
        if not isinstance(text, str):
            raise TypeError("Label text must be str")
        width = None if width is None else _require_int(width, "Label width")
        height = None if height is None else _require_int(height, "Label height")
        if relief is not None and not isinstance(relief, str):
            raise TypeError("Label relief must be str or None")
        if bg is not None and not isinstance(bg, str):
            raise TypeError("Label bg must be str or None")
        self._text = text
        self._background = bg
        _tk_request(
            "create_label",
            window_id=self.master._id,
            widget_id=self._id,
            text=text,
            width=width,
            height=height,
            relief=relief,
            background=bg,
        )

    def config(self, **options):
        self._ensure_live()
        _tk_reject_options("Label.config", set(options) - {"text", "bg", "background"})
        if "bg" in options and "background" in options:
            raise TypeError("Label background was provided twice")
        text = options.get("text")
        background = options.get("bg", options.get("background"))
        if text is not None and not isinstance(text, str):
            raise TypeError("Label text must be str")
        if background is not None and not isinstance(background, str):
            raise TypeError("Label background must be str")
        _tk_request(
            "configure_label",
            widget_id=self._id,
            text=text,
            background=background,
        )
        if text is not None:
            self._text = text
        if background is not None:
            self._background = background

    configure = config


def _tk_destroy_window(window_id):
    window = _tk_windows.pop(window_id, None)
    if window is None:
        return
    window._destroyed = True
    for widget_id in tuple(window._children):
        widget = _tk_objects.pop(widget_id, None)
        if widget is not None:
            widget._destroyed = True
    window._children.clear()


def _tk_event(encoded):
    event = _json.loads(encoded)
    kind = event["event"]
    if kind == "window_closed":
        _tk_destroy_window(event["window_id"])
        return
    widget = _tk_objects.get(event["widget_id"])
    if widget is None or widget._destroyed:
        return
    if kind == "scale_changed" and isinstance(widget, _TkScale):
        widget._value = widget._normalize(event["value"])
        _tk_queue_callback(widget._command, str(widget._value))
    elif kind == "button_invoked" and isinstance(widget, _TkButton):
        _tk_queue_callback(widget._command)


def _pointer_event(encoded):
    event = _json.loads(encoded)
    command = _active_command
    if not isinstance(command, ImageProcPythonCommand):
        return
    area = command.gui
    pointer = _types.SimpleNamespace(x=int(event["x"]), y=int(event["y"]))
    button = event["button"]
    phase = event["phase"]
    if button == "left":
        if phase == "pressed":
            _tk_queue_callback(area.mouseLeftPress, pointer, command.keys)
        elif phase == "moved":
            _tk_queue_callback(area.mouseLeftPressing, pointer, command.keys)
        elif phase == "released":
            _tk_queue_callback(area.mouseLeftRelease, command.keys)
    elif button == "right":
        if phase == "pressed":
            _tk_queue_callback(area.mouseRightPress, pointer, command.keys)
        elif phase == "moved":
            _tk_queue_callback(area.mouseRightPressing, pointer, command.keys)
        elif phase == "released":
            _tk_queue_callback(area.mouseRightRelease, command.keys)


def _reset_tk():
    global _tk_callback_generation
    _tk_callback_generation += 1
    for window_id in tuple(_tk_windows):
        _tk_destroy_window(window_id)
    while True:
        try:
            _tk_callback_queue.get_nowait()
        except _queue.Empty:
            break


def _shutdown_tk():
    global _tk_callback_shutdown
    if _tk_callback_shutdown:
        return
    _tk_callback_shutdown = True
    _reset_tk()
    _tk_callback_queue.put(None)
    _tk_callback_thread.join(timeout=1.0)


def _unsupported_tk(name):
    def construct(*args, **kwargs):
        raise NotImplementedError(f"tkinter.{name} is not implemented")

    return construct


tkinter_module = _types.ModuleType("tkinter")
tkinter_module.__path__ = []
tkinter_module.__all__ = [
    "Toplevel",
    "Scale",
    "Button",
    "Label",
    "HORIZONTAL",
    "VERTICAL",
]
tkinter_module.Toplevel = _TkToplevel
tkinter_module.Scale = _TkScale
tkinter_module.Button = _TkButton
tkinter_module.Label = _TkLabel
tkinter_module.Tk = _unsupported_tk("Tk")
tkinter_module.HORIZONTAL = "horizontal"
tkinter_module.VERTICAL = "vertical"
tkinter_module.SOLID = "solid"


def _tk_module_missing(name):
    raise NotImplementedError(f"tkinter.{name} is not implemented")


tkinter_module.__getattr__ = _tk_module_missing
filedialog_module = _types.ModuleType("tkinter.filedialog")


def _unsupported_filedialog(*args, **kwargs):
    raise NotImplementedError("tkinter.filedialog is not implemented")


filedialog_module.asksaveasfilename = _unsupported_filedialog
filedialog_module.askopenfilename = _unsupported_filedialog
tkinter_module.filedialog = filedialog_module
_sys.modules["tkinter"] = tkinter_module
_sys.modules["tkinter.filedialog"] = filedialog_module


def _load_image(value, binary=False):
    import cv2
    import numpy as np

    if isinstance(value, np.ndarray):
        return np.ascontiguousarray(value).copy()
    if not isinstance(value, str):
        raise TypeError("image path must be str or ndarray")
    mode = cv2.IMREAD_GRAYSCALE if binary else cv2.IMREAD_COLOR
    image = cv2.imread(value, mode)
    return None if image is None else np.ascontiguousarray(image).copy()


def _preprocess(image, use_gray, crop_fmt, crop, bgr_range, threshold_binary):
    import cv2
    import numpy as np

    processed = _crop_image(image, crop_fmt, crop)
    if processed.size == 0:
        raise ValueError("crop produced an empty image")
    if use_gray and processed.ndim == 3:
        processed = cv2.cvtColor(processed, cv2.COLOR_BGR2GRAY)
    elif bgr_range is not None:
        if not isinstance(bgr_range, dict) or set(bgr_range) != {"lower", "upper"}:
            raise ValueError("BGR_range must contain only lower and upper")
        processed = cv2.inRange(
            processed,
            np.asarray(bgr_range["lower"], dtype=np.uint8),
            np.asarray(bgr_range["upper"], dtype=np.uint8),
        )
    if threshold_binary is not None:
        threshold_binary = _require_int(threshold_binary, "threshold_binary")
        if not 0 <= threshold_binary <= 255:
            raise ValueError("threshold_binary must be between 0 and 255")
        _, processed = cv2.threshold(
            processed, threshold_binary, 255, cv2.THRESH_BINARY
        )
    return np.ascontiguousarray(processed)


def _match_template(
    source,
    template,
    *,
    threshold,
    use_gray,
    crop_fmt,
    crop,
    mask,
    bgr_range,
    threshold_binary,
    crop_template,
    show_image,
):
    import cv2
    import math

    source = _preprocess(
        source, use_gray, crop_fmt, crop, bgr_range, threshold_binary
    )
    template = _preprocess(
        template,
        use_gray,
        crop_fmt,
        crop_template,
        bgr_range,
        threshold_binary,
    )
    if source.shape[0] < template.shape[0] or source.shape[1] < template.shape[1]:
        raise ValueError("template must not be larger than the search image")
    prepared_mask = None
    if mask is not None:
        prepared_mask = _crop_image(mask, crop_fmt, crop_template)
        if prepared_mask.ndim == 3:
            prepared_mask = cv2.cvtColor(prepared_mask, cv2.COLOR_BGR2GRAY)
        if prepared_mask.shape[:2] != template.shape[:2]:
            raise ValueError("mask dimensions must match the template")
    method = (
        cv2.TM_CCORR_NORMED
        if prepared_mask is not None
        else cv2.TM_CCOEFF_NORMED
    )
    result = cv2.matchTemplate(source, template, method, mask=prepared_mask)
    _, score, _, location = cv2.minMaxLoc(result)
    if not math.isfinite(score):
        score = float("-inf")
    if show_image:
        _popup_array(source, "template matching source")
    return (
        score > float(threshold),
        tuple(int(value) for value in location),
        int(template.shape[1]),
        int(template.shape[0]),
        float(score),
    )


class ImageProcPythonCommand(PythonCommand):
    camera = None
    cam = None
    gui = None
    canvas = None

    def __init__(self, cam, gui=None):
        super().__init__()
        if not isinstance(cam, Camera):
            raise TypeError("cam must be Camera")
        self.camera = cam
        self.cam = cam
        self.gui = gui if gui is not None else CaptureArea(cam)
        self.canvas = self.gui
        self.template_path_name = _os.path.join(_api.command_root(), "Template")
        self.isSimilarity = False
        self.isGuide = False

    def discord_image(self, content="", index=0, crop_fmt="", crop=None, keys="DISCORD_WEBHOOK"):
        _convert_crop(crop_fmt, crop)
        if index != 0:
            settings_keys = [f"DISCORD_WEBHOOK{index}"]
        elif isinstance(keys, str):
            settings_keys = [keys]
        elif isinstance(keys, list) and all(isinstance(key, str) for key in keys):
            settings_keys = list(keys)
        else:
            raise TypeError("discord_image keys must be str or list[str]")
        normalized_crop = None if crop is None else [_require_int(value, "crop coordinate") for value in crop]
        try:
            encoded = _encode_popup(self.getCameraImage(crop_fmt, crop))
            _notification(
                {
                    "kind": "discord_image",
                    "content": str(content),
                    "settings_keys": settings_keys,
                    "crop_format": str(crop_fmt),
                    "crop": normalized_crop,
                    "content_type": "image/jpeg",
                    "encoded": list(encoded),
                }
            )
        except Exception:
            self._logger.warning("discord_image delivery failed")

    def LINE_image(self, txt, crop_fmt="", crop=None, token=""):
        self._logger.warning("LINE_image is unavailable because LINE Notify has ended")

    def _template_image(self, value, binary=False):
        path = value if not isinstance(value, str) else self.get_filespec(value, "t")
        image = _load_image(path, binary=binary)
        if image is None:
            raise ValueError(f"image could not be loaded from {value!r}")
        return image

    def _display_match(
        self,
        matched,
        location,
        width,
        height,
        *,
        show_position,
        show_only_true_rect,
        ms,
        color,
        crop_fmt,
        crop,
    ):
        if not show_position or (not matched and show_only_true_rect):
            return
        colors = ["blue", "red", "orange"] if color is None else color
        if not isinstance(colors, list) or len(colors) < 2:
            raise ValueError("color must contain at least two entries")
        _, pillow = _convert_crop(crop_fmt, crop)
        x, y = location
        if pillow is not None:
            x += pillow[0]
            y += pillow[1]
        outline = colors[0] if matched else colors[1]
        self.displayRectangle(
            (x, y),
            width,
            height,
            ms=ms,
            color=[outline, colors[-1]],
        )

    def isContainTemplate(
        self,
        template_path,
        threshold=0.7,
        use_gray=True,
        show_value=False,
        show_position=True,
        show_only_true_rect=True,
        ms=2000,
        crop_fmt="",
        crop=None,
        mask_path=None,
        use_gpu=False,
        BGR_range=None,
        threshold_binary=None,
        crop_template=None,
        show_image=False,
        color=None,
    ):
        template = self._template_image(template_path)
        mask = None if mask_path is None else self._template_image(mask_path, binary=True)
        matched, location, width, height, score = _match_template(
            self.camera.readFrame(),
            template,
            threshold=threshold,
            use_gray=bool(use_gray),
            crop_fmt=crop_fmt,
            crop=crop,
            mask=mask,
            bgr_range=BGR_range,
            threshold_binary=threshold_binary,
            crop_template=crop_template,
            show_image=bool(show_image),
        )
        if show_value or self.isSimilarity:
            mode = "NCC" if mask is not None else "ZNCC"
            print(f"{template_path} {mode} value: {score}")
        self._display_match(
            matched,
            location,
            width,
            height,
            show_position=bool(show_position),
            show_only_true_rect=bool(show_only_true_rect),
            ms=ms,
            color=color,
            crop_fmt=crop_fmt,
            crop=crop,
        )
        return matched

    def isContainTemplate_max(
        self,
        template_path_list,
        threshold=0.7,
        use_gray=True,
        show_value=False,
        show_position=True,
        show_only_true_rect=True,
        ms=2000,
        crop_fmt="",
        crop=None,
        mask_path_list=None,
        BGR_range=None,
        threshold_binary=None,
        crop_template=None,
        show_image=False,
        color=None,
    ):
        if not isinstance(template_path_list, list) or not template_path_list:
            raise ValueError("template_path_list must be a non-empty list")
        if mask_path_list is None or mask_path_list == []:
            masks = [None] * len(template_path_list)
        elif not isinstance(mask_path_list, list) or len(mask_path_list) != len(template_path_list):
            return -1, [], []
        else:
            masks = [
                None if value is None else self._template_image(value, binary=True)
                for value in mask_path_list
            ]
        source = self.camera.readFrame()
        matches = []
        for template_path, mask in zip(template_path_list, masks, strict=True):
            result = _match_template(
                source,
                self._template_image(template_path),
                threshold=threshold,
                use_gray=bool(use_gray),
                crop_fmt=crop_fmt,
                crop=crop,
                mask=mask,
                bgr_range=BGR_range,
                threshold_binary=threshold_binary,
                crop_template=crop_template,
                show_image=bool(show_image),
            )
            matches.append(result)
        scores = [result[4] for result in matches]
        best = max(range(len(scores)), key=scores.__getitem__)
        judgments = [result[0] for result in matches]
        if show_value or self.isSimilarity:
            for path, score in zip(template_path_list, scores, strict=True):
                print(f"{path} template value: {score}")
        matched, location, width, height, _ = matches[best]
        self._display_match(
            matched or any(judgments),
            location,
            width,
            height,
            show_position=bool(show_position),
            show_only_true_rect=bool(show_only_true_rect),
            ms=ms,
            color=color,
            crop_fmt=crop_fmt,
            crop=crop,
        )
        return best, scores, judgments

    def isContainTemplateGPU(
        self,
        template_path,
        threshold=0.7,
        use_gray=True,
        show_value=False,
        show_position=True,
        show_only_true_rect=True,
        ms=2000,
        crop_fmt="",
        crop=None,
        mask_path=None,
        BGR_range=None,
        threshold_binary=None,
        crop_template=None,
        show_image=False,
        color=None,
    ):
        return self.isContainTemplate(
            template_path,
            threshold,
            use_gray,
            show_value,
            show_position,
            show_only_true_rect,
            ms,
            crop_fmt,
            crop,
            mask_path,
            True,
            BGR_range,
            threshold_binary,
            crop_template,
            show_image,
            color,
        )

    def isContainedImage(
        self,
        image_path,
        threshold=0.7,
        use_gray=True,
        show_value=False,
        show_position=True,
        show_only_true_rect=True,
        ms=2000,
        crop_fmt="",
        crop=None,
        mask_path=None,
        use_gpu=False,
        BGR_range=None,
        threshold_binary=None,
        crop_template=None,
        show_image=False,
        color=None,
    ):
        image = self._template_image(image_path)
        mask = None if mask_path is None else self._template_image(mask_path, binary=True)
        matched, location, width, height, score = _match_template(
            image,
            self.camera.readFrame(),
            threshold=threshold,
            use_gray=bool(use_gray),
            crop_fmt=crop_fmt,
            crop=crop,
            mask=mask,
            bgr_range=BGR_range,
            threshold_binary=threshold_binary,
            crop_template=crop_template,
            show_image=bool(show_image),
        )
        if show_value or self.isSimilarity:
            print(f"capture_image template value: {score}")
        self._display_match(
            matched,
            location,
            width,
            height,
            show_position=bool(show_position),
            show_only_true_rect=bool(show_only_true_rect),
            ms=ms,
            color=color,
            crop_fmt=crop_fmt,
            crop=crop,
        )
        return matched

    def saveCapture(self, filename=None, crop_fmt="", crop=None, mode=True, format=None):
        image = self.getCameraImage(crop_fmt, crop)
        self.camera.saveCapture(filename=filename, img=image, format=format)

    def popupImage(self, crop_fmt="", crop=None, title="image"):
        _popup_array(self.getCameraImage(crop_fmt, crop), title)

    def getCameraImage(self, crop_fmt="", crop=None):
        return _crop_image(self.camera.image_bgr, crop_fmt, crop)

    def openImage(self, filename, mode="t"):
        if not isinstance(filename, str):
            raise TypeError("filename must be str")
        return _load_image(self.get_filespec(filename, mode))

    def setTemplateDir(self, path):
        if not isinstance(path, str):
            raise TypeError("template path must be str")
        self.template_path_name = (
            path if _os.path.isabs(path) else _os.path.abspath(_os.path.join(_api.command_root(), path))
        )

    def get_filespec(self, filename, mode="t"):
        if not isinstance(filename, str):
            raise TypeError("filename must be str")
        if _os.path.isabs(filename):
            return filename
        if mode == "t":
            base = self.template_path_name
        elif mode == "c":
            base = _os.path.join(_api.data_root(), "Captures")
        else:
            base = _api.command_root()
        return _os.path.abspath(_os.path.join(base, filename))

    def displayRectangle(self, max_loc, width, height, tag=None, ms=2000, color=None, crop_fmt="", crop=None):
        if not isinstance(max_loc, (list, tuple)) or len(max_loc) != 2:
            raise ValueError("max_loc must contain two coordinates")
        x = _require_int(max_loc[0], "max_loc x")
        y = _require_int(max_loc[1], "max_loc y")
        width = _require_int(width, "width")
        height = _require_int(height, "height")
        if width <= 0 or height <= 0:
            raise ValueError("rectangle size must be positive")
        colors = ["blue", "orange"] if color is None else color
        if not isinstance(colors, list) or not colors:
            raise ValueError("color must be a non-empty list")
        tag = tag or f"rect-{_time.time_ns()}-{id(max_loc)}"
        _, pillow = _convert_crop(crop_fmt, crop)
        if pillow is not None and len(colors) > 1:
            self.gui.ImgRect(*pillow, str(colors[1]), tag, int(ms), False)
        self.gui.ImgRect(
            x,
            y,
            x + width + 1,
            y + height + 1,
            str(colors[0]),
            tag,
            int(ms),
            True,
        )

    def displayText(self, position, txt, tag=None, ms=2000, font="UD デジタル 教科書体 NP-B", fontsize=20, color="black"):
        if not isinstance(position, (list, tuple)) or len(position) != 2:
            raise ValueError("position must contain two coordinates")
        self.gui.ImgText(
            _require_int(position[0], "position x"),
            _require_int(position[1], "position y"),
            str(txt),
            tag or f"text-{_time.time_ns()}-{id(position)}",
            int(ms),
            (str(font), _require_int(fontsize, "fontsize")),
            str(color),
            True,
        )


class McuCommand(Command):
    def __init__(self, sync_name):
        super().__init__()
        self.sync_name = sync_name
        self.postProcess = None
        self.isRunning = False

    def start(self, ser, postProcess):
        self.postProcess = postProcess
        ser.writeRow(self.sync_name)
        self.isRunning = True

    def end(self, ser):
        ser.writeRow("end")
        self.isRunning = False
        if self.postProcess is not None:
            self.postProcess()


class BridgeFunctions:
    def __init__(self, commands):
        self.commands = commands
        self.is_extension = True

    def check_pokecon_extension(self):
        return True

    def get_profile_name(self):
        return _api.profile_name()

    def set_template_directory(self, name1, name2):
        path = _os.path.join(name1, name2)
        if _os.path.isdir(path):
            self.commands.setTemplateDir(name1 + "/")

    def get_template_directory(self):
        return getattr(self.commands, "template_path_name", "./Template/")

    def bf_print(self, *objects, sep=" ", end="\n"):
        self.commands.print_tbs("a", *objects, sep=sep, end=end)

    def bf_print_w(self, *objects, sep=" ", end="\n"):
        self.commands.print_tb("w", *objects, sep=sep, end=end)

    def bf_print_a(self, *objects, sep=" ", end="\n"):
        self.commands.print_tb("a", *objects, sep=sep, end=end)

    def bf_isContainTemplate(self, *args, **kwargs):
        return self.commands.isContainTemplate(*args, **kwargs)

    def bf_isContainTemplate_max(self, *args, **kwargs):
        return self.commands.isContainTemplate_max(*args, **kwargs)

    def bf_dialogue(self, *args, **kwargs):
        return self.commands.dialogue(*args, **kwargs)

    def bf_dialogue6widget(self, *args, **kwargs):
        return self.commands.dialogue6widget(*args, **kwargs)

    def bf_dialogue6widget_save_settings(self, *args, **kwargs):
        return self.commands.dialogue6widget_save_settings(*args, **kwargs)

    def bf_dialogue6widget_select_settings(self, *args, **kwargs):
        return self.commands.dialogue6widget_select_settings(*args, **kwargs)

    def bf_show_informations(self, name, developer, contributor=None, description=None):
        self.commands.print_s(name, developer, contributor or "", description or "")


for _bridge_name, _command_name in {
    "bf_isContainTemplate": "isContainTemplate",
    "bf_isContainTemplate_max": "isContainTemplate_max",
    "bf_dialogue": "dialogue",
    "bf_dialogue6widget": "dialogue6widget",
    "bf_dialogue6widget_save_settings": "dialogue6widget_save_settings",
    "bf_dialogue6widget_select_settings": "dialogue6widget_select_settings",
}.items():
    getattr(BridgeFunctions, _bridge_name).__signature__ = _inspect.signature(
        getattr(ImageProcPythonCommand, _command_name)
    )


_current = _threading.local()
_active_command = None


def _command():
    command = getattr(_current, "command", None)
    if command is None:
        raise RuntimeError("Commands module function requires a running command context")
    return command


def _image_command():
    command = _command()
    if not isinstance(command, ImageProcPythonCommand):
        raise RuntimeError("image_proc requires ImageProcPythonCommand context")
    return command


def _forward(name, image=False):
    def call(*args, **kwargs):
        command = _image_command() if image else _command()
        return getattr(command, name)(*args, **kwargs)

    call.__name__ = name
    method = getattr(ImageProcPythonCommand if image else PythonCommand, name)
    parameters = tuple(_inspect.signature(method).parameters.values())[1:]
    call.__signature__ = _inspect.Signature(parameters)
    return call


def _module(name, package=False):
    module = _types.ModuleType(name)
    if package:
        module.__path__ = []
    _sys.modules[name] = module
    return module


Commands = _module("Commands", True)
meta_module = _module("Commands._meta")
command_module = _module("Commands.CommandBase")
keys_module = _module("Commands.Keys")
sender_module = _module("Commands.Sender")
python_module = _module("Commands.PythonCommandBase")
mcu_module = _module("Commands.McuCommandBase")
dialogue_module = _module("Commands.dialogue")
net_module = _module("Commands.net")
image_module = _module("Commands.image_proc")
python_commands = _module("Commands.PythonCommands", True)
bridge_package = _module("Commands.PythonCommands.bridge_functions", True)
bridge_module = _module("Commands.PythonCommands.bridge_functions.bridge_functions")

meta_module.CommandMeta = CommandMeta
command_module.Command = Command
for value in (Button, Hat, Stick, Direction, Touchscreen, KeyPress):
    setattr(keys_module, value.__name__, value)
sender_module.Sender = Sender
for value in (
    StopThread,
    PythonCommand,
    ImageProcPythonCommand,
    Camera,
    CaptureArea,
):
    setattr(python_module, value.__name__, value)
mcu_module.McuCommand = McuCommand
dialogue_module.Widget = Widget
bridge_package.BridgeFunctions = BridgeFunctions
bridge_module.BridgeFunctions = BridgeFunctions

for name in (
    "show_dialog",
    "is_dialog_closed",
    "wait_dialog",
    "dialogue6widget",
    "dialogue6widget_save_settings",
    "dialogue6widget_select_settings",
    "dialogue",
):
    setattr(dialogue_module, name, _forward(name))
for name in (
    "socket_connect",
    "socket_disconnect",
    "socket_transmit_message",
    "socket_receive_message",
    "socket_receive_message2",
    "socket_change_ipaddr",
    "socket_change_port",
    "socket_change_alive",
    "mqtt_transmit_message",
    "mqtt_receive_message",
    "mqtt_receive_message2",
    "mqtt_change_broker_address",
    "mqtt_change_id",
    "mqtt_change_clientId",
    "mqtt_change_pub_token",
    "mqtt_change_sub_token",
):
    setattr(net_module, name, _forward(name))
for name in (
    "isContainTemplate",
    "isContainTemplate_max",
    "isContainTemplateGPU",
    "isContainedImage",
    "saveCapture",
    "popupImage",
    "getCameraImage",
    "openImage",
    "setTemplateDir",
    "get_filespec",
    "displayRectangle",
    "displayText",
):
    setattr(image_module, name, _forward(name, image=True))

Commands.dialogue = dialogue_module
Commands.net = net_module
Commands.image_proc = image_module
Commands._meta = meta_module
Commands.CommandBase = command_module
Commands.Keys = keys_module
Commands.Sender = sender_module
Commands.PythonCommandBase = python_module
Commands.McuCommandBase = mcu_module
Commands.PythonCommands = python_commands
python_commands.bridge_functions = bridge_package
bridge_package.bridge_functions = bridge_module


def _manual_tags(command):
    tags = getattr(command, "TAGS", None)
    if tags is None:
        return []
    if not isinstance(tags, list) or not all(isinstance(tag, str) for tag in tags):
        raise TypeError("command TAGS must be list[str] or None")
    return tags.copy()


def _discover_source(source, path, module_path):
    namespace = {
        "__name__": module_path,
        "__file__": path,
        "__package__": module_path.rpartition(".")[0],
        "__builtins__": __builtins__,
    }
    exec(compile(source, path, "exec"), namespace, namespace)
    commands = []
    for symbol, value in namespace.items():
        if (
            isinstance(value, type)
            and value.__module__ == module_path
            and issubclass(value, PythonCommand)
        ):
            implementation = _inspect.getattr_static(value, "do", None)
            if isinstance(implementation, (classmethod, staticmethod)):
                implementation = implementation.__func__
            if implementation is None or getattr(
                implementation, "__pokecon_abstract_command_method__", False
            ):
                continue
            name = getattr(value, "NAME", symbol)
            kind = "python"
        elif isinstance(value, McuCommand):
            name = getattr(value, "NAME", value.sync_name)
            kind = "mcu"
        else:
            continue
        if not isinstance(name, str):
            raise TypeError("command NAME must be str")
        commands.append(
            {
                "name": name,
                "class_name": symbol,
                "manual_tags": _manual_tags(value),
                "kind": kind,
            }
        )
    return _json.dumps(
        commands,
        ensure_ascii=False,
        separators=(",", ":"),
        allow_nan=False,
    )


def _run_source(source, path, class_name, tags_json):
    global _active_command
    namespace = {
        "__name__": f"__pokecon_user_{class_name}",
        "__file__": path,
        "__builtins__": __builtins__,
    }
    exec(compile(source, path, "exec"), namespace, namespace)
    command_type = namespace.get(class_name)
    tags = _json.loads(tags_json)
    if not isinstance(tags, list) or not all(isinstance(tag, str) for tag in tags):
        raise TypeError("command tags must be list[str]")
    if isinstance(command_type, McuCommand):
        command = command_type
        command.TAGS = tags.copy()
        sender = Sender()
        command.start(sender, None)
        _current.command = command
        _active_command = command
        try:
            while _api.execution_checkpoint():
                _time.sleep(0.02)
        finally:
            command.end(sender)
            if _active_command is command:
                _active_command = None
            _current.command = None
    else:
        if not isinstance(command_type, type) or not issubclass(command_type, PythonCommand):
            raise TypeError(f"{class_name!r} is not a PythonCommand subclass")
        command_type.TAGS = tags.copy()
        if issubclass(command_type, ImageProcPythonCommand):
            camera = Camera()
            command = command_type(camera, CaptureArea(camera))
        else:
            command = command_type()
        command.isRunning = True
        _current.command = command
        _active_command = command
        try:
            command.do()
        finally:
            command.isRunning = False
            if _active_command is command:
                _active_command = None
            _reset_dialogs()
            _current.command = None
"#;
