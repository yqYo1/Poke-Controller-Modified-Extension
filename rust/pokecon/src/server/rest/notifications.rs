use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::post;

use crate::server::api::{NotificationTestRequest, NotificationTestResult, Success};

use super::{RestError, RestResult, RestState, json_request};

pub(super) fn router() -> Router<RestState> {
    Router::new().route("/api/notifications/test", post(test_notification))
}

async fn test_notification(
    State(state): State<RestState>,
    request: Result<Json<NotificationTestRequest>, JsonRejection>,
) -> RestResult<Json<Success<NotificationTestResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .test_notification(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
