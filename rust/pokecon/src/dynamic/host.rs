use std::collections::BTreeMap;
use std::sync::Arc;

use crate::device::controller::{ControllerState, ControllerUpdate};
use async_trait::async_trait;
use parking_lot::Mutex;
use pokecon_contracts::model::{Setting, ValueSchema};
use pokecon_contracts::{ContractError, settings_registry};
use pokecon_settings::pipeline::SECRET_MASK;
use serde_json::Value;

use crate::dynamic::callback::Diagnostic;
use crate::dynamic::protocol::{HostProfileSwitchBeginResult, HostProfileSwitchCommitResult};

/// Host-operation failure exposed to both language bindings with identical
/// code and message semantics.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
#[error("{message}")]
pub struct DynamicHostError {
    pub code: String,
    pub message: String,
}

impl DynamicHostError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Rust-main operations available to the isolated dynamic worker. Methods are
/// synchronous because Python and Lua expose synchronous public APIs; the IPC
/// adapter keeps transport I/O on independent Tokio tasks.
#[async_trait]
pub trait DynamicHost: Send + Sync {
    /// # Errors
    ///
    /// Returns a host transport or snapshot failure.
    fn settings_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError>;

    /// # Errors
    ///
    /// Returns a validation, persistence, or host transport failure.
    fn apply_settings(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError>;

    /// # Errors
    ///
    /// Returns a host transport or snapshot failure.
    fn state_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError>;

    /// # Errors
    ///
    /// Returns a validation, persistence, or host transport failure.
    fn set_state_value(&self, name: &str, value: Value) -> Result<(), DynamicHostError>;

    /// Atomically merges one callback-local state edit with the latest host
    /// value. `before` is the value observed by the callback and `value` is
    /// that callback's edited value.
    ///
    /// # Errors
    ///
    /// Returns a validation, concurrent-update conflict, or host transport
    /// failure.
    fn merge_state_value(
        &self,
        name: &str,
        before: Value,
        value: Value,
    ) -> Result<(), DynamicHostError>;

    /// # Errors
    ///
    /// Returns a host transport or profile lookup failure.
    fn profile_current(&self) -> Result<String, DynamicHostError>;

    /// # Errors
    ///
    /// Returns a host transport or profile lookup failure.
    fn profile_list(&self) -> Result<Vec<String>, DynamicHostError>;

    /// # Errors
    ///
    /// Acquires the non-recursive switch gate, validates the complete target,
    /// and publishes only `pending_profile`.
    ///
    /// # Errors
    ///
    /// Returns a busy, validation, persistence, or host transport failure.
    async fn profile_switch_begin(
        &self,
        name: &str,
        changes: &BTreeMap<String, Value>,
    ) -> Result<HostProfileSwitchBeginResult, DynamicHostError>;

    /// Stops and reaps the old user worker before atomically committing the
    /// prepared profile. The switch gate remains held for Post callbacks.
    ///
    /// # Errors
    ///
    /// Returns a worker, settings commit, or host transport failure.
    async fn profile_switch_commit(
        &self,
    ) -> Result<HostProfileSwitchCommitResult, DynamicHostError>;

    /// Clears a prepared target and releases the switch gate.
    ///
    /// # Errors
    ///
    /// Returns a host transport failure.
    async fn profile_switch_abort(&self) -> Result<(), DynamicHostError>;

    /// Releases the switch gate after Post callbacks.
    ///
    /// # Errors
    ///
    /// Returns a host transport failure.
    async fn profile_switch_end(&self) -> Result<(), DynamicHostError>;

    /// # Errors
    ///
    /// Returns a controller validation or host transport failure.
    fn controller_update(&self, update: ControllerUpdate) -> Result<(), DynamicHostError>;

    /// # Errors
    ///
    /// Returns a controller or host transport failure.
    fn controller_reset(&self) -> Result<(), DynamicHostError>;

    fn record_diagnostic(&self, diagnostic: Diagnostic);

    /// Records one complete line written by dynamic user code. The worker IPC
    /// adapter maps this to a structured `log` envelope instead of raw stdout.
    fn record_output(&self, _message: &str) {}

    /// Requests one coalesced rebuild of all command display-list snapshots.
    fn request_command_recompute(&self) {}
}

/// Applies the changes between `before` and `value` to `current` without
/// discarding independent concurrent edits. Arrays are index-merged and
/// callback-local suffixes are appended after suffixes committed earlier.
///
/// # Errors
///
/// Returns `StateConflict` when both writers incompatibly changed the same
/// scalar, object entry, or removed array entry.
pub fn merge_state_change(
    before: &Value,
    current: &Value,
    value: &Value,
) -> Result<Value, DynamicHostError> {
    merge_state_change_at(before, current, value, "$")
}

fn merge_state_change_at(
    before: &Value,
    current: &Value,
    value: &Value,
    path: &str,
) -> Result<Value, DynamicHostError> {
    if value == before || current == value {
        return Ok(current.clone());
    }
    if current == before {
        return Ok(value.clone());
    }

    match (before, current, value) {
        (Value::Object(before), Value::Object(current), Value::Object(value)) => {
            let mut merged = current.clone();
            for (key, before_value) in before {
                let child_path = format!("{path}.{}", display_state_key(key));
                match value.get(key) {
                    Some(value_value) if value_value != before_value => {
                        let current_value = current.get(key).ok_or_else(|| {
                            state_conflict(&child_path, "the current value was removed")
                        })?;
                        merged.insert(
                            key.clone(),
                            merge_state_change_at(
                                before_value,
                                current_value,
                                value_value,
                                &child_path,
                            )?,
                        );
                    }
                    Some(_) => {}
                    None => match current.get(key) {
                        None => {}
                        Some(current_value) if current_value == before_value => {
                            merged.remove(key);
                        }
                        Some(_) => {
                            return Err(state_conflict(
                                &child_path,
                                "the current value changed before removal",
                            ));
                        }
                    },
                }
            }
            for (key, value_value) in value {
                if before.contains_key(key) {
                    continue;
                }
                let child_path = format!("{path}.{}", display_state_key(key));
                match current.get(key) {
                    None => {
                        merged.insert(key.clone(), value_value.clone());
                    }
                    Some(current_value) if current_value == value_value => {}
                    Some(_) => {
                        return Err(state_conflict(
                            &child_path,
                            "both callbacks added different values",
                        ));
                    }
                }
            }
            Ok(Value::Object(merged))
        }
        (Value::Array(before), Value::Array(current), Value::Array(value)) => {
            if current.len() < before.len() {
                return Err(state_conflict(
                    path,
                    "the current array was shortened concurrently",
                ));
            }

            let retained = before.len().min(value.len());
            let mut merged = current.clone();
            for index in 0..retained {
                let child_path = format!("{path}[{index}]");
                merged[index] = merge_state_change_at(
                    &before[index],
                    &current[index],
                    &value[index],
                    &child_path,
                )?;
            }

            if value.len() < before.len() {
                for index in value.len()..before.len() {
                    if current[index] != before[index] {
                        return Err(state_conflict(
                            &format!("{path}[{index}]"),
                            "the removed entry changed concurrently",
                        ));
                    }
                }
                merged.drain(value.len()..before.len());
            } else {
                merged.extend(value[before.len()..].iter().cloned());
            }
            Ok(Value::Array(merged))
        }
        _ => Err(state_conflict(
            path,
            "both callbacks changed the same value",
        )),
    }
}

fn display_state_key(key: &str) -> String {
    if key
        .chars()
        .all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        key.to_owned()
    } else {
        format!("[{key:?}]")
    }
}

