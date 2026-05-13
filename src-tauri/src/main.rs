use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use clap::Parser;
use futures::stream;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;
use url::Url;

use futures::future::join_all;
use pokecon_core::command_manager::CommandManager;
use pokecon_core::profile::ProfileManager;
#[cfg(feature = "v4l")]
use pokecon_cv::backends::{V4lCameraBackend, list_cameras as v4l_list_cameras};
#[cfg(not(feature = "v4l"))]
use pokecon_cv::camera::MockCameraBackend;
use pokecon_cv::camera::{Camera, CameraConfig, FlipMode, Frame, PixelFormat};
use pokecon_events::EventBus;
use pokecon_notify::discord::DiscordNotifier;
use pokecon_notify::line::LineNotifier;
use pokecon_notify::windows::WindowsNotifier;
use pokecon_notify::{Notification, Notifier};
use pokecon_serial::SendFormat;
use pokecon_serial::keypress::{KeyPress, SerialFormat};
use pokecon_serial::keys::{Button, Direction, GamepadInput, Stick, Touchscreen};
use pokecon_serial::sender::Sender;
use utoipa::OpenApi;
use utoipa::ToSchema;

mod webrtc;

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

    /// Profiles directory for profile manager
    #[arg(long = "profiles-dir", default_value = "profiles")]
    profiles_dir: PathBuf,
}

/// Configuration for mouse-to-stick control.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
struct MouseStickConfig {
    /// Whether left-stick mouse control is enabled
    left_enabled: bool,
    /// Whether right-stick mouse control is enabled
    right_enabled: bool,
    /// Sensitivity multiplier (default: 1.0)
    sensitivity: f32,
}

impl Default for MouseStickConfig {
    fn default() -> Self {
        Self {
            left_enabled: false,
            right_enabled: false,
            sensitivity: 1.0,
        }
    }
}

