use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on, post};

use crate::server::api::{
    InputRecordControlRequest, InputRecordResult, InputRecordSnapshot, Success,
};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route(
            "/api/input-record",
            on(MethodFilter::GET, snapshot).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route("/api/input-record/control", post(control))
}

async fn snapshot(
    State(state): State<RestState>,
) -> RestResult<Json<Success<InputRecordSnapshot>>> {
    let data = state
        .backend
        .input_record()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn control(
    State(state): State<RestState>,
    request: Result<Json<InputRecordControlRequest>, JsonRejection>,
) -> RestResult<Json<Success<InputRecordResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .control_input_record(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
