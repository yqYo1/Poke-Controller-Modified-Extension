//! Discord webhook notifier.
//!
//! Sends messages to a Discord channel via an incoming webhook URL.
//! Supports simple text messages and rich embed notifications.

use crate::{Notification, Notifier, NotifyError, NotifyResult};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::{debug, error};

/// Notifier that delivers messages to a Discord channel via webhooks.
///
/// # Example
///
/// ```no_run
/// use pokecon_notify::{Notifier, Notification, discord::DiscordNotifier};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let notifier = DiscordNotifier::new("https://discord.com/api/webhooks/abc123/def456");
/// notifier.send(&Notification::new("Shiny Pokémon found!")).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DiscordNotifier {
    webhook_url: String,
    client: Client,
}

impl DiscordNotifier {
    /// Create a new Discord webhook notifier.
    ///
    /// `webhook_url` should be a full Discord webhook URL obtained from
    /// your Discord server's channel settings → Integrations → Webhooks.
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
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
impl Notifier for DiscordNotifier {
    async fn send(&self, notification: &Notification) -> NotifyResult<()> {
        let payload = build_payload(notification);

        debug!(
            url = %self.webhook_url,
            message = %notification.message,
            "Sending Discord notification"
        );

        let response = self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(%status, %body, "Discord webhook returned error");
            return Err(NotifyError::Other(format!(
                "Discord webhook returned HTTP {status}: {body}"
            )));
        }

        debug!("Discord notification sent successfully");
        Ok(())
    }
}

/// Build the JSON payload sent to Discord's webhook API.
///
/// If a `title` is set, the message is sent as a Discord embed.
/// Otherwise it is sent as a plain text message.
fn build_payload(notification: &Notification) -> serde_json::Value {
    if let Some(title) = &notification.title {
        // Rich embed notification
        let mut embed = json!({
            "title": title,
            "description": notification.message,
            "color": 0x5865F2, // Discord blurple
        });

        if let Some(subtitle) = &notification.subtitle {
            embed["description"] = json!(format!("{}\n\n{}", subtitle, notification.message));
        }

        json!({
            "embeds": [embed]
        })
    } else {
        // Plain text message
        json!({
            "content": notification.message
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_payload_plain_text() {
        let n = Notification::new("test message");
        let payload = build_payload(&n);
        assert_eq!(payload["content"], "test message");
        assert!(payload.get("embeds").is_none());
    }

    #[test]
    fn test_build_payload_with_embed() {
        let n = Notification::new("description here")
            .with_title("My Title");
        let payload = build_payload(&n);
        assert!(payload.get("content").is_none());
        let embed = &payload["embeds"][0];
        assert_eq!(embed["title"], "My Title");
        assert_eq!(embed["description"], "description here");
    }

    #[test]
    fn test_build_payload_with_subtitle() {
        let n = Notification::new("message text")
            .with_title("Title")
            .with_subtitle("Sub");
        let payload = build_payload(&n);
        let desc = payload["embeds"][0]["description"].as_str().unwrap();
        assert!(desc.contains("Sub"));
        assert!(desc.contains("message text"));
    }
}
