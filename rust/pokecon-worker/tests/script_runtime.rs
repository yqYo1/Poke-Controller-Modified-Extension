use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pokecon_worker::WorkerKind;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    HostControllerInputRequest, HostOutputRequest, ScriptExecuteRequest, ScriptExecutionOutcome,
    ScriptInitializeRequest, ScriptWorkerStatus,
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
