//! Native desktop shell and close-policy boundary for the shared Rust backend.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use pokecon_core::{ShutdownCoordinator, ShutdownReason};
use thiserror::Error;

/// Canonical behavior for a user request to close the last desktop window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CloseBehavior {
    /// Ask whether to keep the backend, shut down, or cancel.
    #[default]
    Ask,
    /// Shut down the complete process without asking.
    Shutdown,
    /// Destroy only the window and retain the backend and tray.
    KeepBackend,
}

impl FromStr for CloseBehavior {
    type Err = DesktopError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ask" => Ok(Self::Ask),
            "shutdown" => Ok(Self::Shutdown),
            "keep_backend" => Ok(Self::KeepBackend),
            _ => Err(DesktopError::InvalidCloseBehavior(value.to_owned())),
        }
    }
}

/// Result selected by the last-window close policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseDecision {
    /// Keep the backend and destroy the desktop window.
    KeepBackend,
    /// Request the common complete shutdown path.
    Shutdown,
    /// Keep the window and all runtime state unchanged.
    Cancel,
}

/// Thread-safe desktop settings that can change after the native window has
/// been created.
#[derive(Clone, Debug)]
pub struct DesktopRuntimeSettings {
    close_behavior: Arc<AtomicU8>,
}

impl DesktopRuntimeSettings {
    /// Creates the live desktop settings projection.
    #[must_use]
    pub fn new(close_behavior: CloseBehavior) -> Self {
        Self {
            close_behavior: Arc::new(AtomicU8::new(close_behavior as u8)),
        }
    }

    /// Returns the close policy used by the next last-window close request.
    #[must_use]
    pub fn close_behavior(&self) -> CloseBehavior {
        match self.close_behavior.load(Ordering::Acquire) {
            1 => CloseBehavior::Shutdown,
            2 => CloseBehavior::KeepBackend,
            _ => CloseBehavior::Ask,
        }
    }

    /// Atomically replaces the close policy for subsequent window events.
    pub fn set_close_behavior(&self, close_behavior: CloseBehavior) {
        self.close_behavior
            .store(close_behavior as u8, Ordering::Release);
    }
}

/// Immutable values required by the native shell.
#[derive(Clone, Debug)]
pub struct DesktopShellConfig {
    /// HTTP URL served by the colocated axum backend.
    pub app_url: String,
    /// Canonical configuration root exposed only to the local Tauri command.
    pub config_directory: PathBuf,
    /// Live desktop settings projection.
    pub runtime_settings: DesktopRuntimeSettings,
    /// Whether startup disabled `WebView` compositing.
    pub disable_compositing: bool,
}

/// Failure while validating or running the native desktop shell.
#[derive(Debug, Error)]
pub enum DesktopError {
    /// The canonical close behavior was not one of the closed enum values.
    #[error("invalid desktop close behavior: {0}")]
    InvalidCloseBehavior(String),
    /// The configured backend URL could not be used by a `WebView`.
    #[error("invalid desktop application URL: {0}")]
    InvalidAppUrl(String),
    /// The primary desktop instance could not start its colocated backend.
    #[error("desktop backend startup failed: {0}")]
    BackendStartup(String),
    /// Tauri could not initialize or run the native shell.
    #[cfg(feature = "tauri-shell")]
    #[error("Tauri desktop shell failed: {0}")]
    Tauri(#[from] tauri::Error),
}

/// Connects desktop requests to the same process-wide shutdown coordinator as
/// operating-system signals and fatal backend errors.
#[derive(Clone, Debug)]
pub struct DesktopLifecycle {
    shutdown: ShutdownCoordinator,
}

impl DesktopLifecycle {
    /// Creates a desktop lifecycle adapter.
    #[must_use]
    pub const fn new(shutdown: ShutdownCoordinator) -> Self {
        Self { shutdown }
    }

