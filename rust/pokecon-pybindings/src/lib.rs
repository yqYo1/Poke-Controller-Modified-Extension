//! Minimal `PyO3` boundary for the Python compatibility package.

use pyo3::prelude::*;

#[pyfunction]
fn runtime_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pyfunction]
fn target_platform() -> &'static str {
    std::env::consts::OS
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(runtime_version, module)?)?;
    module.add_function(wrap_pyfunction!(target_platform, module)?)?;
    Ok(())
}