fn state_conflict(path: &str, reason: &str) -> DynamicHostError {
    DynamicHostError::new(
        "StateConflict",
        format!("concurrent state mutation conflicts at {path}: {reason}"),
    )
}

/// Canonical lookup and validation projection for `pokecon.opt.*`.
#[derive(Debug)]
pub struct DynamicSettingsRegistry {
    by_dynamic_path: BTreeMap<String, Setting>,
    prefixes: std::collections::BTreeSet<String>,
}

impl DynamicSettingsRegistry {
    /// Builds the projection from the single machine-readable registry.
    ///
    /// # Errors
    ///
    /// Returns a contract error if the embedded registry is invalid.
    pub fn load() -> Result<Self, ContractError> {
        let registry = settings_registry()?;
        let mut by_dynamic_path = BTreeMap::new();
        let mut prefixes = std::collections::BTreeSet::new();
        for setting in registry.settings() {
            let Some(path) = &setting.surfaces.dynamic.name else {
                continue;
            };
            by_dynamic_path.insert(path.clone(), setting.clone());
            let mut prefix = String::new();
            for segment in path.split('.').take(path.split('.').count() - 1) {
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(segment);
                prefixes.insert(prefix.clone());
            }
        }
        Ok(Self {
            by_dynamic_path,
            prefixes,
        })
    }

