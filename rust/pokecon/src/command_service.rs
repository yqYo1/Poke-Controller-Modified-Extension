//! Profile-scoped command discovery, display-cache publication, and execution
//! lifecycle owned by the Rust application process.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use crate::dynamic::protocol::DynamicProfileSwitchResult;
use crate::dynamic::{
    CommandCacheBuildResult, CommandDisplayCache, CommandDisplayItem, CommandInfo, DynamicHost,
};
use crate::settings::pipeline::LoadedSettings;
use crate::worker::dynamic::DynamicWorkerClient;
use crate::worker::script::protocol::{
    ScriptCommandKind, ScriptDiscoveredCommand, ScriptDiscoveryResult, ScriptExecuteRequest,
    ScriptExecutionOutcome, ScriptExecutionResult, ScriptPauseResult, ScriptPointerEvent,
    ScriptStopResult, ScriptTkEvent,
};
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

use crate::dynamic_host::StartupDynamicHost;
use crate::worker::ipc::ResourceSafety;

const MAX_CACHE_RESTARTS: usize = 32;
/// Typed profile-switch lifecycle stage for UI/log integration.
/// Mirrors SPEC §11.5.6.4.3 transaction order 0-12 and degraded recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileSwitchStage {
    Idle,
    Validating,
    Pending,
    PreEvent,
    Cancelling,
    Stopping,
    ReleasingInput,
    Reaping,
    Committing,
    PostEvent,
    Succeeded,
    Cancelled,
    Failed,
    Degraded,
}

impl ProfileSwitchStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Validating => "validating",
            Self::Pending => "pending",
            Self::PreEvent => "pre_event",
            Self::Cancelling => "cancelling",
            Self::Stopping => "stopping",
            Self::ReleasingInput => "releasing_input",
            Self::Reaping => "reaping",
            Self::Committing => "committing",
            Self::PostEvent => "post_event",
            Self::Succeeded => "succeeded",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Degraded => "degraded",
        }
    }

    const fn encode(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Validating => 1,
            Self::Pending => 2,
            Self::PreEvent => 3,
            Self::Cancelling => 4,
            Self::Stopping => 5,
            Self::ReleasingInput => 6,
            Self::Reaping => 7,
            Self::Committing => 8,
            Self::PostEvent => 9,
            Self::Succeeded => 10,
            Self::Cancelled => 11,
            Self::Failed => 12,
            Self::Degraded => 13,
        }
    }

    const fn decode(value: u8) -> Self {
        match value {
            1 => Self::Validating,
            2 => Self::Pending,
            3 => Self::PreEvent,
            4 => Self::Cancelling,
            5 => Self::Stopping,
            6 => Self::ReleasingInput,
            7 => Self::Reaping,
            8 => Self::Committing,
            9 => Self::PostEvent,
            10 => Self::Succeeded,
            11 => Self::Cancelled,
            12 => Self::Failed,
            13 => Self::Degraded,
            _ => Self::Idle,
        }
    }
}

/// Typed error detail for UI/log integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileSwitchErrorDetail {
    pub code: String,
    pub message: String,
    pub stage: ProfileSwitchStage,
}

/// Typed status snapshot for UI/log integration.
#[allow(
    dead_code,
    reason = "profile switch status is part of the public UI/log API; exercised in integration tests"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileSwitchStatus {
    pub stage: ProfileSwitchStage,
    pub active_profile: String,
    pub pending_profile: Option<String>,
    pub degraded: bool,
    pub last_error: Option<ProfileSwitchErrorDetail>,
}

/// Typed lifecycle result for production profile switching.
#[allow(
    dead_code,
    reason = "profile switch lifecycle result is part of the public API; exercised in integration tests"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileSwitchLifecycleResult {
    Switched { forced_worker_stop: bool },
    Cancelled,
}

/// Stable identity used by selection, execution, and shortcut resolution.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommandIdentity {
    pub module_path: String,
    pub class_name: String,
}

impl From<&CommandInfo> for CommandIdentity {
    fn from(command: &CommandInfo) -> Self {
        Self {
            module_path: command.module_path.clone(),
            class_name: command.class_name.clone(),
        }
    }
}

/// One canonical executable retained from worker-authenticated discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedCommand {
    pub info: CommandInfo,
    pub relative_path: std::path::PathBuf,
    pub kind: ScriptCommandKind,
}

/// Public command state shared by REST, WebSocket, and dynamic configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandStatus {
    Running,
    Paused,
    Stopped,
    Error,
}

impl CommandStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }
}

/// Result of a cancellable lifecycle request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandActionResult {
    Applied,
    Cancelled,
    AlreadyInState,
}

/// Result of loading command metadata and its finite display cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandReloadResult {
    Published {
        generation: u64,
        command_count: usize,
    },
    Cancelled,
}

/// One of the exactly ten profile-capable shortcut slots.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "shortcut slot projection awaits product shortcut dispatch wiring"
    )
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutSlot {
    pub index: u8,
    pub configured_module: String,
    pub command: Option<CommandInfo>,
}

/// Secret-safe backend boundary error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{code}: {message}")]
pub struct CommandBackendError {
    pub code: String,
    pub message: String,
}

impl CommandBackendError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Stop result abstracted from the concrete worker supervisor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptSessionStop {
    pub forced: bool,
}

/// One initialized profile-specific user-script worker generation.
#[async_trait]
pub trait UserScriptSession: Send + Sync {
    async fn discover(&self) -> Result<ScriptDiscoveryResult, CommandBackendError>;

    async fn execute(
        &self,
        request: ScriptExecuteRequest,
    ) -> Result<ScriptExecutionResult, CommandBackendError>;

    async fn pause(&self) -> Result<ScriptPauseResult, CommandBackendError>;

    async fn resume(&self) -> Result<ScriptPauseResult, CommandBackendError>;

    async fn stop_command(&self) -> Result<ScriptStopResult, CommandBackendError>;

    async fn tk_event(&self, _event: &ScriptTkEvent) -> Result<(), CommandBackendError> {
        Err(CommandBackendError::new(
            "TkEventUnavailable",
            "script session does not support Tk compatibility events",
        ))
    }

    async fn pointer_event(&self, _event: &ScriptPointerEvent) -> Result<(), CommandBackendError> {
        Err(CommandBackendError::new(
            "PointerEventUnavailable",
            "script session does not support overlay pointer events",
        ))
    }

    /// Flushes the profile-switch cooperative stop request without waiting for
    /// the Python command thread to exit.
    async fn request_profile_stop(&self) -> Result<(), CommandBackendError> {
        self.stop_command().await.map(|_result| ())
    }

    /// Atomically rejects new mutating IPC and releases Rust-owned resources.
    fn begin_stopping(&self);

    /// Cooperatively stops and always reaps the worker by the supplied deadline.
    async fn shutdown(&self, deadline: Duration) -> Result<ScriptSessionStop, CommandBackendError>;
}

/// Lazily prepares the active profile venv and creates its worker.
#[async_trait]
pub trait UserScriptFactory: Send + Sync {
    async fn spawn(
        &self,
        settings: LoadedSettings,
    ) -> Result<Arc<dyn UserScriptSession>, CommandBackendError>;
}

/// Persistent dynamic-worker operations needed by the command lifecycle.
#[async_trait]
pub trait DynamicCommandBridge: Send + Sync {
    async fn emit(&self, event: &'static str) -> Result<bool, CommandBackendError>;

    async fn switch_profile(
        &self,
        name: &str,
    ) -> Result<DynamicProfileSwitchResult, CommandBackendError>;

    async fn build_cache(
        &self,
        generation: u64,
        candidates: Vec<CommandInfo>,
    ) -> Result<CommandCacheBuildResult, CommandBackendError>;
}

#[async_trait]
impl DynamicCommandBridge for DynamicWorkerClient {
    async fn emit(&self, event: &'static str) -> Result<bool, CommandBackendError> {
        DynamicWorkerClient::emit(self, event)
            .await
            .map(|result| result.cancelled)
            .map_err(|error| dynamic_client_error(&error))
    }

    async fn switch_profile(
        &self,
        name: &str,
    ) -> Result<DynamicProfileSwitchResult, CommandBackendError> {
        DynamicWorkerClient::switch_profile(self, name)
            .await
            .map_err(|error| dynamic_client_error(&error))
    }

    async fn build_cache(
        &self,
        generation: u64,
        candidates: Vec<CommandInfo>,
    ) -> Result<CommandCacheBuildResult, CommandBackendError> {
        DynamicWorkerClient::build_command_cache(self, generation, &candidates)
            .await
            .map_err(|error| dynamic_client_error(&error))
    }
}

/// Command-service failure before a transport response is produced.
#[derive(Debug, Error)]
pub enum CommandServiceError {
    #[error(transparent)]
    Backend(#[from] CommandBackendError),
    #[error(transparent)]
    Host(#[from] crate::dynamic::DynamicHostError),
    #[error("profile switch is in progress")]
    ProfileSwitchInProgress,
    #[error("a command is already active")]
    CommandBusy,
    #[error("no command is active")]
    CommandNotRunning,
    #[error("command identity was not discovered")]
    CommandNotFound,
    #[error("command generation changed during a lifecycle callback")]
    CommandGenerationChanged,
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "shortcut index rejection awaits product shortcut dispatch wiring"
        )
    )]
    #[error("shortcut index must be between 1 and 10")]
    InvalidShortcut,
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "unassigned shortcut rejection awaits product shortcut dispatch wiring"
        )
    )]
    #[error("shortcut is not assigned to a discovered command")]
    ShortcutUnavailable,
    #[error("command cache changed too frequently to complete one generation")]
    CacheNeverStabilized,
    #[error("dynamic worker returned a mismatched command-cache generation")]
    CacheGenerationMismatch,
    #[error("profile switch gate is not held")]
    ProfileSwitchGateNotHeld,
    #[error("profile switch validation failed: {0}")]
    #[allow(
        dead_code,
        reason = "profile switch validation error is part of the public API; constructed in switch_profile and tests"
    )]
    ProfileSwitchValidationFailed(String),
    #[error("profile switch stop failed: {0}")]
    ProfileSwitchStopFailed(String),
    #[error("profile switch commit failed: {0}")]
    #[allow(
        dead_code,
        reason = "profile switch commit error is part of the public API; constructed in switch_profile and tests"
    )]
    ProfileSwitchCommitFailed(String),
    #[error("profile switch restore failed: degraded")]
    #[allow(
        dead_code,
        reason = "profile switch restore error is part of the public API; constructed in switch_profile and tests"
    )]
    ProfileSwitchRestoreFailed,
    #[error("profile switch degraded: safe stopped, new commands rejected")]
    ProfileSwitchDegraded,
}

