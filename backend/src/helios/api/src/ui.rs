//! The HeliOS UI (`ui/`, a static SvelteKit build) served on the API's own port, so the UI,
//! the API and the device identity's `manage_url` (`http://<hostname>.local:5800/`) are one
//! origin. Everything outside `/v1` is a UI path: files from the build directory (their
//! precompressed `.br`/`.gz` twins when the client takes them), and the app shell
//! (`index.html`) for client-side routes.

use std::path::{Component, Path, PathBuf};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{SharedState, error::ApiError};

const INDEX: &str = "index.html";

/// The router's fallback: a UI file, the app shell, or the API's 404.
pub async fn fallback(State(state): State<SharedState>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let accept = request.headers().get(header::ACCEPT_ENCODING).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    drop(request);
    let not_found = || ApiError::not_found(format!("no route for {method} {path}")).into_response();
    if path == "/v1" || path.starts_with("/v1/") || !matches!(method, Method::GET | Method::HEAD) {
        return not_found();
    }
    let Some(root) = state.config.ui_dir.as_deref() else { return not_found() };
    let Some(file) = resolve(root, &path) else { return not_found() };
    match serve_file(&file, &accept, method == Method::HEAD).await {
        Some(response) => response,
        None => not_found(),
    }
}

/// The file a UI path names: a file under `root`, else the app shell for paths without an
/// extension (client-side routes). `None` for anything that would leave `root`.
pub fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let relative = url_path.trim_start_matches('/');
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => path.push(part),
            _ => return None,
        }
    }
    if relative.contains('\\') || relative.contains('\0') {
        return None;
    }
    if path.is_dir() {
        path.push(INDEX);
    }
    if path.is_file() {
        return Some(path);
    }
    let last = relative.rsplit('/').next().unwrap_or("");
    if last.contains('.') {
        return None;
    }
    let shell = root.join(INDEX);
    shell.is_file().then_some(shell)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn accepts(accept_encoding: &str, coding: &str) -> bool {
    accept_encoding.split(',').any(|part| {
        let mut fields = part.split(';');
        let name = fields.next().unwrap_or("").trim();
        let refused = fields.any(|f| matches!(f.trim(), "q=0" | "q=0.0" | "q=0.00" | "q=0.000"));
        name.eq_ignore_ascii_case(coding) && !refused
    })
}

async fn serve_file(path: &Path, accept_encoding: &str, head: bool) -> Option<Response> {
    let mut encoding = None;
    let mut body_path = path.to_path_buf();
    for (coding, suffix) in [("br", "br"), ("gzip", "gz")] {
        let twin = PathBuf::from(format!("{}.{suffix}", path.display()));
        if accepts(accept_encoding, coding) && twin.is_file() {
            encoding = Some(coding);
            body_path = twin;
            break;
        }
    }
    let bytes = tokio::fs::read(&body_path).await.ok()?;
    let length = bytes.len();
    let mut response = Response::new(if head { Body::empty() } else { Body::from(bytes) });
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type(path)));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    headers.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if let Some(coding) = encoding {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static(coding));
    }
    // SvelteKit's hashed assets never change; the shell must be revalidated after an update.
    let immutable = path.components().any(|c| c.as_os_str() == "immutable");
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(if immutable { "public, max-age=31536000, immutable" } else { "no-cache" }));
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("_app/immutable")).expect("mkdir");
        std::fs::write(dir.path().join("index.html"), "<html>shell</html>").expect("write");
        std::fs::write(dir.path().join("_app/immutable/app.js"), "console.log(1)").expect("write");
        std::fs::write(dir.path().join("_app/immutable/app.js.gz"), "gz").expect("write");
        dir
    }

    #[test]
    fn resolves_files_routes_and_refuses_escapes() {
        let dir = build();
        let root = dir.path();
        assert_eq!(resolve(root, "/"), Some(root.join("index.html")));
        assert_eq!(resolve(root, "/_app/immutable/app.js"), Some(root.join("_app/immutable/app.js")));
        assert_eq!(resolve(root, "/pipelines/abc"), Some(root.join("index.html")), "client-side routes get the shell");
        assert_eq!(resolve(root, "/missing.js"), None);
        assert_eq!(resolve(root, "/../etc/passwd"), None);
        assert_eq!(resolve(root, "/_app/../../secret"), None);
    }

    #[test]
    fn encodings() {
        assert!(accepts("gzip, deflate, br", "br"));
        assert!(accepts("gzip;q=0.8", "gzip"));
        assert!(!accepts("gzip;q=0", "gzip"));
        assert!(!accepts("identity", "gzip"));
    }

    #[tokio::test]
    async fn serves_precompressed_twins() {
        let dir = build();
        let response = serve_file(&dir.path().join("_app/immutable/app.js"), "gzip", false).await.expect("response");
        assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
        assert!(response.headers()[header::CACHE_CONTROL].to_str().expect("str").contains("immutable"));
        let plain = serve_file(&dir.path().join("index.html"), "", false).await.expect("response");
        assert!(plain.headers().get(header::CONTENT_ENCODING).is_none());
        assert_eq!(plain.headers()[header::CACHE_CONTROL], "no-cache");
    }
}
