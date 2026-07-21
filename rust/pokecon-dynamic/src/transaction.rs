use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use pokecon_device::controller::ControllerUpdate;
use serde_json::Value;

use crate::callback::{Callback, CallbackError, CallbackSettings};
use crate::event::{EventBus, EventError, HandlerId, RegistrationOptions};
use crate::host::{DynamicHost, DynamicHostError, DynamicSettingsRegistry};

/// Event mutation staged until a complete top-level/source/reload evaluation
/// commits.
pub enum StagedEventOperation {
    Install {
        handler_id: HandlerId,
        event: String,
        callback: Arc<dyn Callback>,
        options: RegistrationOptions,
        once: bool,
    },
    Off(HandlerId),
    Clear(String),
    Define(String),
}

impl std::fmt::Debug for StagedEventOperation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Install {
                handler_id,
                event,
                options,
                once,
                ..
            } => formatter
                .debug_struct("Install")
                .field("handler_id", handler_id)
                .field("event", event)
                .field("options", options)
                .field("once", once)
                .finish_non_exhaustive(),
            Self::Off(id) => formatter.debug_tuple("Off").field(id).finish(),
            Self::Clear(target) => formatter.debug_tuple("Clear").field(target).finish(),
            Self::Define(event) => formatter.debug_tuple("Define").field(event).finish(),
        }
    }
}

/// Transaction staging or commit failure.
#[derive(Debug, thiserror::Error)]
pub enum TransactionError {
    #[error(transparent)]
    Host(#[from] DynamicHostError),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error(transparent)]
    Callback(#[from] CallbackError),
    #[error("event name must be a non-empty string without NUL")]
    InvalidEventName,
    #[error("autocmd group name is reserved: {0}")]
    ReservedGroup(String),
}

/// One atomic dynamic evaluation. Host settings and callback/event mutations
/// remain invisible until `commit`; dropping this value is rollback.
pub struct EvaluationTransaction {
    settings_registry: Arc<DynamicSettingsRegistry>,
    baseline_settings: BTreeMap<String, Value>,
    prospective_settings: BTreeMap<String, Value>,
    changes: BTreeMap<String, Value>,
    event_operations: Vec<StagedEventOperation>,
    defined_events: BTreeSet<String>,
    reserved_event_names: BTreeSet<String>,
    pending_emits: Vec<String>,
}

impl std::fmt::Debug for EvaluationTransaction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EvaluationTransaction")
            .field("setting_change_count", &self.changes.len())
            .field("event_operation_count", &self.event_operations.len())
            .field("pending_emit_count", &self.pending_emits.len())
            .finish_non_exhaustive()
    }
}

impl EvaluationTransaction {
    /// Snapshots host settings and defined events at the dispatch barrier.
    ///
    /// # Errors
    ///
    /// Returns a host error when the settings snapshot is unavailable.
    pub fn begin(
        host: &dyn DynamicHost,
        settings_registry: Arc<DynamicSettingsRegistry>,
        event_bus: &EventBus,
    ) -> Result<Self, TransactionError> {
        let baseline_settings = host.settings_snapshot()?;
        let defined_events = event_bus
            .list_defined()
            .into_iter()
            .collect::<BTreeSet<_>>();
        Ok(Self {
            prospective_settings: baseline_settings.clone(),
            baseline_settings,
            changes: BTreeMap::new(),
            event_operations: Vec::new(),
            reserved_event_names: defined_events.clone(),
            defined_events,
            pending_emits: Vec::new(),
            settings_registry,
        })
    }

    /// Gets a staged/current canonical setting through its public dynamic path.
    ///
    /// # Errors
    ///
    /// Rejects unsupported dynamic setting paths.
    pub fn setting(&self, dynamic_path: &str) -> Result<Value, TransactionError> {
        let setting = self
            .settings_registry
            .setting(dynamic_path)
            .ok_or_else(|| {
                DynamicHostError::new(
                    "UnknownSetting",
                    format!("dynamic setting path is not supported: {dynamic_path}"),
                )
            })?;
        let value = self
            .prospective_settings
            .get(&setting.id)
            .cloned()
            .ok_or_else(|| {
                DynamicHostError::new(
                    "MissingSetting",
                    format!("setting snapshot is missing {}", setting.id),
                )
            })?;
        Ok(self
            .settings_registry
            .public_dynamic_value(&setting.id, &value))
    }

    /// Validates and stages one setting assignment without host side effects.
    ///
    /// # Errors
    ///
    /// Rejects unsupported paths, invalid values, and invalid callback timeout
    /// combinations.
    pub fn set_setting(
        &mut self,
        dynamic_path: &str,
        value: Value,
    ) -> Result<(), TransactionError> {
        let (id, value) = self.settings_registry.normalize(dynamic_path, value)?;
        let previous = self.prospective_settings.insert(id.clone(), value.clone());
        if let Err(error) = validate_callback_settings(&self.prospective_settings) {
            if let Some(previous) = previous {
                self.prospective_settings.insert(id.clone(), previous);
            } else {
                self.prospective_settings.remove(&id);
            }
            return Err(error.into());
        }
        if self.baseline_settings.get(&id) == Some(&value) {
            self.changes.remove(&id);
        } else {
            self.changes.insert(id, value);
        }
        Ok(())
    }