    /// Requests the complete graceful shutdown path.
    pub fn request_exit(&self) -> bool {
        self.shutdown.request(ShutdownReason::DesktopExit)
    }

    /// Returns the shared coordinator used by shell monitors.
    #[must_use]
    pub fn coordinator(&self) -> ShutdownCoordinator {
        self.shutdown.clone()
    }
}

/// Resolves close policy without mutating application state. A missing result
/// means that the confirmation surface failed and therefore fails safe to a
/// complete shutdown.
#[must_use]
pub const fn close_decision(
    behavior: CloseBehavior,
    confirmation: Option<CloseDecision>,
) -> CloseDecision {
    match behavior {
        CloseBehavior::Ask => match confirmation {
            Some(decision) => decision,
            None => CloseDecision::Shutdown,
        },
        CloseBehavior::Shutdown => CloseDecision::Shutdown,
        CloseBehavior::KeepBackend => CloseDecision::KeepBackend,
    }
}

#[cfg(feature = "tauri-shell")]
mod shell {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    use tauri::image::Image;
    use tauri::ipc::CapabilityBuilder;
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    use tauri::{Manager as _, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

    use super::{
        CloseBehavior, CloseDecision, DesktopError, DesktopLifecycle, DesktopShellConfig,
        close_decision,
    };

    const MAIN_WINDOW_LABEL: &str = "main";

    struct ShellState {
        background: AtomicBool,
        config: DesktopShellConfig,
        lifecycle: DesktopLifecycle,
    }

    fn validate_suggested_name(value: &str) -> Result<(), String> {
        if value.is_empty()
            || value.chars().any(char::is_control)
            || value.contains('/')
            || value.contains('\\')
            || matches!(value, "." | "..")
        {
            Err("suggested filename is invalid".to_owned())
        } else {
            Ok(())
        }
    }

    #[tauri::command]
    async fn choose_save_path(
        suggested_name: String,
        extension: String,
    ) -> Result<Option<String>, String> {
        validate_suggested_name(&suggested_name)?;
        let extension = match extension.as_str() {
            "png" => "png",
            "jpeg" | "jpg" => "jpg",
            "bat" => "bat",
            _ => return Err("save extension is not allowed".to_owned()),
        };
        tauri::async_runtime::spawn_blocking(move || {
            FileDialog::new()
                .set_file_name(&suggested_name)
                .add_filter(extension.to_ascii_uppercase(), &[extension])
                .save_file()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .await
        .map_err(|error| format!("native save dialog failed: {error}"))
    }

    #[tauri::command]
    async fn open_config_directory(state: State<'_, Arc<ShellState>>) -> Result<(), String> {
        let directory = state.config.config_directory.clone();
        tauri::async_runtime::spawn_blocking(move || opener::open(directory))
            .await
            .map_err(|error| format!("config directory task failed: {error}"))?
            .map_err(|error| format!("config directory could not be opened: {error}"))
    }

    fn open_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), DesktopError> {
        if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
            app.state::<Arc<ShellState>>()
                .background
                .store(false, Ordering::Release);
            window.show()?;
            window.set_focus()?;
            return Ok(());
        }
        let state = app.state::<Arc<ShellState>>();
        let url = tauri::Url::parse(&state.config.app_url)
            .map_err(|_error| DesktopError::InvalidAppUrl(state.config.app_url.clone()))?;
        let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::External(url))
            .title("PokeCon Controller")
            .inner_size(1440.0, 900.0)
            .min_inner_size(960.0, 640.0);
        #[cfg(target_os = "windows")]
        let builder = if state.config.disable_compositing {
            builder.additional_browser_args("--disable-gpu-compositing")
        } else {
            builder
        };
        builder.build()?;
        state.background.store(false, Ordering::Release);
        Ok(())
    }

