use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::response::{IntoResponse, Response};
use axum::routing::post;

use crate::api::{GenerateLauncherRequest, GenerateLauncherResult, Success};
use crate::backend::LauncherOutput;

use super::{RestError, RestResult, RestState, download_response, json_request};

pub(super) fn router() -> Router<RestState> {
    Router::new().route("/api/profiles/generate-launcher", post(generate_launcher))
}

async fn generate_launcher(
    State(state): State<RestState>,
    request: Result<Json<GenerateLauncherRequest>, JsonRejection>,
) -> RestResult<Response> {
    let request = json_request(request)?;
    match state
        .backend
        .generate_launcher(request)
        .await
        .map_err(RestError::from)?
    {
        LauncherOutput::Generated(data) => {
            Ok(Json(Success::<GenerateLauncherResult> { data }).into_response())
        }
        LauncherOutput::Download(payload) => download_response(payload),
    }
}
