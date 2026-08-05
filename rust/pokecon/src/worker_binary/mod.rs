//! Process entrypoint and child-only runtime composition for the managed worker.

use std::collections::BTreeMap;
use std::future::Future;

use clap::{Parser, ValueEnum};
use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownReason, TracingInitError,
    init_tracing_to_stderr, install_os_signal_forwarder,
};
use thiserror::Error;

use crate::dynamic::DynamicWorkerRuntime;
use crate::worker::WorkerKind;
use crate::worker::ipc::{
    ConnectionConfig, ConnectionError, Envelope, IpcConnection, IpcErrorPayload, IpcValue,
};

pub(crate) mod dynamic;
mod script;

use self::script::ScriptWorkerRuntime;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum WorkerArgument {
    Script,
    Dynamic,
}

impl From<WorkerArgument> for WorkerKind {
    fn from(value: WorkerArgument) -> Self {
        match value {
            WorkerArgument::Script => Self::Script,
            WorkerArgument::Dynamic => Self::Dynamic,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about = "PokeCon managed worker process")]
struct Cli {
    /// Selects the isolated worker role.
    #[arg(long, value_enum)]
    kind: WorkerArgument,
    /// Exit successfully after the worker runtime starts.
    #[arg(long)]
    exit_after_startup: bool,
}

/// Result of a clean worker lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkerSummary {
    /// Worker role that ran.
    kind: WorkerKind,
    /// First accepted shutdown reason.
    shutdown_reason: ShutdownReason,
}

