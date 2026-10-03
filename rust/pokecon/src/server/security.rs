//! Host, Origin, CORS, and anti-simple-request enforcement.

use std::collections::BTreeSet;
use std::net::SocketAddr;

use axum::extract::{Request, State};
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD, CONTENT_TYPE, HOST, ORIGIN,
    VARY,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};

const REQUEST_MARKER: HeaderName = HeaderName::from_static("x-pokecon-request");
const ALLOWED_METHODS: &str = "GET, PATCH, POST, OPTIONS";
const ALLOWED_HEADERS: &str = "Content-Type, X-Pokecon-Request";

/// Request policy derived only from the effective bound address and UI mode.
#[derive(Clone, Debug)]
pub struct RequestSecurity {
    allowed_hosts: BTreeSet<String>,
    allowed_origins: BTreeSet<String>,
}

impl RequestSecurity {
    #[must_use]
    pub fn new(address: SocketAddr, desktop_mode: bool) -> Self {
        let authority = address.to_string();
        let mut allowed_hosts = BTreeSet::from([authority.clone()]);
        let mut allowed_origins = BTreeSet::from([::std::format!("http://{authority}")]);
        if address.ip().is_loopback() {
            let localhost = ::std::format!("localhost:{}", address.port());
            allowed_origins.insert(::std::format!("http://{localhost}"));
            allowed_hosts.insert(localhost);
        }
        if desktop_mode {
            allowed_origins.insert("tauri://localhost".to_owned());
        }
        Self {
            allowed_hosts,
            allowed_origins,
        }
    }

    fn validate(
        &self,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Result<SecurityDecision, SecurityError> {
        let host = required_header(headers, &HOST)?;
        let host = host.to_str().map_err(|_error| SecurityError::Forbidden)?;
        if !self
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
        {
            return Err(SecurityError::Forbidden);
        }
        if let Some(authority) = uri.authority()
            && !self
                .allowed_hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(authority.as_str()))
        {
            return Err(SecurityError::Forbidden);
        }

        let origin = optional_header(headers, &ORIGIN)?;
        let origin = origin
            .map(|value| {
                let text = value.to_str().map_err(|_error| SecurityError::Forbidden)?;
                self.allowed_origins
                    .contains(text)
                    .then(|| value.clone())
                    .ok_or(SecurityError::Forbidden)
            })
            .transpose()?;

        if method == Method::OPTIONS {
            let origin = origin.ok_or(SecurityError::Forbidden)?;
            validate_preflight(uri.path(), headers)?;
            return Ok(SecurityDecision::Preflight { origin });
        }
        if uri.path() == "/ws" && origin.is_none() {
            return Err(SecurityError::Forbidden);
        }
        if is_mutating(method) {
            let content_type = optional_header(headers, &CONTENT_TYPE)?
                .ok_or(SecurityError::UnsupportedMediaType)?;
            let content_type = content_type
                .to_str()
                .map_err(|_error| SecurityError::UnsupportedMediaType)?;
            if !content_type
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
            {
                return Err(SecurityError::UnsupportedMediaType);
            }
            let marker = required_header(headers, &REQUEST_MARKER)?;
            if marker.as_bytes() != b"1" {
                return Err(SecurityError::Forbidden);
            }
        }
        Ok(SecurityDecision::Continue { origin })
    }
}

#[derive(Clone, Debug)]
enum SecurityDecision {
    Continue { origin: Option<HeaderValue> },
    Preflight { origin: HeaderValue },
}

#[derive(Clone, Copy, Debug)]
enum SecurityError {
    Forbidden,
    UnsupportedMediaType,
}

impl IntoResponse for SecurityError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                ApiErrorCode::RequestForbidden,
                "request validation failed",
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                ApiErrorCode::UnsupportedMediaType,
                "Content-Type must be application/json",
            ),
        };
        (
            status,
            Json(ErrorEnvelope {
                error: ApiError {
                    code,
                    message: message.to_owned(),
                    fields: None,
                },
            }),
        )
            .into_response()
    }
}

fn optional_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<Option<&'a HeaderValue>, SecurityError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(SecurityError::Forbidden);
    }
    Ok(value)
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<&'a HeaderValue, SecurityError> {
    optional_header(headers, name)?.ok_or(SecurityError::Forbidden)
}

fn is_mutating(method: &Method) -> bool {
    method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
}

