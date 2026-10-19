//! Application-service boundary for the public REST transport.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::server::api::{
    ApiError, ApiErrorCode, CameraDevice, CommandControlRequest, CommandHistoryControlRequest,
    CommandHistorySnapshot, CommandTagRequest, CommandTagResult, CommandTagSnapshot,
    DynamicConfigControlRequest, DynamicConfigResult, GenerateLauncherRequest,
    GenerateLauncherResult, ImageFormat, NotificationTestRequest, NotificationTestResult,
    OperationResult, RunListSnapshot, RunListSpec, RunScheduleControlRequest, RunScheduleSnapshot,
    SavedScreenshot, ScreenshotRequest, ScriptCreatorPreview, ScriptCreatorResult,
    ScriptCreatorSnapshot, ScriptCreatorSpec, ScriptUiAction, ScriptUiActionResult,
    SerialControlRequest, SerialPort, SettingsPatchRequest, SettingsSnapshot, UpdateCheckResult,
};
use crate::server::state::StateHub;

/// HTTP status classes that application services may deliberately expose.
/// Transport-only failures such as malformed JSON are created by `rest`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiFailureStatus {
    NotFound,
    Conflict,
    UnprocessableEntity,
    InternalServerError,
}

/// A status-classified and already-redacted application failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiFailure {
    status: ApiFailureStatus,
    error: ApiError,
}

impl ApiFailure {
    #[must_use]
    pub fn new(status: ApiFailureStatus, code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            error: ApiError {
                code,
                message: message.into(),
                fields: None,
            },
        }
    }

    #[must_use]
    pub fn with_fields(mut self, fields: BTreeMap<String, Vec<String>>) -> Self {
        self.error.fields = Some(fields);
        self
    }

    #[must_use]
    pub const fn status(&self) -> ApiFailureStatus {
        self.status
    }

    #[must_use]
    pub const fn error(&self) -> &ApiError {
        &self.error
    }

    #[must_use]
    pub fn into_error(self) -> ApiError {
        self.error
    }
}

pub type ApiResult<T> = Result<T, ApiFailure>;

/// Closed media types permitted for attachment responses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadMediaType {
    Png,
    Jpeg,
    WindowsBatch,
}

impl DownloadMediaType {
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::WindowsBatch => "application/x-bat",
        }
    }
}

impl From<ImageFormat> for DownloadMediaType {
    fn from(value: ImageFormat) -> Self {
        match value {
            ImageFormat::Png => Self::Png,
            ImageFormat::Jpeg => Self::Jpeg,
        }
    }
}

/// Bytes and a logical filename for a safe attachment response. The REST
/// transport performs the final ASCII fallback and RFC 5987 encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadPayload {
    filename: String,
    media_type: DownloadMediaType,
    bytes: Vec<u8>,
}

impl DownloadPayload {
    #[must_use]
    pub fn new(filename: impl Into<String>, media_type: DownloadMediaType, bytes: Vec<u8>) -> Self {
        Self {
            filename: filename.into(),
            media_type,
            bytes,
        }
    }

    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    #[must_use]
    pub const fn media_type(&self) -> DownloadMediaType {
        self.media_type
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScreenshotOutput {
    Saved(SavedScreenshot),
    Download(DownloadPayload),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LauncherOutput {
    Generated(GenerateLauncherResult),
    Download(DownloadPayload),
}

/// Concrete application services implement this trait; handlers remain a
/// closed HTTP adapter and never own domain transaction logic.
#[async_trait]
pub trait RestBackend: Send + Sync + 'static {
    /// Returns the one process-wide UI-visible state and revision gate.
    fn state_hub(&self) -> &StateHub;

    async fn patch_settings(&self, request: SettingsPatchRequest) -> ApiResult<SettingsSnapshot>;

    async fn control_command(&self, request: CommandControlRequest) -> ApiResult<OperationResult>;

    async fn reload_commands(&self) -> ApiResult<OperationResult>;

    async fn enumerate_cameras(&self) -> ApiResult<Vec<CameraDevice>>;

    async fn enumerate_serial_ports(&self) -> ApiResult<Vec<SerialPort>>;

    async fn control_serial(&self, request: SerialControlRequest) -> ApiResult<OperationResult>;

    async fn retry_camera(&self) -> ApiResult<OperationResult>;

    async fn screenshot(&self, request: ScreenshotRequest) -> ApiResult<ScreenshotOutput>;

    async fn test_notification(
        &self,
        request: NotificationTestRequest,
    ) -> ApiResult<NotificationTestResult>;

    /// Saved run lists plus skip reasons for the editor.
    async fn run_lists(&self) -> ApiResult<RunListSnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "run lists are not available",
        ))
    }

    /// Saves the run lists after validation (upstream `save_lists`).
    async fn save_run_lists(&self, _lists: Vec<RunListSpec>) -> ApiResult<RunListSnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "run lists are not available",
        ))
    }

    /// Current start and stop reservations (upstream `run_schedule`).
    async fn run_schedule(&self) -> ApiResult<RunScheduleSnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "run schedule is not available",
        ))
    }

    /// Applies one schedule control and returns the snapshot.
    async fn control_run_schedule(
        &self,
        _request: RunScheduleControlRequest,
    ) -> ApiResult<RunScheduleSnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "run schedule is not available",
        ))
    }

    /// The retained run history, newest first.
    async fn command_history(&self) -> ApiResult<CommandHistorySnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "command history is not available",
        ))
    }

    /// Removes one history entry or clears the history.
    async fn control_command_history(
        &self,
        _request: CommandHistoryControlRequest,
    ) -> ApiResult<CommandHistorySnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "command history is not available",
        ))
    }

    /// Every command file with its editable manual tags and digest.
    async fn command_tags(&self) -> ApiResult<CommandTagSnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "command tag editing is not available",
        ))
    }

    /// Applies one tag edit and returns the refreshed snapshot.
    async fn apply_command_tags(&self, _request: CommandTagRequest) -> ApiResult<CommandTagResult> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "command tag editing is not available",
        ))
    }

    /// Folder choices and remembered author for the script-creator dialog.
    async fn script_creator_snapshot(&self) -> ApiResult<ScriptCreatorSnapshot> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "script creator is not available",
        ))
    }

    /// Preview lines for the entered fields (upstream `preview_lines`).
    async fn preview_script_creation(
        &self,
        _spec: ScriptCreatorSpec,
    ) -> ApiResult<ScriptCreatorPreview> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "script creator is not available",
        ))
    }

    /// Creates the script template (upstream `create_script`).
    async fn create_script(&self, _spec: ScriptCreatorSpec) -> ApiResult<ScriptCreatorResult> {
        Err(ApiFailure::new(
            ApiFailureStatus::NotFound,
            ApiErrorCode::ResourceNotFound,
            "script creator is not available",
        ))
    }

    async fn script_ui_action(&self, _request: ScriptUiAction) -> ApiResult<ScriptUiActionResult> {
        Err(ApiFailure::new(
            ApiFailureStatus::Conflict,
            ApiErrorCode::BackendUnavailable,
            "script UI is unavailable",
        ))
    }

    async fn control_dynamic_config(
        &self,
        request: DynamicConfigControlRequest,
    ) -> ApiResult<DynamicConfigResult>;

    async fn generate_launcher(
        &self,
        request: GenerateLauncherRequest,
    ) -> ApiResult<LauncherOutput>;

    async fn check_update(&self) -> ApiResult<UpdateCheckResult>;
}
