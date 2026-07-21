//! Bidirectional dynamic-worker protocol. Language runtimes stay in the child
//! process while every settings, state, profile, and controller operation is
//! dispatched back to the Rust-main-owned host.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pokecon_dynamic::protocol::{
    self, DynamicCommandCacheRequest, DynamicCommandCacheResult, DynamicDiagnostic,
    DynamicEmitRequest, DynamicEmitResult, DynamicInitializeRequest, DynamicInitializeResult,
    DynamicTagMatchRequest, DynamicWorkerStatus, HostControllerUpdate, HostProfileSwitchRequest,
    HostSetStateValueRequest, HostSettings, HostSettingsChanges, HostState,
};
use pokecon_dynamic::{
    CommandDisplayItem, CommandInfo, Diagnostic, DynamicConfigControl, DynamicEngine,
    DynamicEngineError, DynamicHost, DynamicHostError,
};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::WorkerKind;
use crate::generation::OperationClass;
use crate::ipc::{
    ConnectionError, Envelope, IpcConnection, IpcErrorPayload, IpcValue, LogLevel, LogPayload,
    LogTarget, ValueCodecError, deserialize_value, serialize_value,
};
use crate::supervisor::{ManagedWorker, WorkerRequestError};

const LOG_QUEUE_CAPACITY: usize = 128;

#[derive(Debug)]
struct DispatchError {
    code: String,
    message: String,
}

