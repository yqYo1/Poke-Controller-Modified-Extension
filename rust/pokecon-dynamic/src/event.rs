use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::callback::{
    Callback, CallbackError, CallbackExecutor, CallbackLimits, CallbackOutcome, CallbackReturnKey,
    CallbackSettings, Diagnostic, DiagnosticLevel, DiagnosticSink, Invocation,
};

/// Stable identity shared by event and command callbacks.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HandlerId(u64);

impl HandlerId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Canonical built-in event metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinEvent {
    pub name: &'static str,
    pub cancellable: bool,
}

pub const BUILTIN_EVENTS: &[BuiltinEvent] = &[
    BuiltinEvent {
        name: "AppStartupPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "AppShutdownPre",
        cancellable: false,
    },
    BuiltinEvent {
        name: "SerialConnectPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "SerialDisconnectPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "SerialDisconnectPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "CameraOpenPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "CameraClosePre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "CameraClosePost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "CommandStartPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "CommandStartPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "CommandStopPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "CommandStopPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "CommandErrorPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "CommandErrorPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "ScriptLoadPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "ScriptLoadPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "ConfigReloadPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "ConfigReloadPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "InputPressedPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "InputReleasedPost",
        cancellable: false,
    },
    BuiltinEvent {
        name: "ProfileSwitchPre",
        cancellable: true,
    },
    BuiltinEvent {
        name: "ProfileSwitchPost",
        cancellable: false,
    },
];

/// Registration options common to Python keyword arguments and Lua tables.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegistrationOptions {
    pub group: Option<String>,
    pub priority: i32,
    pub limits: CallbackLimits,
}

#[derive(Clone)]
struct Registration {
    id: HandlerId,
    event: String,
    group: Option<String>,
    priority: i32,
    limits: CallbackLimits,
    once: bool,
    order: u64,
    callback: Arc<dyn Callback>,
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Registration")
            .field("id", &self.id)
            .field("event", &self.event)
            .field("group", &self.group)
            .field("priority", &self.priority)
            .field("limits", &self.limits)
            .field("once", &self.once)
            .field("order", &self.order)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Registry {
    defined: BTreeSet<String>,
    custom: BTreeSet<String>,
    registrations: BTreeMap<HandlerId, Registration>,
    next_handler: u64,
    next_registration_order: u64,
    settings: CallbackSettings,
}

impl Registry {
    fn new(settings: CallbackSettings, first_handler_id: u64) -> Self {
        Self {
            defined: BUILTIN_EVENTS
                .iter()
                .map(|event| event.name.to_owned())
                .collect(),
            custom: BTreeSet::new(),
            registrations: BTreeMap::new(),
            next_handler: first_handler_id,
            next_registration_order: 0,
            settings,
        }
    }
}

/// Event dispatch result. `cancelled` is true only for cancellable Pre events
/// whose callback returned the exact boolean value `false`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventResult {
    pub cancelled: bool,
    pub outcomes: Vec<(HandlerId, CallbackOutcome)>,
}

/// Event registry or dispatch failure.
#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
pub enum EventError {
    #[error("event name must be a non-empty string without NUL")]
    InvalidEventName,
    #[error("event is not defined: {0}")]
    UndefinedEvent(String),
    #[error("autocmd group name is reserved: {0}")]
    ReservedGroup(String),
    #[error("direct recursive event emission was ignored: {0}")]
    DirectRecursion(String),
    #[error(transparent)]
    Callback(#[from] CallbackError),
}

/// Shared dynamic event registry and dispatcher.
#[derive(Clone)]
pub struct EventBus {
    registry: Arc<Mutex<Registry>>,
    executor: CallbackExecutor,
    diagnostics: Arc<dyn DiagnosticSink>,
    next_event_sequence: Arc<AtomicU64>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let registry = self.registry.lock();
        formatter
            .debug_struct("EventBus")
            .field("defined_count", &registry.defined.len())
            .field("registration_count", &registry.registrations.len())
            .finish_non_exhaustive()
    }
}

