//! Rust-main-owned host state used while the persistent dynamic worker runs
//! its startup configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use crate::device::{
    ApplyResult, InputArbiter, InputEvent, InputGeneration, InputPriority, InputSequence,
    InputSnapshot, InputSourceId, InputSourceKind, MouseButtons,
};
use crate::device::{ControllerState, ControllerUpdate};
use crate::settings::lock::LockManager;
use crate::settings::persistence::TomlStore;
use crate::settings::pipeline::{LoadedSettings, PipelineError, PipelineRequest, SettingsPipeline};
use crate::settings::roots::SafeComponent;
use async_trait::async_trait;
use parking_lot::Mutex;
use pokecon_dynamic::protocol::{HostProfileSwitchBeginResult, HostProfileSwitchCommitResult};
use pokecon_dynamic::{
    CommandDisplayCache, CommandInfo, Diagnostic, DiagnosticLevel, DynamicHost, DynamicHostError,
    merge_state_change,
};
use pokecon_worker::ipc::ResourceSafety;
use serde_json::Value;
use tokio::sync::watch;

use crate::command_service::{CommandService, CommandServiceError};
use crate::contracts::model::Scope;

const DYNAMIC_SOURCE: &str = "dynamic-config";
const DYNAMIC_GENERATION: &str = "dynamic-1";
const RUNTIME_STATE_FIELDS: &[&str] = &[
    "serial_connected",
    "camera_opened",
    "is_running",
    "command_state",
    "current_command",
    "command_candidates",
    "tags",
    "command_display_lists",
    "command_display_cache_loading",
    "pending_profile",
    "last_input",
];

/// Controller ownership for the dynamic worker. The same object is passed to
/// the IPC connection as its transport-loss safety boundary.
#[derive(Debug)]
pub struct DynamicControllerSafety {
    arbiter: Arc<Mutex<InputArbiter>>,
    source: InputSourceId,
    generation: InputGeneration,
    sequence: AtomicU64,
    accepting: AtomicBool,
}

impl DynamicControllerSafety {
    fn new() -> Self {
        let source =
            InputSourceId::new(DYNAMIC_SOURCE).expect("the static dynamic input source is valid");
        let generation = InputGeneration::new(DYNAMIC_GENERATION)
            .expect("the static dynamic generation is valid");
        let mut arbiter = InputArbiter::default();
        initialize_dynamic_source(&mut arbiter, &source, &generation)
            .expect("the static neutral snapshot is valid");
        Self {
            arbiter: Arc::new(Mutex::new(arbiter)),
            source,
            generation,
            sequence: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
        }
    }

    fn update(&self, update: ControllerUpdate) -> Result<(), DynamicHostError> {
        let mut arbiter = self.arbiter.lock();
        if !self.accepting.load(Ordering::Acquire) {
            return Err(host_stopping());
        }
        let sequence = self
            .sequence
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                DynamicHostError::new(
                    "InputSequenceExhausted",
                    "dynamic controller input sequence is exhausted",
                )
            })?
            .saturating_add(1);
        let result = arbiter
            .apply_event(
                &self.source,
                &self.generation,
                InputSequence::new(sequence.to_string()).map_err(|error| input_error(&error))?,
                InputEvent::ControllerUpdate(update),
            )
            .map_err(|error| input_error(&error))?;
        if result != ApplyResult::Applied {
            return Err(DynamicHostError::new(
                "ControllerGenerationUnavailable",
                "dynamic controller generation no longer accepts input",
            ));
        }
        Ok(())
    }

    fn reset(&self) -> Result<(), DynamicHostError> {
        let mut arbiter = self.arbiter.lock();
        if !self.accepting.load(Ordering::Acquire) {
            return Err(host_stopping());
        }
        initialize_dynamic_source(&mut arbiter, &self.source, &self.generation)
            .map_err(|error| input_error(&error))?;
        self.sequence.store(0, Ordering::Release);
        Ok(())
    }

    /// Returns the authoritative merged controller output.
    #[must_use]
    pub fn state(&self) -> ControllerState {
        self.arbiter.lock().output()
    }

    /// Returns the one process-wide input arbiter. Browser, hardware,
    /// user-script, and dynamic-config sources must all register here so
    /// ownership union and continuous-input priorities are evaluated once.
    #[must_use]
    pub fn arbiter(&self) -> Arc<Mutex<InputArbiter>> {
        Arc::clone(&self.arbiter)
    }
}

impl ResourceSafety for DynamicControllerSafety {
    fn force_release(&self) {
        self.accepting.store(false, Ordering::Release);
        self.arbiter.lock().disconnect_source(&self.source);
    }
}

fn initialize_dynamic_source(
    arbiter: &mut InputArbiter,
    source: &InputSourceId,
    generation: &InputGeneration,
) -> Result<(), crate::device::InputError> {
    arbiter.begin_generation(
        source.clone(),
        InputSourceKind::DynamicConfig,
        InputPriority::DYNAMIC_CONFIG,
        generation.clone(),
    );
    let neutral = ControllerState::NEUTRAL;
    let (result, acknowledgement) = arbiter.apply_snapshot(
        source,
        InputSnapshot {
            generation: generation.clone(),
            sequence: InputSequence::zero(),
            keyboard_keys: Vec::new(),
            mouse_buttons: MouseButtons::default(),
            buttons: neutral.buttons,
            hat: neutral.hat,
            left_stick: neutral.left_stick,
            right_stick: neutral.right_stick,
            touch: neutral.touch,
        },
    )?;
    if result != ApplyResult::Applied || acknowledgement.is_none() {
        return Err(crate::device::InputError::UnknownSource);
    }
    Ok(())
}

fn input_error(error: &crate::device::InputError) -> DynamicHostError {
    DynamicHostError::new("InvalidControllerUpdate", error.to_string())
}

#[derive(Debug)]
struct StartupHostState {
    request: PipelineRequest,
    loaded: LoadedSettings,
    dynamic_values: BTreeMap<String, Value>,
    runtime_dynamic_values: BTreeMap<String, Value>,
    public_state: BTreeMap<String, Value>,
    diagnostics: Vec<Diagnostic>,
    outputs: Vec<String>,
    command_recompute_requests: u64,
    script_load: Option<ScriptLoadStage>,
    command_cache_published: bool,
    prepared_profile: Option<PreparedProfileSwitch>,
    startup_complete: bool,
    stopping: bool,
}

#[derive(Clone, Debug)]
struct ScriptLoadStage {
    command_candidates: Vec<CommandInfo>,
    tags: Vec<String>,
}