fn validate_preflight(path: &str, headers: &HeaderMap) -> Result<(), SecurityError> {
    let requested_method = required_header(headers, &ACCESS_CONTROL_REQUEST_METHOD)?;
    let requested_method = Method::from_bytes(requested_method.as_bytes())
        .map_err(|_error| SecurityError::Forbidden)?;
    let advertised = match path {
        "/api/settings" => requested_method == Method::GET || requested_method == Method::PATCH,
        "/api/devices/cameras" | "/api/devices/serial-ports" | "/api/state" | "/ws" => {
            requested_method == Method::GET
        }
        "/api/camera/retry"
        | "/api/camera/screenshot"
        | "/api/commands/control"
        | "/api/commands/reload"
        | "/api/dynamic-config/control"
        | "/api/notifications/test"
        | "/api/profiles/generate-launcher"
        | "/api/script-ui/action"
        | "/api/serial/control"
        | "/api/update/check" => requested_method == Method::POST,
        _ => false,
    };
    if !advertised {
        return Err(SecurityError::Forbidden);
    }
    let Some(requested_headers) = optional_header(headers, &ACCESS_CONTROL_REQUEST_HEADERS)? else {
        return Ok(());
    };
    let requested_headers = requested_headers
        .to_str()
        .map_err(|_error| SecurityError::Forbidden)?;
    requested_headers
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .all(|name| {
            name.eq_ignore_ascii_case(CONTENT_TYPE.as_str())
                || name.eq_ignore_ascii_case(REQUEST_MARKER.as_str())
        })
        .then_some(())
        .ok_or(SecurityError::Forbidden)
}

fn add_cors_headers(response: &mut Response, origin: HeaderValue) {
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    response
        .headers_mut()
        .append(VARY, HeaderValue::from_static("Origin"));
}

async fn enforce_security(
    State(security): State<RequestSecurity>,
    request: Request,
    next: Next,
) -> Response {
    match security.validate(request.method(), request.uri(), request.headers()) {
        Ok(SecurityDecision::Continue { origin }) => {
            let mut response = next.run(request).await;
            if let Some(origin) = origin {
                add_cors_headers(&mut response, origin);
            }
            response
        }
        Ok(SecurityDecision::Preflight { origin }) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            add_cors_headers(&mut response, origin);
            response.headers_mut().insert(
                ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static(ALLOWED_METHODS),
            );
            response.headers_mut().insert(
                ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static(ALLOWED_HEADERS),
            );
            response
        }
        Err(error) => error.into_response(),
    }
}

