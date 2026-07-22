use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pokecon_contracts::model::{Access, Mutability, Scope, Setting};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::lock::LockManager;
use crate::persistence::{PersistenceError, TomlStore};
use crate::pipeline::{
    LoadedSettings, PipelineError, ResolvedSettings, ResolvedValue, SECRET_MASK, SettingSource,
    normalize_value, public_value, validate_snapshot,
};
use crate::roots::{RootError, SafeComponent};
use crate::scaffold::{ScaffoldError, ScaffoldManager};

const CAMERA_TRANSACTION: &[&str] = &[
    "camera.device",
    "camera.capture_fps",
    "camera.capture_resolution",
];
const SERIAL_TRANSACTION: &[&str] = &["serial.port", "serial.baud_rate", "serial.data_format"];

/// Normative PATCH transaction class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchClass {
    Profile,
    Camera,
    Serial,
    Ordinary,
}

/// Optimistic-revision settings update request.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchRequest {
    #[serde(default)]
    pub expected_revision: Option<String>,
    pub values: BTreeMap<String, Value>,
}

impl std::fmt::Debug for PatchRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PatchRequest")
            .field("expected_revision", &self.expected_revision)
            .field("value_count", &self.values.len())
            .field("values", &"<redacted>")
            .finish()
    }
}

/// Secret-safe settings update response.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchResponse {
    pub revision: String,
    pub values: BTreeMap<String, Value>,
    pub pending_restart_values: BTreeMap<String, Value>,
    pub restart_required: bool,
    pub apply_failures: BTreeMap<String, String>,
}

/// Runtime side-effect boundary used by camera, serial, and ordinary settings.
pub trait RuntimeSettingsApplier: Send {
    /// Applies a validated set. Error text is intentionally discarded at the
    /// public boundary to prevent downstream secret leakage.
    ///
    /// # Errors
    ///
    /// Returns implementation-private diagnostic text when the runtime side
    /// effect cannot be applied. Callers must replace it with a fixed public
    /// diagnostic.
    fn apply(&mut self, class: PatchClass, changes: &BTreeMap<String, Value>)
    -> Result<(), String>;

    /// Restores a prior transaction after persistence fails.
    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>);
}

/// No-op runtime adapter for startup and isolated contract tests.
#[derive(Debug, Default)]
pub struct NoopSettingsApplier;

impl RuntimeSettingsApplier for NoopSettingsApplier {
    fn apply(
        &mut self,
        _class: PatchClass,
        _changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn rollback(&mut self, _class: PatchClass, _previous: &BTreeMap<String, Value>) {}
}

/// Canonical saved/current settings state and transactional persistence layer.
pub struct SettingsService {
    loaded: LoadedSettings,
    saved: BTreeMap<String, ResolvedValue>,
    current: BTreeMap<String, ResolvedValue>,
    revision: u64,
    switch_gate: Arc<AtomicBool>,
    applier: Box<dyn RuntimeSettingsApplier>,
}

impl std::fmt::Debug for SettingsService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SettingsService")
            .field("active_profile", &self.loaded.active_profile)
            .field("revision", &self.revision)
            .field("settings_count", &self.saved.len())
            .finish_non_exhaustive()
    }
}

impl SettingsService {
    /// Creates a service with revision zero and identical startup saved/current
    /// snapshots.
    #[must_use]
    pub fn new(loaded: LoadedSettings, applier: Box<dyn RuntimeSettingsApplier>) -> Self {
        let saved = loaded.settings.values().clone();
        Self {
            current: saved.clone(),
            loaded,
            saved,
            revision: 0,
            switch_gate: Arc::new(AtomicBool::new(false)),
            applier,
        }
    }

    /// Returns the common settings/state revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the currently active profile spelling exactly as supplied.
    #[must_use]
    pub fn active_profile(&self) -> &str {
        self.loaded.active_profile.as_str()
    }

    /// Returns a REST-safe current snapshot.
    #[must_use]
    pub fn public_snapshot(&self) -> BTreeMap<String, Value> {
        public_snapshot(&self.loaded.settings, &self.current)
    }

