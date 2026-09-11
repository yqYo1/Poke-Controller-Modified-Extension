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

fn deserialize_input_generation<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let generation = String::deserialize(deserializer)?;
    if generation.is_empty() || !generation.is_ascii() {
        return Err(D::Error::custom(
            "input generation must be a non-empty ASCII string",
        ));
    }
    Ok(generation)
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn deserialize_zero_sequence<'de, D>(deserializer: D) -> Result<DecimalString, D::Error>
where
    D: Deserializer<'de>,
{
    let sequence = DecimalString::deserialize(deserializer)?;
    if sequence != DecimalString::zero() {
        return Err(D::Error::custom(
            "initial input snapshot sequence must be zero",
        ));
    }
    Ok(sequence)
}

fn deserialize_pressed_touch<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let pressed = bool::deserialize(deserializer)?;
    if !pressed {
        return Err(D::Error::custom(
            "a present touch point must be pressed; use null to release",
        ));
    }
    Ok(pressed)
}

fn deserialize_touch_x<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let x = u16::deserialize(deserializer)?;
    if x > 319 {
        return Err(D::Error::custom("touch x must be between 0 and 319"));
    }
    Ok(x)
}

fn deserialize_touch_y<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let y = u16::deserialize(deserializer)?;
    if y > 239 {
        return Err(D::Error::custom("touch y must be between 0 and 239"));
    }
    Ok(y)
}

fn pressed_touch_schema() -> utoipa::openapi::schema::Object {
    utoipa::openapi::schema::ObjectBuilder::new()
        .schema_type(utoipa::openapi::schema::Type::Boolean)
        .enum_values(Some([true]))
        .build()
}

fn zero_sequence_schema() -> utoipa::openapi::schema::Object {
    utoipa::openapi::schema::ObjectBuilder::new()
        .schema_type(utoipa::openapi::schema::Type::String)
        .enum_values(Some(["0"]))
        .build()
}

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

/// Closed, stable machine-readable failure codes shared by every public HTTP
/// response. Endpoint response declarations constrain which subset is
/// applicable, while the runtime wire type cannot emit an unregistered code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    RequestForbidden,
    UnsupportedMediaType,
    PayloadTooLarge,
    MalformedJson,
    InvalidRequest,
    ResourceNotFound,
    MethodNotAllowed,
    RevisionConflict,
    CommandNotFound,
    CommandStateConflict,
    ProfileSwitchConflict,
    SerialConnectionFailed,
    CameraUnavailable,
    ScreenshotUnavailable,
    DestinationConflict,
    NotificationUnsupported,
    DynamicConfigUnavailable,
    UnsupportedPlatform,
    PersistenceFailed,
    BackendUnavailable,
    InternalError,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub fields: Option<BTreeMap<String, Vec<String>>>,
}

/// Placeholder runtime type whose `OpenAPI` schema is replaced from the
/// canonical setting registry by `crate::server::openapi`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(transparent)]
pub struct SettingsReadValues(pub BTreeMap<String, Value>);

/// Placeholder runtime type whose `OpenAPI` schema is replaced from the
/// canonical setting registry by `crate::server::openapi`.
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
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CommandControlRequest {
    Start(CommandStartFields),
    Stop {},
    Pause {},
    Resume {},
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandStartFields {
    pub command: CommandIdentity,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyRequest {}

impl<'de> Deserialize<'de> for CommandControlRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, tag = "action", rename_all = "snake_case")]
        enum WireRequest {
            Start(CommandStartFields),
            Stop(EmptyRequest),
            Pause(EmptyRequest),
            Resume(EmptyRequest),
        }

        Ok(match WireRequest::deserialize(deserializer)? {
            WireRequest::Start(fields) => Self::Start(fields),
            WireRequest::Stop(_) => Self::Stop {},
            WireRequest::Pause(_) => Self::Pause {},
            WireRequest::Resume(_) => Self::Resume {},
        })
    }
}

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
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub serial_port: Option<String>,
    pub serial_baud_rate: u32,
    pub serial_connected: bool,
    pub camera_opened: bool,
    pub camera_fps: f64,
    pub camera_resolution: String,
    pub camera_device: CameraSelector,
    pub is_running: bool,
    pub command_state: CommandState,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub current_command: Option<String>,
    pub command_candidates: Vec<CommandInfo>,
    pub tags: Vec<String>,
    pub active_profile: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub pending_profile: Option<String>,
    pub available_profiles: Vec<String>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
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
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SerialControlRequest {
    Connect {},
    Disconnect {},
}

