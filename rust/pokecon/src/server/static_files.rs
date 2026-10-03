//! Static SPA files confined to one canonical root.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{OriginalUri, State};
use axum::http::header::{ALLOW, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderValue, Method, Response, StatusCode};
use axum::response::IntoResponse;
use thiserror::Error;

/// Startup validation failure for `server.web_dir`.
#[derive(Debug, Error)]
pub enum StaticRootError {
    #[error("static root cannot be opened")]
    Unavailable(#[source] io::Error),
    #[error("static root is not a directory")]
    NotDirectory,
}

/// Immutable canonical static root. Files beneath it may be updated between
/// requests, but the root itself cannot be replaced at runtime.
#[derive(Clone, Debug)]
pub struct StaticFiles {
    root: Arc<PathBuf>,
}

impl StaticFiles {
    /// Canonicalizes and validates a readable directory.
    ///
    /// # Errors
    ///
    /// Returns a startup error if the path is absent, cannot be traversed, or
    /// does not resolve to a directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, StaticRootError> {
        let root = std::fs::canonicalize(root).map_err(StaticRootError::Unavailable)?;
        let metadata = std::fs::metadata(&root).map_err(StaticRootError::Unavailable)?;
        if !metadata.is_dir() {
            return Err(StaticRootError::NotDirectory);
        }
        drop(std::fs::read_dir(&root).map_err(StaticRootError::Unavailable)?);
        Ok(Self {
            root: Arc::new(root),
        })
    }

    /// Builds a fallback router for GET/HEAD static requests.
    pub fn router(self) -> Router {
        Router::new().fallback(serve_static).with_state(self)
    }

    async fn resolve(&self, raw_path: &str) -> StaticResolution {
        let Ok(normalized) = normalize_request_path(raw_path) else {
            return StaticResolution::Forbidden;
        };
        if normalized
            .components
            .first()
            .is_some_and(|first| first == "api" || first == "ws")
        {
            return StaticResolution::Missing;
        }
        let candidate =
            normalized
                .components
                .iter()
                .fold(self.root.as_ref().clone(), |mut path, component| {
                    path.push(component);
                    path
                });
        if self.has_unsafe_symlink_prefix(&normalized.components).await {
            return StaticResolution::Forbidden;
        }
        match self.resolve_existing(&candidate).await {
            StaticResolution::Missing if normalized.route_like => {
                self.resolve_existing(&self.root.join("index.html")).await
            }
            resolution => resolution,
        }
    }

    async fn has_unsafe_symlink_prefix(&self, components: &[String]) -> bool {
        let mut path = self.root.as_ref().clone();
        for component in components {
            path.push(component);
            match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let Ok(resolved) = tokio::fs::canonicalize(&path).await else {
                        return true;
                    };
                    if !resolved.starts_with(self.root.as_ref()) {
                        return true;
                    }
                }
                Ok(_metadata) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => return false,
                Err(_error) => return true,
            }
        }
        false
    }

    async fn resolve_existing(&self, candidate: &Path) -> StaticResolution {
        let canonical = match tokio::fs::canonicalize(candidate).await {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return StaticResolution::Missing;
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return StaticResolution::Forbidden;
            }
            Err(_error) => return StaticResolution::Internal,
        };
        if !canonical.starts_with(self.root.as_ref()) {
            return StaticResolution::Forbidden;
        }
        let metadata = match tokio::fs::metadata(&canonical).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return StaticResolution::Missing;
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return StaticResolution::Forbidden;
            }
            Err(_error) => return StaticResolution::Internal,
        };
        if metadata.is_dir() {
            return self
                .resolve_regular_file(&canonical.join("index.html"))
                .await;
        }
        if metadata.is_file() {
            StaticResolution::File(canonical)
        } else {
            StaticResolution::Forbidden
        }
    }

    async fn resolve_regular_file(&self, candidate: &Path) -> StaticResolution {
        let canonical = match tokio::fs::canonicalize(candidate).await {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return StaticResolution::Missing;
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return StaticResolution::Forbidden;
            }
            Err(_error) => return StaticResolution::Internal,
        };
        if !canonical.starts_with(self.root.as_ref()) {
            return StaticResolution::Forbidden;
        }
        match tokio::fs::metadata(&canonical).await {
            Ok(metadata) if metadata.is_file() => StaticResolution::File(canonical),
            Ok(_metadata) => StaticResolution::Forbidden,
            Err(error) if error.kind() == io::ErrorKind::NotFound => StaticResolution::Missing,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                StaticResolution::Forbidden
            }
            Err(_error) => StaticResolution::Internal,
        }
    }
}

#[derive(Debug)]
enum StaticResolution {
    File(PathBuf),
    Forbidden,
    Missing,
    Internal,
}

#[derive(Debug)]
struct NormalizedPath {
    components: Vec<String>,
    route_like: bool,
}

