//! Native desktop shell and close-policy boundary for the shared Rust backend.

#[cfg(any(feature = "tauri-shell", test))]
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
#[cfg(any(feature = "tauri-shell", test))]
use std::sync::atomic::AtomicBool;
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
    /// The colocated backend stopped before publishing its bound listener.
    #[error("desktop backend listener was not published")]
    BackendAddressUnavailable,
    /// The native shell attempted to replace its immutable backend listener.
    #[error("desktop backend listener was published more than once")]
    BackendAddressAlreadyPublished,
    /// The primary desktop instance could not start its colocated backend.
    #[error("desktop backend startup failed: {0}")]
    BackendStartup(String),
    /// The native event loop returned an unsuccessful process status.
    #[error("Tauri desktop event loop exited with status {0}")]
    TauriExit(i32),
    /// The native shell unwound instead of returning its event-loop status.
    #[error("Tauri desktop shell panicked: {0}")]
    TauriPanic(String),
    /// The event loop exited cleanly after a fatal lifecycle request.
    #[error("desktop lifecycle reported a fatal shutdown: {0}")]
    FatalShutdown(String),
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

fn finish_event_loop(exit_code: i32, lifecycle: &DesktopLifecycle) -> Result<(), DesktopError> {
    if exit_code == 0 {
        lifecycle.request_exit();
        return match lifecycle.coordinator().reason() {
            Some(ShutdownReason::FatalError(message)) => Err(DesktopError::FatalShutdown(message)),
            Some(
                ShutdownReason::Signal(_)
                | ShutdownReason::DesktopExit
                | ShutdownReason::StartupProbe
                | ShutdownReason::WorkerStop,
            )
            | None => Ok(()),
        };
    }
    let error = DesktopError::TauriExit(exit_code);
    lifecycle
        .coordinator()
        .request(ShutdownReason::FatalError(error.to_string()));
    Err(error)
}

#[cfg(any(feature = "tauri-shell", test))]
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

#[cfg(any(feature = "tauri-shell", test))]
fn catch_tauri_shell_panic<T, F>(
    lifecycle: &DesktopLifecycle,
    run_shell: F,
) -> Result<T, DesktopError>
where
    F: FnOnce() -> Result<T, DesktopError>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run_shell)) {
        Ok(result) => result,
        Err(payload) => {
            let error = DesktopError::TauriPanic(panic_payload_message(payload.as_ref()));
            lifecycle
                .coordinator()
                .request(ShutdownReason::FatalError(error.to_string()));
            Err(error)
        }
    }
}

