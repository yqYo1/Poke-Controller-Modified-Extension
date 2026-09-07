//! Persistent dynamic-configuration contracts and main-process state.
#![cfg_attr(
    target_os = "windows",
    allow(
        unused_imports,
        reason = "dynamic exports are consumed by the Linux worker runtime"
    )
)]

pub mod callback;
pub mod command;
pub mod control;
pub mod event;
pub mod host;
pub mod protocol;
pub mod source;
pub mod transaction;

pub use callback::{Diagnostic, DiagnosticLevel};
pub use command::{CommandCacheBuildResult, CommandDisplayCache, CommandDisplayItem, CommandInfo};
pub use control::{DynamicConfigControl, DynamicConfigLanguage, DynamicLoadResult};
pub use event::HandlerId;
pub use host::{DynamicHost, DynamicHostError, InMemoryDynamicHost, merge_state_change};