    #[must_use]
    pub fn setting(&self, dynamic_path: &str) -> Option<&Setting> {
        self.by_dynamic_path.get(dynamic_path)
    }

    #[must_use]
    pub fn is_prefix(&self, dynamic_path: &str) -> bool {
        self.prefixes.contains(dynamic_path)
    }

    #[must_use]
    pub fn setting_by_id(&self, id: &str) -> Option<&Setting> {
        self.by_dynamic_path
            .values()
            .find(|setting| setting.id == id)
    }

    /// Converts aliases/case-insensitive enums to their canonical spelling and
    /// validates the closed value schema.
    ///
    /// # Errors
    ///
    /// Returns a host-style validation error for unsupported paths or values.
    pub fn normalize(
        &self,
        dynamic_path: &str,
        value: Value,
    ) -> Result<(String, Value), DynamicHostError> {
        let setting = self.setting(dynamic_path).ok_or_else(|| {
            DynamicHostError::new(
                "UnknownSetting",
                format!("dynamic setting path is not supported: {dynamic_path}"),
            )
        })?;
        let value = normalize_schema_value(&setting.value, value)?;
        setting.value.validate(&value).map_err(|reason| {
            DynamicHostError::new(
                "InvalidSetting",
                format!("invalid value for {}: {reason}", setting.id),
            )
        })?;
        Ok((setting.id.clone(), value))
    }

    #[must_use]
    pub fn public_dynamic_value(&self, id: &str, value: &Value) -> Value {
        if self.setting_by_id(id).is_some_and(|setting| setting.secret)
            && value.as_str().is_some_and(|value| !value.is_empty())
        {
            Value::String(SECRET_MASK.to_owned())
        } else {
            value.clone()
        }
    }
}

fn normalize_schema_value(schema: &ValueSchema, value: Value) -> Result<Value, DynamicHostError> {
    match schema {
        ValueSchema::Enum {
            values,
            aliases,
            ascii_case_insensitive,
        } => {
            let raw = value
                .as_str()
                .ok_or_else(|| DynamicHostError::new("InvalidSetting", "expected enum string"))?;
            if let Some(canonical) = aliases.get(raw) {
                return Ok(Value::String(canonical.clone()));
            }
            if *ascii_case_insensitive
                && let Some(canonical) = values
                    .iter()
                    .find(|canonical| canonical.eq_ignore_ascii_case(raw))
            {
                return Ok(Value::String(canonical.clone()));
            }
            Ok(value)
        }
        ValueSchema::Union { variants } => {
            for variant in variants {
                if let Ok(normalized) = normalize_schema_value(variant, value.clone())
                    && variant.validate(&normalized).is_ok()
                {
                    return Ok(normalized);
                }
            }
            Ok(value)
        }
        _ => Ok(value),
    }
}

#[derive(Debug)]
struct InMemoryState {
    settings: BTreeMap<String, Value>,
    state: BTreeMap<String, Value>,
    active_profile: String,
    profiles: Vec<String>,
    profile_settings: BTreeMap<String, BTreeMap<String, Value>>,
    prepared_profile: Option<(String, BTreeMap<String, Value>)>,
    profile_switching: bool,
    controller: ControllerState,
    diagnostics: Vec<Diagnostic>,
    outputs: Vec<String>,
    command_recompute_requests: u64,
}