impl EventBus {
    /// Creates a bus and reserves all IDs below `first_handler_id` for stable
    /// internal callbacks such as command sorting and tag matching.
    #[must_use]
    pub fn new(
        settings: CallbackSettings,
        first_handler_id: u64,
        diagnostics: Arc<dyn DiagnosticSink>,
    ) -> Self {
        let executor = CallbackExecutor::new(settings, diagnostics.clone());
        Self {
            registry: Arc::new(Mutex::new(Registry::new(settings, first_handler_id))),
            executor,
            diagnostics,
            next_event_sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Registers a persistent callback. Undefined raw event names are valid and
    /// remain pending until `define` is called.
    ///
    /// # Errors
    ///
    /// Rejects invalid event names, reserved groups, and invalid timeouts.
    pub fn on(
        &self,
        event: &str,
        callback: Arc<dyn Callback>,
        options: RegistrationOptions,
    ) -> Result<HandlerId, EventError> {
        self.register(event, callback, options, false)
    }

    /// Registers a callback removed atomically when its first invocation
    /// actually starts.
    ///
    /// # Errors
    ///
    /// Rejects invalid event names, reserved groups, and invalid timeouts.
    pub fn once(
        &self,
        event: &str,
        callback: Arc<dyn Callback>,
        options: RegistrationOptions,
    ) -> Result<HandlerId, EventError> {
        self.register(event, callback, options, true)
    }

    fn register(
        &self,
        event: &str,
        callback: Arc<dyn Callback>,
        options: RegistrationOptions,
        once: bool,
    ) -> Result<HandlerId, EventError> {
        validate_event_name(event)?;
        let mut registry = self.registry.lock();
        options.limits.resolve(registry.settings)?;
        if let Some(group) = &options.group
            && (group == "all"
                || registry.defined.contains(group)
                || registry
                    .registrations
                    .values()
                    .any(|registration| registration.event == *group))
        {
            return Err(EventError::ReservedGroup(group.clone()));
        }
        let id = HandlerId(registry.next_handler);
        registry.next_handler = registry.next_handler.wrapping_add(1);
        let order = registry.next_registration_order;
        registry.next_registration_order = registry.next_registration_order.wrapping_add(1);
        registry.registrations.insert(
            id,
            Registration {
                id,
                event: event.to_owned(),
                group: options.group,
                priority: options.priority,
                limits: options.limits,
                once,
                order,
                callback,
            },
        );
        Ok(id)
    }

    /// Removes one handler. Unknown and already-started once IDs are no-ops.
    pub fn off(&self, handler_id: HandlerId) {
        self.registry.lock().registrations.remove(&handler_id);
    }

    /// Clears all handlers, all handlers for a known event, or one group.
    pub fn clear(&self, target: &str) {
        let mut registry = self.registry.lock();
        if target == "all" {
            registry.registrations.clear();
            return;
        }
        let event_target = registry.defined.contains(target)
            || registry
                .registrations
                .values()
                .any(|registration| registration.event == target);
        registry.registrations.retain(|_, registration| {
            if event_target {
                registration.event != target
            } else {
                registration.group.as_deref() != Some(target)
            }
        });
    }

    /// Idempotently defines a user event and activates any pending handlers.
    ///
    /// # Errors
    ///
    /// Rejects an empty event name or one containing NUL.
    pub fn define(&self, event: &str) -> Result<(), EventError> {
        validate_event_name(event)?;
        let mut registry = self.registry.lock();
        registry.defined.insert(event.to_owned());
        if !BUILTIN_EVENTS.iter().any(|builtin| builtin.name == event) {
            registry.custom.insert(event.to_owned());
        }
        Ok(())
    }

    /// Returns built-in and explicitly defined user events in stable lexical
    /// order. Pending-only names are intentionally absent.
    #[must_use]
    pub fn list_defined(&self) -> Vec<String> {
        self.registry.lock().defined.iter().cloned().collect()
    }

    /// Validates every registration and then atomically updates global timeout
    /// and capacity settings.
    ///
    /// # Errors
    ///
    /// Rejects invalid global settings or a registration whose inherited
    /// effective timeout combination would become invalid.
    pub async fn update_settings(&self, settings: CallbackSettings) -> Result<(), EventError> {
        settings.validate()?;
        {
            let registry = self.registry.lock();
            for registration in registry.registrations.values() {
                registration.limits.resolve(settings)?;
            }
        }
        self.executor.update_settings(settings).await?;
        self.registry.lock().settings = settings;
        Ok(())
    }

    /// Emits a defined event and waits for logical completion of its callback
    /// snapshot. Callback arguments are always empty.
    ///
    /// # Errors
    ///
    /// Rejects invalid or undefined event names and reports scheduler failure.
    pub async fn emit(&self, event: &str) -> Result<EventResult, EventError> {
        validate_event_name(event)?;
        let (registrations, cancellable) = {
            let registry = self.registry.lock();
            if !registry.defined.contains(event) {
                return Err(EventError::UndefinedEvent(event.to_owned()));
            }
            let mut registrations = registry
                .registrations
                .values()
                .filter(|registration| registration.event == event)
                .cloned()
                .collect::<Vec<_>>();
            registrations.sort_by_key(|registration| registration.order);
            let cancellable = BUILTIN_EVENTS
                .iter()
                .find(|builtin| builtin.name == event)
                .is_some_and(|builtin| builtin.cancellable);
            (registrations, cancellable)
        };
        let sequence = self.next_event_sequence.fetch_add(1, Ordering::Relaxed);
        let invocations = registrations
            .iter()
            .map(|registration| {
                let registry = self.registry.clone();
                let id = registration.id;
                let on_start = registration.once.then(|| {
                    Arc::new(move || {
                        registry.lock().registrations.remove(&id);
                    }) as Arc<dyn Fn() + Send + Sync>
                });
                Invocation {
                    handler_id: registration.id,
                    priority: registration.priority,
                    event_sequence: sequence,
                    registration_order: registration.order,
                    event: Some(event.to_owned()),
                    arguments: Vec::new(),
                    limits: registration.limits,
                    callback: registration.callback.clone(),
                    on_start,
                }
            })
            .collect::<Vec<_>>();
        let handles = self.executor.submit_batch(invocations).await?;
        let mut outcomes = Vec::with_capacity(handles.len());
        let mut cancelled = false;
        for (registration, handle) in registrations.into_iter().zip(handles) {
            let outcome = handle.outcome().await;
            if cancellable && outcome == CallbackOutcome::Returned(CallbackReturnKey::False) {
                cancelled = true;
            }
            if let CallbackOutcome::Failed(error) = &outcome {
                self.diagnostics.record(Diagnostic {
                    level: DiagnosticLevel::Error,
                    code: "dynamic_callback_failed",
                    message: error.message.clone(),
                    handler_id: Some(registration.id),
                    event: Some(event.to_owned()),
                });
            }
            outcomes.push((registration.id, outcome));
        }
        Ok(EventResult {
            cancelled,
            outcomes,
        })
    }
}

fn validate_event_name(event: &str) -> Result<(), EventError> {
    if event.is_empty() || event.contains('\0') {
        Err(EventError::InvalidEventName)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::callback::{CallbackError, CallbackReturn, InvocationContext, NoopDiagnosticSink};

    struct CountingCallback {
        calls: Arc<AtomicUsize>,
        result: CallbackReturn,
    }

    #[async_trait]
    impl Callback for CountingCallback {
        async fn invoke(
            &self,
            context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            assert!(context.arguments.is_empty());
            self.calls.fetch_add(1, Ordering::AcqRel);
            Ok(self.result.clone())
        }
    }

    fn bus() -> EventBus {
        EventBus::new(
            CallbackSettings {
                soft_timeout_ms: 0,
                soft_timeout_grace_ms: 0,
                hard_timeout_ms: 0,
                ..CallbackSettings::default()
            },
            3,
            Arc::new(NoopDiagnosticSink),
        )
    }

    #[tokio::test]
    async fn pending_registration_activates_after_define_and_once_is_removed_at_start() {
        let bus = bus();
        let calls = Arc::new(AtomicUsize::new(0));
        let id = bus
            .once(
                "PluginReadyPost",
                Arc::new(CountingCallback {
                    calls: calls.clone(),
                    result: CallbackReturn::None,
                }),
                RegistrationOptions::default(),
            )
            .unwrap();
        assert!(!bus.list_defined().contains(&"PluginReadyPost".to_owned()));
        assert!(matches!(
            bus.emit("PluginReadyPost").await,
            Err(EventError::UndefinedEvent(_))
        ));
        bus.define("PluginReadyPost").unwrap();
        bus.define("PluginReadyPost").unwrap();
        bus.emit("PluginReadyPost").await.unwrap();
        bus.emit("PluginReadyPost").await.unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 1);
        bus.off(id);
    }

    #[tokio::test]
    async fn only_exact_false_cancels_cancellable_pre_events() {
        let bus = bus();
        for result in [
            CallbackReturn::None,
            CallbackReturn::Boolean(true),
            CallbackReturn::Boolean(false),
        ] {
            bus.on(
                "CommandStartPre",
                Arc::new(CountingCallback {
                    calls: Arc::new(AtomicUsize::new(0)),
                    result,
                }),
                RegistrationOptions::default(),
            )
            .unwrap();
        }
        assert!(bus.emit("CommandStartPre").await.unwrap().cancelled);
        assert!(!bus.emit("CommandStartPost").await.unwrap().cancelled);
    }

    #[tokio::test]
    async fn shutdown_pre_is_never_cancellable_and_callbacks_get_no_arguments() {
        let bus = bus();
        bus.on(
            "AppShutdownPre",
            Arc::new(CountingCallback {
                calls: Arc::new(AtomicUsize::new(0)),
                result: CallbackReturn::Boolean(false),
            }),
            RegistrationOptions::default(),
        )
        .unwrap();
        assert!(!bus.emit("AppShutdownPre").await.unwrap().cancelled);
    }

    #[tokio::test]
    async fn groups_cannot_reuse_reserved_event_names() {
        let bus = bus();
        let error = bus.on(
            "UnknownPost",
            Arc::new(CountingCallback {
                calls: Arc::new(AtomicUsize::new(0)),
                result: CallbackReturn::None,
            }),
            RegistrationOptions {
                group: Some("CameraOpenPost".to_owned()),
                ..RegistrationOptions::default()
            },
        );
        assert_eq!(
            error.unwrap_err(),
            EventError::ReservedGroup("CameraOpenPost".to_owned())
        );
    }

    #[tokio::test]
    async fn invalid_global_timeout_update_is_atomic_for_registrations() {
        let bus = bus();
        bus.on(
            "CameraOpenPost",
            Arc::new(CountingCallback {
                calls: Arc::new(AtomicUsize::new(0)),
                result: CallbackReturn::None,
            }),
            RegistrationOptions {
                limits: CallbackLimits {
                    soft_timeout_ms: Some(100),
                    soft_timeout_grace_ms: Some(50),
                    hard_timeout_ms: None,
                },
                ..RegistrationOptions::default()
            },
        )
        .unwrap();
        let invalid = bus
            .update_settings(CallbackSettings {
                soft_timeout_ms: 10,
                soft_timeout_grace_ms: 10,
                hard_timeout_ms: 120,
                ..CallbackSettings::default()
            })
            .await;
        assert!(invalid.is_err());
        assert!(bus.emit("CameraOpenPost").await.is_ok());
    }
}