    fn ask_close() -> Option<CloseDecision> {
        let result = MessageDialog::new()
            .set_level(MessageLevel::Info)
            .set_title("Close PokeCon Controller")
            .set_description(
                "Yes: shut down everything\nNo: keep the backend running\nCancel: keep the window open",
            )
            .set_buttons(MessageButtons::YesNoCancel)
            .show();
        match result {
            MessageDialogResult::Yes => Some(CloseDecision::Shutdown),
            MessageDialogResult::No => Some(CloseDecision::KeepBackend),
            MessageDialogResult::Cancel => Some(CloseDecision::Cancel),
            MessageDialogResult::Ok | MessageDialogResult::Custom(_) => None,
        }
    }

    fn tray_icon() -> Image<'static> {
        let mut pixels = Vec::with_capacity(16 * 16 * 4);
        for y in 0..16 {
            for x in 0..16 {
                let highlighted = (3..=12).contains(&x) && (3..=12).contains(&y);
                pixels.extend_from_slice(if highlighted {
                    &[34, 211, 238, 255]
                } else {
                    &[8, 11, 18, 255]
                });
            }
        }
        Image::new_owned(pixels, 16, 16)
    }

    /// Runs the native `WebView` and tray event loop on the calling thread.
    pub(super) fn run<F>(
        context: tauri::Context<tauri::Wry>,
        config: DesktopShellConfig,
        lifecycle: &DesktopLifecycle,
        on_primary_instance: F,
    ) -> Result<(), DesktopError>
    where
        F: FnOnce(PathBuf) -> Result<(), DesktopError> + Send + 'static,
    {
        tauri::Url::parse(&config.app_url)
            .map_err(|_error| DesktopError::InvalidAppUrl(config.app_url.clone()))?;
        let state = Arc::new(ShellState {
            background: AtomicBool::new(false),
            config,
            lifecycle: lifecycle.clone(),
        });
        let single_instance_state = Arc::clone(&state);
        let setup_state = Arc::clone(&state);
        let builder = tauri::Builder::default()
            .manage(Arc::clone(&state))
            .plugin(tauri_plugin_single_instance::init(
                move |app, _arguments, _cwd| {
                    let _keep_state_alive = &single_instance_state;
                    if let Err(error) = open_main_window(app) {
                        single_instance_state
                            .lifecycle
                            .coordinator()
                            .request(pokecon_core::ShutdownReason::FatalError(error.to_string()));
                    }
                },
            ))
            .invoke_handler(tauri::generate_handler![
                choose_save_path,
                open_config_directory
            ])
            .setup(move |app| {
                let remote = format!("{}/*", setup_state.config.app_url.trim_end_matches('/'));
                app.add_capability(
                    CapabilityBuilder::new("pokecon-local-backend")
                        .local(false)
                        .remote(remote)
                        .window(MAIN_WINDOW_LABEL)
                        .permission("allow-choose-save-path")
                        .permission("allow-open-config-directory"),
                )?;
                on_primary_instance(app.path().resource_dir()?)?;
                open_main_window(app.handle())?;
                let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &quit])?;
                TrayIconBuilder::new()
                    .icon(tray_icon())
                    .menu(&menu)
                    .show_menu_on_left_click(true)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "open" => {
                            if let Err(error) = open_main_window(app) {
                                app.state::<Arc<ShellState>>()
                                    .lifecycle
                                    .coordinator()
                                    .request(pokecon_core::ShutdownReason::FatalError(
                                        error.to_string(),
                                    ));
                            }
                        }
                        "quit" => {
                            app.state::<Arc<ShellState>>().lifecycle.request_exit();
                        }
                        _ => {}
                    })
                    .build(app)?;

                let app_handle = app.handle().clone();
                let shutdown = setup_state.lifecycle.coordinator();
                tauri::async_runtime::spawn(async move {
                    let _reason = shutdown.cancelled().await;
                    app_handle.exit(0);
                });
                Ok(())
            })
            .on_window_event(|window, event| {
                let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                    return;
                };
                if window.app_handle().webview_windows().len() > 1 {
                    return;
                }
                api.prevent_close();
                let state = window.state::<Arc<ShellState>>();
                let behavior = state.config.runtime_settings.close_behavior();
                let confirmation = (behavior == CloseBehavior::Ask).then(ask_close).flatten();
                match close_decision(behavior, confirmation) {
                    CloseDecision::KeepBackend => {
                        state.background.store(true, Ordering::Release);
                        if let Err(error) = window.destroy() {
                            state.background.store(false, Ordering::Release);
                            state.lifecycle.coordinator().request(
                                pokecon_core::ShutdownReason::FatalError(error.to_string()),
                            );
                        }
                    }
                    CloseDecision::Shutdown => {
                        state.lifecycle.request_exit();
                    }
                    CloseDecision::Cancel => {}
                }
            });

        let app = builder.build(context)?;
        run_event_loop(app, state);
        Ok(())
    }

    fn run_event_loop<R: tauri::Runtime>(app: tauri::App<R>, exit_state: Arc<ShellState>) {
        app.run(move |app_handle, event| {
            if let RunEvent::ExitRequested { api, .. } = event
                && exit_state.lifecycle.coordinator().reason().is_none()
            {
                api.prevent_exit();
                if !exit_state.background.load(Ordering::Acquire)
                    || !app_handle.webview_windows().is_empty()
                {
                    exit_state.lifecycle.request_exit();
                }
            }
        });
    }
}

