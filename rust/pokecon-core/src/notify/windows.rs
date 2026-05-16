//! Desktop toast notification notifier.
//!
//! Uses the [`notify-rust`] crate to send native desktop notifications.
//! - **Windows**: Windows Toast (via WinRT)
//! - **Linux**: libnotify / D-Bus notifications
//! - **macOS**: Notification Center
//!
//! This module is named `windows` for historical reasons, but it works
//! on all desktop platforms.

use crate::notify::{Notification, Notifier, NotifyError, NotifyResult};
use async_trait::async_trait;
use notify_rust::Notification as NativeNotification;
use tracing::{debug, warn};

/// Notifier that displays native desktop toast notifications.
///
/// Uses the system's native notification mechanism:
/// - Windows: Toast notifications via the Windows Runtime
/// - Linux: libnotify / D-Bus
/// - macOS: Notification Center
///
/// # Example
///
/// ```no_run
/// use pokecon_core::notify::{Notifier, Notification, windows::WindowsNotifier};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let notifier = WindowsNotifier::new("Poke-Controller");
/// notifier.send(
///     &Notification::new("Shiny Pokémon encountered!")
///         .with_title("Pokémon Alert"),
/// ).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct WindowsNotifier {
    /// Application name displayed in the notification.
    app_name: String,
}

impl WindowsNotifier {
    /// Create a new desktop notifier.
    ///
    /// `app_name` is shown as the notification source (e.g. in the
    /// Windows Action Center or GNOME notification panel).
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
        }
    }
}

#[async_trait]
impl Notifier for WindowsNotifier {
    async fn send(&self, notification: &Notification) -> NotifyResult<()> {
        let mut native = NativeNotification::new();

        native.summary(&self.app_name);

        // Use the title if provided, otherwise fall back to the app name.
        let body = match &notification.title {
            Some(title) => {
                native.body(&format!("{}\n{}", title, notification.message));
                notification.message.clone()
            }
            None => {
                native.body(&notification.message);
                notification.message.clone()
            }
        };

        // Configure the notification timeout and icon hint.
        native.timeout(notify_rust::Timeout::Milliseconds(5000));

        debug!(app = %self.app_name, body = %body, "Showing desktop notification");

        match native.show() {
            Ok(_) => {
                debug!("Desktop notification shown successfully");
                Ok(())
            }
            Err(e) => {
                // notify-rust can fail if no notification daemon is running
                // (e.g. headless server, SSH session without D-Bus).
                warn!(
                    error = %e,
                    "Failed to show desktop notification (headless environment?)"
                );
                Err(NotifyError::Notify(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_windows_notifier_creation() {
        let notifier = WindowsNotifier::new("Poke-Controller");
        assert_eq!(notifier.app_name, "Poke-Controller");
    }

    #[tokio::test]
    async fn test_windows_notifier_display() {
        let notifier = WindowsNotifier::new("Test-App");
        // In a CI/headless environment this may fail, but the code path
        // should not panic.
        let result = notifier
            .send(&Notification::new("test body").with_title("test"))
            .await;
        // May be Ok (desktop) or Err (headless) – either is acceptable.
        if let Err(e) = &result {
            println!("Desktop notification unavailable (expected in CI): {e}");
        }
    }
}
