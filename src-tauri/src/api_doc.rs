use utoipa::OpenApi;

// ═══════════════════════════════════════════════════════════════════════════════
// OpenAPI Documentation
// ═══════════════════════════════════════════════════════════════════════════════
//
// Note: paths(...) uses full module paths because the #[utoipa::path] macro
// generates __path_* items in each handler's own module.

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::handlers::core::api_status,
        crate::handlers::core::api_greet,
        crate::handlers::core::api_openapi_json,
        crate::handlers::controller::controller_set_type,
        crate::handlers::controller::controller_get_type,
        crate::handlers::controller::controller_set_keyboard,
        crate::handlers::controller::controller_get_keyboard,
        crate::handlers::controller::controller_set_mouse_stick,
        crate::handlers::controller::controller_get_mouse_stick,
        crate::handlers::camera::cameras_list,
        crate::handlers::camera::camera_status,
        crate::handlers::camera::camera_open,
        crate::handlers::camera::camera_close,
        crate::handlers::camera::camera_frame,
        crate::handlers::camera::camera_capture,
        crate::handlers::camera::camera_config,
        crate::handlers::input::input_press,
        crate::handlers::input::input_hold,
        crate::handlers::input::input_release,
        crate::handlers::input::input_stick,
        crate::handlers::input::input_touch,
        crate::handlers::websocket::ws_handler,
        crate::handlers::serial::serial_ports,
        crate::handlers::serial::serial_open,
        crate::handlers::serial::serial_close,
        crate::handlers::serial::serial_write,
        crate::handlers::serial::serial_config,
        crate::handlers::serial::serial_status,
        crate::handlers::commands::commands_list,
        crate::handlers::commands::commands_load,
        crate::handlers::commands::commands_start,
        crate::handlers::commands::commands_stop,
        crate::handlers::commands::commands_active,
        crate::handlers::commands::commands_filter,
        crate::handlers::commands::commands_reload,
        crate::handlers::profile::profile_list,
        crate::handlers::profile::profile_set,
        crate::handlers::notifications::notifications_get_config,
        crate::handlers::notifications::notifications_set_config,
        crate::handlers::notifications::notifications_send,
    ),
    components(
        schemas(
            crate::handlers::controller::ControllerTypeRequest,
            crate::handlers::controller::KeyboardRequest,
            crate::handlers::controller::MouseStickRequest,
            crate::handlers::camera::CameraOpenRequest,
            crate::handlers::camera::CaptureRequest,
            crate::handlers::camera::CameraConfigRequest,
            crate::handlers::input::PressRequest,
            crate::handlers::input::HoldRequest,
            crate::handlers::input::StickRequest,
            crate::handlers::input::TouchRequest,
            crate::handlers::serial::OpenRequest,
            crate::handlers::serial::WriteRequest,
            crate::handlers::serial::SerialConfigRequest,
            crate::handlers::NameRequest,
            crate::handlers::commands::FilterRequest,
            crate::handlers::notifications::NotificationConfigRequest,
            crate::handlers::notifications::SendNotificationRequest,
            crate::state::MouseStickConfig,
            crate::state::NotificationConfig,
        )
    ),
    tags(
        (name = "core", description = "Core API endpoints"),
        (name = "controller", description = "Controller configuration endpoints"),
        (name = "camera", description = "Camera management endpoints"),
        (name = "input", description = "Game input endpoints"),
        (name = "websocket", description = "WebSocket real-time event endpoint"),
        (name = "serial", description = "Serial port management endpoints"),
        (name = "commands", description = "Command management endpoints"),
        (name = "profile", description = "Profile management endpoints"),
        (name = "notifications", description = "Notification configuration and sending endpoints"),
    )
)]
pub struct ApiDoc;
