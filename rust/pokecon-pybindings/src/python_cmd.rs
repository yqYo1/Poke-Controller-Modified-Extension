use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[pyclass]
pub struct PythonCommand {
    name: String,
    callbacks: Arc<Mutex<HashMap<String, PyObject>>>,
}

#[pymethods]
impl PythonCommand {
    #[new]
    fn new(name: String) -> Self {
        Self {
            name,
            callbacks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.name.clone())
    }

    fn register_callback(&self, event: String, callback: PyObject) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Runtime error: {}", e))
        })?;
        rt.block_on(async {
            let mut callbacks = self.callbacks.lock().await;
            callbacks.insert(event, callback);
        });
        Ok(())
    }

    fn trigger<'py>(
        &self,
        py: Python<'py>,
        event: String,
        kwargs: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Runtime error: {}", e))
        })?;

        let callback: Option<PyObject> = rt.block_on(async {
            let callbacks = self.callbacks.lock().await;
            callbacks.get(&event).map(|cb| cb.clone_ref(py))
        });

        match callback {
            Some(cb) => {
                let args = pyo3::types::PyTuple::empty(py);
                match kwargs {
                    Some(kwargs) => cb
                        .call(py, args, Some(&kwargs))
                        .map(|o| Some(o.into_bound(py))),
                    None => cb.call(py, args, None).map(|o| Some(o.into_bound(py))),
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