/// Applies request validation to every route and fallback in `router`.
pub fn secure_router(router: Router, security: RequestSecurity) -> Router {
    Router::new()
        .fallback_service(router)
        .layer(middleware::from_fn_with_state(security, enforce_security))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::header::{
        ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
        ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD, ALLOW, CONTENT_TYPE, HOST,
        ORIGIN, VARY,
    };
    use axum::http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode};
    use axum::routing::{get, post};
    use serde_json::json;
    use tower::ServiceExt as _;

    use super::{ALLOWED_HEADERS, ALLOWED_METHODS, RequestSecurity, secure_router};

    fn policy() -> RequestSecurity {
        RequestSecurity::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8020),
            false,
        )
    }

    fn request(method: &str, uri: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:8020")
    }

    fn assert_single_header(headers: &HeaderMap, name: &HeaderName, expected: &str, label: &str) {
        let mut values = headers.get_all(name).iter();
        let value = values
            .next()
            .unwrap_or_else(|| panic!("missing {name} for {label}"));
        assert!(values.next().is_none(), "duplicate {name} for {label}");
        assert_eq!(value, expected, "{name} for {label}");
    }

    async fn assert_bare_options_rejected_without_route_headers(app: &Router, path: &str) {
        let response = app
            .clone()
            .oneshot(
                request("OPTIONS", path)
                    .body(Body::empty())
                    .expect("bare OPTIONS request"),
            )
            .await
            .expect("bare OPTIONS response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert_single_header(response.headers(), &CONTENT_TYPE, "application/json", path);
        for forbidden in [
            &ALLOW,
            &ACCESS_CONTROL_ALLOW_ORIGIN,
            &ACCESS_CONTROL_ALLOW_METHODS,
            &ACCESS_CONTROL_ALLOW_HEADERS,
            &VARY,
        ] {
            assert!(
                !response.headers().contains_key(forbidden),
                "unexpected {forbidden} for bare OPTIONS {path}"
            );
        }
        let body = to_bytes(response.into_body(), 4096)
            .await
            .expect("bare OPTIONS rejection body");
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("bare OPTIONS rejection JSON");
        assert_eq!(
            body,
            json!({
                "error": {
                    "code": "request_forbidden",
                    "fields": null,
                    "message": "request validation failed"
                }
            }),
            "{path}"
        );
    }

    #[tokio::test]
    async fn security_responses_precede_method_router_header_decoration() {
        let app = secure_router(
            Router::new()
                .route("/api/camera/retry", post(|| async {}))
                .route("/api/settings", get(|| async {}).patch(|| async {}))
                .route("/ws", get(|| async {}).head(|| async {})),
            policy(),
        );

        for path in ["/api/camera/retry", "/api/settings", "/ws"] {
            assert_bare_options_rejected_without_route_headers(&app, path).await;
        }

        let preflight = app
            .clone()
            .oneshot(
                request("OPTIONS", "/api/camera/retry")
                    .header(ORIGIN, "http://localhost:8020")
                    .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        ACCESS_CONTROL_REQUEST_HEADERS,
                        "content-type, x-pokecon-request",
                    )
                    .body(Body::empty())
                    .expect("complete preflight request"),
            )
            .await
            .expect("complete preflight response");
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        assert_single_header(
            preflight.headers(),
            &ACCESS_CONTROL_ALLOW_ORIGIN,
            "http://localhost:8020",
            "complete preflight",
        );
        assert_single_header(
            preflight.headers(),
            &ACCESS_CONTROL_ALLOW_METHODS,
            ALLOWED_METHODS,
            "complete preflight",
        );
        assert_single_header(
            preflight.headers(),
            &ACCESS_CONTROL_ALLOW_HEADERS,
            ALLOWED_HEADERS,
            "complete preflight",
        );
        assert_single_header(preflight.headers(), &VARY, "Origin", "complete preflight");
        assert!(!preflight.headers().contains_key(ALLOW));
        assert!(
            to_bytes(preflight.into_body(), 1)
                .await
                .expect("complete preflight body")
                .is_empty()
        );

        let inner_method_rejection = app
            .oneshot(
                request("GET", "/api/camera/retry")
                    .body(Body::empty())
                    .expect("inner wrong-method request"),
            )
            .await
            .expect("inner wrong-method response");
        assert_eq!(
            inner_method_rejection.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_single_header(
            inner_method_rejection.headers(),
            &ALLOW,
            "POST",
            "inner method rejection",
        );
    }

    #[tokio::test]
    async fn loopback_hosts_and_origins_are_derived_from_the_actual_port() {
        let app = secure_router(
            Router::new().route("/api/state", get(|| async { "ok" })),
            policy(),
        );
        for (host, origin) in [
            ("127.0.0.1:8020", "http://127.0.0.1:8020"),
            ("localhost:8020", "http://localhost:8020"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/state")
                        .header(HOST, host)
                        .header(ORIGIN, origin)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN], origin);
        }

        let response = app
            .oneshot(
                request("GET", "/api/state")
                    .header(ORIGIN, "http://localhost:9999")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn non_loopback_does_not_gain_a_localhost_alias() {
        let policy = RequestSecurity::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 8080),
            false,
        );
        let app = secure_router(Router::new().route("/", get(|| async {})), policy);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(HOST, "localhost:8080")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn ipv6_authority_is_bracketed_and_tauri_is_desktop_only() {
        let address = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8020);
        let web = secure_router(
            Router::new().route("/api/state", get(|| async {})),
            RequestSecurity::new(address, false),
        );
        let tauri_request = || {
            Request::builder()
                .uri("/api/state")
                .header(HOST, "[::1]:8020")
                .header(ORIGIN, "tauri://localhost")
                .body(Body::empty())
                .expect("request")
        };
        assert_eq!(
            web.oneshot(tauri_request())
                .await
                .expect("response")
                .status(),
            StatusCode::FORBIDDEN
        );

        let desktop = secure_router(
            Router::new().route("/api/state", get(|| async {})),
            RequestSecurity::new(address, true),
        );
        assert_eq!(
            desktop
                .oneshot(tauri_request())
                .await
                .expect("response")
                .status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn mutations_require_json_and_the_fixed_marker_even_without_origin() {
        let app = secure_router(
            Router::new().route("/api/action", post(|| async {})),
            policy(),
        );
        let response = app
            .clone()
            .oneshot(
                request("POST", "/api/action")
                    .header(CONTENT_TYPE, "text/plain")
                    .header("x-pokecon-request", "1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let response = app
            .clone()
            .oneshot(
                request("POST", "/api/action")
                    .header("x-pokecon-request", "1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let response = app
            .clone()
            .oneshot(
                request("POST", "/api/action")
                    .header(CONTENT_TYPE, "application/json; charset=utf-8")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = app
            .oneshot(
                request("POST", "/api/action")
                    .header(CONTENT_TYPE, "application/json; charset=utf-8")
                    .header("x-pokecon-request", "1")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn websocket_origin_is_mandatory() {
        let app = secure_router(Router::new().route("/ws", get(|| async {})), policy());
        let response = app
            .clone()
            .oneshot(request("GET", "/ws").body(Body::empty()).expect("request"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let response = app
            .clone()
            .oneshot(
                request("GET", "/ws")
                    .header(ORIGIN, "http://localhost:8020")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn preflight_header_lookup_is_canonical_and_singleton() {
        let app = secure_router(Router::new().route("/ws", get(|| async {})), policy());
        let decoy = request("OPTIONS", "/api/state")
            .header(ORIGIN, "http://localhost:8020")
            .header(ACCESS_CONTROL_REQUEST_METHOD, "CONNECT")
            .header("x-pokecon-preflight-method", "GET")
            .body(Body::empty())
            .expect("decoy request");
        let mut duplicate = request("OPTIONS", "/api/state")
            .header(ORIGIN, "http://localhost:8020")
            .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .expect("duplicate request");
        duplicate.headers_mut().append(
            ACCESS_CONTROL_REQUEST_METHOD,
            HeaderValue::from_static("CONNECT"),
        );

        for (label, request) in [("alternate", decoy), ("duplicate", duplicate)] {
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{label}");
            let body = to_bytes(response.into_body(), 4096)
                .await
                .expect("preflight rejection body");
            let body: serde_json::Value =
                serde_json::from_slice(&body).expect("preflight rejection JSON");
            assert_eq!(body["error"]["code"], "request_forbidden", "{label}");
        }
    }

    #[tokio::test]
    async fn preflights_match_the_openapi_surface() {
        let app = secure_router(Router::new().route("/ws", get(|| async {})), policy());
        let preflight_policy: [(&str, &[&str]); 15] = [
            ("/api/camera/retry", &["POST"]),
            ("/api/camera/screenshot", &["POST"]),
            ("/api/commands/control", &["POST"]),
            ("/api/commands/reload", &["POST"]),
            ("/api/devices/cameras", &["GET"]),
            ("/api/devices/serial-ports", &["GET"]),
            ("/api/dynamic-config/control", &["POST"]),
            ("/api/notifications/test", &["POST"]),
            ("/api/profiles/generate-launcher", &["POST"]),
            ("/api/script-ui/action", &["POST"]),
            ("/api/serial/control", &["POST"]),
            ("/api/settings", &["GET", "PATCH"]),
            ("/api/state", &["GET"]),
            ("/api/update/check", &["POST"]),
            ("/ws", &["GET"]),
        ];
        for (path, advertised_methods) in preflight_policy {
            for requested_method in [
                "GET", "HEAD", "PATCH", "POST", "PUT", "DELETE", "OPTIONS", "TRACE", "CONNECT",
            ] {
                let mut request = request("OPTIONS", path)
                    .header(ORIGIN, "http://localhost:8020")
                    .header(ACCESS_CONTROL_REQUEST_METHOD, requested_method);
                if matches!(requested_method, "PATCH" | "POST") {
                    request = request.header(
                        ACCESS_CONTROL_REQUEST_HEADERS,
                        "content-type, x-pokecon-request",
                    );
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).expect("request"))
                    .await
                    .expect("response");
                if advertised_methods.contains(&requested_method) {
                    assert_eq!(
                        response.status(),
                        StatusCode::NO_CONTENT,
                        "OPTIONS {path} for {requested_method}"
                    );
                    assert_eq!(
                        response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
                        "http://localhost:8020"
                    );
                    assert_eq!(
                        response.headers()[ACCESS_CONTROL_ALLOW_METHODS],
                        ALLOWED_METHODS
                    );
                    assert_eq!(
                        response.headers()[ACCESS_CONTROL_ALLOW_HEADERS],
                        ALLOWED_HEADERS
                    );
                    assert!(
                        to_bytes(response.into_body(), 1)
                            .await
                            .expect("preflight body")
                            .is_empty()
                    );
                } else {
                    assert_eq!(
                        response.status(),
                        StatusCode::FORBIDDEN,
                        "OPTIONS {path} for {requested_method}"
                    );
                    let body = to_bytes(response.into_body(), 4096)
                        .await
                        .expect("preflight rejection body");
                    let body: serde_json::Value =
                        serde_json::from_slice(&body).expect("preflight rejection JSON");
                    assert_eq!(body["error"]["code"], "request_forbidden");
                }
            }
        }

        let response = app
            .oneshot(
                request("OPTIONS", "/api/not-in-openapi")
                    .header(ORIGIN, "http://localhost:8020")
                    .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), 4096)
            .await
            .expect("unknown preflight body");
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("unknown preflight JSON");
        assert_eq!(body["error"]["code"], "request_forbidden");
    }
}
