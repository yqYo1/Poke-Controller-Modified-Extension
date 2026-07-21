//! Profile-scoped Python user-script worker, typed host proxies, and client.

pub mod protocol;
mod python;
mod runtime;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::WorkerKind;
use crate::generation::OperationClass;
use crate::ipc::{
    Envelope, IpcConnection, IpcErrorPayload, IpcValue, LogPayload, ValueCodecError,
    deserialize_value, serialize_value,
};
use crate::supervisor::{ManagedWorker, WorkerRequestError};

use self::protocol::{
    HostControllerInputRequest, HostOutputRequest, HostSerialWriteRequest,
    HostSerialWriteRowRequest, ScriptExecuteRequest, ScriptExecutionResult,
    ScriptInitializeRequest, ScriptInitializeResult, ScriptStopResult, ScriptWorkerStatus,
};
pub(crate) use self::runtime::ScriptWorkerRuntime;

const LOG_QUEUE_CAPACITY: usize = 128;

/// Stable failure returned by a Rust-main user-script proxy operation.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("script host error {code}: {message}")]
pub struct ScriptHostError {
    pub code: String,
    pub message: String,
}

impl ScriptHostError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Rust-main-owned operations available to one user-script generation.
pub trait ScriptHost: Send + Sync + 'static {
    /// Applies one script-owned controller press or release.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the input cannot be applied.
    fn controller_input(&self, request: HostControllerInputRequest) -> Result<(), ScriptHostError>;

    /// Forces the script input source to its neutral state.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when neutralization cannot be applied.
    fn controller_neutral(&self) -> Result<(), ScriptHostError>;

    /// Writes uninterpreted bytes to the active serial transport.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the write fails.
    fn serial_write(&self, data: Vec<u8>) -> Result<(), ScriptHostError>;

    /// Writes one compatibility row to the active serial transport.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the row cannot be written.
    fn serial_write_row(&self, row: String) -> Result<(), ScriptHostError>;

    /// Reloads the active serial connection.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when reconnection fails.
    fn serial_reload(&self) -> Result<(), ScriptHostError>;

    /// Routes command output to the selected Rust-owned output target.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the output cannot be accepted.
    fn output(&self, request: HostOutputRequest) -> Result<(), ScriptHostError>;
}

/// Parent-side user-script client or dispatcher failure.
#[derive(Debug, thiserror::Error)]
pub enum ScriptClientError {
    #[error("user-script worker client requires a script worker")]
    WrongWorkerKind,
    #[error(transparent)]
    Worker(#[from] WorkerRequestError),
    #[error(transparent)]
    Payload(#[from] ValueCodecError),
}

/// Rust-main handle for one profile-specific user-script worker generation.
pub struct ScriptWorkerClient {
    worker: Arc<ManagedWorker>,
    logs: Mutex<Option<mpsc::Receiver<LogPayload>>>,
    dropped_logs: Arc<AtomicUsize>,
    dispatcher: JoinHandle<()>,
}

impl std::fmt::Debug for ScriptWorkerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptWorkerClient")
            .field("generation", &self.worker.generation().id())
            .field("dropped_logs", &self.dropped_log_count())
            .finish_non_exhaustive()
    }
}

impl ScriptWorkerClient {
    /// Attaches the sole inbound dispatcher before initialization or execution.
    ///
    /// # Errors
    ///
    /// Rejects a dynamic worker handle.
    pub fn attach(
        worker: Arc<ManagedWorker>,
        host: Arc<dyn ScriptHost>,
    ) -> Result<Self, ScriptClientError> {
        if worker.kind() != WorkerKind::Script {
            return Err(ScriptClientError::WrongWorkerKind);
        }
        let (log_sender, log_receiver) = mpsc::channel(LOG_QUEUE_CAPACITY);
        let dropped_logs = Arc::new(AtomicUsize::new(0));
        let dispatcher = tokio::spawn(dispatch_host_calls(
            worker.connection().clone(),
            host,
            log_sender,
            dropped_logs.clone(),
        ));
        Ok(Self {
            worker,
            logs: Mutex::new(Some(log_receiver)),
            dropped_logs,
            dispatcher,
        })
    }

