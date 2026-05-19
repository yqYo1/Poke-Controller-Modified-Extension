use axum::Json;

use utoipa::OpenApi;

// ═══════════════════════════════════════════════════════════════════════════════
// Core Endpoints
// ═══════════════════════════════════════════════════════════════════════════════

/// Status endpoint — returns basic info.
#[utoipa::path(
    get,
    path = "/api/status",
    tag = "core",
    responses(
        (status = 200, description = "API status information", body = serde_json::Value)
    )
)]
pub async fn api_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "shared"
    }))
}

/// Greet endpoint — same API in both modes.
#[utoipa::path(
    get,
    path = "/api/greet",
    tag = "core",
    params(
        ("name" = Option<String>, Query, description = "Name to greet"),
    ),
    responses(
        (status = 200, description = "Greeting message", body = serde_json::Value)
    )
)]
pub async fn api_greet(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let name = params.get("name").map(|s| s.as_str()).unwrap_or("Trainer");
    Json(serde_json::json!({
        "message": format!("Hello, {name}! Welcome to Poke-Controller.")
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// OpenAPI / Swagger Endpoint
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/openapi.json — return the OpenAPI specification as JSON
#[utoipa::path(
    get,
    path = "/api/openapi.json",
    tag = "core",
    responses(
        (status = 200, description = "OpenAPI specification in JSON format", body = serde_json::Value)
    )
)]
pub async fn api_openapi_json() -> Json<serde_json::Value> {
    Json(serde_json::to_value(crate::api_doc::ApiDoc::openapi()).unwrap())
}
