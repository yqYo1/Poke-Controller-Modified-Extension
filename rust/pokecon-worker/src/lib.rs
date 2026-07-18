//! Managed worker IPC, generation gates, and operating-system supervision.

pub mod generation;
pub mod ipc;
pub mod supervisor;

use std::collections::BTreeMap;

use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownReason, install_os_signal_forwarder,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ipc::{
    ConnectionConfig, ConnectionError, Envelope, IpcConnection, IpcErrorPayload, IpcValue,
};

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
    loop {
        let envelope = tokio::select! {
            biased;
            _reason = shutdown.cancelled() => return Ok(()),
            envelope = connection.recv() => envelope?,
        };
        let Envelope::Request { id, op, payload: _ } = envelope else {
            continue;
        };
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
                let response = connection.respond(id, Some(op), IpcValue::Nil).await;
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
