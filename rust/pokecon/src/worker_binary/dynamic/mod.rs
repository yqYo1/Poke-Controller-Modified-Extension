//! Child-only dynamic engine, language runtimes, and IPC dispatcher.

mod dispatch;
#[allow(
    dead_code,
    reason = "language bindings reach different private engine helpers at runtime"
)]
pub(crate) mod engine;
pub(crate) mod runtime;

pub(crate) use dispatch::DynamicWorkerRuntime;
pub(crate) use engine::{DynamicEngine, DynamicEngineError};
