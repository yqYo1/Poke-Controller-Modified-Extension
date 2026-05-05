use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::routing::{get, post};
use clap::Parser;
use serde::Deserialize;
use tokio::sync::Mutex;

use pokecon_core::command_manager::CommandManager;
use pokecon_events::EventBus;
use pokecon_serial::Sender;

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
    command_manager: Arc<Mutex<CommandManager>>,
    event_bus: EventBus,
}

fn main() {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Create shared application state
    let cm = CommandManager::new(&args.scripts_dir).expect("failed to create command manager");
    let state = AppState {
        serial: Arc::new(Mutex::new(Sender::new(true))),
        command_manager: Arc::new(Mutex::new(cm)),
        event_bus: EventBus::new(),
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

/// Start the HTTP server — shared between web and tauri modes.
async fn start_http_server(port: u16, web_dir: PathBuf, state: AppState) {
    let app = axum::Router::new()
        // Core endpoints
        .route("/api/status", get(api_status))
        .route("/api/greet", get(api_greet))
        // Serial port endpoints
        .route("/api/serial/ports", get(serial_ports))
        .route("/api/serial/open", post(serial_open))
        .route("/api/serial/close", post(serial_close))
        .route("/api/serial/write", post(serial_write))
        .route("/api/serial/status", get(serial_status))
        // Command management endpoints
        .route("/api/commands", get(commands_list))
        .route("/api/commands/load", post(commands_load))
        .route("/api/commands/start", post(commands_start))
        .route("/api/commands/stop", post(commands_stop))
        .route("/api/commands/active", get(commands_active))
        .with_state(state)
        .nest_service(
            "/",
            tower_http::services::ServeDir::new(&web_dir).append_index_html_on_directories(true),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("HTTP server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app).await.expect("server error");
}

// ── Core Endpoints ──────────────────────────────────────────────────────────────

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

// ── Serial Port Endpoints ───────────────────────────────────────────────────────

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

/// Get serial connection status.
async fn serial_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sender = state.serial.lock().await;
    Json(serde_json::json!({
        "is_open": sender.is_opened()
    }))
}

// ── Command Management Endpoints ────────────────────────────────────────────────

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
        Ok(_) => Json(serde_json::json!({
            "status": "ok",
            "message": format!("Command '{}' started", body.name)
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string()
        })),
    }
}

/// Stop the currently active command.
async fn commands_stop(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    let name = cm.active_name().map(|s| s.to_string());
    cm.stop();
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

// ── Tauri ───────────────────────────────────────────────────────────────────────

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
