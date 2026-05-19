use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use tokio::time::Duration;
use utoipa::ToSchema;

use pokecon_core::serial::SendFormat;
use pokecon_core::serial::keys::{Direction, GamepadInput, Stick, Touchscreen};

use crate::helpers;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Key Input Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PressRequest {
    pub buttons: Vec<String>,
    /// Duration in milliseconds to hold before releasing (default: 50)
    #[serde(default = "crate::helpers::default_press_duration")]
    pub duration: u64,
    /// Wait in milliseconds after releasing (default: 0)
    #[serde(default)]
    pub wait: u64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct HoldRequest {
    pub buttons: Vec<String>,
    /// Duration in milliseconds to hold (default: 0 = indefinite via keypress hold state)
    #[serde(default)]
    pub duration: u64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct StickRequest {
    /// Stick identifier: "left" or "right"
    pub stick: String,
    /// X-axis value (0–255, 128 = center)
    pub x: u8,
    /// Y-axis value (0–255, 128 = center)
    pub y: u8,
    /// Duration in milliseconds before centering (default: 100)
    #[serde(default = "crate::helpers::default_stick_duration")]
    pub duration: u64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct TouchRequest {
    /// X coordinate (0–?)
    pub x: u16,
    /// Y coordinate (0–255)
    pub y: u8,
    /// Duration in milliseconds before releasing (default: 100)
    #[serde(default = "crate::helpers::default_touch_duration")]
    pub duration: u64,
}

/// POST /api/input/press — press buttons for a duration, then release
#[utoipa::path(
    post,
    path = "/api/input/press",
    tag = "input",
    request_body = PressRequest,
    responses(
        (status = 200, description = "Buttons pressed", body = serde_json::Value)
    )
)]
pub async fn input_press(
    State(state): State<AppState>,
    Json(body): Json<PressRequest>,
) -> Json<serde_json::Value> {
    let buttons = helpers::parse_buttons(&body.buttons);
    if buttons.is_empty() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "No valid buttons specified. Valid names: A, B, X, Y, L, R, ZL, ZR, MINUS, PLUS, LCLICK, RCLICK, HOME, CAPTURE"
        }));
    }

    // Build the press row and release row first, while holding the lock
    let release_row = {
        let mut sender = state.serial.lock().await;
        if !sender.is_opened() {
            return Json(serde_json::json!({
                "status": "error",
                "message": "Serial port not open"
            }));
        }

        let mut fmt = SendFormat::new();
        fmt.set_button(&buttons);

        let press_row = helpers::format_default_row(&fmt, false, false);
        if let Err(e) = sender.write_row(&press_row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }

        // Build release row while we still have the lock
        let release_fmt = SendFormat::new();
        helpers::format_default_row(&release_fmt, false, false)
    }; // Lock is dropped here

    // Release after duration (lock released, safe to sleep)
    if body.duration > 0 {
        tokio::time::sleep(Duration::from_millis(body.duration)).await;
    }

    // Re-acquire lock to send release
    {
        let mut sender = state.serial.lock().await;
        if let Err(e) = sender.write_row(&release_row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }
    }

    if body.wait > 0 {
        tokio::time::sleep(Duration::from_millis(body.wait)).await;
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": format!("Pressed {buttons:?} for {}ms", body.duration),
    }))
}