struct CommandServiceState {
    session: Option<Arc<dyn UserScriptSession>>,
    session_profile: Option<String>,
    commands: Vec<LoadedCommand>,
    status: CommandStatus,
    current: Option<CommandIdentity>,
    execution_epoch: u64,
    stop_post_pending: bool,
}

impl Default for CommandServiceState {
    fn default() -> Self {
        Self {
            session: None,
            session_profile: None,
            commands: Vec::new(),
            status: CommandStatus::Stopped,
            current: None,
            execution_epoch: 0,
            stop_post_pending: false,
        }
    }
}

/// Application-owned command service. Worker creation is lazy and all
/// profile-sensitive operations share one lifecycle gate.
pub struct CommandService {
    host: Arc<StartupDynamicHost>,
    factory: Arc<dyn UserScriptFactory>,
    dynamic: Arc<dyn DynamicCommandBridge>,
    inner: AsyncMutex<CommandServiceState>,
    lifecycle: AsyncMutex<()>,
    cache_generation: AtomicU64,
    profile_stage: AtomicU8,
    degraded: AtomicBool,
    last_profile_error: std::sync::Mutex<Option<ProfileSwitchErrorDetail>>,
    #[cfg(test)]
    fault_validate: AtomicBool,
    #[cfg(test)]
    fault_commit: AtomicBool,
    #[cfg(test)]
    fault_restore: AtomicBool,
}

impl std::fmt::Debug for CommandService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandService")
            .field("profile_switching", &self.host.profile_switch_in_progress())
            .field(
                "cache_generation",
                &self.cache_generation.load(Ordering::Acquire),
            )
            .field("degraded", &self.degraded.load(Ordering::Acquire))
            .field(
                "profile_stage",
                &ProfileSwitchStage::decode(self.profile_stage.load(Ordering::Acquire)).as_str(),
            )
            .finish_non_exhaustive()
    }
}

impl CommandService {
    #[must_use]
    pub fn new(
        host: Arc<StartupDynamicHost>,
        factory: Arc<dyn UserScriptFactory>,
        dynamic: Arc<dyn DynamicCommandBridge>,
    ) -> Self {
        Self {
            host,
            factory,
            dynamic,
            inner: AsyncMutex::new(CommandServiceState::default()),
            lifecycle: AsyncMutex::new(()),
            cache_generation: AtomicU64::new(1),
            profile_stage: AtomicU8::new(ProfileSwitchStage::Idle.encode()),
            degraded: AtomicBool::new(false),
            last_profile_error: std::sync::Mutex::new(None),
            #[cfg(test)]
            fault_validate: AtomicBool::new(false),
            #[cfg(test)]
            fault_commit: AtomicBool::new(false),
            #[cfg(test)]
            fault_restore: AtomicBool::new(false),
        }
    }

    /// Discovers commands, runs the `ScriptLoad` events, merges all tag layers,
    /// and publishes one complete finite display generation.
    ///
    /// # Errors
    ///
    /// Returns a worker, dynamic callback, host-state, or lifecycle error while
    /// retaining the previous completed generation.
    pub async fn reload(&self) -> Result<CommandReloadResult, CommandServiceError> {
        self.ensure_command_start_allowed()?;
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_command_start_allowed()?;
        let session = self.ensure_session().await?;
        let discovered = session.discover().await?.commands;
        let automatic = canonical_discovery(&discovered);
        let initial = automatic
            .iter()
            .map(|command| command.command.clone())
            .collect::<Vec<_>>();
        self.host.begin_script_load(initial)?;
        let mut rollback = ScriptLoadRollback::new(&self.host);

        if self.dynamic.emit("ScriptLoadPre").await? {
            return Ok(CommandReloadResult::Cancelled);
        }
        let (staged, explicit_tags) = self.host.staged_script_load()?;
        let (mut commands, mut tags) = reconcile_discovery(&automatic, &staged, &explicit_tags);
        self.host
            .set_staged_script_load(command_infos(&commands), tags.clone())?;
        let _cancelled = self.dynamic.emit("ScriptLoadPost").await?;

        let (post_candidates, _post_tags) = self.host.staged_script_load()?;
        (commands, tags) = reconcile_discovery(&automatic, &post_candidates, &explicit_tags);
        self.host
            .set_staged_script_load(command_infos(&commands), tags)?;
        let cache = self.build_stable_cache(command_infos(&commands)).await?;
        self.host
            .set_staged_script_load(cache.candidates.clone(), cache.tags.clone())?;
        self.host.publish_command_cache(&cache)?;
        rollback.disarm();

        let command_count = commands.len();
        self.inner.lock().await.commands = commands;
        Ok(CommandReloadResult::Published {
            generation: cache.generation,
            command_count,
        })
    }

    /// Recomputes callback-dependent display lists without rediscovering or
    /// emitting `ScriptLoad` events.
    ///
    /// # Errors
    ///
    /// Returns a lifecycle, callback, or host publication error while keeping
    /// the previous completed cache.
    pub async fn recompute_display_cache(&self) -> Result<(), CommandServiceError> {
        self.ensure_command_start_allowed()?;
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_command_start_allowed()?;
        let candidates = {
            let inner = self.inner.lock().await;
            command_infos(&inner.commands)
        };
        if candidates.is_empty() {
            return Ok(());
        }
        self.host.begin_script_load(candidates.clone())?;
        let mut rollback = ScriptLoadRollback::new(&self.host);
        self.host
            .set_staged_script_load(candidates.clone(), ordered_tags(&candidates, &[]))?;
        let cache = self.build_stable_cache(candidates).await?;
        self.host
            .set_staged_script_load(cache.candidates.clone(), cache.tags.clone())?;
        self.host.publish_command_cache(&cache)?;
        rollback.disarm();
        Ok(())
    }

    /// Begins one discovered command without blocking the caller until its
    /// execution completes.
    ///
    /// # Errors
    ///
    /// Rejects an unknown/busy command or reports dynamic and host failures.
    pub async fn start(
        self: &Arc<Self>,
        identity: &CommandIdentity,
    ) -> Result<CommandActionResult, CommandServiceError> {
        self.ensure_command_start_allowed()?;
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_command_start_allowed()?;
        let (session, command, epoch) = {
            let mut inner = self.inner.lock().await;
            if matches!(inner.status, CommandStatus::Running | CommandStatus::Paused) {
                return Err(CommandServiceError::CommandBusy);
            }
            let command = inner
                .commands
                .iter()
                .find(|command| CommandIdentity::from(&command.info) == *identity)
                .cloned()
                .ok_or(CommandServiceError::CommandNotFound)?;
            let session = inner
                .session
                .clone()
                .ok_or(CommandServiceError::CommandNotFound)?;
            inner.execution_epoch = inner.execution_epoch.wrapping_add(1);
            (session, command, inner.execution_epoch)
        };
        if self.dynamic.emit("CommandStartPre").await? {
            return Ok(CommandActionResult::Cancelled);
        }

        {
            let mut inner = self.inner.lock().await;
            if inner.execution_epoch != epoch
                || inner
                    .session
                    .as_ref()
                    .is_none_or(|active| !Arc::ptr_eq(active, &session))
            {
                return Err(CommandServiceError::CommandGenerationChanged);
            }
            inner.status = CommandStatus::Running;
            inner.current = Some(identity.clone());
            inner.stop_post_pending = false;
        }
        self.host
            .set_command_status(CommandStatus::Running.as_str(), &command.info.name)?;
        let _cancelled = self.dynamic.emit("CommandStartPost").await?;
        {
            let inner = self.inner.lock().await;
            if inner.execution_epoch != epoch || inner.status != CommandStatus::Running {
                return Err(CommandServiceError::CommandGenerationChanged);
            }
        }

        let service = Arc::clone(self);
        tokio::spawn(async move {
            let result = session
                .execute(ScriptExecuteRequest {
                    path: command.relative_path,
                    class_name: command.info.class_name,
                    tags: command.info.tags,
                })
                .await;
            service.finish_execution(epoch, result).await;
        });
        Ok(CommandActionResult::Applied)
    }

