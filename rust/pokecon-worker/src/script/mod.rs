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
    HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostSerialWriteRequest, HostSerialWriteRowRequest, HostTkRequest, HostTkResult,
    ScriptExecuteRequest, ScriptExecutionResult, ScriptInitializeRequest, ScriptInitializeResult,
    ScriptStopResult, ScriptTkEvent, ScriptWorkerStatus,
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

    /// Opens one Rust-owned script dialog and returns its generation-local ID.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the dialog cannot be created.
    fn dialog_open(
        &self,
        request: HostDialogOpenRequest,
    ) -> Result<HostDialogOpenResult, ScriptHostError>;

    /// Reads one script dialog's authoritative completion state.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the state cannot be read.
    fn dialog_status(
        &self,
        request: HostDialogStatusRequest,
    ) -> Result<HostDialogStatusResult, ScriptHostError>;

    /// Closes every dialog owned by the stopping script generation.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when cleanup cannot be requested.
    fn dialog_close_all(&self) -> Result<(), ScriptHostError>;

    /// Performs one closed socket or MQTT compatibility operation.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the transport operation fails.
    fn network(&self, request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError>;

    /// Sends one fail-soft user-script notification through the Rust service.
    ///
    /// # Errors
    ///
    /// Returns a stable host error for the Python compatibility layer to suppress.
    fn notification(&self, request: HostNotificationRequest) -> Result<(), ScriptHostError>;

    /// Returns the lifetime-fixed shared-frame mapping for this sole reader.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when camera state cannot be initialized.
    fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError>;

    /// Applies or observes one closed camera control operation.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the operation cannot be applied.
    fn camera_control(
        &self,
        request: HostCameraControlRequest,
    ) -> Result<HostCameraState, ScriptHostError>;

    /// Updates Rust-owned overlay or capture-area state.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the control data is invalid.
    fn overlay(&self, request: HostOverlayRequest) -> Result<(), ScriptHostError>;

    /// Publishes one bounded compressed popup image to the UI service.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when the image cannot be accepted.
    fn popup_image(&self, request: HostPopupImageRequest) -> Result<(), ScriptHostError>;

    /// Applies one operation from the fixed Tkinter-to-Web bridge.
    ///
    /// # Errors
    ///
    /// Returns a stable host error when an object or property is invalid.
    fn tk(&self, request: HostTkRequest) -> Result<HostTkResult, ScriptHostError>;
}

/// Parent-side user-script client or dispatcher failure.
#[derive(Debug, thiserror::Error)]
pub enum ScriptClientError {
    #[error("user-script worker client requires a script worker")]
    WrongWorkerKind,
    #[error(transparent)]
    Worker(#[from] WorkerRequestError),
    #[error(transparent)]
    Connection(#[from] crate::ipc::ConnectionError),
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

    /// Queues one UI-originated fixed Tk compatibility callback.
    ///
    /// # Errors
    ///
    /// Returns a payload or transport error when the event cannot be queued.
    pub async fn tk_event(&self, event: &ScriptTkEvent) -> Result<(), ScriptClientError> {
        let payload = serialize_value(event)?;
        self.worker
            .connection()
            .send_event(protocol::TK_EVENT, payload)
            .await?;
        Ok(())
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
        protocol::HOST_DIALOG_OPEN => {
            let request = decode_host::<HostDialogOpenRequest>(payload)?;
            serialize_host_value(host.dialog_open(request))
        }
        protocol::HOST_DIALOG_STATUS => {
            let request = decode_host::<HostDialogStatusRequest>(payload)?;
            serialize_host_value(host.dialog_status(request))
        }
        protocol::HOST_DIALOG_CLOSE_ALL => {
            decode_host::<()>(payload)?;
            serialize_host(host.dialog_close_all())
        }
        protocol::HOST_NETWORK => {
            let request = decode_host::<HostNetworkRequest>(payload)?;
            serialize_host_value(host.network(request))
        }
        protocol::HOST_NOTIFICATION => {
            let request = decode_host::<HostNotificationRequest>(payload)?;
            serialize_host(host.notification(request))
        }
        protocol::HOST_CAMERA_INITIALIZE => {
            decode_host::<()>(payload)?;
            serialize_host_value(host.camera_initialize())
        }
        protocol::HOST_CAMERA_CONTROL => {
            let request = decode_host::<HostCameraControlRequest>(payload)?;
            serialize_host_value(host.camera_control(request))
        }
        protocol::HOST_OVERLAY => {
            let request = decode_host::<HostOverlayRequest>(payload)?;
            serialize_host(host.overlay(request))
        }
        protocol::HOST_POPUP_IMAGE => {
            let request = decode_host::<HostPopupImageRequest>(payload)?;
            if request.encoded.len() > protocol::MAX_POPUP_IMAGE_BYTES {
                return Err(ScriptHostError::new(
                    "PayloadTooLarge",
                    "script popup image exceeds the bounded compressed payload limit",
                ));
            }
            serialize_host(host.popup_image(request))
        }
        protocol::HOST_TK => {
            let request = decode_host::<HostTkRequest>(payload)?;
            serialize_host_value(host.tk(request))
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

fn serialize_host_value<T: serde::Serialize>(
    result: Result<T, ScriptHostError>,
) -> Result<IpcValue, ScriptHostError> {
    let value = result?;
    serialize_value(&value)
        .map_err(|error| ScriptHostError::new("IpcEncodeError", error.to_string()))
}
