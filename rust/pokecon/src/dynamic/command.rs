use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dynamic::callback::{
    Callback, CallbackLimits, CallbackOutcome, CallbackReturn, CallbackSettings, Diagnostic,
    DiagnosticLevel,
};
use crate::dynamic::event::{EventBus, EventError, HandlerId};
use crate::dynamic::host::{DynamicHost, DynamicHostError};
use crate::dynamic::transaction::callback_settings;

pub const SORT_HANDLER_ID: HandlerId = HandlerId::new(1);
pub const TAG_MATCH_HANDLER_ID: HandlerId = HandlerId::new(2);

/// Canonical command metadata shared by worker IPC, Python, Lua, and the UI.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandInfo {
    pub name: String,
    pub module_path: String,
    pub class_name: String,
    pub tags: Vec<String>,
}

/// Closed UI wire shape produced by command precomputation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandDisplayItem {
    Command { command: CommandInfo },
    Separator { label: Option<String> },
}

/// One complete finite-tag display cache. Callers publish this value only as a
/// whole, so no partially computed tag can become UI-visible.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDisplayCache {
    pub generation: u64,
    pub candidates: Vec<CommandInfo>,
    pub tags: Vec<String>,
    pub display_lists: BTreeMap<String, Vec<CommandDisplayItem>>,
}

/// Result of one sequential cache-generation attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum CommandCacheBuildResult {
    Complete { cache: CommandDisplayCache },
    Superseded { generation: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandCallbackKind {
    Sort,
    TagMatch,
}

impl CommandCallbackKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sort => "sort",
            Self::TagMatch => "tag_match",
        }
    }

    const fn handler_id(self) -> HandlerId {
        match self {
            Self::Sort => SORT_HANDLER_ID,
            Self::TagMatch => TAG_MATCH_HANDLER_ID,
        }
    }
}

