fn main() {
    // Skip tauri build when running in nix sandbox or CI
    if std::env::var("TAURI_SKIP_BUILD").is_ok() {
        return;
    }
    tauri_build::build()
}
