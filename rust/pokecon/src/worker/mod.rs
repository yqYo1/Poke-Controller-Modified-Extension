//! Managed worker IPC, generation gates, and operating-system supervision.

pub mod dynamic;
pub mod generation;
pub mod ipc;
pub mod script;
pub mod supervisor;

use serde::{Deserialize, Serialize};

/// Worker roles isolated by the process model in the specification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    /// Profile-specific user-script `CPython` worker.
    Script,
    /// Persistent dynamic Python/Lua configuration worker.
    Dynamic,
}