impl TryFrom<&str> for CommandCallbackKind {
    type Error = CommandError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "sort" => Ok(Self::Sort),
            "tag_match" => Ok(Self::TagMatch),
            _ => Err(CommandError::UnknownCallback(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandOptionField {
    Priority,
    SoftTimeoutMs,
    SoftTimeoutGraceMs,
    HardTimeoutMs,
}

impl TryFrom<&str> for CommandOptionField {
    type Error = CommandError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "priority" => Ok(Self::Priority),
            "soft_timeout_ms" => Ok(Self::SoftTimeoutMs),
            "soft_timeout_grace_ms" => Ok(Self::SoftTimeoutGraceMs),
            "hard_timeout_ms" => Ok(Self::HardTimeoutMs),
            _ => Err(CommandError::UnknownOption(value.to_owned())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandOptionValue {
    Priority(i32),
    Timeout(Option<u64>),
}

/// Shared command callback configuration failure.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Event(#[from] EventError),
    #[error(transparent)]
    Host(#[from] DynamicHostError),
    #[error("unknown command callback: {0}")]
    UnknownCallback(String),
    #[error("unknown command callback option: {0}")]
    UnknownOption(String),
    #[error("command option has the wrong value kind")]
    WrongOptionValue,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Default)]
struct CommandSlot {
    callback: Option<Arc<dyn Callback>>,
    callback_revision: Option<u64>,
    priority: i32,
    limits: CallbackLimits,
}

impl std::fmt::Debug for CommandSlot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandSlot")
            .field("callback_configured", &self.callback.is_some())
            .field("callback_revision", &self.callback_revision)
            .field("priority", &self.priority)
            .field("limits", &self.limits)
            .finish()
    }
}

/// Cloneable generation-local command configuration. The language-native
/// callable remains behind `Arc<dyn Callback>` and never crosses runtimes.
#[derive(Clone, Debug, Default)]
pub(crate) struct CommandState {
    sort: CommandSlot,
    tag_match: CommandSlot,
}

impl CommandState {
    fn slot(&self, kind: CommandCallbackKind) -> &CommandSlot {
        match kind {
            CommandCallbackKind::Sort => &self.sort,
            CommandCallbackKind::TagMatch => &self.tag_match,
        }
    }

    fn slot_mut(&mut self, kind: CommandCallbackKind) -> &mut CommandSlot {
        match kind {
            CommandCallbackKind::Sort => &mut self.sort,
            CommandCallbackKind::TagMatch => &mut self.tag_match,
        }
    }

    pub(crate) fn callback_revision(&self, kind: CommandCallbackKind) -> Option<u64> {
        self.slot(kind).callback_revision
    }

    pub(crate) fn option(
        &self,
        kind: CommandCallbackKind,
        field: CommandOptionField,
    ) -> CommandOptionValue {
        let slot = self.slot(kind);
        match field {
            CommandOptionField::Priority => CommandOptionValue::Priority(slot.priority),
            CommandOptionField::SoftTimeoutMs => {
                CommandOptionValue::Timeout(slot.limits.soft_timeout_ms)
            }
            CommandOptionField::SoftTimeoutGraceMs => {
                CommandOptionValue::Timeout(slot.limits.soft_timeout_grace_ms)
            }
            CommandOptionField::HardTimeoutMs => {
                CommandOptionValue::Timeout(slot.limits.hard_timeout_ms)
            }
        }
    }

    pub(crate) fn set_option(
        &mut self,
        kind: CommandCallbackKind,
        field: CommandOptionField,
        value: CommandOptionValue,
        global: CallbackSettings,
    ) -> Result<(), CommandError> {
        let slot = self.slot_mut(kind);
        let previous = slot.clone();
        match (field, value) {
            (CommandOptionField::Priority, CommandOptionValue::Priority(value)) => {
                slot.priority = value;
            }
            (CommandOptionField::SoftTimeoutMs, CommandOptionValue::Timeout(value)) => {
                slot.limits.soft_timeout_ms = value;
            }
            (CommandOptionField::SoftTimeoutGraceMs, CommandOptionValue::Timeout(value)) => {
                slot.limits.soft_timeout_grace_ms = value;
            }
            (CommandOptionField::HardTimeoutMs, CommandOptionValue::Timeout(value)) => {
                slot.limits.hard_timeout_ms = value;
            }
            _ => return Err(CommandError::WrongOptionValue),
        }
        if let Err(error) = slot.limits.resolve(global) {
            *slot = previous;
            return Err(EventError::Callback(error).into());
        }
        Ok(())
    }

    pub(crate) fn validate(&self, global: CallbackSettings) -> Result<(), CommandError> {
        for slot in [&self.sort, &self.tag_match] {
            slot.limits.resolve(global).map_err(EventError::Callback)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct CommandInvocation {
    callback: Arc<dyn Callback>,
    callback_revision: u64,
    priority: i32,
    limits: CallbackLimits,
}

struct CommandRegistryInner {
    state: Mutex<CommandState>,
    event_bus: EventBus,
    host: Arc<dyn DynamicHost>,
    next_callback_revision: AtomicU64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandSlotConfiguration {
    callback_revision: Option<u64>,
    priority: i32,
    limits: CallbackLimits,
}

impl From<&CommandSlot> for CommandSlotConfiguration {
    fn from(slot: &CommandSlot) -> Self {
        Self {
            callback_revision: slot.callback_revision,
            priority: slot.priority,
            limits: slot.limits,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandConfiguration {
    sort: CommandSlotConfiguration,
    tag_match: CommandSlotConfiguration,
    global: CallbackSettings,
    tag_match_mode: String,
}

/// One language-neutral registry for sort and tag-match callbacks.
#[derive(Clone)]
pub struct CommandRegistry(Arc<CommandRegistryInner>);

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandRegistry")
            .field("state", &*self.0.state.lock())
            .finish_non_exhaustive()
    }
}

impl CommandRegistry {
    #[must_use]
    pub fn new(event_bus: EventBus, host: Arc<dyn DynamicHost>) -> Self {
        Self(Arc::new(CommandRegistryInner {
            state: Mutex::new(CommandState::default()),
            event_bus,
            host,
            next_callback_revision: AtomicU64::new(1),
        }))
    }

    #[must_use]
    pub(crate) fn state_snapshot(&self) -> CommandState {
        self.0.state.lock().clone()
    }

    pub(crate) fn commit_state(&self, state: CommandState) {
        *self.0.state.lock() = state;
        self.0.host.request_command_recompute();
    }

    #[must_use]
    pub(crate) fn replace_callback(
        &self,
        state: &mut CommandState,
        kind: CommandCallbackKind,
        callback: Option<Arc<dyn Callback>>,
    ) -> Option<u64> {
        let revision = callback.as_ref().map(|_| {
            self.0
                .next_callback_revision
                .fetch_add(1, Ordering::Relaxed)
        });
        let slot = state.slot_mut(kind);
        slot.callback = callback;
        slot.callback_revision = revision;
        revision
    }

    pub(crate) fn set_callback(
        &self,
        kind: CommandCallbackKind,
        callback: Option<Arc<dyn Callback>>,
    ) -> Option<u64> {
        let mut state = self.0.state.lock();
        let revision = self.replace_callback(&mut state, kind, callback);
        drop(state);
        self.0.host.request_command_recompute();
        revision
    }

    /// # Errors
    ///
    /// Rejects a timeout override whose effective combination is invalid.
    pub(crate) fn set_option(
        &self,
        kind: CommandCallbackKind,
        field: CommandOptionField,
        value: CommandOptionValue,
    ) -> Result<(), CommandError> {
        let global = callback_settings(&self.0.host.settings_snapshot()?)?;
        self.0.state.lock().set_option(kind, field, value, global)?;
        self.0.host.request_command_recompute();
        Ok(())
    }

    #[must_use]
    pub(crate) fn callback_revision(&self, kind: CommandCallbackKind) -> Option<u64> {
        self.0.state.lock().callback_revision(kind)
    }

    #[must_use]
    pub(crate) fn option(
        &self,
        kind: CommandCallbackKind,
        field: CommandOptionField,
    ) -> CommandOptionValue {
        self.0.state.lock().option(kind, field)
    }

    /// Computes `"-"` and every finite tag strictly sequentially. The
    /// returned generation is complete or explicitly superseded; partial
    /// display lists are never returned.
    ///
    /// # Errors
    ///
    /// Returns a settings snapshot, callback scheduler, or serialization
    /// failure. The caller must retain its previous completed cache on error.
    pub async fn build_display_cache(
        &self,
        generation: u64,
        candidates: Vec<CommandInfo>,
    ) -> Result<CommandCacheBuildResult, CommandError> {
        let candidates = self.canonical_candidates(candidates);
        let configuration = self.configuration()?;
        let tags = ordered_tags(&candidates);
        let mut display_lists = BTreeMap::new();
        for tag in &tags {
            if !self.configuration_is_current(&configuration)? {
                return Ok(CommandCacheBuildResult::Superseded { generation });
            }
            let mut matched = Vec::new();
            if tag == "-" {
                matched.clone_from(&candidates);
            } else {
                for command in &candidates {
                    if !self.configuration_is_current(&configuration)? {
                        return Ok(CommandCacheBuildResult::Superseded { generation });
                    }
                    if self.tag_matches(tag, command).await? {
                        matched.push(command.clone());
                    }
                }
            }
            if !self.configuration_is_current(&configuration)? {
                return Ok(CommandCacheBuildResult::Superseded { generation });
            }
            let display = self.sort(matched).await?;
            if !self.configuration_is_current(&configuration)? {
                return Ok(CommandCacheBuildResult::Superseded { generation });
            }
            display_lists.insert(tag.clone(), display);
        }
        Ok(CommandCacheBuildResult::Complete {
            cache: CommandDisplayCache {
                generation,
                candidates,
                tags,
                display_lists,
            },
        })
    }

    /// Builds an *isolated reload candidate* for the complete command/tag
    /// display cache without holding camera, serial, or script main-path
    /// locks. The current generation remains live; the candidate snapshots
    /// `CommandConfiguration` once, builds every finite tag strictly
    /// sequentially in isolated memory, and returns only a complete cache or
    /// an explicit `Superseded` marker. Callers must publish the cache only
    /// via `StartupDynamicHost::commit_reload_candidate` after all Python/Lua
    /// settings, callback registrations, and validation have succeeded, so
    /// old callbacks drain in the prior generation and failure leaves the
    /// current generation unchanged with a typed `dynamic_reload_failed`
    /// diagnostic. This method reuses `build_display_cache` rather than
    /// duplicating its staging/validation logic.
    ///
    /// # Errors
    ///
    /// Returns a settings, callback scheduler, or serialization failure. The
    /// caller must retain its previous completed cache and emit a typed
    /// failure on error; partial display lists are never returned.
    pub async fn build_reload_candidate(
        &self,
        generation: u64,
        candidates: Vec<CommandInfo>,
    ) -> Result<CommandCacheBuildResult, CommandError> {
        self.build_display_cache(generation, candidates).await
    }

    fn configuration(&self) -> Result<CommandConfiguration, CommandError> {
        let settings = self.0.host.settings_snapshot()?;
        let global = callback_settings(&settings)?;
        let tag_match_mode = settings
            .get("commands.tag_match_mode")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DynamicHostError::new(
                    "MissingSetting",
                    "setting snapshot is missing commands.tag_match_mode",
                )
            })?
            .to_owned();
        let state = self.0.state.lock();
        Ok(CommandConfiguration {
            sort: (&state.sort).into(),
            tag_match: (&state.tag_match).into(),
            global,
            tag_match_mode,
        })
    }

    fn configuration_is_current(
        &self,
        configuration: &CommandConfiguration,
    ) -> Result<bool, CommandError> {
        Ok(self.configuration()? == *configuration)
    }

    /// Applies the configured sort callback or returns canonical discovery
    /// order for this calculation only.
    ///
    /// # Errors
    ///
    /// Returns an error only when the shared scheduler is unavailable.
    pub async fn sort(
        &self,
        candidates: Vec<CommandInfo>,
    ) -> Result<Vec<CommandDisplayItem>, CommandError> {
        let candidates = self.canonical_candidates(candidates);
        let fallback = || {
            candidates
                .iter()
                .cloned()
                .map(|command| CommandDisplayItem::Command { command })
                .collect::<Vec<_>>()
        };
        let Some(invocation) = self.invocation(CommandCallbackKind::Sort) else {
            return Ok(fallback());
        };
        let arguments = vec![serde_json::to_value(&candidates)?];
        let outcome = self
            .invoke(CommandCallbackKind::Sort, &invocation, arguments)
            .await?;
        let CallbackOutcome::Returned(CallbackReturn::Value(Value::Array(items))) = outcome else {
            self.record_fallback(CommandCallbackKind::Sort, &outcome);
            return Ok(fallback());
        };
        let canonical = candidates
            .iter()
            .map(|command| {
                (
                    (command.module_path.clone(), command.class_name.clone()),
                    command,
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            if let Some(separator) = separator_from_value(&item) {
                match separator {
                    Ok(label) => result.push(CommandDisplayItem::Separator { label }),
                    Err(()) => self.record_item_warning(
                        "dynamic_command_separator_invalid",
                        "sort callback returned a separator with an invalid label",
                    ),
                }
                continue;
            }
            match serde_json::from_value::<CommandInfo>(item) {
                Ok(command) => {
                    let identity = (command.module_path.clone(), command.class_name.clone());
                    if let Some(command) = canonical.get(&identity) {
                        result.push(CommandDisplayItem::Command {
                            command: (*command).clone(),
                        });
                    } else {
                        self.record_item_warning(
                            "dynamic_command_sort_unknown_command",
                            "sort callback returned a command outside its canonical input",
                        );
                    }
                }
                Err(_) => self.record_item_warning(
                    "dynamic_command_sort_item_invalid",
                    "sort callback returned an invalid display-list element",
                ),
            }
        }
        Ok(result)
    }

    /// Applies the custom matcher or the configured built-in mode for one
    /// selected-tag/candidate pair.
    ///
    /// # Errors
    ///
    /// Returns host snapshot or scheduler transport failures.
    pub async fn tag_matches(
        &self,
        selected_tag: &str,
        command: &CommandInfo,
    ) -> Result<bool, CommandError> {
        if selected_tag == "-" {
            return Ok(true);
        }
        let Some(invocation) = self.invocation(CommandCallbackKind::TagMatch) else {
            return self.builtin_tag_matches(selected_tag, command);
        };
        let arguments = vec![
            Value::String(selected_tag.to_owned()),
            serde_json::to_value(command)?,
        ];
        let outcome = self
            .invoke(CommandCallbackKind::TagMatch, &invocation, arguments)
            .await?;
        if let CallbackOutcome::Returned(CallbackReturn::Boolean(matches)) = outcome {
            return Ok(matches);
        }
        self.record_fallback(CommandCallbackKind::TagMatch, &outcome);
        self.builtin_tag_matches(selected_tag, command)
    }

    fn invocation(&self, kind: CommandCallbackKind) -> Option<CommandInvocation> {
        let state = self.0.state.lock();
        let slot = state.slot(kind);
        Some(CommandInvocation {
            callback: slot.callback.clone()?,
            callback_revision: slot
                .callback_revision
                .expect("configured command callback has a revision"),
            priority: slot.priority,
            limits: slot.limits,
        })
    }

    async fn invoke(
        &self,
        kind: CommandCallbackKind,
        invocation: &CommandInvocation,
        arguments: Vec<Value>,
    ) -> Result<CallbackOutcome, CommandError> {
        let weak = Arc::downgrade(&self.0);
        let revision = invocation.callback_revision;
        let on_late_return = Arc::new(move || {
            let Some(inner) = Weak::upgrade(&weak) else {
                return;
            };
            let is_current = inner.state.lock().slot(kind).callback_revision == Some(revision);
            if is_current {
                inner.host.request_command_recompute();
            }
        });
        Ok(self
            .0
            .event_bus
            .invoke_internal(
                kind.handler_id(),
                invocation.priority,
                arguments,
                invocation.limits,
                invocation.callback.clone(),
                Some(on_late_return),
            )
            .await?)
    }

    fn canonical_candidates(&self, candidates: Vec<CommandInfo>) -> Vec<CommandInfo> {
        let mut identities = BTreeSet::<(String, String)>::new();
        let mut canonical = Vec::with_capacity(candidates.len());
        for command in candidates {
            let identity = (command.module_path.clone(), command.class_name.clone());
            if identities.insert(identity) {
                canonical.push(command);
            } else {
                self.record_item_warning(
                    "dynamic_command_candidate_duplicate",
                    "duplicate command identity was removed after its first discovery",
                );
            }
        }
        canonical
    }

    fn builtin_tag_matches(
        &self,
        selected_tag: &str,
        command: &CommandInfo,
    ) -> Result<bool, CommandError> {
        let settings = self.0.host.settings_snapshot()?;
        let mode = settings
            .get("commands.tag_match_mode")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DynamicHostError::new(
                    "MissingSetting",
                    "setting snapshot is missing commands.tag_match_mode",
                )
            })?;
        let predicate = |tag: &str| match mode {
            "exact" => tag == selected_tag,
            "partial" => tag.contains(selected_tag),
            "prefix" => tag.starts_with(selected_tag),
            "suffix" => tag.ends_with(selected_tag),
            _ => false,
        };
        if !matches!(mode, "exact" | "partial" | "prefix" | "suffix") {
            return Err(DynamicHostError::new(
                "InvalidSetting",
                format!("unsupported commands.tag_match_mode: {mode}"),
            )
            .into());
        }
        Ok(command.tags.iter().any(|tag| predicate(tag)))
    }

    fn record_fallback(&self, kind: CommandCallbackKind, outcome: &CallbackOutcome) {
        let reason = match outcome {
            CallbackOutcome::Returned(_) => "invalid return value",
            CallbackOutcome::Failed(_) => "callback failure",
            CallbackOutcome::TimedOut(_) => "logical timeout",
            CallbackOutcome::Evicted => "queue eviction",
            CallbackOutcome::LaneBusy => "occupied callback lane",
        };
        self.0.host.record_diagnostic(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: "dynamic_command_callback_fallback".to_owned(),
            message: format!(
                "{} callback used its local built-in fallback after {reason}",
                kind.name()
            ),
            handler_id: Some(kind.handler_id()),
            event: None,
        });
    }

    fn record_item_warning(&self, code: &'static str, message: &'static str) {
        self.0.host.record_diagnostic(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: code.to_owned(),
            message: message.to_owned(),
            handler_id: Some(SORT_HANDLER_ID),
            event: None,
        });
    }
}

fn ordered_tags(candidates: &[CommandInfo]) -> Vec<String> {
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
    }
    tags
}

fn separator_from_value(value: &Value) -> Option<Result<Option<String>, ()>> {
    let object = value.as_object()?;
    if object.get("__pokecon_separator__") != Some(&Value::Bool(true)) {
        return None;
    }
    Some(match object.get("label").unwrap_or(&Value::Null) {
        Value::Null => Ok(None),
        Value::String(label) => Ok(Some(label.clone())),
        _ => Err(()),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::Notify;
    use tokio::time::{Duration, timeout};

    use super::*;
    use crate::dynamic::callback::{
        CallbackError, CallbackReturn, InvocationContext, NoopDiagnosticSink,
    };
    use crate::dynamic::host::InMemoryDynamicHost;

    struct ReturningCallback {
        calls: Arc<AtomicUsize>,
        returned: CallbackReturn,
    }

    struct BlockingCallback {
        calls: Arc<AtomicUsize>,
        release: Arc<Notify>,
    }

    #[async_trait]
    impl Callback for BlockingCallback {
        async fn invoke(
            &self,
            _context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.release.notified().await;
            Ok(CallbackReturn::Value(json!([])))
        }
    }

    #[async_trait]
    impl Callback for ReturningCallback {
        async fn invoke(
            &self,
            _context: InvocationContext,
        ) -> Result<CallbackReturn, CallbackError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Ok(self.returned.clone())
        }
    }

    fn settings() -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("commands.tag_match_mode".to_owned(), json!("prefix")),
            ("dynamic.callback_soft_timeout_ms".to_owned(), json!(2_000)),
            (
                "dynamic.callback_soft_timeout_grace_ms".to_owned(),
                json!(1_000),
            ),
            ("dynamic.callback_hard_timeout_ms".to_owned(), json!(5_000)),
            ("dynamic.callback_max_concurrency".to_owned(), json!(8)),
            ("dynamic.callback_queue_capacity".to_owned(), json!(1_024)),
        ])
    }