/// Runs the native Tauri shell. The backend must already be running on the
/// shared Tokio runtime and use the coordinator held by `lifecycle`.
///
/// # Errors
///
/// Returns an error if the application URL is invalid or Tauri cannot create
/// its event loop, window, menu, or tray.
#[cfg(feature = "tauri-shell")]
pub fn run_tauri_shell<F>(
    context: tauri::Context<tauri::Wry>,
    config: DesktopShellConfig,
    lifecycle: &DesktopLifecycle,
    on_primary_instance: F,
) -> Result<(), DesktopError>
where
    F: FnOnce(PathBuf) -> Result<(), DesktopError> + Send + 'static,
{
    shell::run(context, config, lifecycle, on_primary_instance)
}

#[cfg(test)]
mod tests {
    use pokecon_core::{ShutdownCoordinator, ShutdownReason};

    use super::{
        CloseBehavior, CloseDecision, DesktopLifecycle, DesktopRuntimeSettings, close_decision,
    };

    #[test]
    fn close_policy_is_closed_and_confirmation_failure_is_safe() {
        assert!(matches!(
            "keep_backend".parse::<CloseBehavior>(),
            Ok(CloseBehavior::KeepBackend)
        ));
        assert!("background".parse::<CloseBehavior>().is_err());
        assert_eq!(
            close_decision(CloseBehavior::Ask, Some(CloseDecision::Cancel)),
            CloseDecision::Cancel
        );
        assert_eq!(
            close_decision(CloseBehavior::Ask, None),
            CloseDecision::Shutdown
        );
        assert_eq!(
            close_decision(CloseBehavior::Shutdown, Some(CloseDecision::Cancel)),
            CloseDecision::Shutdown
        );
    }

    #[test]
    fn runtime_close_policy_updates_are_visible_to_the_shell() {
        let settings = DesktopRuntimeSettings::new(CloseBehavior::Ask);
        let shell_view = settings.clone();
        settings.set_close_behavior(CloseBehavior::KeepBackend);
        assert_eq!(shell_view.close_behavior(), CloseBehavior::KeepBackend);
    }

    #[tokio::test]
    async fn desktop_exit_uses_the_common_coordinator() {
        let shutdown = ShutdownCoordinator::new();
        let lifecycle = DesktopLifecycle::new(shutdown.clone());
        assert!(lifecycle.request_exit());
        assert_eq!(shutdown.cancelled().await, ShutdownReason::DesktopExit);
    }
}