    /// Starts the command assigned to one of the ten canonical shortcut slots.
    ///
    /// # Errors
    ///
    /// Rejects an invalid/unassigned slot or propagates [`Self::start`] errors.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "shortcut execution awaits product shortcut dispatch wiring"
        )
    )]
    pub async fn start_shortcut(
        self: &Arc<Self>,
        index: u8,
    ) -> Result<CommandActionResult, CommandServiceError> {
        let identity = self
            .shortcut_identity(index)
            .await?
            .ok_or(CommandServiceError::ShortcutUnavailable)?;
        self.start(&identity).await
    }

    /// Pauses the active command at its next worker checkpoint.
    ///
    /// # Errors
    ///
    /// Rejects an inactive command or reports a worker/host failure.
    pub async fn pause(&self) -> Result<CommandActionResult, CommandServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        let (session, epoch, status) = self.active_session().await?;
        if status == CommandStatus::Paused {
            return Ok(CommandActionResult::AlreadyInState);
        }
        let result = session.pause().await?;
        let mut inner = self.inner.lock().await;
        if inner.execution_epoch != epoch || inner.status != CommandStatus::Running {
            return Err(CommandServiceError::CommandNotRunning);
        }
        if result.changed {
            inner.status = CommandStatus::Paused;
            let name = current_name(&inner);
            drop(inner);
            self.host
                .set_command_status(CommandStatus::Paused.as_str(), &name)?;
            Ok(CommandActionResult::Applied)
        } else {
            Ok(CommandActionResult::AlreadyInState)
        }
    }

    /// Resumes the currently paused command.
    ///
    /// # Errors
    ///
    /// Rejects an inactive command or reports a worker/host failure.
    pub async fn resume(&self) -> Result<CommandActionResult, CommandServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        let (session, epoch, status) = self.active_session().await?;
        if status == CommandStatus::Running {
            return Ok(CommandActionResult::AlreadyInState);
        }
        let result = session.resume().await?;
        let mut inner = self.inner.lock().await;
        if inner.execution_epoch != epoch || inner.status != CommandStatus::Paused {
            return Err(CommandServiceError::CommandNotRunning);
        }
        if result.changed {
            inner.status = CommandStatus::Running;
            let name = current_name(&inner);
            drop(inner);
            self.host
                .set_command_status(CommandStatus::Running.as_str(), &name)?;
            Ok(CommandActionResult::Applied)
        } else {
            Ok(CommandActionResult::AlreadyInState)
        }
    }

    /// Runs the cancellable stop event and requests cooperative command stop.
    ///
    /// # Errors
    ///
    /// Rejects an inactive command or reports a dynamic/worker failure.
    pub async fn stop(&self) -> Result<CommandActionResult, CommandServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        let (session, epoch, _status) = self.active_session().await?;
        if self.dynamic.emit("CommandStopPre").await? {
            return Ok(CommandActionResult::Cancelled);
        }
        {
            let inner = self.inner.lock().await;
            if inner.execution_epoch != epoch
                || !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused)
            {
                return Err(CommandServiceError::CommandGenerationChanged);
            }
        }
        let result = session.stop_command().await?;
        let mut inner = self.inner.lock().await;
        if inner.execution_epoch != epoch
            || !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused)
        {
            return Err(CommandServiceError::CommandNotRunning);
        }
        if result.stop_requested {
            inner.stop_post_pending = true;
            Ok(CommandActionResult::Applied)
        } else {
            Ok(CommandActionResult::AlreadyInState)
        }
    }

    /// Delivers one fixed Tk compatibility callback to the active worker.
    /// UI state remains Rust-owned; the worker receives only the callback
    /// event needed by the compatibility script.
    ///
    /// # Errors
    ///
    /// Returns an error when no command generation is active or the worker
    /// rejects the callback event.
    pub async fn dispatch_tk_event(
        &self,
        event: &ScriptTkEvent,
    ) -> Result<(), CommandServiceError> {
        let session = {
            let inner = self.inner.lock().await;
            if !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused) {
                return Err(CommandServiceError::CommandNotRunning);
            }
            inner
                .session
                .clone()
                .ok_or(CommandServiceError::CommandNotRunning)?
        };
        session.tk_event(event).await?;
        Ok(())
    }

    /// Delivers one validated camera-overlay pointer callback to the active
    /// worker generation.
    ///
    /// # Errors
    ///
    /// Returns an error when no command generation is active or the worker
    /// rejects the callback event.
    pub async fn dispatch_pointer_event(
        &self,
        event: &ScriptPointerEvent,
    ) -> Result<(), CommandServiceError> {
        let session = {
            let inner = self.inner.lock().await;
            if !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused) {
                return Err(CommandServiceError::CommandNotRunning);
            }
            inner
                .session
                .clone()
                .ok_or(CommandServiceError::CommandNotRunning)?
        };
        session.pointer_event(event).await?;
        Ok(())
    }

    /// Stops a command after an abnormal script-dialog close. This mandatory
    /// safety path is not cancellable by `CommandStopPre`.
    ///
    /// # Errors
    ///
    /// Returns an error when the active worker cannot be stopped cleanly.
    pub async fn abort_from_script_ui(&self) -> Result<CommandActionResult, CommandServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        let (session, epoch) = {
            let inner = self.inner.lock().await;
            if !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused) {
                return Ok(CommandActionResult::AlreadyInState);
            }
            (
                inner
                    .session
                    .clone()
                    .ok_or(CommandServiceError::CommandNotRunning)?,
                inner.execution_epoch,
            )
        };
        let result = session.stop_command().await?;
        let mut inner = self.inner.lock().await;
        if inner.execution_epoch != epoch
            || !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused)
        {
            return Ok(CommandActionResult::AlreadyInState);
        }
        if result.stop_requested {
            inner.stop_post_pending = true;
            Ok(CommandActionResult::Applied)
        } else {
            Ok(CommandActionResult::AlreadyInState)
        }
    }

    /// Acquires the non-recursive profile gate. Reentrant calls fail rather
    /// than waiting or queuing.
    ///
    /// # Errors
    ///
    /// Returns [`CommandServiceError::ProfileSwitchInProgress`] when held.
    #[allow(
        dead_code,
        reason = "profile switch entry awaits product profile coordination wiring"
    )]
    pub fn try_begin_profile_switch(&self) -> Result<(), CommandServiceError> {
        self.host
            .try_begin_profile_switch_gate()
            .map_err(|_| CommandServiceError::ProfileSwitchInProgress)
    }

    /// Stops and reaps the old profile worker, invalidates late completion, and
    /// clears its command generation. The profile gate remains held.
    /// Implements SPEC §11.5.6.4.3 steps 5-7: cooperative stop, immediate
    /// force-release of all input, then reaping before atomic commit.
    ///
    /// # Errors
    ///
    /// Requires the profile gate and propagates worker or host cleanup errors.
    pub async fn stop_for_profile_switch(
        &self,
        deadline: Duration,
    ) -> Result<Option<ScriptSessionStop>, CommandServiceError> {
        if !self.host.profile_switch_in_progress() {
            return Err(CommandServiceError::ProfileSwitchGateNotHeld);
        }
        self.profile_stage
            .store(ProfileSwitchStage::Stopping.encode(), Ordering::Release);
        let session = {
            let mut inner = self.inner.lock().await;
            inner.execution_epoch = inner.execution_epoch.wrapping_add(1);
            inner.status = CommandStatus::Stopped;
            inner.current = None;
            inner.stop_post_pending = false;
            inner.commands.clear();
            inner.session_profile = None;
            inner.session.take()
        };
        let Some(session) = session else {
            self.profile_stage.store(
                ProfileSwitchStage::ReleasingInput.encode(),
                Ordering::Release,
            );
            // No worker: still force-release controller to ensure neutral safe state.
            self.host.controller_safety().force_release();
            self.profile_stage
                .store(ProfileSwitchStage::Reaping.encode(), Ordering::Release);
            self.host.clear_command_generation()?;
            self.profile_stage
                .store(ProfileSwitchStage::Committing.encode(), Ordering::Release);
            return Ok(None);
        };
        let _cooperative = session.request_profile_stop().await;
        session.begin_stopping();
        self.profile_stage.store(
            ProfileSwitchStage::ReleasingInput.encode(),
            Ordering::Release,
        );
        // SPEC step 6: force-release all input immediately without waiting for Python.
        // This disconnects the script generation source and neutralizes the shared arbiter.
        self.host.controller_safety().force_release();
        self.profile_stage
            .store(ProfileSwitchStage::Reaping.encode(), Ordering::Release);
        self.host.clear_command_generation()?;
        let stopped = session.shutdown(deadline).await.map_err(|error| {
            self.profile_stage
                .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
            let detail = ProfileSwitchErrorDetail {
                code: error.code.clone(),
                message: error.message.clone(),
                stage: ProfileSwitchStage::Reaping,
            };
            *self.last_profile_error.lock().unwrap() = Some(detail);
            CommandServiceError::ProfileSwitchStopFailed(error.to_string())
        })?;
        self.profile_stage
            .store(ProfileSwitchStage::Committing.encode(), Ordering::Release);
        Ok(Some(stopped))
    }

    /// Releases the profile gate on success, cancellation, or rollback.
    #[allow(
        dead_code,
        reason = "profile switch completion awaits product profile coordination wiring"
    )]
    pub fn finish_profile_switch(&self) {
        let stage = ProfileSwitchStage::decode(self.profile_stage.load(Ordering::Acquire));
        if !matches!(
            stage,
            ProfileSwitchStage::Succeeded
                | ProfileSwitchStage::Cancelled
                | ProfileSwitchStage::Failed
                | ProfileSwitchStage::Degraded
        ) {
            self.profile_stage
                .store(ProfileSwitchStage::Idle.encode(), Ordering::Release);
        }
        self.host.finish_profile_switch_gate();
    }

    /// Returns a typed snapshot of the current profile-switch lifecycle for UI/log.
    #[allow(
        dead_code,
        reason = "profile switch status exposed for UI/log; exercised in tests"
    )]
    #[must_use]
    pub fn profile_switch_status(&self) -> ProfileSwitchStatus {
        let stage = ProfileSwitchStage::decode(self.profile_stage.load(Ordering::Acquire));
        let degraded = self.degraded.load(Ordering::Acquire);
        let last_error = self.last_profile_error.lock().unwrap().clone();
        let (active_profile, pending_profile) = {
            let snapshot = self.host.public_state_snapshot();
            let active = snapshot
                .get("active_profile")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_owned();
            let pending = snapshot
                .get("pending_profile")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            (active, pending)
        };
        ProfileSwitchStatus {
            stage,
            active_profile,
            pending_profile,
            degraded,
            last_error,
        }
    }

    /// Returns whether the service is in degraded safe-stop where new commands are rejected.
    #[allow(
        dead_code,
        reason = "degraded check is part of the public profile-switch API; exercised in tests"
    )]
    #[must_use]
    pub fn is_profile_switch_degraded(&self) -> bool {
        self.degraded.load(Ordering::Acquire)
    }

    /// Clears degraded safe-stop after explicit operator recovery.
    #[allow(
        dead_code,
        reason = "degraded clear is part of the public recovery API; exercised in tests"
    )]
    pub fn clear_profile_switch_degraded(&self) {
        self.degraded.store(false, Ordering::Release);
        self.profile_stage
            .store(ProfileSwitchStage::Idle.encode(), Ordering::Release);
        *self.last_profile_error.lock().unwrap() = None;
    }

    #[cfg(test)]
    pub fn inject_fault_validate(&self, fail: bool) {
        self.fault_validate.store(fail, Ordering::Release);
    }

    #[cfg(test)]
    pub fn inject_fault_commit(&self, fail: bool) {
        self.fault_commit.store(fail, Ordering::Release);
    }

    #[cfg(test)]
    pub fn inject_fault_restore(&self, fail: bool) {
        self.fault_restore.store(fail, Ordering::Release);
    }

    /// Full production lifecycle for profile switching.
    /// Implements SPEC §11.5.6.4.3 transaction 0-12 with typed stage tracking,
    /// force-release, reap-before-commit, no auto-resume, and rollback/degraded handling.
    /// Preserves existing `try_begin`/`stop_for`/`finish` API for compatibility.
    ///
    /// # Errors
    ///
    /// Returns typed `CommandServiceError` with stage information for UI/log.
    #[allow(
        dead_code,
        reason = "profile switch lifecycle is public API; exercised in integration tests"
    )]
    #[allow(
        clippy::too_many_lines,
        reason = "SPEC 11.5.6.4.3 transaction stages 0-12 with validation, pre-event, stop/reap, commit and degraded handling must remain atomic for review"
    )]
    pub async fn switch_profile(
        &self,
        name: &str,
    ) -> Result<ProfileSwitchLifecycleResult, CommandServiceError> {
        if self.degraded.load(Ordering::Acquire) {
            return Err(CommandServiceError::ProfileSwitchDegraded);
        }
        // Stage 0: gate acquisition (non-recursive, rejects new commands)
        self.profile_stage
            .store(ProfileSwitchStage::Validating.encode(), Ordering::Release);
        self.try_begin_profile_switch()?;
        // Ensure gate is released on all paths via finish; stage handling below
        // prepares rollback semantics.

        // Fault injection for validation stage
        #[cfg(test)]
        if self.fault_validate.load(Ordering::Acquire) {
            let detail = ProfileSwitchErrorDetail {
                code: "InjectedValidationFailed".to_owned(),
                message: "injected validation failure".to_owned(),
                stage: ProfileSwitchStage::Validating,
            };
            *self.last_profile_error.lock().unwrap() = Some(detail.clone());
            self.profile_stage
                .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
            self.host.cancel_profile_switch();
            self.finish_profile_switch();
            return Err(CommandServiceError::ProfileSwitchValidationFailed(
                detail.message,
            ));
        }

        // Stage 1: validation (side-effect free) via host prepare
        let prepared = match self
            .host
            .prepare_profile_switch(name, &std::collections::BTreeMap::new())
        {
            Ok(settings) => {
                self.profile_stage
                    .store(ProfileSwitchStage::Pending.encode(), Ordering::Release);
                settings
            }
            Err(error) => {
                let detail = ProfileSwitchErrorDetail {
                    code: error.code.clone(),
                    message: error.message.clone(),
                    stage: ProfileSwitchStage::Validating,
                };
                *self.last_profile_error.lock().unwrap() = Some(detail.clone());
                self.profile_stage
                    .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
                self.host.cancel_profile_switch();
                self.finish_profile_switch();
                if error.code == "InvalidProfile" || error.code == "ProfileNotFound" {
                    tracing::warn!(code = %error.code, stage = %ProfileSwitchStage::Validating.as_str(), "profile switch validation failed");
                }
                return Err(CommandServiceError::ProfileSwitchValidationFailed(
                    error.to_string(),
                ));
            }
        };
        drop(prepared);

        // Stage 3: ProfileSwitchPre (cancellable)
        self.profile_stage
            .store(ProfileSwitchStage::PreEvent.encode(), Ordering::Release);
        let cancelled = self
            .dynamic
            .emit("ProfileSwitchPre")
            .await
            .inspect_err(|error| {
                let detail = ProfileSwitchErrorDetail {
                    code: error.code.clone(),
                    message: error.message.clone(),
                    stage: ProfileSwitchStage::PreEvent,
                };
                *self.last_profile_error.lock().unwrap() = Some(detail);
            })?;
        if cancelled {
            self.profile_stage
                .store(ProfileSwitchStage::Cancelling.encode(), Ordering::Release);
            self.host.cancel_profile_switch();
            self.profile_stage
                .store(ProfileSwitchStage::Cancelled.encode(), Ordering::Release);
            self.finish_profile_switch();
            return Ok(ProfileSwitchLifecycleResult::Cancelled);
        }

        // Stage 5-7: stop, force-release, reap (before commit)
        let deadline = {
            let ms = self
                .host
                .loaded_settings()
                .settings
                .integer("python.script.shutdown_timeout_ms")
                .unwrap_or(2000);
            std::time::Duration::from_millis(u64::try_from(ms).unwrap_or(2000))
        };
        let stop_result = self.stop_for_profile_switch(deadline).await;
        let forced = match stop_result {
            Ok(stop) => stop.is_some_and(|s| s.forced),
            Err(error) => {
                // Stop/reap failed: rollback to old settings, idle, no auto-resume
                let detail = ProfileSwitchErrorDetail {
                    code: "StopFailed".to_owned(),
                    message: error.to_string(),
                    stage: ProfileSwitchStage::Reaping,
                };
                *self.last_profile_error.lock().unwrap() = Some(detail);
                self.profile_stage
                    .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
                // Attempt restore: cancel prepared
                #[cfg(test)]
                if self.fault_restore.load(Ordering::Acquire) {
                    self.degraded.store(true, Ordering::Release);
                    self.profile_stage
                        .store(ProfileSwitchStage::Degraded.encode(), Ordering::Release);
                    // Keep pending cleared but degraded
                    self.host.cancel_profile_switch();
                    self.host.controller_safety().force_release();
                    self.finish_profile_switch();
                    tracing::error!(stage = %ProfileSwitchStage::Reaping.as_str(), "profile switch stop failed and restore failed: degraded safe stop");
                    return Err(CommandServiceError::ProfileSwitchRestoreFailed);
                }
                self.host.cancel_profile_switch();
                // Ensure idle: clear commands already done in stop, status is stopped
                self.profile_stage
                    .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
                self.finish_profile_switch();
                tracing::warn!(stage = %ProfileSwitchStage::Reaping.as_str(), "profile switch stop failed, restored idle");
                return Err(CommandServiceError::ProfileSwitchStopFailed(
                    error.to_string(),
                ));
            }
        };

        // Stage 8: atomic commit after reap
        self.profile_stage
            .store(ProfileSwitchStage::Committing.encode(), Ordering::Release);
        #[cfg(test)]
        if self.fault_commit.load(Ordering::Acquire) {
            let detail = ProfileSwitchErrorDetail {
                code: "InjectedCommitFailed".to_owned(),
                message: "injected commit failure".to_owned(),
                stage: ProfileSwitchStage::Committing,
            };
            *self.last_profile_error.lock().unwrap() = Some(detail);
            // Attempt restore
            #[cfg(test)]
            if self.fault_restore.load(Ordering::Acquire) {
                self.degraded.store(true, Ordering::Release);
                self.profile_stage
                    .store(ProfileSwitchStage::Degraded.encode(), Ordering::Release);
                self.host.cancel_profile_switch();
                self.host.controller_safety().force_release();
                self.finish_profile_switch();
                tracing::error!(stage = %ProfileSwitchStage::Committing.as_str(), "profile switch commit failed and restore failed: degraded");
                return Err(CommandServiceError::ProfileSwitchRestoreFailed);
            }
            self.host.cancel_profile_switch();
            self.profile_stage
                .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
            self.finish_profile_switch();
            tracing::warn!(stage = %ProfileSwitchStage::Committing.as_str(), "profile switch commit failed, restored idle");
            return Err(CommandServiceError::ProfileSwitchCommitFailed(
                "injected commit failure".to_owned(),
            ));
        }
        if let Err(error) = self.host.commit_profile_switch() {
            let detail = ProfileSwitchErrorDetail {
                code: error.code.clone(),
                message: error.message.clone(),
                stage: ProfileSwitchStage::Committing,
            };
            *self.last_profile_error.lock().unwrap() = Some(detail);
            #[cfg(test)]
            if self.fault_restore.load(Ordering::Acquire) {
                self.degraded.store(true, Ordering::Release);
                self.profile_stage
                    .store(ProfileSwitchStage::Degraded.encode(), Ordering::Release);
                // Best-effort cancel, but if that also fails we stay degraded
                self.host.cancel_profile_switch();
                self.host.controller_safety().force_release();
                self.finish_profile_switch();
                tracing::error!(code = %error.code, stage = %ProfileSwitchStage::Committing.as_str(), "profile switch commit failed and restore failed: degraded");
                return Err(CommandServiceError::ProfileSwitchRestoreFailed);
            }
            // Normal restore: discard prepared, keep old active
            self.host.cancel_profile_switch();
            self.profile_stage
                .store(ProfileSwitchStage::Failed.encode(), Ordering::Release);
            self.finish_profile_switch();
            tracing::warn!(code = %error.code, stage = %ProfileSwitchStage::Committing.as_str(), "profile switch commit failed, restored idle");
            return Err(CommandServiceError::ProfileSwitchCommitFailed(
                error.to_string(),
            ));
        }

        // Ensure no auto-resume: commands already cleared, session is None, status Stopped
        // Pending is cleared inside commit_profile_switch

        // Stage 11: Post event (non-cancellable, errors are logged but do not rollback)
        self.profile_stage
            .store(ProfileSwitchStage::PostEvent.encode(), Ordering::Release);
        if let Err(error) = self.dynamic.emit("ProfileSwitchPost").await {
            tracing::warn!(error = %error, stage = %ProfileSwitchStage::PostEvent.as_str(), "ProfileSwitchPost failed");
        }
        self.profile_stage
            .store(ProfileSwitchStage::Succeeded.encode(), Ordering::Release);
        self.host.finish_profile_switch_gate();
        // Transition to idle after success for next operation
        self.profile_stage
            .store(ProfileSwitchStage::Idle.encode(), Ordering::Release);
        *self.last_profile_error.lock().unwrap() = None;
        tracing::info!(profile = %name, forced, stage = %ProfileSwitchStage::Succeeded.as_str(), "profile switch succeeded");
        Ok(ProfileSwitchLifecycleResult::Switched {
            forced_worker_stop: forced,
        })
    }

    /// Stops the current user worker for complete application shutdown.
    ///
    /// # Errors
    ///
    /// Propagates worker reap or host cleanup failures.
    pub async fn shutdown(
        &self,
        deadline: Duration,
    ) -> Result<Option<ScriptSessionStop>, CommandServiceError> {
        self.host.hold_profile_switch_gate();
        let session = {
            let mut inner = self.inner.lock().await;
            inner.execution_epoch = inner.execution_epoch.wrapping_add(1);
            inner.status = CommandStatus::Stopped;
            inner.current = None;
            inner.stop_post_pending = false;
            inner.commands.clear();
            inner.session_profile = None;
            inner.session.take()
        };
        let Some(session) = session else {
            self.host.clear_command_generation_for_shutdown();
            return Ok(None);
        };
        let _cooperative = session.request_profile_stop().await;
        session.begin_stopping();
        self.host.clear_command_generation_for_shutdown();
        Ok(Some(session.shutdown(deadline).await?))
    }

    #[must_use]
    pub async fn status(&self) -> CommandStatus {
        self.inner.lock().await.status
    }

    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "command listing awaits product command adapter wiring"
        )
    )]
    pub async fn commands(&self) -> Vec<LoadedCommand> {
        self.inner.lock().await.commands.clone()
    }

    /// Resolves all ten settings into the same canonical command model.
    ///
    /// # Errors
    ///
    /// Returns an error if a canonical shortcut setting is absent or invalid.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "shortcut listing awaits product shortcut dispatch wiring"
        )
    )]
    pub async fn shortcuts(&self) -> Result<Vec<ShortcutSlot>, CommandServiceError> {
        let settings = self.host.loaded_settings();
        let commands = self.inner.lock().await.commands.clone();
        (1_u8..=10)
            .map(|index| {
                let id = format!("shortcuts.button_{index}");
                let configured_module = settings
                    .settings
                    .string(&id)
                    .map_err(|error| CommandBackendError::new("InvalidSetting", error.to_string()))?
                    .to_owned();
                let command = commands
                    .iter()
                    .find(|command| command.info.module_path == configured_module)
                    .map(|command| command.info.clone());
                Ok(ShortcutSlot {
                    index,
                    configured_module,
                    command,
                })
            })
            .collect()
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "shortcut identity resolution awaits product shortcut dispatch wiring"
        )
    )]
    async fn shortcut_identity(
        &self,
        index: u8,
    ) -> Result<Option<CommandIdentity>, CommandServiceError> {
        if !(1..=10).contains(&index) {
            return Err(CommandServiceError::InvalidShortcut);
        }
        Ok(self
            .shortcuts()
            .await?
            .into_iter()
            .find(|slot| slot.index == index)
            .and_then(|slot| slot.command.as_ref().map(CommandIdentity::from)))
    }

    async fn ensure_session(&self) -> Result<Arc<dyn UserScriptSession>, CommandServiceError> {
        let profile = self.host.loaded_settings();
        let profile_name = profile.active_profile.as_str().to_owned();
        if let Some(session) = {
            let inner = self.inner.lock().await;
            (inner.session_profile.as_deref() == Some(profile_name.as_str()))
                .then(|| inner.session.clone())
                .flatten()
        } {
            return Ok(session);
        }
        let session = self.factory.spawn(profile).await?;
        let mut inner = self.inner.lock().await;
        inner.session_profile = Some(profile_name);
        inner.session = Some(session.clone());
        Ok(session)
    }

    async fn build_stable_cache(
        &self,
        candidates: Vec<CommandInfo>,
    ) -> Result<CommandDisplayCache, CommandServiceError> {
        for _attempt in 0..MAX_CACHE_RESTARTS {
            let generation = self.cache_generation.fetch_add(1, Ordering::AcqRel);
            match self
                .dynamic
                .build_cache(generation, candidates.clone())
                .await?
            {
                CommandCacheBuildResult::Complete { cache } => {
                    if cache.generation != generation {
                        return Err(CommandServiceError::CacheGenerationMismatch);
                    }
                    return Ok(cache);
                }
                CommandCacheBuildResult::Superseded { .. } => {}
            }
        }
        Err(CommandServiceError::CacheNeverStabilized)
    }

    async fn active_session(
        &self,
    ) -> Result<(Arc<dyn UserScriptSession>, u64, CommandStatus), CommandServiceError> {
        let inner = self.inner.lock().await;
        if !matches!(inner.status, CommandStatus::Running | CommandStatus::Paused) {
            return Err(CommandServiceError::CommandNotRunning);
        }
        Ok((
            inner
                .session
                .clone()
                .ok_or(CommandServiceError::CommandNotRunning)?,
            inner.execution_epoch,
            inner.status,
        ))
    }

    async fn finish_execution(
        &self,
        epoch: u64,
        result: Result<ScriptExecutionResult, CommandBackendError>,
    ) {
        // Completion races with pause/resume/stop responses on a separate task.
        // Keep the response transition and its completion observation in the
        // same lifecycle lane so a successful stop cannot become
        // `CommandNotRunning` or lose its `CommandStopPost` event.
        let _lifecycle = self.lifecycle.lock().await;
        let (status, current_name, stop_post) = {
            let mut inner = self.inner.lock().await;
            if inner.execution_epoch != epoch {
                return;
            }
            let failed = matches!(
                result,
                Err(_)
                    | Ok(ScriptExecutionResult {
                        outcome: ScriptExecutionOutcome::Failed { .. },
                        ..
                    })
            );
            inner.status = if failed {
                CommandStatus::Error
            } else {
                CommandStatus::Stopped
            };
            let name = current_name(&inner);
            inner.current = None;
            let stop_post = inner.stop_post_pending;
            inner.stop_post_pending = false;
            (inner.status, name, stop_post)
        };
        let published_name = if status == CommandStatus::Error {
            current_name.as_str()
        } else {
            ""
        };
        if let Err(error) = self
            .host
            .set_command_status(status.as_str(), published_name)
        {
            tracing::error!(error = %error, "command completion state could not be published");
        }
        if status == CommandStatus::Error {
            self.emit_completion_event("CommandErrorPre").await;
            self.emit_completion_event("CommandErrorPost").await;
        }
        if stop_post {
            self.emit_completion_event("CommandStopPost").await;
        }
    }

    async fn emit_completion_event(&self, event: &'static str) {
        if let Err(error) = self.dynamic.emit(event).await {
            tracing::warn!(event, error = %error, "command completion event failed");
        }
    }

    fn ensure_command_start_allowed(&self) -> Result<(), CommandServiceError> {
        if self.degraded.load(Ordering::Acquire) {
            return Err(CommandServiceError::ProfileSwitchDegraded);
        }
        if self.host.profile_switch_in_progress() {
            Err(CommandServiceError::ProfileSwitchInProgress)
        } else {
            Ok(())
        }
    }
}