    /// Returns the complete REST-safe snapshot represented by this service.
    ///
    /// The revision is local to the settings service. Application composition
    /// layers that share a process-wide revision with other state domains must
    /// replace it with the revision produced by their common transaction gate.
    #[must_use]
    pub fn public_response(&self) -> PatchResponse {
        self.response(BTreeMap::new())
    }

    /// Adopts a profile or dynamic-runtime snapshot that was committed by the
    /// application host outside the `OpenAPI` PATCH path.
    ///
    /// Startup-only values retain the process-start value while all other
    /// current and saved values switch atomically to the supplied generation.
    pub fn adopt_loaded(&mut self, loaded: LoadedSettings) {
        let previous_current = self.current.clone();
        let saved = loaded.settings.values().clone();
        let mut current = saved.clone();
        for setting in &loaded.settings.registry().settings {
            if setting.mutability == Mutability::StartupOnly
                && let Some(previous) = previous_current.get(&setting.id)
            {
                current.insert(setting.id.clone(), previous.clone());
            }
        }
        self.loaded = loaded;
        self.saved = saved;
        self.current = current;
        self.bump_revision();
    }

    /// Acquires the non-recursive profile-switch gate for an external switch
    /// transaction. The guard is useful to reject concurrent UI/OpenAPI writes
    /// rather than queueing them against the wrong profile.
    ///
    /// # Errors
    ///
    /// Returns [`PatchError::ProfileSwitchInProgress`] if already held.
    pub fn begin_profile_switch(&self) -> Result<ProfileSwitchGuard, PatchError> {
        acquire_switch_gate(&self.switch_gate)
    }