/// Managed worker runtime failure.
#[derive(Debug, Error)]
pub(crate) enum WorkerError {
    /// Reader/writer or protocol failure.
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    /// The operating-system signal task stopped without accepting shutdown.
    #[error("operating-system signal task stopped before shutdown was requested")]
    SignalStopped,
    /// Operating-system signal task panicked or was cancelled unexpectedly.
    #[error(transparent)]
    SignalTask(#[from] tokio::task::JoinError),
}

enum WorkerTaskObservation {
    Shutdown,
    Signal(WorkerSignalTaskResult),
    Protocol(Result<(), WorkerError>),
}

struct WorkerSignalTaskResult {
    early_signal_error: Option<WorkerError>,
    signal_task_consumed: bool,
}

struct WorkerTaskResult {
    protocol_result: Result<(), WorkerError>,
    early_signal_error: Option<WorkerError>,
    signal_task_consumed: bool,
}

#[derive(Debug, Error)]
pub(crate) enum MainError {
    #[error(transparent)]
    Tracing(#[from] TracingInitError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
}

#[tokio::main]
pub(crate) async fn main() -> Result<(), MainError> {
    let cli = Cli::parse();
    init_tracing_to_stderr("info")?;
    run(cli.kind.into(), cli.exit_after_startup).await?;
    Ok(())
}

/// Runs an isolated worker until a coordinated shutdown request arrives.
async fn run(kind: WorkerKind, exit_after_startup: bool) -> Result<WorkerSummary, WorkerError> {
    let context = RuntimeContext::native();
    let shutdown = context.shutdown().clone();
    let mut signal_task = install_os_signal_forwarder(shutdown.clone()).await;
    tracing::info!(
        diagnostic_id = APP_STARTING,
        worker_kind = ?kind,
        "PokeCon worker is ready"
    );

    let task_result = if exit_after_startup {
        shutdown.request(ShutdownReason::StartupProbe);
        WorkerTaskResult {
            protocol_result: Ok(()),
            early_signal_error: None,
            signal_task_consumed: false,
        }
    } else {
        let connection = match IpcConnection::spawn_without_resources(
            tokio::io::stdin(),
            tokio::io::stdout(),
            ConnectionConfig::default(),
        ) {
            Ok(connection) => connection,
            Err(error) => {
                let error = WorkerError::Connection(error);
                shutdown.request(ShutdownReason::FatalError(error.to_string()));
                let _signal_result = signal_task.await;
                return Err(error);
            }
        };
        let task_result = supervise_worker_tasks(
            &shutdown,
            &mut signal_task,
            run_protocol(kind, &connection, &shutdown),
        )
        .await;
        if let Err(error) = &task_result.protocol_result {
            shutdown.request(ShutdownReason::FatalError(error.to_string()));
        }
        connection.close().await;
        task_result
    };

    let shutdown_reason = shutdown.cancelled().await;
    let WorkerTaskResult {
        protocol_result,
        early_signal_error,
        signal_task_consumed,
    } = task_result;
    let signal_result = if signal_task_consumed {
        Ok(())
    } else {
        signal_task.await.map_err(WorkerError::SignalTask)
    };
    if let Some(error) = early_signal_error {
        return Err(error);
    }
    protocol_result?;
    signal_result?;
    tracing::info!(
        diagnostic_id = APP_STOPPED,
        worker_kind = ?kind,
        ?shutdown_reason,
        "PokeCon worker stopped cleanly"
    );
    Ok(WorkerSummary {
        kind,
        shutdown_reason,
    })
}

async fn supervise_worker_tasks(
    shutdown: &pokecon_core::ShutdownCoordinator,
    signal_task: &mut tokio::task::JoinHandle<()>,
    protocol: impl Future<Output = Result<(), WorkerError>>,
) -> WorkerTaskResult {
    tokio::pin!(protocol);
    let observation = tokio::select! {
        biased;
        _reason = shutdown.cancelled() => WorkerTaskObservation::Shutdown,
        result = &mut *signal_task => {
            WorkerTaskObservation::Signal(completed_signal_task_result(shutdown, result))
        }
        result = &mut protocol => WorkerTaskObservation::Protocol(result),
    };
    match observation {
        WorkerTaskObservation::Shutdown => WorkerTaskResult {
            protocol_result: protocol.await,
            early_signal_error: None,
            signal_task_consumed: false,
        },
        WorkerTaskObservation::Signal(result) => WorkerTaskResult {
            protocol_result: protocol.await,
            early_signal_error: result.early_signal_error,
            signal_task_consumed: result.signal_task_consumed,
        },
        WorkerTaskObservation::Protocol(result) => WorkerTaskResult {
            protocol_result: result,
            early_signal_error: None,
            signal_task_consumed: false,
        },
    }
}

fn completed_signal_task_result(
    shutdown: &pokecon_core::ShutdownCoordinator,
    result: Result<(), tokio::task::JoinError>,
) -> WorkerSignalTaskResult {
    let early_signal_error = match result {
        Ok(()) => {
            let error = WorkerError::SignalStopped;
            let fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            fatal_claimed.then_some(error)
        }
        Err(error) => {
            let error = WorkerError::SignalTask(error);
            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Some(error)
        }
    };
    WorkerSignalTaskResult {
        early_signal_error,
        signal_task_consumed: true,
    }
}

async fn run_protocol(
    kind: WorkerKind,
    connection: &IpcConnection,
    shutdown: &pokecon_core::ShutdownCoordinator,
) -> Result<(), WorkerError> {
    let mut dynamic =
        (kind == WorkerKind::Dynamic).then(|| DynamicWorkerRuntime::new(connection.clone()));
    let mut script =
        (kind == WorkerKind::Script).then(|| ScriptWorkerRuntime::new(connection.clone()));
    loop {
        let envelope = tokio::select! {
            biased;
            _reason = shutdown.cancelled() => return Ok(()),
            envelope = connection.recv() => envelope?,
        };
        if let Envelope::Event { op, payload } = &envelope {
            if ScriptWorkerRuntime::handles_event(op)
                && let Some(runtime) = &script
                && let Err(error) = runtime.handle_event(op, payload)
            {
                tracing::warn!(
                    diagnostic_id = "SCRIPT_EVENT_REJECTED",
                    event = %op,
                    code = %error.code,
                    message = %error.message,
                    "script worker rejected an event"
                );
            }
            continue;
        }
        let Envelope::Request { id, op, payload } = envelope else {
            continue;
        };
        if ScriptWorkerRuntime::handles(&op)
            && let Some(runtime) = &mut script
        {
            runtime.handle(id, op, payload).await?;
            continue;
        }
        if DynamicWorkerRuntime::handles(&op)
            && let Some(runtime) = &mut dynamic
        {
            runtime.handle(id, op, payload).await?;
            continue;
        }
        match op.as_str() {
            "worker.ping" => {
                let mut response = BTreeMap::new();
                response.insert(
                    "kind".to_owned(),
                    IpcValue::String(
                        match kind {
                            WorkerKind::Script => "script",
                            WorkerKind::Dynamic => "dynamic",
                        }
                        .to_owned(),
                    ),
                );
                connection
                    .respond(id, Some(op), IpcValue::Map(response))
                    .await?;
            }
            "worker.shutdown" => {
                if let Some(runtime) = &mut script {
                    runtime.begin_shutdown().await;
                }
                // Acknowledge once the actor is quiescent. Auxiliary Python
                // thread finalization remains bounded by the parent's process
                // deadline and must not hide an accepted cooperative stop.
                let response = connection.respond(id, Some(op), IpcValue::Nil).await;
                if let Some(runtime) = &mut script {
                    runtime.finish_shutdown().await;
                }
                shutdown.request(ShutdownReason::WorkerStop);
                return match response {
                    Ok(()) | Err(ConnectionError::Disconnected(_)) => Ok(()),
                    Err(error) => Err(error.into()),
                };
            }
            _ => {
                connection
                    .respond_error(
                        id,
                        op.clone(),
                        IpcErrorPayload::new(
                            "NotFound",
                            format!("unknown worker operation `{op}`"),
                        )
                        .expect("static NotFound code is valid"),
                    )
                    .await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use pokecon_core::{OsSignal, ShutdownCoordinator, ShutdownReason};
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    use super::{WorkerError, completed_signal_task_result, supervise_worker_tasks};

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);

    async fn protocol_until_shutdown(
        shutdown: ShutdownCoordinator,
        completed: Arc<AtomicBool>,
    ) -> Result<(), WorkerError> {
        shutdown.cancelled().await;
        completed.store(true, Ordering::Release);
        Ok(())
    }

    async fn abort_and_join<T>(task: JoinHandle<T>) {
        task.abort();
        let Err(error) = task.await else {
            panic!("pending test task must be cancelled during cleanup");
        };
        assert!(error.is_cancelled());
    }

    #[tokio::test]
    async fn unexpected_clean_signal_task_completion_stops_worker_protocol() {
        let shutdown = ShutdownCoordinator::new();
        let protocol_completed = Arc::new(AtomicBool::new(false));
        let mut signal_task = tokio::spawn(async {});

        let result = timeout(
            TEST_TIMEOUT,
            supervise_worker_tasks(
                &shutdown,
                &mut signal_task,
                protocol_until_shutdown(shutdown.clone(), Arc::clone(&protocol_completed)),
            ),
        )
        .await
        .expect("signal task completion must stop the worker protocol before the deadline");

        assert!(result.signal_task_consumed);
        assert!(result.protocol_result.is_ok());
        assert!(matches!(
            result.early_signal_error,
            Some(WorkerError::SignalStopped)
        ));
        assert!(protocol_completed.load(Ordering::Acquire));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "operating-system signal task stopped before shutdown was requested".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn panicked_signal_task_stops_worker_protocol() {
        let shutdown = ShutdownCoordinator::new();
        let protocol_completed = Arc::new(AtomicBool::new(false));
        let mut signal_task = tokio::spawn(async { panic!("injected worker signal panic") });

        let result = timeout(
            TEST_TIMEOUT,
            supervise_worker_tasks(
                &shutdown,
                &mut signal_task,
                protocol_until_shutdown(shutdown.clone(), Arc::clone(&protocol_completed)),
            ),
        )
        .await
        .expect("signal task panic must stop the worker protocol before the deadline");

        assert!(result.signal_task_consumed);
        assert!(result.protocol_result.is_ok());
        let Some(WorkerError::SignalTask(error)) = result.early_signal_error else {
            panic!("signal task panic must retain its join error");
        };
        assert!(error.is_panic());
        assert!(protocol_completed.load(Ordering::Acquire));
        let Some(ShutdownReason::FatalError(reason)) = shutdown.reason() else {
            panic!("signal task panic must request fatal worker shutdown");
        };
        assert!(reason.contains("injected worker signal panic"));
    }

    #[tokio::test]
    async fn cancelled_signal_task_stops_worker_protocol() {
        let shutdown = ShutdownCoordinator::new();
        let protocol_completed = Arc::new(AtomicBool::new(false));
        let mut signal_task = tokio::spawn(pending::<()>());
        signal_task.abort();

        let result = timeout(
            TEST_TIMEOUT,
            supervise_worker_tasks(
                &shutdown,
                &mut signal_task,
                protocol_until_shutdown(shutdown.clone(), Arc::clone(&protocol_completed)),
            ),
        )
        .await
        .expect("signal task cancellation must stop the worker protocol before the deadline");

        assert!(result.signal_task_consumed);
        assert!(result.protocol_result.is_ok());
        let Some(WorkerError::SignalTask(error)) = result.early_signal_error else {
            panic!("signal task cancellation must retain its join error");
        };
        assert!(error.is_cancelled());
        assert!(protocol_completed.load(Ordering::Acquire));
        assert!(matches!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(reason)) if reason.ends_with(" was cancelled")
        ));
    }

    #[tokio::test]
    async fn self_coordinating_signal_task_completion_remains_clean_for_worker() {
        let shutdown = ShutdownCoordinator::new();
        let protocol_completed = Arc::new(AtomicBool::new(false));
        let signal_shutdown = shutdown.clone();
        let mut signal_task = tokio::spawn(async move {
            assert!(signal_shutdown.request(ShutdownReason::Signal(OsSignal::Interrupt)));
        });

        let result = timeout(
            TEST_TIMEOUT,
            supervise_worker_tasks(
                &shutdown,
                &mut signal_task,
                protocol_until_shutdown(shutdown.clone(), Arc::clone(&protocol_completed)),
            ),
        )
        .await
        .expect("coordinated signal completion must stop the protocol before the deadline");

        assert!(result.protocol_result.is_ok());
        assert!(result.early_signal_error.is_none());
        assert!(protocol_completed.load(Ordering::Acquire));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::Signal(OsSignal::Interrupt))
        );
        if !result.signal_task_consumed {
            signal_task
                .await
                .expect("unconsumed coordinated signal task must remain joinable exactly once");
        }
    }

