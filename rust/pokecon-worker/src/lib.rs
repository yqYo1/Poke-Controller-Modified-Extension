//! Minimal independent process lifecycle for both managed worker roles.

use pokecon_core::{
    APP_STARTING, APP_STOPPED, RuntimeContext, ShutdownReason, install_os_signal_forwarder,
};
use serde::{Deserialize, Serialize};

/// Worker roles isolated by the process model in the specification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

/// Runs an isolated worker until a coordinated shutdown request arrives.
///
/// # Errors
///
/// Returns an error if the operating-system signal task cannot be joined.
pub async fn run(
    kind: WorkerKind,
    exit_after_startup: bool,
) -> Result<WorkerSummary, tokio::task::JoinError> {
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
