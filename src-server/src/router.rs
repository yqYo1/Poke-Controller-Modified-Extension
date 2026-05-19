use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use tower::service_fn;

use crate::handlers::camera::{
    camera_capture, camera_close, camera_config, camera_frame, camera_open, camera_status,
    camera_stream, cameras_list,
};
use crate::handlers::commands::{
    commands_active, commands_filter, commands_list, commands_load, commands_reload,
    commands_start, commands_stop,
};
use crate::handlers::controller::{
    controller_get_keyboard, controller_get_mouse_stick, controller_get_type,
    controller_set_keyboard, controller_set_mouse_stick, controller_set_type,
};
use crate::handlers::core::{api_greet, api_openapi_json, api_status};
use crate::handlers::input::{input_hold, input_press, input_release, input_stick, input_touch};
use crate::handlers::notifications::{
    notifications_get_config, notifications_send, notifications_set_config,
};
use crate::handlers::profile::{profile_list, profile_set};
use crate::handlers::serial::{
    serial_close, serial_config, serial_open, serial_ports, serial_status, serial_write,
};
use crate::handlers::websocket::ws_handler;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Server Setup
// ═══════════════════════════════════════════════════════════════════════════════

/// Start the HTTP server — shared between web and tauri modes.
pub async fn start_http_server(port: u16, web_dir: PathBuf, state: AppState) {
    // ── Static UI files served under /ui/* ──────────────────────────────
    let web_dir = Arc::new(web_dir);
    let index_path = web_dir.join("index.html");

    // Read index.html once at startup and cache it for SPA fallback responses.
    // This avoids a filesystem read on every client-side route navigation.
    let index_html = match tokio::fs::read_to_string(&index_path).await {
        Ok(html) => {
            tracing::info!(
                "SPA fallback loaded: {} ({} bytes)",
                index_path.display(),
                html.len()
            );
            Some(html)
        }
        Err(e) => {
            tracing::error!(
                "SPA fallback FAILED to load {}: {e}",
                index_path.display()
            );
            None
        }
    };

    let ui_service = tower_http::services::ServeDir::new(web_dir.as_ref())
        .append_index_html_on_directories(true)
        .fallback(service_fn(move |req: axum::extract::Request| {
            let index_html = index_html.clone();
            async move {
                let path = req.uri().path();
                let stripped = path.trim_start_matches('/');

                // ── Determine if this is a request for an actual file ──
                // The fallback is invoked when ServeDir cannot find the file.
                // If the path has a file extension, the file genuinely doesn't
                // exist (e.g. a stale asset reference) — return 404.
                // If the path has NO extension, it's a client-side SPA route —
                // serve index.html so the SPA router can handle it.
                let is_likely_file =
                    stripped.contains('.') && !stripped.ends_with('/');

                if is_likely_file {
                    tracing::debug!(
                        "ServeDir fallback — returning 404 for missing file: {path}"
                    );
                    return Ok::<_, std::convert::Infallible>(
                        StatusCode::NOT_FOUND.into_response(),
                    );
                }

                // ── Client-side SPA route — serve fallback index.html ──
                match &index_html {
                    Some(html) => {
                        tracing::info!(
                            "SPA fallback — serving index.html for client route: {path}"
                        );
                        Ok::<_, std::convert::Infallible>(
                            (
                                StatusCode::OK,
                                [(axum::http::header::CONTENT_TYPE, "text/html")],
                                html.clone(),
                            )
                                .into_response(),
                        )
                    }
                    None => {
                        tracing::error!(
                            "SPA fallback — index.html unavailable for route: {path}"
                        );
                        Ok::<_, std::convert::Infallible>(
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "SPA fallback not available — index.html was not loaded at startup. \
                                 Ensure the web frontend is built before starting the server. \
                                 Run: cd web && npm run build"
                                    .to_string(),
                            )
                                .into_response(),
                        )
                    }
                }
            }
        }));

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
        .nest_service("/ui", ui_service)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("HTTP server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app).await.expect("server error");
}
