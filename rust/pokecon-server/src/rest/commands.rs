use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::post;

use crate::api::{CommandControlRequest, EmptyRequest, OperationResult, Success};

use super::{RestError, RestResult, RestState, json_request};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route("/api/commands/control", post(control))
        .route("/api/commands/reload", post(reload))
}

async fn control(
    State(state): State<RestState>,
    request: Result<Json<CommandControlRequest>, JsonRejection>,
) -> RestResult<Json<Success<OperationResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .control_command(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn reload(
    State(state): State<RestState>,
    request: Result<Json<EmptyRequest>, JsonRejection>,
) -> RestResult<Json<Success<OperationResult>>> {
    let _request = json_request(request)?;
    let data = state
        .backend
        .reload_commands()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
