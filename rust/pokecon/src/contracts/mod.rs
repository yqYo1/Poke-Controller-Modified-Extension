//! Canonical, machine-readable contracts shared by every application surface.
//!
//! The JSON files in `registry/` are the single source of truth. This module is
//! deliberately free of application/runtime dependencies so generators, CI,
//! workers, the server, and the UI build can all consume the same contracts.

pub mod commands_typings;
pub mod dynamic_typings;
pub mod model;
pub mod settings_artifacts;
mod validate;

pub use commands_typings::commands_python_typings;
pub use dynamic_typings::{dynamic_lua_typings, dynamic_python_typings};
pub use model::SettingsRegistry;
pub use settings_artifacts::{settings_json_schema, settings_ui_metadata, value_json_schema};
pub use validate::{ContractError, ValidatedSettingsRegistry};

/// Raw canonical setting registry, embedded for deterministic consumers.
pub const SETTINGS_REGISTRY_JSON: &str = include_str!("../../registry/settings.json");

/// Raw canonical protocol and public-surface registry.
pub const PROTOCOL_REGISTRY_JSON: &str = include_str!("../../registry/protocol.json");

/// Raw immutable fixed-script inventory used by the compatibility projection.
pub const COMPATIBILITY_FIXED_MANIFEST_JSON: &str =
    include_str!("../../../../compatibility/fixed-manifest.json");

/// Parse and semantically validate the canonical settings registry.
///
/// # Errors
///
/// Returns [`ContractError`] when JSON parsing or any registry invariant fails.
pub fn settings_registry() -> Result<ValidatedSettingsRegistry, ContractError> {
    let registry = serde_json::from_str::<SettingsRegistry>(SETTINGS_REGISTRY_JSON)?;
    ValidatedSettingsRegistry::try_from(registry)
}

/// Checks every tracked settings and scripting artifact under a repository root.
///
/// # Errors
///
/// Returns an error when the root is invalid, canonical contracts cannot be loaded, or a
/// generated output is missing or stale.
#[cfg(feature = "contract-generator")]
pub fn check_generated_artifacts(
    repository_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    crate::contract_generator::check_generated_artifacts(repository_root)
}

/// Checks an `OpenAPI` artifact against the server schema generator.
///
/// # Errors
///
/// Returns an error when the document cannot be generated or the artifact is missing or stale.
#[cfg(feature = "contract-generator")]
pub fn check_openapi_artifact(output: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    crate::openapi_generator::check_openapi_artifact(output)
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
