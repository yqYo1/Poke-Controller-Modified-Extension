use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on, post};

use crate::server::api::{CommandTagRequest, CommandTagResult, CommandTagSnapshot, Success};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route(
            "/api/command-tags",
            on(MethodFilter::GET, snapshot).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route("/api/command-tags/apply", post(apply))
}

async fn snapshot(State(state): State<RestState>) -> RestResult<Json<Success<CommandTagSnapshot>>> {
    let data = state
        .backend
        .command_tags()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn apply(
    State(state): State<RestState>,
    request: Result<Json<CommandTagRequest>, JsonRejection>,
) -> RestResult<Json<Success<CommandTagResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .apply_command_tags(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