#[derive(Clone, Debug)]
struct PreparedProfileSwitch {
    target: String,
    loaded: LoadedSettings,
    dynamic_values: Option<BTreeMap<String, Value>>,
    runtime_dynamic_values: BTreeMap<String, Value>,
    public_state: BTreeMap<String, Value>,
}

struct ResolvedPreparedProfile {
    loaded: LoadedSettings,
    dynamic_values: Option<BTreeMap<String, Value>>,
    runtime_dynamic_values: BTreeMap<String, Value>,
}

/// Dynamic host used from worker creation through top-level config commit.
///
/// Every assignment is re-resolved through the canonical settings pipeline,
/// but ordinary CLI values remain deferred until [`Self::finish_startup`].
/// No assignment is persisted to TOML.
#[derive(Debug)]
pub struct StartupDynamicHost {
    inner: Mutex<StartupHostState>,
    controller: Arc<DynamicControllerSafety>,
    command_recompute: watch::Sender<u64>,
    runtime_changes: watch::Sender<u64>,
    profile_switching: AtomicBool,
    command_service: OnceLock<Weak<CommandService>>,
}

impl StartupDynamicHost {
    /// Creates a host from the snapshot produced by
    /// [`SettingsPipeline::load_before_dynamic`].
    ///
    /// # Errors
    ///
    /// Returns an error if the profile directory cannot be enumerated or a
    /// required canonical startup value is missing.
    pub fn new(request: PipelineRequest, loaded: LoadedSettings) -> Result<Self, DynamicHostError> {
        let public_state = startup_state(&loaded)?;
        let (command_recompute, _receiver) = watch::channel(0);
        let (runtime_changes, _receiver) = watch::channel(0);
        Ok(Self {
            inner: Mutex::new(StartupHostState {
                dynamic_values: request.dynamic_values.clone(),
                runtime_dynamic_values: BTreeMap::new(),
                request,
                loaded,
                public_state,
                diagnostics: Vec::new(),
                outputs: Vec::new(),
                command_recompute_requests: 0,
                script_load: None,
                command_cache_published: false,
                prepared_profile: None,
                startup_complete: false,
                stopping: false,
            }),
            controller: Arc::new(DynamicControllerSafety::new()),
            command_recompute,
            runtime_changes,
            profile_switching: AtomicBool::new(false),
            command_service: OnceLock::new(),
        })
    }

