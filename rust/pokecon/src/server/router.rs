//! Composition of API routes, the static fallback, and request security.

use axum::Router;

use crate::server::security::{RequestSecurity, secure_router};
use crate::server::static_files::StaticFiles;

/// Builds the one public router. Security is deliberately applied after the
/// static fallback is attached so no HTTP path can bypass Host validation.
pub fn public_router(api: Router, static_files: StaticFiles, security: RequestSecurity) -> Router {
    secure_router(api.fallback_service(static_files.router()), security)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::header::HOST;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tempfile::tempdir;
    use tower::ServiceExt as _;

    use super::public_router;
    use crate::server::security::RequestSecurity;
    use crate::server::static_files::StaticFiles;

    fn app() -> (tempfile::TempDir, Router) {
        let root = tempdir().expect("static root");
        fs::write(root.path().join("index.html"), "app").expect("write index");
        let static_files = StaticFiles::new(root.path()).expect("valid root");
        let api = Router::new().route("/api/state", get(|| async { "state" }));
        let security = RequestSecurity::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8020),
            false,
        );
        (root, public_router(api, static_files, security))
    }

    #[tokio::test]
    async fn security_wraps_api_and_static_fallback() {
        let (_root, app) = app();
        let forbidden = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(HOST, "attacker.invalid:8020")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        for (path, expected) in [("/", "app"), ("/api/state", "state")] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(HOST, "localhost:8020")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.expect("body"),
                expected
            );
        }
    }

    #[tokio::test]
    async fn unknown_api_paths_never_use_the_spa_document() {
        let (_root, app) = app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/unknown")
                    .header(HOST, "localhost:8020")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
