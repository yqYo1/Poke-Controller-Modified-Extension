use std::fs;
use std::path::{Component, Path, PathBuf};

use tokio::runtime::Handle;

use crate::ipc::{
    ConnectionError, IpcConnection, IpcErrorPayload, IpcValue, ValueCodecError, deserialize_value,
    serialize_value,
};

use super::protocol::{
    self, ScriptDiscoveryResult, ScriptExecuteRequest, ScriptInitializeRequest,
    ScriptInitializeResult, ScriptPauseResult, ScriptStopResult, ScriptTkEvent, ScriptWorkerStatus,
};
use super::python::{DiscoverSource, PythonActor, PythonActorConfig, PythonActorError};

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
            protocol::INITIALIZE
                | protocol::STATUS
                | protocol::DISCOVER
                | protocol::EXECUTE
                | protocol::PAUSE
                | protocol::RESUME
                | protocol::STOP
        )
    }

    pub(crate) fn handles_event(operation: &str) -> bool {
        operation == protocol::TK_EVENT
    }

    pub(crate) fn handle_event(
        &self,
        operation: &str,
        payload: &IpcValue,
    ) -> Result<(), IpcErrorPayload> {
        if operation != protocol::TK_EVENT {
            return Err(DispatchError::new(
                "NotFound",
                format!("unknown script event `{operation}`"),
            )
            .payload());
        }
        let event = deserialize_value::<ScriptTkEvent>(payload)
            .map_err(|error| DispatchError::invalid_payload(&error).payload())?;
        let actor = self.actor.as_ref().ok_or_else(|| {
            DispatchError::new("NotInitialized", "script worker is not initialized").payload()
        })?;
        actor
            .tk_event(&event)
            .map_err(|error| DispatchError::actor(error).payload())
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
            protocol::DISCOVER => {
                decode_empty(payload)?;
                serialize_dispatch(&self.discover().await?)
            }
            protocol::PAUSE => {
                decode_empty(payload)?;
                let changed = self.actor.as_ref().is_some_and(PythonActor::pause);
                serialize_dispatch(&ScriptPauseResult { changed })
            }
            protocol::RESUME => {
                decode_empty(payload)?;
                let changed = self.actor.as_ref().is_some_and(PythonActor::resume);
                serialize_dispatch(&ScriptPauseResult { changed })
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

    async fn discover(&self) -> Result<ScriptDiscoveryResult, DispatchError> {
        let actor = self.actor.as_ref().ok_or_else(|| {
            DispatchError::new("NotInitialized", "script worker is not initialized")
        })?;
        let root = self.command_root.as_ref().ok_or_else(|| {
            DispatchError::new("NotInitialized", "script command root is unavailable")
        })?;
        let sources = discover_sources(root)?;
        let commands = actor
            .discover(sources)
            .await
            .map_err(DispatchError::actor)?;
        Ok(ScriptDiscoveryResult { commands })
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
            .execute(path, request.class_name, request.tags)
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

fn discover_sources(root: &Path) -> Result<Vec<DiscoverSource>, DispatchError> {
    let mut sources = Vec::new();
    visit_command_directory(root, root, &mut sources)?;
    Ok(sources)
}

fn visit_command_directory(
    root: &Path,
    directory: &Path,
    sources: &mut Vec<DiscoverSource>,
) -> Result<(), DispatchError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        DispatchError::new(
            "ScriptDiscoveryError",
            format!("command directory cannot be read: {error}"),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            DispatchError::new(
                "ScriptDiscoveryError",
                format!("command directory entry cannot be read: {error}"),
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            DispatchError::new(
                "ScriptDiscoveryError",
                format!("command entry type cannot be read: {error}"),
            )
        })?;
        let path = entry.path();
        if file_type.is_dir() {
            visit_command_directory(root, &path, sources)?;
            continue;
        }
        if !file_type.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("py")
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_none_or(|stem| stem.starts_with('_'))
        {
            continue;
        }
        let canonical = path.canonicalize().map_err(|error| {
            DispatchError::new(
                "ScriptDiscoveryError",
                format!("command script cannot be resolved: {error}"),
            )
        })?;
        if !canonical.starts_with(root) {
            return Err(DispatchError::new(
                "InvalidScriptPath",
                "discovered script escaped the initialized command root",
            ));
        }
        let relative_path = canonical
            .strip_prefix(root)
            .map_err(|_| {
                DispatchError::new(
                    "InvalidScriptPath",
                    "discovered script escaped the initialized command root",
                )
            })?
            .to_path_buf();
        sources.push(discovery_source(canonical, relative_path)?);
    }
    Ok(())
}

fn discovery_source(
    path: PathBuf,
    relative_path: PathBuf,
) -> Result<DiscoverSource, DispatchError> {
    let mut components = relative_path
        .components()
        .map(|component| match component {
            Component::Normal(component) => {
                component.to_str().map(str::to_owned).ok_or_else(|| {
                    DispatchError::new(
                        "ScriptDiscoveryError",
                        "command path component is not valid Unicode",
                    )
                })
            }
            _ => Err(DispatchError::new(
                "InvalidScriptPath",
                "command path is not a normalized relative path",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let file = components.pop().ok_or_else(|| {
        DispatchError::new("InvalidScriptPath", "command path has no file component")
    })?;
    let stem = Path::new(&file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| DispatchError::new("ScriptDiscoveryError", "command filename is invalid"))?;
    let automatic_tags = components
        .iter()
        .filter(|component| !matches!(component.as_str(), "Commands" | "PythonCommands"))
        .map(|component| format!("@{component}"))
        .collect();
    let mut module = vec!["Commands".to_owned(), "PythonCommands".to_owned()];
    module.extend(
        components
            .into_iter()
            .filter(|component| !matches!(component.as_str(), "Commands" | "PythonCommands")),
    );
    module.push(stem.to_owned());
    Ok(DiscoverSource {
        path,
        relative_path,
        module_path: module.join("."),
        automatic_tags,
    })
}

fn decode_empty(payload: &IpcValue) -> Result<(), DispatchError> {
    deserialize_value::<()>(payload).map_err(|error| DispatchError::invalid_payload(&error))
}

fn serialize_dispatch<T: serde::Serialize>(value: &T) -> Result<IpcValue, DispatchError> {
    serialize_value(value).map_err(|error| DispatchError::invalid_payload(&error))
}
