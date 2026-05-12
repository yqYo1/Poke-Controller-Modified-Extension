//! LINE Notify API notifier.
//!
//! Sends messages via the [LINE Notify API](https://notify-bot.line.me/doc/en/).
//!
//! Uses a personal access token obtained from the LINE Notify service
//! (https://notify-bot.line.me/my/).  Unlike the LINE Messaging API,
//! LINE Notify is a simpler service that sends notifications to a single
//! recipient (the user who generated the token).

use crate::{Notification, Notifier, NotifyError, NotifyResult};
use async_trait::async_trait;
use reqwest::Client;
use tracing::{debug, error};

/// Notifier that delivers messages via the LINE Notify API.
///
/// # Example
///
/// ```no_run
/// use pokecon_notify::{Notifier, Notification, line::LineNotifier};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let notifier = LineNotifier::new("your-line-notify-access-token");
/// notifier.send(&Notification::new("Hello from Poke-Controller!")).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct LineNotifier {
    /// Access token for the LINE Notify API
    /// (obtained from https://notify-bot.line.me/my/)
    access_token: String,
    /// Shared reqwest client for HTTP requests
    client: Client,
}

impl LineNotifier {
    /// Create a new LINE Notify notifier with the given access token.
    ///
    /// The access token must be a valid LINE Notify personal access token.
    /// Tokens can be generated at https://notify-bot.line.me/my/
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            client: Client::new(),
        }
    }

    /// Set a custom [`reqwest::Client`] (useful for testing with mocks).
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }
}

#[async_trait]
impl Notifier for LineNotifier {
    async fn send(&self, notification: &Notification) -> NotifyResult<()> {
        let message = build_message_text(notification);

        debug!(
            message = %message,
            "Sending LINE Notify notification"
        );

        let response = self
            .client
            .post("https://notify-api.line.me/api/notify")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .form(&[("message", message)])
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_default();

        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            error!(
                %status,
                %msg,
                "LINE Notify API returned error"
            );
            return Err(NotifyError::Other(format!(
                "LINE Notify API returned HTTP {status}: {msg}"
            )));
        }

        debug!("LINE Notify notification sent successfully");
        Ok(())
    }
}

/// Build the message text to send via LINE Notify.
///
/// The LINE Notify API supports a single `message` field.  If a title is
/// provided, it is prepended in the format `[Title] message`.
fn build_message_text(notification: &Notification) -> String {
    match &notification.title {
        Some(title) => {
            if let Some(subtitle) = &notification.subtitle {
                format!("[{}]\n{}\n\n{}", title, subtitle, notification.message)
            } else {
                format!("[{}]\n{}", title, notification.message)
            }
        }
        None => notification.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_message_text_plain() {
        let n = Notification::new("test message");
        assert_eq!(build_message_text(&n), "test message");
    }

    #[test]
    fn test_build_message_text_with_title() {
        let n = Notification::new("body").with_title("Title");
        assert_eq!(build_message_text(&n), "[Title]\nbody");
    }

    #[test]
    fn test_build_message_text_with_subtitle() {
        let n = Notification::new("body")
            .with_title("Title")
            .with_subtitle("Sub");
        assert_eq!(build_message_text(&n), "[Title]\nSub\n\nbody");
    }

    #[tokio::test]
    async fn test_line_notifier_creation() {
        let notifier = LineNotifier::new("test-token");
        assert_eq!(notifier.access_token, "test-token");
    }
}
