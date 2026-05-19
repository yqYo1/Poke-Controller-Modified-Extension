use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use pokecon_core::serial::keypress::SerialFormat;

use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Serial Port Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// List available serial ports on the system.
#[utoipa::path(
    get,
    path = "/api/serial/ports",
    tag = "serial",
    responses(
        (status = 200, description = "List of available serial ports", body = serde_json::Value)
    )
)]
pub async fn serial_ports() -> Json<serde_json::Value> {
    match tokio_serial::available_ports() {
        Ok(ports) => {
            let port_list: Vec<serde_json::Value> = ports
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "port_name": p.port_name,
                        "port_type": format!("{:?}", p.port_type),
                    })
                })
                .collect();
            Json(serde_json::json!({ "ports": port_list }))
        }
        Err(e) => Json(serde_json::json!({
            "error": format!("Failed to list ports: {e}")
        })),
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct OpenRequest {
    pub port_num: u32,
    pub port_name: Option<String>,
    pub baudrate: u32,
}

/// Open a serial port connection.
#[utoipa::path(
    post,
    path = "/api/serial/open",
    tag = "serial",
    request_body = OpenRequest,
    responses(
        (status = 200, description = "Serial port opened", body = serde_json::Value)
    )
)]
pub async fn serial_open(
    State(state): State<AppState>,
    Json(body): Json<OpenRequest>,
) -> Json<serde_json::Value> {
    let mut sender = state.serial.lock().await;
    match sender
        .open(body.port_num, body.port_name.as_deref(), body.baudrate)
        .await
    {
        Ok(_) => Json(serde_json::json!({
            "status": "ok",
            "message": "Serial port opened"
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string()
        })),
    }
}

/// Close the serial port connection.
#[utoipa::path(
    post,
    path = "/api/serial/close",
    tag = "serial",
    responses(
        (status = 200, description = "Serial port closed", body = serde_json::Value)
    )
)]
pub async fn serial_close(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut sender = state.serial.lock().await;
    sender.close().await;
    Json(serde_json::json!({
        "status": "ok",
        "message": "Serial port closed"
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct WriteRequest {
    pub data: String,
}

/// Write data to the serial port.
#[utoipa::path(
    post,
    path = "/api/serial/write",
    tag = "serial",
    request_body = WriteRequest,
    responses(
        (status = 200, description = "Data written to serial port", body = serde_json::Value)
    )
)]
pub async fn serial_write(
    State(state): State<AppState>,
    Json(body): Json<WriteRequest>,
) -> Json<serde_json::Value> {
    let mut sender = state.serial.lock().await;
    match sender.write_row(&body.data, true).await {
        Ok(_) => Json(serde_json::json!({
            "status": "ok",
            "message": "Data written to serial port"
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string()
        })),
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SerialConfigRequest {
    pub baudrate: Option<u32>,
    pub data_format: Option<String>,
}

/// POST /api/serial/config — set baudrate and data format
#[utoipa::path(
    post,
    path = "/api/serial/config",
    tag = "serial",
    request_body = SerialConfigRequest,
    responses(
        (status = 200, description = "Serial config updated", body = serde_json::Value)
    )
)]
pub async fn serial_config(
    State(state): State<AppState>,
    Json(body): Json<SerialConfigRequest>,
) -> Json<serde_json::Value> {
    // Separate scopes to avoid holding multiple locks simultaneously
    // Lock ordering: serial → keypress (consistent across all endpoints)
    if let Some(baudrate) = body.baudrate {
        let mut sender = state.serial.lock().await;
        if sender.is_opened() {
            if let Err(e) = sender.set_baudrate(baudrate).await {
                return Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to set baudrate: {e}")
                }));
            }
        }
    } // serial lock released

    if let Some(ref format_str) = body.data_format {
        let format = match format_str.as_str() {
            "Default" => SerialFormat::Default,
            "Qingpi" => SerialFormat::Qingpi,
            "3DS Controller" => SerialFormat::ThreeDsController,
            _ => {
                return Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid data format: {format_str}")
                }));
            }
        };
        // Update serial format first (serial → keypress order)
        {
            let mut sender = state.serial.lock().await;
            sender.set_data_format(format_str);
        } // serial lock released
        {
            let mut kp = state.keypress.lock().await;
            kp.set_serial_format(format);
        } // keypress lock released
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": "Serial config updated"
    }))
}

/// Get serial connection status.
#[utoipa::path(
    get,
    path = "/api/serial/status",
    tag = "serial",
    responses(
        (status = 200, description = "Serial connection status", body = serde_json::Value)
    )
)]
pub async fn serial_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sender = state.serial.lock().await;
    Json(serde_json::json!({
        "is_open": sender.is_opened()
    }))
}
