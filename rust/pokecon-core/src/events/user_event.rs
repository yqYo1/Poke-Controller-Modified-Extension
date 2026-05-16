//! User-defined events for the Poke-Controller event system

use serde::{Deserialize, Serialize};

/// A user-defined event with custom data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEvent {
    /// The name/type of this user event
    pub name: String,
    /// Arbitrary JSON data payload
    pub data: serde_json::Value,
}

impl UserEvent {
    /// Create a new user event
    pub fn new(name: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_event_creation() {
        let event = UserEvent::new("my_event", serde_json::json!({"key": 42}));
        assert_eq!(event.name, "my_event");
        assert_eq!(event.data["key"], 42);
    }

    #[test]
    fn test_user_event_serde() {
        let event = UserEvent::new("test", serde_json::json!({"value": "hello"}));
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: UserEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, event.name);
        assert_eq!(deserialized.data, event.data);
    }
}
