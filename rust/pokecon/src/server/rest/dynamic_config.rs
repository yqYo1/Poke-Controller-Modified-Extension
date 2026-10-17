use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::post;

use crate::server::api::{DynamicConfigControlRequest, DynamicConfigResult, Success};

use super::{RestError, RestResult, RestState, json_request};

pub(super) fn router() -> Router<RestState> {
    Router::new().route("/api/dynamic-config/control", post(control))
}

async fn control(
    State(state): State<RestState>,
    request: Result<Json<DynamicConfigControlRequest>, JsonRejection>,
) -> RestResult<Json<Success<DynamicConfigResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .control_dynamic_config(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