    /// Applies the final ordinary CLI layer and changes subsequent dynamic
    /// assignments into runtime overlays that follow CLI precedence.
    ///
    /// # Errors
    ///
    /// Returns a canonical final-pipeline or state projection error.
    pub fn finish_startup(&self) -> Result<LoadedSettings, DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        let mut request = inner.request.clone();
        request.dynamic_values.clone_from(&inner.dynamic_values);
        let loaded = SettingsPipeline::new(request)
            .load()
            .map_err(|error| pipeline_error(&error))?;
        let public_state = refreshed_state(&loaded, Some(&inner.public_state))?;
        inner.loaded = loaded.clone();
        inner.public_state = public_state;
        inner.startup_complete = true;
        drop(inner);
        self.notify_runtime_change();
        Ok(loaded)
    }

    /// Returns the complete committed top-level assignment map for the final
    /// pipeline and subsequent runtime-host construction.
    #[cfg(test)]
    #[must_use]
    pub fn dynamic_values(&self) -> BTreeMap<String, Value> {
        self.inner.lock().dynamic_values.clone()
    }

    /// Returns the current immutable settings snapshot used to prepare a
    /// profile-scoped user-script worker.
    #[must_use]
    pub fn loaded_settings(&self) -> LoadedSettings {
        self.inner.lock().loaded.clone()
    }

    /// Returns the transport-loss safety object for `WorkerSupervisor::spawn`.
    #[must_use]
    pub fn controller_safety(&self) -> Arc<DynamicControllerSafety> {
        Arc::clone(&self.controller)
    }

    /// Publishes a controller change made by another source registered in the
    /// shared arbiter.
    pub fn notify_controller_change(&self) {
        self.notify_runtime_change();
    }

    #[cfg(test)]
    #[must_use]
    pub fn command_recompute_requests(&self) -> u64 {
        self.inner.lock().command_recompute_requests
    }

    /// Subscribes to coalesced command-cache invalidations requested by the
    /// persistent dynamic worker.
    #[must_use]
    pub fn subscribe_command_recompute(&self) -> watch::Receiver<u64> {
        self.command_recompute.subscribe()
    }

    /// Subscribes to coalesced changes of the committed settings/state view.
    /// Staged command-cache values are deliberately excluded.
    #[must_use]
    pub fn subscribe_runtime_changes(&self) -> watch::Receiver<u64> {
        self.runtime_changes.subscribe()
    }

    /// Returns only the committed UI-visible state. Dynamic callback staging
    /// remains private until a complete command-cache generation is published.
    #[must_use]
    pub fn public_state_snapshot(&self) -> BTreeMap<String, Value> {
        let mut snapshot = self.inner.lock().public_state.clone();
        snapshot.insert(
            "holding_buttons".to_owned(),
            holding_buttons(self.controller.state()),
        );
        snapshot
    }

    /// Re-enumerates profile directories and publishes the resulting list to
    /// the committed runtime state.
    ///
    /// # Errors
    ///
    /// Returns an error if the effective profile directory cannot be read.
    pub fn refresh_available_profiles(&self) -> Result<(), DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        let profiles = serde_json::to_value(list_profiles(&inner.loaded)?).map_err(|_| {
            DynamicHostError::new("StateEncodingFailed", "profile list cannot be encoded")
        })?;
        inner
            .public_state
            .insert("available_profiles".to_owned(), profiles);
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    fn notify_runtime_change(&self) {
        self.runtime_changes
            .send_modify(|generation| *generation = generation.saturating_add(1));
    }

    /// Starts an isolated `ScriptLoadPre` staging generation. Dynamic state
    /// reads and writes see this generation while the last completed UI cache
    /// remains unchanged.
    ///
    /// # Errors
    ///
    /// Rejects overlapping loads or a stopping host.
    pub fn begin_script_load(&self, candidates: Vec<CommandInfo>) -> Result<(), DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        if inner.script_load.is_some() {
            return Err(DynamicHostError::new(
                "ScriptLoadBusy",
                "another command load generation is already active",
            ));
        }
        let initial_load = !inner.command_cache_published;
        inner.public_state.insert(
            "command_display_cache_loading".to_owned(),
            Value::Bool(initial_load),
        );
        inner.script_load = Some(ScriptLoadStage {
            command_candidates: candidates,
            tags: Vec::new(),
        });
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    /// Returns the mutable command metadata produced by `ScriptLoadPre`.
    ///
    /// # Errors
    ///
    /// Returns an error outside an active script-load generation.
    pub fn staged_script_load(&self) -> Result<(Vec<CommandInfo>, Vec<String>), DynamicHostError> {
        let inner = self.inner.lock();
        let stage = inner.script_load.as_ref().ok_or_else(|| {
            DynamicHostError::new("NoScriptLoad", "no command load generation is active")
        })?;
        Ok((stage.command_candidates.clone(), stage.tags.clone()))
    }

    /// Replaces the staged metadata after automatic, manual, and dynamic tags
    /// have been reconciled and before `ScriptLoadPost` is emitted.
    ///
    /// # Errors
    ///
    /// Returns an error outside an active script-load generation.
    pub fn set_staged_script_load(
        &self,
        command_candidates: Vec<CommandInfo>,
        tags: Vec<String>,
    ) -> Result<(), DynamicHostError> {
        let mut inner = self.inner.lock();
        let stage = inner.script_load.as_mut().ok_or_else(|| {
            DynamicHostError::new("NoScriptLoad", "no command load generation is active")
        })?;
        stage.command_candidates = command_candidates;
        stage.tags = unique_strings(tags);
        Ok(())
    }

    /// Discards an unfinished script-load generation and restores the previous
    /// completed cache without exposing staged values.
    pub fn cancel_script_load(&self) {
        let mut inner = self.inner.lock();
        inner.script_load = None;
        inner.public_state.insert(
            "command_display_cache_loading".to_owned(),
            Value::Bool(false),
        );
        drop(inner);
        self.notify_runtime_change();
    }

    /// Atomically publishes all command-visible fields from one completed
    /// cache generation.
    ///
    /// # Errors
    ///
    /// Rejects a cache that does not correspond to the active staged metadata.
    pub fn publish_command_cache(
        &self,
        cache: &CommandDisplayCache,
    ) -> Result<(), DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        let stage = inner.script_load.as_ref().ok_or_else(|| {
            DynamicHostError::new("NoScriptLoad", "no command load generation is active")
        })?;
        if stage.command_candidates != cache.candidates || stage.tags != cache.tags {
            return Err(DynamicHostError::new(
                "CommandCacheGenerationMismatch",
                "completed command cache does not match its staged generation",
            ));
        }
        let candidates = serde_json::to_value(&cache.candidates)
            .map_err(|_| state_encoding_failed("command candidates"))?;
        let tags =
            serde_json::to_value(&cache.tags).map_err(|_| state_encoding_failed("command tags"))?;
        let display_lists = serde_json::to_value(&cache.display_lists)
            .map_err(|_| state_encoding_failed("command display lists"))?;
        inner
            .public_state
            .insert("command_candidates".to_owned(), candidates);
        inner.public_state.insert("tags".to_owned(), tags);
        inner
            .public_state
            .insert("command_display_lists".to_owned(), display_lists);
        inner.command_cache_published = true;
        inner.public_state.insert(
            "command_display_cache_loading".to_owned(),
            Value::Bool(false),
        );
        inner.script_load = None;
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    /// Commits one command execution-state transition.
    ///
    /// # Errors
    ///
    /// Rejects unknown state names or a stopping host.
    pub fn set_command_status(
        &self,
        state: &str,
        current_command: &str,
    ) -> Result<(), DynamicHostError> {
        if !matches!(state, "running" | "paused" | "stopped" | "error") {
            return Err(DynamicHostError::new(
                "InvalidCommandState",
                "command state must be running, paused, stopped, or error",
            ));
        }
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        inner.public_state.insert(
            "is_running".to_owned(),
            Value::Bool(matches!(state, "running" | "paused")),
        );
        inner
            .public_state
            .insert("command_state".to_owned(), Value::String(state.to_owned()));
        inner.public_state.insert(
            "current_command".to_owned(),
            Value::String(current_command.to_owned()),
        );
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    /// Removes every profile-sensitive command value after the old worker has
    /// entered stopping. The operation is one host-state commit.
    ///
    /// # Errors
    ///
    /// Returns an error if the host is already stopping for application exit.
    pub fn clear_command_generation(&self) -> Result<(), DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        clear_command_generation_state(&mut inner);
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    /// Removes Rust-owned command state during application shutdown after the
    /// dynamic host has intentionally closed every external mutation boundary.
    pub(crate) fn clear_command_generation_for_shutdown(&self) {
        let mut inner = self.inner.lock();
        clear_command_generation_state(&mut inner);
        drop(inner);
        self.notify_runtime_change();
    }

    /// Fully resolves one target profile without changing active settings,
    /// then exposes only `pending_profile` for the Pre-event phase.
    ///
    /// # Errors
    ///
    /// Rejects unsafe/missing profiles, invalid target TOML, overlapping
    /// preparations, or a stopping host.
    pub fn prepare_profile_switch(
        &self,
        name: &str,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        if inner.prepared_profile.is_some() {
            return Err(DynamicHostError::new(
                "ProfileSwitchBusy",
                "another profile switch is already prepared",
            ));
        }
        validate_existing_profile(&inner.loaded, name)?;
        if changes.contains_key("active_profile") {
            return Err(DynamicHostError::new(
                "InvalidProfileSwitch",
                "profile switch changes must not contain active_profile",
            ));
        }
        let ResolvedPreparedProfile {
            loaded,
            dynamic_values,
            runtime_dynamic_values,
        } = resolve_prepared_profile(&inner, name, changes)?;
        let public_state = refreshed_state(&loaded, Some(&inner.public_state))?;
        inner
            .public_state
            .insert("pending_profile".to_owned(), Value::String(name.to_owned()));
        let settings = loaded_settings_values(&loaded);
        inner.prepared_profile = Some(PreparedProfileSwitch {
            target: name.to_owned(),
            loaded,
            dynamic_values,
            runtime_dynamic_values,
            public_state,
        });
        drop(inner);
        self.notify_runtime_change();
        Ok(settings)
    }

    /// Discards a prepared target and clears `pending_profile` while leaving
    /// the active settings snapshot unchanged.
    pub fn cancel_profile_switch(&self) {
        let mut inner = self.inner.lock();
        inner.prepared_profile = None;
        inner
            .public_state
            .insert("pending_profile".to_owned(), Value::Null);
        drop(inner);
        self.notify_runtime_change();
    }

    /// Commits a previously validated target after the old worker has been
    /// reaped. The commit itself performs no filesystem parsing.
    ///
    /// # Errors
    ///
    /// Returns an error when no target is prepared or the host is stopping.
    pub fn commit_profile_switch(&self) -> Result<LoadedSettings, DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        let prepared = inner.prepared_profile.take().ok_or_else(|| {
            DynamicHostError::new("NoProfileSwitch", "no profile switch is prepared")
        })?;
        persist_active_profile(&inner.loaded, &prepared.target)?;
        let mut public_state = prepared.public_state;
        for name in RUNTIME_STATE_FIELDS {
            if let Some(value) = inner.public_state.get(*name) {
                public_state.insert((*name).to_owned(), value.clone());
            }
        }
        public_state.insert("active_profile".to_owned(), Value::String(prepared.target));
        public_state.insert("pending_profile".to_owned(), Value::Null);
        inner.loaded = prepared.loaded;
        if let Some(dynamic_values) = prepared.dynamic_values {
            inner.dynamic_values = dynamic_values;
        }
        inner.runtime_dynamic_values = prepared.runtime_dynamic_values;
        inner.public_state = public_state;
        let loaded = inner.loaded.clone();
        drop(inner);
        self.notify_runtime_change();
        Ok(loaded)
    }

    /// Closes every mutating host boundary and immediately releases dynamic
    /// controller ownership before worker shutdown begins.
    pub fn begin_stopping(&self) {
        let mut inner = self.inner.lock();
        inner.stopping = true;
        inner.prepared_profile = None;
        inner
            .public_state
            .insert("pending_profile".to_owned(), Value::Null);
        drop(inner);
        self.finish_profile_switch_gate();
        self.controller.force_release();
        self.notify_runtime_change();
    }

    pub(crate) fn try_begin_profile_switch_gate(&self) -> Result<(), DynamicHostError> {
        self.profile_switching
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                DynamicHostError::new(
                    "ProfileSwitchBusy",
                    "another profile switch is already in progress",
                )
            })
    }

    pub(crate) fn profile_switch_in_progress(&self) -> bool {
        self.profile_switching.load(Ordering::Acquire)
    }

    pub(crate) fn finish_profile_switch_gate(&self) {
        self.profile_switching.store(false, Ordering::Release);
    }

    pub(crate) fn hold_profile_switch_gate(&self) {
        self.profile_switching.store(true, Ordering::Release);
    }

    pub(crate) fn bind_command_service(&self, commands: &Arc<CommandService>) {
        let _already_bound = self.command_service.set(Arc::downgrade(commands));
    }

    fn current_settings(inner: &StartupHostState) -> BTreeMap<String, Value> {
        loaded_settings_values(&inner.loaded)
    }

    fn apply_startup_settings(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        if inner.startup_complete {
            return Self::apply_runtime_settings(&mut inner, changes);
        }
        if let Some(profile) = changes.get("active_profile") {
            let profile = profile.as_str().ok_or_else(|| {
                DynamicHostError::new("InvalidProfile", "active profile must be a string")
            })?;
            validate_existing_profile(&inner.loaded, profile)?;
        }
        let mut dynamic_values = inner.dynamic_values.clone();
        dynamic_values.extend(changes.clone());
        let mut request = inner.request.clone();
        request.dynamic_values.clone_from(&dynamic_values);
        let loaded = SettingsPipeline::new(request)
            .load_through_dynamic()
            .map_err(|error| pipeline_error(&error))?;
        let public_state = refreshed_state(&loaded, Some(&inner.public_state))?;
        inner.dynamic_values = dynamic_values;
        inner.loaded = loaded;
        inner.public_state = public_state;
        Ok(Self::current_settings(&inner))
    }

    fn apply_runtime_settings(
        inner: &mut StartupHostState,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let target_profile = changes
            .get("active_profile")
            .and_then(Value::as_str)
            .filter(|target| *target != inner.loaded.active_profile.as_str());
        let (loaded, runtime_dynamic_values) = if let Some(target) = target_profile {
            let (mut loaded, mut retained) = resolve_runtime_profile(inner, target)?;
            retained.extend(changes.clone());
            loaded = loaded
                .with_runtime_dynamic_changes(&retained)
                .map_err(|error| pipeline_error(&error))?;
            (loaded, retained)
        } else {
            let loaded = inner
                .loaded
                .with_runtime_dynamic_changes(changes)
                .map_err(|error| pipeline_error(&error))?;
            let mut runtime_dynamic_values = inner.runtime_dynamic_values.clone();
            runtime_dynamic_values.extend(changes.clone());
            (loaded, runtime_dynamic_values)
        };
        let public_state = refreshed_state(&loaded, Some(&inner.public_state))?;
        inner.loaded = loaded;
        inner.runtime_dynamic_values = runtime_dynamic_values;
        inner.public_state = public_state;
        Ok(Self::current_settings(inner))
    }
}

