//! Canonical, machine-readable contracts shared by every application surface.
//!
//! The JSON files in `registry/` are the single source of truth. This crate is
//! deliberately free of application/runtime dependencies so generators, CI,
//! workers, the server, and the UI build can all consume the same contracts.

pub mod dynamic_typings;
pub mod model;
mod validate;

pub use dynamic_typings::{dynamic_lua_typings, dynamic_python_typings};
pub use model::{Access, DefaultValue, Mutability, Scope, Setting, SettingsRegistry, ValueSchema};
pub use validate::{ContractError, ValidatedSettingsRegistry};

/// Raw canonical setting registry, embedded for deterministic consumers.
pub const SETTINGS_REGISTRY_JSON: &str = include_str!("../registry/settings.json");

/// Raw canonical protocol and public-surface registry.
pub const PROTOCOL_REGISTRY_JSON: &str = include_str!("../registry/protocol.json");

/// Raw immutable compatibility-baseline registry.
pub const COMPATIBILITY_REGISTRY_JSON: &str = include_str!("../registry/compatibility.json");

/// Raw generated-artifact and drift-check registry.
pub const GENERATION_REGISTRY_JSON: &str = include_str!("../registry/generation.json");

/// Raw CI applicability matrix.
pub const CI_REGISTRY_JSON: &str = include_str!("../registry/ci.json");

/// Raw test taxonomy, fixture convention, and future-path audit.
pub const FOUNDATION_REGISTRY_JSON: &str = include_str!("../registry/foundation.json");

/// Parse and semantically validate the canonical settings registry.
///
/// # Errors
///
/// Returns [`ContractError`] when JSON parsing or any registry invariant fails.
pub fn settings_registry() -> Result<ValidatedSettingsRegistry, ContractError> {
    let registry = serde_json::from_str::<SettingsRegistry>(SETTINGS_REGISTRY_JSON)?;
    ValidatedSettingsRegistry::try_from(registry)
}

/// Parse an embedded non-settings registry and require a JSON object root.
///
/// # Errors
///
/// Returns [`ContractError`] when JSON parsing fails or the root is not an
/// object. Detailed cross-registry invariants live in the contract tests.
pub fn parse_object_registry(
    name: &'static str,
    source: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, ContractError> {
    let value = serde_json::from_str::<serde_json::Value>(source)?;
    value
        .as_object()
        .cloned()
        .ok_or(ContractError::NonObjectRegistry(name))
}
