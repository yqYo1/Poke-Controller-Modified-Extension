fn main() {
    println!("cargo:rerun-if-env-changed=POKECON_BUILD_PYTHON");
    if std::env::var_os("CARGO_FEATURE_TAURI_SHELL").is_some() {
        let manifest = tauri_build::AppManifest::new()
            .commands(&["choose_save_path", "open_config_directory"]);
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
            .expect("Tauri application metadata must be valid");
    }
}
