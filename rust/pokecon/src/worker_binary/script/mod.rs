//! Child-only `CPython` actor and script protocol dispatcher.

pub(crate) use crate::worker::script::protocol;

mod python;
mod runtime;

pub(crate) use runtime::ScriptWorkerRuntime;
