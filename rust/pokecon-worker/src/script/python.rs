use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use pokecon_contracts::PROTOCOL_REGISTRY_JSON;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyModuleMethods};
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::ipc::{ConnectionError, IpcConnection, deserialize_value, serialize_value};

use super::protocol::{
    self, HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostSerialWriteRequest, HostSerialWriteRowRequest,
    ScriptControl, ScriptExecutionOutcome, ScriptExecutionResult, ScriptInputAction,
    ScriptOutputMode, ScriptOutputTarget, ScriptWorkerStatus,
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
}

impl ExecutionState {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            active_id: AtomicU64::new(0),
            alive: AtomicBool::new(false),
            stop_cause: AtomicU8::new(StopCause::None as u8),
        }
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
        true
    }

    fn request_finish(&self) {
        if self.active_id.load(Ordering::Acquire) != 0 {
            self.stop_cause
                .store(StopCause::Finish as u8, Ordering::Release);
            self.alive.store(false, Ordering::Release);
        }
    }

    fn status(&self, profile: &str) -> ScriptWorkerStatus {
        let active = self.active_id.load(Ordering::Acquire);
        ScriptWorkerStatus {
            initialized: true,
            profile: Some(profile.to_owned()),
            running: active != 0,
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
    response: oneshot::Sender<ScriptExecutionResult>,
}

enum ActorCommand {
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
    ) -> Result<(u64, oneshot::Receiver<ScriptExecutionResult>), PythonActorError> {
        let id = self.state.begin()?;
        let (response_sender, response_receiver) = oneshot::channel();
        let command = ActorCommand::Execute(ExecuteCommand {
            id,
            path,
            class_name,
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

    pub(super) fn request_stop(&self) -> bool {
        self.state.request_stop()
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
                ActorCommand::Execute(command) => {
                    execute_command(&self.config, &self.state, command);
                }
                ActorCommand::Shutdown(response) => {
                    let _result = response.send(());
                    return;
                }
            }
        }
    }
}

fn initialize_python(
    config: &PythonActorConfig,
    state: Arc<ExecutionState>,
) -> Result<(), PythonActorError> {
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
                    data_root: config.data_root.clone(),
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
    let source = std::fs::read_to_string(&command.path)
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))
        .and_then(|source| {
            Python::attach(|py| {
                let module = py.import("_pokecon_script")?;
                module.getattr("_run_source")?.call1((
                    source,
                    command.path.to_string_lossy().as_ref(),
                    &command.class_name,
                ))?;
                Ok(())
            })
        });
    let mut outcome = match source {
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
    data_root: PathBuf,
}

