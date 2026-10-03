//! Parent-side client for the bidirectional dynamic-worker protocol.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::dynamic_domain::protocol::{
    self, DynamicCommandCacheRequest, DynamicCommandCacheResult, DynamicDiagnostic,
    DynamicEmitRequest, DynamicEmitResult, DynamicInitializeRequest, DynamicInitializeResult,
    DynamicProfileSwitchRequest, DynamicProfileSwitchResult, DynamicTagMatchRequest,
    DynamicWorkerStatus, HostControllerUpdate, HostMergeStateValueRequest,
    HostProfileSwitchBeginRequest, HostSetStateValueRequest, HostSettingsChanges,
};
use crate::dynamic_domain::{
    CommandDisplayItem, CommandInfo, DynamicConfigControl, DynamicHost, DynamicHostError,
    DynamicLoadResult,
};
use crate::worker::WorkerKind;
use crate::worker::generation::OperationClass;
use crate::worker::ipc::{
    Envelope, IpcConnection, IpcErrorPayload, IpcValue, LogPayload, ValueCodecError,
    deserialize_value, serialize_value,
};
use crate::worker::supervisor::{ManagedWorker, WorkerRequestError};

const LOG_QUEUE_CAPACITY: usize = 128;

#[derive(Debug)]
struct HostDispatchError {
    code: String,
    message: String,
}

impl HostDispatchError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn invalid_payload(error: &ValueCodecError) -> Self {
        Self::new("InvalidPayload", error.to_string())
    }

    fn payload(self) -> IpcErrorPayload {
        IpcErrorPayload::new(self.code, self.message).unwrap_or_else(|_| {
            IpcErrorPayload::new(
                "InternalError",
                "dynamic worker produced an invalid error code",
            )
            .expect("static internal error payload is valid")
        })
    }
}

/// Parent-side dynamic process client or dispatcher failure.
#[derive(Debug, thiserror::Error)]
pub enum DynamicClientError {
    #[error("dynamic worker client requires a dynamic worker")]
    WrongWorkerKind,
    #[error(transparent)]
    Worker(#[from] WorkerRequestError),
    #[error(transparent)]
    Payload(#[from] ValueCodecError),
}

/// Rust-main handle for one persistent dynamic worker generation.
pub struct DynamicWorkerClient {
    worker: Arc<ManagedWorker>,
    logs: Mutex<Option<mpsc::Receiver<LogPayload>>>,
    dropped_logs: Arc<AtomicUsize>,
    dispatcher: JoinHandle<()>,
}

impl std::fmt::Debug for DynamicWorkerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DynamicWorkerClient")
            .field("generation", &self.worker.generation().id())
            .field("dropped_logs", &self.dropped_log_count())
            .finish_non_exhaustive()
    }
}

impl DynamicWorkerClient {
    /// Attaches the sole inbound dispatcher before any request can cause a
    /// reverse host call.
    ///
    /// # Errors
    ///
    /// Rejects a user-script worker handle.
    pub fn attach(
        worker: Arc<ManagedWorker>,
        host: Arc<dyn DynamicHost>,
    ) -> Result<Self, DynamicClientError> {
        if worker.kind() != WorkerKind::Dynamic {
            return Err(DynamicClientError::WrongWorkerKind);
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

    /// Initializes both the dynamic engine and its configured primary runtime.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn initialize(
        &self,
        request: &DynamicInitializeRequest,
    ) -> Result<DynamicInitializeResult, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::INITIALIZE,
            request,
        )
        .await
    }

    /// Reads the child engine's initialization and language state.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn status(&self) -> Result<DynamicWorkerStatus, DynamicClientError> {
        self.request(OperationClass::ReadOnly, protocol::STATUS, &())
            .await
    }

    /// Loads, sources, or reloads one dynamic configuration transaction.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn control(
        &self,
        operation: &DynamicConfigControl,
    ) -> Result<DynamicLoadResult, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::CONTROL,
            operation,
        )
        .await
    }

    /// Emits one canonical dynamic event.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn emit(&self, event: &str) -> Result<DynamicEmitResult, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::EMIT,
            &DynamicEmitRequest {
                event: event.to_owned(),
            },
        )
        .await
    }

    /// Applies the active dynamic command sorter.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn sort_commands(
        &self,
        candidates: &[CommandInfo],
    ) -> Result<Vec<CommandDisplayItem>, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::SORT_COMMANDS,
            &candidates,
        )
        .await
    }

    /// Applies the active dynamic or built-in tag matcher.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn tag_matches(
        &self,
        selected_tag: &str,
        command: &CommandInfo,
    ) -> Result<bool, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::TAG_MATCHES,
            &DynamicTagMatchRequest {
                selected_tag: selected_tag.to_owned(),
                command: command.clone(),
            },
        )
        .await
    }

    /// Builds all finite tag lists sequentially and returns only a complete or
    /// explicitly superseded generation.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn build_command_cache(
        &self,
        generation: u64,
        candidates: &[CommandInfo],
    ) -> Result<DynamicCommandCacheResult, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::BUILD_COMMAND_CACHE,
            &DynamicCommandCacheRequest {
                generation,
                candidates: candidates.to_vec(),
            },
        )
        .await
    }

    /// Runs the complete dynamic Pre/Post profile transaction in the child
    /// engine while Rust-main owns validation, worker reaping, and commit.
    ///
    /// # Errors
    ///
    /// Returns a generation, transport, remote-engine, or payload failure.
    pub async fn switch_profile(
        &self,
        name: &str,
    ) -> Result<DynamicProfileSwitchResult, DynamicClientError> {
        self.request(
            OperationClass::MutatingResource,
            protocol::SWITCH_PROFILE,
            &DynamicProfileSwitchRequest {
                name: name.to_owned(),
            },
        )
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
    ) -> Result<Response, DynamicClientError>
    where
        Request: serde::Serialize,
        Response: serde::de::DeserializeOwned,
    {
        let payload = serialize_value(request)?;
        let response = self.worker.request(class, operation, payload).await?;
        Ok(deserialize_value(&response)?)
    }
}

