use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ═══════════════════════════════════════════════════════════════════════════════
// Controller Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Serialize, Deserialize, ToSchema)]
pub struct ControllerTypeRequest {
    /// Gamepad type: "ProController" or "Xinput"
    pub gamepad_type: String,
}

/// POST /api/controller/type — set the gamepad type
#[utoipa::path(
    post,
    path = "/api/controller/type",
    tag = "controller",
    request_body = ControllerTypeRequest,
    responses(
        (status = 200, description = "Gamepad type set", body = serde_json::Value)
    )
)]
pub async fn controller_set_type(
    State(state): State<AppState>,
    Json(body): Json<ControllerTypeRequest>,
) -> Json<serde_json::Value> {
    match body.gamepad_type.as_str() {
        "ProController" | "Xinput" => {
            let mut gt = state.gamepad_type.lock().await;
            *gt = body.gamepad_type.clone();
            tracing::info!("Gamepad type set to: {}", body.gamepad_type);
            Json(serde_json::json!({
                "status": "ok",
                "gamepad_type": body.gamepad_type,
            }))
        }
        _ => Json(serde_json::json!({
            "status": "error",
            "message": format!("Invalid gamepad type: '{}'. Must be 'ProController' or 'Xinput'", body.gamepad_type),
        })),
    }
}

/// GET /api/controller/type — get the current gamepad type
#[utoipa::path(
    get,
    path = "/api/controller/type",
    tag = "controller",
    responses(
        (status = 200, description = "Current gamepad type", body = serde_json::Value)
    )
)]
pub async fn controller_get_type(State(state): State<AppState>) -> Json<serde_json::Value> {
    let gt = state.gamepad_type.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "gamepad_type": gt.clone(),
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct KeyboardRequest {
    /// Whether keyboard input is enabled
    pub enabled: bool,
}

/// POST /api/controller/keyboard — enable or disable keyboard input
#[utoipa::path(
    post,
    path = "/api/controller/keyboard",
    tag = "controller",
    request_body = KeyboardRequest,
    responses(
        (status = 200, description = "Keyboard input state set", body = serde_json::Value)
    )
)]
pub async fn controller_set_keyboard(
    State(state): State<AppState>,
    Json(body): Json<KeyboardRequest>,
) -> Json<serde_json::Value> {
    let mut ke = state.keyboard_enabled.lock().await;
    *ke = body.enabled;
    tracing::info!("Keyboard input enabled: {}", body.enabled);
    Json(serde_json::json!({
        "status": "ok",
        "keyboard_enabled": body.enabled,
    }))
}

/// GET /api/controller/keyboard — get keyboard enable state
#[utoipa::path(
    get,
    path = "/api/controller/keyboard",
    tag = "controller",
    responses(
        (status = 200, description = "Keyboard enable state", body = serde_json::Value)
    )
)]
pub async fn controller_get_keyboard(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ke = state.keyboard_enabled.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "keyboard_enabled": *ke,
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct MouseStickRequest {
    /// Stick identifier: "left" or "right"
    pub stick: String,
    /// Whether to enable mouse-to-stick control
    pub enabled: bool,
    /// Sensitivity multiplier (optional, default: 1.0)
    #[serde(default = "crate::helpers::default_sensitivity")]
    pub sensitivity: f32,
}

/// POST /api/controller/mouse_stick — enable/disable mouse-to-stick control
#[utoipa::path(
    post,
    path = "/api/controller/mouse_stick",
    tag = "controller",
    request_body = MouseStickRequest,
    responses(
        (status = 200, description = "Mouse stick config updated", body = serde_json::Value)
    )
)]
pub async fn controller_set_mouse_stick(
    State(state): State<AppState>,
    Json(body): Json<MouseStickRequest>,
) -> Json<serde_json::Value> {
    let mut ms = state.mouse_stick.lock().await;
    match body.stick.to_lowercase().as_str() {
        "left" => {
            ms.left_enabled = body.enabled;
            ms.sensitivity = body.sensitivity;
            tracing::info!(
                "Left stick mouse control: {} (sensitivity: {})",
                body.enabled,
                body.sensitivity
            );
            Json(serde_json::json!({
                "status": "ok",
                "stick": "left",
                "enabled": body.enabled,
                "sensitivity": body.sensitivity,
            }))
        }
        "right" => {
            ms.right_enabled = body.enabled;
            ms.sensitivity = body.sensitivity;
            tracing::info!(
                "Right stick mouse control: {} (sensitivity: {})",
                body.enabled,
                body.sensitivity
            );
            Json(serde_json::json!({
                "status": "ok",
                "stick": "right",
                "enabled": body.enabled,
                "sensitivity": body.sensitivity,
            }))
        }
        _ => Json(serde_json::json!({
            "status": "error",
            "message": format!("Invalid stick: '{}'. Must be 'left' or 'right'", body.stick),
        })),
    }
}

/// GET /api/controller/mouse_stick — get current mouse stick configuration
#[utoipa::path(
    get,
    path = "/api/controller/mouse_stick",
    tag = "controller",
    responses(
        (status = 200, description = "Current mouse stick config", body = serde_json::Value)
    )
)]
pub async fn controller_get_mouse_stick(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ms = state.mouse_stick.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "left_enabled": ms.left_enabled,
        "right_enabled": ms.right_enabled,
        "sensitivity": ms.sensitivity,
    }))
}
