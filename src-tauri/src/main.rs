use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use clap::Parser;
use serde::Deserialize;
use tokio::sync::Mutex;

use pokecon_core::command_manager::CommandManager;
use pokecon_cv::camera::{Camera, CameraConfig, FlipMode, Frame, MockCameraBackend, PixelFormat};
use pokecon_events::EventBus;
use pokecon_serial::SendFormat;
use pokecon_serial::keypress::{KeyPress, SerialFormat};
use pokecon_serial::keys::{Button, Direction, GamepadInput, Stick, Touchscreen};
use pokecon_serial::sender::Sender;

/// Poke-Controller Modified Extension — Tauri/Web UI
#[derive(Parser, Debug)]
#[command(name = "pokecon", version, about)]
struct Args {
    /// UI mode: "tauri" (native window) or "web" (HTTP server)
    #[arg(long = "ui", default_value = "tauri")]
    ui: String,

    /// Port for HTTP server (used in both web and tauri modes)
    #[arg(long, default_value = "8020")]
    port: u16,

    /// Static files directory
    #[arg(long = "web-dir", default_value = "web")]
    web_dir: PathBuf,

    /// Scripts directory for command manager
    #[arg(long = "scripts-dir", default_value = "scripts")]
    scripts_dir: PathBuf,
}

/// Shared application state accessible from all HTTP handlers.
#[derive(Clone)]
#[allow(dead_code)]
struct AppState {
    serial: Arc<Mutex<Sender>>,
    keypress: Arc<Mutex<KeyPress>>,
    command_manager: Arc<Mutex<CommandManager>>,
    event_bus: EventBus,
    camera: Arc<Mutex<Option<Camera>>>,
    /// Broadcast channel for WebSocket event forwarding
    event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
}

// ── Helper: Parse a button name string into a Button bitflag ────────────────────

fn parse_button(name: &str) -> Option<Button> {
    match name.to_uppercase().as_str() {
        "A" => Some(Button::A),
        "B" => Some(Button::B),
        "X" => Some(Button::X),
        "Y" => Some(Button::Y),
        "L" => Some(Button::L),
        "R" => Some(Button::R),
        "ZL" => Some(Button::ZL),
        "ZR" => Some(Button::ZR),
        "MINUS" | "SELECT" => Some(Button::MINUS),
        "PLUS" | "START" => Some(Button::PLUS),
        "LCLICK" | "L3" | "POWER" => Some(Button::LCLICK),
        "RCLICK" | "R3" | "WIRELESS" => Some(Button::RCLICK),
        "HOME" => Some(Button::HOME),
        "CAPTURE" => Some(Button::CAPTURE),
        _ => None,
    }
}

fn parse_buttons(names: &[String]) -> Vec<Button> {
    names.iter().filter_map(|n| parse_button(n)).collect()
}

// ── Helper: Encode a camera Frame as base64 JPEG ───────────────────────────────

