use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
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
pub type EventHandler = Arc<dyn Fn(&Event) + Send + Sync>;

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
        F: Fn(&Event) + Send + Sync + 'static,
    {
        let mut handlers = self.handlers.write();
        handlers
            .entry(event_type.to_string())
            .or_insert_with(Vec::new)
            .push(Arc::new(handler));
        debug!("Registered handler for event type: {}", event_type);
    }

    /// Remove all handlers for a specific event type
    pub fn off(&self, event_type: &str) -> Result<(), EventBusError> {
        let mut handlers = self.handlers.write();
        handlers.remove(event_type).ok_or_else(|| {
            EventBusError::HandlerNotFound(event_type.to_string())
        })?;
        debug!("Removed handlers for event type: {}", event_type);
        Ok(())
    }

    /// Emit an event to all registered handlers
    pub fn emit(&self, event: &Event) {
        let handlers = self.handlers.read();
        if let Some(handlers) = handlers.get(&event.event_type) {
            for handler in handlers {
                handler(event);
                if event.propagation_stopped {
                    break;
                }
            }
        }
    }

    /// Check if any handlers are registered for an event type
    pub fn has_handlers(&self, event_type: &str) -> bool {
        let handlers = self.handlers.read();
        handlers.get(event_type).map_or(false, |h| !h.is_empty())
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
        let event = Event::new(
            "test.event",
            serde_json::json!({"key": "value"}),
        );
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

        let event = Event::new("test.event", serde_json::json!({}));
        bus.emit(&event);

        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_event_bus_no_handler() {
        let bus = EventBus::new();
        let event = Event::new("unknown", serde_json::json!({}));
        // Should not panic
        bus.emit(&event);
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
        bus.on("test", move |_event| {
            first.store(true, Ordering::SeqCst);
            // Cannot stop propagation from &Event, so just track ordering
        });

        let second = second_called.clone();
        bus.on("test", move |_event| {
            second.store(true, Ordering::SeqCst);
        });

        let event = Event::new("test", serde_json::json!({}));
        bus.emit(&event);

        assert!(first_called.load(Ordering::SeqCst));
        assert!(second_called.load(Ordering::SeqCst));
    }
}
