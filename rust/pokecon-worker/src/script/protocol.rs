//! Closed user-script worker and reverse-host payloads.

use std::path::PathBuf;

use pokecon_camera::{
    CameraSelector, CaptureResolution, FlipMode, MappingDescriptor, ScreenshotFormat,
};
use serde::{Deserialize, Serialize};

pub const INITIALIZE: &str = "script.initialize";
pub const STATUS: &str = "script.status";
pub const EXECUTE: &str = "script.execute";
pub const STOP: &str = "script.stop";

/// Private worker environment bridge for the exact synchronized script venv.
pub const PYTHON_SITE_PACKAGES_ENV: &str = "POKECON_INTERNAL_SCRIPT_SITE_PACKAGES";

pub const HOST_CONTROLLER_INPUT: &str = "script.host.controller_input";
pub const HOST_CONTROLLER_NEUTRAL: &str = "script.host.controller_neutral";
pub const HOST_SERIAL_WRITE: &str = "script.host.serial_write";
pub const HOST_SERIAL_WRITE_ROW: &str = "script.host.serial_write_row";
pub const HOST_SERIAL_RELOAD: &str = "script.host.serial_reload";
pub const HOST_OUTPUT: &str = "script.host.output";
pub const HOST_DIALOG_OPEN: &str = "script.host.dialog_open";
pub const HOST_DIALOG_STATUS: &str = "script.host.dialog_status";
pub const HOST_DIALOG_CLOSE_ALL: &str = "script.host.dialog_close_all";
pub const HOST_NETWORK: &str = "script.host.network";
pub const HOST_NOTIFICATION: &str = "script.host.notification";
pub const HOST_CAMERA_INITIALIZE: &str = "script.host.camera_initialize";
pub const HOST_CAMERA_CONTROL: &str = "script.host.camera_control";
pub const HOST_OVERLAY: &str = "script.host.overlay";
pub const HOST_POPUP_IMAGE: &str = "script.host.popup_image";
pub const HOST_TK: &str = "script.host.tk";
pub const TK_EVENT: &str = "script.tk.event";