fn frame_to_base64_jpeg(frame: &Frame) -> Result<String, String> {
    use base64::Engine;

    // Normalize pixel data to RGB (3 bytes per pixel) for JPEG encoding
    let (data, width, height) = match frame.format {
        PixelFormat::Rgb => (frame.data.clone(), frame.width, frame.height),
        PixelFormat::Bgr => {
            // Convert BGR → RGB by swapping first and third byte
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(3)
                .flat_map(|c| [c[2], c[1], c[0]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Rgba => {
            // Drop alpha channel
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(4)
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Gray => {
            // Expand grayscale to RGB
            let rgb: Vec<u8> = frame.data.iter().flat_map(|&g| [g, g, g]).collect();
            (rgb, frame.width, frame.height)
        }
    };

    let mut buf = Vec::new();
    {
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buf);
        encoder
            .encode(&data, width, height, image::ColorType::Rgb8)
            .map_err(|e| format!("JPEG encode error: {}", e))?;
    }

    Ok(base64::engine::general_purpose::STANDARD.encode(&buf))
}

// ── Helper: Convert send format to default serial row string ───────────────────

fn format_default_row(fmt: &SendFormat, l_stick_changed: bool, r_stick_changed: bool) -> String {
    let mut send_btn = (fmt.btn as u32) << 2;
    if l_stick_changed {
        send_btn |= 0x2;
    }
    if r_stick_changed {
        send_btn |= 0x1;
    }

    let mut result = format!("{:#08x}", send_btn);
    result.push(' ');
    result.push_str(&fmt.hat.to_string());

    if l_stick_changed {
        result.push_str(&format!(" {:x} {:x}", fmt.lx, fmt.ly));
    }
    if r_stick_changed {
        result.push_str(&format!(" {:x} {:x}", fmt.rx, fmt.ry));
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Camera Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/cameras — list available camera devices (placeholder)
async fn cameras_list() -> Json<serde_json::Value> {
    // TODO: Implement real camera enumeration when backend is ready
    Json(serde_json::json!({
        "status": "ok",
        "devices": [{"index": 0, "name": "Default Camera"}],
    }))
}

/// GET /api/camera/status — get camera connection status
async fn camera_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => {
            let cfg = camera.config();
            // Check open state via the internal backend
            Json(serde_json::json!({
                "status": "ok",
                "is_open": true,
                "device_index": cfg.device_index,
                "width": cfg.width,
                "height": cfg.height,
                "fps": cfg.fps,
                "flip": cfg.flip.as_str(),
            }))
        }
        None => Json(serde_json::json!({
            "status": "ok",
            "is_open": false,
        })),
    }
}

#[derive(Deserialize)]
struct CameraOpenRequest {
    device_index: Option<i32>,
    width: Option<u32>,
    height: Option<u32>,
}

/// POST /api/camera/open — open camera with optional config
async fn camera_open(
    State(state): State<AppState>,
    Json(body): Json<CameraOpenRequest>,
) -> Json<serde_json::Value> {
    let mut cam = state.camera.lock().await;
    let config = CameraConfig {
        device_index: body.device_index.unwrap_or(0),
        width: body.width.unwrap_or(1280),
        height: body.height.unwrap_or(720),
        fps: 30,
        flip: Default::default(),
    };

    // MockCameraBackendを使用（実装時にV4L2/OpenCVバックエンドに置き換え）
    let backend = MockCameraBackend::new();
    let mut camera = Camera::new(Box::new(backend));

    match camera.open(config).await {
        Ok(()) => {
            *cam = Some(camera);
            Json(serde_json::json!({
                "status": "ok",
                "message": "Camera opened",
                "device_index": body.device_index.unwrap_or(0),
                "width": body.width.unwrap_or(1280),
                "height": body.height.unwrap_or(720),
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

/// POST /api/camera/close — close camera
async fn camera_close(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut cam = state.camera.lock().await;
    if let Some(ref camera) = *cam {
        camera.close().await;
    }
    *cam = None;
    Json(serde_json::json!({
        "status": "ok",
        "message": "Camera closed"
    }))
}

/// GET /api/camera/frame — get current frame as base64 JPEG
async fn camera_frame(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => match camera.capture().await {
            Ok(frame) => match frame_to_base64_jpeg(&frame) {
                Ok(b64) => Json(serde_json::json!({
                    "status": "ok",
                    "width": frame.width,
                    "height": frame.height,
                    "format": format!("{:?}", frame.format),
                    "data": b64,
                })),
                Err(e) => Json(serde_json::json!({
                    "status": "error",
                    "message": e,
                })),
            },
            Err(e) => Json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })),
        },
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Camera not opened",
        })),
    }
}

#[derive(Deserialize)]
struct CaptureRequest {
    filename: String,
}

#[derive(Deserialize)]
struct CameraConfigRequest {
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
    flip: Option<String>,
}

/// POST /api/camera/capture — save current frame to a file
async fn camera_capture(
    State(state): State<AppState>,
    Json(body): Json<CaptureRequest>,
) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => match camera.capture().await {
            Ok(frame) => {
                // Save as JPEG file
                let path = PathBuf::from(&body.filename);
                match save_frame_as_jpeg(&frame, &path) {
                    Ok(()) => Json(serde_json::json!({
                        "status": "ok",
                        "message": format!("Captured to {}", body.filename),
                        "path": body.filename,
                        "width": frame.width,
                        "height": frame.height,
                    })),
                    Err(e) => Json(serde_json::json!({
                        "status": "error",
                        "message": e,
                    })),
                }
            }
            Err(e) => Json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })),
        },
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Camera not opened",
        })),
    }
}