impl<'de> Deserialize<'de> for SerialControlRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, tag = "action", rename_all = "snake_case")]
        enum WireRequest {
            Connect(EmptyRequest),
            Disconnect(EmptyRequest),
        }

        Ok(match WireRequest::deserialize(deserializer)? {
            WireRequest::Connect(_) => Self::Connect {},
            WireRequest::Disconnect(_) => Self::Disconnect {},
        })
    }
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "channel", rename_all = "lowercase")]
pub enum NotificationTestRequest {
    Windows {},
    Discord {},
}

impl<'de> Deserialize<'de> for NotificationTestRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, tag = "channel", rename_all = "lowercase")]
        enum WireRequest {
            Windows(EmptyRequest),
            Discord(EmptyRequest),
        }

        Ok(match WireRequest::deserialize(deserializer)? {
            WireRequest::Windows(_) => Self::Windows {},
            WireRequest::Discord(_) => Self::Discord {},
        })
    }
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DynamicConfigControlRequest {
    LoadPath(DynamicLoadPath),
    LoadContent(DynamicLoadContent),
    Reload {},
}

impl<'de> Deserialize<'de> for DynamicConfigControlRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, tag = "action", rename_all = "snake_case")]
        enum WireRequest {
            LoadPath(DynamicLoadPath),
            LoadContent(DynamicLoadContent),
            Reload(EmptyRequest),
        }

        Ok(match WireRequest::deserialize(deserializer)? {
            WireRequest::LoadPath(fields) => Self::LoadPath(fields),
            WireRequest::LoadContent(fields) => Self::LoadContent(fields),
            WireRequest::Reload(_) => Self::Reload {},
        })
    }
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
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
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

/// Mutation semantics for one output-panel message. Ordinary diagnostics
/// append, while the compatibility print APIs may replace or clear a panel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogOperation {
    Append,
    Replace,
    Clear,
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
    pub operation: LogOperation,
}

