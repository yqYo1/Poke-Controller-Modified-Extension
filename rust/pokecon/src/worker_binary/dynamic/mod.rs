//! Child-only dynamic engine, language runtimes, and IPC dispatcher.

mod dispatch;
#[allow(
    dead_code,
    unused_imports,
    clippy::struct_field_names,
    clippy::unused_self,
    reason = "the canonical domain is compiled privately with only child-side entrypoints reachable"
)]
#[path = "../../dynamic/mod.rs"]
mod domain;
#[allow(
    dead_code,
    reason = "language bindings reach different private engine helpers at runtime"
)]
pub(crate) mod engine;
pub(crate) mod runtime;

pub(crate) use dispatch::DynamicWorkerRuntime;
#[allow(
    unused_imports,
    reason = "canonical domain modules resolve their shared types through this child-only facade"
)]
pub(crate) use domain::*;
pub(crate) use engine::{DynamicEngine, DynamicEngineError};