/// POST /api/camera/config - update camera configuration (width, height, fps, flip)
async fn camera_config(
    State(state): State<AppState>,
    Json(body): Json<CameraConfigRequest>,
) -> Json<serde_json::Value> {
    let mut cam = state.camera.lock().await;
    match cam.as_mut() {
        Some(camera) => {
            let mut config = camera.config().clone();

            if let Some(width) = body.width {
                config.width = width;
            }
            if let Some(height) = body.height {
                config.height = height;
            }
            if let Some(fps) = body.fps {
                config.fps = fps;
            }
            if let Some(ref flip_str) = body.flip {
                config.flip = FlipMode::parse_str(flip_str);
            }

            camera.update_config(config.clone());

            Json(serde_json::json!({
                "status": "ok",
                "device_index": config.device_index,
                "width": config.width,
                "height": config.height,
                "fps": config.fps,
                "flip": config.flip.as_str(),
            }))
        }
        None => Json(serde_json::json!({
            "status": "error",
            "message": "Camera not opened",
        })),
    }
}

fn save_frame_as_jpeg(frame: &Frame, path: &PathBuf) -> Result<(), String> {
    let (data, width, height) = match frame.format {
        PixelFormat::Rgb => (frame.data.clone(), frame.width, frame.height),
        PixelFormat::Bgr => {
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(3)
                .flat_map(|c| [c[2], c[1], c[0]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Rgba => {
            let rgb: Vec<u8> = frame
                .data
                .chunks_exact(4)
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect();
            (rgb, frame.width, frame.height)
        }
        PixelFormat::Gray => {
            let rgb: Vec<u8> = frame.data.iter().flat_map(|&g| [g, g, g]).collect();
            (rgb, frame.width, frame.height)
        }
    };

    let mut file =
        std::fs::File::create(path).map_err(|e| format!("Failed to create file: {}", e))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut file);
    encoder
        .encode(&data, width, height, image::ColorType::Rgb8)
        .map_err(|e| format!("JPEG encode error: {}", e))?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Key Input Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct PressRequest {
    buttons: Vec<String>,
    /// Duration in milliseconds to hold before releasing (default: 50)
    #[serde(default = "default_press_duration")]
    duration: u64,
    /// Wait in milliseconds after releasing (default: 0)
    #[serde(default)]
    wait: u64,
}

fn default_press_duration() -> u64 {
    50
}

#[derive(Deserialize)]
struct HoldRequest {
    buttons: Vec<String>,
    /// Duration in milliseconds to hold (default: 0 = indefinite via keypress hold state)
    #[serde(default)]
    duration: u64,
}

#[derive(Deserialize)]
struct StickRequest {
    /// Stick identifier: "left" or "right"
    stick: String,
    /// X-axis value (0–255, 128 = center)
    x: u8,
    /// Y-axis value (0–255, 128 = center)
    y: u8,
    /// Duration in milliseconds before centering (default: 100)
    #[serde(default = "default_stick_duration")]
    duration: u64,
}

fn default_stick_duration() -> u64 {
    100
}

#[derive(Deserialize)]
struct TouchRequest {
    /// X coordinate (0–?)
    x: u16,
    /// Y coordinate (0–255)
    y: u8,
    /// Duration in milliseconds before releasing (default: 100)
    #[serde(default = "default_touch_duration")]
    duration: u64,
}

fn default_touch_duration() -> u64 {
    100
}

/// POST /api/input/press — press buttons for a duration, then release
async fn input_press(
    State(state): State<AppState>,
    Json(body): Json<PressRequest>,
) -> Json<serde_json::Value> {
    let buttons = parse_buttons(&body.buttons);
    if buttons.is_empty() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "No valid buttons specified. Valid names: A, B, X, Y, L, R, ZL, ZR, MINUS, PLUS, LCLICK, RCLICK, HOME, CAPTURE"
        }));
    }

    let mut sender = state.serial.lock().await;
    if !sender.is_opened() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Serial port not open"
        }));
    }

    let mut fmt = SendFormat::new();
    fmt.set_button(&buttons);

    let row = format_default_row(&fmt, false, false);
    if let Err(e) = sender.write_row(&row, true).await {
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }

    // Release after duration
    if body.duration > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
    }

    let release_fmt = SendFormat::new();
    let release_row = format_default_row(&release_fmt, false, false);
    if let Err(e) = sender.write_row(&release_row, true).await {
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }

    if body.wait > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(body.wait)).await;
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": format!("Pressed {:?} for {}ms", buttons, body.duration),
    }))
}