/// Deterministic host used by cross-language conformance and transaction tests.
#[derive(Debug)]
pub struct InMemoryDynamicHost {
    registry: Arc<DynamicSettingsRegistry>,
    inner: Mutex<InMemoryState>,
}

impl InMemoryDynamicHost {
    /// Creates an in-memory host from canonical-ID settings and public state.
    ///
    /// # Errors
    ///
    /// Returns a contract error if the embedded settings registry is invalid.
    pub fn new(
        settings: BTreeMap<String, Value>,
        state: BTreeMap<String, Value>,
    ) -> Result<Self, ContractError> {
        let active_profile = state
            .get("active_profile")
            .and_then(Value::as_str)
            .unwrap_or("default")
            .to_owned();
        let profiles = state
            .get("available_profiles")
            .and_then(Value::as_array)
            .map_or_else(
                || vec![active_profile.clone()],
                |values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                },
            );
        Ok(Self {
            registry: Arc::new(DynamicSettingsRegistry::load()?),
            inner: Mutex::new(InMemoryState {
                settings,
                state,
                active_profile,
                profiles,
                profile_settings: BTreeMap::new(),
                prepared_profile: None,
                profile_switching: false,
                controller: ControllerState::NEUTRAL,
                diagnostics: Vec::new(),
                outputs: Vec::new(),
                command_recompute_requests: 0,
            }),
        })
    }

    #[must_use]
    pub fn controller_state(&self) -> ControllerState {
        self.inner.lock().controller
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

    /// Installs a deterministic per-profile settings overlay for conformance
    /// tests that need target values to differ from the active snapshot.
    pub fn set_profile_settings(&self, profile: &str, settings: BTreeMap<String, Value>) {
        self.inner
            .lock()
            .profile_settings
            .insert(profile.to_owned(), settings);
    }
}