#[cfg(any(feature = "tauri-shell", test))]
fn open_main_window_if_shell_ready<E, F>(
    shell_ready: &AtomicBool,
    open_main_window: F,
) -> Result<(), E>
where
    F: FnOnce() -> Result<(), E>,
{
    if !shell_ready.load(Ordering::Acquire) {
        return Ok(());
    }
    open_main_window()
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

#[cfg(any(feature = "tauri-shell", test))]
fn backend_app_url(address: SocketAddr) -> String {
    format!("http://{address}")
}

#[cfg(feature = "tauri-shell")]
mod shell {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};

    use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
    use tauri::image::Image;
    use tauri::ipc::CapabilityBuilder;
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    use tauri::{Manager as _, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

    use super::{
        CloseBehavior, CloseDecision, DesktopError, DesktopLifecycle, DesktopShellConfig,
        backend_app_url, close_decision, finish_event_loop, open_main_window_if_shell_ready,
    };

    const MAIN_WINDOW_LABEL: &str = "main";

    struct ShellState {
        background: AtomicBool,
        backend_address: OnceLock<SocketAddr>,
        config: DesktopShellConfig,
        lifecycle: DesktopLifecycle,
        shell_ready: AtomicBool,
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
        let address = state
            .backend_address
            .get()
            .copied()
            .ok_or(DesktopError::BackendAddressUnavailable)?;
        let app_url = backend_app_url(address);
        let url =
            tauri::Url::parse(&app_url).map_err(|_error| DesktopError::InvalidAppUrl(app_url))?;
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
        F: FnOnce() -> Result<SocketAddr, DesktopError> + Send + 'static,
    {
        let state = Arc::new(ShellState {
            background: AtomicBool::new(false),
            backend_address: OnceLock::new(),
            config,
            lifecycle: lifecycle.clone(),
            shell_ready: AtomicBool::new(false),
        });
        let single_instance_state = Arc::clone(&state);
        let setup_state = Arc::clone(&state);
        let builder =
            tauri::Builder::default()
                .manage(Arc::clone(&state))
                .plugin(tauri_plugin_single_instance::init(
                    move |app, _arguments, _cwd| {
                        if let Err(error) = open_main_window_if_shell_ready(
                            &single_instance_state.shell_ready,
                            || open_main_window(app),
                        ) {
                            single_instance_state.lifecycle.coordinator().request(
                                pokecon_core::ShutdownReason::FatalError(error.to_string()),
                            );
                        }
                    },
                ))
                .invoke_handler(tauri::generate_handler![
                    choose_save_path,
                    open_config_directory
                ])
                .setup(move |app| {
                    let backend_address = on_primary_instance()?;
                    let app_url = backend_app_url(backend_address);
                    tauri::Url::parse(&app_url)
                        .map_err(|_error| DesktopError::InvalidAppUrl(app_url.clone()))?;
                    let remote = format!("{}/*", app_url.trim_end_matches('/'));
                    app.add_capability(
                        CapabilityBuilder::new("pokecon-local-backend")
                            .local(false)
                            .remote(remote)
                            .window(MAIN_WINDOW_LABEL)
                            .permission("allow-choose-save-path")
                            .permission("allow-open-config-directory"),
                    )?;
                    setup_state
                        .backend_address
                        .set(backend_address)
                        .map_err(|_address| DesktopError::BackendAddressAlreadyPublished)?;
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
                    setup_state.shell_ready.store(true, Ordering::Release);
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
        let exit_code = run_event_loop(app, state);
        finish_event_loop(exit_code, lifecycle)
    }

    fn run_event_loop<R: tauri::Runtime>(app: tauri::App<R>, exit_state: Arc<ShellState>) -> i32 {
        app.run_return(move |app_handle, event| {
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
        })
    }
}

/// Runs the native Tauri shell. The primary-instance callback starts the
/// backend on the shared Tokio runtime and returns its actual listener before
/// the capability and native window are created.
///
/// # Errors
///
/// Returns an error if the application URL is invalid, Tauri cannot create its
/// event loop, window, menu, or tray, or the event loop returns a nonzero status.
#[cfg(feature = "tauri-shell")]
pub fn run_tauri_shell<F>(
    context: tauri::Context<tauri::Wry>,
    config: DesktopShellConfig,
    lifecycle: &DesktopLifecycle,
    on_primary_instance: F,
) -> Result<(), DesktopError>
where
    F: FnOnce() -> Result<SocketAddr, DesktopError> + Send + 'static,
{
    // Tauri 2.11.5's `App::run_return` panics when `Builder::setup` returns an
    // error. Catch that pinned behavior here so a backend published by setup
    // is cancelled and joined by the outer desktop lifecycle before reporting
    // the typed shell failure.
    catch_tauri_shell_panic(lifecycle, || {
        shell::run(context, config, lifecycle, on_primary_instance)
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, Ordering};

    use pokecon_core::{ShutdownCoordinator, ShutdownReason};

    use super::{
        CloseBehavior, CloseDecision, DesktopLifecycle, DesktopRuntimeSettings, backend_app_url,
        catch_tauri_shell_panic, close_decision, finish_event_loop,
        open_main_window_if_shell_ready, panic_payload_message,
    };

    #[test]
    fn early_single_instance_launch_does_not_open_or_fail() {
        let shell_ready = AtomicBool::new(false);
        let open_called = Cell::new(false);

        let result = open_main_window_if_shell_ready(&shell_ready, || {
            open_called.set(true);
            Err("window open failed")
        });

        assert_eq!(result, Ok(()));
        assert!(!open_called.get());
    }

    #[test]
    fn ready_single_instance_launch_invokes_and_reports_opener_failure() {
        let shell_ready = AtomicBool::new(false);
        shell_ready.store(true, Ordering::Release);
        let open_called = Cell::new(false);

        let result = open_main_window_if_shell_ready(&shell_ready, || {
            open_called.set(true);
            Err("window open failed")
        });

        assert_eq!(result, Err("window open failed"));
        assert!(open_called.get());
    }

    #[test]
    fn backend_url_preserves_ipv4_and_ipv6_listener_authority() {
        assert_eq!(
            backend_app_url(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8020)),
            "http://127.0.0.1:8020"
        );
        assert_eq!(
            backend_app_url(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8020)),
            "http://[::1]:8020"
        );
    }

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

    #[test]
    fn event_loop_status_requests_shutdown_and_preserves_the_first_reason() {
        let clean_shutdown = ShutdownCoordinator::new();
        let clean_lifecycle = DesktopLifecycle::new(clean_shutdown.clone());
        assert!(finish_event_loop(0, &clean_lifecycle).is_ok());
        assert_eq!(clean_shutdown.reason(), Some(ShutdownReason::DesktopExit));

        let failed_shutdown = ShutdownCoordinator::new();
        let failed_lifecycle = DesktopLifecycle::new(failed_shutdown.clone());
        assert!(matches!(
            finish_event_loop(23, &failed_lifecycle),
            Err(super::DesktopError::TauriExit(23))
        ));
        assert_eq!(
            failed_shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "Tauri desktop event loop exited with status 23".to_owned()
            ))
        );

        let prior_shutdown = ShutdownCoordinator::new();
        assert!(prior_shutdown.request(ShutdownReason::StartupProbe));
        let prior_lifecycle = DesktopLifecycle::new(prior_shutdown.clone());
        assert!(matches!(
            finish_event_loop(29, &prior_lifecycle),
            Err(super::DesktopError::TauriExit(29))
        ));
        assert_eq!(prior_shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[test]
    fn zero_event_loop_status_reports_prior_fatal_but_accepts_clean_reasons() {
        let failed_shutdown = ShutdownCoordinator::new();
        assert!(failed_shutdown.request(ShutdownReason::FatalError("backend failed".to_owned())));
        let failed_lifecycle = DesktopLifecycle::new(failed_shutdown.clone());
        assert!(matches!(
            finish_event_loop(0, &failed_lifecycle),
            Err(super::DesktopError::FatalShutdown(message)) if message == "backend failed"
        ));
        assert_eq!(
            failed_shutdown.reason(),
            Some(ShutdownReason::FatalError("backend failed".to_owned()))
        );

        let clean_shutdown = ShutdownCoordinator::new();
        assert!(clean_shutdown.request(ShutdownReason::StartupProbe));
        let clean_lifecycle = DesktopLifecycle::new(clean_shutdown.clone());
        assert!(finish_event_loop(0, &clean_lifecycle).is_ok());
        assert_eq!(clean_shutdown.reason(), Some(ShutdownReason::StartupProbe));
    }

    #[test]
    fn panic_payload_messages_are_deterministic() {
        assert_eq!(panic_payload_message(&"borrowed panic"), "borrowed panic");
        assert_eq!(
            panic_payload_message(&"owned panic".to_owned()),
            "owned panic"
        );
        assert_eq!(panic_payload_message(&23_u8), "non-string panic payload");
    }

    #[test]
    fn caught_tauri_panic_requests_fatal_shutdown() {
        let shutdown = ShutdownCoordinator::new();
        let lifecycle = DesktopLifecycle::new(shutdown.clone());
        let result = catch_tauri_shell_panic(&lifecycle, || -> Result<(), super::DesktopError> {
            std::panic::panic_any("setup failed");
        });

        assert!(matches!(
            result,
            Err(super::DesktopError::TauriPanic(message)) if message == "setup failed"
        ));
        assert_eq!(
            shutdown.reason(),
            Some(ShutdownReason::FatalError(
                "Tauri desktop shell panicked: setup failed".to_owned()
            ))
        );
    }
}
