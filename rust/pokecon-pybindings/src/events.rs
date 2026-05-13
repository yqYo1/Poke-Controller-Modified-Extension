use pyo3::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// A synchronous event bus for registering and dispatching Python callbacks.
///
/// Python usage::
///
///     from pokecon.events import EventBus
///
///     def handler(event_type, data):
///         print(f"Got {event_type}: {data}")
///
///     bus = EventBus()
///     bus.on("my_event", handler)
///     bus.emit("my_event", '{"key": "value"}')
///
/// This class is intentionally **not** async.  All callbacks are invoked
/// synchronously from the calling thread.
#[pyclass]
pub struct EventBus {
    /// Python callbacks keyed by event type string.
    callbacks: Mutex<HashMap<String, Vec<PyObject>>>,
}

#[pymethods]
impl EventBus {
    #[new]
    fn new() -> Self {
        Self {
            callbacks: Mutex::new(HashMap::new()),
        }
    }

    /// Register a callback for *event_type*.
    ///
    /// The callback receives two positional arguments: ``(event_type, data)``
    /// where *data* is the JSON string passed to :meth:`emit`.
    fn on(&self, event_type: String, callback: PyObject) -> PyResult<()> {
        let mut inner = self.callbacks.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
        })?;
        inner.entry(event_type).or_default().push(callback);
        Ok(())
    }

    /// Remove all callbacks registered for *event_type*.
    fn off(&self, event_type: &str) -> PyResult<()> {
        let mut inner = self.callbacks.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
        })?;
        inner.remove(event_type);
        Ok(())
    }

    /// Emit an event, calling all registered callbacks synchronously.
    ///
    /// Each callback is called with ``(event_type, data)``.
    ///
    /// To avoid deadlocks, the callback list is cloned before invocation so
    /// that callbacks can safely call back into the bus.
    fn emit(&self, py: Python<'_>, event_type: String, data: String) -> PyResult<()> {
        // Clone callbacks while holding the lock, then release before calling Python
        let callbacks: Vec<PyObject> = {
            let inner = self.callbacks.lock().map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
            })?;
            inner
                .get(&event_type)
                .map_or_else(Vec::new, |cbs| cbs.clone())
        };

        for cb in &callbacks {
            let args = (event_type.clone(), data.clone());
            cb.bind(py).call(args, None)?;
        }

        Ok(())
    }

    /// Return ``True`` if at least one callback is registered for *event_type*.
    fn has_handlers(&self, event_type: &str) -> bool {
        self.callbacks
            .lock()
            .map(|inner| inner.contains_key(event_type))
            .unwrap_or(false)
    }

    /// Return the number of registered event types.
    fn num_event_types(&self) -> usize {
        self.callbacks.lock().map(|inner| inner.len()).unwrap_or(0)
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<EventBus>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_event_bus_empty() {
        let bus = EventBus::new();
        assert!(!bus.has_handlers("test"));
        assert_eq!(bus.num_event_types(), 0);
    }

    #[test]
    fn test_on_registers_callback() {
        let bus = EventBus::new();
        Python::with_gil(|py| {
            let obj = py.None();
            bus.on("evt".to_string(), obj).unwrap();
            assert!(bus.has_handlers("evt"));
            assert_eq!(bus.num_event_types(), 1);
        });
    }

    #[test]
    fn test_on_off_removes_all() {
        let bus = EventBus::new();
        Python::with_gil(|py| {
            let obj = py.None();
            bus.on("evt".to_string(), obj).unwrap();
            assert!(bus.has_handlers("evt"));
            bus.off("evt").unwrap();
            assert!(!bus.has_handlers("evt"));
            assert_eq!(bus.num_event_types(), 0);
        });
    }

    #[test]
    fn test_multiple_callbacks_same_event() {
        let bus = EventBus::new();
        Python::with_gil(|py| {
            let obj1: PyObject = py.None();
            let obj2: PyObject = py.None();
            bus.on("evt".to_string(), obj1).unwrap();
            bus.on("evt".to_string(), obj2).unwrap();
            assert!(bus.has_handlers("evt"));
            assert_eq!(bus.num_event_types(), 1);
        });
    }

    #[test]
    fn test_off_nonexistent_is_error() {
        let bus = EventBus::new();
        // off on non-existent key should still return Ok (it's a remove)
        bus.off("nonexistent").unwrap();
    }
}
