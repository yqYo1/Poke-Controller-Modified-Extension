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

use crate::api::{ApiError, ApiErrorCode, ErrorEnvelope};

const REQUEST_MARKER: HeaderName = HeaderName::from_static("x-pokecon-request");
const ALLOWED_METHODS: &str = "GET, HEAD, PATCH, POST, PUT, DELETE, OPTIONS";
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
        let mut allowed_origins = BTreeSet::from([format!("http://{authority}")]);
        if address.ip().is_loopback() {
            let localhost = format!("localhost:{}", address.port());
            allowed_origins.insert(format!("http://{localhost}"));
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
            validate_preflight(headers)?;
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

fn validate_preflight(headers: &HeaderMap) -> Result<(), SecurityError> {
    let requested_method = required_header(headers, &ACCESS_CONTROL_REQUEST_METHOD)?;
    let requested_method = Method::from_bytes(requested_method.as_bytes())
        .map_err(|_error| SecurityError::Forbidden)?;
    if !matches!(
        requested_method,
        Method::GET | Method::HEAD | Method::PATCH | Method::POST | Method::PUT | Method::DELETE
    ) {
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
    router.layer(middleware::from_fn_with_state(security, enforce_security))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use axum::Router;
    use axum::body::Body;
    use axum::http::header::{
        ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD,
        CONTENT_TYPE, HOST, ORIGIN,
    };
    use axum::http::{Request, StatusCode};
    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::{RequestSecurity, secure_router};

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
    async fn websocket_origin_is_mandatory_and_preflight_is_closed() {
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

        let response = app
            .oneshot(
                request("OPTIONS", "/api/settings")
                    .header(ORIGIN, "http://localhost:8020")
                    .header(ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
                    .header(
                        ACCESS_CONTROL_REQUEST_HEADERS,
                        "content-type, x-pokecon-request",
                    )
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            response.headers()[ACCESS_CONTROL_ALLOW_ORIGIN],
            "http://localhost:8020"
        );
    }
}
