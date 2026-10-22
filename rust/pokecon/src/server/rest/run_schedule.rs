use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on, post};

use crate::server::api::{RunScheduleControlRequest, RunScheduleSnapshot, Success};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route(
            "/api/run-schedule",
            on(MethodFilter::GET, snapshot).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route("/api/run-schedule/control", post(control))
}

async fn snapshot(
    State(state): State<RestState>,
) -> RestResult<Json<Success<RunScheduleSnapshot>>> {
    let data = state
        .backend
        .run_schedule()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn control(
    State(state): State<RestState>,
    request: Result<Json<RunScheduleControlRequest>, JsonRejection>,
) -> RestResult<Json<Success<RunScheduleSnapshot>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .control_run_schedule(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