    fn command(name: &str, module_path: &str, class_name: &str) -> CommandInfo {
        CommandInfo {
            name: name.to_owned(),
            module_path: module_path.to_owned(),
            class_name: class_name.to_owned(),
            tags: vec!["sample-fast".to_owned()],
        }
    }

    fn registry() -> (CommandRegistry, Arc<InMemoryDynamicHost>) {
        let host = Arc::new(InMemoryDynamicHost::new(settings(), BTreeMap::new()).unwrap());
        let bus = EventBus::new(CallbackSettings::default(), 3, Arc::new(NoopDiagnosticSink));
        (CommandRegistry::new(bus, host.clone()), host)
    }

    #[tokio::test]
    async fn sort_resolves_canonical_commands_and_skips_invalid_items() {
        let (registry, host) = registry();
        let first = command("First", "Commands.First", "First");
        let second = command("Second", "Commands.Second", "Second");
        let calls = Arc::new(AtomicUsize::new(0));
        let returned = CallbackReturn::Value(json!([
            {
                "name": "mutated",
                "module_path": second.module_path,
                "class_name": second.class_name,
                "tags": []
            },
            {"__pokecon_separator__": true, "label": "Group"},
            {
                "name": "unknown",
                "module_path": "Commands.Unknown",
                "class_name": "Unknown",
                "tags": []
            },
            7,
            {
                "name": "again",
                "module_path": first.module_path,
                "class_name": first.class_name,
                "tags": ["ignored"]
            }
        ]));
        registry.set_callback(
            CommandCallbackKind::Sort,
            Some(Arc::new(ReturningCallback {
                calls: calls.clone(),
                returned,
            })),
        );
        let result = registry
            .sort(vec![first.clone(), second.clone()])
            .await
            .unwrap();
        assert_eq!(
            result,
            vec![
                CommandDisplayItem::Command { command: second },
                CommandDisplayItem::Separator {
                    label: Some("Group".to_owned())
                },
                CommandDisplayItem::Command { command: first },
            ]
        );
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(host.diagnostics().len(), 2);
    }

