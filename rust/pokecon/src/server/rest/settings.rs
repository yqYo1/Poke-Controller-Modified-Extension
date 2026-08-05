use axum::Router;
use axum::extract::{Json, State};
use axum::routing::{MethodFilter, on};

use crate::server::api::{SettingsPatchRequest, SettingsSnapshot, Success};

use super::{RestError, RestResult, RestState, json_request, method_not_allowed};

pub(super) fn router() -> Router<RestState> {
    Router::new().route(
        "/api/settings",
        on(MethodFilter::GET, get_settings)
            .on(MethodFilter::HEAD, method_not_allowed)
            .patch(patch_settings),
    )
}

async fn get_settings(State(state): State<RestState>) -> Json<Success<SettingsSnapshot>> {
    Json(Success {
        data: state.backend.state_hub().settings_snapshot().await,
    })
}

async fn patch_settings(
    State(state): State<RestState>,
    request: Result<Json<SettingsPatchRequest>, axum::extract::rejection::JsonRejection>,
) -> RestResult<Json<Success<SettingsSnapshot>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .patch_settings(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}
