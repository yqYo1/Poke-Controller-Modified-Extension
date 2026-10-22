use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on, post};

use crate::server::api::{CommandHistoryControlRequest, CommandHistorySnapshot, Success};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route(
            "/api/command-history",
            on(MethodFilter::GET, snapshot).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route("/api/command-history/control", post(control))
}

async fn snapshot(
    State(state): State<RestState>,
) -> RestResult<Json<Success<CommandHistorySnapshot>>> {
    let data = state
        .backend
        .command_history()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn control(
    State(state): State<RestState>,
    request: Result<Json<CommandHistoryControlRequest>, JsonRejection>,
) -> RestResult<Json<Success<CommandHistorySnapshot>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .control_command_history(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
