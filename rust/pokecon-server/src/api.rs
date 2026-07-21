//! Public HTTP and WebSocket wire contracts.
//!
//! These types deliberately contain no application services.  The axum
//! handlers and the generated TypeScript client both consume this module, so
//! `serde` and `OpenAPI` cannot silently drift apart.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// An exact non-negative integer wire value that never depends on JavaScript
/// `Number` precision.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, ToSchema)]
#[serde(transparent)]
#[schema(
    value_type = String,
    pattern = r"^(0|[1-9][0-9]*)$",
    example = "42"
)]
pub struct DecimalString(String);

impl DecimalString {
    #[must_use]
    pub fn zero() -> Self {
        Self("0".to_owned())
    }

    #[must_use]
    pub fn from_u64(value: u64) -> Self {
        Self(value.to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DecimalString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for DecimalString {
    type Err = DecimalStringError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = value == "0"
            || (!value.starts_with('0')
                && !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit()));
        valid
            .then(|| Self(value.to_owned()))
            .ok_or(DecimalStringError)
    }
}

impl<'de> Deserialize<'de> for DecimalString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("value must be a canonical non-negative decimal string")]
pub struct DecimalStringError;

/// Common successful JSON envelope.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Success<T> {
    pub data: T,
}

/// Common secret-safe error envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelope {
    pub error: ApiError,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub fields: Option<BTreeMap<String, Vec<String>>>,
}

/// Placeholder runtime type whose `OpenAPI` schema is replaced from the
/// canonical setting registry by `crate::openapi`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct SettingsReadValues(pub BTreeMap<String, Value>);