#[async_trait]
impl DynamicHost for InMemoryDynamicHost {
    fn settings_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        Ok(self.inner.lock().settings.clone())
    }

    fn apply_settings(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        let mut inner = self.inner.lock();
        let mut prospective = inner.settings.clone();
        for (id, value) in changes {
            let setting = self.registry.setting_by_id(id).ok_or_else(|| {
                DynamicHostError::new("UnknownSetting", format!("unknown setting: {id}"))
            })?;
            setting.value.validate(value).map_err(|reason| {
                DynamicHostError::new(
                    "InvalidSetting",
                    format!("invalid value for {id}: {reason}"),
                )
            })?;
            prospective.insert(id.clone(), value.clone());
        }
        validate_known_cross_constraints(&prospective)?;
        inner.settings = prospective.clone();
        Ok(prospective)
    }

    fn state_snapshot(&self) -> Result<BTreeMap<String, Value>, DynamicHostError> {
        Ok(self.inner.lock().state.clone())
    }

    fn set_state_value(&self, name: &str, value: Value) -> Result<(), DynamicHostError> {
        if !matches!(name, "command_candidates" | "tags") {
            return Err(DynamicHostError::new(
                "ReadOnlyState",
                format!("state property is read-only: {name}"),
            ));
        }
        self.inner.lock().state.insert(name.to_owned(), value);
        Ok(())
    }

    fn merge_state_value(
        &self,
        name: &str,
        before: Value,
        value: Value,
    ) -> Result<(), DynamicHostError> {
        if !matches!(name, "command_candidates" | "tags") {
            return Err(DynamicHostError::new(
                "ReadOnlyState",
                format!("state property is read-only: {name}"),
            ));
        }
        let mut inner = self.inner.lock();
        let current = inner.state.get(name).ok_or_else(|| {
            DynamicHostError::new("UnknownState", format!("unknown state property: {name}"))
        })?;
        let merged = merge_state_change(&before, current, &value)?;
        inner.state.insert(name.to_owned(), merged);
        Ok(())
    }

    fn profile_current(&self) -> Result<String, DynamicHostError> {
        Ok(self.inner.lock().active_profile.clone())
    }

    fn profile_list(&self) -> Result<Vec<String>, DynamicHostError> {
        Ok(self.inner.lock().profiles.clone())
    }

    async fn profile_switch_begin(
        &self,
        name: &str,
        changes: &BTreeMap<String, Value>,
    ) -> Result<HostProfileSwitchBeginResult, DynamicHostError> {
        validate_profile_name(name)?;
        let mut inner = self.inner.lock();
        if inner.profile_switching {
            return Err(DynamicHostError::new(
                "ProfileSwitchBusy",
                "another profile switch is already in progress",
            ));
        }
        if !inner.profiles.iter().any(|profile| profile == name) {
            return Err(DynamicHostError::new(
                "ProfileNotFound",
                "profile does not exist",
            ));
        }
        let mut prospective = inner.settings.clone();
        if let Some(profile_settings) = inner.profile_settings.get(name) {
            for (id, value) in profile_settings {
                let setting = self.registry.setting_by_id(id).ok_or_else(|| {
                    DynamicHostError::new("UnknownSetting", format!("unknown setting: {id}"))
                })?;
                setting.value.validate(value).map_err(|reason| {
                    DynamicHostError::new(
                        "InvalidSetting",
                        format!("invalid value for {id}: {reason}"),
                    )
                })?;
                prospective.insert(id.clone(), value.clone());
            }
        }
        for (id, value) in changes {
            let setting = self.registry.setting_by_id(id).ok_or_else(|| {
                DynamicHostError::new("UnknownSetting", format!("unknown setting: {id}"))
            })?;
            setting.value.validate(value).map_err(|reason| {
                DynamicHostError::new(
                    "InvalidSetting",
                    format!("invalid value for {id}: {reason}"),
                )
            })?;
            prospective.insert(id.clone(), value.clone());
        }
        prospective.insert("active_profile".to_owned(), Value::String(name.to_owned()));
        validate_known_cross_constraints(&prospective)?;
        inner.profile_switching = true;
        inner
            .state
            .insert("pending_profile".to_owned(), Value::String(name.to_owned()));
        inner.prepared_profile = Some((name.to_owned(), prospective.clone()));
        Ok(HostProfileSwitchBeginResult {
            settings: prospective,
        })
    }

    async fn profile_switch_commit(
        &self,
    ) -> Result<HostProfileSwitchCommitResult, DynamicHostError> {
        let mut inner = self.inner.lock();
        let (name, settings) = inner.prepared_profile.take().ok_or_else(|| {
            DynamicHostError::new("NoProfileSwitch", "no profile switch is prepared")
        })?;
        name.clone_into(&mut inner.active_profile);
        inner.settings = settings;
        inner
            .state
            .insert("active_profile".to_owned(), Value::String(name));
        inner
            .state
            .insert("pending_profile".to_owned(), Value::Null);
        Ok(HostProfileSwitchCommitResult {
            forced_worker_stop: false,
        })
    }

    async fn profile_switch_abort(&self) -> Result<(), DynamicHostError> {
        let mut inner = self.inner.lock();
        inner.prepared_profile = None;
        inner.profile_switching = false;
        inner
            .state
            .insert("pending_profile".to_owned(), Value::Null);
        Ok(())
    }

    async fn profile_switch_end(&self) -> Result<(), DynamicHostError> {
        self.inner.lock().profile_switching = false;
        Ok(())
    }

    fn controller_update(&self, update: ControllerUpdate) -> Result<(), DynamicHostError> {
        self.inner
            .lock()
            .controller
            .apply(&update)
            .map_err(|error| DynamicHostError::new("InvalidControllerUpdate", error.to_string()))
    }

    fn controller_reset(&self) -> Result<(), DynamicHostError> {
        self.inner.lock().controller = ControllerState::NEUTRAL;
        Ok(())
    }

    fn record_diagnostic(&self, diagnostic: Diagnostic) {
        self.inner.lock().diagnostics.push(diagnostic);
    }

    fn record_output(&self, message: &str) {
        self.inner.lock().outputs.push(message.to_owned());
    }

    fn request_command_recompute(&self) {
        self.inner.lock().command_recompute_requests += 1;
    }
}