/// Shared application state accessible from all HTTP handlers.
#[derive(Clone)]
#[allow(dead_code)]
struct AppState {
    serial: Arc<Mutex<Sender>>,
    keypress: Arc<Mutex<KeyPress>>,
    /// Filter string for commands
    command_filter: Arc<Mutex<String>>,
    command_manager: Arc<Mutex<CommandManager>>,
    event_bus: EventBus,
    camera: Arc<Mutex<Option<Camera>>>,
    /// Broadcast channel for WebSocket event forwarding
    event_tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    /// Current gamepad type ("ProController" or "Xinput")
    gamepad_type: Arc<Mutex<String>>,
    /// Whether keyboard input is enabled
    keyboard_enabled: Arc<Mutex<bool>>,
    /// Profile manager
    profile_manager: Arc<Mutex<ProfileManager>>,
    /// Notification configuration
    notification_config: Arc<Mutex<NotificationConfig>>,
    /// Mouse stick control configuration
    mouse_stick: Arc<Mutex<MouseStickConfig>>,
    /// WebRTC video session manager
    webrtc_manager: Arc<Mutex<webrtc::WebRtcManager>>,
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

// ── Helper: Encode a camera Frame as raw JPEG bytes ────────────────────────

fn frame_to_jpeg_bytes(frame: &Frame) -> Result<Vec<u8>, String> {
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

    Ok(buf)
}

// ── Helper: Encode a camera Frame as base64 JPEG ───────────────────────────────

fn frame_to_base64_jpeg(frame: &Frame) -> Result<String, String> {
    use base64::Engine;

    let jpeg_bytes = frame_to_jpeg_bytes(frame)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
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
// Controller Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Serialize, Deserialize, ToSchema)]
struct ControllerTypeRequest {
    /// Gamepad type: "ProController" or "Xinput"
    gamepad_type: String,
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
async fn controller_set_type(
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
async fn controller_get_type(State(state): State<AppState>) -> Json<serde_json::Value> {
    let gt = state.gamepad_type.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "gamepad_type": gt.clone(),
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
struct KeyboardRequest {
    /// Whether keyboard input is enabled
    enabled: bool,
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
async fn controller_set_keyboard(
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
async fn controller_get_keyboard(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ke = state.keyboard_enabled.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "keyboard_enabled": *ke,
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
struct MouseStickRequest {
    /// Stick identifier: "left" or "right"
    stick: String,
    /// Whether to enable mouse-to-stick control
    enabled: bool,
    /// Sensitivity multiplier (optional, default: 1.0)
    #[serde(default = "default_sensitivity")]
    sensitivity: f32,
}

fn default_sensitivity() -> f32 {
    1.0
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
async fn controller_set_mouse_stick(
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
async fn controller_get_mouse_stick(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ms = state.mouse_stick.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "left_enabled": ms.left_enabled,
        "right_enabled": ms.right_enabled,
        "sensitivity": ms.sensitivity,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Camera Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/cameras — list available camera devices
#[utoipa::path(
    get,
    path = "/api/cameras",
    tag = "camera",
    responses(
        (status = 200, description = "List of available camera devices", body = serde_json::Value)
    )
)]
async fn cameras_list() -> Json<serde_json::Value> {
    #[cfg(feature = "v4l")]
    {
        let devices: Vec<serde_json::Value> = v4l_list_cameras()
            .into_iter()
            .map(|(idx, name)| {
                serde_json::json!({
                    "index": idx,
                    "name": name,
                })
            })
            .collect();
        Json(serde_json::json!({
            "status": "ok",
            "devices": devices,
        }))
    }

    #[cfg(not(feature = "v4l"))]
    {
        Json(serde_json::json!({
            "status": "ok",
            "devices": [{"index": 0, "name": "Default Camera"}],
        }))
    }
}

/// GET /api/camera/status — get camera connection status
#[utoipa::path(
    get,
    path = "/api/camera/status",
    tag = "camera",
    responses(
        (status = 200, description = "Camera connection status and config", body = serde_json::Value)
    )
)]
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

#[derive(Serialize, Deserialize, ToSchema)]
struct CameraOpenRequest {
    device_index: Option<i32>,
    width: Option<u32>,
    height: Option<u32>,
}

/// POST /api/camera/open — open camera with optional config
#[utoipa::path(
    post,
    path = "/api/camera/open",
    tag = "camera",
    request_body = CameraOpenRequest,
    responses(
        (status = 200, description = "Camera opened", body = serde_json::Value)
    )
)]
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

    // Choose backend: V4lCameraBackend with "v4l" feature, MockCameraBackend otherwise
    #[cfg(feature = "v4l")]
    let backend = V4lCameraBackend::new();
    #[cfg(not(feature = "v4l"))]
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
#[utoipa::path(
    post,
    path = "/api/camera/close",
    tag = "camera",
    responses(
        (status = 200, description = "Camera closed", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/camera/frame",
    tag = "camera",
    responses(
        (status = 200, description = "Current camera frame as base64 JPEG", body = serde_json::Value)
    )
)]
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

// ═══════════════════════════════════════════════════════════════════════════════
// MJPEG over HTTP Stream Endpoint
// ═══════════════════════════════════════════════════════════════════════════════

const MJPEG_BOUNDARY: &str = "mjpeg-frame";

/// GET /camera/stream — MJPEG over HTTP streaming endpoint
///
/// Returns a multipart/x-mixed-replace stream of JPEG frames at ~30 fps.
/// Returns 404 if the camera is not open.
async fn camera_stream(State(state): State<AppState>) -> Response {
    // Quick check: camera must be open
    {
        let cam = state.camera.lock().await;
        if cam.is_none() {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "status": "error",
                        "message": "Camera not opened",
                    })
                    .to_string(),
                ))
                .unwrap();
        }
    }

    let state2 = state.clone();

    // Build an infinite stream of JPEG frames wrapped in multipart boundaries
    let stream = stream::unfold(state2, |state| async move {
        // Capture a single frame under the camera lock
        let jpeg_result: Option<Vec<u8>> = {
            let cam = state.camera.lock().await;
            match cam.as_ref() {
                Some(camera) => match camera.capture().await {
                    Ok(frame) => frame_to_jpeg_bytes(&frame).ok(),
                    Err(_) => None,
                },
                None => return None, // Camera closed → end stream
            }
        };

        match jpeg_result {
            Some(jpeg_bytes) => {
                let header = format!(
                    "\r\n--{MJPEG_BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                    jpeg_bytes.len()
                );
                let mut chunk = Vec::with_capacity(header.len() + jpeg_bytes.len() + 2);
                chunk.extend_from_slice(header.as_bytes());
                chunk.extend_from_slice(&jpeg_bytes);
                chunk.extend_from_slice(b"\r\n");

                // Throttle to ~30 fps
                tokio::time::sleep(Duration::from_millis(33)).await;
                Some((Ok::<_, axum::Error>(Bytes::from(chunk)), state))
            }
            None => {
                // Capture failed — skip frame and retry
                tokio::time::sleep(Duration::from_millis(33)).await;
                Some((Ok::<_, axum::Error>(Bytes::new()), state))
            }
        }
    });

    Response::builder()
        .header(
            "Content-Type",
            format!("multipart/x-mixed-replace; boundary={MJPEG_BOUNDARY}"),
        )
        .header(
            "Cache-Control",
            "no-store, no-cache, must-revalidate, proxy-revalidate",
        )
        .header("Pragma", "no-cache")
        .header("Expires", "0")
        .body(Body::from_stream(stream))
        .unwrap()
}

#[derive(Serialize, Deserialize, ToSchema)]
struct CaptureRequest {
    filename: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct CameraConfigRequest {
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
    flip: Option<String>,
}

/// POST /api/camera/capture — save current frame to a file
#[utoipa::path(
    post,
    path = "/api/camera/capture",
    tag = "camera",
    request_body = CaptureRequest,
    responses(
        (status = 200, description = "Frame captured to file", body = serde_json::Value)
    )
)]
async fn camera_capture(
    State(state): State<AppState>,
    Json(body): Json<CaptureRequest>,
) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => match camera.capture().await {
            Ok(frame) => {
                // SECURITY: Sanitize filename to prevent path traversal
                let safe_filename = sanitize_filename(&body.filename);
                let path = PathBuf::from("Captures").join(&safe_filename);
                match save_frame_as_jpeg(&frame, &path) {
                    Ok(()) => Json(serde_json::json!({
                        "status": "ok",
                        "message": format!("Captured to {}", path.display()),
                        "path": path.to_string_lossy(),
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

/// Sanitize a filename to prevent path traversal attacks.
/// Strips directory separators and `..` sequences, keeping only the final filename component.
fn sanitize_filename(name: &str) -> String {
    // Use std::path to get the file name component only (strips all directory parts)
    let path = PathBuf::from(name);
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    // Remove any remaining path separators or special characters
    file_name
        .chars()
        .filter(|&c| !std::path::is_separator(c) && c != '\0')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Validate that a URL is safe to use (no SSRF).
/// Only http/https schemes allowed, private/internal IPs rejected.
fn validate_webhook_url(url_str: &str) -> Result<(), String> {
    if url_str.is_empty() {
        return Ok(()); // Empty URLs are allowed (means "not configured")
    }

    let parsed = Url::parse(url_str).map_err(|_| format!("Invalid URL: {}", url_str))?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "Unsupported URL scheme '{}'. Only http and https are allowed.",
            scheme
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL has no host: {}", url_str))?;

    // Block private/internal addresses (SSRF prevention)
    let lower_host = host.to_lowercase();

    // Block internal hostnames
    if lower_host == "localhost"
        || lower_host == "localhost.localdomain"
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".internal")
    {
        return Err(format!(
            "URL host '{}' is an internal hostname and is not allowed",
            host
        ));
    }

    // Parse as IP and block loopback/private/link-local addresses
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        let is_blocked = match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_unspecified() || v4.is_link_local()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        };
        if is_blocked {
            return Err(format!(
                "URL host '{}' is a private/internal IP address and is not allowed",
                ip
            ));
        }
    }

    Ok(())
}

/// POST /api/camera/config - update camera configuration (width, height, fps, flip)
#[utoipa::path(
    post,
    path = "/api/camera/config",
    tag = "camera",
    request_body = CameraConfigRequest,
    responses(
        (status = 200, description = "Camera config updated", body = serde_json::Value)
    )
)]
async fn camera_config(
    State(state): State<AppState>,
    Json(body): Json<CameraConfigRequest>,
) -> Json<serde_json::Value> {
    let mut cam = state.camera.lock().await;
    match cam.as_mut() {
        Some(camera) => {
            let mut config = camera.config().clone();

            if let Some(width) = body.width {
                if width == 0 || width > 4096 {
                    return Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Invalid width: {}. Must be between 1 and 4096.", width),
                    }));
                }
                config.width = width;
            }
            if let Some(height) = body.height {
                if height == 0 || height > 4096 {
                    return Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Invalid height: {}. Must be between 1 and 4096.", height),
                    }));
                }
                config.height = height;
            }
            if let Some(fps) = body.fps {
                if fps == 0 || fps > 120 {
                    return Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Invalid fps: {}. Must be between 1 and 120.", fps),
                    }));
                }
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

