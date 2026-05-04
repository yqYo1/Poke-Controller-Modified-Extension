//! LINE Messaging API notifier (dummy implementation).
//!
//! This module provides a placeholder [`LineNotifier`] that logs
//! notifications via `tracing` instead of actually calling the
//! LINE Messaging API.  Replace with a real implementation that
//! uses the LINE Messaging API SDK or raw HTTP requests.

use crate::{Notification, Notifier, NotifyResult};
use async_trait::async_trait;
use tracing::info;

/// Dummy notifier for LINE Messenger.
///
/// Currently logs all notifications at `info` level.  A real
/// implementation should POST to `https://api.line.me/v2/bot/message/push`
/// using a channel access token.
///
/// # Example
///
/// ```no_run
/// use pokecon_notify::{Notifier, Notification, line::LineNotifier};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let notifier = LineNotifier::new("your-channel-access-token");
/// notifier.send(&Notification::new("Hello LINE!")).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct LineNotifier {
    /// Channel access token for the LINE Messaging API.
    #[allow(dead_code)]
    channel_access_token: String,
}

impl LineNotifier {
    /// Create a new LINE notifier with the given channel access token.
    pub fn new(channel_access_token: impl Into<String>) -> Self {
        Self {
            channel_access_token: channel_access_token.into(),
        }
    }
}

#[async_trait]
impl Notifier for LineNotifier {
    async fn send(&self, notification: &Notification) -> NotifyResult<()> {
        // TODO: Replace with real LINE Messaging API call
        // https://developers.line.biz/en/reference/messaging-api/#send-push-message
        //
        // let body = json!({
        //     "to": "...",
        //     "messages": [{
        //         "type": "text",
        //         "text": notification.message,
        //     }],
        // });
        //
        // let resp = client
        //     .post("https://api.line.me/v2/bot/message/push")
        //     .header("Authorization", format!("Bearer {}", self.channel_access_token))
        //     .json(&body)
        //     .send()
        //     .await?;

        info!(
            message = %notification.message,
            title = ?notification.title,
            "[LINE dummy] Would send notification"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_line_notifier_dummy() {
        let notifier = LineNotifier::new("test-token");
        // Should not fail – the dummy impl always returns Ok.
        let result = notifier
            .send(&Notification::new("test"))
            .await;
        assert!(result.is_ok());
    }
}