fn validate_known_cross_constraints(
    values: &BTreeMap<String, Value>,
) -> Result<(), DynamicHostError> {
    let integer = |id: &str| values.get(id).and_then(Value::as_u64);
    if let (Some(soft), Some(grace), Some(hard)) = (
        integer("dynamic.callback_soft_timeout_ms"),
        integer("dynamic.callback_soft_timeout_grace_ms"),
        integer("dynamic.callback_hard_timeout_ms"),
    ) && soft > 0
        && hard > 0
        && hard < soft.saturating_add(grace)
    {
        return Err(DynamicHostError::new(
            "InvalidSetting",
            "dynamic callback hard timeout must be at least soft plus grace",
        ));
    }
    if let (Some(fps), Some(options)) = (
        values.get("ui.fps").and_then(Value::as_i64),
        values.get("ui.fps_options").and_then(Value::as_array),
    ) && !options
        .iter()
        .any(|candidate| candidate.as_i64() == Some(fps))
    {
        return Err(DynamicHostError::new(
            "InvalidSetting",
            "ui.fps must be present in ui.fps_options",
        ));
    }
    Ok(())
}

fn validate_profile_name(name: &str) -> Result<(), DynamicHostError> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', '\0'])
        || (name.len() >= 2 && name.as_bytes()[1] == b':')
    {
        Err(DynamicHostError::new(
            "InvalidProfile",
            "profile name must be one safe path component",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn settings_are_atomic_normalized_and_secret_getters_are_masked() {
        let registry = DynamicSettingsRegistry::load().unwrap();
        let (id, value) = registry
            .normalize("pokecon.opt.camera.flip_mode", json!("BOTH"))
            .unwrap();
        assert_eq!(id, "camera.flip_mode");
        assert_eq!(value, json!("both"));
        assert_eq!(
            registry.public_dynamic_value(
                "notifications.discord.webhook_url",
                &json!("https://discord.com/api/webhooks/1/token")
            ),
            json!("********")
        );
    }

    #[tokio::test]
    async fn controller_validation_and_profile_names_have_no_partial_side_effects() {
        let host = InMemoryDynamicHost::new(
            BTreeMap::new(),
            BTreeMap::from([
                ("active_profile".to_owned(), json!("default")),
                ("available_profiles".to_owned(), json!(["default", "Other"])),
            ]),
        )
        .unwrap();
        let invalid = serde_json::from_value::<ControllerUpdate>(json!({
            "a": true,
            "left_stick": {"x": 1}
        }));
        assert!(invalid.is_err());
        assert_eq!(host.controller_state(), ControllerState::NEUTRAL);
        assert!(
            host.profile_switch_begin("../escape", &BTreeMap::new())
                .await
                .is_err()
        );
        assert_eq!(host.profile_current().unwrap(), "default");
    }

    #[test]
    fn state_merge_preserves_independent_nested_appends() {
        let before = json!([{
            "name": "Auto",
            "module_path": "Commands.Auto",
            "class_name": "Auto",
            "tags": ["base"],
        }]);
        let current = json!([{
            "name": "Auto",
            "module_path": "Commands.Auto",
            "class_name": "Auto",
            "tags": ["base", "@Python"],
        }]);
        let value = json!([{
            "name": "Auto",
            "module_path": "Commands.Auto",
            "class_name": "Auto",
            "tags": ["base", "@Lua"],
        }]);

        assert_eq!(
            merge_state_change(&before, &current, &value).unwrap(),
            json!([{
                "name": "Auto",
                "module_path": "Commands.Auto",
                "class_name": "Auto",
                "tags": ["base", "@Python", "@Lua"],
            }])
        );
    }

    #[test]
    fn state_merge_reports_same_leaf_conflicts() {
        let error = merge_state_change(
            &json!({"name": "before"}),
            &json!({"name": "first"}),
            &json!({"name": "second"}),
        )
        .unwrap_err();

        assert_eq!(error.code, "StateConflict");
        assert!(error.message.contains("$.name"));
    }
}
