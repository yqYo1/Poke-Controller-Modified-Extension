//! Non-fatal Discord and Windows notification hooks.

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::RwLock;
use url::Url;

/// Strictly validated Discord webhook secret.
#[derive(Clone)]
pub struct DiscordWebhookUrl(Url);

impl DiscordWebhookUrl {
    /// Accepts only `https://discord.com/api/webhooks/<id>/<token>` with no
    /// credentials, query, fragment, or custom port.
    ///
    /// # Errors
    ///
    /// Returns a fixed validation error that never repeats the secret.
    pub fn parse(raw: &str) -> Result<Self, NotificationError> {
        let url = Url::parse(raw).map_err(|_| NotificationError::InvalidWebhookUrl)?;
        let segments = url
            .path_segments()
            .ok_or(NotificationError::InvalidWebhookUrl)?
            .collect::<Vec<_>>();
        if url.scheme() != "https"
            || url.host_str() != Some("discord.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || segments.len() != 4
            || segments[0] != "api"
            || segments[1] != "webhooks"
            || segments[2].is_empty()
            || segments[3].is_empty()
        {
            return Err(NotificationError::InvalidWebhookUrl);
        }
        Ok(Self(url))
    }

    fn as_url(&self) -> &Url {
        &self.0
    }
}

impl std::fmt::Debug for DiscordWebhookUrl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiscordWebhookUrl(<redacted>)")
    }
}

/// Discord delivery settings. The custom avatar is public metadata, while the
/// webhook is always redacted from `Debug` output.
#[derive(Clone, Default)]
pub struct DiscordNotificationConfig {
    pub webhook: Option<DiscordWebhookUrl>,
    pub username: Option<String>,
    pub avatar_url: Option<Url>,
    pub on_script_start: bool,
    pub on_script_end: bool,
}

impl std::fmt::Debug for DiscordNotificationConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiscordNotificationConfig")
            .field("webhook_configured", &self.webhook.is_some())
            .field("username", &self.username)
            .field("avatar_url", &self.avatar_url)
            .field("on_script_start", &self.on_script_start)
            .field("on_script_end", &self.on_script_end)
            .finish()
    }
}

/// Windows-only native notification flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowsNotificationConfig {
    pub on_script_start: bool,
    pub on_script_end: bool,
}

/// Complete immediately applied notification settings.
#[derive(Clone, Debug, Default)]
pub struct NotificationConfig {
    pub discord: DiscordNotificationConfig,
    pub windows: WindowsNotificationConfig,
}

/// Script lifecycle hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptNotificationEvent {
    Start,
    End,
}

/// Test-operation channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationChannel {
    Discord,
    Windows,
}

/// Per-channel non-fatal outcome returned to hooks and the test endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationOutcome {
    Delivered,
    Disabled,
    SkippedMissingConfiguration,
    SkippedUnsupportedPlatform,
    Failed,
}

/// One lifecycle/test invocation always returns both requested outcomes rather
/// than failing command execution.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationReport {
    pub outcomes: Vec<(NotificationChannel, NotificationOutcome)>,
}

#[derive(Serialize)]
struct DiscordPayload<'a> {
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<&'a str>,
}

/// Secret-safe HTTP delivery boundary.
#[async_trait]
pub trait DiscordTransport: Send + Sync {
    async fn send(
        &self,
        webhook: &DiscordWebhookUrl,
        content: &str,
        username: Option<&str>,
        avatar_url: Option<&Url>,
    ) -> Result<(), NotificationError>;
}

/// Reqwest implementation using rustls and a fixed JSON payload.
#[derive(Clone, Debug)]
pub struct ReqwestDiscordTransport {
    client: reqwest::Client,
}

