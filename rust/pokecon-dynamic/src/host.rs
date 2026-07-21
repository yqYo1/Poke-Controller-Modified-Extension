use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;
use pokecon_contracts::model::{Setting, ValueSchema};
use pokecon_contracts::{ContractError, settings_registry};
use pokecon_device::controller::{ControllerState, ControllerUpdate};
use pokecon_settings::pipeline::SECRET_MASK;
use serde_json::Value;

use crate::callback::Diagnostic;

/// Host-operation failure exposed to both language bindings with identical
/// code and message semantics.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
#[error("{message}")]
pub struct DynamicHostError {
    pub code: &'static str,
    pub message: String,
}

impl DynamicHostError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Rust-main operations available to the isolated dynamic worker. Methods are
/// synchronous because Python and Lua expose synchronous public APIs; the IPC
/// adapter keeps transport I/O on independent Tokio tasks.
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
    /// Returns a profile validation, persistence, or host transport failure.
    fn profile_switch(&self, name: &str) -> Result<bool, DynamicHostError>;

    /// # Errors
    ///
    /// Returns a controller validation or host transport failure.
    fn controller_update(&self, update: ControllerUpdate) -> Result<(), DynamicHostError>;

    /// # Errors
    ///
    /// Returns a controller or host transport failure.
    fn controller_reset(&self) -> Result<(), DynamicHostError>;

    fn record_diagnostic(&self, diagnostic: Diagnostic);

    /// Requests one coalesced rebuild of all command display-list snapshots.
    fn request_command_recompute(&self) {}
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
    controller: ControllerState,
    diagnostics: Vec<Diagnostic>,
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
                controller: ControllerState::NEUTRAL,
                diagnostics: Vec::new(),
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
    pub fn command_recompute_requests(&self) -> u64 {
        self.inner.lock().command_recompute_requests
    }
}

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

    fn profile_current(&self) -> Result<String, DynamicHostError> {
        Ok(self.inner.lock().active_profile.clone())
    }

    fn profile_list(&self) -> Result<Vec<String>, DynamicHostError> {
        Ok(self.inner.lock().profiles.clone())
    }

    fn profile_switch(&self, name: &str) -> Result<bool, DynamicHostError> {
        validate_profile_name(name)?;
        let mut inner = self.inner.lock();
        if !inner.profiles.iter().any(|profile| profile == name) {
            return Ok(false);
        }
        name.clone_into(&mut inner.active_profile);
        inner
            .state
            .insert("active_profile".to_owned(), Value::String(name.to_owned()));
        Ok(true)
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

    #[test]
    fn controller_validation_and_profile_names_have_no_partial_side_effects() {
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
        assert!(host.profile_switch("../escape").is_err());
        assert_eq!(host.profile_current().unwrap(), "default");
    }
}