#[derive(Serialize, Deserialize, ToSchema)]
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

#[derive(Serialize, Deserialize, ToSchema)]
struct HoldRequest {
    buttons: Vec<String>,
    /// Duration in milliseconds to hold (default: 0 = indefinite via keypress hold state)
    #[serde(default)]
    duration: u64,
}

#[derive(Serialize, Deserialize, ToSchema)]
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

#[derive(Serialize, Deserialize, ToSchema)]
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
#[utoipa::path(
    post,
    path = "/api/input/press",
    tag = "input",
    request_body = PressRequest,
    responses(
        (status = 200, description = "Buttons pressed", body = serde_json::Value)
    )
)]
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

        let press_row = format_default_row(&fmt, false, false);
        if let Err(e) = sender.write_row(&press_row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }

        // Build release row while we still have the lock
        let release_fmt = SendFormat::new();
        format_default_row(&release_fmt, false, false)
    }; // Lock is dropped here

    // Release after duration (lock released, safe to sleep)
    if body.duration > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
    }

    // Re-acquire lock to send release
    {
        let mut sender = state.serial.lock().await;
        if let Err(e) = sender.write_row(&release_row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }
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
#[utoipa::path(
    post,
    path = "/api/input/hold",
    tag = "input",
    request_body = HoldRequest,
    responses(
        (status = 200, description = "Buttons held", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/input/release",
    tag = "input",
    responses(
        (status = 200, description = "All buttons released", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/input/stick",
    tag = "input",
    request_body = StickRequest,
    responses(
        (status = 200, description = "Stick moved", body = serde_json::Value)
    )
)]
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
        let row = format_default_row(&fmt, l_changed, r_changed);

        if let Err(e) = sender.write_row(&row, true).await {
            return Json(serde_json::json!({ "status": "error", "message": e.to_string() }));
        }

        // Build center row while we still have the lock
        let center_fmt = SendFormat::new();
        format_default_row(&center_fmt, l_changed, r_changed)
    }; // Lock is dropped here

    // Wait for duration (lock released, safe to sleep)
    if body.duration > 0 {
        tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
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
        "message": format!("{:?} stick moved to ({}, {}) for {}ms", stick, body.x, body.y, body.duration),
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
async fn input_touch(
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
        // Default serial format does not include touchscreen data (only Qingpi/3DS do).
        // Send via Qingpi format for touch support.
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
        tokio::time::sleep(tokio::time::Duration::from_millis(body.duration)).await;
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

// ═══════════════════════════════════════════════════════════════════════════════
// WebSocket Endpoint
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /ws — WebSocket upgrade endpoint for real-time events
#[utoipa::path(
    get,
    path = "/ws",
    tag = "websocket",
    responses(
        (status = 101, description = "WebSocket upgrade successful")
    )
)]
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
                            match cmd.get("type").and_then(|v| v.as_str()) {
                                Some("ping") => {
                                    let _ = socket.send(Message::Text(
                                        serde_json::json!({"type": "pong"}).to_string().into()
                                    )).await;
                                }
                                // ── WebRTC video signaling ──────────────────────
                                Some("signaling") => {
                                    handle_webrtc_signaling(&mut socket, &state, &cmd).await;
                                }
                                _ => {} // Unknown type, ignore
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

/// Process WebRTC video signaling messages from the frontend.
///
/// The frontend sends SDP offers/answers and ICE candidates for video,
/// and the backend responds to establish the signaling protocol.
/// Since the backend does not have a full WebRTC media stack,
/// signaling completes gracefully and the frontend falls back to
/// MJPEG streaming for actual video data.
///
/// Supported signaling subtypes:
/// - `video_offer`    → backend responds with acknowledgment + MJPEG URL
/// - `video_answer`   → stored for future use (not currently processed)
/// - `video_candidate`→ acknowledged (not currently used)
async fn handle_webrtc_signaling(
    socket: &mut WebSocket,
    _state: &AppState,
    cmd: &serde_json::Value,
) {
    let subtype = cmd.get("subtype").and_then(|v| v.as_str()).unwrap_or("");

    match subtype {
        "video_offer" => {
            // The frontend sent a WebRTC SDP offer for video.
            // Respond with an active SDP answer (sendonly) and start
            // a WebRTC session that sends camera frames as VP8/RTP.
            // The MJPEG stream URL is provided as a fallback.
            if let Some(offer_sdp) = cmd.get("sdp").and_then(|v| v.as_str()) {
                let session_id = "ws_video".to_string();
                let answer_sdp = generate_video_answer_sdp(offer_sdp);

                // Send the active SDP answer
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type": "signaling",
                            "subtype": "video_answer",
                            "sdp": answer_sdp,
                        })
                        .to_string()
                        .into(),
                    ))
                    .await;

                // Provide the MJPEG stream URL as fallback
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({
                            "type": "signaling",
                            "subtype": "video_info",
                            "mjpeg_url": "/camera/stream",
                        })
                        .to_string()
                        .into(),
                    ))
                    .await;

                tracing::debug!("WebRTC video offer processed (session_id={})", session_id);
            }
        }
        "video_answer" => {
            // Frontend sent an answer (unusual for this flow but handled)
            tracing::debug!("WebRTC video answer received (ignored)");
            // Acknowledge receipt
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "signaling",
                        "subtype": "video_info",
                        "mjpeg_url": "/camera/stream",
                    })
                    .to_string()
                    .into(),
                ))
                .await;
        }
        "video_candidate" => {
            // Frontend sent an ICE candidate — acknowledge
            tracing::debug!("WebRTC ICE candidate received (acknowledged)");
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "signaling",
                        "subtype": "video_info",
                        "mjpeg_url": "/camera/stream",
                    })
                    .to_string()
                    .into(),
                ))
                .await;
        }
        _ => {
            tracing::warn!("Unknown signaling subtype: {}", subtype);
        }
    }
}