fn loaded_settings_values(loaded: &LoadedSettings) -> BTreeMap<String, Value> {
    loaded
        .settings
        .values()
        .iter()
        .map(|(id, resolved)| (id.clone(), resolved.value.clone()))
        .collect()
}

#[async_trait]
impl DynamicHost for StartupDynamicHost {
    fn settings_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        Ok(Self::current_settings(&self.inner.lock()))
    }

    fn apply_settings(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let settings = self.apply_startup_settings(changes)?;
        self.notify_runtime_change();
        Ok(settings)
    }

    fn state_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let inner = self.inner.lock();
        let mut snapshot = inner.public_state.clone();
        if let Some(stage) = &inner.script_load {
            snapshot.insert(
                "command_candidates".to_owned(),
                serde_json::to_value(&stage.command_candidates)
                    .map_err(|_| state_encoding_failed("command candidates"))?,
            );
            snapshot.insert(
                "tags".to_owned(),
                serde_json::to_value(&stage.tags)
                    .map_err(|_| state_encoding_failed("command tags"))?,
            );
        }
        snapshot.insert(
            "holding_buttons".to_owned(),
            holding_buttons(self.controller.state()),
        );
        Ok(snapshot)
    }

    fn set_state_value(&self, name: &str, value: Value) -> Result<(), DynamicHostError> {
        let value = normalize_writable_state(name, value)?;
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        store_writable_state(&mut inner, name, value)?;
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    fn merge_state_value(
        &self,
        name: &str,
        before: Value,
        value: Value,
    ) -> Result<(), DynamicHostError> {
        ensure_writable_state_name(name)?;
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        let current = writable_state_value(&inner, name)?;
        let merged = merge_state_change(&before, &current, &value)?;
        let merged = normalize_writable_state(name, merged)?;
        store_writable_state(&mut inner, name, merged)?;
        drop(inner);
        self.notify_runtime_change();
        Ok(())
    }

    fn profile_current(&self) -> Result<String, DynamicHostError> {
        Ok(self.inner.lock().loaded.active_profile.as_str().to_owned())
    }

    fn profile_list(&self) -> Result<Vec<String>, DynamicHostError> {
        list_profiles(&self.inner.lock().loaded)
    }

    async fn profile_switch_begin(
        &self,
        name: &str,
        changes: &BTreeMap<String, Value>,
    ) -> Result<HostProfileSwitchBeginResult, DynamicHostError> {
        self.try_begin_profile_switch_gate()?;
        match self.prepare_profile_switch(name, changes) {
            Ok(settings) => Ok(HostProfileSwitchBeginResult { settings }),
            Err(error) => {
                self.finish_profile_switch_gate();
                Err(error)
            }
        }
    }

    async fn profile_switch_commit(
        &self,
    ) -> Result<HostProfileSwitchCommitResult, DynamicHostError> {
        let stop = if let Some(commands) = self
            .command_service
            .get()
            .and_then(std::sync::Weak::upgrade)
        {
            let timeout = profile_shutdown_timeout(self)?;
            commands
                .stop_for_profile_switch(timeout)
                .await
                .map_err(profile_command_error)?
        } else {
            self.clear_command_generation()?;
            None
        };
        self.commit_profile_switch()?;
        Ok(HostProfileSwitchCommitResult {
            forced_worker_stop: stop.is_some_and(|stop| stop.forced),
        })
    }

    async fn profile_switch_abort(&self) -> Result<(), DynamicHostError> {
        self.cancel_profile_switch();
        self.finish_profile_switch_gate();
        Ok(())
    }

    async fn profile_switch_end(&self) -> Result<(), DynamicHostError> {
        self.finish_profile_switch_gate();
        Ok(())
    }

    fn controller_update(&self, update: ControllerUpdate) -> Result<(), DynamicHostError> {
        ensure_running(&self.inner.lock())?;
        self.controller.update(update)?;
        self.notify_runtime_change();
        Ok(())
    }

    fn controller_reset(&self) -> Result<(), DynamicHostError> {
        ensure_running(&self.inner.lock())?;
        self.controller.reset()?;
        self.notify_runtime_change();
        Ok(())
    }

    fn record_diagnostic(&self, diagnostic: Diagnostic) {
        match diagnostic.level {
            DiagnosticLevel::Warning => tracing::warn!(
                code = diagnostic.code,
                message = diagnostic.message,
                event = diagnostic.event,
                handler_id = diagnostic.handler_id.map(pokecon_dynamic::HandlerId::get),
                "dynamic worker diagnostic"
            ),
            DiagnosticLevel::Error => tracing::error!(
                code = diagnostic.code,
                message = diagnostic.message,
                event = diagnostic.event,
                handler_id = diagnostic.handler_id.map(pokecon_dynamic::HandlerId::get),
                "dynamic worker diagnostic"
            ),
        }
        self.inner.lock().diagnostics.push(diagnostic);
    }

    fn record_output(&self, message: &str) {
        tracing::info!(target: "pokecon.dynamic.stdout", "{message}");
        self.inner.lock().outputs.push(message.to_owned());
    }

    fn request_command_recompute(&self) {
        let mut inner = self.inner.lock();
        if !inner.stopping {
            inner.command_recompute_requests = inner.command_recompute_requests.saturating_add(1);
            self.command_recompute
                .send_replace(inner.command_recompute_requests);
        }
    }
}

