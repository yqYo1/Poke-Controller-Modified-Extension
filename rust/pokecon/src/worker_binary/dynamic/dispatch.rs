//! Child-side persistent dynamic engine and Rust-main host proxy.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::runtime::Handle;

use crate::dynamic::protocol::{
    self, DynamicCommandCacheRequest, DynamicEmitRequest, DynamicEmitResult,
    DynamicInitializeRequest, DynamicInitializeResult, DynamicProfileSwitchRequest,
    DynamicTagMatchRequest, DynamicWorkerStatus, HostControllerUpdate, HostMergeStateValueRequest,
    HostProfileSwitchBeginRequest, HostProfileSwitchBeginResult, HostProfileSwitchCommitResult,
    HostSetStateValueRequest, HostSettings, HostSettingsChanges, HostState,
};
use crate::dynamic::{
    CommandInfo, Diagnostic, DynamicConfigControl, DynamicEngine, DynamicEngineError, DynamicHost,
    DynamicHostError,
};
use crate::worker::ipc::{
    ConnectionError, IpcConnection, IpcErrorPayload, IpcValue, LogLevel, LogPayload, LogTarget,
    ValueCodecError, deserialize_value, serialize_value,
};

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
                | protocol::SWITCH_PROFILE
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
            protocol::SWITCH_PROFILE => {
                let request = deserialize_value::<DynamicProfileSwitchRequest>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                let result = self
                    .run_engine(move |engine, handle| {
                        handle.block_on(engine.switch_profile(&request.name))
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

#[async_trait]
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

    fn merge_state_value(
        &self,
        name: &str,
        before: serde_json::Value,
        value: serde_json::Value,
    ) -> Result<(), DynamicHostError> {
        self.request(
            protocol::HOST_MERGE_STATE_VALUE,
            &HostMergeStateValueRequest {
                name: name.to_owned(),
                before,
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

    async fn profile_switch_begin(
        &self,
        name: &str,
        changes: &HostSettingsChanges,
    ) -> Result<HostProfileSwitchBeginResult, DynamicHostError> {
        self.request(
            protocol::HOST_PROFILE_SWITCH_BEGIN,
            &HostProfileSwitchBeginRequest {
                name: name.to_owned(),
                changes: changes.clone(),
            },
        )
    }

    async fn profile_switch_commit(
        &self,
    ) -> Result<HostProfileSwitchCommitResult, DynamicHostError> {
        self.request(protocol::HOST_PROFILE_SWITCH_COMMIT, &())
    }

    async fn profile_switch_abort(&self) -> Result<(), DynamicHostError> {
        self.request(protocol::HOST_PROFILE_SWITCH_ABORT, &())
    }

    async fn profile_switch_end(&self) -> Result<(), DynamicHostError> {
        self.request(protocol::HOST_PROFILE_SWITCH_END, &())
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
