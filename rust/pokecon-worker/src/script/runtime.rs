use std::path::{Path, PathBuf};

use tokio::runtime::Handle;

use crate::ipc::{
    ConnectionError, IpcConnection, IpcErrorPayload, IpcValue, ValueCodecError, deserialize_value,
    serialize_value,
};

use super::protocol::{
    self, ScriptExecuteRequest, ScriptInitializeRequest, ScriptInitializeResult, ScriptStopResult,
    ScriptWorkerStatus,
};
use super::python::{PythonActor, PythonActorConfig, PythonActorError};

#[derive(Debug)]
struct DispatchError {
    code: &'static str,
    message: String,
}

impl DispatchError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn invalid_payload(error: &ValueCodecError) -> Self {
        Self::new("InvalidPayload", error.to_string())
    }

    fn actor(error: PythonActorError) -> Self {
        Self::new(error.code, error.message)
    }

    fn payload(self) -> IpcErrorPayload {
        IpcErrorPayload::new(self.code, self.message).expect("static script error code is valid")
    }
}

pub(crate) struct ScriptWorkerRuntime {
    connection: IpcConnection,
    runtime_handle: Handle,
    command_root: Option<PathBuf>,
    actor: Option<PythonActor>,
}

impl ScriptWorkerRuntime {
    pub(crate) fn new(connection: IpcConnection) -> Self {
        Self {
            connection,
            runtime_handle: Handle::current(),
            command_root: None,
            actor: None,
        }
    }

    pub(crate) fn handles(operation: &str) -> bool {
        matches!(
            operation,
            protocol::INITIALIZE | protocol::STATUS | protocol::EXECUTE | protocol::STOP
        )
    }

    pub(crate) async fn handle(
        &mut self,
        id: u64,
        operation: String,
        payload: IpcValue,
    ) -> Result<(), ConnectionError> {
        if operation == protocol::EXECUTE {
            return self.handle_execute(id, operation, &payload).await;
        }
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

    pub(crate) async fn shutdown(&mut self) {
        if let Some(actor) = &mut self.actor {
            actor.shutdown().await;
        }
        self.actor = None;
    }

    async fn dispatch(
        &mut self,
        operation: &str,
        payload: &IpcValue,
    ) -> Result<IpcValue, DispatchError> {
        match operation {
            protocol::INITIALIZE => {
                let request = deserialize_value::<ScriptInitializeRequest>(payload)
                    .map_err(|error| DispatchError::invalid_payload(&error))?;
                serialize_dispatch(&self.initialize(request).await?)
            }
            protocol::STATUS => {
                decode_empty(payload)?;
                serialize_dispatch(&self.status())
            }
            protocol::STOP => {
                decode_empty(payload)?;
                let stop_requested = self.actor.as_ref().is_some_and(PythonActor::request_stop);
                serialize_dispatch(&ScriptStopResult { stop_requested })
            }
            _ => Err(DispatchError::new(
                "NotFound",
                format!("unknown script worker operation `{operation}`"),
            )),
        }
    }

    async fn initialize(
        &mut self,
        request: ScriptInitializeRequest,
    ) -> Result<ScriptInitializeResult, DispatchError> {
        if self.actor.is_some() {
            return Err(DispatchError::new(
                "AlreadyInitialized",
                "script worker is already initialized",
            ));
        }
        validate_profile(&request.profile)?;
        let command_root = canonical_directory(&request.command_root, "command_root")?;
        let data_root = canonical_directory(&request.data_root, "data_root")?;
        let actor = PythonActor::spawn(PythonActorConfig {
            profile: request.profile,
            command_root: command_root.clone(),
            data_root,
            connection: self.connection.clone(),
            runtime_handle: self.runtime_handle.clone(),
        })
        .await
        .map_err(DispatchError::actor)?;
        let status = actor.status();
        self.command_root = Some(command_root);
        self.actor = Some(actor);
        Ok(ScriptInitializeResult { status })
    }

    fn status(&self) -> ScriptWorkerStatus {
        self.actor
            .as_ref()
            .map_or_else(ScriptWorkerStatus::uninitialized, PythonActor::status)
    }

    async fn handle_execute(
        &self,
        id: u64,
        operation: String,
        payload: &IpcValue,
    ) -> Result<(), ConnectionError> {
        let result = self.begin_execute(payload);
        let receiver = match result {
            Ok(receiver) => receiver,
            Err(error) => {
                return self
                    .connection
                    .respond_error(id, operation, error.payload())
                    .await;
            }
        };
        let connection = self.connection.clone();
        tokio::spawn(async move {
            let result = receiver
                .await
                .map_err(|error| DispatchError::new("ScriptThreadStopped", error.to_string()));
            match result.and_then(|result| serialize_dispatch(&result)) {
                Ok(payload) => {
                    let _response = connection.respond(id, Some(operation), payload).await;
                }
                Err(error) => {
                    let _response = connection
                        .respond_error(id, operation, error.payload())
                        .await;
                }
            }
        });
        Ok(())
    }

    fn begin_execute(
        &self,
        payload: &IpcValue,
    ) -> Result<tokio::sync::oneshot::Receiver<super::protocol::ScriptExecutionResult>, DispatchError>
    {
        let request = deserialize_value::<ScriptExecuteRequest>(payload)
            .map_err(|error| DispatchError::invalid_payload(&error))?;
        let actor = self.actor.as_ref().ok_or_else(|| {
            DispatchError::new("NotInitialized", "script worker is not initialized")
        })?;
        let root = self.command_root.as_ref().ok_or_else(|| {
            DispatchError::new("NotInitialized", "script command root is unavailable")
        })?;
        let path = canonical_script_path(root, &request.path)?;
        if request.class_name.is_empty() || request.class_name.contains('\0') {
            return Err(DispatchError::new(
                "InvalidClassName",
                "script class name must be non-empty and contain no NUL",
            ));
        }
        actor
            .execute(path, request.class_name)
            .map(|(_id, receiver)| receiver)
            .map_err(DispatchError::actor)
    }
}

impl Drop for ScriptWorkerRuntime {
    fn drop(&mut self) {
        if let Some(actor) = &self.actor {
            actor.request_stop();
        }
    }
}

fn validate_profile(profile: &str) -> Result<(), DispatchError> {
    if profile.is_empty() || matches!(profile, "." | "..") || profile.contains(['/', '\\', '\0']) {
        return Err(DispatchError::new(
            "InvalidProfile",
            "script profile must be one safe path component",
        ));
    }
    Ok(())
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, DispatchError> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(DispatchError::new(
            "InvalidPath",
            format!("script {label} must be an existing absolute directory"),
        ));
    }
    path.canonicalize().map_err(|error| {
        DispatchError::new(
            "InvalidPath",
            format!("script {label} cannot be resolved: {error}"),
        )
    })
}

fn canonical_script_path(root: &Path, path: &Path) -> Result<PathBuf, DispatchError> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let canonical = candidate.canonicalize().map_err(|error| {
        DispatchError::new(
            "ScriptNotFound",
            format!("script path cannot be resolved: {error}"),
        )
    })?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(DispatchError::new(
            "InvalidScriptPath",
            "script path must resolve to a regular file inside command_root",
        ));
    }
    Ok(canonical)
}

fn decode_empty(payload: &IpcValue) -> Result<(), DispatchError> {
    deserialize_value::<()>(payload).map_err(|error| DispatchError::invalid_payload(&error))
}

fn serialize_dispatch<T: serde::Serialize>(value: &T) -> Result<IpcValue, DispatchError> {
    serialize_value(value).map_err(|error| DispatchError::invalid_payload(&error))
}