    /// Initializes the fixed Python runtime for one active profile.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, validation, or Python bootstrap error.
    pub async fn initialize(
        &self,
        request: &ScriptInitializeRequest,
    ) -> Result<ScriptInitializeResult, ScriptClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::INITIALIZE,
            request,
        )
        .await
    }

    /// Reads the child runtime's current execution state.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, or payload error.
    pub async fn status(&self) -> Result<ScriptWorkerStatus, ScriptClientError> {
        self.request(OperationClass::ReadOnly, protocol::STATUS, &())
            .await
    }

    /// Runs one command class on the worker's dedicated user thread.
    ///
    /// The request remains correlated while the IPC dispatcher continues to
    /// service reverse host calls and independent stop requests.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, validation, or execution-boundary error.
    pub async fn execute(
        &self,
        request: &ScriptExecuteRequest,
    ) -> Result<ScriptExecutionResult, ScriptClientError> {
        self.request(OperationClass::MutatingResource, protocol::EXECUTE, request)
            .await
    }

    /// Cooperatively requests that the current command stop.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, or payload error.
    pub async fn stop(&self) -> Result<ScriptStopResult, ScriptClientError> {
        self.request(OperationClass::MutatingResource, protocol::STOP, &())
            .await
    }

    #[must_use]
    pub fn take_logs(&self) -> Option<mpsc::Receiver<LogPayload>> {
        self.logs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    #[must_use]
    pub fn dropped_log_count(&self) -> usize {
        self.dropped_logs.load(Ordering::Acquire)
    }

    async fn request<Request, Response>(
        &self,
        class: OperationClass,
        operation: &'static str,
        request: &Request,
    ) -> Result<Response, ScriptClientError>
    where
        Request: serde::Serialize,
        Response: serde::de::DeserializeOwned,
    {
        let payload = serialize_value(request)?;
        let response = self.worker.request(class, operation, payload).await?;
        Ok(deserialize_value(&response)?)
    }
}

impl Drop for ScriptWorkerClient {
    fn drop(&mut self) {
        self.dispatcher.abort();
    }
}

async fn dispatch_host_calls(
    connection: IpcConnection,
    host: Arc<dyn ScriptHost>,
    logs: mpsc::Sender<LogPayload>,
    dropped_logs: Arc<AtomicUsize>,
) {
    while let Ok(envelope) = connection.recv().await {
        match envelope {
            Envelope::Request { id, op, payload } => {
                respond_to_host_call(&connection, host.as_ref(), id, op, &payload).await;
            }
            Envelope::Log { payload } => {
                if logs.try_send(payload).is_err() {
                    dropped_logs.fetch_add(1, Ordering::AcqRel);
                }
            }
            Envelope::Event { .. } | Envelope::Response { .. } | Envelope::Error { .. } => {}
        }
    }
}

async fn respond_to_host_call(
    connection: &IpcConnection,
    host: &dyn ScriptHost,
    id: u64,
    operation: String,
    payload: &IpcValue,
) {
    match dispatch_host_call(host, &operation, payload) {
        Ok(payload) => {
            let _result = connection.respond(id, Some(operation), payload).await;
        }
        Err(error) => {
            let payload = IpcErrorPayload::new(error.code, error.message).unwrap_or_else(|_| {
                IpcErrorPayload::new(
                    "InternalError",
                    "script host produced an invalid error code",
                )
                .expect("static internal error payload is valid")
            });
            let _result = connection.respond_error(id, operation, payload).await;
        }
    }
}

fn dispatch_host_call(
    host: &dyn ScriptHost,
    operation: &str,
    payload: &IpcValue,
) -> Result<IpcValue, ScriptHostError> {
    match operation {
        protocol::HOST_CONTROLLER_INPUT => {
            let request = decode_host::<HostControllerInputRequest>(payload)?;
            serialize_host(host.controller_input(request))
        }
        protocol::HOST_CONTROLLER_NEUTRAL => {
            decode_host::<()>(payload)?;
            serialize_host(host.controller_neutral())
        }
        protocol::HOST_SERIAL_WRITE => {
            let request = decode_host::<HostSerialWriteRequest>(payload)?;
            serialize_host(host.serial_write(request.data))
        }
        protocol::HOST_SERIAL_WRITE_ROW => {
            let request = decode_host::<HostSerialWriteRowRequest>(payload)?;
            serialize_host(host.serial_write_row(request.row))
        }
        protocol::HOST_SERIAL_RELOAD => {
            decode_host::<()>(payload)?;
            serialize_host(host.serial_reload())
        }
        protocol::HOST_OUTPUT => {
            let request = decode_host::<HostOutputRequest>(payload)?;
            serialize_host(host.output(request))
        }
        _ => Err(ScriptHostError::new(
            "NotFound",
            format!("unknown script host operation `{operation}`"),
        )),
    }
}

fn decode_host<T: serde::de::DeserializeOwned>(payload: &IpcValue) -> Result<T, ScriptHostError> {
    deserialize_value(payload)
        .map_err(|error| ScriptHostError::new("InvalidPayload", error.to_string()))
}

fn serialize_host(result: Result<(), ScriptHostError>) -> Result<IpcValue, ScriptHostError> {
    result?;
    serialize_value(&()).map_err(|error| ScriptHostError::new("IpcEncodeError", error.to_string()))
}