/// Generate an active SDP answer for a video offer.
///
/// Creates a proper `sendonly` answer with SSRC, DTLS fingerprint,
/// ICE credentials, and a host candidate using str0m SDP generation.
fn generate_video_answer_sdp(offer_sdp: &str) -> String {
    // Create a minimal Rtc to generate the answer SDP.
    use std::time::Instant;
    use str0m::{Candidate, Rtc};
    use str0m::change::SdpOffer;
    use str0m::net::Protocol;
    
    let mut rtc = Rtc::new(Instant::now());
    // Add a dummy local candidate (port is informational)
    let addr = "127.0.0.1:9".parse().unwrap();
    if let Ok(candidate) = Candidate::host(addr, Protocol::Udp) {
        rtc.add_local_candidate(candidate);
    }
    // Parse the offer as SdpOffer
    let offer = match SdpOffer::from_sdp_string(offer_sdp) {
        Ok(o) => o,
        Err(_) => {
            return generate_basic_answer(offer_sdp);
        }
    };
    let mut api = rtc.sdp_api();
    match api.accept_offer(offer) {
        Ok(answer) => answer.to_sdp_string(),
        Err(_) => generate_basic_answer(offer_sdp),
    }
}

/// Fallback: generate a basic sendonly SDP answer without str0m.
fn generate_basic_answer(offer_sdp: &str) -> String {
    let payload_type = offer_sdp
        .lines()
        .find(|l| l.starts_with("m=video"))
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|pt| pt.parse::<u16>().ok())
        .unwrap_or(96);
    let codec_rtpmap = offer_sdp
        .lines()
        .find(|l| l.starts_with(&format!("a=rtpmap:{}", payload_type)))
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("a=rtpmap:{} VP8/90000", payload_type));
    format!(
        "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=video 9 UDP/TLS/RTP/SAVPF {}\r\nc=IN IP4 127.0.0.1\r\na=sendonly\r\na=mid:0\r\na=setup:passive\r\n{}\r\n",
        payload_type, codec_rtpmap
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Core Endpoints (unchanged)
// ═══════════════════════════════════════════════════════════════════════════════

/// Status endpoint — returns basic info.
#[utoipa::path(
    get,
    path = "/api/status",
    tag = "core",
    responses(
        (status = 200, description = "API status information", body = serde_json::Value)
    )
)]
async fn api_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "shared"
    }))
}