impl PyApi {
    fn request<Request, Response>(
        &self,
        operation: &'static str,
        request: &Request,
    ) -> PyResult<Response>
    where
        Request: serde::Serialize,
        Response: serde::de::DeserializeOwned,
    {
        request_host(&self.connection, &self.runtime_handle, operation, request)
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
import json as _json
import logging as _logging
import math as _math
import os as _os
import sys as _sys
import threading as _threading
import time as _time
import types as _types


class StopThread(Exception):
    pass


def _monitor_stop(_code, _offset):
    if _api.is_executing() and not _api.is_alive():
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


class PythonCommand:
    _logger = None
    keys = None

    def __init__(self):
        self._logger = _make_logger()
        self.keys = KeyPress(Sender())

    def do(self):
        raise NotImplementedError("PythonCommand.do() must be overridden")

    def finish(self):
        _api.finish()
        raise StopThread()

    def checkIfAlive(self):
        if not _api.is_alive():
            raise StopThread()
        return True

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
        internal = {"_logger", "keys"}
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
        _network("mqtt_transmit", room_id=roomid, message=message)
        self.checkIfAlive()

    def mqtt_receive_message(self, roomid, header, show_msg=False):
        if not isinstance(roomid, str) or not isinstance(header, str):
            raise TypeError("MQTT room ID and header must be str")
        result = _network(
            "mqtt_receive",
            room_id=roomid,
            headers=[header],
            show_message=bool(show_msg),
        )
        self.checkIfAlive()
        return result

    def mqtt_receive_message2(self, roomid, headerlist, show_msg=False):
        if not isinstance(roomid, str):
            raise TypeError("MQTT room ID must be str")
        if not isinstance(headerlist, list) or any(
            not isinstance(header, str) for header in headerlist
        ):
            raise TypeError("MQTT headers must be list[str]")
        result = _network(
            "mqtt_receive",
            room_id=roomid,
            headers=headerlist,
            show_message=bool(show_msg),
        )
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


class Camera:
    def __init__(self, fps=45):
        self._fps = int(fps)
        self._capture_size = (1280, 720)
        self._flip = False
        self._flip_mode = 0
        self._opened = False

    @property
    def image_bgr(self):
        import numpy as np

        width, height = self._capture_size
        return np.zeros((height, width, 3), dtype=np.uint8)

    def readFrame(self):
        return self.image_bgr

    def isOpened(self):
        return self._opened

    @property
    def fps(self):
        return self._fps

    @fps.setter
    def fps(self, value):
        self._fps = int(value)

    @property
    def capture_size(self):
        return self._capture_size

    @property
    def flip(self):
        return self._flip

    @property
    def flip_mode(self):
        return self._flip_mode

    def set_flip(self, value):
        normalized = str(value).casefold()
        values = {"none": (False, self._flip_mode), "vertical": (True, 0), "horizontal": (True, 1), "both": (True, -1)}
        if normalized not in values:
            raise ValueError("flip must be None, Vertical, Horizontal, or Both")
        self._flip, self._flip_mode = values[normalized]

    def saveCapture(self, *args, **kwargs):
        return _not_implemented("Camera.saveCapture")

    def openCamera(self, cameraId):
        self._opened = True

    def destroy(self):
        self._opened = False

    def camera_thread_start(self):
        self._opened = True

    def camera_thread_stop(self):
        self._opened = False

    def camera_update(self):
        return None


class CaptureArea:
    def __init__(self):
        self._show_size = (720, 1280)
        self._is_show_var = True
        self._rectangles = {}
        self._texts = {}

    @property
    def show_size(self):
        return self._show_size

    @property
    def is_show_var(self):
        return self._is_show_var

    def ImgRect(self, x1, y1, x2, y2, outline, tag, ms, flag=True):
        self._rectangles[tag] = (x1, y1, x2, y2, outline, ms, flag)

    def ImgText(self, x1, y1, txt, tag, ms, ft=("UD デジタル 教科書体 NP-B", 20), color="black", flag=True):
        self._texts[tag] = (x1, y1, txt, ms, ft, color, flag)

    def deleteImageRect(self, tag):
        self._rectangles.pop(tag, None)

    def deleteImageText(self, tag):
        self._texts.pop(tag, None)

    def setFps(self, fps):
        self.fps = int(fps)

    def setShowsize(self, show_height, show_width):
        self._show_size = (int(show_height), int(show_width))

    def changeRightMouseMode(self, mode):
        self.right_mouse_mode = str(mode)

    def setTouchscreenArea(self, x1, y1, x2, y2):
        if self._show_size[0] <= 0 or self._show_size[1] <= 0 or x1 == x2 or y1 == y2:
            raise ValueError("touchscreen area must be non-degenerate")
        self.touchscreen_area = (min(x1, x2), min(y1, y2), max(x1, x2), max(y1, y2))

    def saveCapture(self):
        return _not_implemented("CaptureArea.saveCapture")

    def update(self):
        return None

    def BindLeftClick(self):
        return None

    def BindRightClick(self):
        return None

    def UnbindLeftClick(self):
        return None

    def UnbindRightClick(self):
        return None

    def mouseCtrlLeftPress(self, event):
        return None

    def mouseLeftPress(self, event, keys_):
        return None

    def mouseLeftPressing(self, event, keys_):
        return None

    def mouseRightPress(self, event, keys_):
        return None

    def mouseRightPressing(self, event, keys_):
        return None

    def StartRangeSS(self, event):
        return None

    def MotionRangeSS(self, event):
        return None

    def ReleaseRangeSS(self, event):
        return None


class ImageProcPythonCommand(PythonCommand):
    camera = None
    cam = None
    gui = None
    canvas = None

    def __init__(self, cam, gui=None):
        super().__init__()
        self.camera = cam
        self.cam = cam
        self.gui = gui if gui is not None else CaptureArea()
        self.canvas = self.gui

    def discord_image(self, content="", index=0, crop_fmt="", crop=None, keys="DISCORD_WEBHOOK"):
        if index != 0:
            settings_keys = [f"DISCORD_WEBHOOK{index}"]
        elif isinstance(keys, str):
            settings_keys = [keys]
        elif isinstance(keys, list) and all(isinstance(key, str) for key in keys):
            settings_keys = list(keys)
        else:
            raise TypeError("discord_image keys must be str or list[str]")
        normalized_crop = None if crop is None else [int(value) for value in crop]
        try:
            _notification(
                {
                    "kind": "discord_image",
                    "content": str(content),
                    "settings_keys": settings_keys,
                    "crop_format": str(crop_fmt),
                    "crop": normalized_crop,
                }
            )
        except Exception:
            self._logger.warning("discord_image delivery failed")

    def LINE_image(self, txt, crop_fmt="", crop=None, token=""):
        self._logger.warning("LINE_image is unavailable because LINE Notify has ended")

    def isContainTemplate(self, *args, **kwargs):
        return _not_implemented("isContainTemplate")

    def isContainTemplate_max(self, *args, **kwargs):
        return _not_implemented("isContainTemplate_max")

    def isContainTemplateGPU(self, *args, **kwargs):
        return _not_implemented("isContainTemplateGPU")

    def isContainedImage(self, *args, **kwargs):
        return _not_implemented("isContainedImage")

    def saveCapture(self, *args, **kwargs):
        return _not_implemented("ImageProcPythonCommand.saveCapture")

    def popupImage(self, *args, **kwargs):
        return _not_implemented("popupImage")

    def getCameraImage(self, crop_fmt="", crop=None):
        return self.camera.image_bgr

    def openImage(self, filename, mode="t"):
        import cv2

        return cv2.imread(self.get_filespec(filename, mode))

    def setTemplateDir(self, path):
        self.template_path_name = str(path)

    def get_filespec(self, filename, mode="t"):
        base = getattr(self, "template_path_name", "./Template/") if mode == "t" else ""
        return _os.path.abspath(_os.path.join(base, filename))

    def displayRectangle(self, max_loc, width, height, tag=None, ms=2000, color=None, crop_fmt="", crop=None):
        tag = tag or f"rect-{id(max_loc)}"
        outline = (color or ["blue"])[0]
        self.gui.ImgRect(max_loc[0], max_loc[1], max_loc[0] + width, max_loc[1] + height, outline, tag, int(ms), True)

    def displayText(self, position, txt, tag=None, ms=2000, font="UD デジタル 教科書体 NP-B", fontsize=20, color="black"):
        self.gui.ImgText(position[0], position[1], txt, tag or f"text-{id(position)}", int(ms), (font, fontsize), color, True)


class McuCommand:
    def __init__(self, sync_name):
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


_current = _threading.local()


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
    return call


def _module(name, package=False):
    module = _types.ModuleType(name)
    if package:
        module.__path__ = []
    _sys.modules[name] = module
    return module


Commands = _module("Commands", True)
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
Commands.Keys = keys_module
Commands.Sender = sender_module
Commands.PythonCommandBase = python_module
Commands.McuCommandBase = mcu_module
Commands.PythonCommands = python_commands
python_commands.bridge_functions = bridge_package
bridge_package.bridge_functions = bridge_module


def _run_source(source, path, class_name):
    namespace = {
        "__name__": f"__pokecon_user_{class_name}",
        "__file__": path,
        "__builtins__": __builtins__,
    }
    exec(compile(source, path, "exec"), namespace, namespace)
    command_type = namespace.get(class_name)
    if not isinstance(command_type, type) or not issubclass(command_type, PythonCommand):
        raise TypeError(f"{class_name!r} is not a PythonCommand subclass")
    if issubclass(command_type, ImageProcPythonCommand):
        command = command_type(Camera(), CaptureArea())
    else:
        command = command_type()
    _current.command = command
    try:
        command.do()
    finally:
        _reset_dialogs()
        _current.command = None
"#;