    /// Stages `on`/`once` and returns a never-reused ID immediately.
    ///
    /// # Errors
    ///
    /// Rejects invalid event names, reserved groups, and invalid effective
    /// timeout combinations.
    pub fn register(
        &mut self,
        event_bus: &EventBus,
        event: &str,
        callback: Arc<dyn Callback>,
        options: RegistrationOptions,
        once: bool,
    ) -> Result<HandlerId, TransactionError> {
        validate_event_name(event)?;
        if let Some(group) = &options.group
            && (group == "all" || self.reserved_event_names.contains(group))
        {
            return Err(TransactionError::ReservedGroup(group.clone()));
        }
        options
            .limits
            .resolve(callback_settings(&self.prospective_settings)?)?;
        let handler_id = event_bus.reserve_handler_id();
        self.reserved_event_names.insert(event.to_owned());
        self.event_operations.push(StagedEventOperation::Install {
            handler_id,
            event: event.to_owned(),
            callback,
            options,
            once,
        });
        Ok(handler_id)
    }

    pub fn off(&mut self, handler_id: HandlerId) {
        self.event_operations
            .push(StagedEventOperation::Off(handler_id));
    }

    pub fn clear(&mut self, target: impl Into<String>) {
        self.event_operations
            .push(StagedEventOperation::Clear(target.into()));
    }

    /// Stages an idempotent user event definition.
    ///
    /// # Errors
    ///
    /// Rejects an empty event name or one containing NUL.
    pub fn define(&mut self, event: &str) -> Result<(), TransactionError> {
        validate_event_name(event)?;
        if self.defined_events.insert(event.to_owned()) {
            self.event_operations
                .push(StagedEventOperation::Define(event.to_owned()));
        }
        self.reserved_event_names.insert(event.to_owned());
        Ok(())
    }

    #[must_use]
    pub fn list_defined(&self) -> Vec<String> {
        self.defined_events.iter().cloned().collect()
    }

    /// Buffers an event until the evaluation commits and callback dispatch is
    /// allowed to cross the barrier.
    ///
    /// # Errors
    ///
    /// Rejects undefined events without implicitly defining them.
    pub fn emit(&mut self, event: &str) -> Result<(), TransactionError> {
        validate_event_name(event)?;
        if !self.defined_events.contains(event) {
            return Err(EventError::UndefinedEvent(event.to_owned()).into());
        }
        self.pending_emits.push(event.to_owned());
        Ok(())
    }

    /// Commits settings, callback policy, event registry mutations, and finally
    /// dispatches events accepted behind the barrier.
    ///
    /// # Errors
    ///
    /// Returns a host or event failure. All validation occurs before host
    /// mutation; the worker treats a scheduler disconnect during commit as a
    /// terminal IPC failure rather than continuing with a partial generation.
    pub async fn commit(
        self,
        host: &dyn DynamicHost,
        event_bus: &EventBus,
    ) -> Result<(), TransactionError> {
        let pending_emits = self.commit_staged(host, event_bus).await?;
        for event in pending_emits {
            event_bus.emit(&event).await?;
        }
        Ok(())
    }

    /// Commits state and registry mutations without starting callbacks for
    /// buffered events. This lets the engine release its evaluation barrier
    /// only after a complete generation is visible.
    ///
    /// # Errors
    ///
    /// Returns a host or event failure. Validation completes before host
    /// mutation; a scheduler disconnect is terminal for the worker.
    pub async fn commit_staged(
        self,
        host: &dyn DynamicHost,
        event_bus: &EventBus,
    ) -> Result<Vec<String>, TransactionError> {
        let next_callback_settings = callback_settings(&self.prospective_settings)?;
        for operation in &self.event_operations {
            if let StagedEventOperation::Install { options, .. } = operation {
                options.limits.resolve(next_callback_settings)?;
            }
        }
        if !self.changes.is_empty() {
            host.apply_settings(&self.changes)?;
        }
        event_bus.update_settings(next_callback_settings).await?;
        for operation in self.event_operations {
            match operation {
                StagedEventOperation::Install {
                    handler_id,
                    event,
                    callback,
                    options,
                    once,
                } => {
                    event_bus.install(handler_id, &event, callback, options, once)?;
                }
                StagedEventOperation::Off(handler_id) => event_bus.off(handler_id),
                StagedEventOperation::Clear(target) => event_bus.clear(&target),
                StagedEventOperation::Define(event) => event_bus.define(&event)?,
            }
        }
        Ok(self.pending_emits)
    }
}

