use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pokecon_camera::{BgrFrame, CaptureResolution, FlipMode, ScreenshotFormat, SharedFrameRing};
use pokecon_worker::WorkerKind;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    self, HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostTkRequest, HostTkResult, ScriptCommandKind, ScriptControl, ScriptDialogState,
    ScriptExecuteRequest, ScriptExecutionOutcome, ScriptInitializeRequest, ScriptInputAction,
    ScriptPointerButton, ScriptPointerEvent, ScriptPointerPhase, ScriptStick, ScriptTkEvent,
    ScriptWorkerStatus,
};
use pokecon_worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};
use pokecon_worker::supervisor::{ManagedWorker, StopPurpose, WorkerLaunch, WorkerSupervisor};
use tempfile::TempDir;
use tokio::sync::Notify;

#[derive(Debug, Default)]
struct RecordingScriptHost {
    controller_inputs: Mutex<Vec<HostControllerInputRequest>>,
    serial_bytes: Mutex<Vec<Vec<u8>>>,
    serial_rows: Mutex<Vec<String>>,
    serial_reloads: AtomicUsize,
    outputs: Mutex<Vec<HostOutputRequest>>,
    neutralizations: AtomicUsize,
    next_dialog_id: AtomicU64,
    dialogs: Mutex<BTreeMap<u64, HostDialogOpenRequest>>,
    abort_dialogs: AtomicBool,
    dialog_cleanups: AtomicUsize,
    network_requests: Mutex<Vec<HostNetworkRequest>>,
    notification_requests: Mutex<Vec<HostNotificationRequest>>,
    fail_notifications: AtomicBool,
    camera_ring: Mutex<Option<SharedFrameRing>>,
    camera_state: Mutex<Option<HostCameraState>>,
    overlay_requests: Mutex<Vec<HostOverlayRequest>>,
    popup_requests: Mutex<Vec<HostPopupImageRequest>>,
    tk_requests: Mutex<Vec<HostTkRequest>>,
    tk_scales: Mutex<BTreeMap<u64, f64>>,
    block_tk_title: AtomicBool,
    tk_title_blocked: AtomicBool,
    tk_title_released: Mutex<bool>,
    tk_title_release: Condvar,
    activity: Notify,
}

impl ScriptHost for RecordingScriptHost {
    fn controller_input(&self, request: HostControllerInputRequest) -> Result<(), ScriptHostError> {
        self.controller_inputs.lock().unwrap().push(request);
        Ok(())
    }