fn ensure_writable_state_name(name: &str) -> Result<(), DynamicHostError> {
    if matches!(name, "command_candidates" | "tags") {
        Ok(())
    } else {
        Err(DynamicHostError::new(
            "ReadOnlyState",
            format!("state property is read-only: {name}"),
        ))
    }
}

fn normalize_writable_state(name: &str, value: Value) -> Result<Value, DynamicHostError> {
    match name {
        "command_candidates" => {
            serde_json::from_value::<Vec<CommandInfo>>(value.clone()).map_err(|_| {
                DynamicHostError::new(
                    "InvalidState",
                    "command_candidates must be a list of CommandInfo objects",
                )
            })?;
            Ok(value)
        }
        "tags" => {
            let mut tags = serde_json::from_value::<Vec<String>>(value).map_err(|_| {
                DynamicHostError::new("InvalidState", "tags must be a list of strings")
            })?;
            let mut retained = BTreeSet::new();
            tags.retain(|tag| retained.insert(tag.clone()));
            serde_json::to_value(tags)
                .map_err(|_| DynamicHostError::new("StateEncodingFailed", "tags cannot be encoded"))
        }
        _ => {
            ensure_writable_state_name(name)?;
            unreachable!("writable state names are exhausted")
        }
    }
}

fn writable_state_value(inner: &StartupHostState, name: &str) -> Result<Value, DynamicHostError> {
    ensure_writable_state_name(name)?;
    if let Some(stage) = &inner.script_load {
        return match name {
            "command_candidates" => serde_json::to_value(&stage.command_candidates)
                .map_err(|_| state_encoding_failed("command candidates")),
            "tags" => {
                serde_json::to_value(&stage.tags).map_err(|_| state_encoding_failed("command tags"))
            }
            _ => unreachable!("writable state names were validated above"),
        };
    }
    inner.public_state.get(name).cloned().ok_or_else(|| {
        DynamicHostError::new("UnknownState", format!("unknown state property: {name}"))
    })
}

fn store_writable_state(
    inner: &mut StartupHostState,
    name: &str,
    value: Value,
) -> Result<(), DynamicHostError> {
    ensure_writable_state_name(name)?;
    if let Some(stage) = inner.script_load.as_mut() {
        match name {
            "command_candidates" => {
                stage.command_candidates = serde_json::from_value(value).map_err(|_| {
                    DynamicHostError::new(
                        "InvalidState",
                        "command_candidates must be a list of CommandInfo objects",
                    )
                })?;
            }
            "tags" => {
                stage.tags = serde_json::from_value(value).map_err(|_| {
                    DynamicHostError::new("InvalidState", "tags must be a list of strings")
                })?;
            }
            _ => unreachable!("writable state names were validated above"),
        }
    } else {
        inner.public_state.insert(name.to_owned(), value);
    }
    Ok(())
}

fn clear_command_generation_state(inner: &mut StartupHostState) {
    inner.script_load = None;
    inner
        .public_state
        .insert("is_running".to_owned(), Value::Bool(false));
    inner.public_state.insert(
        "command_state".to_owned(),
        Value::String("stopped".to_owned()),
    );
    inner
        .public_state
        .insert("current_command".to_owned(), Value::String(String::new()));
    inner
        .public_state
        .insert("command_candidates".to_owned(), Value::Array(Vec::new()));
    inner
        .public_state
        .insert("tags".to_owned(), Value::Array(Vec::new()));
    inner.public_state.insert(
        "command_display_lists".to_owned(),
        serde_json::json!({"-": []}),
    );
    inner.command_cache_published = false;
    inner.public_state.insert(
        "command_display_cache_loading".to_owned(),
        Value::Bool(false),
    );
}

