/// Start the Tauri native window — loads the same web UI via WebView.
pub fn start_tauri(port: u16) {
    tauri::Builder::default()
        .setup(move |app| {
            let _window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(
                    format!("http://127.0.0.1:{port}/ui/").parse().unwrap(),
                ),
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
