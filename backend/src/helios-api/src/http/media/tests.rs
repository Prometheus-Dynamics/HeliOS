use super::*;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{
    Request, StatusCode,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use std::io::Read;
use std::sync::{Arc, OnceLock};
use tower::ServiceExt;
use uuid::Uuid;

fn init_data_dir() -> &'static std::path::Path {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = tempfile::tempdir().expect("tempdir").keep();
        super::super::storage::set_data_root_for_tests(dir.clone());
        dir
    })
    .as_path()
}

fn multipart(boundary: &str, parts: &[(&str, Option<&str>, &str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, content_type, bytes) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(filename) => {
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes());
                body.extend_from_slice(bytes);
                body.extend_from_slice(b"\r\n");
            }
            None => {
                body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
                body.extend_from_slice(bytes);
                body.extend_from_slice(b"\r\n");
            }
        }
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

async fn test_app() -> Router {
    let state = Arc::new(crate::app_state::ApiAppState::new(Arc::new(crate::ipc::connect_all().await)));
    Router::new().nest("/media", router()).with_state(state)
}

#[tokio::test]
async fn upload_accepts_metadata_before_file() {
    let root = init_data_dir();

    let app = test_app().await;
    let boundary = "BOUNDARY";
    let body = multipart(boundary, &[("kind", None, "text/plain", b"image"), ("files", Some("hello.png"), "image/png", b"PNGDATA")]);
    let content_len = body.len();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .header(CONTENT_LENGTH, content_len)
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CREATED);
    let stored = root.join("media").join("hello.png");
    assert!(stored.exists(), "expected stored file at {}", stored.display());
    assert_eq!(std::fs::read(stored).expect("read stored file"), b"PNGDATA");
}

#[tokio::test]
async fn upload_rejects_multiple_files() {
    let _ = init_data_dir();

    let app = test_app().await;
    let boundary = "BOUNDARY2";
    let body = multipart(boundary, &[("files", Some("a.txt"), "text/plain", b"A"), ("files", Some("b.txt"), "text/plain", b"B")]);
    let content_len = body.len();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/media")
                .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .header(CONTENT_LENGTH, content_len)
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn download_archive_includes_selected_files() {
    let root = init_data_dir();
    let media_dir = root.join("media");
    std::fs::create_dir_all(&media_dir).expect("create media directory");

    let name_a = format!("archive-test-a-{}.txt", Uuid::new_v4());
    let name_b = format!("archive-test-b-{}.txt", Uuid::new_v4());
    std::fs::write(media_dir.join(&name_a), b"alpha").expect("write first media file");
    std::fs::write(media_dir.join(&name_b), b"beta").expect("write second media file");

    let app = test_app().await;
    let response = app.oneshot(Request::builder().method("GET").uri(format!("/media/download.zip?name={name_a}&name={name_b}")).body(Body::empty()).expect("request")).await.expect("response");

    let (parts, body) = response.into_parts();
    let body = to_bytes(body, usize::MAX).await.expect("archive body");
    assert_eq!(parts.status, StatusCode::OK, "unexpected body: {}", String::from_utf8_lossy(&body));
    assert_eq!(parts.headers.get(CONTENT_TYPE).and_then(|value| value.to_str().ok()), Some("application/zip"));
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body.to_vec())).expect("valid zip archive");

    {
        let mut file_a = archive.by_name(&name_a).expect("first archive entry");
        let mut content_a = String::new();
        file_a.read_to_string(&mut content_a).expect("read first archive entry");
        assert_eq!(content_a, "alpha");
    }

    {
        let mut file_b = archive.by_name(&name_b).expect("second archive entry");
        let mut content_b = String::new();
        file_b.read_to_string(&mut content_b).expect("read second archive entry");
        assert_eq!(content_b, "beta");
    }
}
