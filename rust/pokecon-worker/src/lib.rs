//! Managed worker IPC, generation gates, and operating-system supervision.

pub mod dynamic;
pub mod generation;
pub mod ipc;
pub mod script;
pub mod supervisor;

use std::collections::BTreeMap;

use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownReason, install_os_signal_forwarder,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dynamic::DynamicWorkerRuntime;
use crate::ipc::{
    ConnectionConfig, ConnectionError, Envelope, IpcConnection, IpcErrorPayload, IpcValue,
};
use crate::script::ScriptWorkerRuntime;

/// Worker roles isolated by the process model in the specification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    /// Profile-specific user-script `CPython` worker.
    Script,
    /// Persistent dynamic Python/Lua configuration worker.
    Dynamic,
}

/// Result of a clean worker lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerSummary {
    /// Worker role that ran.
    pub kind: WorkerKind,
    /// First accepted shutdown reason.
    pub shutdown_reason: ShutdownReason,
}

/// Managed worker runtime failure.
#[derive(Debug, Error)]
pub enum WorkerError {
    /// Reader/writer or protocol failure.
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    /// Operating-system signal task panicked.
    #[error(transparent)]
    SignalTask(#[from] tokio::task::JoinError),
}

/// Runs an isolated worker until a coordinated shutdown request arrives.
///
/// # Errors
///
/// Returns an error if the operating-system signal task cannot be joined.
pub async fn run(kind: WorkerKind, exit_after_startup: bool) -> Result<WorkerSummary, WorkerError> {
    let context = RuntimeContext::native();
    let shutdown = context.shutdown().clone();
    let signal_task = install_os_signal_forwarder(shutdown.clone()).await;
    tracing::info!(
        diagnostic_id = APP_STARTING,
        worker_kind = ?kind,
        "PokeCon worker is ready"
    );

    if exit_after_startup {
        shutdown.request(ShutdownReason::StartupProbe);
    } else {
        let connection = IpcConnection::spawn_without_resources(
            tokio::io::stdin(),
            tokio::io::stdout(),
            ConnectionConfig::default(),
        )?;
        if let Err(error) = run_protocol(kind, &connection, &shutdown).await {
            shutdown.request(ShutdownReason::FatalError(error.to_string()));
            connection.close().await;
            signal_task.await?;
            return Err(error);
        }
        connection.close().await;
    }

    let shutdown_reason = shutdown.cancelled().await;
    signal_task.await?;
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
