use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::OnceLock;

use pokecon_core::notify::{self, Notifier};

// ---------------------------------------------------------------------------
// Global tokio runtime  (shared pattern with sender.rs and keys.rs)
// ---------------------------------------------------------------------------

fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"))
}

// ---------------------------------------------------------------------------
// Notification  —  wraps pokecon_core::notify::Notification
// ---------------------------------------------------------------------------

/// A notification payload that can be sent through any notifier channel.
///
/// Parameters
/// ----------
/// message : str
///     The main body text of the notification.
///
/// Python usage::
///
///     from pokecon.notify import Notification
///
///     notif = Notification("Shiny Pokémon encountered!")
///     notif = Notification("Message").with_title("Title")
///     notif = Notification("Body").with_title("Title").with_subtitle("Subtitle")
#[pyclass(name = "Notification")]
#[derive(Clone)]
pub struct PyNotification {
    inner: notify::Notification,
}

#[pymethods]
impl PyNotification {
    #[new]
    fn new(message: String) -> Self {
        Self {
            inner: notify::Notification::new(message),
        }
    }

    /// Attach a title to this notification (builder style).
    fn with_title(&self, title: String) -> Self {
        Self {
            inner: self.inner.clone().with_title(title),
        }
    }

    /// Attach a subtitle to this notification (builder style).
    fn with_subtitle(&self, subtitle: String) -> Self {
        Self {
            inner: self.inner.clone().with_subtitle(subtitle),
        }
    }

    /// The main body text of the notification.
    #[getter]
    fn message(&self) -> String {
        self.inner.message.clone()
    }

    /// Optional title (may be ``None``).
    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    /// Optional subtitle / description (may be ``None``).
    #[getter]
    fn subtitle(&self) -> Option<String> {
        self.inner.subtitle.clone()
    }

    fn __repr__(&self) -> String {
        let title = self.inner.title.as_deref().unwrap_or("");
        format!(
            "<Notification title={:?} message={:?}>",
            title, self.inner.message
        )
    }
}

// ---------------------------------------------------------------------------
// LineNotifier  —  wraps pokecon_core::notify::LineNotifier (DEPRECATED stub)
// ---------------------------------------------------------------------------

/// Notifier that delivers messages via the LINE Notify API.
///
/// **DEPRECATED**: LINE Notify service was discontinued in April 2025.
/// This notifier is now a no-op stub. All send operations are silently
/// ignored to maintain backward compatibility with existing scripts.
///
/// Parameters
/// ----------
/// access_token : str
///     LINE Notify access token (stored for API compatibility but never used).
///
/// Python usage::
///
///     from pokecon.notify import LineNotifier, Notification
///
///     notifier = LineNotifier("your-access-token")
///     notifier.send(Notification("Hello!"))  # no-op
#[pyclass(name = "LineNotifier")]
pub struct PyLineNotifier {
    inner: notify::LineNotifier,
}

#[pymethods]
impl PyLineNotifier {
    #[new]
    fn new(access_token: String) -> Self {
        Self {
            inner: notify::LineNotifier::new(access_token),
        }
    }

    /// Send a notification via LINE Notify (deprecated — no-op stub).
    fn send(&self, notification: &PyNotification) -> PyResult<()> {
        global_runtime()
            .block_on(self.inner.send(&notification.inner))
            .map_err(|e| PyRuntimeError::new_err(format!("LINE notify failed: {e}")))
    }

    fn __repr__(&self) -> String {
        "<LineNotifier (deprecated, no-op)>".to_string()
    }
}

// ---------------------------------------------------------------------------
// DesktopNotifier  —  wraps pokecon_core::notify::WindowsNotifier
// ---------------------------------------------------------------------------

/// Notifier that displays native desktop toast notifications.
///
/// Uses the system's native notification mechanism:
///
/// - **Windows**: Toast notifications via the Windows Runtime
/// - **Linux**: libnotify / D-Bus
/// - **macOS**: Notification Center
///
/// Parameters
/// ----------
/// app_name : str
///     Application name displayed as the notification source.
///
/// Python usage::
///
///     from pokecon.notify import DesktopNotifier, Notification
///
///     notifier = DesktopNotifier("Poke-Controller")
///     notifier.send(
///         Notification("Shiny Pokémon encountered!")
///             .with_title("Pokémon Alert")
///     )
#[pyclass(name = "DesktopNotifier")]
pub struct PyDesktopNotifier {
    inner: notify::WindowsNotifier,
}

#[pymethods]
impl PyDesktopNotifier {
    #[new]
    fn new(app_name: String) -> Self {
        Self {
            inner: notify::WindowsNotifier::new(app_name),
        }
    }

    /// Send a notification as a native desktop toast notification.
    fn send(&self, notification: &PyNotification) -> PyResult<()> {
        global_runtime()
            .block_on(self.inner.send(&notification.inner))
            .map_err(|e| PyRuntimeError::new_err(format!("Desktop notification failed: {e}")))
    }

    fn __repr__(&self) -> String {
        "<DesktopNotifier>".to_string()
    }
}