/// POST /api/input/hold — hold buttons (toggle on)
async fn input_hold(
    State(state): State<AppState>,
    Json(body): Json<HoldRequest>,
) -> Json<serde_json::Value> {
    let buttons = parse_buttons(&body.buttons);
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
                tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
            }
            Json(serde_json::json!({
                "status": "ok",
                "message": format!("Holding {:?}", buttons),
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

/// POST /api/input/release — release all held buttons
async fn input_release(State(state): State<AppState>) -> Json<serde_json::Value> {
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
async fn input_stick(
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

    let mut sender = state.serial.lock().await;
    if !sender.is_opened() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Serial port not open"
        }));
    }

    let mut fmt = SendFormat::new();
    fmt.set_any_direction(&[direction.clone()]);

    let l_changed = matches!(stick, Stick::Left);
    let r_changed = matches!(stick, Stick::Right);
    let row = format_default_row(&fmt, l_changed, r_changed);

    if let Err(e) = sender.write_row(&row, true).await {
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }

    // Wait for duration
    if body.duration > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
    }

    // Recenter
    let center_fmt = SendFormat::new();
    let center_row = format_default_row(&center_fmt, l_changed, r_changed);
    if let Err(e) = sender.write_row(&center_row, true).await {
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": format!("{:?} stick moved to ({}, {}) for {}ms", stick, body.x, body.y, body.duration),
    }))
}

/// POST /api/input/touch — tap the touchscreen
async fn input_touch(
    State(state): State<AppState>,
    Json(body): Json<TouchRequest>,
) -> Json<serde_json::Value> {
    let touch = Touchscreen::new(body.x, body.y);

    let mut sender = state.serial.lock().await;
    if !sender.is_opened() {
        return Json(serde_json::json!({
            "status": "error",
            "message": "Serial port not open"
        }));
    }

    // For touchscreen in Default format, we need to use a format that supports it.
    // Default serial format does not include touchscreen data (only Qingpi/3DS do).
    // Send via Qingpi format for touch support.
    let mut fmt = SendFormat::new();
    fmt.set_touchscreen(&[touch]);

    // Use a simple row approach: write the Qingpi format bytes
    let qingpi_data = fmt.convert_to_qingpi();
    if let Err(e) = sender.write_list(&qingpi_data, true).await {
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }

    if body.duration > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
    }

    // Release touch
    let release_fmt = SendFormat::new();
    let release_data = release_fmt.convert_to_qingpi();
    if let Err(e) = sender.write_list(&release_data, true).await {
        return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": format!("Touched ({}, {}) for {}ms", body.x, body.y, body.duration),
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// WebSocket Endpoint
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /ws — WebSocket upgrade endpoint for real-time events
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Handle an individual WebSocket connection
async fn handle_ws(mut socket: WebSocket, state: AppState) {
    // Subscribe to the global event broadcast channel
    let mut rx = state.event_tx.subscribe();

    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
            // Incoming message from the WebSocket client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        tracing::debug!("WS received: {}", text);
                        // Parse incoming JSON commands (optional)
                        if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some("ping") = cmd.get("type").and_then(|v| v.as_str()) {
                                let _ = socket.send(Message::Text(
                                    serde_json::json!({"type": "pong"}).to_string().into()
                                )).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(e)) => {
                        tracing::warn!("WS error: {}", e);
                        break;
                    }
                    None => break,
                    _ => {} // Ignore Binary, Ping, Pong
                }
            }
            // Event from the broadcast channel → forward to client
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if socket.send(Message::Text(event.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("WS receiver lagged by {} messages", n);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    tracing::info!("WebSocket client disconnected");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Core Endpoints (unchanged)
// ═══════════════════════════════════════════════════════════════════════════════

/// Status endpoint — returns basic info.
async fn api_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "shared"
    }))
}

/// Greet endpoint — same API in both modes.
async fn api_greet(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let name = params.get("name").map(|s| s.as_str()).unwrap_or("Trainer");
    Json(serde_json::json!({
        "message": format!("Hello, {}! Welcome to Poke-Controller.", name)
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Serial Port Endpoints (unchanged)
// ═══════════════════════════════════════════════════════════════════════════════

/// List available serial ports on the system.
async fn serial_ports() -> Json<serde_json::Value> {
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
            "error": format!("Failed to list ports: {}", e)
        })),
    }
}