/// Script-dialog value kept type-safe across Python, Rust, and the browser.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum ScriptDialogValue {
    None,
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptDialogWidget {
    pub kind: ScriptDialogWidgetKind,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub label: Option<String>,
    pub value: ScriptDialogValue,
    pub options: Vec<ScriptDialogValue>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub minimum: Option<f64>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub maximum: Option<f64>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub precision: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptDialog {
    pub id: DecimalString,
    pub title: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub description: Option<String>,
    pub widgets: Vec<ScriptDialogWidget>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptTkScale {
    pub id: DecimalString,
    pub from_value: f64,
    pub to_value: f64,
    pub orient: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub label: Option<String>,
    pub value: f64,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub pady: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptTkButton {
    pub id: DecimalString,
    pub text: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub pady: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptTkLabel {
    pub id: DecimalString,
    pub text: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub width: Option<i64>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub height: Option<i64>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub relief: Option<String>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub background: Option<String>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub pady: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ScriptTkWidget {
    Scale(ScriptTkScale),
    Button(ScriptTkButton),
    Label(ScriptTkLabel),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptTkWindow {
    pub id: DecimalString,
    pub title: String,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub geometry: Option<String>,
    pub widgets: Vec<ScriptTkWidget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptOverlayRectangle {
    pub tag: String,
    pub x1: i64,
    pub y1: i64,
    pub x2: i64,
    pub y2: i64,
    pub outline: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptOverlayText {
    pub tag: String,
    pub x: i64,
    pub y: i64,
    pub text: String,
    pub font: String,
    pub font_size: u32,
    pub color: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ScriptOverlayShape {
    Rectangle(ScriptOverlayRectangle),
    Text(ScriptOverlayText),
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptPointerBindings {
    pub left: bool,
    pub right: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptOverlaySnapshot {
    pub fps: u32,
    pub show_width: u32,
    pub show_height: u32,
    pub right_mouse_mode: String,
    pub touchscreen_area: NormalizedRegion,
    pub bindings: ScriptPointerBindings,
    pub shapes: Vec<ScriptOverlayShape>,
}

impl Default for ScriptOverlaySnapshot {
    fn default() -> Self {
        Self {
            fps: 30,
            show_width: 1280,
            show_height: 720,
            right_mouse_mode: "Default".to_owned(),
            touchscreen_area: NormalizedRegion {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            bindings: ScriptPointerBindings::default(),
            shapes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptPopupImage {
    pub id: DecimalString,
    pub title: String,
    pub content_type: String,
    pub encoded_base64: String,
}

/// Latest complete profile-scoped script UI state. The broker replays this
/// snapshot to every newly connected client, so browser reconnects never need
/// to infer missed dialog or Tk mutations.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptUiSnapshot {
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub generation: Option<String>,
    pub dialogs: Vec<ScriptDialog>,
    pub tk_windows: Vec<ScriptTkWindow>,
    pub overlay: ScriptOverlaySnapshot,
    pub popup_images: Vec<ScriptPopupImage>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScriptDialogAbortReason {
    Close,
    Escape,
    Destroy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPointerButton {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPointerPhase {
    Pressed,
    Moved,
    Released,
}

/// Browser-originated, generation-checked interaction with script-owned UI.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "action")]
pub enum ScriptUiAction {
    DialogConfirm {
        generation: String,
        dialog_id: DecimalString,
        values: Vec<ScriptDialogValue>,
    },
    DialogAbort {
        generation: String,
        dialog_id: DecimalString,
        reason: ScriptDialogAbortReason,
    },
    TkScaleChanged {
        generation: String,
        widget_id: DecimalString,
        value: f64,
    },
    TkButtonInvoked {
        generation: String,
        widget_id: DecimalString,
    },
    TkWindowClosed {
        generation: String,
        window_id: DecimalString,
    },
    PopupClosed {
        generation: String,
        popup_id: DecimalString,
    },
    Pointer {
        generation: String,
        button: ScriptPointerButton,
        phase: ScriptPointerPhase,
        x: u32,
        y: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptUiActionResult {
    pub accepted: bool,
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
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub sdp_mid: Option<String>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
    pub sdp_mline_index: Option<u16>,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
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
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InputApplied {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
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
    #[schema(maximum = 255)]
    pub x: u8,
    #[schema(maximum = 255)]
    pub y: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TouchPoint {
    #[serde(deserialize_with = "deserialize_touch_x")]
    #[schema(maximum = 319)]
    pub x: u16,
    #[serde(deserialize_with = "deserialize_touch_y")]
    #[schema(maximum = 239)]
    pub y: u16,
    #[serde(deserialize_with = "deserialize_pressed_touch")]
    #[schema(schema_with = pressed_touch_schema)]
    pub pressed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InputSnapshot {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    #[serde(deserialize_with = "deserialize_zero_sequence")]
    #[schema(schema_with = zero_sequence_schema)]
    pub sequence: DecimalString,
    pub keyboard_keys: Vec<String>,
    pub mouse_buttons: MouseButtons,
    pub buttons: ButtonState,
    pub hat: Hat,
    pub left_stick: StickPosition,
    pub right_stick: StickPosition,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
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
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    pub sequence: DecimalString,
    pub key: String,
    pub state: PressState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MouseStickInput {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    pub sequence: DecimalString,
    pub stick: StickName,
    #[schema(maximum = 255)]
    pub x: u8,
    #[schema(maximum = 255)]
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
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum GamepadButton {
    A,
    B,
    X,
    Y,
    L,
    R,
    Zl,
    Zr,
    Lclick,
    Rclick,
    Minus,
    Plus,
    Home,
    Capture,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadButtonInput {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    pub sequence: DecimalString,
    pub button: GamepadButton,
    pub state: PressState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadStickInput {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    pub sequence: DecimalString,
    pub stick: StickName,
    #[schema(maximum = 255)]
    pub x: u8,
    #[schema(maximum = 255)]
    pub y: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
pub enum GamepadHat {
    #[serde(rename = "UP")]
    Up,
    #[serde(rename = "DOWN")]
    Down,
    #[serde(rename = "LEFT")]
    Left,
    #[serde(rename = "RIGHT")]
    Right,
    #[serde(rename = "TOP_RIGHT")]
    TopRight,
    #[serde(rename = "BTM_RIGHT")]
    BottomRight,
    #[serde(rename = "BTM_LEFT")]
    BottomLeft,
    #[serde(rename = "TOP_LEFT")]
    TopLeft,
    #[serde(rename = "CENTER")]
    Center,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadHatInput {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    pub sequence: DecimalString,
    pub hat: GamepadHat,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GamepadTouchInput {
    #[serde(deserialize_with = "deserialize_input_generation")]
    #[schema(min_length = 1, pattern = r"^[\x00-\x7F]+$")]
    pub generation: String,
    pub sequence: DecimalString,
    #[serde(deserialize_with = "deserialize_required_option")]
    #[schema(required = true)]
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
    #[serde(rename = "script.ui")]
    ScriptUi(MessageData<ScriptUiSnapshot>),
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
    use serde_json::{Value, json};

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

        for button in ["LCLICK", "RCLICK"] {
            let valid = json!({
                "type": "gamepad_input",
                "data": {
                    "kind": "button",
                    "generation": "g",
                    "sequence": "1",
                    "button": button,
                    "state": "pressed"
                }
            });
            assert!(serde_json::from_value::<ClientMessage>(valid).is_ok());
        }

        let invalid = json!({
            "type": "gamepad_input",
            "data": {
                "kind": "button",
                "generation": "g",
                "sequence": "1",
                "button": "STICKCLICK",
                "state": "pressed"
            }
        });
        assert!(serde_json::from_value::<ClientMessage>(invalid).is_err());
    }

    #[test]
    fn input_wire_constraints_are_enforced_before_dispatch() {
        let snapshot = |generation: &str, sequence: &str, touch: serde_json::Value| {
            json!({
                "type": "input.snapshot",
                "data": {
                    "generation": generation,
                    "sequence": sequence,
                    "keyboard_keys": [],
                    "mouse_buttons": {"left": false, "right": false, "middle": false},
                    "buttons": {
                        "a": false, "b": false, "x": false, "y": false,
                        "l": false, "r": false, "zl": false, "zr": false,
                        "lclick": false, "rclick": false, "plus": false, "minus": false,
                        "home": false, "capture": false
                    },
                    "hat": "neutral",
                    "left_stick": {"x": 128, "y": 128},
                    "right_stick": {"x": 128, "y": 128},
                    "touch": touch
                }
            })
        };

        assert!(
            serde_json::from_value::<ClientMessage>(snapshot("generation-1", "0", Value::Null))
                .is_ok()
        );
        assert!(serde_json::from_value::<ClientMessage>(snapshot("", "0", Value::Null)).is_err());
        assert!(
            serde_json::from_value::<ClientMessage>(snapshot("世代", "0", Value::Null)).is_err()
        );
        assert!(
            serde_json::from_value::<ClientMessage>(snapshot("generation-1", "1", Value::Null))
                .is_err()
        );
        let mut missing_touch = snapshot("generation-1", "0", Value::Null);
        missing_touch["data"]
            .as_object_mut()
            .expect("snapshot data")
            .remove("touch");
        assert!(serde_json::from_value::<ClientMessage>(missing_touch).is_err());
        assert!(
            serde_json::from_value::<ClientMessage>(snapshot(
                "generation-1",
                "0",
                json!({"x": 0, "y": 0, "pressed": false})
            ))
            .is_err()
        );
        for touch in [
            json!({"x": 320, "y": 0, "pressed": true}),
            json!({"x": 0, "y": 240, "pressed": true}),
        ] {
            assert!(
                serde_json::from_value::<ClientMessage>(snapshot("generation-1", "0", touch))
                    .is_err()
            );
        }
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
