//! Persistent dynamic-configuration execution primitives.
//!
//! The language runtimes build on the event and callback machinery in this
//! crate. Keeping the scheduler independent from `CPython` and `LuaJIT` makes the
//! lane, queue, and timeout rules directly testable without either interpreter.

pub mod callback;
pub mod command;
pub mod control;
pub mod engine;
pub mod event;
pub mod host;
mod runtime;
pub mod source;
pub mod transaction;

pub use callback::{
    Callback, CallbackError, CallbackErrorKind, CallbackExecutor, CallbackLimits, CallbackOutcome,
    CallbackReturn, CallbackSettings, DeadlineSignal, Diagnostic, DiagnosticLevel, DiagnosticSink,
    Invocation, InvocationContext, NoopDiagnosticSink, TimeoutStage,
};
pub use command::{
    CommandCallbackKind, CommandDisplayItem, CommandError, CommandInfo, CommandOptionField,
    CommandOptionValue, CommandRegistry, SORT_HANDLER_ID, TAG_MATCH_HANDLER_ID,
};
pub use control::{DynamicConfigControl, DynamicConfigLanguage, DynamicLoadResult, DynamicSource};
pub use event::{
    BUILTIN_EVENTS, BuiltinEvent, EventBus, EventError, EventResult, HandlerId, RegistrationOptions,
};
pub use host::{DynamicHost, DynamicHostError, DynamicSettingsRegistry, InMemoryDynamicHost};
pub use source::{ResolvedSource, SourceError, SourceStore};
pub use transaction::{EvaluationTransaction, StagedEventOperation, TransactionError};
