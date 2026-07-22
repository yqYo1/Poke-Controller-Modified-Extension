//! Application-service boundary for the public REST transport.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::api::{
    ApiError, ApiErrorCode, CameraDevice, CommandControlRequest, DynamicConfigControlRequest,
    DynamicConfigResult, GenerateLauncherRequest, GenerateLauncherResult, ImageFormat,
    NotificationTestRequest, NotificationTestResult, OperationResult, SavedScreenshot,
    ScreenshotRequest, SerialControlRequest, SerialPort, SettingsPatchRequest, SettingsSnapshot,
    UpdateCheckResult,
};
use crate::state::StateHub;

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
