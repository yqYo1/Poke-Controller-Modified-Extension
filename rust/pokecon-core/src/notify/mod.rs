pub mod discord;
pub mod line;
pub mod windows;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during notification delivery.
#[derive(Debug, Error)]
pub enum NotifyError {
    /// HTTP request failed (Discord webhook, etc.)
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// I/O or platform-specific error (desktop notification)
    #[error("notification error: {0}")]
    Notify(#[from] notify_rust::error::Error),

    /// General error with a message
    #[error("{0}")]
    Other(String),
}

/// Convenience alias for notification results.
pub type NotifyResult<T> = Result<T, NotifyError>;

// ---------------------------------------------------------------------------
// Notification payload
// ---------------------------------------------------------------------------

/// A notification to be delivered through one of the notifier channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Main body text of the notification.
    pub message: String,
    /// Optional title (used by desktop notifications and Discord embeds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional sub-title / description (Discord embed description).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

impl Notification {
    /// Create a simple text-only notification.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            title: None,
            subtitle: None,
        }
    }

    /// Builder-style: attach an optional title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Builder-style: attach an optional subtitle.
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Notifier trait
// ---------------------------------------------------------------------------

/// Common interface for all notification channels.
#[async_trait::async_trait]
pub trait Notifier: Send + Sync {
    /// Deliver a notification.
    async fn send(&self, notification: &Notification) -> NotifyResult<()>;
}

pub use discord::DiscordNotifier;
pub use line::LineNotifier;
pub use windows::WindowsNotifier;
