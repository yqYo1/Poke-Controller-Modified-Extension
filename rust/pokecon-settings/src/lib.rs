//! Canonical settings, persistence, and managed Python-environment services.
//!
//! Every public settings surface consumes the same embedded registry from
//! `pokecon-contracts`. Runtime code must not maintain a second handwritten
//! setting list.

pub mod hmac_key;
pub mod lock;
pub mod manifest;
pub mod package;
pub mod path;
pub mod persistence;
pub mod pipeline;
pub mod python;
pub mod roots;
pub mod scaffold;
pub mod service;
pub mod uv;
pub mod venv;

/// Build-time projection of the canonical Python dependency source sets.
pub const APPLICATION_REQUIREMENTS_JSON: &str =
    include_str!(concat!(env!("OUT_DIR"), "/application-requirements.json"));

/// Build-time identity of the Nix-pinned uv executable, or `null` for
/// non-packaged compile-only environments.
pub const MANAGED_UV_SOURCE_JSON: &str =
    include_str!(concat!(env!("OUT_DIR"), "/managed-uv-source.json"));