/// Placeholder runtime type whose `OpenAPI` schema is replaced from the
/// canonical setting registry by `crate::openapi`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct SettingsWriteValues(pub BTreeMap<String, Value>);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsPatchRequest {
    #[serde(default)]
    pub expected_revision: Option<DecimalString>,
    pub values: SettingsWriteValues,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsSnapshot {
    pub revision: DecimalString,
    pub values: SettingsReadValues,
    pub pending_restart_values: SettingsWriteValues,
    pub restart_required: Vec<String>,
    pub apply_failures: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandIdentity {
    pub module_path: String,
    pub class_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandInfo {
    pub name: String,
    pub module_path: String,
    pub class_name: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandDisplayItem {
    Command(CommandDisplayCommand),
    Separator(CommandDisplaySeparator),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandDisplayCommand {
    pub command: CommandInfo,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandDisplaySeparator {
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CommandControlRequest {
    Start(CommandStartFields),
    Stop(EmptyRequest),
    Pause(EmptyRequest),
    Resume(EmptyRequest),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandStartFields {
    pub command: CommandIdentity,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyRequest {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    Running,
    Paused,
    Stopped,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationResult {
    pub changed: bool,
    pub revision: DecimalString,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // The state wire contract has explicit booleans.
pub struct StateSnapshot {
    pub revision: DecimalString,
    pub serial_port: Option<String>,
    pub serial_baud_rate: u32,
    pub serial_connected: bool,
    pub camera_opened: bool,
    pub camera_fps: f64,
    pub camera_resolution: String,
    pub camera_device: CameraSelector,
    pub is_running: bool,
    pub command_state: CommandState,
    pub current_command: Option<String>,
    pub command_candidates: Vec<CommandInfo>,
    pub tags: Vec<String>,
    pub active_profile: String,
    pub pending_profile: Option<String>,
    pub available_profiles: Vec<String>,
    pub last_input: Option<String>,
    pub holding_buttons: Vec<String>,
    pub pid: u32,
    pub command_display_lists: BTreeMap<String, Vec<CommandDisplayItem>>,
    pub command_display_cache_loading: bool,
}

/// Sparse state payload emitted by exactly one visible transaction.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StatePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_port: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_baud_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_connected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_opened: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_device: Option<CameraSelector>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_running: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_state: Option<CommandState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_command: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_candidates: Option<Vec<CommandInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_profile: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_profiles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_input: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holding_buttons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_display_lists: Option<BTreeMap<String, Vec<CommandDisplayItem>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_display_cache_loading: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsChange {
    pub values: SettingsWriteValues,
    pub pending_restart_values: SettingsWriteValues,
    pub restart_required: Vec<String>,
    pub apply_failures: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StateChangeCause {
    Settings,
    Camera,
    Serial,
    Command,
    Profile,
    DynamicConfig,
    Commands,
    Shutdown,
    Other,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UiStateChange {
    pub cause: StateChangeCause,
    pub state: StatePatch,
    pub settings: Option<SettingsChange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CameraDevice {
    pub selector: CameraSelector,
    pub label: String,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(untagged)]
pub enum CameraSelector {
    Index(u32),
    Name(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SerialPort {
    pub selector: String,
    pub label: String,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SerialControlRequest {
    Connect(EmptyRequest),
    Disconnect(EmptyRequest),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizedRegion {
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub x: f64,
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub y: f64,
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub width: f64,
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(tag = "destination", rename_all = "lowercase")]
pub enum ScreenshotRequest {
    Captures(ScreenshotCaptures),
    Path(ScreenshotPath),
    Download(ScreenshotDownload),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotCaptures {
    #[serde(default)]
    pub region: Option<NormalizedRegion>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub format: Option<ImageFormat>,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotPath {
    #[serde(default)]
    pub region: Option<NormalizedRegion>,
    pub path: String,
    pub format: ImageFormat,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotDownload {
    #[serde(default)]
    pub region: Option<NormalizedRegion>,
    #[serde(default)]
    pub filename: Option<String>,
    pub format: ImageFormat,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedScreenshot {
    pub display_path: String,
    pub format: ImageFormat,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "channel", rename_all = "lowercase")]
pub enum NotificationTestRequest {
    Windows(EmptyRequest),
    Discord(EmptyRequest),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationTestResult {
    pub delivered: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DynamicLanguage {
    Python,
    Lua,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DynamicConfigControlRequest {
    LoadPath(DynamicLoadPath),
    LoadContent(DynamicLoadContent),
    Reload(EmptyRequest),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicLoadPath {
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicLoadContent {
    pub language: DynamicLanguage,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicConfigResult {
    pub display_path: String,
    pub language: DynamicLanguage,
    pub loaded: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LauncherDestination {
    Path(LauncherPath),
    Download(LauncherDownload),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LauncherPath {
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LauncherDownload {
    pub filename: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateLauncherRequest {
    pub profile: String,
    pub copy_current: bool,
    pub destination: LauncherDestination,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateLauncherResult {
    pub profile_created: bool,
    pub launcher_created: bool,
    pub revision: DecimalString,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogTarget {
    Stdout,
    Panel1,
    Panel2,
    Log,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SerialData {
    pub encoding: SerialEncoding,
    pub data: String,
    pub byte_length: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SerialEncoding {
    Base64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LogData {
    pub level: LogLevel,
    pub message: String,
    pub target: LogTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionDescription {
    pub sdp: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
    pub username_fragment: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Nonce {
    pub nonce: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InputGeneration {
    pub generation: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InputApplied {
    pub generation: String,
    pub sequence: DecimalString,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct ButtonState {
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub l: bool,
    pub r: bool,
    pub zl: bool,
    pub zr: bool,
    pub lclick: bool,
    pub rclick: bool,
    pub plus: bool,
    pub minus: bool,
    pub home: bool,
    pub capture: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Hat {
    Up,
    Down,
    Left,
    Right,
    UpRight,
    UpLeft,
    DownRight,
    DownLeft,
    Neutral,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StickPosition {
    pub x: u8,
    pub y: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TouchPoint {
    #[schema(maximum = 319)]
    pub x: u16,
    #[schema(maximum = 239)]
    pub y: u16,
    pub pressed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InputSnapshot {
    pub generation: String,
    pub sequence: DecimalString,
    pub keyboard_keys: Vec<String>,
    pub mouse_buttons: MouseButtons,
    pub buttons: ButtonState,
    pub hat: Hat,
    pub left_stick: StickPosition,
    pub right_stick: StickPosition,
    pub touch: Option<TouchPoint>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PressState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub enum StickName {
    #[serde(rename = "LSTICK")]
    LStick,
    #[serde(rename = "RSTICK")]
    RStick,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyboardInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub key: String,
    pub state: PressState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MouseStickInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub stick: StickName,
    pub x: u8,
    pub y: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MouseInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub button: MouseButton,
    pub state: PressState,
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GamepadInput {
    Button(GamepadButtonInput),
    Stick(GamepadStickInput),
    Hat(GamepadHatInput),
    Touch(GamepadTouchInput),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadButtonInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub button: String,
    pub state: PressState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadStickInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub stick: StickName,
    pub x: u8,
    pub y: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadHatInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub hat: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadTouchInput {
    pub generation: String,
    pub sequence: DecimalString,
    pub touch: Option<TouchPoint>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MessageData<T> {
    pub data: T,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionedStateChange {
    pub revision: DecimalString,
    pub data: UiStateChange,
}

/// Server-to-client WebSocket union. Every JSON variant is closed and only
/// state changes carry a revision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "ui.state.changed")]
    UiStateChanged(Box<RevisionedStateChange>),
    #[serde(rename = "serial.data")]
    SerialData(MessageData<SerialData>),
    #[serde(rename = "log")]
    Log(MessageData<LogData>),
    #[serde(rename = "webrtc.offer")]
    WebRtcOffer(MessageData<SessionDescription>),
    #[serde(rename = "webrtc.answer")]
    WebRtcAnswer(MessageData<SessionDescription>),
    #[serde(rename = "webrtc.ice_candidate")]
    WebRtcIceCandidate(MessageData<IceCandidate>),
    #[serde(rename = "input.generation")]
    InputGeneration(MessageData<InputGeneration>),
    #[serde(rename = "input.snapshot.applied")]
    InputSnapshotApplied(MessageData<InputApplied>),
    #[serde(rename = "ping")]
    Ping(MessageData<Nonce>),
}

/// Client-to-server WebSocket/DataChannel union.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "webrtc.offer")]
    WebRtcOffer(MessageData<SessionDescription>),
    #[serde(rename = "webrtc.answer")]
    WebRtcAnswer(MessageData<SessionDescription>),
    #[serde(rename = "webrtc.ice_candidate")]
    WebRtcIceCandidate(MessageData<IceCandidate>),
    #[serde(rename = "input.snapshot")]
    InputSnapshot(MessageData<InputSnapshot>),
    #[serde(rename = "keyboard_input")]
    KeyboardInput(MessageData<KeyboardInput>),
    #[serde(rename = "mouse_stick_input")]
    MouseStickInput(MessageData<MouseStickInput>),
    #[serde(rename = "mouse_input")]
    MouseInput(MessageData<MouseInput>),
    #[serde(rename = "gamepad_input")]
    GamepadInput(MessageData<GamepadInput>),
    #[serde(rename = "pong")]
    Pong(MessageData<Nonce>),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ClientMessage, CommandControlRequest, DecimalString, DynamicConfigControlRequest,
        GamepadInput, LauncherDestination, NotificationTestRequest, RevisionedStateChange,
        ScreenshotRequest, SerialControlRequest, ServerMessage, StateChangeCause, StatePatch,
        UiStateChange,
    };

    #[test]
    fn decimal_strings_are_canonical() {
        for valid in ["0", "1", "42", "18446744073709551616"] {
            assert!(valid.parse::<DecimalString>().is_ok(), "{valid}");
        }
        for invalid in ["", "00", "01", "+1", "-1", "1.0", " 1"] {
            assert!(invalid.parse::<DecimalString>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn command_union_rejects_cross_variant_fields() {
        let invalid = json!({"action": "stop", "command": {
            "module_path": "sample", "class_name": "Sample"
        }});
        assert!(serde_json::from_value::<CommandControlRequest>(invalid).is_err());
    }

    #[test]
    fn every_rest_union_rejects_cross_variant_fields() {
        let cases = [
            serde_json::from_value::<SerialControlRequest>(json!({
                "action": "disconnect",
                "path": "unexpected"
            }))
            .is_err(),
            serde_json::from_value::<ScreenshotRequest>(json!({
                "destination": "download",
                "filename": null,
                "format": "png",
                "path": "/unexpected"
            }))
            .is_err(),
            serde_json::from_value::<NotificationTestRequest>(json!({
                "channel": "windows",
                "content": "unexpected"
            }))
            .is_err(),
            serde_json::from_value::<DynamicConfigControlRequest>(json!({
                "action": "reload",
                "path": "unexpected.py"
            }))
            .is_err(),
            serde_json::from_value::<LauncherDestination>(json!({
                "kind": "download",
                "filename": null,
                "path": "unexpected.bat"
            }))
            .is_err(),
            serde_json::from_value::<GamepadInput>(json!({
                "kind": "hat",
                "generation": "g",
                "sequence": "1",
                "hat": "UP",
                "button": "A"
            }))
            .is_err(),
        ];
        assert!(cases.into_iter().all(|rejected| rejected));
    }

    #[test]
    fn websocket_unions_reject_unknown_fields() {
        let invalid = json!({"type": "pong", "data": {"nonce": "n", "extra": true}});
        assert!(serde_json::from_value::<ClientMessage>(invalid).is_err());

        let invalid = json!({"type": "ping", "data": {"nonce": "n"}, "revision": "1"});
        assert!(serde_json::from_value::<ServerMessage>(invalid).is_err());
    }

    #[test]
    fn state_change_revision_is_outside_data() {
        let message = ServerMessage::UiStateChanged(Box::new(RevisionedStateChange {
            revision: DecimalString::from_u64(7),
            data: UiStateChange {
                cause: StateChangeCause::Other,
                state: StatePatch::default(),
                settings: None,
            },
        }));
        let encoded = serde_json::to_value(message).unwrap();
        assert_eq!(encoded["type"], "ui.state.changed");
        assert_eq!(encoded["revision"], "7");
        assert_eq!(encoded["data"]["cause"], "other");
        assert!(encoded["data"].get("revision").is_none());
    }
}