// ---------------------------------------------------------------------------
// DiscordNotifier  —  wraps pokecon_core::notify::DiscordNotifier
// ---------------------------------------------------------------------------

/// Notifier that delivers messages to a Discord channel via webhooks.
///
/// Parameters
/// ----------
/// webhook_url : str
///     A full Discord webhook URL obtained from your Discord server's
///     channel settings → Integrations → Webhooks.
///
/// Python usage::
///
///     from pokecon.notify import DiscordNotifier, Notification
///
///     notifier = DiscordNotifier("https://discord.com/api/webhooks/abc123/def456")
///     notifier.send(Notification("Shiny Pokémon found!"))
#[pyclass(name = "DiscordNotifier")]
pub struct PyDiscordNotifier {
    inner: notify::DiscordNotifier,
}

#[pymethods]
impl PyDiscordNotifier {
    #[new]
    fn new(webhook_url: String) -> Self {
        Self {
            inner: notify::DiscordNotifier::new(webhook_url),
        }
    }

    /// Send a notification via a Discord webhook URL.
    fn send(&self, notification: &PyNotification) -> PyResult<()> {
        global_runtime()
            .block_on(self.inner.send(&notification.inner))
            .map_err(|e| PyRuntimeError::new_err(format!("Discord notification failed: {e}")))
    }

    fn __repr__(&self) -> String {
        "<DiscordNotifier>".to_string()
    }
}

// ===================================================================
// Module-level convenience functions
// ===================================================================

/// Send a notification via LINE Notify (deprecated — no-op stub).
///
/// LINE Notify was discontinued in April 2025. This function is kept
/// for backward compatibility and silently ignores all input.
///
/// Parameters
/// ----------
/// access_token : str
///     LINE Notify access token (unused).
/// message : str
///     Notification body text.
/// title : str, optional
///     Notification title (ignored).
///
/// Python usage::
///
///     from pokecon.notify import send_line
///     send_line("your-token", "Hello!")  # no-op
#[pyfunction]
#[pyo3(signature = (access_token, message, title = None))]
fn send_line(access_token: String, message: String, title: Option<String>) -> PyResult<()> {
    let notifier = notify::LineNotifier::new(access_token);
    let mut notification = notify::Notification::new(message);
    if let Some(t) = title {
        notification = notification.with_title(t);
    }
    global_runtime()
        .block_on(notifier.send(&notification))
        .map_err(|e| PyRuntimeError::new_err(format!("LINE notify failed: {e}")))
}

/// Send a native desktop notification.
///
/// Parameters
/// ----------
/// app_name : str
///     Application name shown as the notification source.
/// message : str
///     Main body text of the notification.
/// title : str, optional
///     Optional notification title.
///
/// Python usage::
///
///     from pokecon.notify import send_desktop
///     send_desktop("Poke-Controller", "Shiny Pokémon!", title="Alert")
#[pyfunction]
#[pyo3(signature = (app_name, message, title = None))]
fn send_desktop(app_name: String, message: String, title: Option<String>) -> PyResult<()> {
    let notifier = notify::WindowsNotifier::new(app_name);
    let mut notification = notify::Notification::new(message);
    if let Some(t) = title {
        notification = notification.with_title(t);
    }
    global_runtime()
        .block_on(notifier.send(&notification))
        .map_err(|e| PyRuntimeError::new_err(format!("Desktop notification failed: {e}")))
}

/// Send a notification via a Discord webhook.
///
/// Parameters
/// ----------
/// webhook_url : str
///     Full Discord webhook URL.
/// message : str
///     Content of the message to send.
/// title : str, optional
///     Optional title (sent as a Discord embed when present).
///
/// Python usage::
///
///     from pokecon.notify import send_discord
///     send_discord("https://discord.com/api/webhooks/...", "Hello!")
#[pyfunction]
#[pyo3(signature = (webhook_url, message, title = None))]
fn send_discord(webhook_url: String, message: String, title: Option<String>) -> PyResult<()> {
    let notifier = notify::DiscordNotifier::new(webhook_url);
    let mut notification = notify::Notification::new(message);
    if let Some(t) = title {
        notification = notification.with_title(t);
    }
    global_runtime()
        .block_on(notifier.send(&notification))
        .map_err(|e| PyRuntimeError::new_err(format!("Discord notification failed: {e}")))
}

/// Send an email notification.
///
/// .. note::
///     Email notification is **not yet implemented** in the core library.
///     This function is a placeholder and will raise ``NotImplementedError``.
///
/// Parameters
/// ----------
/// to : str
///     Recipient email address.
/// subject : str
///     Email subject line.
/// body : str
///     Email body content.
///
/// Python usage::
///
///     from pokecon.notify import send_email
///     send_email("user@example.com", "Subject", "Body")
///     # raises NotImplementedError
#[pyfunction]
#[pyo3(signature = (to, subject, body))]
fn send_email(to: String, subject: String, body: String) -> PyResult<()> {
    Err(pyo3::exceptions::PyNotImplementedError::new_err(format!(
        "Email notification is not yet implemented. Would send to: {to}, \
         subject: {subject}, body: {body}"
    )))
}

