use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::handlers::NameRequest;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Command Management Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// List all available (loaded) commands.
#[utoipa::path(
    get,
    path = "/api/commands",
    tag = "commands",
    responses(
        (status = 200, description = "List of commands", body = serde_json::Value)
    )
)]
pub async fn commands_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cm = state.command_manager.lock().await;
    let commands: Vec<serde_json::Value> = cm
        .list()
        .iter()
        .map(|info| {
            serde_json::json!({
                "name": info.name,
                "path": info.path.to_string_lossy(),
                "description": info.description,
            })
        })
        .collect();
    Json(serde_json::json!({ "commands": commands }))
}

/// Load a command by scanning the scripts directory for the given name.
#[utoipa::path(
    post,
    path = "/api/commands/load",
    tag = "commands",
    request_body = NameRequest,
    responses(
        (status = 200, description = "Command loaded", body = serde_json::Value)
    )
)]
pub async fn commands_load(
    State(state): State<AppState>,
    Json(body): Json<NameRequest>,
) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    // Scan the scripts directory to discover available commands
    match cm.scan() {
        Ok(names) => {
            if names.contains(&body.name) {
                Json(serde_json::json!({
                    "status": "ok",
                    "message": format!("Command '{}' loaded", body.name)
                }))
            } else if cm.get(&body.name).is_some() {
                Json(serde_json::json!({
                    "status": "ok",
                    "message": format!("Command '{}' already loaded", body.name)
                }))
            } else {
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Command '{}' not found in scripts directory", body.name)
                }))
            }
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to scan scripts: {e}")
        })),
    }
}

/// Start (activate) a command by name.
#[utoipa::path(
    post,
    path = "/api/commands/start",
    tag = "commands",
    request_body = NameRequest,
    responses(
        (status = 200, description = "Command started", body = serde_json::Value)
    )
)]
pub async fn commands_start(
    State(state): State<AppState>,
    Json(body): Json<NameRequest>,
) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    // Ensure command is loaded before activating
    if cm.get(&body.name).is_none() {
        let _ = cm.scan();
    }

    match cm.set_active(&body.name) {
        Ok(_) => {
            // Emit command lifecycle event
            let _ = state.event_tx.send(serde_json::json!({
                "type": "command.start",
                "data": { "name": &body.name },
            }));
            Json(serde_json::json!({
                "status": "ok",
                "message": format!("Command '{}' started", body.name)
            }))
        }
        Err(e) => {
            let _ = state.event_tx.send(serde_json::json!({
                "type": "command.error",
                "data": { "name": &body.name, "error": e.to_string() },
            }));
            Json(serde_json::json!({
                "status": "error",
                "message": e.to_string()
            }))
        }
    }
}

/// Stop the currently active command.
#[utoipa::path(
    post,
    path = "/api/commands/stop",
    tag = "commands",
    responses(
        (status = 200, description = "Command stopped", body = serde_json::Value)
    )
)]
pub async fn commands_stop(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    let name = cm.active_name().map(|s| s.to_string());
    cm.stop();

    if let Some(ref n) = name {
        let _ = state.event_tx.send(serde_json::json!({
            "type": "command.stop",
            "data": { "name": n },
        }));
    }

    Json(serde_json::json!({
        "status": "ok",
        "message": match name {
            Some(ref n) => format!("Command '{n}' stopped"),
            None => "No active command to stop".to_string(),
        }
    }))
}

/// Get information about the currently active command.
#[utoipa::path(
    get,
    path = "/api/commands/active",
    tag = "commands",
    responses(
        (status = 200, description = "Currently active command info", body = serde_json::Value)
    )
)]
pub async fn commands_active(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cm = state.command_manager.lock().await;
    match cm.active() {
        Some(info) => Json(serde_json::json!({
            "active": true,
            "name": info.name,
            "path": info.path.to_string_lossy(),
            "description": info.description,
        })),
        None => Json(serde_json::json!({
            "active": false,
        })),
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct FilterRequest {
    pub filter: String,
}

/// POST /api/commands/filter — set command filter string and return filtered list
#[utoipa::path(
    post,
    path = "/api/commands/filter",
    tag = "commands",
    request_body = FilterRequest,
    responses(
        (status = 200, description = "Filtered command list", body = serde_json::Value)
    )
)]
pub async fn commands_filter(
    State(state): State<AppState>,
    Json(body): Json<FilterRequest>,
) -> Json<serde_json::Value> {
    // Store the filter in shared state
    {
        let mut filter = state.command_filter.lock().await;
        *filter = body.filter.clone();
    }

    // Return filtered command list
    let cm = state.command_manager.lock().await;
    let filter_lower = body.filter.to_lowercase();
    let commands: Vec<serde_json::Value> = cm
        .list()
        .iter()
        .filter(|info| {
            if filter_lower.is_empty() {
                return true;
            }
            let name_match = info.name.to_lowercase().contains(&filter_lower);
            let desc_match = info
                .description
                .as_ref()
                .map(|d| d.to_lowercase().contains(&filter_lower))
                .unwrap_or(false);
            let path_match = info
                .path
                .to_string_lossy()
                .to_lowercase()
                .contains(&filter_lower);
            name_match || desc_match || path_match
        })
        .map(|info| {
            serde_json::json!({
                "name": info.name,
                "path": info.path.to_string_lossy(),
                "description": info.description,
            })
        })
        .collect();
    Json(serde_json::json!({ "status": "ok", "filter": body.filter, "commands": commands }))
}

/// POST /api/commands/reload — rescan the scripts directory and reload all commands
#[utoipa::path(
    post,
    path = "/api/commands/reload",
    tag = "commands",
    responses(
        (status = 200, description = "Commands reloaded", body = serde_json::Value)
    )
)]
pub async fn commands_reload(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut cm = state.command_manager.lock().await;
    match cm.scan() {
        Ok(names) => {
            let commands: Vec<serde_json::Value> = cm
                .list()
                .iter()
                .map(|info| {
                    serde_json::json!({
                        "name": info.name,
                        "path": info.path.to_string_lossy(),
                        "description": info.description,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "status": "ok",
                "message": format!("Scanned {} commands", names.len()),
                "commands": commands,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to reload commands: {e}"),
        })),
    }
}
