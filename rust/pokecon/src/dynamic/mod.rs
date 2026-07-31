//! Persistent dynamic-configuration contracts and main-process state.

pub mod callback;
pub mod command;
pub mod control;
pub mod event;
pub mod host;
pub mod protocol;
pub mod source;
pub mod transaction;

pub use callback::{
    Callback, CallbackError, CallbackErrorKind, CallbackExecutor, CallbackLimits, CallbackOutcome,
    CallbackReturn, CallbackSettings, DeadlineSignal, Diagnostic, DiagnosticLevel, DiagnosticSink,
    Invocation, InvocationContext, NoopDiagnosticSink, TimeoutStage,
};
pub use command::{
    CommandCacheBuildResult, CommandCallbackKind, CommandDisplayCache, CommandDisplayItem,
    CommandError, CommandInfo, CommandOptionField, CommandOptionValue, CommandRegistry,
    SORT_HANDLER_ID, TAG_MATCH_HANDLER_ID,
};
pub use control::{DynamicConfigControl, DynamicConfigLanguage, DynamicLoadResult, DynamicSource};
pub use event::{
    BUILTIN_EVENTS, BuiltinEvent, EventBus, EventError, EventResult, HandlerId, RegistrationOptions,
};
pub use host::{
    DynamicHost, DynamicHostError, DynamicSettingsRegistry, InMemoryDynamicHost, merge_state_change,
};
pub use source::{ResolvedSource, SourceError, SourceStore};
pub use transaction::{EvaluationTransaction, StagedEventOperation, TransactionError};
