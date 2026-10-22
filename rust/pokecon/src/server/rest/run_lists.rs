use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on};

use crate::server::api::{RunListSaveRequest, RunListSnapshot, Success};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new().route(
        "/api/run-lists",
        on(MethodFilter::GET, snapshot)
            .on(MethodFilter::HEAD, method_not_allowed)
            .put(save),
    )
}

async fn snapshot(State(state): State<RestState>) -> RestResult<Json<Success<RunListSnapshot>>> {
    let data = state.backend.run_lists().await.map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn save(
    State(state): State<RestState>,
    request: Result<Json<RunListSaveRequest>, JsonRejection>,
) -> RestResult<Json<Success<RunListSnapshot>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .save_run_lists(request.lists)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