fn ensure_running(inner: &StartupHostState) -> Result<(), DynamicHostError> {
    if inner.stopping {
        Err(host_stopping())
    } else {
        Ok(())
    }
}

fn host_stopping() -> DynamicHostError {
    DynamicHostError::new(
        "HostStopping",
        "dynamic host no longer accepts mutating operations",
    )
}

fn pipeline_error(error: &PipelineError) -> DynamicHostError {
    DynamicHostError::new("InvalidSetting", error.to_string())
}

fn profile_shutdown_timeout(host: &StartupDynamicHost) -> Result<Duration, DynamicHostError> {
    let timeout = host
        .loaded_settings()
        .settings
        .integer("python.script.shutdown_timeout_ms")
        .map_err(|error| pipeline_error(&error))?;
    let milliseconds = u64::try_from(timeout).map_err(|_| {
        DynamicHostError::new(
            "InvalidSetting",
            "python.script.shutdown_timeout_ms is outside the u64 range",
        )
    })?;
    Ok(Duration::from_millis(milliseconds))
}

fn profile_command_error(error: CommandServiceError) -> DynamicHostError {
    match error {
        CommandServiceError::Backend(error) => DynamicHostError::new(error.code, error.message),
        CommandServiceError::Host(error) => error,
        error => DynamicHostError::new("UserWorkerStopFailed", error.to_string()),
    }
}

fn resolve_prepared_profile(
    inner: &StartupHostState,
    target: &str,
    changes: &BTreeMap<String, Value>,
) -> Result<ResolvedPreparedProfile, DynamicHostError> {
    if inner.startup_complete {
        let (mut loaded, mut runtime_dynamic_values) = resolve_runtime_profile(inner, target)?;
        runtime_dynamic_values.extend(changes.clone());
        loaded = loaded
            .with_runtime_dynamic_changes(&runtime_dynamic_values)
            .map_err(|error| pipeline_error(&error))?;
        return Ok(ResolvedPreparedProfile {
            loaded,
            dynamic_values: None,
            runtime_dynamic_values,
        });
    }

    let mut dynamic_values = inner.dynamic_values.clone();
    dynamic_values.extend(changes.clone());
    dynamic_values.insert(
        "active_profile".to_owned(),
        Value::String(target.to_owned()),
    );
    let mut request = inner.request.clone();
    request.dynamic_values.clone_from(&dynamic_values);
    let loaded = SettingsPipeline::new(request)
        .load_through_dynamic()
        .map_err(|error| pipeline_error(&error))?;
    Ok(ResolvedPreparedProfile {
        loaded,
        dynamic_values: Some(dynamic_values),
        runtime_dynamic_values: inner.runtime_dynamic_values.clone(),
    })
}

fn resolve_runtime_profile(
    inner: &StartupHostState,
    target: &str,
) -> Result<(LoadedSettings, BTreeMap<String, Value>), DynamicHostError> {
    let mut retained = inner.runtime_dynamic_values.clone();
    retained.retain(|id, _value| {
        id != "active_profile"
            && inner
                .loaded
                .settings
                .registry()
                .settings
                .iter()
                .find(|setting| setting.id == *id)
                .is_some_and(|setting| setting.scope == Scope::Global)
    });
    retained.insert(
        "active_profile".to_owned(),
        Value::String(target.to_owned()),
    );
    let switched = inner
        .loaded
        .switch_profile_in_memory(target)
        .map_err(|error| pipeline_error(&error))?;
    let loaded = switched
        .with_runtime_dynamic_changes(&retained)
        .map_err(|error| pipeline_error(&error))?;
    Ok((loaded, retained))
}

fn state_encoding_failed(subject: &str) -> DynamicHostError {
    DynamicHostError::new(
        "StateEncodingFailed",
        format!("{subject} cannot be encoded"),
    )
}

fn unique_strings(mut values: Vec<String>) -> Vec<String> {
    let mut retained = BTreeSet::new();
    values.retain(|value| retained.insert(value.clone()));
    values
}

fn validate_existing_profile(loaded: &LoadedSettings, name: &str) -> Result<(), DynamicHostError> {
    let name = SafeComponent::new(name)
        .map_err(|_| DynamicHostError::new("InvalidProfile", "invalid profile name"))?;
    let profile = loaded.roots.config.join("profiles").join(name.as_str());
    if !profile.is_dir() {
        return Err(DynamicHostError::new(
            "ProfileNotFound",
            "profile directory does not exist",
        ));
    }
    Ok(())
}

fn persist_active_profile(loaded: &LoadedSettings, target: &str) -> Result<(), DynamicHostError> {
    let setting = loaded
        .settings
        .registry()
        .settings
        .iter()
        .find(|setting| setting.id == "active_profile")
        .ok_or_else(|| {
            DynamicHostError::new(
                "MissingSetting",
                "active_profile is missing from the canonical registry",
            )
        })?;
    let toml_path = setting.surfaces.toml.name.clone().ok_or_else(|| {
        DynamicHostError::new(
            "ProfilePersistenceFailed",
            "active_profile has no global TOML projection",
        )
    })?;
    TomlStore::new(LockManager::new(&loaded.roots))
        .update(
            &loaded.global_settings_path,
            &[(toml_path, Value::String(target.to_owned()))],
        )
        .map(|_document| ())
        .map_err(|_| {
            DynamicHostError::new(
                "ProfilePersistenceFailed",
                "active profile could not be saved",
            )
        })
}

fn list_profiles(loaded: &LoadedSettings) -> Result<Vec<String>, DynamicHostError> {
    let directory = loaded.roots.config.join("profiles");
    let entries = fs::read_dir(&directory).map_err(|error| {
        DynamicHostError::new(
            "ProfileListFailed",
            format!("profile directory cannot be read: {error}"),
        )
    })?;
    let mut profiles = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            DynamicHostError::new(
                "ProfileListFailed",
                format!("profile entry cannot be read: {error}"),
            )
        })?;
        if !entry
            .file_type()
            .map_err(|error| {
                DynamicHostError::new(
                    "ProfileListFailed",
                    format!("profile entry type cannot be read: {error}"),
                )
            })?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().into_string().map_err(|_| {
            DynamicHostError::new("ProfileListFailed", "profile name is not valid Unicode")
        })?;
        profiles.push(name);
    }
    profiles.sort();
    Ok(profiles)
}

