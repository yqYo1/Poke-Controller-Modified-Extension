use axum::Json;
use axum::extract::State;

use crate::handlers::NameRequest;
use crate::state::AppState;

// ═══════════════════════════════════════════════════════════════════════════════
// Profile Management Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/profile — list all available profiles
#[utoipa::path(
    get,
    path = "/api/profile",
    tag = "profile",
    responses(
        (status = 200, description = "List of profiles", body = serde_json::Value)
    )
)]
pub async fn profile_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let pm = state.profile_manager.lock().await;
    let profiles: Vec<serde_json::Value> = pm
        .list()
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "description": p.description,
                "active": pm.active_name() == Some(&p.name),
            })
        })
        .collect();
    Json(serde_json::json!({
        "status": "ok",
        "profiles": profiles,
        "active": pm.active_name(),
    }))
}

/// POST /api/profile — activate a profile by name
#[utoipa::path(
    post,
    path = "/api/profile",
    tag = "profile",
    request_body = NameRequest,
    responses(
        (status = 200, description = "Profile activated", body = serde_json::Value)
    )
)]
pub async fn profile_set(
    State(state): State<AppState>,
    Json(body): Json<NameRequest>,
) -> Json<serde_json::Value> {
    let mut pm = state.profile_manager.lock().await;
    match pm.activate(&body.name) {
        Ok(_) => Json(serde_json::json!({
            "status": "ok",
            "message": format!("Profile {} activated", body.name),
            "active": body.name,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}
