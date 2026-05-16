//! LINE Notify API notifier (stub).
//!
//! LINE Notify service was discontinued in April 2025.
//! This module is kept as a no-op stub for backward compatibility
//! with existing user scripts.

use crate::notify::{Notification, Notifier, NotifyResult};
use async_trait::async_trait;
use tracing::debug;

/// Notifier that delivers messages via the LINE Notify API.
///
/// **DEPRECATED**: LINE Notify service was discontinued in April 2025.
/// This notifier is now a no-op stub. All send operations are silently
/// ignored to maintain backward compatibility with existing scripts.
#[derive(Debug, Clone)]
pub struct LineNotifier {
    /// Access token (kept for API compatibility but never used)
    #[allow(dead_code)]
    access_token: String,
}

impl LineNotifier {
    /// Create a new LINE Notify notifier with the given access token.
    ///
    /// The token is stored for API compatibility but never used.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
        }
    }
}

#[async_trait]
impl Notifier for LineNotifier {
    async fn send(&self, _notification: &Notification) -> NotifyResult<()> {
        debug!("LINE Notify notification skipped (service discontinued)");
        Ok(())
    }
}
