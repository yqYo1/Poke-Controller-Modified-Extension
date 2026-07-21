//! Rust-main-owned host state used while the persistent dynamic worker runs
//! its startup configuration.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;
use pokecon_contracts::model::Scope;
use pokecon_device::controller::{ControllerState, ControllerUpdate};
use pokecon_device::input::{
    ApplyResult, InputArbiter, InputEvent, InputGeneration, InputPriority, InputSequence,
    InputSnapshot, InputSourceId, InputSourceKind, MouseButtons,
};
use pokecon_dynamic::{CommandInfo, Diagnostic, DiagnosticLevel, DynamicHost, DynamicHostError};
use pokecon_settings::pipeline::{
    LoadedSettings, PipelineError, PipelineRequest, SettingsPipeline,
};
use pokecon_settings::roots::SafeComponent;
use pokecon_worker::ipc::ResourceSafety;
use serde_json::Value;

const DYNAMIC_SOURCE: &str = "dynamic-config";
const DYNAMIC_GENERATION: &str = "dynamic-1";

/// Controller ownership for the dynamic worker. The same object is passed to
/// the IPC connection as its transport-loss safety boundary.
#[derive(Debug)]
pub struct DynamicControllerSafety {
    arbiter: Mutex<InputArbiter>,
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
            arbiter: Mutex::new(arbiter),
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
) -> Result<(), pokecon_device::input::InputError> {
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
        return Err(pokecon_device::input::InputError::UnknownSource);
    }
    Ok(())
}

