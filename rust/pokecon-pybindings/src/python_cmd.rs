use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::sync::Mutex;

/// A Python command that holds named callbacks.
///
/// This is the Python-facing representation of a command.  User scripts register
/// callback functions (e.g. ``"do"``) that the Rust runtime can invoke
/// synchronously.
///
/// Python usage::
///
///     from pokecon.command import PythonCommand
///
///     def on_do(**kwargs):
///         print("do called with", kwargs)
///
///     cmd = PythonCommand("my_command")
///     cmd.register_callback("do", on_do)
///     result = cmd.trigger("do", {"arg": 42})
#[pyclass]
pub struct PythonCommand {
    name: String,
    callbacks: Mutex<HashMap<String, PyObject>>,
}

#[pymethods]
impl PythonCommand {
    #[new]
    fn new(name: String) -> Self {
        Self {
            name,
            callbacks: Mutex::new(HashMap::new()),
        }
    }

    /// The name of this command.
    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.name.clone())
    }

    /// Register a Python callback for a named event (e.g. ``"do"``).
    fn register_callback(&self, event: String, callback: PyObject) -> PyResult<()> {
        let mut callbacks = self.callbacks.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
        })?;
        callbacks.insert(event, callback);
        Ok(())
    }

    /// Trigger the callback registered for *event*, passing *kwargs* as keyword
    /// arguments.  Returns the callback's return value, or ``None`` if no
    /// callback is registered.
    fn trigger<'py>(
        &self,
        py: Python<'py>,
        event: String,
        kwargs: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        // Clone the callback while holding the lock so we can release it
        // before calling into Python (avoids potential deadlocks).
        let cb = {
            let callbacks = self.callbacks.lock().map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
            })?;
            callbacks.get(&event).cloned()
        };

        match cb {
            Some(cb) => {
                let args = pyo3::types::PyTuple::empty(py);
                match kwargs {
                    Some(ref kwargs) => cb.bind(py).call(args, Some(kwargs)).map(Some),
                    None => cb.bind(py).call(args, None).map(Some),
                }
            }
            None => Ok(None),
        }
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PythonCommand>()?;
    Ok(())
}
