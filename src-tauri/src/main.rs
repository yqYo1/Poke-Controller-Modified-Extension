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

    /// Port for HTTP server (used in both web and tauri modes)
    #[arg(long, default_value = "8020")]
    port: u16,

    /// Static files directory
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

    // Start the HTTP server in both modes — Tauri embeds it internally
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let server_handle = rt.spawn(start_http_server(args.port, args.web_dir.clone()));

    match args.ui.as_str() {
        "web" => {
            tracing::info!("Web UI mode — serving at http://127.0.0.1:{}", args.port);
            rt.block_on(server_handle).expect("server task failed");
        }
        _ => {
            tracing::info!("Tauri UI mode — HTTP server at http://127.0.0.1:{}", args.port);
            start_tauri(args.port);
        }
    }
}

/// Start the HTTP server — shared between web and tauri modes.
async fn start_http_server(port: u16, web_dir: PathBuf) {
    let app = axum::Router::new()
        .route("/api/status", axum::routing::get(api_status))
        .route("/api/greet", axum::routing::get(api_greet))
        .nest_service(
            "/",
            tower_http::services::ServeDir::new(&web_dir)
                .append_index_html_on_directories(true),
        );

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("HTTP server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app).await.expect("server error");
}

/// Status endpoint — returns basic info.
async fn api_status() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "shared"
    }))
}

/// Greet endpoint — same API in both modes.
async fn api_greet(axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>) -> axum::Json<serde_json::Value> {
    let name = params.get("name").map(|s| s.as_str()).unwrap_or("Trainer");
    axum::Json(serde_json::json!({
        "message": format!("Hello, {}! Welcome to Poke-Controller.", name)
    }))
}

/// Start the Tauri native window — loads the same web UI via WebView.
fn start_tauri(port: u16) {
    tauri::Builder::default()
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(
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
