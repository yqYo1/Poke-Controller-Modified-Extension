//! Closed axum routing and common REST response handling.

mod commands;
mod devices;
mod dynamic_config;
mod notifications;
mod profiles;
mod script_ui;
mod settings;
mod state;
mod update;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde::de::DeserializeOwned;

use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};
use crate::server::backend::{ApiFailure, ApiFailureStatus, DownloadPayload, RestBackend};

#[derive(Clone)]
pub(crate) struct RestState {
    backend: Arc<dyn RestBackend>,
}

/// Builds every normative `/api` route around one application backend.
pub fn router(backend: Arc<dyn RestBackend>) -> Router {
    let state = RestState { backend };
    Router::<RestState>::new()
        .merge(settings::router())
        .merge(commands::router())
        .merge(devices::router())
        .merge(notifications::router())
        .merge(dynamic_config::router())
        .merge(profiles::router())
        .merge(script_ui::router())
        .merge(state::router())
        .merge(update::router())
        .route("/api", any(not_found))
        .route("/api/{*path}", any(not_found))
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
}

pub(crate) type RestResult<T> = Result<T, RestError>;

#[derive(Debug)]
pub(crate) struct RestError {
    status: StatusCode,
    error: ApiError,
}

impl RestError {
    fn new(status: StatusCode, code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            error: ApiError {
                code,
                message: message.into(),
                fields: None,
            },
        }
    }

    fn with_field(mut self, field: &str, diagnostic: &str) -> Self {
        self.error.fields = Some(BTreeMap::from([(
            field.to_owned(),
            vec![diagnostic.to_owned()],
        )]));
        self
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
            "internal server error",
        )
    }
}

impl From<ApiFailure> for RestError {
    fn from(failure: ApiFailure) -> Self {
        let status = match failure.status() {
            ApiFailureStatus::NotFound => StatusCode::NOT_FOUND,
            ApiFailureStatus::Conflict => StatusCode::CONFLICT,
            ApiFailureStatus::UnprocessableEntity => StatusCode::UNPROCESSABLE_ENTITY,
            ApiFailureStatus::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            error: failure.into_error(),
        }
    }
}

impl IntoResponse for RestError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorEnvelope { error: self.error })).into_response()
    }
}

pub(crate) fn json_request<T>(request: Result<Json<T>, JsonRejection>) -> RestResult<T>
where
    T: DeserializeOwned,
{
    request.map(|Json(value)| value).map_err(|rejection| {
        let status = rejection.status();
        if status == StatusCode::BAD_REQUEST {
            RestError::new(
                status,
                ApiErrorCode::MalformedJson,
                "request body is not valid JSON",
            )
        } else if status == StatusCode::PAYLOAD_TOO_LARGE {
            RestError::new(
                status,
                ApiErrorCode::PayloadTooLarge,
                "request body is too large",
            )
        } else if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
            RestError::new(
                status,
                ApiErrorCode::UnsupportedMediaType,
                "Content-Type must be application/json",
            )
        } else {
            RestError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiErrorCode::InvalidRequest,
                "request body does not match the endpoint schema",
            )
            .with_field("body", "request body does not match the endpoint schema")
        }
    })
}

pub(crate) fn download_response(payload: DownloadPayload) -> RestResult<Response> {
    let content_type = HeaderValue::from_static(payload.media_type().content_type());
    let disposition = HeaderValue::from_str(&content_disposition(payload.filename()))
        .map_err(|_error| RestError::internal())?;
    let mut response = Response::new(Body::from(payload.into_bytes()));
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert(CONTENT_DISPOSITION, disposition);
    Ok(response)
}

fn content_disposition(filename: &str) -> String {
    let fallback = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let fallback = if fallback.is_empty() {
        "download"
    } else {
        &fallback
    };
    format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
        rfc5987(filename)
    )
}