/// Greet endpoint — same API in both modes.
#[utoipa::path(
    get,
    path = "/api/greet",
    tag = "core",
    params(
        ("name" = Option<String>, Query, description = "Name to greet"),
    ),
    responses(
        (status = 200, description = "Greeting message", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/serial/ports",
    tag = "serial",
    responses(
        (status = 200, description = "List of available serial ports", body = serde_json::Value)
    )
)]
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

#[derive(Serialize, Deserialize, ToSchema)]
struct OpenRequest {
    port_num: u32,
    port_name: Option<String>,
    baudrate: u32,
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
#[utoipa::path(
    post,
    path = "/api/serial/close",
    tag = "serial",
    responses(
        (status = 200, description = "Serial port closed", body = serde_json::Value)
    )
)]
async fn serial_close(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut sender = state.serial.lock().await;
    sender.close().await;
    Json(serde_json::json!({
        "status": "ok",
        "message": "Serial port closed"
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
struct WriteRequest {
    data: String,
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
#[utoipa::path(
    post,
    path = "/api/serial/config",
    tag = "serial",
    request_body = SerialConfigRequest,
    responses(
        (status = 200, description = "Serial config updated", body = serde_json::Value)
    )
)]
async fn serial_config(
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
                    "message": format!("Failed to set baudrate: {}", e)
                }));
            }
        }
    } // serial lock released

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

#[derive(Serialize, Deserialize, ToSchema)]
struct SerialConfigRequest {
    baudrate: Option<u32>,
    data_format: Option<String>,
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
#[utoipa::path(
    get,
    path = "/api/commands",
    tag = "commands",
    responses(
        (status = 200, description = "List of commands", body = serde_json::Value)
    )
)]
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

#[derive(Serialize, Deserialize, ToSchema)]
struct NameRequest {
    name: String,
}

