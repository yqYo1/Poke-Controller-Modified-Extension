//! Private facade for canonical dynamic contracts compiled by the compatibility crate.

pub(crate) use pokecon_dynamic::protocol;
pub(crate) use pokecon_dynamic::{
    CommandCacheBuildResult, CommandDisplayCache, CommandDisplayItem, CommandInfo, Diagnostic,
    DiagnosticLevel, DynamicConfigControl, DynamicConfigLanguage, DynamicHost, DynamicHostError,
    HandlerId, merge_state_change,
};