impl Drop for DynamicWorkerClient {
    fn drop(&mut self) {
        self.dispatcher.abort();
    }
}

async fn dispatch_host_calls(
    connection: IpcConnection,
    host: Arc<dyn DynamicHost>,
    logs: mpsc::Sender<LogPayload>,
    dropped_logs: Arc<AtomicUsize>,
) {
    while let Ok(envelope) = connection.recv().await {
        match envelope {
            Envelope::Request { id, op, payload } => {
                respond_to_host_call(&connection, host.as_ref(), id, op, &payload).await;
            }
            Envelope::Event { op, payload } if op == protocol::DIAGNOSTIC_EVENT => {
                if let Ok(diagnostic) = deserialize_value::<DynamicDiagnostic>(&payload) {
                    host.record_diagnostic(diagnostic);
                }
            }
            Envelope::Event { op, .. } if op == protocol::COMMAND_RECOMPUTE_EVENT => {
                host.request_command_recompute();
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
    host: &dyn DynamicHost,
    id: u64,
    operation: String,
    payload: &IpcValue,
) {
    let response = dispatch_host_call(host, &operation, payload).await;
    match response {
        Ok(payload) => {
            let _result = connection.respond(id, Some(operation), payload).await;
        }
        Err(error) => {
            let _result = connection
                .respond_error(id, operation, error.payload())
                .await;
        }
    }
}

async fn dispatch_host_call(
    host: &dyn DynamicHost,
    operation: &str,
    payload: &IpcValue,
) -> Result<IpcValue, HostDispatchError> {
    match operation {
        protocol::HOST_SETTINGS_SNAPSHOT => {
            decode_empty(payload)?;
            serialize_host(host.settings_snapshot())
        }
        protocol::HOST_APPLY_SETTINGS => {
            let changes = decode_host::<HostSettingsChanges>(payload)?;
            serialize_host(host.apply_settings(&changes))
        }
        protocol::HOST_STATE_SNAPSHOT => {
            decode_empty(payload)?;
            serialize_host(host.state_snapshot())
        }
        protocol::HOST_SET_STATE_VALUE => {
            let request = decode_host::<HostSetStateValueRequest>(payload)?;
            serialize_host(host.set_state_value(&request.name, request.value))
        }
        protocol::HOST_MERGE_STATE_VALUE => {
            let request = decode_host::<HostMergeStateValueRequest>(payload)?;
            serialize_host(host.merge_state_value(&request.name, request.before, request.value))
        }
        protocol::HOST_PROFILE_CURRENT => {
            decode_empty(payload)?;
            serialize_host(host.profile_current())
        }
        protocol::HOST_PROFILE_LIST => {
            decode_empty(payload)?;
            serialize_host(host.profile_list())
        }
        protocol::HOST_PROFILE_SWITCH_BEGIN => {
            let request = decode_host::<HostProfileSwitchBeginRequest>(payload)?;
            serialize_host(
                host.profile_switch_begin(&request.name, &request.changes)
                    .await,
            )
        }
        protocol::HOST_PROFILE_SWITCH_COMMIT => {
            decode_empty(payload)?;
            serialize_host(host.profile_switch_commit().await)
        }
        protocol::HOST_PROFILE_SWITCH_ABORT => {
            decode_empty(payload)?;
            serialize_host(host.profile_switch_abort().await)
        }
        protocol::HOST_PROFILE_SWITCH_END => {
            decode_empty(payload)?;
            serialize_host(host.profile_switch_end().await)
        }
        protocol::HOST_CONTROLLER_UPDATE => {
            let update = decode_host::<HostControllerUpdate>(payload)?;
            serialize_host(host.controller_update(update))
        }
        protocol::HOST_CONTROLLER_RESET => {
            decode_empty(payload)?;
            serialize_host(host.controller_reset())
        }
        _ => Err(HostDispatchError::new(
            "NotFound",
            format!("unknown dynamic host operation `{operation}`"),
        )),
    }
}

fn decode_empty(payload: &IpcValue) -> Result<(), HostDispatchError> {
    decode_host::<()>(payload)
}

fn decode_host<T: serde::de::DeserializeOwned>(payload: &IpcValue) -> Result<T, HostDispatchError> {
    deserialize_value(payload).map_err(|error| HostDispatchError::invalid_payload(&error))
}

fn serialize_host<T: serde::Serialize>(
    result: Result<T, DynamicHostError>,
) -> Result<IpcValue, HostDispatchError> {
    result
        .map_err(|error| HostDispatchError::new(error.code, error.message))
        .and_then(|value| serialize_dispatch(&value))
}
fn serialize_dispatch<T: serde::Serialize>(value: &T) -> Result<IpcValue, HostDispatchError> {
    serialize_value(value).map_err(|error| HostDispatchError::invalid_payload(&error))
}
