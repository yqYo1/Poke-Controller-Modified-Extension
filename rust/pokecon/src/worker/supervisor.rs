use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout, timeout_at};

use crate::worker::WorkerKind;
use crate::worker::generation::{
    GenerationError, GenerationManager, GenerationPhase, OperationClass, WorkerGeneration,
};
use crate::worker::ipc::{
    ConnectionConfig, ConnectionError, DisconnectReason, IpcConnection, IpcValue, ResourceSafety,
};

const OOB_QUEUE_CAPACITY: usize = 128;

/// Why a parent is stopping a worker process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopPurpose {
    /// Profile switch affects only the user-script worker.
    ProfileSwitch,
    /// Complete application shutdown; the sole forced-stop exception for the
    /// persistent dynamic worker.
    ApplicationShutdown,
}

/// Redacted child-process launch description.
#[derive(Clone)]
pub struct WorkerLaunch {
    kind: WorkerKind,
    program: PathBuf,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    clear_environment: bool,
    current_directory: Option<PathBuf>,
}

impl std::fmt::Debug for WorkerLaunch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkerLaunch")
            .field("kind", &self.kind)
            .field("program", &self.program)
            .field("argument_count", &self.arguments.len())
            .field("environment_key_count", &self.environment.len())
            .field("clear_environment", &self.clear_environment)
            .field("current_directory", &self.current_directory)
            .finish()
    }
}

impl WorkerLaunch {
    /// Builds the normal `pokecon-worker --kind ...` command.
    #[must_use]
    pub fn managed(program: impl Into<PathBuf>, kind: WorkerKind) -> Self {
        let kind_name = match kind {
            WorkerKind::Script => "script",
            WorkerKind::Dynamic => "dynamic",
        };
        Self {
            kind,
            program: program.into(),
            arguments: vec!["--kind".into(), kind_name.into()],
            environment: Vec::new(),
            clear_environment: false,
            current_directory: None,
        }
    }

    /// Builds a custom command for hermetic worker/fault fixtures.
    #[must_use]
    pub fn custom(program: impl Into<PathBuf>, kind: WorkerKind) -> Self {
        Self {
            kind,
            program: program.into(),
            arguments: Vec::new(),
            environment: Vec::new(),
            clear_environment: false,
            current_directory: None,
        }
    }

    /// Appends one child argument.
    #[must_use]
    pub fn argument(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.arguments.push(argument.as_ref().to_owned());
        self
    }

    /// Sets one inherited child environment override.
    #[must_use]
    pub fn environment(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.environment
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    /// Starts the child from an empty environment. Callers must explicitly
    /// restore every safe variable required by the worker role.
    #[must_use]
    pub const fn clear_environment(mut self) -> Self {
        self.clear_environment = true;
        self
    }

    /// Sets the child working directory.
    #[must_use]
    pub fn current_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.current_directory = Some(directory.into());
        self
    }

    /// Returns the worker role attached to this launch.
    #[must_use]
    pub const fn kind(&self) -> WorkerKind {
        self.kind
    }
}

/// Raw stderr bytes captured out-of-band from a worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OobDiagnostic {
    /// Diagnostic bytes. They are never interpreted as protocol frames.
    pub bytes: Vec<u8>,
}

/// Reaped operating-system process result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitReport {
    /// Whether the child returned a success status.
    pub success: bool,
    /// Portable numeric status when the OS exposes one.
    pub code: Option<i32>,
}

impl From<ExitStatus> for ExitReport {
    fn from(status: ExitStatus) -> Self {
        Self {
            success: status.success(),
            code: status.code(),
        }
    }
}

/// Result of cooperative/deadline-based worker stop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StopReport {
    /// Final reaped status.
    pub exit: ExitReport,
    /// Whether `worker.shutdown` returned a success response before exit.
    pub cooperative_acknowledged: bool,
    /// Whether the supervisor had to force termination after the deadline.
    pub forced: bool,
}