#[derive(Deserialize)]
struct OpenRequest {
    port_num: u32,
    port_name: Option<String>,
    baudrate: u32,
}

/// Open a serial port connection.
async fn serial_open(
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
async fn serial_close(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut sender = state.serial.lock().await;
    sender.close().await;
    Json(serde_json::json!({
        "status": "ok",
        "message": "Serial port closed"
    }))
}

#[derive(Deserialize)]
struct WriteRequest {
    data: String,
}

/// Write data to the serial port.
async fn serial_write(
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

/// POST /api/serial/config — set baudrate and data format
async fn serial_config(
    State(state): State<AppState>,
    Json(body): Json<SerialConfigRequest>,
) -> Json<serde_json::Value> {
    // Separate scopes to avoid holding locks simultaneously
    if let Some(baudrate) = body.baudrate {
        let mut sender = state.serial.lock().await;
        if sender.is_opened() {
            if let Err(e) = sender.set_baudrate(baudrate).await {
                return Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to set baudrate: {}", e)
                }));
            }
        }
    }

    if let Some(ref format_str) = body.data_format {
        let format = match format_str.as_str() {
            "Default" => SerialFormat::Default,
            "Qingpi" => SerialFormat::Qingpi,
            "3DS Controller" => SerialFormat::_3dsController,
            _ => {
                return Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid data format: {}", format_str)
                }));
            }
        };
        let mut kp = state.keypress.lock().await;
        kp.set_serial_format(format);
        let mut sender = state.serial.lock().await;
        sender.set_data_format(format_str);
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": "Serial config updated"
    }))
}

#[derive(Deserialize)]
struct SerialConfigRequest {
    baudrate: Option<u32>,
    data_format: Option<String>,
}

/// Get serial connection status.
async fn serial_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sender = state.serial.lock().await;
    Json(serde_json::json!({
        "is_open": sender.is_opened()
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command Management Endpoints (unchanged)
// ═══════════════════════════════════════════════════════════════════════════════

/// List all available (loaded) commands.
async fn commands_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cm = state.command_manager.lock().await;
    let commands: Vec<serde_json::Value> = cm
        .list()
        .iter()
        .map(|info| {
            serde_json::json!({
                "name": info.name,
                "path": info.path.to_string_lossy(),
                "description": info.description,
            })
        })
        .collect();
    Json(serde_json::json!({ "commands": commands }))
}

#[derive(Deserialize)]
struct NameRequest {
    name: String,
}

/// Load a command by scanning the scripts directory for the given name.
async fn commands_load(
    State(state): State<AppState>,
    Json(body): Json<NameRequest>,
) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    // Scan the scripts directory to discover available commands
    match cm.scan() {
        Ok(names) => {
            if names.contains(&body.name) {
                Json(serde_json::json!({
                    "status": "ok",
                    "message": format!("Command '{}' loaded", body.name)
                }))
            } else if cm.get(&body.name).is_some() {
                Json(serde_json::json!({
                    "status": "ok",
                    "message": format!("Command '{}' already loaded", body.name)
                }))
            } else {
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Command '{}' not found in scripts directory", body.name)
                }))
            }
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to scan scripts: {}", e)
        })),
    }
}

/// Start (activate) a command by name.
async fn commands_start(
    State(state): State<AppState>,
    Json(body): Json<NameRequest>,
) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    // Ensure command is loaded before activating
    if cm.get(&body.name).is_none() {
        let _ = cm.scan();
    }

    match cm.set_active(&body.name) {
        Ok(_) => {
            // Emit command lifecycle event
            let _ = state.event_tx.send(serde_json::json!({
                "type": "command.start",
                "data": { "name": &body.name },
            }));
            Json(serde_json::json!({
                "status": "ok",
                "message": format!("Command '{}' started", body.name)
            }))
        }
        Err(e) => {
            let _ = state.event_tx.send(serde_json::json!({
                "type": "command.error",
                "data": { "name": &body.name, "error": e.to_string() },
            }));
            Json(serde_json::json!({
                "status": "error",
                "message": e.to_string()
            }))
        }
    }
}