impl ReqwestDiscordTransport {
    /// Builds the reusable HTTP client.
    ///
    /// # Errors
    ///
    /// Returns a fixed initialization failure.
    pub fn new() -> Result<Self, NotificationError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .build()
            .map_err(|_| NotificationError::DeliveryFailed)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl DiscordTransport for ReqwestDiscordTransport {
    async fn send(
        &self,
        webhook: &DiscordWebhookUrl,
        content: &str,
        username: Option<&str>,
        avatar_url: Option<&Url>,
    ) -> Result<(), NotificationError> {
        let response = self
            .client
            .post(webhook.as_url().clone())
            .json(&DiscordPayload {
                content,
                username,
                avatar_url: avatar_url.map(Url::as_str),
            })
            .send()
            .await
            .map_err(|_| NotificationError::DeliveryFailed)?;
        if response.status() == StatusCode::NO_CONTENT || response.status().is_success() {
            Ok(())
        } else {
            Err(NotificationError::DeliveryFailed)
        }
    }
}

/// Fail-soft transport used when the process cannot initialize its TLS trust
/// store. Notification attempts still produce the normal non-fatal failed
/// outcome without preventing the application from starting.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableDiscordTransport;

#[async_trait]
impl DiscordTransport for UnavailableDiscordTransport {
    async fn send(
        &self,
        _webhook: &DiscordWebhookUrl,
        _content: &str,
        _username: Option<&str>,
        _avatar_url: Option<&Url>,
    ) -> Result<(), NotificationError> {
        Err(NotificationError::DeliveryFailed)
    }
}

/// Native notification platform boundary.
#[async_trait]
pub trait NativeNotificationTransport: Send + Sync {
    fn is_supported(&self) -> bool;

    async fn show(&self, title: &str, body: &str) -> Result<(), NotificationError>;
}

/// Windows toast adapter. On non-Windows targets it retains settings but skips
/// delivery, as required by the cross-platform contract.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsNativeNotificationTransport;

#[async_trait]
impl NativeNotificationTransport for WindowsNativeNotificationTransport {
    fn is_supported(&self) -> bool {
        cfg!(target_os = "windows")
    }

    async fn show(&self, title: &str, body: &str) -> Result<(), NotificationError> {
        #[cfg(target_os = "windows")]
        {
            let title = title.to_owned();
            let body = body.to_owned();
            tokio::task::spawn_blocking(move || show_windows_toast(&title, &body))
                .await
                .map_err(|_| NotificationError::DeliveryFailed)??;
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (title, body);
            Err(NotificationError::UnsupportedPlatform)
        }
    }
}

#[cfg(target_os = "windows")]
fn show_windows_toast(title: &str, body: &str) -> Result<(), NotificationError> {
    use win32_notif::{NotificationBuilder, notification::visual::Text, notifier::ToastsNotifier};

    let notifier = ToastsNotifier::new(Some("PokeCon.Modified.Extension"))
        .map_err(|_| NotificationError::DeliveryFailed)?;
    let notification = NotificationBuilder::new()
        .visual(Text::create(0, title))
        .visual(Text::create(1, body))
        .build(0, &notifier, "pokecon", "script")
        .map_err(|_| NotificationError::DeliveryFailed)?;
    notification
        .show()
        .map_err(|_| NotificationError::DeliveryFailed)
}

/// Immediate notification settings plus lifecycle and test hooks.
pub struct NotificationService {
    config: RwLock<NotificationConfig>,
    discord: Arc<dyn DiscordTransport>,
    native: Arc<dyn NativeNotificationTransport>,
}

impl std::fmt::Debug for NotificationService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationService")
            .finish_non_exhaustive()
    }
}

impl NotificationService {
    #[must_use]
    pub fn new(
        config: NotificationConfig,
        discord: Arc<dyn DiscordTransport>,
        native: Arc<dyn NativeNotificationTransport>,
    ) -> Self {
        Self {
            config: RwLock::new(config),
            discord,
            native,
        }
    }

    /// Applies future-hook settings immediately.
    pub async fn update_config(&self, config: NotificationConfig) {
        *self.config.write().await = config;
    }

    /// Executes the script-start hook without propagating delivery failure.
    pub async fn on_script_start(&self, script_name: &str) -> NotificationReport {
        self.notify_lifecycle(ScriptNotificationEvent::Start, script_name)
            .await
    }

    /// Executes the script-end hook without propagating delivery failure.
    pub async fn on_script_end(&self, script_name: &str) -> NotificationReport {
        self.notify_lifecycle(ScriptNotificationEvent::End, script_name)
            .await
    }

