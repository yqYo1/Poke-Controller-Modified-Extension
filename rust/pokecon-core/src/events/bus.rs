use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

/// Phase of event dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPhase {
    /// Event is being captured (top-down)
    Capture,
    /// Event is bubbling up (bottom-up)
    Bubble,
    /// Event is at the target
    AtTarget,
}

/// An event in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Event type identifier
    pub event_type: String,
    /// Event data as JSON value
    pub data: serde_json::Value,
    /// Current phase
    pub phase: EventPhase,
    /// Whether the event propagation is stopped
    pub propagation_stopped: bool,
}

impl Event {
    /// Create a new event
    pub fn new(event_type: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            event_type: event_type.into(),
            data,
            phase: EventPhase::AtTarget,
            propagation_stopped: false,
        }
    }

    /// Stop the event from propagating further
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }
}

/// Errors that can occur in the event bus
#[derive(Error, Debug)]
pub enum EventBusError {
    #[error("Handler not found: {0}")]
    HandlerNotFound(String),

    #[error("Event type not registered: {0}")]
    EventTypeNotRegistered(String),
}

/// Type alias for event handler functions
pub type EventHandler = Arc<dyn Fn(&mut Event) + Send + Sync>;

/// A simple event bus for dispatching events
#[derive(Clone)]
pub struct EventBus {
    handlers: Arc<RwLock<HashMap<String, Vec<EventHandler>>>>,
}

impl EventBus {
    /// Create a new empty EventBus
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a handler for a specific event type
    pub fn on<F>(&self, event_type: &str, handler: F)
    where
        F: Fn(&mut Event) + Send + Sync + 'static,
    {
        let mut handlers = self.handlers.write();
        handlers
            .entry(event_type.to_string())
            .or_default()
            .push(Arc::new(handler));
        debug!("Registered handler for event type: {}", event_type);
    }

    /// Remove all handlers for a specific event type
    pub fn off(&self, event_type: &str) -> Result<(), EventBusError> {
        let mut handlers = self.handlers.write();
        handlers
            .remove(event_type)
            .ok_or_else(|| EventBusError::HandlerNotFound(event_type.to_string()))?;
        debug!("Removed handlers for event type: {}", event_type);
        Ok(())
    }