fn rfc5987(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
        {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

async fn not_found() -> RestError {
    RestError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::ResourceNotFound,
        "API resource was not found",
    )
}

async fn method_not_allowed() -> RestError {
    RestError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        ApiErrorCode::MethodNotAllowed,
        "HTTP method is not allowed for this resource",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::body::{Body, to_bytes};
    use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tower::ServiceExt as _;

    use super::router;
    use crate::server::api::{
        ApiErrorCode, CameraDevice, CameraSelector, CommandControlRequest, CommandState,
        DecimalString, DynamicConfigControlRequest, DynamicConfigResult, DynamicLanguage,
        GenerateLauncherRequest, GenerateLauncherResult, ImageFormat, NotificationTestRequest,
        NotificationTestResult, OperationResult, SavedScreenshot, ScreenshotRequest,
        SerialControlRequest, SerialPort, SettingsChange, SettingsPatchRequest, SettingsReadValues,
        SettingsSnapshot, SettingsWriteValues, StateChangeCause, StatePatch, StateSnapshot,
        UpdateCheckResult,
    };
    use crate::server::backend::{
        ApiFailure, ApiFailureStatus, ApiResult, DownloadMediaType, DownloadPayload,
        LauncherOutput, RestBackend, ScreenshotOutput,
    };
    use crate::server::state::{StateHub, StateTransaction};
    use crate::server::{
        router::public_router, security::RequestSecurity, static_files::StaticFiles,
    };

    struct MockBackend {
        hub: StateHub,
        fail_update: bool,
        attachment_name: Option<String>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                hub: test_hub(),
                fail_update: false,
                attachment_name: None,
            }
        }

        async fn operation(
            &self,
            cause: StateChangeCause,
            state: StatePatch,
        ) -> ApiResult<OperationResult> {
            let mut transaction = StateTransaction::new(cause);
            transaction.state = state;
            let outcome = self.hub.commit(transaction).await.map_err(|_error| {
                ApiFailure::new(
                    ApiFailureStatus::InternalServerError,
                    ApiErrorCode::InternalError,
                    "state transaction failed",
                )
            })?;
            Ok(OperationResult {
                changed: outcome.changed(),
                revision: outcome.revision().clone(),
            })
        }
    }

    #[async_trait]
    impl RestBackend for MockBackend {
        fn state_hub(&self) -> &StateHub {
            &self.hub
        }

        async fn patch_settings(
            &self,
            request: SettingsPatchRequest,
        ) -> ApiResult<SettingsSnapshot> {
            let mut transaction = StateTransaction::new(StateChangeCause::Settings);
            transaction.expected_revision = request.expected_revision;
            transaction.settings = Some(SettingsChange {
                values: request.values,
                ..SettingsChange::default()
            });
            self.hub
                .commit(transaction)
                .await
                .map(|outcome| outcome.snapshots.settings)
                .map_err(|_error| {
                    ApiFailure::new(
                        ApiFailureStatus::Conflict,
                        ApiErrorCode::RevisionConflict,
                        "settings revision conflict",
                    )
                })
        }

        async fn control_command(
            &self,
            _request: CommandControlRequest,
        ) -> ApiResult<OperationResult> {
            self.operation(
                StateChangeCause::Command,
                StatePatch {
                    last_input: Some(Some("command-control".to_owned())),
                    ..StatePatch::default()
                },
            )
            .await
        }

        async fn reload_commands(&self) -> ApiResult<OperationResult> {
            let revision = self.hub.state_snapshot().await.revision;
            Ok(OperationResult {
                changed: false,
                revision,
            })
        }

        async fn enumerate_cameras(&self) -> ApiResult<Vec<CameraDevice>> {
            Ok(vec![CameraDevice {
                selector: CameraSelector::Index(0),
                label: "Camera 0".to_owned(),
                available: true,
            }])
        }

        async fn enumerate_serial_ports(&self) -> ApiResult<Vec<SerialPort>> {
            Ok(vec![SerialPort {
                selector: "COM1".to_owned(),
                label: "COM1".to_owned(),
                available: true,
            }])
        }

        async fn control_serial(
            &self,
            _request: SerialControlRequest,
        ) -> ApiResult<OperationResult> {
            let revision = self.hub.state_snapshot().await.revision;
            Ok(OperationResult {
                changed: false,
                revision,
            })
        }

        async fn retry_camera(&self) -> ApiResult<OperationResult> {
            let revision = self.hub.state_snapshot().await.revision;
            Ok(OperationResult {
                changed: false,
                revision,
            })
        }

        async fn screenshot(&self, _request: ScreenshotRequest) -> ApiResult<ScreenshotOutput> {
            if let Some(filename) = &self.attachment_name {
                Ok(ScreenshotOutput::Download(DownloadPayload::new(
                    filename,
                    DownloadMediaType::Png,
                    vec![1, 2, 3],
                )))
            } else {
                Ok(ScreenshotOutput::Saved(SavedScreenshot {
                    display_path: "capture.png".to_owned(),
                    format: ImageFormat::Png,
                }))
            }
        }

        async fn test_notification(
            &self,
            _request: NotificationTestRequest,
        ) -> ApiResult<NotificationTestResult> {
            Ok(NotificationTestResult { delivered: true })
        }

        async fn control_dynamic_config(
            &self,
            _request: DynamicConfigControlRequest,
        ) -> ApiResult<DynamicConfigResult> {
            Ok(DynamicConfigResult {
                display_path: "init.py".to_owned(),
                language: DynamicLanguage::Python,
                loaded: true,
            })
        }

        async fn generate_launcher(
            &self,
            _request: GenerateLauncherRequest,
        ) -> ApiResult<LauncherOutput> {
            Ok(LauncherOutput::Generated(GenerateLauncherResult {
                profile_created: false,
                launcher_created: false,
                revision: self.hub.state_snapshot().await.revision,
            }))
        }

        async fn check_update(&self) -> ApiResult<UpdateCheckResult> {
            if self.fail_update {
                Err(ApiFailure::new(
                    ApiFailureStatus::InternalServerError,
                    ApiErrorCode::BackendUnavailable,
                    "update service is unavailable",
                ))
            } else {
                Ok(UpdateCheckResult {
                    current_version: "1.0.0".to_owned(),
                    latest_version: "1.0.0".to_owned(),
                    update_available: false,
                    release_url: "https://example.invalid/releases".to_owned(),
                })
            }
        }
    }

    fn test_hub() -> StateHub {
        let settings = SettingsSnapshot {
            revision: DecimalString::zero(),
            values: SettingsReadValues(BTreeMap::from([("sample.mode".to_owned(), json!("old"))])),
            pending_restart_values: SettingsWriteValues::default(),
            restart_required: Vec::new(),
            apply_failures: BTreeMap::new(),
        };
        let state = StateSnapshot {
            revision: DecimalString::zero(),
            serial_port: None,
            serial_baud_rate: 115_200,
            serial_connected: false,
            camera_opened: false,
            camera_fps: 0.0,
            camera_resolution: "1280x720".to_owned(),
            camera_device: CameraSelector::Index(0),
            is_running: false,
            command_state: CommandState::Stopped,
            current_command: None,
            command_candidates: Vec::new(),
            tags: Vec::new(),
            active_profile: "default".to_owned(),
            pending_profile: None,
            available_profiles: vec!["default".to_owned()],
            last_input: None,
            holding_buttons: Vec::new(),
            pid: 42,
            command_display_lists: BTreeMap::from([("-".to_owned(), Vec::new())]),
            command_display_cache_loading: false,
        };
        StateHub::new(settings, state, 16).expect("valid state hub")
    }

    fn json_request(method: Method, path: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .expect("request")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("bounded response body");
        serde_json::from_slice(&bytes).expect("JSON response")
    }

    #[tokio::test]
    async fn every_normative_rest_route_is_bound() {
        let app = router(Arc::new(MockBackend::new()));
        let requests = [
            json_request(Method::GET, "/api/settings", ""),
            json_request(Method::PATCH, "/api/settings", r#"{"values":{}}"#),
            json_request(Method::GET, "/api/state", ""),
            json_request(
                Method::POST,
                "/api/commands/control",
                r#"{"action":"stop"}"#,
            ),
            json_request(Method::POST, "/api/commands/reload", "{}"),
            json_request(Method::GET, "/api/devices/cameras", ""),
            json_request(Method::GET, "/api/devices/serial-ports", ""),
            json_request(
                Method::POST,
                "/api/serial/control",
                r#"{"action":"disconnect"}"#,
            ),
            json_request(Method::POST, "/api/camera/retry", "{}"),
            json_request(
                Method::POST,
                "/api/camera/screenshot",
                r#"{"destination":"captures"}"#,
            ),
            json_request(
                Method::POST,
                "/api/notifications/test",
                r#"{"channel":"windows"}"#,
            ),
            json_request(
                Method::POST,
                "/api/dynamic-config/control",
                r#"{"action":"load_content","language":"python","content":""}"#,
            ),
            json_request(
                Method::POST,
                "/api/profiles/generate-launcher",
                r#"{"profile":"default","copy_current":false,"destination":{"kind":"path","path":"C:\\launcher.bat"}}"#,
            ),
            json_request(Method::POST, "/api/update/check", "{}"),
        ];
        for request in requests {
            let path = request.uri().path().to_owned();
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::OK, "route {path}");
        }
    }

    #[tokio::test]
    async fn operation_response_snapshots_and_event_share_the_committed_revision() {
        let backend = Arc::new(MockBackend::new());
        let mut events = backend.hub.subscribe();
        let app = router(backend);
        let response = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                "/api/commands/control",
                r#"{"action":"stop"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let operation = response_json(response).await;
        assert_eq!(operation.pointer("/data/revision"), Some(&json!("1")));
        assert_eq!(events.recv().await.expect("event").revision.as_str(), "1");

        for path in ["/api/settings", "/api/state"] {
            let response = app
                .clone()
                .oneshot(json_request(Method::GET, path, ""))
                .await
                .expect("response");
            assert_eq!(
                response_json(response).await.pointer("/data/revision"),
                Some(&json!("1"))
            );
        }
    }

    #[tokio::test]
    async fn json_rejections_use_the_common_secret_safe_envelope() {
        let app = router(Arc::new(MockBackend::new()));
        for (body, expected_status, expected_code) in [
            ("{", StatusCode::BAD_REQUEST, ApiErrorCode::MalformedJson),
            (
                r#"{"unknown":true}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiErrorCode::InvalidRequest,
            ),
        ] {
            let response = app
                .clone()
                .oneshot(json_request(Method::POST, "/api/commands/reload", body))
                .await
                .expect("response");
            assert_eq!(response.status(), expected_status);
            let body = response_json(response).await;
            assert_eq!(
                body.pointer("/error/code"),
                Some(&serde_json::to_value(expected_code).expect("code"))
            );
            assert!(body.get("data").is_none());
        }
    }

    #[tokio::test]
    async fn unknown_resources_and_methods_have_common_errors() {
        let app = router(Arc::new(MockBackend::new()));
        for (request, status, code) in [
            (
                json_request(Method::GET, "/api/unknown", ""),
                StatusCode::NOT_FOUND,
                ApiErrorCode::ResourceNotFound,
            ),
            (
                json_request(Method::DELETE, "/api/state", "{}"),
                StatusCode::METHOD_NOT_ALLOWED,
                ApiErrorCode::MethodNotAllowed,
            ),
        ] {
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), status);
            assert_eq!(
                response_json(response).await.pointer("/error/code"),
                Some(&serde_json::to_value(code).expect("code"))
            );
        }
    }

    #[tokio::test]
    async fn api_catch_all_and_spa_fallback_compose_without_overlap() {
        let root = tempdir().expect("static root");
        fs::write(root.path().join("index.html"), "spa").expect("index");
        let app = public_router(
            router(Arc::new(MockBackend::new())),
            StaticFiles::new(root.path()).expect("static files"),
            RequestSecurity::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8020),
                false,
            ),
        );

        let spa = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .header("host", "localhost:8020")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(spa.status(), StatusCode::OK);
        assert_eq!(to_bytes(spa.into_body(), 16).await.expect("body"), "spa");

        let api = app
            .oneshot(
                Request::builder()
                    .uri("/api/unknown")
                    .header("host", "localhost:8020")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(api.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response_json(api).await.pointer("/error/code"),
            Some(&json!("resource_not_found"))
        );
    }

    #[tokio::test]
    async fn backend_failures_preserve_status_and_redacted_contract() {
        let app = router(Arc::new(MockBackend {
            fail_update: true,
            ..MockBackend::new()
        }));
        let response = app
            .oneshot(json_request(Method::POST, "/api/update/check", "{}"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = response_json(response).await;
        assert_eq!(
            body.pointer("/error/code"),
            Some(&json!("backend_unavailable"))
        );
        assert_eq!(
            body.pointer("/error/message"),
            Some(&json!("update service is unavailable"))
        );
    }

    #[tokio::test]
    async fn attachment_headers_encode_untrusted_filename_bytes() {
        let app = router(Arc::new(MockBackend {
            attachment_name: Some("unsafe\"\r\nX-Injected: yes.png".to_owned()),
            ..MockBackend::new()
        }));
        let response = app
            .oneshot(json_request(
                Method::POST,
                "/api/camera/screenshot",
                r#"{"destination":"download","format":"png"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/png");
        let disposition = response.headers()[CONTENT_DISPOSITION]
            .to_str()
            .expect("ASCII header");
        assert!(!disposition.contains('\r'));
        assert!(!disposition.contains('\n'));
        assert!(disposition.contains("%0D%0A"));
        assert_eq!(
            to_bytes(response.into_body(), 16).await.expect("body"),
            [1, 2, 3].as_slice()
        );
    }
}
