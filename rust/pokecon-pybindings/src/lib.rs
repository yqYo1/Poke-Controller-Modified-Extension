mod python_cmd;
mod image_proc;
mod keys;
mod events;

use pyo3::prelude::*;

#[pymodule]
fn pokecon(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