    /// Executes one notification test independent of lifecycle enable flags.
    pub async fn test(&self, channel: NotificationChannel) -> NotificationReport {
        let config = self.config.read().await.clone();
        let outcome = match channel {
            NotificationChannel::Discord => {
                self.deliver_discord(&config.discord, "PokeCon notification test")
                    .await
            }
            NotificationChannel::Windows => {
                self.deliver_native("PokeCon", "Notification test").await
            }
        };
        NotificationReport {
            outcomes: vec![(channel, outcome)],
        }
    }

    /// Delivers script-originated Discord text through the currently effective
    /// canonical webhook without exposing the secret to the worker.
    pub async fn send_script_discord(&self, content: &str) -> NotificationOutcome {
        let config = self.config.read().await.clone();
        self.deliver_discord(&config.discord, content).await
    }

    async fn notify_lifecycle(
        &self,
        event: ScriptNotificationEvent,
        script_name: &str,
    ) -> NotificationReport {
        let config = self.config.read().await.clone();
        let content = match event {
            ScriptNotificationEvent::Start => format!("Script started: {script_name}"),
            ScriptNotificationEvent::End => format!("Script finished: {script_name}"),
        };
        let discord_enabled = match event {
            ScriptNotificationEvent::Start => config.discord.on_script_start,
            ScriptNotificationEvent::End => config.discord.on_script_end,
        };
        let windows_enabled = match event {
            ScriptNotificationEvent::Start => config.windows.on_script_start,
            ScriptNotificationEvent::End => config.windows.on_script_end,
        };
        let discord = if discord_enabled {
            self.deliver_discord(&config.discord, &content).await
        } else {
            NotificationOutcome::Disabled
        };
        let windows = if windows_enabled {
            self.deliver_native("PokeCon", &content).await
        } else {
            NotificationOutcome::Disabled
        };
        NotificationReport {
            outcomes: vec![
                (NotificationChannel::Discord, discord),
                (NotificationChannel::Windows, windows),
            ],
        }
    }

    async fn deliver_discord(
        &self,
        config: &DiscordNotificationConfig,
        content: &str,
    ) -> NotificationOutcome {
        let Some(webhook) = &config.webhook else {
            tracing::warn!(
                diagnostic_id = "NOTIFICATION_DISCORD_CONFIG_MISSING",
                "Discord notification skipped because no webhook is configured"
            );
            return NotificationOutcome::SkippedMissingConfiguration;
        };
        if self
            .discord
            .send(
                webhook,
                content,
                config.username.as_deref(),
                config.avatar_url.as_ref(),
            )
            .await
            .is_ok()
        {
            NotificationOutcome::Delivered
        } else {
            tracing::warn!(
                diagnostic_id = "NOTIFICATION_DISCORD_DELIVERY_FAILED",
                "Discord notification delivery failed"
            );
            NotificationOutcome::Failed
        }
    }

    async fn deliver_native(&self, title: &str, body: &str) -> NotificationOutcome {
        if !self.native.is_supported() {
            return NotificationOutcome::SkippedUnsupportedPlatform;
        }
        if self.native.show(title, body).await.is_ok() {
            NotificationOutcome::Delivered
        } else {
            tracing::warn!(
                diagnostic_id = "NOTIFICATION_WINDOWS_DELIVERY_FAILED",
                "Windows native notification delivery failed"
            );
            NotificationOutcome::Failed
        }
    }
}

