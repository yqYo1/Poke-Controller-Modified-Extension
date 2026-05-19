use axum::Json;
use axum::extract::State;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use pokecon_core::notify::discord::DiscordNotifier;
use pokecon_core::notify::windows::DesktopNotifier;
use pokecon_core::notify::{Notification, Notifier};

use crate::helpers;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Notification Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/notifications/config — get current notification settings
#[utoipa::path(
    get,
    path = "/api/notifications/config",
    tag = "notifications",
    responses(
        (status = 200, description = "Current notification settings", body = serde_json::Value)
    )
)]
pub async fn notifications_get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cfg = state.notification_config.lock().await;
    Json(serde_json::json!({
        "status": "ok",
        "windows_enabled": cfg.windows_enabled,
        "discord_enabled": cfg.discord_enabled,
        "discord_webhook_url": cfg.discord_webhook_url.as_deref().map(helpers::mask_secret),
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct NotificationConfigRequest {
    pub windows_enabled: Option<bool>,
    pub discord_enabled: Option<bool>,
    pub discord_webhook_url: Option<String>,
}

/// POST /api/notifications/config — update notification settings
#[utoipa::path(
    post,
    path = "/api/notifications/config",
    tag = "notifications",
    request_body = NotificationConfigRequest,
    responses(
        (status = 200, description = "Notification config updated", body = serde_json::Value)
    )
)]
pub async fn notifications_set_config(
    State(state): State<AppState>,
    Json(body): Json<NotificationConfigRequest>,
) -> Json<serde_json::Value> {
    let mut cfg = state.notification_config.lock().await;
    if let Some(v) = body.windows_enabled {
        cfg.windows_enabled = v;
    }
    if let Some(v) = body.discord_enabled {
        cfg.discord_enabled = v;
    }
    if let Some(v) = body.discord_webhook_url {
        // SSRF protection: validate webhook URL
        if let Err(e) = helpers::validate_webhook_url(&v) {
            return Json(serde_json::json!({
                "status": "error",
                "message": format!("Invalid discord_webhook_url: {e}"),
            }));
        }
        cfg.discord_webhook_url = Some(v);
    }
    tracing::info!("Notification config updated");
    Json(serde_json::json!({
        "status": "ok",
        "message": "Notification config updated",
    }))
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SendNotificationRequest {
    /// Notification message text
    pub message: String,
    /// Optional title
    pub title: Option<String>,
}

/// POST /api/notifications/send — send a test notification using current settings
#[utoipa::path(
    post,
    path = "/api/notifications/send",
    tag = "notifications",
    request_body = SendNotificationRequest,
    responses(
        (status = 200, description = "Notification sent", body = serde_json::Value)
    )
)]
pub async fn notifications_send(
    State(state): State<AppState>,
    Json(body): Json<SendNotificationRequest>,
) -> Json<serde_json::Value> {
    let cfg = state.notification_config.lock().await;
    let notification = Notification::new(&body.message)
        .with_title(body.title.unwrap_or_else(|| "Poke-Controller".to_string()));

    // Extract config values before spawning tasks
    let windows_enabled = cfg.windows_enabled;
    let discord_enabled = cfg.discord_enabled;
    let discord_url = cfg.discord_webhook_url.clone().unwrap_or_default();
    drop(cfg); // Release the lock before spawning parallel tasks

    let mut handles: Vec<tokio::task::JoinHandle<serde_json::Value>> = Vec::new();

    // Windows desktop notification — parallel task
    if windows_enabled {
        let notif = notification.clone();
        handles.push(tokio::spawn(async move {
            let notifier = DesktopNotifier::new("Poke-Controller");
            match notifier.send(&notif).await {
                Ok(_) => serde_json::json!({"channel": "windows", "status": "sent"}),
                Err(e) => serde_json::json!({"channel": "windows", "status": "error", "error": e.to_string()}),
            }
        }));
    }

    // Discord webhook notification — parallel task
    if discord_enabled && !discord_url.is_empty() {
        // SSRF protection: validate URL before sending
        if let Err(e) = helpers::validate_webhook_url(&discord_url) {
            handles.push(tokio::spawn(async move {
                serde_json::json!({
                    "channel": "discord",
                    "status": "error",
                    "error": format!("Invalid webhook URL: {e}"),
                })
            }));
        } else {
            let notif = notification.clone();
            handles.push(tokio::spawn(async move {
                let notifier = DiscordNotifier::new(discord_url);
                match notifier.send(&notif).await {
                    Ok(_) => serde_json::json!({"channel": "discord", "status": "sent"}),
                    Err(e) => serde_json::json!({"channel": "discord", "status": "error", "error": e.to_string()}),
                }
            }));
        }
    }

    // Wait for all notification tasks to complete in parallel
    let results: Vec<serde_json::Value> = join_all(handles)
        .await
        .into_iter()
        .map(|r| {
            r.unwrap_or_else(|e| {
                serde_json::json!({
                    "channel": "unknown",
                    "status": "error",
                    "error": format!("notification task panicked: {e}"),
                })
            })
        })
        .collect();

    Json(serde_json::json!({
        "status": "ok",
        "results": results,
    }))
}