// ===================================================================
// Module registration
// ===================================================================

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyNotification>()?;
    m.add_class::<PyLineNotifier>()?;
    m.add_class::<PyDesktopNotifier>()?;
    m.add_class::<PyDiscordNotifier>()?;

    m.add_function(wrap_pyfunction!(send_line, m)?)?;
    m.add_function(wrap_pyfunction!(send_desktop, m)?)?;
    m.add_function(wrap_pyfunction!(send_discord, m)?)?;
    m.add_function(wrap_pyfunction!(send_email, m)?)?;

    Ok(())
}

// ===================================================================
// Tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Notification tests
    // ------------------------------------------------------------------

    #[test]
    fn test_notification_new() {
        Python::with_gil(|py| {
            let notif = Bound::new(py, PyNotification::new("test".to_string())).unwrap();
            let msg: String = notif.call_method0("message").unwrap().extract().unwrap();
            assert_eq!(msg, "test");
            let title: Option<String> = notif.call_method0("title").unwrap().extract().unwrap();
            assert!(title.is_none());
            let subtitle: Option<String> =
                notif.call_method0("subtitle").unwrap().extract().unwrap();
            assert!(subtitle.is_none());
        });
    }

    #[test]
    fn test_notification_with_title() {
        Python::with_gil(|py| {
            let notif = Bound::new(
                py,
                PyNotification::new("body".to_string()).with_title("mytitle".to_string()),
            )
            .unwrap();
            let msg: String = notif.call_method0("message").unwrap().extract().unwrap();
            assert_eq!(msg, "body");
            let title: Option<String> = notif.call_method0("title").unwrap().extract().unwrap();
            assert_eq!(title.as_deref(), Some("mytitle"));
        });
    }

    #[test]
    fn test_notification_with_subtitle() {
        Python::with_gil(|py| {
            let notif = Bound::new(
                py,
                PyNotification::new("body".to_string())
                    .with_title("t".to_string())
                    .with_subtitle("sub".to_string()),
            )
            .unwrap();
            let sub: Option<String> = notif.call_method0("subtitle").unwrap().extract().unwrap();
            assert_eq!(sub.as_deref(), Some("sub"));
        });
    }

    #[test]
    fn test_notification_repr() {
        Python::with_gil(|py| {
            let notif = Bound::new(py, PyNotification::new("hello".to_string())).unwrap();
            let repr: String = notif.call_method0("__repr__").unwrap().extract().unwrap();
            assert!(repr.contains("hello"));
        });
    }

    // ------------------------------------------------------------------
    // Notifier construction tests
    // ------------------------------------------------------------------

    #[test]
    fn test_line_notifier_new() {
        Python::with_gil(|py| {
            let n = Bound::new(py, PyLineNotifier::new("test-token".to_string())).unwrap();
            let repr: String = n.call_method0("__repr__").unwrap().extract().unwrap();
            assert!(repr.contains("LineNotifier"));
        });
    }

    #[test]
    fn test_desktop_notifier_new() {
        Python::with_gil(|py| {
            let n = Bound::new(py, PyDesktopNotifier::new("TestApp".to_string())).unwrap();
            let repr: String = n.call_method0("__repr__").unwrap().extract().unwrap();
            assert!(repr.contains("DesktopNotifier"));
        });
    }

    #[test]
    fn test_discord_notifier_new() {
        Python::with_gil(|py| {
            let n = Bound::new(
                py,
                PyDiscordNotifier::new("https://discord.com/api/webhooks/foo/bar".to_string()),
            )
            .unwrap();
            let repr: String = n.call_method0("__repr__").unwrap().extract().unwrap();
            assert!(repr.contains("DiscordNotifier"));
        });
    }

    // ------------------------------------------------------------------
    // LineNotifier send test (no-op stub)
    // ------------------------------------------------------------------

    #[test]
    fn test_line_notifier_send_ok() {
        Python::with_gil(|py| {
            let n = Bound::new(py, PyLineNotifier::new("test-token".to_string())).unwrap();
            let notif = Bound::new(py, PyNotification::new("test".to_string())).unwrap();
            // LINE is a no-op stub, so this should always succeed
            n.call_method1("send", (notif,)).unwrap();
        });
    }

    // ------------------------------------------------------------------
    // Convenience function tests
    // ------------------------------------------------------------------

    #[test]
    fn test_send_email_not_implemented() {
        Python::with_gil(|py| {
            let m = PyModule::new(py, "test_notify").unwrap();
            register(&m).unwrap();
            let f = m
                .getattr("send_email")
                .expect("send_email should be registered");
            let result = f.call1(("a@b.c", "sub", "body"));
            assert!(result.is_err());
            let err = result.unwrap_err();
            let msg = format!("{err}");
            assert!(
                msg.contains("NotImplementedError") || msg.contains("not yet implemented"),
                "Expected NotImplementedError, got: {msg}"
            );
        });
    }
}
