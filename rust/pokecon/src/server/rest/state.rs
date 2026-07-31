use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use crate::server::api::{StateSnapshot, Success};

use super::RestState;

pub(super) fn router() -> Router<RestState> {
    Router::new().route("/api/state", get(get_state))
}

async fn get_state(State(state): State<RestState>) -> Json<Success<StateSnapshot>> {
    Json(Success {
        data: state.backend.state_hub().state_snapshot().await,
    })
}