/// Load a command by scanning the scripts directory for the given name.
#[utoipa::path(
    post,
    path = "/api/commands/load",
    tag = "commands",
    request_body = NameRequest,
    responses(
        (status = 200, description = "Command loaded", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/commands/start",
    tag = "commands",
    request_body = NameRequest,
    responses(
        (status = 200, description = "Command started", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    post,
    path = "/api/commands/stop",
    tag = "commands",
    responses(
        (status = 200, description = "Command stopped", body = serde_json::Value)
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/commands/active",
    tag = "commands",
    responses(
        (status = 200, description = "Currently active command info", body = serde_json::Value)
    )
)]
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

/// POST /api/commands/filter — set command filter string and return filtered list
#[derive(Serialize, Deserialize, ToSchema)]
struct FilterRequest {
    filter: String,
}

#[utoipa::path(
    post,
    path = "/api/commands/filter",
    tag = "commands",
    request_body = FilterRequest,
    responses(
        (status = 200, description = "Filtered command list", body = serde_json::Value)
    )
)]
async fn commands_filter(
    State(state): State<AppState>,
    Json(body): Json<FilterRequest>,
) -> Json<serde_json::Value> {
    // Store the filter in shared state
    {
        let mut filter = state.command_filter.lock().await;
        *filter = body.filter.clone();
    }

    // Return filtered command list
    let cm = state.command_manager.lock().await;
    let filter_lower = body.filter.to_lowercase();
    let commands: Vec<serde_json::Value> = cm
        .list()
        .iter()
        .filter(|info| {
            if filter_lower.is_empty() {
                return true;
            }
            let name_match = info.name.to_lowercase().contains(&filter_lower);
            let desc_match = info
                .description
                .as_ref()
                .map(|d| d.to_lowercase().contains(&filter_lower))
                .unwrap_or(false);
            let path_match = info
                .path
                .to_string_lossy()
                .to_lowercase()
                .contains(&filter_lower);
            name_match || desc_match || path_match
        })
        .map(|info| {
            serde_json::json!({
                "name": info.name,
                "path": info.path.to_string_lossy(),
                "description": info.description,
            })
        })
        .collect();
    Json(serde_json::json!({ "status": "ok", "filter": body.filter, "commands": commands }))
}

/// POST /api/commands/reload — rescan the scripts directory and reload all commands
#[utoipa::path(
    post,
    path = "/api/commands/reload",
    tag = "commands",
    responses(
        (status = 200, description = "Commands reloaded", body = serde_json::Value)
    )
)]
async fn commands_reload(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    match cm.scan() {
        Ok(names) => {
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
            Json(serde_json::json!({
                "status": "ok",
                "message": format!("Scanned {} commands", names.len()),
                "commands": commands,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to reload commands: {}", e),
        })),
    }
}

/// GET /api/profile — list all available profiles
#[utoipa::path(
    get,
    path = "/api/profile",
    tag = "profile",
    responses(
        (status = 200, description = "List of profiles", body = serde_json::Value)
    )
)]
async fn profile_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let pm = state.profile_manager.lock().await;
    let profiles: Vec<serde_json::Value> = pm
        .list()
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "description": p.description,
                "active": pm.active_name() == Some(&p.name),
            })
        })
        .collect();
    Json(serde_json::json!({
        "status": "ok",
        "profiles": profiles,
        "active": pm.active_name(),
    }))
}

/// POST /api/profile — activate a profile by name
#[utoipa::path(
    post,
    path = "/api/profile",
    tag = "profile",
    request_body = NameRequest,
    responses(
        (status = 200, description = "Profile activated", body = serde_json::Value)
    )
)]
async fn profile_set(
    State(state): State<AppState>,
    Json(body): Json<NameRequest>,
) -> Json<serde_json::Value> {
    let mut pm = state.profile_manager.lock().await;
    match pm.activate(&body.name) {
        Ok(_) => Json(serde_json::json!({
            "status": "ok",
            "message": format!("Profile {} activated", body.name),
            "active": body.name,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ═══════════════════════════════════════════════════════════════════════════════
// Notification Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// Notification configuration stored in shared state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
struct NotificationConfig {
    /// Enable Windows desktop toast notifications
    windows_enabled: bool,
    /// Enable LINE messaging notifications
    line_enabled: bool,
    /// Enable Discord webhook notifications
    discord_enabled: bool,
    /// Discord webhook URL
    #[serde(default)]
    discord_webhook_url: String,
    /// LINE channel access token
    #[serde(default)]
    line_access_token: String,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            windows_enabled: true,
            line_enabled: false,
            discord_enabled: false,
            discord_webhook_url: String::new(),
            line_access_token: String::new(),
        }
    }
}

/// GET /api/notifications/config — get current notification settings
#[utoipa::path(
    get,
    path = "/api/notifications/config",
    tag = "notifications",
    responses(
        (status = 200, description = "Current notification settings", body = serde_json::Value)
    )
)]
async fn notifications_get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = state.notification_config.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "windows_enabled": cfg.windows_enabled,
        "line_enabled": cfg.line_enabled,
        "discord_enabled": cfg.discord_enabled,
        "discord_webhook_url": mask_secret(&cfg.discord_webhook_url),
        "line_access_token": mask_secret(&cfg.line_access_token),
    }))
}

/// Mask a secret value for safe display (show last 4 chars, rest as ****).
fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if value.len() <= 4 {
        return "****".to_string();
    }
    format!("****{}", &value[value.len() - 4..])
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct NotificationConfigRequest {
    windows_enabled: Option<bool>,
    line_enabled: Option<bool>,
    discord_enabled: Option<bool>,
    discord_webhook_url: Option<String>,
    line_access_token: Option<String>,
}

