//! `OpenAPI` path declarations shared with the axum route modules.

use crate::server::api::{
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
    request_body(content = crate::server::api::NotificationTestRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Test notification delivered", body = Success<crate::server::api::NotificationTestResult>),
        (status = 409, description = "Unsupported notification channel", body = ErrorEnvelope),
        (status = 422, description = "Notification configuration is invalid", body = ErrorEnvelope),
        (status = 500, description = "Notification delivery failure", body = ErrorEnvelope)
    )
)]
pub fn test_notification() {}

#[utoipa::path(
    get,
    path = "/api/run-lists",
    tag = "run_lists",
    responses(
        (status = 200, description = "Saved run lists and skip reasons", body = Success<crate::server::api::RunListSnapshot>),
        (status = 500, description = "Run list read failure", body = ErrorEnvelope)
    )
)]
pub fn run_lists() {}

#[utoipa::path(
    put,
    path = "/api/run-lists",
    tag = "run_lists",
    request_body(content = crate::server::api::RunListSaveRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Run lists saved after validation", body = Success<crate::server::api::RunListSnapshot>),
        (status = 422, description = "A run list is invalid", body = ErrorEnvelope),
        (status = 500, description = "Run list save failure", body = ErrorEnvelope)
    )
)]
pub fn save_run_lists() {}

#[utoipa::path(
    get,
    path = "/api/run-schedule",
    tag = "run_schedule",
    responses(
        (status = 200, description = "Current start and stop reservations", body = Success<crate::server::api::RunScheduleSnapshot>),
        (status = 500, description = "Run schedule read failure", body = ErrorEnvelope)
    )
)]
pub fn run_schedule() {}

#[utoipa::path(
    post,
    path = "/api/run-schedule/control",
    tag = "run_schedule",
    request_body(content = crate::server::api::RunScheduleControlRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Reservation applied and the snapshot returned", body = Success<crate::server::api::RunScheduleSnapshot>),
        (status = 422, description = "Reservation input is invalid", body = ErrorEnvelope),
        (status = 500, description = "Run schedule control failure", body = ErrorEnvelope)
    )
)]
pub fn control_run_schedule() {}

#[utoipa::path(
    get,
    path = "/api/command-history",
    tag = "command_history",
    responses(
        (status = 200, description = "Retained run history, newest first", body = Success<crate::server::api::CommandHistorySnapshot>),
        (status = 500, description = "Command history read failure", body = ErrorEnvelope)
    )
)]
pub fn command_history() {}

#[utoipa::path(
    post,
    path = "/api/command-history/control",
    tag = "command_history",
    request_body(content = crate::server::api::CommandHistoryControlRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "History updated and the snapshot returned", body = Success<crate::server::api::CommandHistorySnapshot>),
        (status = 422, description = "History control input is invalid", body = ErrorEnvelope),
        (status = 500, description = "Command history update failure", body = ErrorEnvelope)
    )
)]
pub fn control_command_history() {}

#[utoipa::path(
    get,
    path = "/api/command-pause",
    tag = "command_pause",
    responses(
        (status = 200, description = "Pause location and variables while paused", body = Success<crate::server::api::CommandPauseSnapshot>),
        (status = 500, description = "Pause information read failure", body = ErrorEnvelope)
    )
)]
pub fn command_pause() {}

#[utoipa::path(
    get,
    path = "/api/input-record",
    tag = "input_record",
    responses(
        (status = 200, description = "Current controller input recording state", body = Success<crate::server::api::InputRecordSnapshot>),
        (status = 500, description = "Input recording read failure", body = ErrorEnvelope)
    )
)]
pub fn input_record() {}

#[utoipa::path(
    post,
    path = "/api/input-record/control",
    tag = "input_record",
    request_body(content = crate::server::api::InputRecordControlRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Recording control applied and the snapshot returned", body = Success<crate::server::api::InputRecordResult>),
        (status = 409, description = "Recording state conflict", body = ErrorEnvelope),
        (status = 422, description = "Recording input or name is invalid", body = ErrorEnvelope),
        (status = 500, description = "Recording write failure", body = ErrorEnvelope)
    )
)]
pub fn control_input_record() {}

#[utoipa::path(
    get,
    path = "/api/command-tags",
    tag = "command_tags",
    responses(
        (status = 200, description = "Every command file with its editable manual tags", body = Success<crate::server::api::CommandTagSnapshot>),
        (status = 500, description = "Command tag snapshot failure", body = ErrorEnvelope)
    )
)]
pub fn command_tags() {}

#[utoipa::path(
    post,
    path = "/api/command-tags/apply",
    tag = "command_tags",
    request_body(content = crate::server::api::CommandTagRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Tag edit applied and the refreshed snapshot returned", body = Success<crate::server::api::CommandTagResult>),
        (status = 422, description = "Tag input is invalid", body = ErrorEnvelope),
        (status = 500, description = "Tag edit failure", body = ErrorEnvelope)
    )
)]
pub fn apply_command_tags() {}

#[utoipa::path(
    get,
    path = "/api/script-creator",
    tag = "script_creator",
    responses(
        (status = 200, description = "Script creator folder choices and remembered author", body = Success<crate::server::api::ScriptCreatorSnapshot>),
        (status = 500, description = "Script creator snapshot failure", body = ErrorEnvelope)
    )
)]
pub fn script_creator_snapshot() {}

#[utoipa::path(
    post,
    path = "/api/script-creator/preview",
    tag = "script_creator",
    request_body(content = crate::server::api::ScriptCreatorSpec, content_type = "application/json"),
    responses(
        (status = 200, description = "Preview of the files the creation would write", body = Success<crate::server::api::ScriptCreatorPreview>),
        (status = 422, description = "Script fields are invalid", body = ErrorEnvelope),
        (status = 500, description = "Script preview failure", body = ErrorEnvelope)
    )
)]
pub fn preview_script_creation() {}

#[utoipa::path(
    post,
    path = "/api/script-creator/create",
    tag = "script_creator",
    request_body(content = crate::server::api::ScriptCreatorSpec, content_type = "application/json"),
    responses(
        (status = 200, description = "Script template created", body = Success<crate::server::api::ScriptCreatorResult>),
        (status = 422, description = "Script fields are invalid or the file already exists", body = ErrorEnvelope),
        (status = 500, description = "Script creation failure", body = ErrorEnvelope)
    )
)]
pub fn create_script() {}

#[utoipa::path(
    post,
    path = "/api/script-ui/action",
    tag = "script_ui",
    request_body(content = crate::server::api::ScriptUiAction, content_type = "application/json"),
    responses(
        (status = 200, description = "Script UI interaction accepted", body = Success<crate::server::api::ScriptUiActionResult>),
        (status = 409, description = "Stale generation or completed UI object", body = ErrorEnvelope),
        (status = 422, description = "Invalid dialog value or Tk interaction", body = ErrorEnvelope),
        (status = 500, description = "Script worker interaction failed", body = ErrorEnvelope)
    )
)]
pub fn script_ui_action() {}

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
