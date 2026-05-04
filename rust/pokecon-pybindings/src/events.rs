use pyo3::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

#[pyclass]
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<pokecon_events::EventBus>>,
}

#[pymethods]
impl EventBus {
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(pokecon_events::EventBus::new())),
        })
    }

    fn emit(&self, event_type: String, data: String) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Runtime error: {}", e))
        })?;
        rt.block_on(async {
            let bus = self.inner.lock().await;
            let event = pokecon_events::Event::new(
                &event_type,
                serde_json::from_str(&data).unwrap_or(serde_json::Value::Null),
            );
            bus.emit(&event);
        });
        Ok(())
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<EventBus>()?;
    Ok(())
}