fn input_error(error: &pokecon_device::input::InputError) -> DynamicHostError {
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
    startup_complete: bool,
    stopping: bool,
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
                startup_complete: false,
                stopping: false,
            }),
            controller: Arc::new(DynamicControllerSafety::new()),
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
        Ok(loaded)
    }

    /// Returns the complete committed top-level assignment map for the final
    /// pipeline and subsequent runtime-host construction.
    #[must_use]
    pub fn dynamic_values(&self) -> BTreeMap<String, Value> {
        self.inner.lock().dynamic_values.clone()
    }

    /// Returns the transport-loss safety object for `WorkerSupervisor::spawn`.
    #[must_use]
    pub fn controller_safety(&self) -> Arc<DynamicControllerSafety> {
        Arc::clone(&self.controller)
    }

    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.inner.lock().diagnostics.clone()
    }

    #[must_use]
    pub fn outputs(&self) -> Vec<String> {
        self.inner.lock().outputs.clone()
    }

    #[must_use]
    pub fn command_recompute_requests(&self) -> u64 {
        self.inner.lock().command_recompute_requests
    }

    /// Closes every mutating host boundary and immediately releases dynamic
    /// controller ownership before worker shutdown begins.
    pub fn begin_stopping(&self) {
        self.inner.lock().stopping = true;
        self.controller.force_release();
    }

    fn current_settings(inner: &StartupHostState) -> BTreeMap<String, Value> {
        inner
            .loaded
            .settings
            .values()
            .iter()
            .map(|(id, resolved)| (id.clone(), resolved.value.clone()))
            .collect()
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
            validate_existing_profile(&inner.loaded, target)?;
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
            retained.extend(changes.clone());
            let switched = inner
                .loaded
                .switch_profile_in_memory(target)
                .map_err(|error| pipeline_error(&error))?;
            let loaded = switched
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

impl DynamicHost for StartupDynamicHost {
    fn settings_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        Ok(Self::current_settings(&self.inner.lock()))
    }

    fn apply_settings(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        self.apply_startup_settings(changes)
    }

    fn state_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let inner = self.inner.lock();
        let mut snapshot = inner.public_state.clone();
        snapshot.insert(
            "holding_buttons".to_owned(),
            holding_buttons(self.controller.state()),
        );
        Ok(snapshot)
    }

    fn set_state_value(&self, name: &str, value: Value) -> Result<(), DynamicHostError> {
        let value = match name {
            "command_candidates" => {
                serde_json::from_value::<Vec<CommandInfo>>(value.clone()).map_err(|_| {
                    DynamicHostError::new(
                        "InvalidState",
                        "command_candidates must be a list of CommandInfo objects",
                    )
                })?;
                value
            }
            "tags" => {
                let mut tags = serde_json::from_value::<Vec<String>>(value).map_err(|_| {
                    DynamicHostError::new("InvalidState", "tags must be a list of strings")
                })?;
                let mut retained = BTreeSet::new();
                tags.retain(|tag| retained.insert(tag.clone()));
                serde_json::to_value(tags).map_err(|_| {
                    DynamicHostError::new("StateEncodingFailed", "tags cannot be encoded")
                })?
            }
            _ => {
                return Err(DynamicHostError::new(
                    "ReadOnlyState",
                    format!("state property is read-only: {name}"),
                ));
            }
        };
        let mut inner = self.inner.lock();
        ensure_running(&inner)?;
        inner.public_state.insert(name.to_owned(), value);
        Ok(())
    }

    fn profile_current(&self) -> Result<String, DynamicHostError> {
        Ok(self.inner.lock().loaded.active_profile.as_str().to_owned())
    }

    fn profile_list(&self) -> Result<Vec<String>, DynamicHostError> {
        list_profiles(&self.inner.lock().loaded)
    }

    fn profile_switch(&self, name: &str) -> Result<bool, DynamicHostError> {
        {
            let inner = self.inner.lock();
            ensure_running(&inner)?;
            if validate_existing_profile(&inner.loaded, name).is_err() {
                return Ok(false);
            }
        }
        self.apply_startup_settings(&BTreeMap::from([(
            "active_profile".to_owned(),
            Value::String(name.to_owned()),
        )]))?;
        Ok(true)
    }

    fn controller_update(&self, update: ControllerUpdate) -> Result<(), DynamicHostError> {
        ensure_running(&self.inner.lock())?;
        self.controller.update(update)
    }

    fn controller_reset(&self) -> Result<(), DynamicHostError> {
        ensure_running(&self.inner.lock())?;
        self.controller.reset()
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
        }
    }
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
        for name in [
            "serial_connected",
            "camera_opened",
            "is_running",
            "command_state",
            "current_command",
            "command_candidates",
            "tags",
            "pending_profile",
            "last_input",
        ] {
            if let Some(value) = previous.get(name) {
                state.insert(name.to_owned(), value.clone());
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

    use pokecon_device::controller::ControllerUpdate;
    use pokecon_dynamic::DynamicHost;
    use pokecon_settings::pipeline::{PipelineRequest, SettingsPipeline};
    use pokecon_settings::roots::{BaseDirectories, RootEnvironment};
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

    #[test]
    fn profile_state_and_controller_are_rust_owned() {
        let (_temporary, request, loaded) = fixture();
        let host = StartupDynamicHost::new(request, loaded).expect("host must initialize");
        assert_eq!(host.profile_list().unwrap(), ["Other", "default"]);
        host.finish_startup().expect("startup must finalize");
        assert!(host.profile_switch("Other").unwrap());
        assert_eq!(host.profile_current().unwrap(), "Other");
        assert!(!host.profile_switch("Missing").unwrap());

        host.controller_update(ControllerUpdate {
            a: Some(true),
            ..ControllerUpdate::default()
        })
        .expect("controller update must apply");
        assert!(host.controller_safety().state().buttons.a);
        assert_eq!(
            host.state_snapshot().unwrap()["holding_buttons"],
            json!(["A"])
        );
        host.controller_safety().force_release();
        assert_eq!(host.controller_safety().state(), ControllerState::NEUTRAL);
        assert_eq!(host.state_snapshot().unwrap()["holding_buttons"], json!([]));
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
        assert_eq!(
            host.set_state_value("pid", json!(1)).unwrap_err().code,
            "ReadOnlyState"
        );
        host.apply_settings(&BTreeMap::from([("language".to_owned(), json!("en"))]))
            .expect("settings refresh must succeed");
        assert_eq!(
            host.state_snapshot().unwrap()["tags"],
            json!(["same", "different"])
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
}
