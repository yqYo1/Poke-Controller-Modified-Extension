use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pokecon_worker::WorkerKind;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, ScriptDialogState, ScriptExecuteRequest,
    ScriptExecutionOutcome, ScriptInitializeRequest, ScriptWorkerStatus,
};
use pokecon_worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};
use pokecon_worker::supervisor::{ManagedWorker, StopPurpose, WorkerLaunch, WorkerSupervisor};
use tempfile::TempDir;

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
    let worker = supervisor
        .spawn(
            WorkerLaunch::managed(env!("CARGO_BIN_EXE_pokecon-worker"), WorkerKind::Script)
                .clear_environment(),
            host.clone(),
        )
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
    assert!(report.cooperative_acknowledged);
    assert!(!report.forced);
    assert!(report.exit.success);
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
        })
        .await
        .expect("dialog script execution succeeds");
    assert_eq!(result.outcome, ScriptExecutionOutcome::Completed);
    assert_eq!(host.next_dialog_id.load(Ordering::Acquire), 6);
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
    host.fail_notifications.store(true, Ordering::Release);
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    let result = client
        .execute(&ScriptExecuteRequest {
            path: "network.py".into(),
            class_name: "NetworkAndNotifications".to_owned(),
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
            HostNotificationRequest::DiscordImage { settings_keys, crop, .. }
                if settings_keys == &["HOOK_A", "HOOK_B"]
                    && crop.as_deref() == Some(&[0, 0, 10, 10][..])
        ));
        for request in notifications.iter() {
            let debug = format!("{request:?}");
            assert!(!debug.contains("private"));
        }
    }

    stop_worker(&worker).await;
}