/// Stop the currently active command.
async fn commands_stop(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    let name = cm.active_name().map(|s| s.to_string());
    cm.stop();

    if let Some(ref n) = name {
        let _ = state.event_tx.send(serde_json::json!({
            "type": "command.stop",
            "data": { "name": n },
        }));
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": match name {
            Some(ref n) => format!("Command '{}' stopped", n),
            None => "No active command to stop".to_string(),
        }
    }))
}

/// Get information about the currently active command.
async fn commands_active(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cm = state.command_manager.lock().await;
    match cm.active() {
        Some(info) => Json(serde_json::json!({
            "active": true,
            "name": info.name,
            "path": info.path.to_string_lossy(),
            "description": info.description,
        })),
        None => Json(serde_json::json!({
            "active": false,
        })),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Server Setup
// ═══════════════════════════════════════════════════════════════════════════════

/// Start the HTTP server — shared between web and tauri modes.
async fn start_http_server(port: u16, web_dir: PathBuf, state: AppState) {
    let app = axum::Router::new()
        // Core endpoints
        .route("/api/status", get(api_status))
        .route("/api/greet", get(api_greet))
        // Camera endpoints
        .route("/api/cameras", get(cameras_list))
        .route("/api/camera/status", get(camera_status))
        .route("/api/camera/open", post(camera_open))
        .route("/api/camera/close", post(camera_close))
        .route("/api/camera/frame", get(camera_frame))
        .route("/api/camera/capture", post(camera_capture))
        .route("/api/camera/config", post(camera_config))
        // Key input endpoints
        .route("/api/input/press", post(input_press))
        .route("/api/input/hold", post(input_hold))
        .route("/api/input/release", post(input_release))
        .route("/api/input/stick", post(input_stick))
        .route("/api/input/touch", post(input_touch))
        // WebSocket endpoint
        .route("/ws", get(ws_handler))
        // Serial port endpoints
        .route("/api/serial/ports", get(serial_ports))
        .route("/api/serial/open", post(serial_open))
        .route("/api/serial/close", post(serial_close))
        .route("/api/serial/write", post(serial_write))
        .route("/api/serial/config", post(serial_config))
        .route("/api/serial/status", get(serial_status))
        // Command management endpoints
        .route("/api/commands", get(commands_list))
        .route("/api/commands/load", post(commands_load))
        .route("/api/commands/start", post(commands_start))
        .route("/api/commands/stop", post(commands_stop))
        .route("/api/commands/active", get(commands_active))
        .fallback_service(
            tower_http::services::ServeDir::new(&web_dir).append_index_html_on_directories(true),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("HTTP server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app).await.expect("server error");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main
// ═══════════════════════════════════════════════════════════════════════════════

fn main() {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Create broadcast channel for WebSocket event forwarding
    let (event_tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(256);

    // Create shared application state
    let cm = CommandManager::new(&args.scripts_dir).expect("failed to create command manager");
    let state = AppState {
        serial: Arc::new(Mutex::new(Sender::new(true))),
        keypress: Arc::new(Mutex::new(KeyPress::new(Sender::new(true)))),
        command_manager: Arc::new(Mutex::new(cm)),
        event_bus: EventBus::new(),
        camera: Arc::new(Mutex::new(None)),
        event_tx,
    };

    // Start the HTTP server in both modes — Tauri embeds it internally
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let server_handle = rt.spawn(start_http_server(args.port, args.web_dir.clone(), state));

    match args.ui.as_str() {
        "web" => {
            tracing::info!("Web UI mode — serving at http://127.0.0.1:{}", args.port);
            rt.block_on(server_handle).expect("server task failed");
        }
        _ => {
            tracing::info!(
                "Tauri UI mode — HTTP server at http://127.0.0.1:{}",
                args.port
            );
            start_tauri(args.port);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tauri
// ═══════════════════════════════════════════════════════════════════════════════

/// Start the Tauri native window — loads the same web UI via WebView.
fn start_tauri(port: u16) {
    tauri::Builder::default()
        .setup(move |app| {
            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(format!("http://127.0.0.1:{}", port).parse().unwrap()),
            )
            .title("Poke-Controller Modified Extension")
            .inner_size(1280.0, 800.0)
            .center()
            .build()?;

            tracing::debug!("Tauri window opened with WebView");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