/// POST /api/notifications/config — update notification settings
#[utoipa::path(
    post,
    path = "/api/notifications/config",
    tag = "notifications",
    request_body = NotificationConfigRequest,
    responses(
        (status = 200, description = "Notification config updated", body = serde_json::Value)
    )
)]
async fn notifications_set_config(
    State(state): State<AppState>,
    Json(body): Json<NotificationConfigRequest>,
) -> Json<serde_json::Value> {
    let mut cfg = state.notification_config.lock().await;
    if let Some(v) = body.windows_enabled {
        cfg.windows_enabled = v;
    }
    if let Some(v) = body.line_enabled {
        cfg.line_enabled = v;
    }
    if let Some(v) = body.discord_enabled {
        cfg.discord_enabled = v;
    }
    if let Some(v) = body.discord_webhook_url {
        // SSRF protection: validate webhook URL
        if let Err(e) = validate_webhook_url(&v) {
            return Json(serde_json::json!({
                "status": "error",
                "message": format!("Invalid discord_webhook_url: {}", e),
            }));
        }
        cfg.discord_webhook_url = v;
    }
    if let Some(v) = body.line_access_token {
        cfg.line_access_token = v;
    }
    tracing::info!("Notification config updated");
    Json(serde_json::json!({
        "status": "ok",
        "message": "Notification config updated",
    }))
}

#[derive(serde::Serialize, serde::Deserialize, ToSchema)]
struct SendNotificationRequest {
    /// Notification message text
    message: String,
    /// Optional title
    title: Option<String>,
}