pub(crate) fn callback_settings(
    values: &BTreeMap<String, Value>,
) -> Result<CallbackSettings, DynamicHostError> {
    let u64_value = |id: &str| {
        values.get(id).and_then(Value::as_u64).ok_or_else(|| {
            DynamicHostError::new(
                "MissingSetting",
                format!("setting snapshot is missing {id}"),
            )
        })
    };
    let usize_value = |id: &str| {
        usize::try_from(u64_value(id)?).map_err(|_| {
            DynamicHostError::new("InvalidSetting", format!("setting is too large: {id}"))
        })
    };
    let settings = CallbackSettings {
        soft_timeout_ms: u64_value("dynamic.callback_soft_timeout_ms")?,
        soft_timeout_grace_ms: u64_value("dynamic.callback_soft_timeout_grace_ms")?,
        hard_timeout_ms: u64_value("dynamic.callback_hard_timeout_ms")?,
        max_concurrency: usize_value("dynamic.callback_max_concurrency")?,
        queue_capacity: usize_value("dynamic.callback_queue_capacity")?,
    };
    settings
        .validate()
        .map_err(|error| DynamicHostError::new("InvalidSetting", error.message))?;
    Ok(settings)
}

fn validate_callback_settings(values: &BTreeMap<String, Value>) -> Result<(), DynamicHostError> {
    callback_settings(values).map(|_| ())
}

fn validate_event_name(event: &str) -> Result<(), TransactionError> {
    if event.is_empty() || event.contains('\0') {
        Err(TransactionError::InvalidEventName)
    } else {
        Ok(())
    }
}

/// Validates controller updates before a host adapter sees any side effect.
///
/// # Errors
///
/// Rejects closed-shape, range, and mixed stick-form violations.
pub fn controller_update_from_value(value: Value) -> Result<ControllerUpdate, DynamicHostError> {
    serde_json::from_value(value).map_err(|error| {
        DynamicHostError::new(
            "InvalidControllerUpdate",
            format!("invalid controller update: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::callback::{CallbackError, CallbackReturn, InvocationContext, NoopDiagnosticSink};
    use crate::host::InMemoryDynamicHost;

    struct CountingCallback(Arc<AtomicUsize>);

    #[async_trait]
    impl Callback for CountingCallback {
        async fn invoke(
            &self,
            _context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(CallbackReturn::None)
        }
    }

    fn initial_settings() -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("language".to_owned(), json!("ja")),
            ("dynamic.callback_soft_timeout_ms".to_owned(), json!(2000)),
            (
                "dynamic.callback_soft_timeout_grace_ms".to_owned(),
                json!(1000),
            ),
            ("dynamic.callback_hard_timeout_ms".to_owned(), json!(5000)),
            ("dynamic.callback_max_concurrency".to_owned(), json!(8)),
            ("dynamic.callback_queue_capacity".to_owned(), json!(1024)),
        ])
    }

    #[tokio::test]
    async fn drop_rolls_back_and_commit_releases_buffered_events_after_settings() {
        let host = InMemoryDynamicHost::new(initial_settings(), BTreeMap::new()).unwrap();
        let registry = Arc::new(DynamicSettingsRegistry::load().unwrap());
        let bus = EventBus::new(CallbackSettings::default(), 3, Arc::new(NoopDiagnosticSink));
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let mut transaction =
                EvaluationTransaction::begin(&host, registry.clone(), &bus).unwrap();
            transaction
                .set_setting("pokecon.opt.language", json!("en"))
                .unwrap();
            transaction
                .register(
                    &bus,
                    "PluginReadyPost",
                    Arc::new(CountingCallback(calls.clone())),
                    RegistrationOptions::default(),
                    false,
                )
                .unwrap();
            transaction.define("PluginReadyPost").unwrap();
            transaction.emit("PluginReadyPost").unwrap();
        }
        assert_eq!(host.settings_snapshot().unwrap()["language"], json!("ja"));
        assert_eq!(calls.load(Ordering::Acquire), 0);

        let mut transaction = EvaluationTransaction::begin(&host, registry, &bus).unwrap();
        transaction
            .set_setting("pokecon.opt.language", json!("EN"))
            .unwrap();
        transaction
            .register(
                &bus,
                "PluginReadyPost",
                Arc::new(CountingCallback(calls.clone())),
                RegistrationOptions::default(),
                true,
            )
            .unwrap();
        transaction.define("PluginReadyPost").unwrap();
        transaction.emit("PluginReadyPost").unwrap();
        transaction.commit(&host, &bus).await.unwrap();
        assert_eq!(host.settings_snapshot().unwrap()["language"], json!("en"));
        assert_eq!(calls.load(Ordering::Acquire), 1);
        bus.emit("PluginReadyPost").await.unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn timeout_assignment_is_rejected_without_changing_prospective_value() {
        let host = InMemoryDynamicHost::new(initial_settings(), BTreeMap::new()).unwrap();
        let registry = Arc::new(DynamicSettingsRegistry::load().unwrap());
        let bus = EventBus::new(CallbackSettings::default(), 3, Arc::new(NoopDiagnosticSink));
        let mut transaction = EvaluationTransaction::begin(&host, registry, &bus).unwrap();
        let result =
            transaction.set_setting("pokecon.opt.dynamic.callback_hard_timeout_ms", json!(100));
        assert!(result.is_err());
        assert_eq!(
            transaction
                .setting("pokecon.opt.dynamic.callback_hard_timeout_ms")
                .unwrap(),
            json!(5000)
        );
    }
}