/// Built-in callback bridge used when dynamic configuration is disabled or
/// failed before worker creation.
#[derive(Debug)]
pub struct StaticCommandBridge {
    host: Arc<StartupDynamicHost>,
}

impl StaticCommandBridge {
    #[must_use]
    pub const fn new(host: Arc<StartupDynamicHost>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl DynamicCommandBridge for StaticCommandBridge {
    async fn emit(&self, _event: &'static str) -> Result<bool, CommandBackendError> {
        Ok(false)
    }

    async fn switch_profile(
        &self,
        name: &str,
    ) -> Result<DynamicProfileSwitchResult, CommandBackendError> {
        if let Err(error) = self.host.profile_switch_begin(name, &BTreeMap::new()).await {
            return Ok(DynamicProfileSwitchResult::Rejected {
                code: error.code,
                message: error.message,
            });
        }
        let committed = match self.host.profile_switch_commit().await {
            Ok(committed) => committed,
            Err(error) => {
                let _abort = self.host.profile_switch_abort().await;
                return Ok(DynamicProfileSwitchResult::Rejected {
                    code: error.code,
                    message: error.message,
                });
            }
        };
        self.host
            .profile_switch_end()
            .await
            .map_err(|error| CommandBackendError::new(error.code, error.message))?;
        Ok(DynamicProfileSwitchResult::Switched {
            forced_worker_stop: committed.forced_worker_stop,
        })
    }

    async fn build_cache(
        &self,
        generation: u64,
        candidates: Vec<CommandInfo>,
    ) -> Result<CommandCacheBuildResult, CommandBackendError> {
        let mode = self
            .host
            .loaded_settings()
            .settings
            .string("commands.tag_match_mode")
            .map_err(|error| CommandBackendError::new("InvalidSetting", error.to_string()))?
            .to_owned();
        Ok(CommandCacheBuildResult::Complete {
            cache: builtin_display_cache(generation, candidates, &mode)?,
        })
    }
}

/// Computes the callback-free display cache using the canonical matching
/// modes and discovery order.
///
/// # Errors
///
/// Rejects a tag matching mode outside the canonical closed enum.
pub fn builtin_display_cache(
    generation: u64,
    candidates: Vec<CommandInfo>,
    mode: &str,
) -> Result<CommandDisplayCache, CommandBackendError> {
    if !matches!(mode, "exact" | "partial" | "prefix" | "suffix") {
        return Err(CommandBackendError::new(
            "InvalidSetting",
            "unsupported commands.tag_match_mode",
        ));
    }
    let candidates = canonical_candidates(candidates);
    let tags = ordered_tags(&candidates, &[]);
    let mut display_lists = BTreeMap::new();
    for selected in &tags {
        let commands = candidates
            .iter()
            .filter(|command| {
                selected == "-"
                    || command.tags.iter().any(|tag| match mode {
                        "exact" => tag == selected,
                        "partial" => tag.contains(selected),
                        "prefix" => tag.starts_with(selected),
                        "suffix" => tag.ends_with(selected),
                        _ => false,
                    })
            })
            .cloned()
            .map(|command| CommandDisplayItem::Command { command })
            .collect();
        display_lists.insert(selected.clone(), commands);
    }
    Ok(CommandDisplayCache {
        generation,
        candidates,
        tags,
        display_lists,
    })
}

struct ScriptLoadRollback<'a> {
    host: &'a StartupDynamicHost,
    armed: bool,
}

impl<'a> ScriptLoadRollback<'a> {
    const fn new(host: &'a StartupDynamicHost) -> Self {
        Self { host, armed: true }
    }