impl DispatchError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn invalid_payload(error: &ValueCodecError) -> Self {
        Self::new("InvalidPayload", error.to_string())
    }

    fn engine(error: DynamicEngineError) -> Self {
        match error {
            DynamicEngineError::Host(error) => Self::new(error.code, error.message),
            DynamicEngineError::Source(error) => Self::new("DynamicSourceError", error.to_string()),
            DynamicEngineError::Transaction(error) => {
                Self::new("DynamicTransactionError", error.to_string())
            }
            DynamicEngineError::Event(error) => Self::new("DynamicEventError", error.to_string()),
            DynamicEngineError::Command(error) => {
                Self::new("DynamicCommandError", error.to_string())
            }
            DynamicEngineError::Python(error) => Self::new("PythonError", error),
            DynamicEngineError::Lua(error) => Self::new("LuaError", error),
            DynamicEngineError::Evaluation(error) => Self::new("DynamicEvaluationError", error),
            DynamicEngineError::NoActiveEvaluation => Self::new(
                "NoActiveEvaluation",
                "dynamic API requires an active evaluation",
            ),
            DynamicEngineError::NoTokioRuntime => Self::new(
                "NoTokioRuntime",
                "dynamic engine requires an active Tokio runtime",
            ),
        }
    }

    fn join(error: &tokio::task::JoinError) -> Self {
        Self::new("DynamicTaskFailed", error.to_string())
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

/// Child-side persistent engine and Rust-main host proxy.
pub(crate) struct DynamicWorkerRuntime {
    connection: IpcConnection,
    runtime_handle: Handle,
    engine: Option<DynamicEngine>,
}

impl DynamicWorkerRuntime {
    pub(crate) fn new(connection: IpcConnection) -> Self {
        Self {
            connection,
            runtime_handle: Handle::current(),
            engine: None,
        }
    }

    pub(crate) fn handles(operation: &str) -> bool {
        matches!(
            operation,
            protocol::INITIALIZE
                | protocol::STATUS
                | protocol::CONTROL
                | protocol::EMIT
                | protocol::SORT_COMMANDS
                | protocol::TAG_MATCHES
                | protocol::BUILD_COMMAND_CACHE
        )
    }

    pub(crate) async fn handle(
        &mut self,
        id: u64,
        operation: String,
        payload: IpcValue,
    ) -> Result<(), ConnectionError> {
        let result = self.dispatch(&operation, &payload).await;
        match result {
            Ok(payload) => self.connection.respond(id, Some(operation), payload).await,
            Err(error) => {
                self.connection
                    .respond_error(id, operation, error.payload())
                    .await
            }
        }
    }

    async fn dispatch(
        &mut self,
        operation: &str,
        payload: &IpcValue,
    ) -> Result<IpcValue, DispatchError> {
        match operation {
            protocol::INITIALIZE => {
                let request = deserialize_value::<DynamicInitializeRequest>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                serialize_dispatch(&self.initialize(request).await?)
            }
            protocol::STATUS => {
                deserialize_value::<()>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                serialize_dispatch(&self.status())
            }
            protocol::CONTROL => {
                let operation = deserialize_value::<DynamicConfigControl>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                let result = self
                    .run_engine(move |engine, handle| handle.block_on(engine.control(operation)))
                    .await?;
                serialize_dispatch(&result)
            }
            protocol::EMIT => {
                let request = deserialize_value::<DynamicEmitRequest>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                let result = self
                    .run_engine(move |engine, handle| handle.block_on(engine.emit(&request.event)))
                    .await?;
                serialize_dispatch(&DynamicEmitResult {
                    cancelled: result.cancelled,
                })
            }
            protocol::SORT_COMMANDS => {
                let candidates = deserialize_value::<Vec<CommandInfo>>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                let result = self
                    .run_engine(move |engine, handle| {
                        handle.block_on(engine.sort_commands(candidates))
                    })
                    .await?;
                serialize_dispatch(&result)
            }
            protocol::TAG_MATCHES => {
                let request = deserialize_value::<DynamicTagMatchRequest>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                let result = self
                    .run_engine(move |engine, handle| {
                        handle.block_on(engine.tag_matches(&request.selected_tag, &request.command))
                    })
                    .await?;
                serialize_dispatch(&result)
            }
            protocol::BUILD_COMMAND_CACHE => {
                let request = deserialize_value::<DynamicCommandCacheRequest>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                let result = self
                    .run_engine(move |engine, handle| {
                        handle.block_on(
                            engine.build_command_cache(request.generation, request.candidates),
                        )
                    })
                    .await?;
                serialize_dispatch(&result)
            }
            _ => Err(DispatchError::new(
                "NotFound",
                format!("unknown dynamic worker operation `{operation}`"),
            )),
        }
    }

    async fn initialize(
        &mut self,
        request: DynamicInitializeRequest,
    ) -> Result<DynamicInitializeResult, DispatchError> {
        if self.engine.is_some() {
            return Err(DispatchError::new(
                "AlreadyInitialized",
                "dynamic worker is already initialized",
            ));
        }
        let connection = self.connection.clone();
        let runtime_handle = self.runtime_handle.clone();
        let task_handle = runtime_handle.clone();
        let (engine, startup_load) = tokio::task::spawn_blocking(move || {
            let host: Arc<dyn DynamicHost> = Arc::new(IpcDynamicHost {
                connection,
                runtime_handle: task_handle.clone(),
            });
            let startup_path = request
                .config_root
                .join(format!("init.{}", request.primary.extension()));
            let engine = {
                let _runtime = task_handle.enter();
                DynamicEngine::new(
                    &request.config_root,
                    request.home,
                    Some(request.primary),
                    host,
                )?
            };
            let startup_load = if startup_path.is_file() {
                Some(
                    task_handle.block_on(engine.control(DynamicConfigControl::LoadPath {
                        path: startup_path.to_string_lossy().into_owned(),
                    }))?,
                )
            } else {
                None
            };
            Ok::<_, DynamicEngineError>((engine, startup_load))
        })
        .await
        .map_err(|error| DispatchError::join(&error))?
        .map_err(DispatchError::engine)?;
        self.engine = Some(engine);
        Ok(DynamicInitializeResult {
            status: self.status(),
            startup_load,
        })
    }

    fn status(&self) -> DynamicWorkerStatus {
        self.engine
            .as_ref()
            .map_or_else(DynamicWorkerStatus::uninitialized, |engine| {
                DynamicWorkerStatus {
                    initialized: true,
                    generation: engine.generation(),
                    initialized_languages: engine.initialized_languages(),
                }
            })
    }

    async fn run_engine<T, F>(&self, operation: F) -> Result<T, DispatchError>
    where
        T: Send + 'static,
        F: FnOnce(DynamicEngine, Handle) -> Result<T, DynamicEngineError> + Send + 'static,
    {
        let engine = self.engine.clone().ok_or_else(|| {
            DispatchError::new("NotInitialized", "dynamic worker is not initialized")
        })?;
        let task_handle = self.runtime_handle.clone();
        tokio::task::spawn_blocking(move || operation(engine, task_handle))
            .await
            .map_err(|error| DispatchError::join(&error))?
            .map_err(DispatchError::engine)
    }
}

fn serialize_dispatch<T: serde::Serialize>(value: &T) -> Result<IpcValue, DispatchError> {
    serialize_value(value).map_err(|error| DispatchError::invalid_payload(&error))
}

struct IpcDynamicHost {
    connection: IpcConnection,
    runtime_handle: Handle,
}

impl IpcDynamicHost {
    fn wait<Output>(&self, future: impl Future<Output = Output>) -> Output {
        if Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| self.runtime_handle.block_on(future))
        } else {
            self.runtime_handle.block_on(future)
        }
    }

    fn request<Request, Response>(
        &self,
        operation: &'static str,
        request: &Request,
    ) -> Result<Response, DynamicHostError>
    where
        Request: serde::Serialize,
        Response: serde::de::DeserializeOwned,
    {
        let payload = serialize_value(request)
            .map_err(|error| DynamicHostError::new("IpcEncodeError", error.to_string()))?;
        let response = self
            .wait(self.connection.request(operation, payload))
            .map_err(connection_host_error)?;
        deserialize_value(&response)
            .map_err(|error| DynamicHostError::new("IpcDecodeError", error.to_string()))
    }

    fn event<T: serde::Serialize>(&self, operation: &'static str, value: &T) {
        let Ok(payload) = serialize_value(value) else {
            return;
        };
        let _result = self.wait(self.connection.send_event(operation, payload));
    }
}

