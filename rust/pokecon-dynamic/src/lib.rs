//! Persistent dynamic-configuration execution primitives.
//!
//! The language runtimes build on the event and callback machinery in this
//! crate. Keeping the scheduler independent from `CPython` and `LuaJIT` makes the
//! lane, queue, and timeout rules directly testable without either interpreter.

pub mod callback;
pub mod event;

pub use callback::{
    Callback, CallbackError, CallbackErrorKind, CallbackExecutor, CallbackLimits, CallbackOutcome,
    CallbackReturn, CallbackSettings, DeadlineSignal, Diagnostic, DiagnosticLevel, DiagnosticSink,
    Invocation, InvocationContext, NoopDiagnosticSink, TimeoutStage,
};
pub use event::{
    BUILTIN_EVENTS, BuiltinEvent, EventBus, EventError, EventResult, HandlerId, RegistrationOptions,
};
