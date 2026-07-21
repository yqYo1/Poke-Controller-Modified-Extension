use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::Mutex as AsyncMutex;

use crate::callback::{
    Callback, Diagnostic, DiagnosticLevel, DiagnosticSink, InvocationContext, TimeoutStage,
};
use crate::control::{DynamicConfigControl, DynamicConfigLanguage, DynamicLoadResult};
use crate::event::{EventBus, EventError, EventResult, HandlerId, RegistrationOptions};
use crate::host::{DynamicHost, DynamicHostError, DynamicSettingsRegistry};
use crate::runtime::lua::LuaRuntime;
use crate::runtime::python::PythonRuntime;
use crate::source::{ResolvedSource, SourceError, SourceStore};
use crate::transaction::{
    EvaluationTransaction, TransactionError, callback_settings, controller_update_from_value,
};

const FIRST_PUBLIC_HANDLER_ID: u64 = 3;

thread_local! {
    static CURRENT_EVALUATION: RefCell<Option<Arc<EvaluationSession>>> = const { RefCell::new(None) };
    static CURRENT_INVOCATION: RefCell<Option<InvocationFrame>> = const { RefCell::new(None) };
}

/// Dynamic configuration initialization, evaluation, or host bridge failure.
#[derive(Debug, thiserror::Error)]
pub enum DynamicEngineError {
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Transaction(#[from] TransactionError),
    #[error(transparent)]
    Host(#[from] DynamicHostError),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("Python dynamic runtime failed: {0}")]
    Python(String),
    #[error("Lua dynamic runtime failed: {0}")]
    Lua(String),
    #[error("dynamic source evaluation failed: {0}")]
    Evaluation(String),
    #[error("dynamic API requires an active evaluation")]
    NoActiveEvaluation,
    #[error("dynamic engine requires an active Tokio runtime")]
    NoTokioRuntime,
}

#[derive(Debug)]
struct EvaluationSession {
    transaction: Mutex<Option<EvaluationTransaction>>,
}

impl EvaluationSession {
    fn new(transaction: EvaluationTransaction) -> Self {
        Self {
            transaction: Mutex::new(Some(transaction)),
        }
    }

    fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&mut EvaluationTransaction) -> Result<T, DynamicEngineError>,
    ) -> Result<T, DynamicEngineError> {
        let mut transaction = self.transaction.lock();
        let transaction = transaction
            .as_mut()
            .ok_or(DynamicEngineError::NoActiveEvaluation)?;
        operation(transaction)
    }

    fn take_transaction(&self) -> Result<EvaluationTransaction, DynamicEngineError> {
        self.transaction
            .lock()
            .take()
            .ok_or(DynamicEngineError::NoActiveEvaluation)
    }
}

struct EvaluationScope {
    previous: Option<Arc<EvaluationSession>>,
}

impl EvaluationScope {
    fn enter(session: Arc<EvaluationSession>) -> Self {
        let previous = CURRENT_EVALUATION.with(|current| current.replace(Some(session)));
        Self { previous }
    }
}

impl Drop for EvaluationScope {
    fn drop(&mut self) {
        CURRENT_EVALUATION.with(|current| {
            current.replace(self.previous.take());
        });
    }
}

#[derive(Clone, Debug)]
struct InvocationFrame {
    context: InvocationContext,
    soft_delivered: bool,
}

pub(crate) struct InvocationScope {
    previous: Option<InvocationFrame>,
}

impl InvocationScope {
    pub(crate) fn enter(context: InvocationContext) -> Self {
        let previous = CURRENT_INVOCATION.with(|current| {
            current.replace(Some(InvocationFrame {
                context,
                soft_delivered: false,
            }))
        });
        Self { previous }
    }
}

