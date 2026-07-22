//! `OpenAPI` path declarations shared with the axum route modules.

use crate::api::{
    CameraDevice, CommandControlRequest, DynamicConfigControlRequest, DynamicConfigResult,
    EmptyRequest, ErrorEnvelope, GenerateLauncherRequest, GenerateLauncherResult, OperationResult,
    SavedScreenshot, ScreenshotRequest, SerialControlRequest, SerialPort, SettingsPatchRequest,
    SettingsSnapshot, StateSnapshot, Success, UpdateCheckResult,
};

#[utoipa::path(
    get,
    path = "/api/settings",
    tag = "settings",
    responses(
        (status = 200, description = "Current canonical settings", body = Success<SettingsSnapshot>),
        (status = 500, description = "Settings snapshot failed", body = ErrorEnvelope)
    )
)]
pub fn get_settings() {}

#[utoipa::path(
    patch,
    path = "/api/settings",
    tag = "settings",
    request_body(content = SettingsPatchRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Committed settings snapshot", body = Success<SettingsSnapshot>),
        (status = 400, description = "Malformed JSON", body = ErrorEnvelope),
        (status = 409, description = "Revision or runtime conflict", body = ErrorEnvelope),
        (status = 422, description = "Invalid setting transaction", body = ErrorEnvelope),
        (status = 500, description = "Persistence failure", body = ErrorEnvelope)
    )
)]
pub fn patch_settings() {}

#[utoipa::path(
    get,
    path = "/api/state",
    tag = "state",
    responses(
        (status = 200, description = "Current runtime state", body = Success<StateSnapshot>),
        (status = 500, description = "State snapshot failed", body = ErrorEnvelope)
    )
)]
pub fn get_state() {}

#[utoipa::path(
    post,
    path = "/api/commands/control",
    tag = "commands",
    request_body(content = CommandControlRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Command transition applied or a no-op", body = Success<OperationResult>),
        (status = 404, description = "Command identity was not found", body = ErrorEnvelope),
        (status = 409, description = "Command state conflict", body = ErrorEnvelope),
        (status = 422, description = "Invalid command request", body = ErrorEnvelope),
        (status = 500, description = "Command backend failure", body = ErrorEnvelope)
    )
)]
pub fn control_command() {}

#[utoipa::path(
    post,
    path = "/api/commands/reload",
    tag = "commands",
    request_body(content = EmptyRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Command generation reloaded", body = Success<OperationResult>),
        (status = 409, description = "Profile switch conflict", body = ErrorEnvelope),
        (status = 500, description = "Worker or cache failure", body = ErrorEnvelope)
    )
)]
pub fn reload_commands() {}

#[utoipa::path(
    get,
    path = "/api/devices/cameras",
    tag = "devices",
    responses(
        (status = 200, description = "Fresh camera enumeration", body = Success<Vec<CameraDevice>>),
        (status = 500, description = "Camera enumeration failure", body = ErrorEnvelope)
    )
)]
pub fn get_cameras() {}

#[utoipa::path(
    get,
    path = "/api/devices/serial-ports",
    tag = "devices",
    responses(
        (status = 200, description = "Fresh serial-port enumeration", body = Success<Vec<SerialPort>>),
        (status = 500, description = "Serial enumeration failure", body = ErrorEnvelope)
    )
)]
pub fn get_serial_ports() {}

#[utoipa::path(
    post,
    path = "/api/serial/control",
    tag = "devices",
    request_body(content = SerialControlRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Serial transition applied or a no-op", body = Success<OperationResult>),
        (status = 409, description = "Serial connection failure", body = ErrorEnvelope),
        (status = 422, description = "Invalid serial request", body = ErrorEnvelope),
        (status = 500, description = "Serial backend failure", body = ErrorEnvelope)
    )
)]
pub fn control_serial() {}

#[utoipa::path(
    post,
    path = "/api/camera/retry",
    tag = "devices",
    request_body(content = EmptyRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Camera retry applied or a no-op", body = Success<OperationResult>),
        (status = 409, description = "Camera retry failure", body = ErrorEnvelope),
        (status = 500, description = "Camera backend failure", body = ErrorEnvelope)
    )
)]
pub fn retry_camera() {}

#[utoipa::path(
    post,
    path = "/api/camera/screenshot",
    tag = "devices",
    request_body(content = ScreenshotRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Saved screenshot metadata or downloaded image bytes",
            content(
                (Success<SavedScreenshot> = "application/json"),
                ([u8] = "image/png"),
                ([u8] = "image/jpeg")
            ),
            headers(
                ("Content-Disposition" = String, description = "Present for attachment responses")
            )
        ),
        (status = 409, description = "No frame or incompatible UI mode", body = ErrorEnvelope),
        (status = 422, description = "Invalid crop, name, path, or format", body = ErrorEnvelope),
        (status = 500, description = "Screenshot failure", body = ErrorEnvelope)
    )
)]
pub fn screenshot() {}

#[utoipa::path(
    post,
    path = "/api/notifications/test",
    tag = "notifications",
    request_body(content = crate::api::NotificationTestRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Test notification delivered", body = Success<crate::api::NotificationTestResult>),
        (status = 409, description = "Unsupported notification channel", body = ErrorEnvelope),
        (status = 422, description = "Notification configuration is invalid", body = ErrorEnvelope),
        (status = 500, description = "Notification delivery failure", body = ErrorEnvelope)
    )
)]
pub fn test_notification() {}

#[utoipa::path(
    post,
    path = "/api/dynamic-config/control",
    tag = "dynamic_config",
    request_body(content = DynamicConfigControlRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Dynamic configuration load result", body = Success<DynamicConfigResult>),
        (status = 409, description = "No current dynamic file", body = ErrorEnvelope),
        (status = 422, description = "Invalid language, path, or content", body = ErrorEnvelope),
        (status = 500, description = "Dynamic configuration transport failure", body = ErrorEnvelope)
    )
)]
pub fn control_dynamic_config() {}

#[utoipa::path(
    post,
    path = "/api/profiles/generate-launcher",
    tag = "profiles",
    request_body(content = GenerateLauncherRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Profile and launcher result or downloaded Windows batch bytes",
            content(
                (Success<GenerateLauncherResult> = "application/json"),
                ([u8] = "application/x-bat")
            ),
            headers(
                ("Content-Disposition" = String, description = "Present for attachment responses")
            )
        ),
        (status = 409, description = "Unsupported platform or UI mode", body = ErrorEnvelope),
        (status = 422, description = "Invalid profile or destination", body = ErrorEnvelope),
        (status = 500, description = "Profile or launcher write failure", body = ErrorEnvelope)
    )
)]
pub fn generate_launcher() {}

#[utoipa::path(
    post,
    path = "/api/update/check",
    tag = "update",
    request_body(content = EmptyRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Version comparison", body = Success<UpdateCheckResult>),
        (status = 500, description = "Update service unavailable", body = ErrorEnvelope)
    )
)]
pub fn check_update() {}

#[utoipa::path(
    get,
    path = "/ws",
    tag = "websocket",
    responses(
        (status = 101, description = "WebSocket upgrade"),
        (status = 400, description = "Invalid WebSocket upgrade", body = ErrorEnvelope),
        (status = 403, description = "Host or Origin rejected", body = ErrorEnvelope),
        (status = 500, description = "Connection identifier exhausted", body = ErrorEnvelope)
    )
)]
pub fn websocket() {}
