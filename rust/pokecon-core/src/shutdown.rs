use std::io;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{SHUTDOWN_REQUESTED, SIGNAL_HANDLER_FAILED};

/// Operating-system signals normalized across supported platforms.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsSignal {
    /// Ctrl+C, SIGINT, or the equivalent console interrupt.
    Interrupt,
    /// SIGTERM on Unix or the equivalent process termination request.
    Terminate,
}

/// The first accepted reason for a process-wide shutdown.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum ShutdownReason {
    /// An operating-system signal requested termination.
    Signal(OsSignal),
    /// The Tauri lifecycle requested complete application shutdown.
    DesktopExit,
    /// A startup probe requested an immediate clean exit.
    StartupProbe,
    /// The parent process requested cooperative worker termination over IPC.
    WorkerStop,
    /// An unrecoverable process error requested shutdown.
    FatalError(String),
}

#[derive(Debug)]
struct ShutdownState {
    cancellation: CancellationToken,
    reason: OnceLock<ShutdownReason>,
}

/// A cloneable, first-writer-wins shutdown coordinator.
#[derive(Clone, Debug)]
pub struct ShutdownCoordinator {
    state: Arc<ShutdownState>,
}

impl ShutdownCoordinator {
    /// Creates an idle coordinator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(ShutdownState {
                cancellation: CancellationToken::new(),
                reason: OnceLock::new(),
            }),
        }
    }

    /// Returns a token cancelled by the first accepted shutdown request.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.state.cancellation.clone()
    }

    /// Records a shutdown reason and cancels all dependent tasks.
    ///
    /// Returns `true` only for the first request.
    pub fn request(&self, reason: ShutdownReason) -> bool {
        if self.state.reason.set(reason).is_err() {
            return false;
        }
        if let Some(accepted_reason) = self.state.reason.get() {
            tracing::info!(
                diagnostic_id = SHUTDOWN_REQUESTED,
                shutdown_reason = ?accepted_reason,
                "shutdown requested"
            );
        }
        self.state.cancellation.cancel();
        true
    }

    /// Waits until shutdown is requested and returns the accepted reason.
    pub async fn cancelled(&self) -> ShutdownReason {
        self.state.cancellation.cancelled().await;
        self.state.reason.get().cloned().unwrap_or_else(|| {
            ShutdownReason::FatalError("shutdown cancellation occurred without a reason".to_owned())
        })
    }

    /// Returns the accepted reason, if shutdown has begun.
    #[must_use]
    pub fn reason(&self) -> Option<ShutdownReason> {
        self.state.reason.get().cloned()
    }
}

impl Default for ShutdownCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Installs a task that forwards SIGINT/SIGTERM equivalents to a coordinator.
///
/// The returned future does not complete until the platform signal streams are
/// registered (or registration failure has requested a fatal shutdown). This
/// removes the startup race between publishing a ready process and polling the
/// signal task for the first time.
#[must_use]
pub async fn install_os_signal_forwarder(coordinator: ShutdownCoordinator) -> JoinHandle<()> {
    let (ready_sender, ready_receiver) = oneshot::channel();
    let signal_task = tokio::spawn(async move {
        tokio::select! {
            () = coordinator.cancellation_token().cancelled_owned() => {}
            signal = wait_for_os_signal(ready_sender) => {
                match signal {
                    Ok(signal) => {
                        coordinator.request(ShutdownReason::Signal(signal));
                    }
                    Err(error) => {
                        tracing::error!(
                            diagnostic_id = SIGNAL_HANDLER_FAILED,
                            %error,
                            "operating-system signal handler failed"
                        );
                        coordinator.request(ShutdownReason::FatalError(error.to_string()));
                    }
                }
            }
        }
    });
    let _registration_result = ready_receiver.await;
    signal_task
}

#[cfg(unix)]
async fn wait_for_os_signal(ready: oneshot::Sender<()>) -> io::Result<OsSignal> {
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let _ready_result = ready.send(());
    tokio::select! {
        signal = interrupt.recv() => signal
            .map(|()| OsSignal::Interrupt)
            .ok_or_else(|| io::Error::other("SIGINT stream closed unexpectedly")),
        signal = terminate.recv() => signal
            .map(|()| OsSignal::Terminate)
            .ok_or_else(|| io::Error::other("SIGTERM stream closed unexpectedly")),
    }
}

#[cfg(windows)]
async fn wait_for_os_signal(ready: oneshot::Sender<()>) -> io::Result<OsSignal> {
    let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
    let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
    let _ready_result = ready.send(());
    tokio::select! {
        signal = ctrl_c.recv() => signal
            .map(|()| OsSignal::Interrupt)
            .ok_or_else(|| io::Error::other("Ctrl+C stream closed unexpectedly")),
        signal = ctrl_break.recv() => signal
            .map(|()| OsSignal::Terminate)
            .ok_or_else(|| io::Error::other("Ctrl+Break stream closed unexpectedly")),
    }
}

#[cfg(not(any(unix, windows)))]
async fn wait_for_os_signal(ready: oneshot::Sender<()>) -> io::Result<OsSignal> {
    let _ready_result = ready.send(());
    tokio::signal::ctrl_c().await?;
    Ok(OsSignal::Interrupt)
}

#[cfg(test)]
mod tests {
    use super::{ShutdownCoordinator, ShutdownReason};

    #[tokio::test]
    async fn first_shutdown_reason_wins() {
        let coordinator = ShutdownCoordinator::new();
        assert!(coordinator.request(ShutdownReason::StartupProbe));
        assert!(!coordinator.request(ShutdownReason::DesktopExit));
        assert_eq!(coordinator.cancelled().await, ShutdownReason::StartupProbe);
    }
}