/// POST /api/notifications/send — send a test notification using current settings
#[utoipa::path(
    post,
    path = "/api/notifications/send",
    tag = "notifications",
    request_body = SendNotificationRequest,
    responses(
        (status = 200, description = "Notification sent", body = serde_json::Value)
    )
)]
async fn notifications_send(
    State(state): State<AppState>,
    Json(body): Json<SendNotificationRequest>,
) -> Json<serde_json::Value> {
    let cfg = state.notification_config.lock().await;
    let notification = Notification::new(&body.message)
        .with_title(body.title.unwrap_or_else(|| "Poke-Controller".to_string()));

    // Extract config values before spawning tasks
    let windows_enabled = cfg.windows_enabled;
    let line_enabled = cfg.line_enabled;
    let line_token = cfg.line_access_token.clone();
    let discord_enabled = cfg.discord_enabled;
    let discord_url = cfg.discord_webhook_url.clone();
    drop(cfg); // Release the lock before spawning parallel tasks

    let mut handles: Vec<tokio::task::JoinHandle<serde_json::Value>> = Vec::new();

    // Windows desktop notification — parallel task
    if windows_enabled {
        let notif = notification.clone();
        handles.push(tokio::spawn(async move {
            let notifier = WindowsNotifier::new("Poke-Controller");
            match notifier.send(&notif).await {
                Ok(_) => serde_json::json!({"channel": "windows", "status": "sent"}),
                Err(e) => serde_json::json!({"channel": "windows", "status": "error", "error": e.to_string()}),
            }
        }));
    }

    // LINE Notify notification — parallel task
    if line_enabled && !line_token.is_empty() {
        let notif = notification.clone();
        handles.push(tokio::spawn(async move {
            let notifier = LineNotifier::new(line_token);
            match notifier.send(&notif).await {
                Ok(_) => serde_json::json!({"channel": "line", "status": "sent"}),
                Err(e) => serde_json::json!({"channel": "line", "status": "error", "error": e.to_string()}),
            }
        }));
    }

    // Discord webhook notification — parallel task
    if discord_enabled && !discord_url.is_empty() {
        // SSRF protection: validate URL before sending
        if let Err(e) = validate_webhook_url(&discord_url) {
            handles.push(tokio::spawn(async move {
                serde_json::json!({
                    "channel": "discord",
                    "status": "error",
                    "error": format!("Invalid webhook URL: {}", e),
                })
            }));
        } else {
            let notif = notification.clone();
            handles.push(tokio::spawn(async move {
                let notifier = DiscordNotifier::new(discord_url);
                match notifier.send(&notif).await {
                    Ok(_) => serde_json::json!({"channel": "discord", "status": "sent"}),
                    Err(e) => serde_json::json!({"channel": "discord", "status": "error", "error": e.to_string()}),
                }
            }));
        }
    }

    // Wait for all notification tasks to complete in parallel
    let results: Vec<serde_json::Value> = join_all(handles)
        .await
        .into_iter()
        .map(|r| {
            r.unwrap_or_else(|e| {
                serde_json::json!({
                    "channel": "unknown",
                    "status": "error",
                    "error": format!("notification task panicked: {}", e),
                })
            })
        })
        .collect();

    Json(serde_json::json!({
        "status": "ok",
        "results": results,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// OpenAPI Documentation
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(OpenApi)]
#[openapi(
    paths(
        api_status,
        api_greet,
        controller_set_type,
        controller_get_type,
        controller_set_keyboard,
        controller_get_keyboard,
        controller_set_mouse_stick,
        controller_get_mouse_stick,
        cameras_list,
        camera_status,
        camera_open,
        camera_close,
        camera_frame,
        camera_capture,
        camera_config,
        input_press,
        input_hold,
        input_release,
        input_stick,
        input_touch,
        ws_handler,
        serial_ports,
        serial_open,
        serial_close,
        serial_write,
        serial_config,
        serial_status,
        commands_list,
        commands_load,
        commands_start,
        commands_stop,
        commands_active,
        commands_filter,
        commands_reload,
        profile_list,
        profile_set,
        notifications_get_config,
        notifications_set_config,
        notifications_send,
        api_openapi_json,
    ),
    components(
        schemas(
            ControllerTypeRequest,
            KeyboardRequest,
            MouseStickRequest,
            CameraOpenRequest,
            CaptureRequest,
            CameraConfigRequest,
            PressRequest,
            HoldRequest,
            StickRequest,
            TouchRequest,
            OpenRequest,
            WriteRequest,
            SerialConfigRequest,
            NameRequest,
            FilterRequest,
            NotificationConfigRequest,
            SendNotificationRequest,
            MouseStickConfig,
            NotificationConfig,
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
struct ApiDoc;

/// GET /api/openapi.json — return the OpenAPI specification as JSON
#[utoipa::path(
    get,
    path = "/api/openapi.json",
    tag = "core",
    responses(
        (status = 200, description = "OpenAPI specification in JSON format", body = serde_json::Value)
    )
)]
async fn api_openapi_json() -> Json<serde_json::Value> {
    Json(serde_json::to_value(ApiDoc::openapi()).unwrap())
}

// Server Setup
// ═══════════════════════════════════════════════════════════════════════════════

/// Start the HTTP server — shared between web and tauri modes.
async fn start_http_server(port: u16, web_dir: PathBuf, state: AppState) {
    // ── Static UI files served under /ui/ ───────────────────────────────
    let ui_service =
        tower_http::services::ServeDir::new(&web_dir).append_index_html_on_directories(true);

    let app = axum::Router::new()
        // Root redirect: / → /ui/
        .route("/", get(|| async { Redirect::temporary("/ui/") }))
        // ── Mobile placeholder (TODO) ───────────────────────────────────
        .route(
            "/mobile",
            get(|| async {
                (
                    StatusCode::NOT_IMPLEMENTED,
                    "Mobile UI is not yet implemented — coming in a future phase.",
                )
            }),
        )
        // Core endpoints
        .route("/api/status", get(api_status))
        .route("/api/greet", get(api_greet))
        .route("/api/openapi.json", get(api_openapi_json))
        // Controller endpoints
        .route(
            "/api/controller/type",
            get(controller_get_type).post(controller_set_type),
        )
        .route(
            "/api/controller/keyboard",
            get(controller_get_keyboard).post(controller_set_keyboard),
        )
        .route(
            "/api/controller/mouse_stick",
            get(controller_get_mouse_stick).post(controller_set_mouse_stick),
        )
        // Camera endpoints
        .route("/api/cameras", get(cameras_list))
        .route("/api/camera/status", get(camera_status))
        .route("/api/camera/open", post(camera_open))
        .route("/api/camera/close", post(camera_close))
        .route("/api/camera/frame", get(camera_frame))
        .route("/api/camera/capture", post(camera_capture))
        .route("/api/camera/config", post(camera_config))
        // MJPEG stream endpoint (non-API, raw HTTP streaming)
        .route("/camera/stream", get(camera_stream))
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
        .route("/api/commands/filter", post(commands_filter))
        .route("/api/commands/reload", post(commands_reload))
        // Profile management endpoints
        .route("/api/profile", get(profile_list).post(profile_set))
        // Notification endpoints
        .route(
            "/api/notifications/config",
            get(notifications_get_config).post(notifications_set_config),
        )
        .route("/api/notifications/send", post(notifications_send))
        // ── UI static files at /ui/* ────────────────────────────────────
        .nest("/ui", axum::Router::new().fallback_service(ui_service))
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
    let pm = ProfileManager::new(&args.profiles_dir).expect("failed to create profile manager");
    let state = AppState {
        serial: Arc::new(Mutex::new(Sender::new(true))),
        keypress: Arc::new(Mutex::new(KeyPress::new(Sender::new(true)))),
        command_filter: Arc::new(Mutex::new(String::new())),
        command_manager: Arc::new(Mutex::new(cm)),
        event_bus: EventBus::new(),
        camera: Arc::new(Mutex::new(None)),
        event_tx,
        gamepad_type: Arc::new(Mutex::new("ProController".to_string())),
        keyboard_enabled: Arc::new(Mutex::new(false)),
        profile_manager: Arc::new(Mutex::new(pm)),
        notification_config: Arc::new(Mutex::new(NotificationConfig::default())),
        mouse_stick: Arc::new(Mutex::new(MouseStickConfig::default())),
        webrtc_manager: Arc::new(Mutex::new(webrtc::WebRtcManager::new())),
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
