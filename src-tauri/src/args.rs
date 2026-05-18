use clap::Parser;
use std::path::PathBuf;

/// Poke-Controller Modified Extension — Tauri/Web UI
#[derive(Parser, Debug)]
#[command(name = "pokecon", version, about)]
pub struct Args {
    /// UI mode: "tauri" (native window) or "web" (HTTP server)
    #[arg(long = "ui", default_value = "tauri")]
    pub ui: String,

    /// Port for HTTP server (used in both web and tauri modes)
    #[arg(long, default_value = "8020")]
    pub port: u16,

    /// Static files directory
    #[arg(long = "web-dir", default_value = "web/dist")]
    pub web_dir: PathBuf,

    /// Scripts directory for command manager
    #[arg(long = "scripts-dir", default_value = "scripts")]
    pub scripts_dir: PathBuf,

    /// Profiles directory for profile manager
    #[arg(long = "profiles-dir", default_value = "profiles")]
    pub profiles_dir: PathBuf,
}
