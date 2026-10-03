//! Narrow entrypoints for executables owned by the `pokecon` package.
//!
//! The implementation modules stay private to the library crate. Each Cargo
//! binary calls only its feature-gated function here, so internal domain
//! modules do not become a public reuse API merely to cross a Cargo target
//! boundary.

/// Runs the immutable compatibility-corpus discovery tool.
///
/// # Errors
///
/// Returns an error when the managed worker cannot be initialized, queried,
/// or stopped, or when the resulting inventory cannot be serialized.
#[cfg(feature = "compatibility-tool")]
pub fn compatibility() -> Result<(), Box<dyn std::error::Error>> {
    crate::compatibility_tool::run()
}

/// Checks or writes the generated settings and scripting contracts.
///
/// # Errors
///
/// Returns an error when canonical contracts cannot be loaded or an output is
/// missing, stale, or unwritable.
#[cfg(feature = "contract-generator")]
pub fn generate_contracts() -> Result<(), Box<dyn std::error::Error>> {
    crate::contract_generator::run()
}

/// Checks or writes the generated `OpenAPI` document.
///
/// # Errors
///
/// Returns an error when the API document cannot be generated, read, or
/// written.
#[cfg(feature = "contract-generator")]
pub fn generate_openapi() -> Result<(), Box<dyn std::error::Error>> {
    crate::openapi_generator::run()
}

/// Runs the managed worker process selected by `--kind`.
///
/// # Errors
///
/// Returns an error when tracing, signal handling, IPC, or a worker runtime
/// cannot be initialized or completed.
#[cfg(feature = "worker-binary")]
pub fn worker() -> Result<(), Box<dyn std::error::Error>> {
    crate::worker_binary::main().map_err(Into::into)
}