/// Child launch, policy, or process-management failure.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// Child creation or OS process operation failed.
    #[error("worker process operation failed: {0}")]
    Process(String),
    /// Per-role generation invariant rejected activation.
    #[error(transparent)]
    Generation(#[from] GenerationError),
    /// Profile switching must not stop/recreate the persistent dynamic worker.
    #[error("profile switching cannot stop the persistent dynamic worker")]
    DynamicProfileSwitchForbidden,
    /// A stopping script must be reaped before replacement.
    #[error("worker is already stopping")]
    AlreadyStopping,
}

/// Generation-gate or transport failure for a generation-bound request.
#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum WorkerRequestError {
    /// The worker generation no longer permits the requested operation class.
    #[error(transparent)]
    Generation(#[from] GenerationError),
    /// The request failed at the IPC transport or remote operation boundary.
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

#[derive(Clone, Debug)]
struct ActorFailure(String);

type ExitOutcome = Result<ExitReport, ActorFailure>;

enum ActorCommand {
    Stop {
        purpose: StopPurpose,
        deadline: Duration,
        response: oneshot::Sender<Result<StopReport, ActorFailure>>,
    },
}

/// One supervised worker generation. The process actor always owns and reaps
/// the child, even when every external handle is dropped.
pub struct ManagedWorker {
    kind: WorkerKind,
    generation: Arc<WorkerGeneration>,
    connection: IpcConnection,
    commands: mpsc::Sender<ActorCommand>,
    exit: watch::Receiver<Option<ExitOutcome>>,
    diagnostics: Mutex<Option<mpsc::Receiver<OobDiagnostic>>>,
    dropped_diagnostics: Arc<AtomicUsize>,
}

impl std::fmt::Debug for ManagedWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedWorker")
            .field("kind", &self.kind)
            .field("generation", &self.generation.id())
            .field("phase", &self.generation.phase())
            .finish_non_exhaustive()
    }
}

impl ManagedWorker {
    /// Worker role.
    #[must_use]
    pub const fn kind(&self) -> WorkerKind {
        self.kind
    }

    /// Per-process generation gate.
    #[must_use]
    pub fn generation(&self) -> &Arc<WorkerGeneration> {
        &self.generation
    }

    /// Typed IPC endpoint.
    #[must_use]
    pub const fn connection(&self) -> &IpcConnection {
        &self.connection
    }

    /// Issues an operation through this worker's generation gate.
    ///
    /// Mutating requests use the per-generation cancellation token, so entering
    /// `stopping` cancels queued waiters and their late responses are discarded.
    ///
    /// # Errors
    ///
    /// Returns a stopping/stopped generation error or an IPC request error.
    pub async fn request(
        &self,
        class: OperationClass,
        operation: impl Into<String>,
        payload: IpcValue,
    ) -> Result<IpcValue, WorkerRequestError> {
        self.generation.permit(class)?;
        if class == OperationClass::MutatingResource {
            Ok(self
                .connection
                .request_with_cancellation(operation, payload, self.generation.cancellation_token())
                .await?)
        } else {
            Ok(self.connection.request(operation, payload).await?)
        }
    }

