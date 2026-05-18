use clap::Parser;
use pokecon_core::settings::{DEFAULT_PROFILES_DIR, DEFAULT_SCRIPTS_DIR};
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
    #[arg(long = "scripts-dir", default_value = DEFAULT_SCRIPTS_DIR)]
    pub scripts_dir: PathBuf,

    /// Profiles directory for profile manager
    #[arg(long = "profiles-dir", default_value = DEFAULT_PROFILES_DIR)]
    pub profiles_dir: PathBuf,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_default_ui() {
        let args = Args::parse_from(&["pokecon"]);
        assert_eq!(args.ui, "tauri");
    }

    #[test]
    fn test_args_default_port() {
        let args = Args::parse_from(&["pokecon"]);
        assert_eq!(args.port, 8020);
    }

    #[test]
    fn test_args_custom_port() {
        let args = Args::parse_from(&["pokecon", "--port", "9090"]);
        assert_eq!(args.port, 9090);
    }

    #[test]
    fn test_args_web_mode() {
        let args = Args::parse_from(&["pokecon", "--ui", "web"]);
        assert_eq!(args.ui, "web");
    }

    #[test]
    fn test_args_custom_web_dir() {
        let args = Args::parse_from(&["pokecon", "--web-dir", "custom/dist"]);
        assert_eq!(args.web_dir, PathBuf::from("custom/dist"));
    }

    #[test]
    fn test_args_debug_format() {
        let args = Args::parse_from(&["pokecon", "--port", "1234"]);
        let debug = format!("{:?}", args);
        assert!(debug.contains("1234"));
    }
}
