use axum::Router;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on};

use crate::server::api::{CommandPauseSnapshot, Success};

use super::{RestError, RestResult, RestState, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new().route(
        "/api/command-pause",
        on(MethodFilter::GET, snapshot).on(MethodFilter::HEAD, method_not_allowed),
    )
}

async fn snapshot(
    State(state): State<RestState>,
) -> RestResult<Json<Success<CommandPauseSnapshot>>> {
    let data = state
        .backend
        .command_pause()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
