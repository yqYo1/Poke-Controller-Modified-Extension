//! `GPUI` `PoC` shell: Tokio/`GPUI` coexistence adapter.
//!
//! The single backend lifecycle (`run_packaged_backend` on the Tokio runtime)
//! is shared with the Web and Tauri modes. Only the GPUI platform event loop
//! moves, onto its own dedicated UI thread, because `Application::run`
//! blocks the calling thread for the lifetime of the app. Close and shutdown
//! signals cross the thread boundary explicitly through [`GpuiUiSignal`]; the
//! UI thread never owns backend state and never spawns a second server or
//! worker.

mod fake_view;

use std::{env, io, thread};

use thiserror::Error;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use gpui_kit::AppContext as _;

use fake_view::GpuiFakeView;

/// Explicit close/shutdown signals sent from the GPUI UI thread to the Tokio
/// side. Every application exit path produces one of these values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GpuiUiSignal {
    /// The native window opened successfully.
    WindowOpened,
    /// The native window failed to open; carries the platform error.
    WindowOpenFailed(String),
    /// The native window was closed by the user or window manager.
    WindowClosed,
    /// The GPUI event loop returned after `quit` was requested.
    EventLoopExited,
}

/// Failure while starting or supervising the `GPUI` `PoC` shell.
#[derive(Debug, Error)]
pub enum GpuiError {
    /// Spawning the dedicated GPUI UI thread failed.
    #[error("failed to spawn the GPUI UI thread: {0}")]
    UiThread(#[source] io::Error),
    /// A Tokio task owned by the GPUI shell failed.
    #[error("GPUI backend task failed: {0}")]
    BackendTask(#[source] tokio::task::JoinError),
    /// The backend stopped before the UI shell closed.
    #[error("the primary GPUI instance did not start its backend")]
    BackendNotStarted,
    /// The native window failed to open.
    #[error("GPUI native window failed to open: {0}")]
    WindowOpen(String),
}

/// Opens the unbounded signal channel shared by the UI thread and the Tokio
/// supervisor.
#[must_use]
pub fn gpui_signal_channel() -> (
    UnboundedSender<GpuiUiSignal>,
    tokio::sync::mpsc::UnboundedReceiver<GpuiUiSignal>,
) {
    unbounded_channel()
}

/// Runs the GPUI platform event loop on a dedicated UI thread and returns
/// immediately with the thread handle.
///
/// Window close requests quit the application from inside the close observer,
/// so every exit path ends the event loop and emits [`GpuiUiSignal`]. The
/// caller owns all shutdown decisions; this function only forwards signals.
///
/// # Errors
///
/// Returns [`GpuiError::UiThread`] if the dedicated UI thread cannot spawn.
pub fn spawn_gpui_ui_thread(
    signals: UnboundedSender<GpuiUiSignal>,
    exit_after_startup: bool,
) -> Result<thread::JoinHandle<()>, GpuiError> {
    thread::Builder::new()
        .name("pokecon-gpui-ui".to_owned())
        .spawn(move || {
            let run_signals = signals.clone();
            let application = if env::var_os("POKECON_GPUI_HEADLESS").is_some() {
                gpui_kit::platform::headless()
            } else {
                gpui_kit::application()
            };
            application.run(move |cx| {
                gpui_kit::init(cx);
                match cx.open_window(gpui_kit::WindowOptions::default(), move |_, cx| {
                    cx.new(GpuiFakeView::new)
                }) {
                    Ok(handle) => {
                        let opened = run_signals.send(GpuiUiSignal::WindowOpened);
                        drop(opened);
                        let closer = run_signals.clone();
                        // The subscription must outlive the launch callback,
                        // which returns while the event loop keeps running.
                        std::mem::forget(cx.on_window_closed(move |cx, _| {
                            let close_send = closer.send(GpuiUiSignal::WindowClosed);
                            drop(close_send);
                            cx.quit();
                        }));
                        cx.activate(true);
                        if exit_after_startup
                            && cx
                                .update_window(handle.into(), |_, window, _| {
                                    window.remove_window();
                                })
                                .is_err()
                        {
                            cx.quit();
                        }
                    }
                    Err(error) => {
                        let failed =
                            run_signals.send(GpuiUiSignal::WindowOpenFailed(error.to_string()));
                        drop(failed);
                        cx.quit();
                    }
                }
            });
            let exited = signals.send(GpuiUiSignal::EventLoopExited);
            drop(exited);
        })
        .map_err(GpuiError::UiThread)
}
