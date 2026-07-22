use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::post;

use crate::api::{EmptyRequest, Success, UpdateCheckResult};

use super::{RestError, RestResult, RestState, json_request};

pub(super) fn router() -> Router<RestState> {
    Router::new().route("/api/update/check", post(check))
}

async fn check(
    State(state): State<RestState>,
    request: Result<Json<EmptyRequest>, JsonRejection>,
) -> RestResult<Json<Success<UpdateCheckResult>>> {
    let _request = json_request(request)?;
    let data = state
        .backend
        .check_update()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