impl Drop for InvocationScope {
    fn drop(&mut self) {
        CURRENT_INVOCATION.with(|current| {
            current.replace(self.previous.take());
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeadlineCheckpoint {
    Soft {
        handler_id: HandlerId,
        elapsed_ms: u64,
        soft_timeout_ms: u64,
    },
    Hard,
}

pub(crate) fn deadline_checkpoint() -> Option<DeadlineCheckpoint> {
    CURRENT_INVOCATION.with(|current| {
        let mut current = current.borrow_mut();
        let frame = current.as_mut()?;
        match frame.context.deadline.stage() {
            TimeoutStage::Running => None,
            TimeoutStage::Soft if frame.soft_delivered => None,
            TimeoutStage::Soft => {
                frame.soft_delivered = true;
                let elapsed_ms = u64::try_from(frame.context.started_at.elapsed().as_millis())
                    .unwrap_or(u64::MAX);
                Some(DeadlineCheckpoint::Soft {
                    handler_id: frame.context.handler_id,
                    elapsed_ms,
                    soft_timeout_ms: frame.context.limits.soft_timeout_ms,
                })
            }
            TimeoutStage::Hard => Some(DeadlineCheckpoint::Hard),
        }
    })
}

fn current_evaluation() -> Option<Arc<EvaluationSession>> {
    CURRENT_EVALUATION.with(|current| current.borrow().clone())
}

fn current_event() -> Option<String> {
    CURRENT_INVOCATION.with(|current| {
        current
            .borrow()
            .as_ref()
            .and_then(|frame| frame.context.event.clone())
    })
}

#[derive(Default)]
struct RuntimeSet {
    python: Option<Arc<PythonRuntime>>,
    lua: Option<Arc<LuaRuntime>>,
}

struct HostDiagnosticSink {
    host: Arc<dyn DynamicHost>,
}

impl DiagnosticSink for HostDiagnosticSink {
    fn record(&self, diagnostic: Diagnostic) {
        self.host.record_diagnostic(diagnostic);
    }
}

pub(crate) struct EngineInner {
    host: Arc<dyn DynamicHost>,
    settings_registry: Arc<DynamicSettingsRegistry>,
    source_store: SourceStore,
    event_bus: EventBus,
    coordinator: AsyncMutex<()>,
    runtimes: Mutex<RuntimeSet>,
    runtime_handle: Handle,
    generation: AtomicU64,
    weak_self: Weak<Self>,
}

impl std::fmt::Debug for EngineInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EngineInner")
            .field("config_root", &self.source_store.config_root())
            .field("generation", &self.generation.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

/// One persistent dynamic worker engine containing both embedded language
/// runtimes, one source coordinator, and the shared event executor.
#[derive(Clone, Debug)]
pub struct DynamicEngine(Arc<EngineInner>);

impl DynamicEngine {
    /// Creates an engine and eagerly initializes only the configured primary
    /// runtime. The other runtime remains lazy until a cross-language source.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid settings snapshot, source root, missing
    /// Tokio runtime, or primary language initialization failure.
    pub fn new(
        config_root: impl Into<PathBuf>,
        home: Option<PathBuf>,
        primary: Option<DynamicConfigLanguage>,
        host: Arc<dyn DynamicHost>,
    ) -> Result<Self, DynamicEngineError> {
        let runtime_handle =
            Handle::try_current().map_err(|_| DynamicEngineError::NoTokioRuntime)?;
        let settings_registry = Arc::new(
            DynamicSettingsRegistry::load()
                .map_err(|error| DynamicHostError::new("InvalidRegistry", error.to_string()))?,
        );
        let settings = host.settings_snapshot()?;
        let callback_settings = callback_settings(&settings)?;
        let source_store = SourceStore::new(config_root, home)?;
        let diagnostics: Arc<dyn DiagnosticSink> =
            Arc::new(HostDiagnosticSink { host: host.clone() });
        let event_bus = EventBus::new(callback_settings, FIRST_PUBLIC_HANDLER_ID, diagnostics);
        let inner = Arc::new_cyclic(|weak_self| EngineInner {
            host,
            settings_registry,
            source_store,
            event_bus,
            coordinator: AsyncMutex::new(()),
            runtimes: Mutex::new(RuntimeSet::default()),
            runtime_handle,
            generation: AtomicU64::new(0),
            weak_self: weak_self.clone(),
        });
        if let Some(primary) = primary {
            inner.ensure_runtime(primary)?;
        }
        Ok(Self(inner))
    }

    #[must_use]
    pub fn event_bus(&self) -> EventBus {
        self.0.event_bus.clone()
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn initialized_languages(&self) -> Vec<DynamicConfigLanguage> {
        let runtimes = self.0.runtimes.lock();
        let mut languages = Vec::with_capacity(2);
        if runtimes.python.is_some() {
            languages.push(DynamicConfigLanguage::Python);
        }
        if runtimes.lua.is_some() {
            languages.push(DynamicConfigLanguage::Lua);
        }
        languages
    }

    /// Executes the closed control operation union used by worker IPC and REST.
    ///
    /// # Errors
    ///
    /// Returns source persistence/path or worker bridge failures. Language
    /// evaluation errors are reported as `loaded=false` and retain the prior
    /// generation.
    pub async fn control(
        &self,
        operation: DynamicConfigControl,
    ) -> Result<DynamicLoadResult, DynamicEngineError> {
        let source = match operation {
            DynamicConfigControl::LoadPath { path } => self.0.source_store.resolve(&path)?,
            DynamicConfigControl::LoadContent { language, content } => {
                self.0.source_store.save_content(language, &content)?
            }
            DynamicConfigControl::Reload {} => self.0.source_store.reload_source()?,
        };
        self.0.load_resolved(source, true, true).await
    }

    /// Emits an external canonical event after waiting for any in-flight
    /// evaluation transaction to cross its dispatch barrier.
    ///
    /// # Errors
    ///
    /// Returns event validation or callback scheduler failures.
    pub async fn emit(&self, event: &str) -> Result<EventResult, DynamicEngineError> {
        self.0.emit(event).await
    }
}

impl EngineInner {
    fn ensure_runtime(&self, language: DynamicConfigLanguage) -> Result<(), DynamicEngineError> {
        let mut runtimes = self.runtimes.lock();
        match language {
            DynamicConfigLanguage::Python if runtimes.python.is_none() => {
                runtimes.python = Some(Arc::new(PythonRuntime::new(self.weak_self.clone())?));
            }
            DynamicConfigLanguage::Lua if runtimes.lua.is_none() => {
                runtimes.lua = Some(Arc::new(LuaRuntime::new(&self.weak_self)?));
            }
            DynamicConfigLanguage::Python | DynamicConfigLanguage::Lua => {}
        }
        Ok(())
    }

    fn evaluate_source(&self, source: &ResolvedSource) -> Result<(), DynamicEngineError> {
        let _source_guard = self.source_store.push(source)?;
        let content = self.source_store.read(source)?;
        self.ensure_runtime(source.language)?;
        match source.language {
            DynamicConfigLanguage::Python => {
                let runtime = self
                    .runtimes
                    .lock()
                    .python
                    .clone()
                    .expect("Python runtime was initialized");
                runtime.evaluate(&content, &source.display_path)
            }
            DynamicConfigLanguage::Lua => {
                let runtime = self
                    .runtimes
                    .lock()
                    .lua
                    .clone()
                    .expect("Lua runtime was initialized");
                runtime.evaluate(&content, &source.display_path)
            }
        }
    }

    async fn load_resolved(
        self: &Arc<Self>,
        source: ResolvedSource,
        replace_generation: bool,
        mark_current: bool,
    ) -> Result<DynamicLoadResult, DynamicEngineError> {
        let coordinator = self.coordinator.lock().await;
        let mut transaction = EvaluationTransaction::begin(
            self.host.as_ref(),
            self.settings_registry.clone(),
            &self.event_bus,
        )?;
        if replace_generation {
            transaction.clear("all");
        }
        let session = Arc::new(EvaluationSession::new(transaction));
        let evaluation = {
            let _scope = EvaluationScope::enter(session.clone());
            self.evaluate_source(&source)
        };
        let pending_emits = match evaluation {
            Ok(()) => {
                let transaction = session.take_transaction()?;
                match transaction
                    .commit_staged(self.host.as_ref(), &self.event_bus)
                    .await
                {
                    Ok(events) => events,
                    Err(error) => {
                        let diagnostic = error.to_string();
                        self.record_evaluation_failure(&diagnostic);
                        return Ok(DynamicLoadResult {
                            display_path: source.display_path,
                            language: source.language,
                            loaded: false,
                            diagnostic: Some(diagnostic),
                        });
                    }
                }
            }
            Err(error) => {
                let diagnostic = error.to_string();
                self.record_evaluation_failure(&diagnostic);
                return Ok(DynamicLoadResult {
                    display_path: source.display_path,
                    language: source.language,
                    loaded: false,
                    diagnostic: Some(diagnostic),
                });
            }
        };
        if mark_current {
            self.source_store.mark_success(&source);
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        drop(coordinator);
        for event in pending_emits {
            let engine = self.clone();
            self.runtime_handle.spawn(async move {
                if let Err(error) = engine.emit(&event).await {
                    engine.record_evaluation_failure(&error.to_string());
                }
            });
        }
        Ok(DynamicLoadResult {
            display_path: source.display_path,
            language: source.language,
            loaded: true,
            diagnostic: None,
        })
    }

    async fn emit(self: &Arc<Self>, event: &str) -> Result<EventResult, DynamicEngineError> {
        let _coordinator = self.coordinator.lock().await;
        Ok(self.event_bus.emit(event).await?)
    }

    fn record_evaluation_failure(&self, message: &str) {
        self.host.record_diagnostic(Diagnostic {
            level: DiagnosticLevel::Error,
            code: "dynamic_config_evaluation_failed",
            message: message.to_owned(),
            handler_id: None,
            event: None,
        });
    }

    pub(crate) fn setting_kind(&self, path: &str) -> Option<&'static str> {
        if self.settings_registry.setting(path).is_some() {
            Some("value")
        } else if self.settings_registry.is_prefix(path) {
            Some("namespace")
        } else {
            None
        }
    }

    pub(crate) fn setting(&self, path: &str) -> Result<Value, DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| Ok(transaction.setting(path)?));
        }
        let setting = self.settings_registry.setting(path).ok_or_else(|| {
            DynamicHostError::new(
                "UnknownSetting",
                format!("unsupported dynamic path: {path}"),
            )
        })?;
        let snapshot = self.host.settings_snapshot()?;
        let value = snapshot.get(&setting.id).ok_or_else(|| {
            DynamicHostError::new(
                "MissingSetting",
                format!("setting snapshot is missing {}", setting.id),
            )
        })?;
        Ok(self
            .settings_registry
            .public_dynamic_value(&setting.id, value))
    }

    pub(crate) fn set_setting(
        self: &Arc<Self>,
        path: &str,
        value: Value,
    ) -> Result<(), DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| {
                transaction.set_setting(path, value)?;
                Ok(())
            });
        }
        let mut transaction = EvaluationTransaction::begin(
            self.host.as_ref(),
            self.settings_registry.clone(),
            &self.event_bus,
        )?;
        transaction.set_setting(path, value)?;
        self.runtime_handle.block_on(async {
            transaction
                .commit_staged(self.host.as_ref(), &self.event_bus)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    pub(crate) fn register(
        self: &Arc<Self>,
        event: &str,
        callback: Arc<dyn Callback>,
        options: RegistrationOptions,
        once: bool,
    ) -> Result<HandlerId, DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| {
                Ok(transaction.register(&self.event_bus, event, callback, options, once)?)
            });
        }
        Ok(if once {
            self.event_bus.once(event, callback, options)?
        } else {
            self.event_bus.on(event, callback, options)?
        })
    }

