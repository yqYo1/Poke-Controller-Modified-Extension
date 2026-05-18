use std::path::PathBuf;
use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use futures::stream;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[cfg(feature = "v4l")]
use pokecon_core::cv::backends::{V4lCameraBackend, list_cameras as v4l_list_cameras};
#[cfg(not(feature = "v4l"))]
use pokecon_core::cv::camera::MockCameraBackend;
use pokecon_core::cv::camera::{Camera, CameraConfig, FlipMode};

use crate::helpers;
use crate::state::AppState;

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
pub async fn cameras_list() -> Json<serde_json::Value> {
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
pub async fn camera_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => {
            let cfg = camera.config();
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
pub struct CameraOpenRequest {
    pub device_index: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
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
pub async fn camera_open(
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
pub async fn camera_close(State(state): State<AppState>) -> Json<serde_json::Value> {
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
pub async fn camera_frame(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => match camera.capture().await {
            Ok(frame) => match helpers::frame_to_base64_jpeg(&frame) {
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
pub async fn camera_stream(State(state): State<AppState>) -> Response {
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
                    Ok(frame) => helpers::frame_to_jpeg_bytes(&frame).ok(),
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
pub struct CaptureRequest {
    pub filename: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CameraConfigRequest {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub flip: Option<String>,
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
pub async fn camera_capture(
    State(state): State<AppState>,
    Json(body): Json<CaptureRequest>,
) -> Json<serde_json::Value> {
    let cam = state.camera.lock().await;
    match cam.as_ref() {
        Some(camera) => match camera.capture().await {
            Ok(frame) => {
                // SECURITY: Sanitize filename to prevent path traversal
                let safe_filename = helpers::sanitize_filename(&body.filename);
                let path = PathBuf::from("Captures").join(&safe_filename);
                match helpers::save_frame_as_jpeg(&frame, &path) {
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
pub async fn camera_config(
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
                        "message": format!("Invalid width: {width}. Must be between 1 and 4096."),
                    }));
                }
                config.width = width;
            }
            if let Some(height) = body.height {
                if height == 0 || height > 4096 {
                    return Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Invalid height: {height}. Must be between 1 and 4096."),
                    }));
                }
                config.height = height;
            }
            if let Some(fps) = body.fps {
                if fps == 0 || fps > 120 {
                    return Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Invalid fps: {fps}. Must be between 1 and 120."),
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
