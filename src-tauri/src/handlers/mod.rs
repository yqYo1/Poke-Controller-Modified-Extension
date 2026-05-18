//! Handlers sub-module — all HTTP request handlers organized by domain.
//!
//! This module provides shared types used across handlers.

// Shared request types used in multiple handlers
pub use serde::{Deserialize, Serialize};
pub use utoipa::ToSchema;

/// A request containing just a name — used for command loading, profile activation, etc.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct NameRequest {
    pub name: String,
}

pub mod camera;
pub mod commands;
pub mod controller;
pub mod core;
pub mod input;
pub mod notifications;
pub mod profile;
pub mod serial;
pub mod websocket;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_request_serialize_roundtrip() {
        let req = NameRequest {
            name: "test-command".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: NameRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test-command");
    }

    #[test]
    fn test_name_request_deserialize() {
        let json = r#"{"name": "my-profile"}"#;
        let req: NameRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "my-profile");
    }

    #[test]
    fn test_name_request_json_format() {
        let req = NameRequest {
            name: "test".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"name":"test"}"#);
    }
}