fn connection_host_error(error: ConnectionError) -> DynamicHostError {
    match error {
        ConnectionError::Remote { code, message } => DynamicHostError::new(code, message),
        error => DynamicHostError::new("IpcDisconnected", error.to_string()),
    }
}

impl DynamicHost for IpcDynamicHost {
    fn settings_snapshot(&self) -> Result<HostSettings, DynamicHostError> {
        self.request(protocol::HOST_SETTINGS_SNAPSHOT, &())
    }

    fn apply_settings(
        &self,
        changes: &HostSettingsChanges,
    ) -> Result<HostSettings, DynamicHostError> {
        self.request(protocol::HOST_APPLY_SETTINGS, changes)
    }

    fn state_snapshot(&self) -> Result<HostState, DynamicHostError> {
        self.request(protocol::HOST_STATE_SNAPSHOT, &())
    }

    fn set_state_value(
        &self,
        name: &str,
        value: serde_json::Value,
    ) -> Result<(), DynamicHostError> {
        self.request(
            protocol::HOST_SET_STATE_VALUE,
            &HostSetStateValueRequest {
                name: name.to_owned(),
                value,
            },
        )
    }

    fn profile_current(&self) -> Result<String, DynamicHostError> {
        self.request(protocol::HOST_PROFILE_CURRENT, &())
    }

    fn profile_list(&self) -> Result<Vec<String>, DynamicHostError> {
        self.request(protocol::HOST_PROFILE_LIST, &())
    }

    fn profile_switch(&self, name: &str) -> Result<bool, DynamicHostError> {
        self.request(
            protocol::HOST_PROFILE_SWITCH,
            &HostProfileSwitchRequest {
                name: name.to_owned(),
            },
        )
    }

    fn controller_update(&self, update: HostControllerUpdate) -> Result<(), DynamicHostError> {
        self.request(protocol::HOST_CONTROLLER_UPDATE, &update)
    }

    fn controller_reset(&self) -> Result<(), DynamicHostError> {
        self.request(protocol::HOST_CONTROLLER_RESET, &())
    }

    fn record_diagnostic(&self, diagnostic: Diagnostic) {
        self.event(protocol::DIAGNOSTIC_EVENT, &diagnostic);
    }

    fn record_output(&self, message: &str) {
        let _result = self.wait(self.connection.send_log(LogPayload {
            level: LogLevel::Info,
            message: message.to_owned(),
            target: LogTarget::Stdout,
        }));
    }

    fn request_command_recompute(&self) {
        self.event(protocol::COMMAND_RECOMPUTE_EVENT, &());
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
    ) -> Result<pokecon_dynamic::DynamicLoadResult, DynamicClientError> {
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
    let response = dispatch_host_call(host, &operation, payload);
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

fn dispatch_host_call(
    host: &dyn DynamicHost,
    operation: &str,
    payload: &IpcValue,
) -> Result<IpcValue, DispatchError> {
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
        protocol::HOST_PROFILE_CURRENT => {
            decode_empty(payload)?;
            serialize_host(host.profile_current())
        }
        protocol::HOST_PROFILE_LIST => {
            decode_empty(payload)?;
            serialize_host(host.profile_list())
        }
        protocol::HOST_PROFILE_SWITCH => {
            let request = decode_host::<HostProfileSwitchRequest>(payload)?;
            serialize_host(host.profile_switch(&request.name))
        }
        protocol::HOST_CONTROLLER_UPDATE => {
            let update = decode_host::<HostControllerUpdate>(payload)?;
            serialize_host(host.controller_update(update))
        }
        protocol::HOST_CONTROLLER_RESET => {
            decode_empty(payload)?;
            serialize_host(host.controller_reset())
        }
        _ => Err(DispatchError::new(
            "NotFound",
            format!("unknown dynamic host operation `{operation}`"),
        )),
    }
}

fn decode_empty(payload: &IpcValue) -> Result<(), DispatchError> {
    decode_host::<()>(payload)
}

fn decode_host<T: serde::de::DeserializeOwned>(payload: &IpcValue) -> Result<T, DispatchError> {
    deserialize_value(payload).map_err(|error| DispatchError::invalid_payload(&error))
}

fn serialize_host<T: serde::Serialize>(
    result: Result<T, DynamicHostError>,
) -> Result<IpcValue, DispatchError> {
    result
        .map_err(|error| DispatchError::new(error.code, error.message))
        .and_then(|value| serialize_dispatch(&value))
}
