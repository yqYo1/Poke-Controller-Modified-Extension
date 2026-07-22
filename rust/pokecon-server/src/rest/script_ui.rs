use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::routing::post;

use crate::api::{ScriptUiAction, ScriptUiActionResult, Success};

use super::{RestError, RestResult, RestState, json_request};

pub(super) fn router() -> Router<RestState> {
    Router::new().route("/api/script-ui/action", post(script_ui_action))
}

async fn script_ui_action(
    State(state): State<RestState>,
    request: Result<Json<ScriptUiAction>, JsonRejection>,
) -> RestResult<Json<Success<ScriptUiActionResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .script_ui_action(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