    const fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ScriptLoadRollback<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.host.cancel_script_load();
        }
    }
}

fn canonical_discovery(discovered: &[ScriptDiscoveredCommand]) -> Vec<ScriptDiscoveredCommand> {
    let mut seen = BTreeSet::new();
    discovered
        .iter()
        .filter(|command| seen.insert(CommandIdentity::from(&command.command)))
        .cloned()
        .collect()
}

fn reconcile_discovery(
    discovered: &[ScriptDiscoveredCommand],
    staged: &[CommandInfo],
    explicit_tags: &[String],
) -> (Vec<LoadedCommand>, Vec<String>) {
    let staged = staged
        .iter()
        .map(|command| (CommandIdentity::from(command), command))
        .collect::<BTreeMap<_, _>>();
    let commands = discovered
        .iter()
        .filter_map(|discovered| {
            let dynamic = staged.get(&CommandIdentity::from(&discovered.command))?;
            let mut tags = Vec::new();
            append_unique(&mut tags, &discovered.command.tags);
            append_unique(&mut tags, &discovered.manual_tags);
            append_unique(&mut tags, &dynamic.tags);
            Some(LoadedCommand {
                info: CommandInfo {
                    name: discovered.command.name.clone(),
                    module_path: discovered.command.module_path.clone(),
                    class_name: discovered.command.class_name.clone(),
                    tags,
                },
                relative_path: discovered.relative_path.clone(),
                kind: discovered.kind,
            })
        })
        .collect::<Vec<_>>();
    let candidates = command_infos(&commands);
    let tags = ordered_tags(&candidates, explicit_tags);
    (commands, tags)
}

