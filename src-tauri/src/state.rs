use std::sync::Arc;

use pokecon_core::command_manager::CommandManager;
use pokecon_core::cv::camera::Camera;
use pokecon_core::events::EventBus;
use pokecon_core::profile::ProfileManager;
use pokecon_core::serial::keypress::KeyPress;
use pokecon_core::serial::sender::Sender;
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

/// Notification configuration stored in shared state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct NotificationConfig {
    /// Enable Windows desktop toast notifications
    pub windows_enabled: bool,
    /// Enable Discord webhook notifications
    pub discord_enabled: bool,
    /// Discord webhook URL
    #[serde(default)]
    pub discord_webhook_url: String,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            windows_enabled: true,
            discord_enabled: false,
            discord_webhook_url: String::new(),
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
    pub notification_config: Arc<Mutex<NotificationConfig>>,
    /// Mouse stick control configuration
    pub mouse_stick: Arc<Mutex<MouseStickConfig>>,
    /// WebRTC video session manager
    pub webrtc_manager: Arc<Mutex<webrtc::WebRtcManager>>,
}
