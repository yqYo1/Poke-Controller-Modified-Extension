use axum::extract::State;
use axum::routing::{MethodFilter, on};
use axum::{Json, Router};

use crate::server::api::{StateSnapshot, Success};

use super::{RestState, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new().route(
        "/api/state",
        on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
    )
}

async fn get_state(State(state): State<RestState>) -> Json<Success<StateSnapshot>> {
    Json(Success {
        data: state.backend.state_hub().state_snapshot().await,
    })
}