    /// Takes the bounded out-of-band stderr receiver. It can be taken once.
    #[must_use]
    pub fn take_diagnostics(&self) -> Option<mpsc::Receiver<OobDiagnostic>> {
        self.diagnostics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Count of stderr chunks dropped to ensure a noisy worker cannot deadlock
    /// on its diagnostic pipe.
    #[must_use]
    pub fn dropped_diagnostic_count(&self) -> usize {
        self.dropped_diagnostics.load(Ordering::Acquire)
    }

    /// Waits for the process actor's single OS reap result.
    ///
    /// # Errors
    ///
    /// Returns the OS wait failure recorded by the process actor.
    pub async fn wait(&self) -> Result<ExitReport, SupervisorError> {
        let mut exit = self.exit.clone();
        loop {
            if let Some(outcome) = exit.borrow().clone() {
                return outcome.map_err(|failure| SupervisorError::Process(failure.0));
            }
            exit.changed()
                .await
                .map_err(|error| SupervisorError::Process(error.to_string()))?;
        }
    }

    /// Requests cooperative stop, waits to the deadline, then applies only the
    /// force-termination paths permitted by the specification.
    ///
    /// # Errors
    ///
    /// Rejects profile-switch stop for the dynamic worker or reports an OS
    /// process failure.
    pub async fn stop(
        &self,
        purpose: StopPurpose,
        deadline: Duration,
    ) -> Result<StopReport, SupervisorError> {
        if self.kind == WorkerKind::Dynamic && purpose == StopPurpose::ProfileSwitch {
            return Err(SupervisorError::DynamicProfileSwitchForbidden);
        }
        if self.generation.phase() == GenerationPhase::Stopped {
            return Ok(StopReport {
                exit: self.wait().await?,
                cooperative_acknowledged: false,
                forced: false,
            });
        }
        let (response_sender, response_receiver) = oneshot::channel();
        self.commands
            .send(ActorCommand::Stop {
                purpose,
                deadline,
                response: response_sender,
            })
            .await
            .map_err(|_| SupervisorError::AlreadyStopping)?;
        match response_receiver.await {
            Ok(Ok(report)) => Ok(report),
            Ok(Err(failure)) => Err(SupervisorError::Process(failure.0)),
            Err(_) => Ok(StopReport {
                exit: self.wait().await?,
                cooperative_acknowledged: false,
                forced: false,
            }),
        }
    }
}

/// Common process supervisor for both worker roles.
#[derive(Debug, Default)]
pub struct WorkerSupervisor {
    generations: Arc<GenerationManager>,
    workers: tokio::sync::Mutex<HashMap<WorkerKind, Arc<ManagedWorker>>>,
}

impl WorkerSupervisor {
    /// Creates an empty supervisor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the generation registry shared with request dispatch gates.
    #[must_use]
    pub fn generations(&self) -> &Arc<GenerationManager> {
        &self.generations
    }

    /// Returns the current managed worker for a role.
    pub async fn current(&self, kind: WorkerKind) -> Option<Arc<ManagedWorker>> {
        self.workers.lock().await.get(&kind).cloned()
    }