    #[tokio::test]
    async fn preaccepted_shutdown_wins_over_completed_worker_signal_task() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let protocol_completed = Arc::new(AtomicBool::new(false));
        let completed = Arc::clone(&protocol_completed);
        let protocol = async move {
            completed.store(true, Ordering::Release);
            Ok(())
        };
        let mut signal_task = tokio::spawn(async {});

        let result = timeout(
            TEST_TIMEOUT,
            supervise_worker_tasks(&shutdown, &mut signal_task, protocol),
        )
        .await
        .expect("accepted worker shutdown must win before the deadline");

        assert!(!result.signal_task_consumed);
        assert!(result.protocol_result.is_ok());
        assert!(result.early_signal_error.is_none());
        assert!(protocol_completed.load(Ordering::Acquire));
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
        signal_task
            .await
            .expect("biased shutdown branch must leave the worker signal handle joinable");
    }

    #[tokio::test]
    async fn completed_worker_signal_task_after_preaccepted_shutdown_is_consumed_cleanly() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let signal_result = tokio::spawn(async {}).await;

        let result = completed_signal_task_result(&shutdown, signal_result);

        assert!(result.early_signal_error.is_none());
        assert!(result.signal_task_consumed);
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[tokio::test]
    async fn cancelled_worker_signal_task_after_preaccepted_shutdown_retains_typed_error() {
        let shutdown = ShutdownCoordinator::new();
        assert!(shutdown.request(ShutdownReason::StartupProbe));
        let signal_task = tokio::spawn(pending::<()>());
        signal_task.abort();
        let signal_result = signal_task.await;

        let result = completed_signal_task_result(&shutdown, signal_result);

        assert!(result.signal_task_consumed);
        let Some(WorkerError::SignalTask(error)) = result.early_signal_error else {
            panic!("worker signal task cancellation must retain its join error");
        };
        assert!(error.is_cancelled());
        assert_eq!(shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[tokio::test]
    async fn protocol_completion_leaves_pending_signal_task_for_final_join() {
        let shutdown = ShutdownCoordinator::new();
        let mut signal_task = tokio::spawn(pending::<()>());

        let result = timeout(
            TEST_TIMEOUT,
            supervise_worker_tasks(&shutdown, &mut signal_task, async { Ok(()) }),
        )
        .await
        .expect("worker protocol completion must be observed before the deadline");

        assert!(!result.signal_task_consumed);
        assert!(result.protocol_result.is_ok());
        assert!(result.early_signal_error.is_none());
        assert_eq!(shutdown.reason(), None);
        abort_and_join(signal_task).await;
    }
}