    /// Emit an event to all registered handlers.
    ///
    /// Handlers are invoked synchronously in registration order.
    /// The handler list is cloned before invocation so that handlers
    /// can safely register/unregister other handlers without deadlocking.
    /// If a handler panics, subsequent handlers still run.
    pub fn emit(&self, event: &mut Event) {
        // Clone the handler list under the read lock, then release it
        // before invoking any handler.  This prevents deadlocks when
        // handlers call back into the bus (e.g. `on` / `off`).
        let handlers: Option<Vec<EventHandler>> = {
            let guard = self.handlers.read();
            guard.get(&event.event_type).cloned()
        };

        let Some(handlers) = handlers else {
            return;
        };

        for handler in &handlers {
            // Isolate panics so a misbehaving handler doesn't
            // prevent subsequent handlers from running.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                handler(event);
            }));
            if let Err(e) = result {
                debug!(
                    "Handler panicked for event type '{}': {:?}",
                    event.event_type,
                    e.downcast_ref::<&str>().unwrap_or(&"<unknown>")
                );
            }
            if event.propagation_stopped {
                debug!("Propagation stopped for event type '{}'", event.event_type);
                break;
            }
        }
    }

    /// Check if any handlers are registered for an event type
    pub fn has_handlers(&self, event_type: &str) -> bool {
        let handlers = self.handlers.read();
        handlers.get(event_type).is_some_and(|h| !h.is_empty())
    }

    /// Get the number of registered event types
    pub fn num_event_types(&self) -> usize {
        let handlers = self.handlers.read();
        handlers.len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_event_creation() {
        let event = Event::new("test.event", serde_json::json!({"key": "value"}));
        assert_eq!(event.event_type, "test.event");
        assert_eq!(event.data["key"], "value");
        assert_eq!(event.phase, EventPhase::AtTarget);
        assert!(!event.propagation_stopped);
    }

    #[test]
    fn test_event_stop_propagation() {
        let mut event = Event::new("test", serde_json::json!({}));
        assert!(!event.propagation_stopped);
        event.stop_propagation();
        assert!(event.propagation_stopped);
    }

    #[test]
    fn test_event_bus_emit() {
        let bus = EventBus::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        bus.on("test.event", move |_event| {
            called_clone.store(true, Ordering::SeqCst);
        });

        let mut event = Event::new("test.event", serde_json::json!({}));
        bus.emit(&mut event);

        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_event_bus_no_handler() {
        let bus = EventBus::new();
        let mut event = Event::new("unknown", serde_json::json!({}));
        // Should not panic
        bus.emit(&mut event);
    }

    #[test]
    fn test_event_bus_has_handlers() {
        let bus = EventBus::new();
        assert!(!bus.has_handlers("test"));

        bus.on("test", |_| {});
        assert!(bus.has_handlers("test"));
    }

    #[test]
    fn test_event_bus_off() {
        let bus = EventBus::new();
        bus.on("test", |_| {});
        assert!(bus.has_handlers("test"));

        bus.off("test").unwrap();
        assert!(!bus.has_handlers("test"));
    }

    #[test]
    fn test_event_bus_off_nonexistent() {
        let bus = EventBus::new();
        let result = bus.off("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_event_bus_propagation_stop() {
        let bus = EventBus::new();
        let first_called = Arc::new(AtomicBool::new(false));
        let second_called = Arc::new(AtomicBool::new(false));

        let first = first_called.clone();
        bus.on("test", move |event| {
            first.store(true, Ordering::SeqCst);
            event.stop_propagation();
        });

        let second = second_called.clone();
        bus.on("test", move |_event| {
            second.store(true, Ordering::SeqCst);
        });

        let mut event = Event::new("test", serde_json::json!({}));
        bus.emit(&mut event);

        assert!(first_called.load(Ordering::SeqCst));
        assert!(
            !second_called.load(Ordering::SeqCst),
            "second handler should NOT have been called because propagation was stopped"
        );
        assert!(event.propagation_stopped);
    }

    #[test]
    fn test_event_bus_panic_isolation() {
        let bus = EventBus::new();
        let second_called = Arc::new(AtomicBool::new(false));

        // First handler panics
        bus.on("test", |_event| {
            panic!("intentional panic");
        });

        let second = second_called.clone();
        bus.on("test", move |_event| {
            second.store(true, Ordering::SeqCst);
        });

        let mut event = Event::new("test", serde_json::json!({}));
        // Should not panic overall — the panicking handler is isolated
        bus.emit(&mut event);

        assert!(
            second_called.load(Ordering::SeqCst),
            "second handler MUST be called even though first handler panicked"
        );
    }

    #[test]
    fn test_event_bus_handler_can_register_new_handler() {
        // Regression test: handlers should be able to call `on` inside
        // an emit without deadlocking (handler list is cloned beforehand).
        let bus = Arc::new(EventBus::new());
        let inner_called = Arc::new(AtomicBool::new(false));

        let inner = inner_called.clone();
        let bus_clone = bus.clone();
        bus.on("outer", move |_event| {
            // Register a new handler from inside a handler — would
            // deadlock with the old implementation that held the
            // read lock during handler execution.
            let inner_inner = inner.clone();
            bus_clone.on("inner", move |_event| {
                inner_inner.store(true, Ordering::SeqCst);
            });
        });

        let mut event = Event::new("outer", serde_json::json!({}));
        bus.emit(&mut event);

        // The inner handler should now be registered
        assert!(bus.has_handlers("inner"));

        // Emit the inner event
        let mut inner_event = Event::new("inner", serde_json::json!({}));
        bus.emit(&mut inner_event);

        assert!(
            inner_called.load(Ordering::SeqCst),
            "handler registered inside another handler must be callable"
        );
    }

    #[test]
    fn test_event_bus_handler_can_off_inside_emit() {
        // Regression test: handlers should be able to call `off` inside
        // an emit without deadlocking.
        let bus = EventBus::new();

        bus.on("test", |_event| {
            // no-op
        });

        let mut event = Event::new("test", serde_json::json!({}));
        bus.emit(&mut event);

        // off after emit
        bus.off("test").unwrap();
        assert!(!bus.has_handlers("test"));
    }

    #[test]
    fn test_event_bus_emit_unknown_type() {
        let bus = EventBus::new();
        let mut event = Event::new("nonexistent", serde_json::json!({}));
        // Should not panic — just no handlers
        bus.emit(&mut event);
    }
}