/// POST /api/input/hold — hold buttons (toggle on)
#[utoipa::path(
    post,
    path = "/api/input/hold",
    tag = "input",
    request_body = HoldRequest,
    responses(
        (status = 200, description = "Buttons held", body = serde_json::Value)
    )
)]
pub async fn input_hold(
    State(state): State<AppState>,
    Json(body): Json<HoldRequest>,
) -> Json<serde_json::Value> {
    let buttons = helpers::parse_buttons(&body.buttons);
    if buttons.is_empty() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "No valid buttons specified"
        }));
    }

    let mut kp = state.keypress.lock().await;
    let inputs: Vec<GamepadInput> = buttons
        .iter()
        .map(|&b| GamepadInput::SingleButton(b))
        .collect();

    match kp.hold(&inputs).await {
        Ok(()) => {
            if body.duration > 0 {
                tokio::time::sleep(Duration::from_millis(body.duration)).await;
            }
            Json(serde_json::json!({
                "status": "ok",
                "message": format!("Holding {buttons:?}"),
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

/// POST /api/input/release — release all held buttons
#[utoipa::path(
    post,
    path = "/api/input/release",
    tag = "input",
    responses(
        (status = 200, description = "All buttons released", body = serde_json::Value)
    )
)]
pub async fn input_release(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut kp = state.keypress.lock().await;
    match kp.neutral().await {
        Ok(()) => Json(serde_json::json!({
            "status": "ok",
            "message": "All buttons released",
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

/// POST /api/input/stick — move a stick and recenter after duration
#[utoipa::path(
    post,
    path = "/api/input/stick",
    tag = "input",
    request_body = StickRequest,
    responses(
        (status = 200, description = "Stick moved", body = serde_json::Value)
    )
)]
pub async fn input_stick(
    State(state): State<AppState>,
    Json(body): Json<StickRequest>,
) -> Json<serde_json::Value> {
    let stick = match body.stick.to_lowercase().as_str() {
        "left" => Stick::Left,
        "right" => Stick::Right,
        _ => {
            return Json(serde_json::json!({
                "status": "error",
                "message": "Invalid stick: must be 'left' or 'right'"
            }));
        }
    };

    let direction = Direction::from_xy(stick, body.x, body.y);

    // Build the movement row and center row while holding the lock
    let center_row = {
        let mut sender = state.serial.lock().await;
        if !sender.is_opened() {
            return Json(serde_json::json!({
                "status": "error",
                "message": "Serial port not open"
            }));
        }

        let mut fmt = SendFormat::new();
        fmt.set_any_direction(std::slice::from_ref(&direction));

        let l_changed = matches!(stick, Stick::Left);
        let r_changed = matches!(stick, Stick::Right);
        let row = helpers::format_default_row(&fmt, l_changed, r_changed);

        if let Err(e) = sender.write_row(&row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }

        // Build center row while we still have the lock
        let center_fmt = SendFormat::new();
        helpers::format_default_row(&center_fmt, l_changed, r_changed)
    }; // Lock is dropped here

    // Wait for duration (lock released, safe to sleep)
    if body.duration > 0 {
        tokio::time::sleep(Duration::from_millis(body.duration)).await;
    }

    // Re-acquire lock to recenter
    {
        let mut sender = state.serial.lock().await;
        if let Err(e) = sender.write_row(&center_row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": format!("{stick:?} stick moved to ({}, {}) for {}ms", body.x, body.y, body.duration),
    }))
}

/// POST /api/input/touch — tap the touchscreen
#[utoipa::path(
    post,
    path = "/api/input/touch",
    tag = "input",
    request_body = TouchRequest,
    responses(
        (status = 200, description = "Touchscreen tapped", body = serde_json::Value)
    )
)]
pub async fn input_touch(
    State(state): State<AppState>,
    Json(body): Json<TouchRequest>,
) -> Json<serde_json::Value> {
    let touch = Touchscreen::new(body.x, body.y);

    // Build the touch data and release data while holding the lock
    let release_data = {
        let mut sender = state.serial.lock().await;
        if !sender.is_opened() {
            return Json(serde_json::json!({
                "status": "error",
                "message": "Serial port not open"
            }));
        }

        // For touchscreen in Default format, we need to use a format that supports it.
        let mut fmt = SendFormat::new();
        fmt.set_touchscreen(&[touch]);

        // Use a simple row approach: write the Qingpi format bytes
        let qingpi_data = fmt.convert_to_qingpi();
        if let Err(e) = sender.write_list(&qingpi_data, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }

        // Build release data while we still have the lock
        let release_fmt = SendFormat::new();
        release_fmt.convert_to_qingpi()
    }; // Lock is dropped here

    if body.duration > 0 {
        tokio::time::sleep(Duration::from_millis(body.duration)).await;
    }

    // Re-acquire lock to release touch
    {
        let mut sender = state.serial.lock().await;
        if let Err(e) = sender.write_list(&release_data, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": format!("Touched ({}, {}) for {}ms", body.x, body.y, body.duration),
    }))
}
