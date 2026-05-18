mod args;
mod helpers;
mod router;
mod state;
mod tauri;

#[cfg(feature = "vaapi")]
mod vaapi_encoder;
mod webrtc;

mod api_doc;
mod handlers;

use std::sync::Arc;

use clap::Parser;
use pokecon_core::command_manager::CommandManager;
use pokecon_core::events::EventBus;
use pokecon_core::profile::ProfileManager;
use pokecon_core::serial::keypress::KeyPress;
use pokecon_core::serial::sender::Sender;
use pokecon_core::settings::Settings;
use tokio::sync::Mutex;

use args::Args;
use state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Entry Point
// ═══════════════════════════════════════════════════════════════════════════════

fn main() {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load settings from config file, falling back to defaults gracefully
    let settings = match Settings::load_default() {
        Ok(s) => {
            tracing::info!("Loaded settings from config file");
            s
        }
        Err(e) => {
            tracing::warn!("Could not load settings: {e}. Using defaults.");
            Settings::default()
        }
    };

    // Merge: CLI args take precedence over config file settings.
    // If a CLI arg is not provided (None), fall back to config file or default.
    let scripts_dir = args.scripts_dir.clone().unwrap_or(settings.script_dir);

    let profiles_dir = args.profiles_dir.clone().unwrap_or(settings.profile_dir);

    // Create broadcast channel for WebSocket event forwarding
    let (event_tx, _) = tokio::sync::broadcast::channel::<serde_json::Value>(256);

    // Create shared application state
    let cm = CommandManager::new(&scripts_dir).expect("failed to create command manager");
    let pm = ProfileManager::new(&profiles_dir).expect("failed to create profile manager");
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
        notification_config: Arc::new(Mutex::new(settings.notify)),
        mouse_stick: Arc::new(Mutex::new(Default::default())),
        webrtc_manager: Arc::new(Mutex::new(webrtc::WebRtcManager::new())),
    };

    // Start the HTTP server in both modes — Tauri embeds it internally
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let server_handle = rt.spawn(router::start_http_server(
        args.port,
        args.web_dir.clone(),
        state,
    ));

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
            tauri::start_tauri(args.port);
        }
    }
}
