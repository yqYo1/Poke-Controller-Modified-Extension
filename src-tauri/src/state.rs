use std::sync::Arc;

use pokecon_core::command_manager::CommandManager;
use pokecon_core::cv::camera::Camera;
use pokecon_core::events::EventBus;
use pokecon_core::profile::ProfileManager;
use pokecon_core::serial::keypress::KeyPress;
use pokecon_core::serial::sender::Sender;
use pokecon_core::settings::NotifySettings;
use tokio::sync::Mutex;
use utoipa::ToSchema;

use crate::webrtc;

/// Configuration for mouse-to-stick control.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct MouseStickConfig {
    /// Whether left-stick mouse control is enabled
    pub left_enabled: bool,
    /// Whether right-stick mouse control is enabled
    pub right_enabled: bool,
    /// Sensitivity multiplier (default: 1.0)
    #[serde(default)]
    pub sensitivity: f32,
}

impl Default for MouseStickConfig {
    fn default() -> Self {
        Self {
            left_enabled: false,
            right_enabled: false,
            sensitivity: 1.0,
        }
    }
}

/// Shared application state accessible from all HTTP handlers.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub serial: Arc<Mutex<Sender>>,
    pub keypress: Arc<Mutex<KeyPress>>,
    /// Filter string for commands
    pub command_filter: Arc<Mutex<String>>,
    pub command_manager: Arc<Mutex<CommandManager>>,
    pub event_bus: EventBus,
    pub camera: Arc<Mutex<Option<Camera>>>,
    /// Broadcast channel for WebSocket event forwarding
    pub event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Current gamepad type ("ProController" or "Xinput")
    pub gamepad_type: Arc<Mutex<String>>,
    /// Whether keyboard input is enabled
    pub keyboard_enabled: Arc<Mutex<bool>>,
    /// Profile manager
    pub profile_manager: Arc<Mutex<ProfileManager>>,
    /// Notification configuration
    pub notification_config: Arc<Mutex<NotifySettings>>,
    /// Mouse stick control configuration
    pub mouse_stick: Arc<Mutex<MouseStickConfig>>,
    /// WebRTC video session manager
    pub webrtc_manager: Arc<Mutex<webrtc::WebRtcManager>>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── MouseStickConfig ───────────────────────────────────────────────

    #[test]
    fn test_mouse_stick_config_default() {
        let config = MouseStickConfig::default();
        assert!(!config.left_enabled);
        assert!(!config.right_enabled);
        assert_eq!(config.sensitivity, 1.0);
    }

    #[test]
    fn test_mouse_stick_config_serialize_roundtrip() {
        let config = MouseStickConfig {
            left_enabled: true,
            right_enabled: false,
            sensitivity: 2.5,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MouseStickConfig = serde_json::from_str(&json).unwrap();

        assert!(deserialized.left_enabled);
        assert!(!deserialized.right_enabled);
        assert_eq!(deserialized.sensitivity, 2.5);
    }

    #[test]
    fn test_mouse_stick_config_deserialize_partial() {
        // Ensure missing fields get defaults from the struct, not serde defaults
        let json = r#"{"left_enabled": true, "right_enabled": true}"#;
        let config: MouseStickConfig = serde_json::from_str(json).unwrap();
        assert!(config.left_enabled);
        assert!(config.right_enabled);
        // #[serde(default)] uses f32 default (0.0), not struct Default (1.0)
        assert_eq!(config.sensitivity, 0.0);
    }

    #[test]
    fn test_mouse_stick_config_clone() {
        let a = MouseStickConfig {
            left_enabled: true,
            right_enabled: true,
            sensitivity: 0.5,
        };
        let b = a.clone();
        assert_eq!(a.left_enabled, b.left_enabled);
        assert_eq!(a.right_enabled, b.right_enabled);
        assert_eq!(a.sensitivity, b.sensitivity);
    }

    #[test]
    fn test_mouse_stick_config_debug() {
        let config = MouseStickConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("left_enabled"));
        assert!(debug_str.contains("right_enabled"));
        assert!(debug_str.contains("sensitivity"));
    }
}
