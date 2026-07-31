//! Compatibility facade for canonical dynamic contracts and state owned by `pokecon`.

#[allow(
    dead_code,
    reason = "engine-only helpers are consumed by the worker binary's private copy"
)]
#[path = "../../pokecon/src/dynamic/mod.rs"]
mod dynamic;

pub use dynamic::*;