    pub(crate) fn off(&self, handler_id: HandlerId) -> Result<(), DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| {
                transaction.off(handler_id);
                Ok(())
            });
        }
        self.event_bus.off(handler_id);
        Ok(())
    }

    pub(crate) fn clear(&self, target: &str) -> Result<(), DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| {
                transaction.clear(target);
                Ok(())
            });
        }
        self.event_bus.clear(target);
        Ok(())
    }

    pub(crate) fn define(&self, event: &str) -> Result<(), DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| {
                transaction.define(event)?;
                Ok(())
            });
        }
        Ok(self.event_bus.define(event)?)
    }

    pub(crate) fn list_defined(&self) -> Result<Vec<String>, DynamicEngineError> {
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| Ok(transaction.list_defined()));
        }
        Ok(self.event_bus.list_defined())
    }

    pub(crate) fn emit_from_binding(
        self: &Arc<Self>,
        event: &str,
    ) -> Result<(), DynamicEngineError> {
        if current_event().as_deref() == Some(event) {
            self.host.record_diagnostic(Diagnostic {
                level: DiagnosticLevel::Error,
                code: "dynamic_event_direct_recursion",
                message: format!("direct recursive event emission was ignored: {event}"),
                handler_id: None,
                event: Some(event.to_owned()),
            });
            return Ok(());
        }
        if let Some(session) = current_evaluation() {
            return session.with_transaction(|transaction| {
                transaction.emit(event)?;
                Ok(())
            });
        }
        if !self
            .event_bus
            .list_defined()
            .iter()
            .any(|name| name == event)
        {
            return Err(EventError::UndefinedEvent(event.to_owned()).into());
        }
        let event = event.to_owned();
        let engine = self.clone();
        self.runtime_handle.spawn(async move {
            if let Err(error) = engine.emit(&event).await {
                engine.record_evaluation_failure(&error.to_string());
            }
        });
        Ok(())
    }

    pub(crate) fn source_from_binding(
        self: &Arc<Self>,
        path: &str,
    ) -> Result<(), DynamicEngineError> {
        let source = self.source_store.resolve(path)?;
        if current_evaluation().is_some() {
            return self.evaluate_source(&source);
        }
        let result = self
            .runtime_handle
            .block_on(self.load_resolved(source, false, false))?;
        if result.loaded {
            Ok(())
        } else {
            Err(DynamicEngineError::Evaluation(
                result
                    .diagnostic
                    .unwrap_or_else(|| "dynamic source failed".to_owned()),
            ))
        }
    }

    pub(crate) fn state(&self, name: &str) -> Result<Value, DynamicEngineError> {
        self.host.state_snapshot()?.remove(name).ok_or_else(|| {
            DynamicHostError::new("UnknownState", format!("unknown state property: {name}")).into()
        })
    }

    pub(crate) fn set_state(&self, name: &str, value: Value) -> Result<(), DynamicEngineError> {
        Ok(self.host.set_state_value(name, value)?)
    }

    pub(crate) fn profile_current(&self) -> Result<String, DynamicEngineError> {
        Ok(self.host.profile_current()?)
    }

    pub(crate) fn profile_list(&self) -> Result<Vec<String>, DynamicEngineError> {
        Ok(self.host.profile_list()?)
    }

    pub(crate) fn profile_switch(&self, name: &str) -> Result<bool, DynamicEngineError> {
        Ok(self.host.profile_switch(name)?)
    }

    pub(crate) fn controller_update(&self, value: Value) -> Result<(), DynamicEngineError> {
        Ok(self
            .host
            .controller_update(controller_update_from_value(value)?)?)
    }

    pub(crate) fn controller_reset(&self) -> Result<(), DynamicEngineError> {
        Ok(self.host.controller_reset()?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::host::InMemoryDynamicHost;

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

    const LUA_TIMEOUT_SOURCE: &str = r#"pokecon.opt.language = "EN"
pokecon.autocmd.on("AppShutdownPre", {
    callback = function()
        local ok, error = pcall(function()
            while true do end
        end)
        assert(not ok)
        assert(pokecon.errors.is_callback_soft_timeout(error))
        assert(pokecon.errors.CallbackSoftTimeoutError.__name == "CallbackSoftTimeoutError")
        pokecon.state.tags = {
            error.handler_id,
            error.elapsed_ms,
            error.soft_timeout_ms,
        }
    end,
    soft_timeout_ms = 1,
    soft_timeout_grace_ms = 100,
    hard_timeout_ms = 200,
})
"#;

    const PYTHON_CROSS_SOURCE: &str = r#"import pokecon

pokecon.opt.language = "EN"

def on_ready():
    pokecon.state.tags = ["python"]

pokecon.autocmd.on("AppStartupPost", callback=on_ready)

def on_timeout():
    try:
        while True:
            pass
    except pokecon.errors.CallbackSoftTimeoutError as error:
        pokecon.state.tags = [
            error.handler_id,
            error.elapsed_ms,
            error.soft_timeout_ms,
        ]

pokecon.autocmd.on(
    "AppShutdownPre",
    callback=on_timeout,
    soft_timeout_ms=1,
    soft_timeout_grace_ms=100,
    hard_timeout_ms=200,
)
pokecon.source("./extra.lua")
"#;

    const LUA_CROSS_SOURCE: &str = r##"pokecon.opt.ui.fps = 60
pokecon.autocmd.on("AppStartupPost", {
    callback = function(...)
        assert(select("#", ...) == 0)
        pokecon.state.command_candidates = {{
            name = "Example",
            module_path = "Commands.Example",
            class_name = "Example",
            tags = {"sample"},
        }}
    end,
})
"##;

    const FAILED_RELOAD_SOURCE: &str = r#"import pokecon
pokecon.opt.language = "JA"
pokecon.autocmd.clear("all")
raise RuntimeError("reload sentinel")
"#;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn embedded_luajit_commits_one_complete_generation() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("init.lua"), LUA_TIMEOUT_SOURCE).unwrap();
        let host = Arc::new(
            InMemoryDynamicHost::new(
                initial_settings(),
                BTreeMap::from([("tags".to_owned(), json!([]))]),
            )
            .unwrap(),
        );
        let engine = DynamicEngine::new(
            &config,
            Some(temporary.path().to_path_buf()),
            Some(DynamicConfigLanguage::Lua),
            host.clone(),
        )
        .unwrap();
        assert_eq!(
            engine.initialized_languages(),
            vec![DynamicConfigLanguage::Lua]
        );
        let result = engine
            .control(DynamicConfigControl::LoadPath {
                path: "init.lua".to_owned(),
            })
            .await
            .unwrap();
        assert!(result.loaded, "{:?}", result.diagnostic);
        assert_eq!(host.settings_snapshot().unwrap()["language"], json!("en"));
        assert_eq!(engine.generation(), 1);
        let event = engine.emit("AppShutdownPre").await.unwrap();
        assert_eq!(event.outcomes.len(), 1);
        let timeout = &host.state_snapshot().unwrap()["tags"];
        assert_eq!(timeout[0], json!(3));
        assert!(timeout[1].as_u64().is_some_and(|elapsed| elapsed >= 1));
        assert_eq!(timeout[2], json!(1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cross_language_source_events_and_failed_reload_preserve_generation() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("init.py"), PYTHON_CROSS_SOURCE).unwrap();
        fs::write(config.join("extra.lua"), LUA_CROSS_SOURCE).unwrap();
        let host = Arc::new(
            InMemoryDynamicHost::new(
                initial_settings(),
                BTreeMap::from([
                    ("tags".to_owned(), json!([])),
                    ("command_candidates".to_owned(), json!([])),
                ]),
            )
            .unwrap(),
        );
        let engine = DynamicEngine::new(
            &config,
            Some(temporary.path().to_path_buf()),
            Some(DynamicConfigLanguage::Python),
            host.clone(),
        )
        .unwrap();
        let result = engine
            .control(DynamicConfigControl::LoadPath {
                path: "init.py".to_owned(),
            })
            .await
            .unwrap();
        assert!(result.loaded, "{:?}", result.diagnostic);
        assert_eq!(host.settings_snapshot().unwrap()["language"], json!("en"));
        assert_eq!(host.settings_snapshot().unwrap()["ui.fps"], json!(60));
        assert_eq!(
            engine.initialized_languages(),
            vec![DynamicConfigLanguage::Python, DynamicConfigLanguage::Lua]
        );
        assert_eq!(engine.generation(), 1);

        let event = engine.emit("AppStartupPost").await.unwrap();
        assert_eq!(event.outcomes.len(), 2);
        let state = host.state_snapshot().unwrap();
        assert_eq!(state["tags"], json!(["python"]));
        assert_eq!(state["command_candidates"][0]["name"], json!("Example"));

        let timeout_event = engine.emit("AppShutdownPre").await.unwrap();
        assert_eq!(timeout_event.outcomes.len(), 1);
        let timeout = &host.state_snapshot().unwrap()["tags"];
        assert!(
            timeout[0]
                .as_u64()
                .is_some_and(|handler_id| handler_id >= 3)
        );
        assert!(timeout[1].as_u64().is_some_and(|elapsed| elapsed >= 1));
        assert_eq!(timeout[2], json!(1));

        fs::write(config.join("init.py"), FAILED_RELOAD_SOURCE).unwrap();
        let failed = engine
            .control(DynamicConfigControl::Reload {})
            .await
            .unwrap();
        assert!(!failed.loaded);
        assert!(
            failed
                .diagnostic
                .as_deref()
                .is_some_and(|message| message.contains("reload sentinel"))
        );
        assert_eq!(engine.generation(), 1);
        assert_eq!(host.settings_snapshot().unwrap()["language"], json!("en"));

        host.set_state_value("tags", json!([])).unwrap();
        host.set_state_value("command_candidates", json!([]))
            .unwrap();
        let event = engine.emit("AppStartupPost").await.unwrap();
        assert_eq!(event.outcomes.len(), 2);
        let state = host.state_snapshot().unwrap();
        assert_eq!(state["tags"], json!(["python"]));
        assert_eq!(state["command_candidates"][0]["name"], json!("Example"));
    }
}
