//! Event type registry

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Metadata for a registered event type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTypeInfo {
    /// The event type string
    pub event_type: String,
    /// Human-readable description
    pub description: String,
}

/// Registry for event types and their metadata
#[derive(Clone)]
pub struct EventRegistry {
    types: Arc<RwLock<HashMap<String, EventTypeInfo>>>,
}

impl EventRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            types: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an event type
    pub fn register(&self, event_type: &str, description: &str) {
        let mut types = self.types.write();
        types.insert(
            event_type.to_string(),
            EventTypeInfo {
                event_type: event_type.to_string(),
                description: description.to_string(),
            },
        );
    }

    /// Check if an event type is registered
    pub fn is_registered(&self, event_type: &str) -> bool {
        let types = self.types.read();
        types.contains_key(event_type)
    }

    /// Get info about a registered event type
    pub fn get_info(&self, event_type: &str) -> Option<EventTypeInfo> {
        let types = self.types.read();
        types.get(event_type).cloned()
    }

    /// List all registered event types
    pub fn list_types(&self) -> Vec<EventTypeInfo> {
        let types = self.types.read();
        types.values().cloned().collect()
    }
}

impl Default for EventRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_check() {
        let registry = EventRegistry::new();
        assert!(!registry.is_registered("test.event"));

        registry.register("test.event", "A test event");
        assert!(registry.is_registered("test.event"));
    }

    #[test]
    fn test_get_info() {
        let registry = EventRegistry::new();
        registry.register("test.event", "A test event");

        let info = registry.get_info("test.event").unwrap();
        assert_eq!(info.event_type, "test.event");
        assert_eq!(info.description, "A test event");
    }

    #[test]
    fn test_get_info_nonexistent() {
        let registry = EventRegistry::new();
        assert!(registry.get_info("nonexistent").is_none());
    }

    #[test]
    fn test_list_types() {
        let registry = EventRegistry::new();
        registry.register("event.a", "Event A");
        registry.register("event.b", "Event B");

        let types = registry.list_types();
        assert_eq!(types.len(), 2);
    }
}