fn startup_state(loaded: &LoadedSettings) -> Result<BTreeMap<String, Value>, DynamicHostError> {
    let value = |id: &str| {
        loaded
            .settings
            .get(id)
            .map(|resolved| resolved.value.clone())
            .ok_or_else(|| {
                DynamicHostError::new(
                    "MissingSetting",
                    format!("startup settings are missing {id}"),
                )
            })
    };
    Ok(BTreeMap::from([
        ("serial_port".to_owned(), value("serial.port")?),
        ("serial_baud_rate".to_owned(), value("serial.baud_rate")?),
        ("serial_connected".to_owned(), Value::Bool(false)),
        ("camera_opened".to_owned(), Value::Bool(false)),
        ("camera_fps".to_owned(), value("camera.capture_fps")?),
        (
            "camera_resolution".to_owned(),
            value("camera.capture_resolution")?,
        ),
        ("camera_device".to_owned(), value("camera.device")?),
        ("is_running".to_owned(), Value::Bool(false)),
        (
            "command_state".to_owned(),
            Value::String("stopped".to_owned()),
        ),
        ("current_command".to_owned(), Value::String(String::new())),
        ("command_candidates".to_owned(), Value::Array(Vec::new())),
        ("tags".to_owned(), Value::Array(Vec::new())),
        (
            "command_display_lists".to_owned(),
            serde_json::json!({"-": []}),
        ),
        (
            "command_display_cache_loading".to_owned(),
            Value::Bool(false),
        ),
        (
            "active_profile".to_owned(),
            Value::String(loaded.active_profile.as_str().to_owned()),
        ),
        ("pending_profile".to_owned(), Value::Null),
        (
            "available_profiles".to_owned(),
            serde_json::to_value(list_profiles(loaded)?).map_err(|_| {
                DynamicHostError::new("StateEncodingFailed", "profile list cannot be encoded")
            })?,
        ),
        ("last_input".to_owned(), Value::Null),
        ("holding_buttons".to_owned(), Value::Array(Vec::new())),
        ("pid".to_owned(), Value::from(std::process::id())),
    ]))
}

fn refreshed_state(
    loaded: &LoadedSettings,
    previous: Option<&BTreeMap<String, Value>>,
) -> Result<BTreeMap<String, Value>, DynamicHostError> {
    let mut state = startup_state(loaded)?;
    if let Some(previous) = previous {
        for name in RUNTIME_STATE_FIELDS {
            if let Some(value) = previous.get(*name) {
                state.insert((*name).to_owned(), value.clone());
            }
        }
    }
    Ok(state)
}