    fn controller_neutral(&self) -> Result<(), ScriptHostError> {
        self.neutralizations.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn serial_write(&self, data: Vec<u8>) -> Result<(), ScriptHostError> {
        self.serial_bytes.lock().unwrap().push(data);
        Ok(())
    }

    fn serial_write_row(&self, row: String) -> Result<(), ScriptHostError> {
        self.serial_rows.lock().unwrap().push(row);
        Ok(())
    }

    fn serial_reload(&self) -> Result<(), ScriptHostError> {
        self.serial_reloads.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn output(&self, request: HostOutputRequest) -> Result<(), ScriptHostError> {
        self.outputs.lock().unwrap().push(request);
        self.activity.notify_one();
        Ok(())
    }

    fn dialog_open(
        &self,
        request: HostDialogOpenRequest,
    ) -> Result<HostDialogOpenResult, ScriptHostError> {
        let dialog_id = self.next_dialog_id.fetch_add(1, Ordering::AcqRel) + 1;
        self.dialogs.lock().unwrap().insert(dialog_id, request);
        Ok(HostDialogOpenResult { dialog_id })
    }

    fn dialog_status(
        &self,
        request: HostDialogStatusRequest,
    ) -> Result<HostDialogStatusResult, ScriptHostError> {
        let dialogs = self.dialogs.lock().unwrap();
        let dialog = dialogs
            .get(&request.dialog_id)
            .ok_or_else(|| ScriptHostError::new("DialogNotFound", "test dialog does not exist"))?;
        let state = if self.abort_dialogs.load(Ordering::Acquire) {
            ScriptDialogState::Aborted
        } else {
            ScriptDialogState::Confirmed {
                values: dialog
                    .widgets
                    .iter()
                    .map(|widget| widget.value.clone())
                    .collect(),
            }
        };
        Ok(HostDialogStatusResult { state })
    }

    fn dialog_close_all(&self) -> Result<(), ScriptHostError> {
        self.dialogs.lock().unwrap().clear();
        self.dialog_cleanups.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    fn network(&self, request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError> {
        let message = match &request {
            HostNetworkRequest::SocketReceive { .. } => Some("socket-response".to_owned()),
            HostNetworkRequest::MqttReceive { .. } => Some("mqtt-response".to_owned()),
            _ => None,
        };
        self.network_requests.lock().unwrap().push(request);
        Ok(HostNetworkResult { message })
    }

    fn notification(&self, request: HostNotificationRequest) -> Result<(), ScriptHostError> {
        self.notification_requests.lock().unwrap().push(request);
        if self.fail_notifications.load(Ordering::Acquire) {
            return Err(ScriptHostError::new(
                "NotificationFailed",
                "deliberate notification failure",
            ));
        }
        Ok(())
    }

    fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError> {
        Ok(HostCameraInitializeResult {
            mapping: self
                .camera_ring
                .lock()
                .unwrap()
                .as_ref()
                .map(SharedFrameRing::descriptor),
            state: self.current_camera_state(),
        })
    }

    fn camera_control(
        &self,
        request: HostCameraControlRequest,
    ) -> Result<HostCameraState, ScriptHostError> {
        let mut state = self.camera_state.lock().unwrap();
        let state = state.get_or_insert_with(default_camera_state);
        match request {
            HostCameraControlRequest::State | HostCameraControlRequest::Update => {}
            HostCameraControlRequest::SetFps { fps } => state.fps = fps,
            HostCameraControlRequest::SetFlip { mode } => state.flip_mode = mode,
            HostCameraControlRequest::Open { .. } | HostCameraControlRequest::ThreadStart => {
                state.opened = true;
            }
            HostCameraControlRequest::Destroy | HostCameraControlRequest::ThreadStop => {
                state.opened = false;
            }
        }
        Ok(*state)
    }

    fn overlay(&self, request: HostOverlayRequest) -> Result<(), ScriptHostError> {
        self.overlay_requests.lock().unwrap().push(request);
        Ok(())
    }

    fn popup_image(&self, request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
        self.popup_requests.lock().unwrap().push(request);
        Ok(())
    }

    fn tk(&self, request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        if self.block_tk_title.load(Ordering::Acquire)
            && matches!(&request, HostTkRequest::SetTitle { .. })
        {
            self.tk_title_blocked.store(true, Ordering::Release);
            self.activity.notify_one();
            let released = self.tk_title_released.lock().unwrap();
            let _wait_result = self
                .tk_title_release
                .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                .unwrap();
        }
        let result = match &request {
            HostTkRequest::CreateScale {
                widget_id,
                from_value,
                ..
            } => {
                self.tk_scales
                    .lock()
                    .unwrap()
                    .insert(*widget_id, *from_value);
                HostTkResult::default()
            }
            HostTkRequest::SetScale { widget_id, value } => {
                self.tk_scales.lock().unwrap().insert(*widget_id, *value);
                HostTkResult::default()
            }
            HostTkRequest::GetScale { widget_id } => HostTkResult {
                value: self.tk_scales.lock().unwrap().get(widget_id).copied(),
            },
            HostTkRequest::Cleanup => {
                self.tk_scales.lock().unwrap().clear();
                HostTkResult::default()
            }
            _ => HostTkResult::default(),
        };
        self.tk_requests.lock().unwrap().push(request);
        self.activity.notify_one();
        Ok(result)
    }
}

impl RecordingScriptHost {
    fn current_camera_state(&self) -> HostCameraState {
        self.camera_state
            .lock()
            .unwrap()
            .unwrap_or_else(default_camera_state)
    }

    fn release_tk_title(&self) {
        *self.tk_title_released.lock().unwrap() = true;
        self.tk_title_release.notify_all();
    }
}

const fn default_camera_state() -> HostCameraState {
    HostCameraState {
        opened: false,
        fps: 45,
        capture_resolution: CaptureResolution::R1280x720,
        flip_mode: FlipMode::None,
        screenshot_format: ScreenshotFormat::Png,
    }
}

impl ResourceSafety for RecordingScriptHost {
    fn force_release(&self) {
        self.neutralizations.fetch_add(1, Ordering::AcqRel);
    }
}

fn create_profile() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temporary = TempDir::new().expect("temporary profile is created");
    let command_root = temporary.path().join("Commands");
    let data_root = temporary.path().join("Data");
    std::fs::create_dir(&command_root).expect("command root is created");
    std::fs::create_dir(&data_root).expect("data root is created");
    (temporary, command_root, data_root)
}

async fn spawn_client(
    host: Arc<RecordingScriptHost>,
) -> (Arc<ManagedWorker>, Arc<ScriptWorkerClient>) {
    let supervisor = WorkerSupervisor::new();
    let mut launch =
        WorkerLaunch::managed(env!("CARGO_BIN_EXE_pokecon-worker"), WorkerKind::Script)
            .clear_environment();
    if let Some(site_packages) = std::env::var_os(protocol::PYTHON_SITE_PACKAGES_ENV) {
        launch = launch.environment(protocol::PYTHON_SITE_PACKAGES_ENV, site_packages);
    }
    let worker = supervisor
        .spawn(launch, host.clone())
        .await
        .expect("script worker starts");
    let client =
        Arc::new(ScriptWorkerClient::attach(worker.clone(), host).expect("script client attaches"));
    (worker, client)
}

async fn initialize(
    client: &ScriptWorkerClient,
    command_root: &std::path::Path,
    data_root: &std::path::Path,
) {
    assert_eq!(
        client.status().await.expect("initial status succeeds"),
        ScriptWorkerStatus::uninitialized()
    );
    let initialized = client
        .initialize(&ScriptInitializeRequest {
            profile: "default".to_owned(),
            command_root: command_root.to_path_buf(),
            data_root: data_root.to_path_buf(),
        })
        .await
        .expect("script runtime initializes");
    assert!(initialized.status.initialized);
    assert_eq!(initialized.status.profile.as_deref(), Some("default"));
    assert!(!initialized.status.running);
}

async fn stop_worker(worker: &ManagedWorker) {
    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(3))
        .await
        .expect("script worker stops cooperatively");
    assert!(report.cooperative_acknowledged, "{report:?}");
    assert!(!report.forced);
    assert!(report.exit.success);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discovery_preserves_declaration_order_and_separates_tag_layers() {
    const SOURCE: &str = r#"
from Commands.McuCommandBase import McuCommand
from Commands.PythonCommandBase import PythonCommand


class Abstract(PythonCommand):
    pass


class Second(PythonCommand):
    NAME = "Display second"
    TAGS = ["manual", "manual"]

    def do(self):
        print(self.__class__.TAGS)


class First(PythonCommand):
    TAGS = None

    def do(self):
        pass


firmware = McuCommand("firmware-sync")
firmware.NAME = "Firmware"
firmware.TAGS = ["mcu"]
"#;

    let (_temporary, command_root, data_root) = create_profile();
    let directory = command_root.join("Samples/Rank");
    std::fs::create_dir_all(&directory).expect("nested command directory is created");
    std::fs::write(directory.join("commands.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let discovered = client.discover().await.expect("command discovery succeeds");
    assert_eq!(discovered.commands.len(), 3);
    assert_eq!(
        discovered
            .commands
            .iter()
            .map(|entry| entry.command.class_name.as_str())
            .collect::<Vec<_>>(),
        ["Second", "First", "firmware"]
    );
    assert_eq!(discovered.commands[0].command.name, "Display second");
    assert_eq!(
        discovered.commands[0].command.module_path,
        "Commands.PythonCommands.Samples.Rank.commands"
    );
    assert_eq!(discovered.commands[0].command.tags, ["@Samples", "@Rank"]);
    assert_eq!(discovered.commands[0].manual_tags, ["manual", "manual"]);
    assert_eq!(discovered.commands[0].kind, ScriptCommandKind::Python);
    assert_eq!(discovered.commands[2].kind, ScriptCommandKind::Mcu);

    let result = client
        .execute(&ScriptExecuteRequest {
            path: discovered.commands[0].relative_path.clone(),
            class_name: "Second".to_owned(),
            tags: vec![
                "@Samples".to_owned(),
                "manual".to_owned(),
                "dynamic".to_owned(),
            ],
        })
        .await
        .expect("discovered command executes");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);
    assert!(
        host.outputs
            .lock()
            .unwrap()
            .iter()
            .any(|output| { output.message.contains("['@Samples', 'manual', 'dynamic']") })
    );
    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_are_idempotent_and_stop_wakes_a_paused_command() {
    const SOURCE: &str = r"
from Commands.PythonCommandBase import PythonCommand


class Pausable(PythonCommand):
    def do(self):
        while True:
            pass
";

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("pausable.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host).await;
    initialize(&client, &command_root, &data_root).await;

    let execution = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .execute(&ScriptExecuteRequest {
                    path: "pausable.py".into(),
                    class_name: "Pausable".to_owned(),
                    tags: Vec::new(),
                })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while !client.status().await.expect("status succeeds").running {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("command starts");

    assert!(client.pause().await.expect("pause succeeds").changed);
    assert!(
        !client
            .pause()
            .await
            .expect("repeated pause succeeds")
            .changed
    );
    assert!(
        client
            .status()
            .await
            .expect("paused status succeeds")
            .paused
    );
    assert!(client.resume().await.expect("resume succeeds").changed);
    assert!(
        !client
            .resume()
            .await
            .expect("repeated resume succeeds")
            .changed
    );
    assert!(
        !client
            .status()
            .await
            .expect("running status succeeds")
            .paused
    );
    assert!(client.pause().await.expect("second pause succeeds").changed);
    assert!(client.stop().await.expect("stop succeeds").stop_requested);

    let result = tokio::time::timeout(Duration::from_secs(2), execution)
        .await
        .expect("paused command stops")
        .expect("execution task joins")
        .expect("execution response succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Stopped);
    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn script_worker_executes_controller_serial_and_output_proxies() {
    const SOURCE: &str = r#"
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button, Direction


class Exercise(PythonCommand):
    def do(self):
        self.press(Button.A | Button.B, duration=0, wait=0)
        self.hold(Direction.UP, wait=0)
        self.holdEnd(Direction.UP)
        self.keys.ser.write(b"\x01\x02")
        self.direct_serial(["raw\n"], [0])
        self.reload_com_port()
        self.print_t1("panel")
        print("stdout")
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("exercise.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "exercise.py".into(),
            class_name: "Exercise".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("script execution request succeeds");
    assert_eq!(result.execution_id, 1);
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);
    assert_eq!(host.controller_inputs.lock().unwrap().len(), 4);
    assert_eq!(*host.serial_bytes.lock().unwrap(), vec![vec![1, 2]]);
    assert_eq!(*host.serial_rows.lock().unwrap(), vec!["raw"]);
    assert_eq!(host.serial_reloads.load(Ordering::Acquire), 1);
    {
        let outputs = host.outputs.lock().unwrap();
        assert!(outputs.iter().any(|output| output.message == "panel\n"));
        assert!(outputs.iter().any(|output| output.message == "stdout"));
    }
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 1);
    assert!(!client.status().await.unwrap().running);

    stop_worker(&worker).await;
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
async fn runtime_signatures_match_generated_typings_and_command_meta_is_enforced() {
    let typings_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../python/pokecon/typings/Commands");
    let typings_root = serde_json::to_string(&typings_root.to_string_lossy())
        .expect("typing root is representable as a Python string");
    let source = format!(
        r#"
import ast
import importlib
import inspect
import pathlib

from Commands._meta import CommandMeta
from Commands.CommandBase import Command
from Commands.McuCommandBase import McuCommand
from Commands.PythonCommandBase import ImageProcPythonCommand, PythonCommand


_TYPINGS_ROOT = pathlib.Path({typings_root})


def _ast_default(node):
    if node is None:
        return ("required",)
    if isinstance(node, ast.Constant):
        return ("literal", node.value)
    if isinstance(node, ast.Name) and node.id in {{"dict", "list"}}:
        return ("type", node.id)
    if isinstance(node, ast.Tuple):
        return ("tuple", tuple(_ast_default(item) for item in node.elts))
    if (
        isinstance(node, ast.UnaryOp)
        and isinstance(node.op, ast.USub)
        and isinstance(node.operand, ast.Constant)
    ):
        return ("literal", -node.operand.value)
    return ("source", ast.unparse(node))


def _runtime_default(value):
    if value is inspect.Parameter.empty:
        return ("required",)
    if value in (dict, list):
        return ("type", value.__name__)
    if isinstance(value, tuple):
        return ("tuple", tuple(_runtime_default(item) for item in value))
    if value is None or isinstance(value, (bool, float, int, str)):
        return ("literal", value)
    return ("runtime", repr(value))


def _ast_shape(function):
    arguments = function.args
    positional = [
        *((argument, "POSITIONAL_ONLY") for argument in arguments.posonlyargs),
        *((argument, "POSITIONAL_OR_KEYWORD") for argument in arguments.args),
    ]
    defaults = [None] * (len(positional) - len(arguments.defaults)) + list(
        arguments.defaults
    )
    shape = [
        (argument.arg, kind, _ast_default(default))
        for (argument, kind), default in zip(positional, defaults, strict=True)
    ]
    if arguments.vararg is not None:
        shape.append((arguments.vararg.arg, "VAR_POSITIONAL", ("required",)))
    shape.extend(
        (
            argument.arg,
            "KEYWORD_ONLY",
            _ast_default(default),
        )
        for argument, default in zip(
            arguments.kwonlyargs, arguments.kw_defaults, strict=True
        )
    )
    if arguments.kwarg is not None:
        shape.append((arguments.kwarg.arg, "VAR_KEYWORD", ("required",)))
    return tuple(shape)


def _runtime_shape(callable_object):
    return tuple(
        (parameter.name, parameter.kind.name, _runtime_default(parameter.default))
        for parameter in inspect.signature(callable_object).parameters.values()
    )


def _is_property(function):
    return any(
        (isinstance(decorator, ast.Name) and decorator.id == "property")
        or (
            isinstance(decorator, ast.Attribute)
            and decorator.attr in {{"deleter", "getter", "setter"}}
        )
        for decorator in function.decorator_list
    )


def _assert_signature(callable_object, declarations, label):
    actual = _runtime_shape(callable_object)
    expected = tuple(_ast_shape(declaration) for declaration in declarations)
    assert actual in expected, f"{{label}} signature {{actual!r}} not in {{expected!r}}"


def _check_stub(filename, module_name):
    tree = ast.parse((_TYPINGS_ROOT / filename).read_text(encoding="utf-8"))
    module = importlib.import_module(module_name)
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            declarations = [
                candidate
                for candidate in tree.body
                if isinstance(candidate, type(node)) and candidate.name == node.name
            ]
            if declarations[0] is not node:
                continue
            _assert_signature(
                getattr(module, node.name), declarations, f"{{module_name}}.{{node.name}}"
            )
        if not isinstance(node, ast.ClassDef):
            continue
        runtime_class = getattr(module, node.name)
        methods = {{}}
        for item in node.body:
            if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                methods.setdefault(item.name, []).append(item)
        for name, declarations in methods.items():
            if node.name == "Widget" and name == "__init__":
                continue
            member = inspect.getattr_static(runtime_class, name)
            if any(_is_property(declaration) for declaration in declarations):
                assert isinstance(member, property), f"{{module_name}}.{{node.name}}.{{name}}"
                continue
            if isinstance(member, (classmethod, staticmethod)):
                member = member.__func__
            _assert_signature(
                member, declarations, f"{{module_name}}.{{node.name}}.{{name}}"
            )


class Contracts(PythonCommand):
    def do(self):
        for filename, module_name in (
            ("_meta.pyi", "Commands._meta"),
            ("CommandBase.pyi", "Commands.CommandBase"),
            ("Sender.pyi", "Commands.Sender"),
            ("Keys.pyi", "Commands.Keys"),
            ("dialogue.pyi", "Commands.dialogue"),
            ("net.pyi", "Commands.net"),
            ("PythonCommandBase.pyi", "Commands.PythonCommandBase"),
            ("image_proc.pyi", "Commands.image_proc"),
            ("McuCommandBase.pyi", "Commands.McuCommandBase"),
            (
                "PythonCommands/bridge_functions/bridge_functions.pyi",
                "Commands.PythonCommands.bridge_functions.bridge_functions",
            ),
        ):
            _check_stub(filename, module_name)

        assert isinstance(Command, CommandMeta)
        assert isinstance(PythonCommand, CommandMeta)
        assert isinstance(ImageProcPythonCommand, CommandMeta)
        assert isinstance(McuCommand, CommandMeta)
        assert isinstance(self, Command)
        assert self.alive is True
        assert self.isRunning is True
        assert self.postProcess is None

        class MissingDo(PythonCommand):
            pass

        class Concrete(PythonCommand):
            def do(self):
                return None

        class DerivedConcrete(Concrete):
            pass

        for abstract in (PythonCommand, ImageProcPythonCommand, MissingDo):
            try:
                abstract()
            except TypeError as error:
                assert "without implementing do()" in str(error)
            else:
                raise AssertionError(f"{{abstract.__name__}} must be abstract")
        assert isinstance(Concrete(), Command)
        assert isinstance(DerivedConcrete(), Command)
"#
    );

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("contracts.py"), source).expect("contract fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "contracts.py".into(),
            class_name: "Contracts".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("contract script executes");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cooperative_stop_observes_alive_and_runs_cleanup_once() {
    const SOURCE: &str = r#"
from Commands.PythonCommandBase import PythonCommand


class Cooperative(PythonCommand):
    def do(self):
        assert self.alive is True
        self.postProcess = lambda: self.keys.ser.writeRow("post-process")
        self.keys.ser.writeRow("ready")
        while self.alive:
            pass
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("cooperative.py"), SOURCE)
        .expect("cooperative fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let execution_client = client.clone();
    let execution = tokio::spawn(async move {
        execution_client
            .execute(&ScriptExecuteRequest {
                path: "cooperative.py".into(),
                class_name: "Cooperative".to_owned(),
                tags: Vec::new(),
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !client.status().await.expect("status succeeds").running {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cooperative script starts");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if host
                .serial_rows
                .lock()
                .unwrap()
                .iter()
                .any(|row| row == "ready")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cooperative script installs its cleanup callback");

    assert!(client.stop().await.expect("stop succeeds").stop_requested);
    let result = tokio::time::timeout(Duration::from_secs(3), execution)
        .await
        .expect("cooperative script observes stop")
        .expect("execution task joins")
        .expect("execution response succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Stopped);
    assert_eq!(
        *host.serial_rows.lock().unwrap(),
        vec!["ready", "end", "post-process"]
    );
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 1);

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finish_reports_normal_outcome_and_runs_cleanup_once() {
    const SOURCE: &str = r#"
from Commands.PythonCommandBase import PythonCommand


class Finisher(PythonCommand):
    def do(self):
        self.postProcess = lambda: self.keys.ser.writeRow("post-process")
        self.finish()
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("finisher.py"), SOURCE).expect("finisher fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "finisher.py".into(),
            class_name: "Finisher".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("finisher executes");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Finished);
    assert_eq!(
        *host.serial_rows.lock().unwrap(),
        vec!["end", "post-process"]
    );
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 1);

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn script_stop_interrupts_busy_python_without_blocking_ipc() {
    const SOURCE: &str = r"
from Commands.PythonCommandBase import PythonCommand


class Busy(PythonCommand):
    def do(self):
        while True:
            pass
";

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("busy.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let execution_client = client.clone();
    let execution = tokio::spawn(async move {
        execution_client
            .execute(&ScriptExecuteRequest {
                path: "busy.py".into(),
                class_name: "Busy".to_owned(),
                tags: Vec::new(),
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if client
                .status()
                .await
                .expect("status remains responsive")
                .running
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("busy script starts while IPC remains responsive");

    assert!(
        client
            .stop()
            .await
            .expect("stop request succeeds")
            .stop_requested
    );
    let result = tokio::time::timeout(Duration::from_secs(3), execution)
        .await
        .expect("monitoring callback interrupts the busy loop")
        .expect("execution task joins")
        .expect("execution response succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Stopped);
    assert!(!client.status().await.unwrap().running);
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 1);

    stop_worker(&worker).await;
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn script_dialogs_preserve_widget_lifecycle_and_legacy_forms() {
    const SOURCE: &str = r#"
import json
import os
import tkinter.messagebox as messagebox

from Commands import dialogue as dialogue_api
from Commands.PythonCommandBase import PythonCommand
from Commands.dialogue import Widget


class Dialogs(PythonCommand):
    def do(self):
        entry = Widget("Entry", "name", "default")
        check = Widget("Check", "enabled", True)
        assert entry.value == "default" and not entry.has_result
        assert self.show_dialog("blocking", [entry, check]) == 0
        assert entry.value == "default" and entry.has_result
        assert check.value is True and check.has_result
        try:
            entry.has_result = False
        except AttributeError:
            pass
        else:
            raise AssertionError("has_result must be read-only")

        dialog_id = dialogue_api.show_dialog(
            "nonblocking", entry, blocking=False
        )
        assert dialog_id > 0 and not entry.has_result
        assert dialogue_api.is_dialog_closed(dialog_id)
        assert entry.has_result
        assert dialogue_api.wait_dialog(dialog_id) == 0

        assert self.dialogue("entry", ["first", "second"]) == ["", ""]
        assert messagebox.showinfo("legacy", "message") == "ok"
        legacy = [
            ["Check", "check", "True"],
            ["Next"],
            ["Combo", "choice", ["a", "b"], "b"],
            ["Scale", "amount", 0, 10, 5, 0],
        ]
        assert self.dialogue6widget("legacy", legacy) == [True, "b", 5]
        assert self.dialogue6widget("legacy-dict", legacy, need=dict) == {
            "check": True,
            "choice": "b",
            "amount": 5,
        }

        settings = os.path.join(os.path.dirname(__file__), "dialog-settings.json")
        saved = self.dialogue6widget_save_settings(
            "save", [["Entry", "value", "remembered"]], settings
        )
        assert saved == ["remembered"]
        with open(settings, encoding="utf-8") as settings_file:
            assert json.load(settings_file) == {"value": "remembered"}
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("dialogs.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "dialogs.py".into(),
            class_name: "Dialogs".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("dialog script execution succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);
    assert_eq!(host.next_dialog_id.load(Ordering::Acquire), 7);
    assert_eq!(host.dialog_cleanups.load(Ordering::Acquire), 1);
    assert!(host.dialogs.lock().unwrap().is_empty());

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abnormal_dialog_close_stops_the_script_and_cleans_all_dialogs() {
    const SOURCE: &str = r#"
from Commands.PythonCommandBase import PythonCommand
from Commands.dialogue import Widget


class AbortedDialog(PythonCommand):
    def do(self):
        self.show_dialog("abort", Widget("Entry", "value", "unchanged"))
        raise AssertionError("an aborted dialog must not return")
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("aborted_dialog.py"), SOURCE)
        .expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    host.abort_dialogs.store(true, Ordering::Release);
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "aborted_dialog.py".into(),
            class_name: "AbortedDialog".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("aborted dialog uses a typed execution outcome");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Stopped);
    assert_eq!(host.dialog_cleanups.load(Ordering::Acquire), 1);
    assert!(host.dialogs.lock().unwrap().is_empty());
    assert_eq!(host.neutralizations.load(Ordering::Acquire), 1);

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
async fn script_network_and_notifications_use_closed_fail_soft_proxies() {
    const SOURCE: &str = r#"
from Commands import net
from Commands.PythonCommandBase import ImageProcPythonCommand


class NetworkAndNotifications(ImageProcPythonCommand):
    def do(self):
        self.socket_change_ipaddr("127.0.0.1")
        self.socket_change_port(4242)
        self.socket_change_alive(True)
        self.socket_connect()
        self.socket_transmit_message("outbound")
        assert self.socket_receive_message("one") == "socket-response"
        assert self.socket_receive_message2(["two", "three"], True) == "socket-response"
        self.socket_disconnect()
        net.socket_connect()
        net.socket_disconnect()

        self.mqtt_change_broker_address("broker.invalid")
        self.mqtt_change_id("account")
        self.mqtt_change_clientId("client")
        self.mqtt_change_pub_token("super-secret-publish")
        self.mqtt_change_sub_token("super-secret-subscribe")
        self.mqtt_transmit_message("room", "outbound")
        assert self.mqtt_receive_message("room", "one") == "mqtt-response"
        assert self.mqtt_receive_message2("room", ["two"], True) == "mqtt-response"

        self.discord_text("private-content", index=2, keys="ignored")
        self.discord_image(
            "private-image-content",
            crop_fmt="1",
            crop=[0, 0, 10, 10],
            keys=["HOOK_A", "HOOK_B"],
        )
        self.LINE_text("ignored")
        self.LINE_image("ignored")
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("network.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let ring =
        SharedFrameRing::create(CaptureResolution::R640x360).expect("test frame ring is created");
    ring.publish(&BgrFrame::new(640, 360, vec![0; 640 * 360 * 3]).expect("test frame is valid"))
        .expect("test frame is published");
    *host.camera_ring.lock().unwrap() = Some(ring);
    *host.camera_state.lock().unwrap() = Some(HostCameraState {
        opened: true,
        fps: 30,
        capture_resolution: CaptureResolution::R640x360,
        flip_mode: FlipMode::None,
        screenshot_format: ScreenshotFormat::Png,
    });
    host.fail_notifications.store(true, Ordering::Release);
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "network.py".into(),
            class_name: "NetworkAndNotifications".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("network script execution succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);

    {
        let requests = host.network_requests.lock().unwrap();
        assert!(
            requests.iter().any(|request| matches!(
                request,
                HostNetworkRequest::SocketChangePort { port: 4242 }
            ))
        );
        assert!(requests.iter().any(|request| matches!(
            request,
            HostNetworkRequest::MqttReceive { headers, .. } if headers == &["two"]
        )));
        for request in requests.iter() {
            let debug = format!("{request:?}");
            assert!(!debug.contains("super-secret"));
            assert!(!debug.contains("private-content"));
        }
    }

    {
        let notifications = host.notification_requests.lock().unwrap();
        assert_eq!(notifications.len(), 2);
        assert!(matches!(
            &notifications[0],
            HostNotificationRequest::DiscordText { settings_key, .. }
                if settings_key == "DISCORD_WEBHOOK2"
        ));
        assert!(matches!(
            &notifications[1],
            HostNotificationRequest::DiscordImage {
                settings_keys,
                crop,
                content_type,
                encoded,
                ..
            }
                if settings_keys == &["HOOK_A", "HOOK_B"]
                    && crop.as_deref() == Some(&[0, 0, 10, 10][..])
                    && content_type == "image/jpeg"
                    && encoded.starts_with(&[0xff, 0xd8])
                    && encoded.len() <= protocol::MAX_NOTIFICATION_IMAGE_BYTES
        ));
        for request in notifications.iter() {
            let debug = format!("{request:?}");
            assert!(!debug.contains("private"));
        }
    }

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
async fn overlay_pointer_events_run_fifo_callbacks_without_blocking_ipc() {
    const SOURCE: &str = r#"
from Commands.PythonCommandBase import ImageProcPythonCommand


class PointerCallbacks(ImageProcPythonCommand):
    def do(self):
        self.gui.setShowsize(100, 200)
        self.gui.setTouchscreenArea(0, 0, 200, 100)
        self.gui.changeRightMouseMode("Qingpi")
        self.gui.BindLeftClick()
        self.gui.BindRightClick()
        while True:
            pass
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("pointer.py"), SOURCE).expect("script fixture is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let execution = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .execute(&ScriptExecuteRequest {
                    path: "pointer.py".into(),
                    class_name: "PointerCallbacks".to_owned(),
                    tags: Vec::new(),
                })
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let bindings = host
                .overlay_requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(request, HostOverlayRequest::SetBinding { .. }))
                .count();
            if bindings == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pointer bindings are installed");

    for event in [
        ScriptPointerEvent {
            button: ScriptPointerButton::Left,
            phase: ScriptPointerPhase::Pressed,
            x: 100,
            y: 50,
        },
        ScriptPointerEvent {
            button: ScriptPointerButton::Left,
            phase: ScriptPointerPhase::Moved,
            x: 160,
            y: 50,
        },
        ScriptPointerEvent {
            button: ScriptPointerButton::Left,
            phase: ScriptPointerPhase::Released,
            x: 160,
            y: 50,
        },
        ScriptPointerEvent {
            button: ScriptPointerButton::Right,
            phase: ScriptPointerPhase::Pressed,
            x: 100,
            y: 50,
        },
        ScriptPointerEvent {
            button: ScriptPointerButton::Right,
            phase: ScriptPointerPhase::Released,
            x: 100,
            y: 50,
        },
    ] {
        client
            .pointer_event(&event)
            .await
            .expect("pointer callback is queued");
    }

    tokio::time::timeout(Duration::from_secs(3), async {
        while host.controller_inputs.lock().unwrap().len() < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pointer callbacks reach the controller host");
    {
        let inputs = host.controller_inputs.lock().unwrap();
        assert!(matches!(
            &inputs[0],
            HostControllerInputRequest {
                action: ScriptInputAction::Press,
                controls,
                ..
            } if controls == &[ScriptControl::Stick {
                stick: ScriptStick::Left,
                x: 255,
                y: 128,
            }]
        ));
        assert!(matches!(
            &inputs[1],
            HostControllerInputRequest {
                action: ScriptInputAction::Press,
                controls,
                ..
            } if controls == &[ScriptControl::Stick {
                stick: ScriptStick::Left,
                x: 128,
                y: 128,
            }]
        ));
        assert!(matches!(
            &inputs[2],
            HostControllerInputRequest {
                action: ScriptInputAction::Press,
                controls,
                ..
            } if controls == &[ScriptControl::Touchscreen { x: 160, y: 120 }]
        ));
        assert!(matches!(
            &inputs[3],
            HostControllerInputRequest {
                action: ScriptInputAction::Release,
                controls,
                ..
            } if controls == &[ScriptControl::Touchscreen { x: 0, y: 0 }]
        ));
    }

    assert!(client.stop().await.expect("stop succeeds").stop_requested);
    let result = tokio::time::timeout(Duration::from_secs(3), execution)
        .await
        .expect("pointer script stops")
        .expect("execution task joins")
        .expect("execution response succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Stopped);
    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
async fn script_camera_images_are_private_and_image_processing_stays_worker_local() {
    const SOURCE: &str = r#"
import os

import cv2
import numpy as np

from Commands.PythonCommandBase import ImageProcPythonCommand


class Images(ImageProcPythonCommand):
    def do(self):
        assert self.camera.isOpened()
        assert self.camera.fps == 30
        assert self.camera.capture_size == (640, 360)
        assert self.camera.flip is False

        first = self.camera.image_bgr
        assert first.shape == (360, 640, 3)
        assert first.dtype == np.uint8 and first.flags["W"]
        original = first[50, 60].copy()
        first[50, 60] = (0, 0, 0)
        second = self.camera.readFrame()
        assert np.array_equal(second[50, 60], original)

        crop_cases = [
            ("1", [60, 50, 90, 70]),
            ("2", [60, 50, 30, 20]),
            ("3", [60, 90, 50, 70]),
            ("4", [60, 30, 50, 20]),
            ("11", [50, 60, 70, 90]),
            ("12", [50, 60, 20, 30]),
            ("13", [50, 70, 60, 90]),
            ("14", [50, 20, 60, 30]),
        ]
        for crop_fmt, crop in crop_cases:
            cropped = self.getCameraImage(crop_fmt, crop)
            assert cropped.shape == (20, 30, 3)
            assert cropped.flags["W"]
            assert np.array_equal(cropped[0, 0], original)

        os.makedirs(self.template_path_name, exist_ok=True)
        template_path = self.get_filespec("template.png")
        mask_path = self.get_filespec("mask.png")
        scene_path = self.get_filespec("scene.png")
        assert cv2.imwrite(template_path, second[50:70, 60:90])
        assert cv2.imwrite(mask_path, np.full((20, 30), 255, dtype=np.uint8))
        assert cv2.imwrite(scene_path, second)
        loaded = self.openImage("template.png")
        assert loaded is not None and loaded.shape == (20, 30, 3)

        assert self.isContainTemplate(
            "template.png",
            threshold=0.99,
            mask_path="mask.png",
            show_position=True,
        )
        assert self.isContainTemplateGPU(
            "template.png", threshold=0.99, show_position=False
        )
        best, scores, matches = self.isContainTemplate_max(
            ["template.png", "template.png"],
            threshold=0.99,
            mask_path_list=[None, "mask.png"],
            show_position=False,
        )
        assert best in (0, 1)
        assert len(scores) == 2 and matches == [True, True]
        assert self.isContainedImage(
            "scene.png",
            threshold=0.99,
            crop_fmt="1",
            crop_template=[60, 50, 90, 70],
            show_position=False,
        )

        self.camera.saveCapture(
            "camera-layer", crop=1, crop_ax=[60, 50, 90, 70]
        )
        self.saveCapture(
            "image-layer", crop_fmt="2", crop=[60, 50, 30, 20]
        )
        self.camera.saveCapture(
            "custom-layer",
            img=np.zeros((5, 7, 3), dtype=np.uint8),
            format="jpeg",
        )
        self.popupImage("1", [60, 50, 90, 70], "worker popup")

        self.displayText((10, 12), "ready", ms=25, color="green")
        self.gui.ImgRect(1, 2, 3, 4, "red", 99, 25)
        self.gui.setFps("24")
        self.gui.setShowsize(180, 320)
        self.gui.setTouchscreenArea(-5, 10, 400, 200)
        assert self.gui.touchscreen_area == (0.0, 10 / 180, 1.0, 1.0)
        self.gui.changeRightMouseMode("Qingpi")
        self.gui.BindLeftClick()
        self.gui.UnbindLeftClick()
        self.gui.update()

        self.camera.fps = 60
        self.camera.set_flip("vErTiCaL")
        assert self.camera.flip and self.camera.flip_mode == 0
        self.camera.set_flip("Horizontal")
        assert self.camera.flip_mode == 1
        self.camera.set_flip("Both")
        assert self.camera.flip_mode == -1
        self.camera.set_flip("None")
        assert not self.camera.flip
        self.camera.openCamera("/dev/video-test")
        self.camera.camera_thread_stop()
        self.camera.camera_thread_start()
        self.camera.camera_update()
        self.camera.destroy()
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("images.py"), SOURCE).expect("image script is written");

    let ring =
        SharedFrameRing::create(CaptureResolution::R640x360).expect("test frame ring is created");
    let mut pixels = Vec::with_capacity(640 * 360 * 3);
    for y in 0_u32..360 {
        for x in 0_u32..640 {
            pixels.extend_from_slice(&[
                u8::try_from((x * 3 + y) % 251).unwrap(),
                u8::try_from((x + y * 5) % 253).unwrap(),
                u8::try_from((x * 7 + y * 11) % 255).unwrap(),
            ]);
        }
    }
    let frame = BgrFrame::new(640, 360, pixels).expect("test frame is valid");
    ring.publish(&frame).expect("test frame is published");

    let host = Arc::new(RecordingScriptHost::default());
    *host.camera_ring.lock().unwrap() = Some(ring);
    *host.camera_state.lock().unwrap() = Some(HostCameraState {
        opened: true,
        fps: 30,
        capture_resolution: CaptureResolution::R640x360,
        flip_mode: FlipMode::None,
        screenshot_format: ScreenshotFormat::Png,
    });
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "images.py".into(),
            class_name: "Images".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("image script execution succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);

    let captures = data_root.join("Captures");
    assert!(captures.join("camera-layer.png").is_file());
    assert!(captures.join("image-layer.png").is_file());
    assert!(captures.join("custom-layer.jpg").is_file());
    assert_eq!(host.popup_requests.lock().unwrap().len(), 1);
    assert!(
        host.popup_requests.lock().unwrap()[0].encoded.len() <= protocol::MAX_POPUP_IMAGE_BYTES
    );
    {
        let overlays = host.overlay_requests.lock().unwrap();
        assert!(
            overlays
                .iter()
                .any(|request| matches!(request, HostOverlayRequest::Rectangle { .. }))
        );
        assert!(
            overlays
                .iter()
                .any(|request| matches!(request, HostOverlayRequest::Text { .. }))
        );
        assert_eq!(overlays.last(), Some(&HostOverlayRequest::Cleanup));
    }
    assert_eq!(host.current_camera_state().fps, 60);
    assert!(!host.current_camera_state().opened);

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
async fn fixed_tk_bridge_runs_ui_callbacks_on_one_fifo_thread() {
    const EVENT_TIMEOUT: Duration = Duration::from_secs(10);
    const SOURCE: &str = r##"
import threading
import tkinter as tk
from tkinter import filedialog

from Commands.PythonCommandBase import ImageProcPythonCommand


class TkBridge(ImageProcPythonCommand):
    def changed(self, value=None):
        self.scale_value = self.scales[0].get()
        self.order.append("scale")
        self.callback_threads.append(threading.current_thread().name)
        self.label.config(bg="#112233")

    def clicked(self):
        self.order.append("button")
        self.callback_threads.append(threading.current_thread().name)
        self.print_t1("button callback")

    def do(self):
        try:
            tk.Tk()
        except NotImplementedError as error:
            assert "tkinter.Tk" in str(error)
        else:
            raise AssertionError("native Tk roots must never be created")
        try:
            filedialog.askopenfilename()
        except NotImplementedError:
            pass
        else:
            raise AssertionError("unsupported filedialog must fail explicitly")

        self.order = []
        self.callback_threads = []
        self.scale_value = None
        self.window = tk.Toplevel(self.gui)
        self.window.title("HSV Thresholds")
        self.window.geometry("400x500")
        labels = [
            "Target Hue",
            "Target Sat",
            "Target Val",
            "Hue Error",
            "Sat Error",
            "Val Error",
        ]
        values = [250, 200, 120, 10, 105, 128]
        self.scales = []
        for label, value in zip(labels, values, strict=True):
            scale = tk.Scale(
                self.window,
                from_=0,
                to=360 if label == "Target Hue" else 255,
                orient=tk.HORIZONTAL,
                label=label,
                command=self.changed,
            )
            scale.set(value)
            scale.pack()
            self.scales.append(scale)
        self.button = tk.Button(self.window, text="Run", command=self.clicked)
        self.button.pack(pady=20)
        self.label = tk.Label(
            self.window,
            text="Detected",
            width=20,
            height=2,
            relief=tk.SOLID,
            bg="white",
        )
        self.label.pack(pady=10)

        while len(self.order) < 2:
            self.wait(0.01)
        assert self.order == ["scale", "button"]
        assert self.scale_value == 123
        assert self.callback_threads == [
            "pokecon-tk-callback",
            "pokecon-tk-callback",
        ]
        self.wait(0.2)
        assert self.window._destroyed
"##;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("tk_bridge.py"), SOURCE).expect("Tk bridge script is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let execution_client = client.clone();
    let execution = tokio::spawn(async move {
        execution_client
            .execute(&ScriptExecuteRequest {
                path: "tk_bridge.py".into(),
                class_name: "TkBridge".to_owned(),
                tags: Vec::new(),
            })
            .await
    });

    let (window_id, scale_id, button_id) = tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let activity = host.activity.notified();
            let (window_id, scale_id, button_id, label_packed) = {
                let requests = host.tk_requests.lock().unwrap();
                let window_id = requests.iter().find_map(|request| match request {
                    HostTkRequest::CreateToplevel { window_id } => Some(*window_id),
                    _ => None,
                });
                let scale_id = requests.iter().find_map(|request| match request {
                    HostTkRequest::CreateScale {
                        widget_id,
                        label: Some(label),
                        ..
                    } if label == "Target Hue" => Some(*widget_id),
                    _ => None,
                });
                let button_id = requests.iter().find_map(|request| match request {
                    HostTkRequest::CreateButton {
                        widget_id, text, ..
                    } if text == "Run" => Some(*widget_id),
                    _ => None,
                });
                let label_id = requests.iter().find_map(|request| match request {
                    HostTkRequest::CreateLabel {
                        widget_id, text, ..
                    } if text == "Detected" => Some(*widget_id),
                    _ => None,
                });
                let label_packed = label_id.is_some_and(|label_id| {
                    requests.iter().any(|request| {
                        matches!(
                            request,
                            HostTkRequest::Pack { widget_id, .. } if *widget_id == label_id
                        )
                    })
                });
                (window_id, scale_id, button_id, label_packed)
            };
            if let (Some(window_id), Some(scale_id), Some(button_id), true) =
                (window_id, scale_id, button_id, label_packed)
            {
                break (window_id, scale_id, button_id);
            }
            activity.await;
        }
    })
    .await
    .expect("fixed Tk widgets are projected to the host");

    host.tk_scales.lock().unwrap().insert(scale_id, 123.0);
    client
        .tk_event(&ScriptTkEvent::ScaleChanged {
            widget_id: scale_id,
            value: 123.0,
        })
        .await
        .expect("scale callback event is queued");
    client
        .tk_event(&ScriptTkEvent::ButtonInvoked {
            widget_id: button_id,
        })
        .await
        .expect("button callback event is queued");

    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let activity = host.activity.notified();
            let configured = host.tk_requests.lock().unwrap().iter().any(|request| {
                matches!(
                    request,
                    HostTkRequest::ConfigureLabel {
                        background: Some(background),
                        ..
                    } if background == "#112233"
                )
            });
            let button_ran = host
                .outputs
                .lock()
                .unwrap()
                .iter()
                .any(|output| output.message == "button callback\n");
            if configured && button_ran {
                break;
            }
            activity.await;
        }
    })
    .await
    .expect("FIFO Tk callbacks run while the command thread remains active");

    client
        .tk_event(&ScriptTkEvent::WindowClosed { window_id })
        .await
        .expect("window close event is queued");
    let result = tokio::time::timeout(EVENT_TIMEOUT, execution)
        .await
        .expect("Tk bridge command completes")
        .expect("Tk bridge execution task joins")
        .expect("Tk bridge execution succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);
    assert_eq!(
        host.tk_requests.lock().unwrap().last(),
        Some(&HostTkRequest::Cleanup)
    );

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocking_host_ipc_releases_the_gil_for_tk_event_dispatch() {
    const EVENT_TIMEOUT: Duration = Duration::from_secs(2);
    const SOURCE: &str = r#"
import threading
import tkinter as tk

from Commands.PythonCommandBase import ImageProcPythonCommand


class TkGilBridge(ImageProcPythonCommand):
    def clicked(self):
        self.callback_completed.set()

    def do(self):
        self.callback_completed = threading.Event()
        self.window = tk.Toplevel(self.gui)
        self.button = tk.Button(self.window, text="Release GIL", command=self.clicked)
        self.button.pack()
        self.window.title("Blocking host request")
        assert self.callback_completed.wait(1.0)
"#;

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("tk_gil_bridge.py"), SOURCE)
        .expect("Tk GIL bridge script is written");
    let host = Arc::new(RecordingScriptHost::default());
    host.block_tk_title.store(true, Ordering::Release);
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let execution_client = client.clone();
    let execution = tokio::spawn(async move {
        execution_client
            .execute(&ScriptExecuteRequest {
                path: "tk_gil_bridge.py".into(),
                class_name: "TkGilBridge".to_owned(),
                tags: Vec::new(),
            })
            .await
    });

    let button_id = tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let activity = host.activity.notified();
            let button_id =
                host.tk_requests
                    .lock()
                    .unwrap()
                    .iter()
                    .find_map(|request| match request {
                        HostTkRequest::CreateButton {
                            widget_id, text, ..
                        } if text == "Release GIL" => Some(*widget_id),
                        _ => None,
                    });
            if host.tk_title_blocked.load(Ordering::Acquire)
                && let Some(button_id) = button_id
            {
                break button_id;
            }
            activity.await;
        }
    })
    .await
    .expect("the command blocks inside the host request after creating its button");

    let event_result = tokio::time::timeout(
        EVENT_TIMEOUT,
        client.tk_event(&ScriptTkEvent::ButtonInvoked {
            widget_id: button_id,
        }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    host.release_tk_title();
    event_result
        .expect("Tk events acquire the GIL while another Python thread waits on host IPC")
        .expect("button callback event is queued");

    let result = tokio::time::timeout(EVENT_TIMEOUT, execution)
        .await
        .expect("Tk GIL bridge command completes")
        .expect("Tk GIL bridge execution task joins")
        .expect("Tk GIL bridge execution succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);

    stop_worker(&worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_acknowledges_before_slow_local_finalization_is_reaped() {
    const SOURCE: &str = r"
import time
import _pokecon_script

from Commands.PythonCommandBase import PythonCommand


class SlowShutdown(PythonCommand):
    def do(self):
        original_shutdown = _pokecon_script._shutdown_tk

        def slow_shutdown():
            time.sleep(1.0)
            original_shutdown()

        _pokecon_script._shutdown_tk = slow_shutdown
";

    let (_temporary, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("slow_shutdown.py"), SOURCE)
        .expect("slow shutdown script is written");
    let host = Arc::new(RecordingScriptHost::default());
    let (worker, client) = spawn_client(host).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "slow_shutdown.py".into(),
            class_name: "SlowShutdown".to_owned(),
            tags: Vec::new(),
        })
        .await
        .expect("the slow finalization hook is installed");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);

    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_millis(100))
        .await
        .expect("the worker is reaped at the shutdown deadline");
    assert!(report.cooperative_acknowledged, "{report:?}");
    assert!(report.forced, "{report:?}");
}