    /// Validates, persists, applies, and revisions one normative PATCH.
    ///
    /// # Errors
    ///
    /// Returns a status-classified, secret-safe error. Validation completes
    /// before revision gating and no failing request performs a partial save.
    pub fn patch(&mut self, request: &PatchRequest) -> Result<PatchResponse, PatchError> {
        let expected_revision = request
            .expected_revision
            .as_deref()
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| PatchError::InvalidRevision)?;
        let class = self.classify_and_validate_ids(request.values.keys())?;
        let mut normalized = self.normalize_changes(&request.values)?;
        let mut prospective = self.saved.clone();
        for (id, value) in &normalized {
            prospective.insert(
                id.clone(),
                ResolvedValue {
                    value: value.clone(),
                    source: SettingSource::OpenApi,
                },
            );
        }
        validate_snapshot(&prospective)?;
        if let Some(expected) = expected_revision
            && expected != self.revision
        {
            return Err(PatchError::RevisionMismatch {
                expected,
                actual: self.revision,
            });
        }
        normalized.retain(|id, value| {
            self.saved
                .get(id)
                .is_none_or(|resolved| resolved.value != *value)
        });
        if normalized.is_empty() {
            return Ok(self.response(BTreeMap::new()));
        }
        match class {
            PatchClass::Profile => self.patch_profile(&normalized),
            PatchClass::Camera | PatchClass::Serial => {
                self.patch_runtime_transaction(class, &normalized)
            }
            PatchClass::Ordinary => self.patch_ordinary(&normalized),
        }
    }

    fn classify_and_validate_ids<'a>(
        &self,
        ids: impl Iterator<Item = &'a String>,
    ) -> Result<PatchClass, PatchError> {
        let ids = ids.map(String::as_str).collect::<BTreeSet<_>>();
        if ids.is_empty() {
            return Ok(PatchClass::Ordinary);
        }
        let registry = self.loaded.settings.registry();
        for id in &ids {
            let setting = registry
                .settings
                .iter()
                .find(|setting| setting.id == *id)
                .ok_or_else(|| PatchError::UnknownSetting((*id).to_owned()))?;
            if setting.scope == Scope::Bootstrap {
                return Err(PatchError::BootstrapSetting((*id).to_owned()));
            }
            if !matches!(
                setting.surfaces.openapi.access,
                Access::Write | Access::ReadWrite
            ) {
                return Err(PatchError::NotWritable((*id).to_owned()));
            }
        }
        if ids.contains("active_profile") {
            return (ids.len() == 1)
                .then_some(PatchClass::Profile)
                .ok_or(PatchError::MixedTransactionClasses);
        }
        let camera = ids.iter().any(|id| CAMERA_TRANSACTION.contains(id));
        let serial = ids.iter().any(|id| SERIAL_TRANSACTION.contains(id));
        if camera {
            return ids
                .iter()
                .all(|id| CAMERA_TRANSACTION.contains(id))
                .then_some(PatchClass::Camera)
                .ok_or(PatchError::MixedTransactionClasses);
        }
        if serial {
            return ids
                .iter()
                .all(|id| SERIAL_TRANSACTION.contains(id))
                .then_some(PatchClass::Serial)
                .ok_or(PatchError::MixedTransactionClasses);
        }
        let scopes = ids
            .iter()
            .map(|id| {
                registry
                    .settings
                    .iter()
                    .find(|setting| setting.id == *id)
                    .expect("IDs were validated")
                    .scope
            })
            .collect::<BTreeSet<_>>();
        if scopes.len() != 1 {
            return Err(PatchError::MixedPersistenceDestinations);
        }
        if scopes.contains(&Scope::Profile) && self.switch_gate.load(Ordering::Acquire) {
            return Err(PatchError::ProfileSwitchInProgress);
        }
        Ok(PatchClass::Ordinary)
    }

    fn normalize_changes(
        &self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, PatchError> {
        let mut normalized = BTreeMap::new();
        for (id, raw) in changes {
            let setting = self.setting(id)?;
            if setting.secret && raw.as_str() == Some(SECRET_MASK) {
                let configured = self
                    .saved
                    .get(id)
                    .and_then(|value| value.value.as_str())
                    .is_some_and(|value| !value.is_empty());
                if configured {
                    continue;
                }
            }
            normalized.insert(
                id.clone(),
                normalize_value(
                    setting,
                    raw.clone(),
                    SettingSource::OpenApi,
                    &self.loaded.roots,
                    &self.loaded.recipe.request,
                )?,
            );
        }
        Ok(normalized)
    }

    fn patch_profile(
        &mut self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<PatchResponse, PatchError> {
        let target = changes
            .get("active_profile")
            .and_then(Value::as_str)
            .ok_or_else(|| PatchError::InvalidSetting("active_profile".to_owned()))?;
        let target = SafeComponent::new(target)?;
        let _gate = acquire_switch_gate(&self.switch_gate)?;
        let previous = self.loaded.active_profile.clone();
        ScaffoldManager::new(self.loaded.roots.clone()).ensure(target.as_str())?;
        self.persist(changes, Scope::Global)?;
        if let Err(error) = self.loaded.recipe.refresh_global() {
            self.rollback_profile(&previous)?;
            return Err(PatchError::Pipeline(error));
        }
        let next = match self.loaded.recipe.resolve_profile(target.as_str()) {
            Ok(next) => next,
            Err(error) => {
                self.rollback_profile(&previous)?;
                return Err(PatchError::Pipeline(error));
            }
        };
        let previous_current = self.current.clone();
        self.loaded.active_profile = target;
        self.loaded.profile_settings_path = self
            .loaded
            .roots
            .profile_settings(self.loaded.active_profile.as_str())?;
        self.loaded.settings = next;
        self.saved = self.loaded.settings.values().clone();
        self.current.clone_from(&self.saved);
        for setting in &self.loaded.settings.registry().settings {
            if setting.mutability == Mutability::StartupOnly
                && let Some(previous) = previous_current.get(&setting.id)
            {
                self.current.insert(setting.id.clone(), previous.clone());
            }
        }
        self.bump_revision();
        Ok(self.response(BTreeMap::new()))
    }

    fn rollback_profile(&mut self, previous: &SafeComponent) -> Result<(), PatchError> {
        let rollback = BTreeMap::from([(
            "active_profile".to_owned(),
            Value::String(previous.as_str().to_owned()),
        )]);
        self.persist(&rollback, Scope::Global)
            .map_err(|_| PatchError::ProfileRollback)?;
        self.loaded
            .recipe
            .refresh_global()
            .map_err(|_| PatchError::ProfileRollback)
    }

    fn patch_runtime_transaction(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<PatchResponse, PatchError> {
        let previous = changes
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    self.current
                        .get(id)
                        .expect("validated setting must exist")
                        .value
                        .clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        self.applier
            .apply(class, changes)
            .map_err(|_| PatchError::DeviceConflict)?;
        if self.persist(changes, Scope::Global).is_err() {
            self.applier.rollback(class, &previous);
            return Err(PatchError::DeviceConflict);
        }
        self.commit_saved_and_current(changes);
        self.loaded.recipe.refresh_global()?;
        self.bump_revision();
        Ok(self.response(BTreeMap::new()))
    }

    fn patch_ordinary(
        &mut self,
        changes: &BTreeMap<String, Value>,
    ) -> Result<PatchResponse, PatchError> {
        let scope = changes
            .keys()
            .next()
            .map(|id| self.setting(id).map(|setting| setting.scope))
            .transpose()?
            .unwrap_or(Scope::Global);
        self.persist(changes, scope)?;
        for (id, value) in changes {
            self.saved.insert(
                id.clone(),
                ResolvedValue {
                    value: value.clone(),
                    source: SettingSource::OpenApi,
                },
            );
        }
        let mut failures = BTreeMap::new();
        for (id, value) in changes {
            let setting = self.setting(id)?;
            match setting.mutability {
                Mutability::StartupOnly => {}
                Mutability::RuntimeDeferred => {
                    self.current.insert(
                        id.clone(),
                        ResolvedValue {
                            value: value.clone(),
                            source: SettingSource::OpenApi,
                        },
                    );
                }
                Mutability::RuntimeImmediate => {
                    let one = BTreeMap::from([(id.clone(), value.clone())]);
                    if self.applier.apply(PatchClass::Ordinary, &one).is_err() {
                        failures.insert(id.clone(), "runtime apply failed".to_owned());
                    }
                    self.current.insert(
                        id.clone(),
                        ResolvedValue {
                            value: value.clone(),
                            source: SettingSource::OpenApi,
                        },
                    );
                }
            }
        }
        if scope == Scope::Global {
            self.loaded.recipe.refresh_global()?;
        }
        self.bump_revision();
        Ok(self.response(failures))
    }

    fn persist(&self, changes: &BTreeMap<String, Value>, scope: Scope) -> Result<(), PatchError> {
        let path = match scope {
            Scope::Global => &self.loaded.global_settings_path,
            Scope::Profile => &self.loaded.profile_settings_path,
            Scope::Bootstrap => return Err(PatchError::BootstrapSetting("bootstrap".to_owned())),
        };
        let updates = changes
            .iter()
            .map(|(id, value)| {
                let toml_path = self
                    .setting(id)?
                    .surfaces
                    .toml
                    .name
                    .clone()
                    .ok_or_else(|| PatchError::NotWritable(id.clone()))?;
                Ok((toml_path, value.clone()))
            })
            .collect::<Result<Vec<_>, PatchError>>()?;
        TomlStore::new(LockManager::new(&self.loaded.roots)).update(path, &updates)?;
        Ok(())
    }

    fn commit_saved_and_current(&mut self, changes: &BTreeMap<String, Value>) {
        for (id, value) in changes {
            let resolved = ResolvedValue {
                value: value.clone(),
                source: SettingSource::OpenApi,
            };
            self.saved.insert(id.clone(), resolved.clone());
            self.current.insert(id.clone(), resolved);
        }
    }

    fn setting(&self, id: &str) -> Result<&Setting, PatchError> {
        self.loaded
            .settings
            .registry()
            .settings
            .iter()
            .find(|setting| setting.id == id)
            .ok_or_else(|| PatchError::UnknownSetting(id.to_owned()))
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn response(&self, apply_failures: BTreeMap<String, String>) -> PatchResponse {
        let pending_restart_values = self
            .loaded
            .settings
            .registry()
            .settings
            .iter()
            .filter(|setting| setting.mutability == Mutability::StartupOnly)
            .filter_map(|setting| {
                let saved = self.saved.get(&setting.id)?;
                let current = self.current.get(&setting.id)?;
                (saved.value != current.value)
                    .then(|| (setting.id.clone(), public_value(setting, &saved.value)))
            })
            .collect::<BTreeMap<_, _>>();
        PatchResponse {
            revision: self.revision.to_string(),
            values: public_snapshot(&self.loaded.settings, &self.saved),
            restart_required: !pending_restart_values.is_empty(),
            pending_restart_values,
            apply_failures,
        }
    }
}

fn public_snapshot(
    settings: &ResolvedSettings,
    values: &BTreeMap<String, ResolvedValue>,
) -> BTreeMap<String, Value> {
    settings
        .registry()
        .settings
        .iter()
        .filter_map(|setting| {
            values
                .get(&setting.id)
                .map(|resolved| (setting.id.clone(), public_value(setting, &resolved.value)))
        })
        .collect()
}

fn acquire_switch_gate(gate: &Arc<AtomicBool>) -> Result<ProfileSwitchGuard, PatchError> {
    gate.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| PatchError::ProfileSwitchInProgress)?;
    Ok(ProfileSwitchGuard {
        gate: Arc::clone(gate),
    })
}

/// RAII profile-switch gate.
#[derive(Debug)]
pub struct ProfileSwitchGuard {
    gate: Arc<AtomicBool>,
}

impl Drop for ProfileSwitchGuard {
    fn drop(&mut self) {
        self.gate.store(false, Ordering::Release);
    }
}

/// PATCH validation/transaction failures and their HTTP-equivalent class.
#[derive(Debug, Error)]
pub enum PatchError {
    #[error("expected_revision must be an unsigned decimal string")]
    InvalidRevision,
    #[error("settings revision mismatch: expected {expected}, current {actual}")]
    RevisionMismatch { expected: u64, actual: u64 },
    #[error("unknown canonical setting {0}")]
    UnknownSetting(String),
    #[error("bootstrap setting is not PATCH-writable: {0}")]
    BootstrapSetting(String),
    #[error("setting is not OpenAPI-writable: {0}")]
    NotWritable(String),
    #[error("PATCH mixes transaction classes")]
    MixedTransactionClasses,
    #[error("ordinary PATCH mixes global and profile persistence destinations")]
    MixedPersistenceDestinations,
    #[error("profile switch is in progress")]
    ProfileSwitchInProgress,
    #[error("invalid setting {0}")]
    InvalidSetting(String),
    #[error("transactional runtime apply failed")]
    DeviceConflict,
    #[error("profile rollback failed after a switch error")]
    ProfileRollback,
    #[error(transparent)]
    Pipeline(#[from] PipelineError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Root(#[from] RootError),
    #[error(transparent)]
    Scaffold(#[from] ScaffoldError),
}

impl PatchError {
    /// HTTP status code required by the service contract.
    #[must_use]
    pub const fn status_code(&self) -> u16 {
        match self {
            Self::RevisionMismatch { .. }
            | Self::ProfileSwitchInProgress
            | Self::DeviceConflict => 409,
            Self::Persistence(_) | Self::ProfileRollback | Self::Scaffold(_) => 500,
            Self::InvalidRevision
            | Self::UnknownSetting(_)
            | Self::BootstrapSetting(_)
            | Self::NotWritable(_)
            | Self::MixedTransactionClasses
            | Self::MixedPersistenceDestinations
            | Self::InvalidSetting(_)
            | Self::Pipeline(_)
            | Self::Root(_) => 422,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::sync::{Arc, Mutex};

    use serde_json::json;
    use tempfile::TempDir;

    use super::{PatchClass, PatchRequest, RuntimeSettingsApplier, SettingsService};
    use crate::pipeline::{PipelineRequest, SettingsPipeline};
    use crate::roots::{BaseDirectories, RootEnvironment};

    type RecordedCall = (PatchClass, BTreeMap<String, serde_json::Value>);

    #[derive(Clone, Debug, Default)]
    struct RecordingApplier {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
        fail_ids: Arc<Mutex<Vec<String>>>,
        rollbacks: Arc<Mutex<usize>>,
    }

    impl RuntimeSettingsApplier for RecordingApplier {
        fn apply(
            &mut self,
            class: PatchClass,
            changes: &BTreeMap<String, serde_json::Value>,
        ) -> Result<(), String> {
            self.calls
                .lock()
                .expect("calls lock must work")
                .push((class, changes.clone()));
            if changes.keys().any(|id| {
                self.fail_ids
                    .lock()
                    .expect("fail lock must work")
                    .contains(id)
            }) {
                Err("deliberately secret-looking downstream error".to_owned())
            } else {
                Ok(())
            }
        }

        fn rollback(
            &mut self,
            _class: PatchClass,
            _previous: &BTreeMap<String, serde_json::Value>,
        ) {
            *self.rollbacks.lock().expect("rollback lock must work") += 1;
        }
    }

    fn service(temp: &TempDir, applier: RecordingApplier) -> SettingsService {
        let base = temp.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("bases must resolve");
        let loaded = SettingsPipeline::new(PipelineRequest {
            arguments: vec![OsString::from("pokecon")],
            environment: RootEnvironment::from_values([("HOME", base.as_os_str())]),
            startup_cwd: base.to_path_buf(),
            resource_root: base.join("resources"),
            base_directories: Some(bases),
            dynamic_values: BTreeMap::new(),
        })
        .load()
        .expect("settings must load");
        SettingsService::new(loaded, Box::new(applier))
    }

    fn patch(
        service: &mut SettingsService,
        values: BTreeMap<String, serde_json::Value>,
    ) -> super::PatchResponse {
        service
            .patch(&PatchRequest {
                expected_revision: Some(service.revision().to_string()),
                values,
            })
            .expect("patch must succeed")
    }

    #[test]
    fn startup_only_values_are_saved_but_not_made_current() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        let response = patch(
            &mut service,
            BTreeMap::from([("server.port".to_owned(), json!(9000))]),
        );
        assert_eq!(response.values["server.port"], json!(9000));
        assert_eq!(response.pending_restart_values["server.port"], json!(9000));
        assert!(response.restart_required);
        assert_eq!(service.revision(), 1);
    }

    #[test]
    fn profile_switch_preserves_pending_startup_only_values() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        patch(
            &mut service,
            BTreeMap::from([("server.port".to_owned(), json!(9000))]),
        );
        let response = patch(
            &mut service,
            BTreeMap::from([("active_profile".to_owned(), json!("Second"))]),
        );
        assert_eq!(service.active_profile(), "Second");
        assert_eq!(response.pending_restart_values["server.port"], json!(9000));
        assert!(response.restart_required);
        assert_eq!(service.revision(), 2);
    }

    #[test]
    fn externally_committed_profile_snapshot_preserves_process_start_values() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        patch(
            &mut service,
            BTreeMap::from([("server.port".to_owned(), json!(9000))]),
        );
        std::fs::create_dir_all(service.loaded.roots.config.join("profiles/Second"))
            .expect("target profile must exist");
        let loaded = service
            .loaded
            .switch_profile_in_memory("Second")
            .expect("target profile must resolve");

        service.adopt_loaded(loaded);

        let response = service.public_response();
        assert_eq!(service.active_profile(), "Second");
        assert_eq!(response.values["active_profile"], json!("Second"));
        assert_eq!(response.values["server.port"], json!(9000));
        assert_eq!(response.pending_restart_values["server.port"], json!(9000));
    }

    #[test]
    fn secret_mask_is_a_revision_preserving_round_trip_and_plaintext_never_returns() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        let response = patch(
            &mut service,
            BTreeMap::from([(
                "notifications.discord.webhook_url".to_owned(),
                json!("https://discord.com/api/webhooks/123/example_token"),
            )]),
        );
        assert_eq!(
            response.values["notifications.discord.webhook_url"],
            json!({"configured": true})
        );
        let revision = service.revision();
        let response = patch(
            &mut service,
            BTreeMap::from([(
                "notifications.discord.webhook_url".to_owned(),
                json!("********"),
            )]),
        );
        assert_eq!(service.revision(), revision);
        assert_eq!(
            response.values["notifications.discord.webhook_url"],
            json!({"configured": true})
        );
        assert!(!format!("{response:?}").contains("example_token"));
    }

    #[test]
    fn revision_and_transaction_class_failures_leave_state_unchanged() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        let mixed = service.patch(&PatchRequest {
            expected_revision: Some("0".to_owned()),
            values: BTreeMap::from([
                ("camera.capture_fps".to_owned(), json!(30)),
                ("serial.baud_rate".to_owned(), json!(115_200)),
            ]),
        });
        assert_eq!(
            mixed.expect_err("mixed request must fail").status_code(),
            422
        );
        let mismatch = service.patch(&PatchRequest {
            expected_revision: Some("99".to_owned()),
            values: BTreeMap::from([("camera.capture_fps".to_owned(), json!(30))]),
        });
        assert_eq!(mismatch.expect_err("revision must fail").status_code(), 409);
        assert_eq!(service.revision(), 0);
    }

    #[test]
    fn profile_gate_rejects_profile_writes_but_allows_independent_global_writes() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        let gate = service
            .begin_profile_switch()
            .expect("profile gate must be acquired");
        let blocked = service.patch(&PatchRequest {
            expected_revision: Some("0".to_owned()),
            values: BTreeMap::from([("ui.fps".to_owned(), json!(15))]),
        });
        assert_eq!(
            blocked.expect_err("profile write must fail").status_code(),
            409
        );
        let global = patch(
            &mut service,
            BTreeMap::from([("auto_reload_config".to_owned(), json!(true))]),
        );
        assert_eq!(global.revision, "1");
        drop(gate);
    }

    #[test]
    fn ordinary_apply_failure_keeps_saved_snapshot_and_reports_fixed_error() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let applier = RecordingApplier::default();
        applier
            .fail_ids
            .lock()
            .expect("fail lock must work")
            .push("auto_reload_config".to_owned());
        let mut service = service(&temp, applier);
        let response = patch(
            &mut service,
            BTreeMap::from([("auto_reload_config".to_owned(), json!(true))]),
        );
        assert_eq!(response.values["auto_reload_config"], json!(true));
        assert_eq!(
            response.apply_failures["auto_reload_config"],
            "runtime apply failed"
        );
    }

    #[test]
    fn omitted_revision_is_last_write_wins_but_cross_setting_validation_is_atomic() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let mut service = service(&temp, RecordingApplier::default());
        let response = service
            .patch(&PatchRequest {
                expected_revision: None,
                values: BTreeMap::from([("ui.fps".to_owned(), json!(15))]),
            })
            .expect("an omitted revision must retain last-write-wins compatibility");
        assert_eq!(response.revision, "1");

        let invalid = service.patch(&PatchRequest {
            expected_revision: None,
            values: BTreeMap::from([("ui.fps".to_owned(), json!(7))]),
        });
        assert_eq!(
            invalid
                .expect_err("fps outside the configured options must fail")
                .status_code(),
            422
        );
        assert_eq!(service.revision(), 1);
        assert_eq!(service.public_snapshot()["ui.fps"], json!(15));
    }

    #[test]
    fn camera_transaction_apply_failure_is_a_conflict_without_revision_change() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let applier = RecordingApplier::default();
        applier
            .fail_ids
            .lock()
            .expect("fail lock must work")
            .push("camera.capture_fps".to_owned());
        let mut service = service(&temp, applier);
        let failure = service.patch(&PatchRequest {
            expected_revision: Some("0".to_owned()),
            values: BTreeMap::from([("camera.capture_fps".to_owned(), json!(30))]),
        });
        assert_eq!(
            failure
                .expect_err("device transaction must fail")
                .status_code(),
            409
        );
        assert_eq!(service.revision(), 0);
    }

    #[test]
    fn patch_request_debug_never_contains_plaintext_values() {
        let secret = "https://example.invalid/private-token";
        let request = PatchRequest {
            expected_revision: Some("0".to_owned()),
            values: BTreeMap::from([(
                "notifications.discord.webhook_url".to_owned(),
                json!(secret),
            )]),
        };
        assert!(!format!("{request:?}").contains(secret));
    }
}