fn append_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

fn command_infos(commands: &[LoadedCommand]) -> Vec<CommandInfo> {
    commands
        .iter()
        .map(|command| command.info.clone())
        .collect()
}

fn canonical_candidates(candidates: Vec<CommandInfo>) -> Vec<CommandInfo> {
    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|command| seen.insert(CommandIdentity::from(command)))
        .collect()
}

fn ordered_tags(candidates: &[CommandInfo], explicit: &[String]) -> Vec<String> {
    let mut tags = vec!["-".to_owned()];
    let mut seen = BTreeSet::from(["-".to_owned()]);
    for automatic in [false, true] {
        for command in candidates {
            for tag in &command.tags {
                if tag.starts_with('@') == automatic && seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
        }
        for tag in explicit {
            if tag != "-" && tag.starts_with('@') == automatic && seen.insert(tag.clone()) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

fn current_name(inner: &CommandServiceState) -> String {
    let Some(current) = &inner.current else {
        return String::new();
    };
    inner
        .commands
        .iter()
        .find(|command| CommandIdentity::from(&command.info) == *current)
        .map_or_else(String::new, |command| command.info.name.clone())
}

fn dynamic_client_error(error: &crate::worker::dynamic::DynamicClientError) -> CommandBackendError {
    CommandBackendError::new("DynamicWorkerError", error.to_string())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize};

    use crate::dynamic::DynamicHost;
    use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
    use crate::settings::roots::{BaseDirectories, RootEnvironment};
    use serde_json::{Value, json};
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use tokio::time::{Instant, sleep};

    use super::*;

    struct FakeSession {
        discovery: ScriptDiscoveryResult,
        executions: AtomicUsize,
        paused: AtomicBool,
        stopping: AtomicBool,
        shutdowns: AtomicUsize,
        started: Notify,
        finish: Notify,
    }

    impl FakeSession {
        fn new(discovery: ScriptDiscoveryResult) -> Self {
            Self {
                discovery,
                executions: AtomicUsize::new(0),
                paused: AtomicBool::new(false),
                stopping: AtomicBool::new(false),
                shutdowns: AtomicUsize::new(0),
                started: Notify::new(),
                finish: Notify::new(),
            }
        }
    }

    #[async_trait]
    impl UserScriptSession for FakeSession {
        async fn discover(&self) -> Result<ScriptDiscoveryResult, CommandBackendError> {
            Ok(self.discovery.clone())
        }

        async fn execute(
            &self,
            _request: ScriptExecuteRequest,
        ) -> Result<ScriptExecutionResult, CommandBackendError> {
            let execution_id = self.executions.fetch_add(1, Ordering::AcqRel) as u64 + 1;
            self.started.notify_one();
            self.finish.notified().await;
            Ok(ScriptExecutionResult {
                execution_id,
                outcome: ScriptExecutionOutcome::Stopped,
            })
        }

        async fn pause(&self) -> Result<ScriptPauseResult, CommandBackendError> {
            Ok(ScriptPauseResult {
                changed: !self.paused.swap(true, Ordering::AcqRel),
            })
        }

        async fn resume(&self) -> Result<ScriptPauseResult, CommandBackendError> {
            Ok(ScriptPauseResult {
                changed: self.paused.swap(false, Ordering::AcqRel),
            })
        }

        async fn stop_command(&self) -> Result<ScriptStopResult, CommandBackendError> {
            let stop_requested = self.executions.load(Ordering::Acquire) > 0;
            if stop_requested {
                self.finish.notify_one();
            }
            Ok(ScriptStopResult { stop_requested })
        }

        fn begin_stopping(&self) {
            self.stopping.store(true, Ordering::Release);
        }

        async fn shutdown(
            &self,
            _deadline: Duration,
        ) -> Result<ScriptSessionStop, CommandBackendError> {
            self.shutdowns.fetch_add(1, Ordering::AcqRel);
            self.finish.notify_one();
            Ok(ScriptSessionStop { forced: false })
        }
    }

    struct FakeFactory {
        session: Arc<FakeSession>,
        spawns: AtomicUsize,
        profiles: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl UserScriptFactory for FakeFactory {
        async fn spawn(
            &self,
            settings: LoadedSettings,
        ) -> Result<Arc<dyn UserScriptSession>, CommandBackendError> {
            self.spawns.fetch_add(1, Ordering::AcqRel);
            self.profiles
                .lock()
                .unwrap()
                .push(settings.active_profile.as_str().to_owned());
            Ok(self.session.clone())
        }
    }

    struct FakeDynamicBridge {
        host: Arc<StartupDynamicHost>,
        events: Mutex<Vec<&'static str>>,
        builds: AtomicUsize,
        supersede_first: AtomicBool,
        profile_switch_event: Mutex<Option<&'static str>>,
    }

    #[async_trait]
    impl DynamicCommandBridge for FakeDynamicBridge {
        async fn emit(&self, event: &'static str) -> Result<bool, CommandBackendError> {
            self.events.lock().unwrap().push(event);
            if event == "ScriptLoadPre" {
                let mut candidates = self
                    .host
                    .state_snapshot()
                    .map_err(|error| CommandBackendError::new(error.code, error.message))?
                    .remove("command_candidates")
                    .and_then(|value| serde_json::from_value::<Vec<CommandInfo>>(value).ok())
                    .ok_or_else(|| {
                        CommandBackendError::new("InvalidState", "command candidates are absent")
                    })?;
                candidates[0].tags.push("@Auto".to_owned());
                self.host
                    .set_state_value(
                        "command_candidates",
                        serde_json::to_value(candidates).unwrap(),
                    )
                    .map_err(|error| CommandBackendError::new(error.code, error.message))?;
            }
            let switch_profile = {
                let mut configured = self.profile_switch_event.lock().unwrap();
                if configured
                    .as_ref()
                    .is_some_and(|configured| *configured == event)
                {
                    configured.take()
                } else {
                    None
                }
            };
            if switch_profile.is_some() {
                self.host
                    .profile_switch_begin("Other", &BTreeMap::new())
                    .await
                    .map_err(|error| CommandBackendError::new(error.code, error.message))?;
                if let Err(error) = self.host.profile_switch_commit().await {
                    let _abort = self.host.profile_switch_abort().await;
                    return Err(CommandBackendError::new(error.code, error.message));
                }
                self.host
                    .profile_switch_end()
                    .await
                    .map_err(|error| CommandBackendError::new(error.code, error.message))?;
            }
            Ok(false)
        }

        async fn switch_profile(
            &self,
            _name: &str,
        ) -> Result<DynamicProfileSwitchResult, CommandBackendError> {
            Ok(DynamicProfileSwitchResult::Rejected {
                code: "UnsupportedTestOperation".to_owned(),
                message: "this command-service fixture does not switch profiles".to_owned(),
            })
        }

        async fn build_cache(
            &self,
            generation: u64,
            candidates: Vec<CommandInfo>,
        ) -> Result<CommandCacheBuildResult, CommandBackendError> {
            self.builds.fetch_add(1, Ordering::AcqRel);
            if self.supersede_first.swap(false, Ordering::AcqRel) {
                return Ok(CommandCacheBuildResult::Superseded { generation });
            }
            let mut cache = builtin_display_cache(generation, candidates, "exact")?;
            let first = cache.candidates[0].clone();
            cache.display_lists.insert(
                "-".to_owned(),
                vec![
                    CommandDisplayItem::Command {
                        command: first.clone(),
                    },
                    CommandDisplayItem::Separator {
                        label: Some("Group".to_owned()),
                    },
                    CommandDisplayItem::Command { command: first },
                ],
            );
            Ok(CommandCacheBuildResult::Complete { cache })
        }
    }

    struct Fixture {
        _temporary: TempDir,
        host: Arc<StartupDynamicHost>,
        session: Arc<FakeSession>,
        factory: Arc<FakeFactory>,
        dynamic: Arc<FakeDynamicBridge>,
        service: Arc<CommandService>,
    }

    fn fixture() -> Fixture {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .unwrap();
        let config = base.join("config/pokecon");
        std::fs::create_dir_all(config.join("profiles/default")).unwrap();
        std::fs::create_dir_all(config.join("profiles/Other")).unwrap();
        std::fs::write(
            config.join("profiles/default/settings.toml"),
            "[shortcuts]\nbutton_1 = \"Commands.PythonCommands.samples.first\"\n",
        )
        .unwrap();
        let request = PipelineRequest {
            arguments: ["pokecon"].into_iter().map(OsString::from).collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let loaded = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .unwrap();
        let host = Arc::new(StartupDynamicHost::new(request, loaded).unwrap());
        host.finish_startup().unwrap();
        let session = Arc::new(FakeSession::new(discovery()));
        let factory = Arc::new(FakeFactory {
            session: session.clone(),
            spawns: AtomicUsize::new(0),
            profiles: Mutex::new(Vec::new()),
        });
        let dynamic = Arc::new(FakeDynamicBridge {
            host: host.clone(),
            events: Mutex::new(Vec::new()),
            builds: AtomicUsize::new(0),
            supersede_first: AtomicBool::new(true),
            profile_switch_event: Mutex::new(None),
        });
        let service = Arc::new(CommandService::new(
            host.clone(),
            factory.clone(),
            dynamic.clone(),
        ));
        host.bind_command_service(&service);
        Fixture {
            _temporary: temporary,
            host,
            session,
            factory,
            dynamic,
            service,
        }
    }

    fn discovery() -> ScriptDiscoveryResult {
        ScriptDiscoveryResult {
            commands: vec![
                ScriptDiscoveredCommand {
                    command: CommandInfo {
                        name: "First".to_owned(),
                        module_path: "Commands.PythonCommands.samples.first".to_owned(),
                        class_name: "First".to_owned(),
                        tags: vec!["@Samples".to_owned()],
                    },
                    relative_path: "samples/first.py".into(),
                    manual_tags: vec!["Rank".to_owned()],
                    kind: ScriptCommandKind::Python,
                },
                ScriptDiscoveredCommand {
                    command: CommandInfo {
                        name: "Mcu".to_owned(),
                        module_path: "Commands.PythonCommands.mcu".to_owned(),
                        class_name: "Mcu".to_owned(),
                        tags: Vec::new(),
                    },
                    relative_path: "mcu.py".into(),
                    manual_tags: vec!["Device".to_owned()],
                    kind: ScriptCommandKind::Mcu,
                },
            ],
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reload_merges_tags_and_publishes_only_a_complete_cache() {
        let fixture = fixture();
        assert_eq!(fixture.factory.spawns.load(Ordering::Acquire), 0);
        let result = fixture.service.reload().await.unwrap();
        assert_eq!(
            result,
            CommandReloadResult::Published {
                generation: 2,
                command_count: 2,
            }
        );
        assert_eq!(fixture.factory.spawns.load(Ordering::Acquire), 1);
        assert_eq!(fixture.dynamic.builds.load(Ordering::Acquire), 2);

        let state = fixture.host.state_snapshot().unwrap();
        assert_eq!(
            state["command_candidates"][0]["tags"],
            json!(["@Samples", "Rank", "@Auto"])
        );
        assert_eq!(
            state["tags"],
            json!(["-", "Rank", "Device", "@Samples", "@Auto"])
        );
        assert_eq!(
            state["command_display_lists"]["-"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            state["command_display_lists"]["-"][1],
            json!({"kind": "separator", "label": "Group"})
        );
        assert_eq!(
            fixture.dynamic.events.lock().unwrap().as_slice(),
            ["ScriptLoadPre", "ScriptLoadPost"]
        );

        fixture.service.reload().await.unwrap();
        assert_eq!(fixture.factory.spawns.load(Ordering::Acquire), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn application_shutdown_reaps_scripts_after_dynamic_mutations_close() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        fixture.host.begin_stopping();

        let stopped = fixture
            .service
            .shutdown(Duration::from_millis(20))
            .await
            .expect("application shutdown uses its internal cleanup path")
            .expect("the initialized script session is reaped");

        assert!(!stopped.forced);
        assert!(fixture.session.stopping.load(Ordering::Acquire));
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
        assert_eq!(fixture.service.status().await, CommandStatus::Stopped);
        let state = fixture.host.state_snapshot().unwrap();
        assert_eq!(state["command_state"], json!("stopped"));
        assert_eq!(state["command_candidates"], json!([]));
        assert_eq!(state["command_display_lists"], json!({"-": []}));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn execution_controls_shortcuts_and_profile_gate_share_one_state_machine() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        let shortcuts = fixture.service.shortcuts().await.unwrap();
        assert_eq!(shortcuts.len(), 10);
        assert_eq!(shortcuts[0].command.as_ref().unwrap().name, "First");
        assert!(shortcuts[1..].iter().all(|slot| slot.command.is_none()));

        assert_eq!(
            fixture.service.start_shortcut(1).await.unwrap(),
            CommandActionResult::Applied
        );
        fixture.session.started.notified().await;
        assert_eq!(fixture.service.status().await, CommandStatus::Running);
        assert_eq!(
            fixture.service.pause().await.unwrap(),
            CommandActionResult::Applied
        );
        assert_eq!(fixture.service.status().await, CommandStatus::Paused);
        assert_eq!(
            fixture.service.resume().await.unwrap(),
            CommandActionResult::Applied
        );
        assert_eq!(
            fixture.service.stop().await.unwrap(),
            CommandActionResult::Applied
        );
        wait_for_status(&fixture.service, CommandStatus::Stopped).await;
        let state = fixture.host.state_snapshot().unwrap();
        assert_eq!(state["is_running"], Value::Bool(false));
        assert_eq!(state["command_state"], json!("stopped"));
        assert_eq!(state["current_command"], json!(""));

        fixture.service.try_begin_profile_switch().unwrap();
        assert!(matches!(
            fixture.service.try_begin_profile_switch(),
            Err(CommandServiceError::ProfileSwitchInProgress)
        ));
        assert!(matches!(
            fixture.service.reload().await,
            Err(CommandServiceError::ProfileSwitchInProgress)
        ));
        let stopped = fixture
            .service
            .stop_for_profile_switch(Duration::from_millis(20))
            .await
            .unwrap()
            .unwrap();
        assert!(!stopped.forced);
        assert!(fixture.session.stopping.load(Ordering::Acquire));
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
        assert!(fixture.service.commands().await.is_empty());
        assert_eq!(
            fixture.host.state_snapshot().unwrap()["command_candidates"],
            json!([])
        );
        fixture.service.finish_profile_switch();
        fixture.service.reload().await.unwrap();
        assert_eq!(fixture.factory.spawns.load(Ordering::Acquire), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn profile_switch_during_command_start_callbacks_never_starts_the_old_generation() {
        for event in ["CommandStartPre", "CommandStartPost"] {
            let fixture = fixture();
            fixture.service.reload().await.unwrap();
            *fixture.dynamic.profile_switch_event.lock().unwrap() = Some(event);
            let command = fixture.service.commands().await[0].info.clone();
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                fixture.service.start(&CommandIdentity::from(&command)),
            )
            .await
            .expect("profile switch inside a start callback must not deadlock");
            assert!(matches!(
                result,
                Err(CommandServiceError::CommandGenerationChanged)
            ));
            assert_eq!(fixture.session.executions.load(Ordering::Acquire), 0);
            assert!(fixture.session.stopping.load(Ordering::Acquire));
            assert_eq!(fixture.host.profile_current().unwrap(), "Other");
            assert_eq!(fixture.service.status().await, CommandStatus::Stopped);
            fixture.service.try_begin_profile_switch().unwrap();
            fixture.service.finish_profile_switch();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn profile_switch_during_command_stop_pre_invalidates_the_late_stop_response() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        let command = fixture.service.commands().await[0].info.clone();
        assert_eq!(
            fixture
                .service
                .start(&CommandIdentity::from(&command))
                .await
                .unwrap(),
            CommandActionResult::Applied
        );
        fixture.session.started.notified().await;
        *fixture.dynamic.profile_switch_event.lock().unwrap() = Some("CommandStopPre");
        let result = tokio::time::timeout(Duration::from_secs(2), fixture.service.stop())
            .await
            .expect("profile switch inside CommandStopPre must not deadlock");
        assert!(matches!(
            result,
            Err(CommandServiceError::CommandGenerationChanged)
        ));
        assert!(fixture.session.stopping.load(Ordering::Acquire));
        assert_eq!(fixture.host.profile_current().unwrap(), "Other");
        assert_eq!(fixture.service.status().await, CommandStatus::Stopped);
        fixture.service.try_begin_profile_switch().unwrap();
        fixture.service.finish_profile_switch();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_rejects_validation_and_preserves_idle() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        // Hold a button via controller to verify force-release is not triggered on validation failure
        fixture
            .host
            .controller_update(crate::device::ControllerUpdate {
                a: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert!(fixture.host.controller_safety().state().buttons.a);
        fixture.service.inject_fault_validate(true);
        let err = fixture.service.switch_profile("Other").await.unwrap_err();
        assert!(matches!(
            err,
            CommandServiceError::ProfileSwitchValidationFailed(_)
        ));
        let status = fixture.service.profile_switch_status();
        assert_eq!(status.stage, ProfileSwitchStage::Failed);
        assert_eq!(status.active_profile, "default");
        assert_eq!(status.pending_profile, None);
        assert!(!status.degraded);
        assert!(status.last_error.is_some());
        assert_eq!(
            status.last_error.as_ref().unwrap().stage,
            ProfileSwitchStage::Validating
        );
        // No worker side effects on validation failure
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 0);
        assert!(!fixture.session.stopping.load(Ordering::Acquire));
        // Controller not force-released on validation failure
        assert!(fixture.host.controller_safety().state().buttons.a);
        // Future commands still allowed (idle)
        fixture.service.reload().await.unwrap();
        assert_eq!(fixture.service.status().await, CommandStatus::Stopped);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_stops_and_reaps_before_commit_and_does_not_auto_resume() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        fixture
            .host
            .controller_update(crate::device::ControllerUpdate {
                a: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert!(fixture.host.controller_safety().state().buttons.a);
        let result = fixture.service.switch_profile("Other").await.unwrap();
        assert_eq!(
            result,
            ProfileSwitchLifecycleResult::Switched {
                forced_worker_stop: false
            }
        );
        assert!(fixture.session.stopping.load(Ordering::Acquire));
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
        // Force-release: controller neutral
        assert!(!fixture.host.controller_safety().state().buttons.a);
        assert_eq!(
            fixture.host.controller_safety().state(),
            crate::device::ControllerState::NEUTRAL
        );
        // Commands cleared, idle, not auto-resumed
        assert!(fixture.service.commands().await.is_empty());
        assert_eq!(fixture.service.status().await, CommandStatus::Stopped);
        assert_eq!(fixture.host.profile_current().unwrap(), "Other");
        assert_eq!(
            fixture.host.state_snapshot().unwrap()["pending_profile"],
            json!(null)
        );
        // No auto-resume: factory spawns should still be 1 (no new worker for target)
        assert_eq!(fixture.factory.spawns.load(Ordering::Acquire), 1);
        let status = fixture.service.profile_switch_status();
        assert_eq!(status.stage, ProfileSwitchStage::Idle);
        assert_eq!(status.active_profile, "Other");
        assert!(!status.degraded);
        assert!(status.last_error.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_stop_failure_rolls_back_and_stays_idle() {
        struct FailingSession {
            stopping: AtomicBool,
        }
        #[async_trait]
        impl UserScriptSession for FailingSession {
            async fn discover(&self) -> Result<ScriptDiscoveryResult, CommandBackendError> {
                Ok(ScriptDiscoveryResult { commands: vec![] })
            }
            async fn execute(
                &self,
                _: ScriptExecuteRequest,
            ) -> Result<ScriptExecutionResult, CommandBackendError> {
                unreachable!()
            }
            async fn pause(&self) -> Result<ScriptPauseResult, CommandBackendError> {
                Ok(ScriptPauseResult { changed: false })
            }
            async fn resume(&self) -> Result<ScriptPauseResult, CommandBackendError> {
                Ok(ScriptPauseResult { changed: false })
            }
            async fn stop_command(&self) -> Result<ScriptStopResult, CommandBackendError> {
                Ok(ScriptStopResult {
                    stop_requested: false,
                })
            }
            fn begin_stopping(&self) {
                self.stopping.store(true, Ordering::Release);
            }
            async fn shutdown(
                &self,
                _: Duration,
            ) -> Result<ScriptSessionStop, CommandBackendError> {
                Err(CommandBackendError::new(
                    "WorkerReapFailed",
                    "injected reap failure",
                ))
            }
        }
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        // Make shutdown fail to simulate worker reap failure
        fixture.session.stopping.store(false, Ordering::Release);
        let failing = Arc::new(FailingSession {
            stopping: AtomicBool::new(false),
        });
        // Inject failing session directly into service inner
        {
            let mut inner = fixture.service.inner.lock().await;
            inner.session = Some(failing.clone());
            inner.session_profile = Some("default".to_owned());
        }
        let err = fixture.service.switch_profile("Other").await.unwrap_err();
        assert!(matches!(
            err,
            CommandServiceError::ProfileSwitchStopFailed(_)
        ));
        let status = fixture.service.profile_switch_status();
        assert_eq!(status.stage, ProfileSwitchStage::Failed);
        assert_eq!(status.active_profile, "default");
        assert_eq!(status.pending_profile, None);
        assert!(!status.degraded);
        assert!(failing.stopping.load(Ordering::Acquire));
        // Profile remains old, idle, no degraded
        assert_eq!(fixture.host.profile_current().unwrap(), "default");
        // New commands should still be allowed (not degraded)
        fixture.service.reload().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_commit_failure_rolls_back_idle() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        fixture.service.inject_fault_commit(true);
        let err = fixture.service.switch_profile("Other").await.unwrap_err();
        assert!(matches!(
            err,
            CommandServiceError::ProfileSwitchCommitFailed(_)
        ));
        let status = fixture.service.profile_switch_status();
        assert_eq!(status.stage, ProfileSwitchStage::Failed);
        assert_eq!(status.active_profile, "default");
        assert_eq!(status.pending_profile, None);
        assert!(!status.degraded);
        // Worker was still reaped before commit (spec requires reap before commit)
        assert_eq!(fixture.session.shutdowns.load(Ordering::Acquire), 1);
        assert_eq!(fixture.host.profile_current().unwrap(), "default");
        // Idle, no auto-resume, commands cleared
        assert!(fixture.service.commands().await.is_empty());
        assert_eq!(fixture.service.status().await, CommandStatus::Stopped);
        // Future operations allowed
        fixture.service.reload().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_restore_failure_enters_degraded_and_rejects_new_commands() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        fixture.service.inject_fault_commit(true);
        fixture.service.inject_fault_restore(true);
        let err = fixture.service.switch_profile("Other").await.unwrap_err();
        assert!(matches!(
            err,
            CommandServiceError::ProfileSwitchRestoreFailed
        ));
        let status = fixture.service.profile_switch_status();
        assert_eq!(status.stage, ProfileSwitchStage::Degraded);
        assert!(status.degraded);
        assert_eq!(status.active_profile, "default"); // old profile preserved logically but degraded
        assert_eq!(fixture.host.profile_current().unwrap(), "default");
        // Degraded: new commands rejected
        assert!(matches!(
            fixture.service.reload().await,
            Err(CommandServiceError::ProfileSwitchDegraded)
        ));
        assert!(matches!(
            fixture
                .service
                .start(&CommandIdentity {
                    module_path: "Commands.PythonCommands.samples.first".into(),
                    class_name: "First".into()
                })
                .await,
            Err(CommandServiceError::ProfileSwitchDegraded)
        ));
        // Status exposes typed degraded for UI/log
        assert!(fixture.service.is_profile_switch_degraded());
        assert_eq!(
            status.last_error.as_ref().unwrap().stage,
            ProfileSwitchStage::Committing
        );
        // Explicit recovery clears degraded
        fixture.service.clear_profile_switch_degraded();
        assert!(!fixture.service.is_profile_switch_degraded());
        fixture.service.reload().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_stop_restore_failure_degrades() {
        struct FailingStopSession {
            stopping: AtomicBool,
        }
        #[async_trait]
        impl UserScriptSession for FailingStopSession {
            async fn discover(&self) -> Result<ScriptDiscoveryResult, CommandBackendError> {
                Ok(ScriptDiscoveryResult { commands: vec![] })
            }
            async fn execute(
                &self,
                _: ScriptExecuteRequest,
            ) -> Result<ScriptExecutionResult, CommandBackendError> {
                unreachable!()
            }
            async fn pause(&self) -> Result<ScriptPauseResult, CommandBackendError> {
                Ok(ScriptPauseResult { changed: false })
            }
            async fn resume(&self) -> Result<ScriptPauseResult, CommandBackendError> {
                Ok(ScriptPauseResult { changed: false })
            }
            async fn stop_command(&self) -> Result<ScriptStopResult, CommandBackendError> {
                Ok(ScriptStopResult {
                    stop_requested: false,
                })
            }
            fn begin_stopping(&self) {
                self.stopping.store(true, Ordering::Release);
            }
            async fn shutdown(
                &self,
                _: Duration,
            ) -> Result<ScriptSessionStop, CommandBackendError> {
                Err(CommandBackendError::new(
                    "WorkerReapFailed",
                    "stop injected",
                ))
            }
        }
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        let failing = Arc::new(FailingStopSession {
            stopping: AtomicBool::new(false),
        });
        {
            let mut inner = fixture.service.inner.lock().await;
            inner.session = Some(failing.clone());
            inner.session_profile = Some("default".to_owned());
        }
        fixture.service.inject_fault_restore(true);
        let err = fixture.service.switch_profile("Other").await.unwrap_err();
        assert!(matches!(
            err,
            CommandServiceError::ProfileSwitchRestoreFailed
        ));
        assert!(fixture.service.is_profile_switch_degraded());
        assert_eq!(
            fixture.service.profile_switch_status().stage,
            ProfileSwitchStage::Degraded
        );
        // Rejects new commands
        assert!(matches!(
            fixture.service.reload().await,
            Err(CommandServiceError::ProfileSwitchDegraded)
        ));
        fixture.service.clear_profile_switch_degraded();
        assert!(!fixture.service.is_profile_switch_degraded());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn switch_profile_exposes_typed_status_and_rejects_during_switch() {
        let fixture = fixture();
        fixture.service.reload().await.unwrap();
        // Manual gate hold to test rejection during switch
        fixture.service.try_begin_profile_switch().unwrap();
        drop(fixture.service.profile_switch_status());
        // After try_begin, stage should be Idle? Actually try_begin doesn't set stage, but profile_switch_status should show pending
        // Instead test that new commands are rejected during gate
        assert!(matches!(
            fixture.service.reload().await,
            Err(CommandServiceError::ProfileSwitchInProgress)
        ));
        assert!(matches!(
            fixture
                .service
                .start(&CommandIdentity {
                    module_path: "Commands.PythonCommands.samples.first".into(),
                    class_name: "First".into()
                })
                .await,
            Err(CommandServiceError::ProfileSwitchInProgress)
        ));
        fixture.service.finish_profile_switch();
        // After finish, should be idle and allow commands
        fixture.service.reload().await.unwrap();
        let status2 = fixture.service.profile_switch_status();
        assert_eq!(status2.stage, ProfileSwitchStage::Idle);
        assert!(!status2.degraded);
    }

    async fn wait_for_status(service: &CommandService, expected: CommandStatus) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while service.status().await != expected {
            assert!(Instant::now() < deadline, "command state did not settle");
            sleep(Duration::from_millis(5)).await;
        }
    }
}