    /// Ensures a role has a supervised process.
    ///
    /// A running worker is reused. A reaped script worker can be replaced. A
    /// dynamic worker is never regenerated after its first successful spawn.
    ///
    /// # Errors
    ///
    /// Returns a generation-policy or child launch error.
    pub async fn spawn(
        &self,
        launch: WorkerLaunch,
        resources: Arc<dyn ResourceSafety>,
    ) -> Result<Arc<ManagedWorker>, SupervisorError> {
        let mut workers = self.workers.lock().await;
        if let Some(existing) = workers.get(&launch.kind) {
            match existing.generation.phase() {
                GenerationPhase::Running => return Ok(existing.clone()),
                GenerationPhase::Stopping => return Err(SupervisorError::AlreadyStopping),
                GenerationPhase::Stopped if launch.kind == WorkerKind::Dynamic => {
                    return Err(SupervisorError::Generation(
                        GenerationError::DynamicRestartForbidden,
                    ));
                }
                GenerationPhase::Stopped => {}
            }
        }

        let mut command = build_command(&launch);
        let mut child = command
            .spawn()
            .map_err(|error| SupervisorError::Process(error.to_string()))?;
        let Some(stdin) = child.stdin.take() else {
            reap_failed_launch(&mut child).await;
            return Err(SupervisorError::Process(
                "spawned worker has no stdin pipe".to_owned(),
            ));
        };
        let Some(stdout) = child.stdout.take() else {
            reap_failed_launch(&mut child).await;
            return Err(SupervisorError::Process(
                "spawned worker has no stdout pipe".to_owned(),
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            reap_failed_launch(&mut child).await;
            return Err(SupervisorError::Process(
                "spawned worker has no stderr pipe".to_owned(),
            ));
        };

        let generation = match self.generations.activate(launch.kind) {
            Ok(generation) => generation,
            Err(error) => {
                reap_failed_launch(&mut child).await;
                return Err(error.into());
            }
        };
        let connection =
            IpcConnection::spawn(stdout, stdin, ConnectionConfig::default(), resources)
                .map_err(|error| SupervisorError::Process(error.to_string()))?;
        let (diagnostic_sender, diagnostic_receiver) = mpsc::channel(OOB_QUEUE_CAPACITY);
        let dropped_diagnostics = Arc::new(AtomicUsize::new(0));
        let stderr_task = tokio::spawn(read_stderr(
            stderr,
            diagnostic_sender,
            dropped_diagnostics.clone(),
        ));
        let (command_sender, command_receiver) = mpsc::channel(1);
        let (exit_sender, exit_receiver) = watch::channel(None);
        tokio::spawn(process_actor(
            child,
            connection.clone(),
            generation.clone(),
            command_receiver,
            exit_sender,
            stderr_task,
        ));
        let worker = Arc::new(ManagedWorker {
            kind: launch.kind,
            generation,
            connection,
            commands: command_sender,
            exit: exit_receiver,
            diagnostics: Mutex::new(Some(diagnostic_receiver)),
            dropped_diagnostics,
        });
        workers.insert(launch.kind, worker.clone());
        Ok(worker)
    }

    /// Stops and reaps all current workers for complete application shutdown.
    ///
    /// The caller supplies the user-script configured deadline; dynamic worker
    /// always uses the fixed internal two-second deadline.
    pub async fn shutdown_all(
        &self,
        script_deadline: Duration,
    ) -> Vec<(WorkerKind, Result<StopReport, SupervisorError>)> {
        let workers = self
            .workers
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut tasks = JoinSet::new();
        for worker in workers {
            tasks.spawn(async move {
                let kind = worker.kind();
                let deadline = if kind == WorkerKind::Dynamic {
                    Duration::from_secs(2)
                } else {
                    script_deadline
                };
                (
                    kind,
                    worker
                        .stop(StopPurpose::ApplicationShutdown, deadline)
                        .await,
                )
            });
        }
        let mut reports = Vec::new();
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(report) => reports.push(report),
                Err(error) => reports.push((
                    WorkerKind::Script,
                    Err(SupervisorError::Process(error.to_string())),
                )),
            }
        }
        reports
    }
}

fn build_command(launch: &WorkerLaunch) -> Command {
    let mut command = Command::new(&launch.program);
    if launch.clear_environment {
        command.env_clear();
    }
    command
        .args(&launch.arguments)
        .envs(launch.environment.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    if let Some(directory) = &launch.current_directory {
        command.current_dir(directory);
    }
    command
}

async fn reap_failed_launch(child: &mut Child) {
    let _kill_result = child.start_kill();
    let _wait_result = child.wait().await;
}

async fn read_stderr(
    mut stderr: tokio::process::ChildStderr,
    diagnostics: mpsc::Sender<OobDiagnostic>,
    dropped: Arc<AtomicUsize>,
) {
    let mut buffer = vec![0_u8; 4096];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(length) => {
                if diagnostics
                    .try_send(OobDiagnostic {
                        bytes: buffer[..length].to_vec(),
                    })
                    .is_err()
                {
                    dropped.fetch_add(1, Ordering::AcqRel);
                }
            }
        }
    }
}

