use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on, post};

use crate::server::api::{
    ScriptCreatorPreview, ScriptCreatorResult, ScriptCreatorSnapshot, ScriptCreatorSpec, Success,
};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route(
            "/api/script-creator",
            on(MethodFilter::GET, snapshot).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route("/api/script-creator/preview", post(preview))
        .route("/api/script-creator/create", post(create))
}

async fn snapshot(
    State(state): State<RestState>,
) -> RestResult<Json<Success<ScriptCreatorSnapshot>>> {
    let data = state
        .backend
        .script_creator_snapshot()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn preview(
    State(state): State<RestState>,
    request: Result<Json<ScriptCreatorSpec>, JsonRejection>,
) -> RestResult<Json<Success<ScriptCreatorPreview>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .preview_script_creation(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn create(
    State(state): State<RestState>,
    request: Result<Json<ScriptCreatorSpec>, JsonRejection>,
) -> RestResult<Json<Success<ScriptCreatorResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .create_script(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