    #[tokio::test]
    async fn tag_match_uses_exact_boolean_and_never_calls_for_disabled_filter() {
        let (registry, host) = registry();
        let command = command("First", "Commands.First", "First");
        let calls = Arc::new(AtomicUsize::new(0));
        registry.set_callback(
            CommandCallbackKind::TagMatch,
            Some(Arc::new(ReturningCallback {
                calls: calls.clone(),
                returned: CallbackReturn::None,
            })),
        );
        assert!(registry.tag_matches("-", &command).await.unwrap());
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert!(registry.tag_matches("sample", &command).await.unwrap());
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(host.diagnostics().len(), 1);

        registry.set_callback(
            CommandCallbackKind::TagMatch,
            Some(Arc::new(ReturningCallback {
                calls,
                returned: CallbackReturn::Boolean(false),
            })),
        );
        assert!(!registry.tag_matches("sample", &command).await.unwrap());
    }

    #[tokio::test]
    async fn busy_lane_falls_back_and_late_return_requests_one_recompute() {
        let (registry, host) = registry();
        registry
            .set_option(
                CommandCallbackKind::Sort,
                CommandOptionField::SoftTimeoutMs,
                CommandOptionValue::Timeout(Some(1)),
            )
            .unwrap();
        registry
            .set_option(
                CommandCallbackKind::Sort,
                CommandOptionField::SoftTimeoutGraceMs,
                CommandOptionValue::Timeout(Some(1)),
            )
            .unwrap();
        registry
            .set_option(
                CommandCallbackKind::Sort,
                CommandOptionField::HardTimeoutMs,
                CommandOptionValue::Timeout(Some(0)),
            )
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        registry.set_callback(
            CommandCallbackKind::Sort,
            Some(Arc::new(BlockingCallback {
                calls: calls.clone(),
                release: release.clone(),
            })),
        );
        let baseline_requests = host.command_recompute_requests();
        let command = command("First", "Commands.First", "First");
        let fallback = vec![CommandDisplayItem::Command {
            command: command.clone(),
        }];

        assert_eq!(
            timeout(Duration::from_secs(1), registry.sort(vec![command.clone()]))
                .await
                .unwrap()
                .unwrap(),
            fallback
        );
        assert_eq!(registry.sort(vec![command]).await.unwrap(), fallback);
        assert_eq!(calls.load(Ordering::Acquire), 1);

        release.notify_one();
        timeout(Duration::from_secs(1), async {
            while host.command_recompute_requests() == baseline_requests {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(host.command_recompute_requests(), baseline_requests + 1);
    }

    #[tokio::test]
    async fn display_cache_is_complete_and_orders_manual_tags_before_automatic_tags() {
        let (registry, _host) = registry();
        let mut first = command("First", "Commands.First", "First");
        first.tags = vec!["z".to_owned(), "@Auto".to_owned(), "a".to_owned()];
        let mut second = command("Second", "Commands.Second", "Second");
        second.tags = vec!["@Auto".to_owned(), "b".to_owned()];

        let result = registry
            .build_display_cache(17, vec![first.clone(), second.clone()])
            .await
            .unwrap();
        let CommandCacheBuildResult::Complete { cache } = result else {
            panic!("unchanged configuration must produce a complete cache");
        };
        assert_eq!(cache.generation, 17);
        assert_eq!(cache.candidates, [first.clone(), second.clone()]);
        assert_eq!(cache.tags, ["-", "z", "a", "b", "@Auto"]);
        assert_eq!(
            cache.display_lists["-"],
            [
                CommandDisplayItem::Command {
                    command: first.clone()
                },
                CommandDisplayItem::Command {
                    command: second.clone()
                }
            ]
        );
        assert_eq!(
            cache.display_lists["z"],
            [CommandDisplayItem::Command { command: first }]
        );
        assert_eq!(
            cache.display_lists["b"],
            [CommandDisplayItem::Command { command: second }]
        );
    }

    #[tokio::test]
    async fn display_cache_discards_a_generation_changed_during_callback_execution() {
        let (registry, _host) = registry();
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        registry.set_callback(
            CommandCallbackKind::Sort,
            Some(Arc::new(BlockingCallback {
                calls: calls.clone(),
                release: release.clone(),
            })),
        );
        let building = {
            let registry = registry.clone();
            tokio::spawn(async move {
                registry
                    .build_display_cache(23, vec![command("First", "Commands.First", "First")])
                    .await
            })
        };
        timeout(Duration::from_secs(1), async {
            while calls.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        registry
            .set_option(
                CommandCallbackKind::Sort,
                CommandOptionField::Priority,
                CommandOptionValue::Priority(9),
            )
            .unwrap();
        release.notify_one();

        assert_eq!(
            timeout(Duration::from_secs(1), building)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            CommandCacheBuildResult::Superseded { generation: 23 }
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reload_candidate_is_isolated_and_generation_traced() {
        let (registry, host) = registry();
        let first = command("First", "Commands.First", "First");
        let second = command("Second", "Commands.Second", "Second");
        let result = registry
            .build_reload_candidate(7, vec![first.clone(), second.clone()])
            .await
            .unwrap();
        let CommandCacheBuildResult::Complete { cache } = result else {
            panic!("candidate must be complete");
        };
        assert_eq!(cache.generation, 7);
        assert_eq!(cache.tags, ["-", "sample-fast"]);
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        registry.set_callback(
            CommandCallbackKind::Sort,
            Some(Arc::new(BlockingCallback {
                calls: calls.clone(),
                release: release.clone(),
            })),
        );
        let registry_clone = registry.clone();
        let building = tokio::spawn(async move {
            registry_clone
                .build_reload_candidate(8, vec![first.clone()])
                .await
        });
        timeout(Duration::from_secs(1), async {
            while calls.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        registry
            .set_option(
                CommandCallbackKind::TagMatch,
                CommandOptionField::Priority,
                CommandOptionValue::Priority(5),
            )
            .unwrap();
        release.notify_one();
        let superseded = timeout(Duration::from_secs(1), building)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            superseded,
            CommandCacheBuildResult::Superseded { generation: 8 }
        );
        assert!(
            host.diagnostics()
                .iter()
                .all(|diagnostic| !diagnostic.code.is_empty())
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_reload_candidates_only_one_generation_wins() {
        let (registry, _host) = registry();
        let mut handles = Vec::new();
        for i in 0..4u64 {
            let registry = registry.clone();
            let cmd = command(
                &format!("Cmd{i}"),
                &format!("Commands.Cmd{i}"),
                &format!("Cmd{i}"),
            );
            handles.push(tokio::spawn(async move {
                let gen_id = 100 + i;
                let result = registry
                    .build_reload_candidate(gen_id, vec![cmd])
                    .await
                    .unwrap();
                (gen_id, result)
            }));
        }
        let mut completed = Vec::new();
        for handle in handles {
            let (gen_id, result) = handle.await.unwrap();
            match result {
                CommandCacheBuildResult::Complete { cache } => {
                    assert_eq!(cache.generation, gen_id);
                    completed.push(gen_id);
                }
                CommandCacheBuildResult::Superseded { generation } => {
                    assert_eq!(generation, gen_id);
                }
            }
        }
        assert!(
            !completed.is_empty(),
            "at least one candidate must complete"
        );
        completed.sort_unstable();
        for window in completed.windows(2) {
            assert!(window[0] < window[1]);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn old_callbacks_finish_in_old_generation_while_new_candidate_builds() {
        let (registry, host) = registry();
        let calls_old = Arc::new(AtomicUsize::new(0));
        let release_old = Arc::new(Notify::new());
        let old_revision = registry.set_callback(
            CommandCallbackKind::Sort,
            Some(Arc::new(BlockingCallback {
                calls: calls_old.clone(),
                release: release_old.clone(),
            })),
        );
        assert!(old_revision.is_some());
        let registry_clone = registry.clone();
        let old_build = tokio::spawn(async move {
            registry_clone
                .sort(vec![command("Old", "Commands.Old", "Old")])
                .await
                .unwrap()
        });
        timeout(Duration::from_secs(1), async {
            while calls_old.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let new_calls = Arc::new(AtomicUsize::new(0));
        registry.set_callback(
            CommandCallbackKind::Sort,
            Some(Arc::new(ReturningCallback {
                calls: new_calls.clone(),
                returned: CallbackReturn::Value(json!([{
                    "name": "New",
                    "module_path": "Commands.New",
                    "class_name": "New",
                    "tags": []
                }])),
            })),
        );
        let fallback = vec![CommandDisplayItem::Command {
            command: command("New", "Commands.New", "New"),
        }];
        let new_result = timeout(
            Duration::from_secs(1),
            registry.sort(vec![command("New", "Commands.New", "New")]),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(new_result, fallback);
        release_old.notify_one();
        let _old_result = timeout(Duration::from_secs(1), old_build)
            .await
            .unwrap()
            .unwrap();
        assert!(
            host.diagnostics()
                .iter()
                .any(|d| d.code == "dynamic_command_callback_fallback")
        );
    }
}
