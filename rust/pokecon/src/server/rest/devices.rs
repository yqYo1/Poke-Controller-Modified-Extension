use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, on, post};

use crate::server::api::{
    CameraDevice, EmptyRequest, OperationResult, SavedScreenshot, ScreenshotRequest,
    SerialControlRequest, SerialPort, Success,
};
use crate::server::backend::ScreenshotOutput;

use super::{
    RestError, RestResult, RestState, download_response, json_request, method_not_allowed,
};

pub(super) fn router() -> Router<RestState> {
    Router::new()
        .route(
            "/api/devices/cameras",
            on(MethodFilter::GET, cameras).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route(
            "/api/devices/serial-ports",
            on(MethodFilter::GET, serial_ports).on(MethodFilter::HEAD, method_not_allowed),
        )
        .route("/api/serial/control", post(control_serial))
        .route("/api/camera/retry", post(retry_camera))
        .route("/api/camera/screenshot", post(screenshot))
}

async fn cameras(State(state): State<RestState>) -> RestResult<Json<Success<Vec<CameraDevice>>>> {
    let data = state
        .backend
        .enumerate_cameras()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn serial_ports(
    State(state): State<RestState>,
) -> RestResult<Json<Success<Vec<SerialPort>>>> {
    let data = state
        .backend
        .enumerate_serial_ports()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn control_serial(
    State(state): State<RestState>,
    request: Result<Json<SerialControlRequest>, JsonRejection>,
) -> RestResult<Json<Success<OperationResult>>> {
    let request = json_request(request)?;
    let data = state
        .backend
        .control_serial(request)
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn retry_camera(
    State(state): State<RestState>,
    request: Result<Json<EmptyRequest>, JsonRejection>,
) -> RestResult<Json<Success<OperationResult>>> {
    let _request = json_request(request)?;
    let data = state
        .backend
        .retry_camera()
        .await
        .map_err(RestError::from)?;
    Ok(Json(Success { data }))
}

async fn screenshot(
    State(state): State<RestState>,
    request: Result<Json<ScreenshotRequest>, JsonRejection>,
) -> RestResult<Response> {
    let request = json_request(request)?;
    match state
        .backend
        .screenshot(request)
        .await
        .map_err(RestError::from)?
    {
        ScreenshotOutput::Saved(data) => {
            Ok(Json(Success::<SavedScreenshot> { data }).into_response())
        }
        ScreenshotOutput::Download(payload) => download_response(payload),
    }
}