fn holding_buttons(state: ControllerState) -> Value {
    serde_json::to_value(state.buttons.pressed()).unwrap_or_else(|_| Value::Array(Vec::new()))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use crate::device::ControllerUpdate;
    use crate::settings::pipeline::{PipelineRequest, SettingsPipeline};
    use crate::settings::roots::{BaseDirectories, RootEnvironment};
    use pokecon_dynamic::{CommandDisplayItem, DynamicHost};
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn fixture() -> (TempDir, PipelineRequest, LoadedSettings) {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("base directories must resolve");
        let request = PipelineRequest {
            arguments: ["pokecon", "--language", "en"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.join("cwd"),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        };
        let loaded = SettingsPipeline::new(request.clone())
            .load_before_dynamic()
            .expect("pre-dynamic settings must resolve");
        fs::create_dir_all(loaded.roots.config.join("profiles/default"))
            .expect("default profile must exist");
        fs::create_dir_all(loaded.roots.config.join("profiles/Other"))
            .expect("other profile must exist");
        (temporary, request, loaded)
    }

    #[test]
    fn startup_assignments_are_memory_only_and_cli_is_applied_last() {
        let (_temporary, request, loaded) = fixture();
        let global_settings = loaded.global_settings_path.clone();
        let host = StartupDynamicHost::new(request, loaded).expect("host must initialize");
        let applied = host
            .apply_settings(&BTreeMap::from([(
                "language".to_owned(),
                Value::from("ja"),
            )]))
            .expect("dynamic assignment must apply");
        assert_eq!(applied["language"], Value::from("ja"));
        assert!(!global_settings.exists());
        let final_settings = host.finish_startup().expect("startup must finalize");
        assert_eq!(final_settings.settings.string("language").unwrap(), "en");
        assert_eq!(host.dynamic_values()["language"], Value::from("ja"));
        let runtime = host
            .apply_settings(&BTreeMap::from([(
                "language".to_owned(),
                Value::from("ja"),
            )]))
            .expect("runtime assignment must follow CLI");
        assert_eq!(runtime["language"], Value::from("ja"));
        assert!(!global_settings.exists());
    }

    #[tokio::test]
    async fn profile_state_and_controller_are_rust_owned() {
        let (_temporary, request, loaded) = fixture();
        let global_settings = loaded.global_settings_path.clone();
        let host = StartupDynamicHost::new(request, loaded).expect("host must initialize");
        let mut runtime_changes = host.subscribe_runtime_changes();
        assert_eq!(host.profile_list().unwrap(), ["Other", "default"]);
        fs::create_dir(host.loaded_settings().roots.config.join("profiles/New"))
            .expect("new profile must be created");
        host.refresh_available_profiles()
            .expect("profile state must refresh");
        assert_eq!(
            host.public_state_snapshot()["available_profiles"],
            json!(["New", "Other", "default"])
        );
        host.finish_startup().expect("startup must finalize");
        host.profile_switch_begin("Other", &BTreeMap::new())
            .await
            .unwrap();
        host.profile_switch_commit().await.unwrap();
        host.profile_switch_end().await.unwrap();
        assert_eq!(host.profile_current().unwrap(), "Other");
        assert!(
            std::fs::read_to_string(global_settings)
                .expect("active profile must be persisted")
                .contains("active_profile = \"Other\"")
        );
        runtime_changes
            .changed()
            .await
            .expect("committed runtime changes must be observable");
        assert!(
            host.profile_switch_begin("Missing", &BTreeMap::new())
                .await
                .is_err()
        );

        host.controller_update(ControllerUpdate {
            a: Some(true),
            ..ControllerUpdate::default()
        })
        .expect("controller update must apply");
        assert!(host.controller_safety().state().buttons.a);
        let external_source = InputSourceId::new("test-browser").unwrap();
        let external_generation = InputGeneration::new("test-generation").unwrap();
        let arbiter = host.controller_safety().arbiter();
        let mut arbiter = arbiter.lock();
        arbiter.begin_generation(
            external_source.clone(),
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            external_generation.clone(),
        );
        let mut external = ControllerState::NEUTRAL;
        external.buttons.b = true;
        let (applied, acknowledgement) = arbiter
            .apply_snapshot(
                &external_source,
                InputSnapshot {
                    generation: external_generation,
                    sequence: InputSequence::zero(),
                    keyboard_keys: Vec::new(),
                    mouse_buttons: MouseButtons::default(),
                    buttons: external.buttons,
                    hat: external.hat,
                    left_stick: external.left_stick,
                    right_stick: external.right_stick,
                    touch: external.touch,
                },
            )
            .unwrap();
        assert_eq!(applied, ApplyResult::Applied);
        assert!(acknowledgement.is_some());
        drop(arbiter);
        assert!(host.controller_safety().state().buttons.a);
        assert!(host.controller_safety().state().buttons.b);
        assert_eq!(
            host.state_snapshot().unwrap()["holding_buttons"],
            json!(["A", "B"])
        );
        host.controller_safety().force_release();
        assert!(!host.controller_safety().state().buttons.a);
        assert!(host.controller_safety().state().buttons.b);
        assert_eq!(
            host.state_snapshot().unwrap()["holding_buttons"],
            json!(["B"])
        );
    }

    #[test]
    fn writable_state_is_closed_and_validated() {
        let (_temporary, request, loaded) = fixture();
        let host = StartupDynamicHost::new(request, loaded).expect("host must initialize");
        host.set_state_value("tags", json!(["alpha", "beta"]))
            .expect("unique tags must apply");
        assert_eq!(
            host.state_snapshot().unwrap()["tags"],
            json!(["alpha", "beta"])
        );
        host.set_state_value("tags", json!(["same", "same", "different"]))
            .expect("duplicate tags must be deduplicated");
        assert_eq!(
            host.state_snapshot().unwrap()["tags"],
            json!(["same", "different"])
        );
        let before = json!(["same", "different"]);
        host.merge_state_value(
            "tags",
            before.clone(),
            json!(["same", "different", "python"]),
        )
        .expect("the first callback append must apply");
        host.merge_state_value("tags", before, json!(["same", "different", "lua"]))
            .expect("an independent callback append must merge");
        assert_eq!(
            host.state_snapshot().unwrap()["tags"],
            json!(["same", "different", "python", "lua"])
        );
        assert_eq!(
            host.set_state_value("pid", json!(1)).unwrap_err().code,
            "ReadOnlyState"
        );
        host.apply_settings(&BTreeMap::from([("language".to_owned(), json!("en"))]))
            .expect("settings refresh must succeed");
        assert_eq!(
            host.state_snapshot().unwrap()["tags"],
            json!(["same", "different", "python", "lua"])
        );
        host.begin_stopping();
        assert_eq!(
            host.set_state_value("tags", json!([])).unwrap_err().code,
            "HostStopping"
        );
        assert_eq!(
            host.controller_update(ControllerUpdate::default())
                .unwrap_err()
                .code,
            "HostStopping"
        );
    }

    #[test]
    fn script_load_staging_never_exposes_a_partial_display_generation() {
        let (_temporary, request, loaded) = fixture();
        let host = StartupDynamicHost::new(request, loaded).expect("host must initialize");
        let initial = CommandInfo {
            name: "Initial".to_owned(),
            module_path: "Commands.PythonCommands.initial".to_owned(),
            class_name: "Initial".to_owned(),
            tags: vec!["@Samples".to_owned()],
        };
        host.begin_script_load(vec![initial.clone()])
            .expect("initial load must begin");
        assert_eq!(
            host.state_snapshot().unwrap()["command_candidates"],
            json!([initial])
        );
        assert_eq!(
            host.state_snapshot().unwrap()["command_display_lists"],
            json!({"-": []})
        );
        assert_eq!(
            host.state_snapshot().unwrap()["command_display_cache_loading"],
            json!(true)
        );

        host.set_state_value(
            "command_candidates",
            json!([{
                "name": "Initial",
                "module_path": "Commands.PythonCommands.initial",
                "class_name": "Initial",
                "tags": ["@Samples", "dynamic"]
            }]),
        )
        .expect("ScriptLoadPre mutation must be staged");
        assert_eq!(
            host.public_state_snapshot()["command_candidates"],
            json!([])
        );
        let (candidates, _tags) = host.staged_script_load().unwrap();
        host.set_staged_script_load(candidates.clone(), vec!["-".into(), "dynamic".into()])
            .expect("final tags must be staged");
        let cache = CommandDisplayCache {
            generation: 1,
            candidates: candidates.clone(),
            tags: vec!["-".into(), "dynamic".into()],
            display_lists: BTreeMap::from([
                (
                    "-".to_owned(),
                    vec![CommandDisplayItem::Command {
                        command: candidates[0].clone(),
                    }],
                ),
                (
                    "dynamic".to_owned(),
                    vec![CommandDisplayItem::Command {
                        command: candidates[0].clone(),
                    }],
                ),
            ]),
        };
        host.publish_command_cache(&cache)
            .expect("complete generation must publish");
        let published = host.state_snapshot().unwrap();
        assert_eq!(published["command_candidates"], json!(candidates));
        assert_eq!(published["tags"], json!(["-", "dynamic"]));
        assert_eq!(published["command_display_cache_loading"], json!(false));

        let replacement = CommandInfo {
            name: "Replacement".to_owned(),
            module_path: "Commands.PythonCommands.replacement".to_owned(),
            class_name: "Replacement".to_owned(),
            tags: Vec::new(),
        };
        host.begin_script_load(vec![replacement.clone()])
            .expect("recomputation must stage");
        let recomputing = host.state_snapshot().unwrap();
        assert_eq!(recomputing["command_candidates"], json!([replacement]));
        assert_eq!(
            recomputing["command_display_lists"],
            published["command_display_lists"]
        );
        assert_eq!(recomputing["command_display_cache_loading"], json!(false));
        host.cancel_script_load();
        assert_eq!(
            host.state_snapshot().unwrap()["command_candidates"],
            published["command_candidates"]
        );
    }

    #[test]
    fn command_recompute_notifications_are_monotonic_and_coalesced() {
        let (_temporary, request, loaded) = fixture();
        let host = StartupDynamicHost::new(request, loaded).expect("host must initialize");
        let receiver = host.subscribe_command_recompute();
        host.request_command_recompute();
        host.request_command_recompute();
        assert_eq!(*receiver.borrow(), 2);
        assert_eq!(host.command_recompute_requests(), 2);
    }
}