/// Leaves room for the `MessagePack` envelope and typed request fields below
/// the protocol-wide one-MiB frame limit.
pub const MAX_POPUP_IMAGE_BYTES: usize = 262_144;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptInitializeRequest {
    pub profile: String,
    pub command_root: PathBuf,
    pub data_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptInitializeResult {
    pub status: ScriptWorkerStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptWorkerStatus {
    pub initialized: bool,
    pub profile: Option<String>,
    pub running: bool,
    pub execution_id: Option<u64>,
}

impl ScriptWorkerStatus {
    #[must_use]
    pub const fn uninitialized() -> Self {
        Self {
            initialized: false,
            profile: None,
            running: false,
            execution_id: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptExecuteRequest {
    pub path: PathBuf,
    pub class_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ScriptExecutionOutcome {
    Completed,
    Finished,
    Stopped,
    Failed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptExecutionResult {
    pub execution_id: u64,
    pub outcome: ScriptExecutionOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptStopResult {
    pub stop_requested: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptInputAction {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ScriptButton {
    Y,
    B,
    A,
    X,
    L,
    R,
    Zl,
    Zr,
    Minus,
    Plus,
    Lclick,
    Rclick,
    Home,
    Capture,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptHat {
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
    Center,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStick {
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum ScriptControl {
    Button { button: ScriptButton },
    Hat { direction: ScriptHat },
    Stick { stick: ScriptStick, x: u8, y: u8 },
    Touchscreen { x: u16, y: u16 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostControllerInputRequest {
    pub action: ScriptInputAction,
    pub controls: Vec<ScriptControl>,
    pub unset_hat: bool,
    pub unset_touchscreen: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSerialWriteRequest {
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSerialWriteRowRequest {
    pub row: String,
}

/// Rust-main camera state observed by the compatibility proxy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostCameraState {
    pub opened: bool,
    pub fps: u32,
    pub capture_resolution: CaptureResolution,
    pub flip_mode: FlipMode,
    pub screenshot_format: ScreenshotFormat,
}

/// One-time persistent shared-frame mapping plus its initial state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostCameraInitializeResult {
    pub mapping: Option<MappingDescriptor>,
    pub state: HostCameraState,
}

/// Closed camera lifecycle and runtime-setting operations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "operation")]
pub enum HostCameraControlRequest {
    State,
    SetFps { fps: u32 },
    SetFlip { mode: FlipMode },
    Open { selector: CameraSelector },
    Destroy,
    ThreadStart,
    ThreadStop,
    Update,
}

/// UI-independent camera-overlay and capture-area control operations.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "operation")]
pub enum HostOverlayRequest {
    Rectangle {
        x1: i64,
        y1: i64,
        x2: i64,
        y2: i64,
        outline: String,
        tag: String,
        expires_ms: Option<u64>,
    },
    Text {
        x: i64,
        y: i64,
        text: String,
        tag: String,
        expires_ms: Option<u64>,
        font: String,
        font_size: u32,
        color: String,
    },
    DeleteRectangle {
        tag: String,
    },
    DeleteText {
        tag: String,
    },
    SetFps {
        fps: u32,
    },
    SetShowSize {
        height: u32,
        width: u32,
    },
    SetRightMouseMode {
        mode: String,
    },
    SetTouchscreenArea {
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
    },
    SetBinding {
        button: ScriptPointerButton,
        enabled: bool,
    },
    Update,
    Cleanup,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPointerButton {
    Left,
    Right,
}

/// Compressed worker-local image prepared for a UI popup.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostPopupImageRequest {
    pub title: String,
    pub content_type: String,
    pub encoded: Vec<u8>,
}

impl std::fmt::Debug for HostPopupImageRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostPopupImageRequest")
            .field("title", &self.title)
            .field("content_type", &self.content_type)
            .field("encoded_len", &self.encoded.len())
            .finish()
    }
}

/// Fixed, deliberately small Tkinter-to-Web compatibility operation set.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "operation")]
pub enum HostTkRequest {
    CreateToplevel {
        window_id: u64,
    },
    SetTitle {
        window_id: u64,
        title: String,
    },
    SetGeometry {
        window_id: u64,
        geometry: String,
    },
    CreateScale {
        window_id: u64,
        widget_id: u64,
        from_value: f64,
        to_value: f64,
        orient: String,
        label: Option<String>,
    },
    CreateButton {
        window_id: u64,
        widget_id: u64,
        text: String,
    },
    CreateLabel {
        window_id: u64,
        widget_id: u64,
        text: String,
        width: Option<i64>,
        height: Option<i64>,
        relief: Option<String>,
        background: Option<String>,
    },
    Pack {
        widget_id: u64,
        pady: Option<i64>,
    },
    SetScale {
        widget_id: u64,
        value: f64,
    },
    GetScale {
        widget_id: u64,
    },
    ConfigureLabel {
        widget_id: u64,
        text: Option<String>,
        background: Option<String>,
    },
    DestroyWindow {
        window_id: u64,
    },
    Cleanup,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostTkResult {
    pub value: Option<f64>,
}

/// UI-originated Tk bridge callback delivered to the worker callback queue.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "event")]
pub enum ScriptTkEvent {
    ScaleChanged { widget_id: u64, value: f64 },
    ButtonInvoked { widget_id: u64 },
    WindowClosed { window_id: u64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScriptOutputTarget {
    #[serde(rename = "stdout")]
    Stdout,
    #[serde(rename = "panel1")]
    Panel1,
    #[serde(rename = "panel2")]
    Panel2,
    #[serde(rename = "alternate")]
    Alternate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptOutputMode {
    Write,
    Append,
    Delete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostOutputRequest {
    pub target: ScriptOutputTarget,
    pub mode: Option<ScriptOutputMode>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum ScriptDialogValue {
    None,
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptDialogWidgetKind {
    Entry,
    Check,
    Combo,
    Radio,
    Spin,
    Scale,
    Next,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptDialogWidget {
    pub kind: ScriptDialogWidgetKind,
    pub label: Option<String>,
    pub value: ScriptDialogValue,
    pub options: Vec<ScriptDialogValue>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub precision: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogOpenRequest {
    pub title: String,
    pub description: Option<String>,
    pub widgets: Vec<ScriptDialogWidget>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogOpenResult {
    pub dialog_id: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogStatusRequest {
    pub dialog_id: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ScriptDialogState {
    Open,
    Confirmed { values: Vec<ScriptDialogValue> },
    Aborted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogStatusResult {
    pub state: ScriptDialogState,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "operation")]
pub enum HostNetworkRequest {
    Cleanup,
    SocketConnect,
    SocketDisconnect,
    SocketTransmit {
        message: String,
    },
    SocketReceive {
        headers: Vec<String>,
        show_message: bool,
    },
    SocketChangeAddress {
        address: String,
    },
    SocketChangePort {
        port: u16,
    },
    SocketChangeAlive {
        alive: bool,
    },
    MqttTransmit {
        room_id: String,
        message: String,
    },
    MqttReceive {
        room_id: String,
        headers: Vec<String>,
        show_message: bool,
    },
    MqttChangeBrokerAddress {
        broker_address: String,
    },
    MqttChangeId {
        mqtt_id: String,
    },
    MqttChangeClientId {
        client_id: String,
    },
    MqttChangePublishToken {
        token: String,
    },
    MqttChangeSubscribeToken {
        token: String,
    },
}

impl std::fmt::Debug for HostNetworkRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let operation = match self {
            Self::Cleanup => "cleanup",
            Self::SocketConnect => "socket_connect",
            Self::SocketDisconnect => "socket_disconnect",
            Self::SocketTransmit { .. } => "socket_transmit",
            Self::SocketReceive { .. } => "socket_receive",
            Self::SocketChangeAddress { .. } => "socket_change_address",
            Self::SocketChangePort { .. } => "socket_change_port",
            Self::SocketChangeAlive { .. } => "socket_change_alive",
            Self::MqttTransmit { .. } => "mqtt_transmit",
            Self::MqttReceive { .. } => "mqtt_receive",
            Self::MqttChangeBrokerAddress { .. } => "mqtt_change_broker_address",
            Self::MqttChangeId { .. } => "mqtt_change_id",
            Self::MqttChangeClientId { .. } => "mqtt_change_client_id",
            Self::MqttChangePublishToken { .. } => "mqtt_change_publish_token",
            Self::MqttChangeSubscribeToken { .. } => "mqtt_change_subscribe_token",
        };
        formatter
            .debug_struct("HostNetworkRequest")
            .field("operation", &operation)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostNetworkResult {
    pub message: Option<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum HostNotificationRequest {
    DiscordText {
        content: String,
        settings_key: String,
    },
    DiscordImage {
        content: String,
        settings_keys: Vec<String>,
        crop_format: String,
        crop: Option<Vec<i64>>,
    },
}

impl std::fmt::Debug for HostNotificationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::DiscordText { .. } => "discord_text",
            Self::DiscordImage { .. } => "discord_image",
        };
        formatter
            .debug_struct("HostNotificationRequest")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}
