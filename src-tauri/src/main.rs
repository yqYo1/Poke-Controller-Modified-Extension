use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

/// Poke-Controller Modified Extension — Tauri/Web UI
#[derive(Parser, Debug)]
#[command(name = "pokecon", version, about)]
struct Args {
    /// UI mode: "tauri" (native window) or "web" (HTTP server)
    #[arg(long = "ui", default_value = "tauri")]
    ui: String,

    /// Port for web UI mode (only used when --ui=web)
    #[arg(long, default_value = "8020")]
    port: u16,

    /// Static files directory for web UI mode
    #[arg(long = "web-dir", default_value = "web")]
    web_dir: PathBuf,
}

fn main() {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    match args.ui.as_str() {
        "web" => {
            tracing::info!("Starting web UI server on port {}", args.port);
            start_web_server(args.port, args.web_dir);
        }
        _ => {
            tracing::info!("Starting Tauri native UI");
            start_tauri();
        }
    }
}

/// Start the web UI mode — serves the web frontend via axum HTTP server.
fn start_web_server(port: u16, web_dir: PathBuf) {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let app = axum::Router::new()
            // Serve static files from the web directory
            .nest_service(
                "/",
                tower_http::services::ServeDir::new(&web_dir)
                    .append_index_html_on_directories(true),
            )
            // Placeholder for future API endpoints
            .route("/api/status", axum::routing::get(api_status));

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        tracing::info!("Web UI listening on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("failed to bind address");
        axum::serve(listener, app).await.expect("server error");
    });
}

/// Placeholder API endpoint — returns basic status info.
async fn api_status() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "web"
    }))
}

/// Start the Tauri native window mode.
fn start_tauri() {
    tauri::Builder::default()
        .setup(|_app| {
            tracing::debug!("Tauri app setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet,])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Simple greet command — placeholder for IPC communication.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Poke-Controller.", name)
}