fn normalize_request_path(raw_path: &str) -> Result<NormalizedPath, ()> {
    let relative = raw_path.strip_prefix('/').ok_or(())?;
    if relative.starts_with('/') || relative.starts_with('\\') {
        return Err(());
    }
    let mut decoded = relative.to_owned();
    let mut strict = true;
    for _pass in 0..=relative.len() {
        let (next, changed) = percent_decode_once(&decoded, strict)?;
        decoded = next;
        strict = false;
        if !changed {
            break;
        }
    }
    if decoded.contains('\0') || decoded.starts_with('/') || decoded.starts_with('\\') {
        return Err(());
    }
    let mut components = Vec::new();
    for component in decoded.split(['/', '\\']).filter(|value| !value.is_empty()) {
        let bytes = component.as_bytes();
        if matches!(component, "." | "..")
            || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        {
            return Err(());
        }
        components.push(component.to_owned());
    }
    let route_like = components
        .last()
        .is_none_or(|component| Path::new(component).extension().is_none());
    Ok(NormalizedPath {
        components,
        route_like,
    })
}

fn percent_decode_once(value: &str, reject_invalid: bool) -> Result<(String, bool), ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut changed = false;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(high) = bytes.get(index + 1).and_then(|byte| hex_value(*byte)) else {
            if reject_invalid {
                return Err(());
            }
            decoded.push(bytes[index]);
            index += 1;
            continue;
        };
        let Some(low) = bytes.get(index + 2).and_then(|byte| hex_value(*byte)) else {
            if reject_invalid {
                return Err(());
            }
            decoded.push(bytes[index]);
            index += 1;
            continue;
        };
        decoded.push((high << 4) | low);
        index += 3;
        changed = true;
    }
    String::from_utf8(decoded)
        .map(|decoded| (decoded, changed))
        .map_err(|_error| ())
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

async fn serve_static(
    State(files): State<StaticFiles>,
    method: Method,
    OriginalUri(uri): OriginalUri,
) -> Response<Body> {
    if method != Method::GET && method != Method::HEAD {
        let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    match files.resolve(uri.path()).await {
        StaticResolution::File(path) => serve_file(&path, method == Method::HEAD).await,
        StaticResolution::Forbidden => status_response(StatusCode::FORBIDDEN),
        StaticResolution::Missing => status_response(StatusCode::NOT_FOUND),
        StaticResolution::Internal => status_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn serve_file(path: &Path, head: bool) -> Response<Body> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return status_response(StatusCode::NOT_FOUND);
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return status_response(StatusCode::FORBIDDEN);
        }
        Err(_error) => return status_response(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let content_type = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = HeaderValue::from_str(content_type.essence_str())
        .expect("MIME essence values are valid HTTP headers");
    let content_length = HeaderValue::from_str(&bytes.len().to_string())
        .expect("decimal lengths are valid HTTP headers");
    let body = if head {
        Body::empty()
    } else {
        Body::from(bytes)
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert(CONTENT_LENGTH, content_length);
    response
}

fn status_response(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use std::fs;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tempfile::tempdir;
    use tower::ServiceExt as _;

    use super::{StaticFiles, StaticResolution};

    fn fixture() -> (tempfile::TempDir, StaticFiles) {
        let directory = tempdir().expect("temporary root");
        fs::write(directory.path().join("index.html"), b"<main>app</main>").expect("write index");
        fs::create_dir(directory.path().join("assets")).expect("create assets");
        fs::write(directory.path().join("assets/app.js"), b"export {};").expect("write asset");
        let files = StaticFiles::new(directory.path()).expect("valid static root");
        (directory, files)
    }

    #[tokio::test]
    async fn serves_assets_and_falls_back_only_for_spa_routes() {
        let (_directory, files) = fixture();
        let app = files.router();
        let asset = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/assets/app.js")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(asset.headers()["content-type"], "text/javascript");
        assert_eq!(
            to_bytes(asset.into_body(), 1024).await.expect("body"),
            "export {};"
        );

        let route = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/commands/history")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(route.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(route.into_body(), 1024).await.expect("body"),
            "<main>app</main>"
        );

        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/assets/missing.js")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn encoded_cross_platform_traversal_is_forbidden() {
        let (_directory, files) = fixture();
        for path in [
            "/../secret",
            "/%2e%2e/secret",
            "/%252e%252e/secret",
            "/C:%5csecret",
            "/%5c%5cserver%5cshare",
            "/%00",
        ] {
            assert!(
                matches!(files.resolve(path).await, StaticResolution::Forbidden),
                "{path}"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn outside_and_dangling_symlinks_are_forbidden() {
        use std::os::unix::fs::symlink;

        let (directory, files) = fixture();
        let outside = tempdir().expect("outside root");
        fs::write(outside.path().join("secret.txt"), b"secret").expect("write secret");
        symlink(
            outside.path().join("secret.txt"),
            directory.path().join("outside.txt"),
        )
        .expect("outside symlink");
        symlink(
            outside.path().join("missing.txt"),
            directory.path().join("dangling.txt"),
        )
        .expect("dangling symlink");

        assert!(matches!(
            files.resolve("/outside.txt").await,
            StaticResolution::Forbidden
        ));
        assert!(matches!(
            files.resolve("/dangling.txt").await,
            StaticResolution::Forbidden
        ));
    }

    #[tokio::test]
    async fn api_and_websocket_names_never_receive_the_spa_fallback() {
        let (_directory, files) = fixture();
        assert!(matches!(
            files.resolve("/api/unknown").await,
            StaticResolution::Missing
        ));
        assert!(matches!(
            files.resolve("/ws").await,
            StaticResolution::Missing
        ));
    }
}