async fn process_actor(
    mut child: Child,
    connection: IpcConnection,
    generation: Arc<WorkerGeneration>,
    mut commands: mpsc::Receiver<ActorCommand>,
    exit_sender: watch::Sender<Option<ExitOutcome>>,
    stderr_task: JoinHandle<()>,
) {
    let outcome = tokio::select! {
        status = child.wait() => {
            status
                .map(ExitReport::from)
                .map_err(|error| ActorFailure(error.to_string()))
        }
        command = commands.recv() => {
            let Some(ActorCommand::Stop { purpose, deadline, response }) = command else {
                return finish_process_actor(
                    child.wait()
                        .await
                        .map(ExitReport::from)
                        .map_err(|error| ActorFailure(error.to_string())),
                    connection,
                    generation,
                    exit_sender,
                    stderr_task,
                )
                .await;
            };
            match stop_child(
                &mut child,
                &connection,
                &generation,
                purpose,
                deadline,
            )
            .await
            {
                Ok(report) => {
                    let exit = report.exit.clone();
                    let _response_result = response.send(Ok(report));
                    Ok(exit)
                }
                Err(failure) => {
                    let _response_result = response.send(Err(failure.clone()));
                    Err(failure)
                }
            }
        }
    };

    finish_process_actor(outcome, connection, generation, exit_sender, stderr_task).await;
}

async fn finish_process_actor(
    outcome: ExitOutcome,
    connection: IpcConnection,
    generation: Arc<WorkerGeneration>,
    exit_sender: watch::Sender<Option<ExitOutcome>>,
    stderr_task: JoinHandle<()>,
) {
    generation.mark_stopped();
    let _reader_drain = timeout(Duration::from_millis(100), connection.wait_disconnected()).await;
    connection.disconnect(DisconnectReason::ProcessExited);
    connection.close().await;
    finish_stderr_task(stderr_task).await;
    let _send_result = exit_sender.send(Some(outcome));
}

async fn stop_child(
    child: &mut Child,
    connection: &IpcConnection,
    generation: &WorkerGeneration,
    purpose: StopPurpose,
    deadline: Duration,
) -> Result<StopReport, ActorFailure> {
    if generation.kind() == WorkerKind::Dynamic && purpose == StopPurpose::ProfileSwitch {
        return Err(ActorFailure(
            "profile switching cannot stop the dynamic worker".to_owned(),
        ));
    }
    generation.begin_stopping();
    connection.force_release_resources();
    let deadline_at = Instant::now() + deadline;
    let cooperative_acknowledged = if deadline.is_zero() {
        false
    } else {
        matches!(
            timeout_at(
                deadline_at,
                connection.request("worker.shutdown", IpcValue::Nil)
            )
            .await,
            Ok(Ok(_))
        )
    };
    // Tokio's process-global stdin adapter can retain an internal blocking read.
    // Closing the parent's pipe after the cooperative exchange both forbids new
    // work for this stopping generation and guarantees that read observes EOF.
    connection.close().await;

    if let Some(status) = child
        .try_wait()
        .map_err(|error| ActorFailure(error.to_string()))?
    {
        return Ok(StopReport {
            exit: status.into(),
            cooperative_acknowledged,
            forced: false,
        });
    }

    if Instant::now() < deadline_at
        && let Ok(status) = timeout_at(deadline_at, child.wait()).await
    {
        return status
            .map(|status| StopReport {
                exit: status.into(),
                cooperative_acknowledged,
                forced: false,
            })
            .map_err(|error| ActorFailure(error.to_string()));
    }

    child
        .start_kill()
        .map_err(|error| ActorFailure(error.to_string()))?;
    let status = child
        .wait()
        .await
        .map_err(|error| ActorFailure(error.to_string()))?;
    Ok(StopReport {
        exit: status.into(),
        cooperative_acknowledged,
        forced: true,
    })
}

async fn finish_stderr_task(mut task: JoinHandle<()>) {
    if timeout(Duration::from_millis(100), &mut task)
        .await
        .is_err()
    {
        task.abort();
        let _result = task.await;
    }
}
