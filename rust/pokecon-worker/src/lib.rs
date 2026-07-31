//! Compatibility facade for canonical worker supervision owned by `pokecon`.

pub(crate) use pokecon_dynamic as dynamic_domain;

#[path = "../../pokecon/src/worker/mod.rs"]
mod worker;

pub use worker::*;