/// Secret-safe notification validation/delivery failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NotificationError {
    #[error("Discord webhook URL is invalid")]
    InvalidWebhookUrl,
    #[error("notification delivery failed")]
    DeliveryFailed,
    #[error("native notifications are unsupported on this platform")]
    UnsupportedPlatform,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use url::Url;

    use super::{
        DiscordNotificationConfig, DiscordTransport, DiscordWebhookUrl,
        NativeNotificationTransport, NotificationChannel, NotificationConfig, NotificationError,
        NotificationOutcome, NotificationService, WindowsNotificationConfig,
    };

    #[derive(Debug)]
    struct FakeDiscord {
        calls: AtomicUsize,
        fails: bool,
    }

    #[async_trait]
    impl DiscordTransport for FakeDiscord {
        async fn send(
            &self,
            _webhook: &DiscordWebhookUrl,
            _content: &str,
            _username: Option<&str>,
            _avatar_url: Option<&Url>,
        ) -> Result<(), NotificationError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            if self.fails {
                Err(NotificationError::DeliveryFailed)
            } else {
                Ok(())
            }
        }
    }

    #[derive(Debug)]
    struct FakeNative {
        supported: bool,
        fails: bool,
    }

    #[async_trait]
    impl NativeNotificationTransport for FakeNative {
        fn is_supported(&self) -> bool {
            self.supported
        }

        async fn show(&self, _title: &str, _body: &str) -> Result<(), NotificationError> {
            if self.fails {
                Err(NotificationError::DeliveryFailed)
            } else {
                Ok(())
            }
        }
    }

    fn webhook() -> DiscordWebhookUrl {
        DiscordWebhookUrl::parse("https://discord.com/api/webhooks/123/super-secret-token").unwrap()
    }

    #[test]
    fn webhook_validation_is_strict_and_debug_is_secret_safe() {
        let value = webhook();
        assert!(!format!("{value:?}").contains("super-secret-token"));
        for invalid in [
            "http://discord.com/api/webhooks/1/token",
            "https://example.com/api/webhooks/1/token",
            "https://discord.com/api/webhooks/1",
            "https://discord.com/api/webhooks/1/token?wait=true",
        ] {
            assert!(DiscordWebhookUrl::parse(invalid).is_err());
        }
    }

    #[tokio::test]
    async fn failure_is_reported_but_never_propagated_to_script_hook() {
        let discord = Arc::new(FakeDiscord {
            calls: AtomicUsize::new(0),
            fails: true,
        });
        let service = NotificationService::new(
            NotificationConfig {
                discord: DiscordNotificationConfig {
                    webhook: Some(webhook()),
                    username: None,
                    avatar_url: None,
                    on_script_start: true,
                    on_script_end: false,
                },
                windows: WindowsNotificationConfig::default(),
            },
            discord.clone(),
            Arc::new(FakeNative {
                supported: false,
                fails: false,
            }),
        );
        let report = service.on_script_start("command.py").await;
        assert_eq!(
            report.outcomes,
            vec![
                (NotificationChannel::Discord, NotificationOutcome::Failed),
                (NotificationChannel::Windows, NotificationOutcome::Disabled),
            ]
        );
        assert_eq!(discord.calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn enabled_missing_webhook_is_skipped_once_per_event() {
        let discord = Arc::new(FakeDiscord {
            calls: AtomicUsize::new(0),
            fails: false,
        });
        let service = NotificationService::new(
            NotificationConfig {
                discord: DiscordNotificationConfig {
                    on_script_start: true,
                    ..DiscordNotificationConfig::default()
                },
                windows: WindowsNotificationConfig::default(),
            },
            discord.clone(),
            Arc::new(FakeNative {
                supported: false,
                fails: false,
            }),
        );
        let report = service.on_script_start("command.py").await;
        assert_eq!(
            report.outcomes[0],
            (
                NotificationChannel::Discord,
                NotificationOutcome::SkippedMissingConfiguration
            )
        );
        assert_eq!(discord.calls.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn test_operation_ignores_flags_and_non_windows_is_retained() {
        let service = NotificationService::new(
            NotificationConfig {
                discord: DiscordNotificationConfig {
                    webhook: Some(webhook()),
                    ..DiscordNotificationConfig::default()
                },
                windows: WindowsNotificationConfig {
                    on_script_start: true,
                    on_script_end: true,
                },
            },
            Arc::new(FakeDiscord {
                calls: AtomicUsize::new(0),
                fails: false,
            }),
            Arc::new(FakeNative {
                supported: false,
                fails: false,
            }),
        );
        assert_eq!(
            service.test(NotificationChannel::Discord).await.outcomes[0].1,
            NotificationOutcome::Delivered
        );
        assert_eq!(
            service.test(NotificationChannel::Windows).await.outcomes[0].1,
            NotificationOutcome::SkippedUnsupportedPlatform
        );
    }
}
