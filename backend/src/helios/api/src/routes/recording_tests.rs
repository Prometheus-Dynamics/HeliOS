//! Router tests for `/v1/recordings`: settings, listing, download and delete of stored parts, and
//! the refusals (no Orion, nothing running, bad ids). A running recording needs a camera service
//! and Orion, so those paths are covered by helios-recorder's own tests.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

use crate::{ApiConfig, AppState, router};

fn test_state(dir: &std::path::Path) -> crate::SharedState {
    AppState::new(ApiConfig {
        orion_socket: dir.join("no-orion.sock"),
        orion_stream_socket: dir.join("no-orion-stream.sock"),
        state_dir: dir.join("state"),
        board: Some(helios_layout::BoardContract { identity_file: dir.join("identity.json"), update_tool: dir.join("no-board-update"), update_status: dir.join("update.json"), update_progress: dir.join("update-progress"), ..helios_layout::BoardContract::image() }),
        board_update_systemd_run: false,
        ui_dir: None,
        upload_dir: dir.join("uploads"),
        auth_file: dir.join("auth").join("auth.json"),
        lemnos_socket: dir.join("no-lemnosd.sock"),
        ..ApiConfig::default()
    })
}

/// One request: its status, content type and body bytes.
async fn raw(state: &crate::SharedState, method: &str, uri: &str, body: Option<serde_json::Value>) -> (StatusCode, String, Vec<u8>) {
    let mut request = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(json) => {
            request = request.header("content-type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state.clone()).oneshot(request.body(body).expect("request")).await.expect("response");
    let status = response.status();
    let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body").to_vec();
    (status, content_type, bytes)
}

async fn call(state: &crate::SharedState, method: &str, uri: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
    let (status, _, bytes) = raw(state, method, uri, body).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

fn with_storage(dir: &std::path::Path) -> serde_json::Value {
    serde_json::json!({ "storage_dir": dir.join("rec"), "reserve_bytes": 0 })
}

#[tokio::test]
async fn settings_default_then_persist_and_validate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "GET", "/v1/recordings/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression"], true);
    assert_eq!(body["max_fps"], 30.0, "30 frames a second by default");
    assert_eq!(body["frames"], "raw", "the sensor's raw frames by default");

    let (status, _) = call(&state, "PUT", "/v1/recordings/settings", Some(serde_json::json!({ "max_fps": -1 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&state, "PUT", "/v1/recordings/settings", Some(serde_json::json!({ "bogus": 1 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = call(&state, "PUT", "/v1/recordings/settings", Some(serde_json::json!({ "compression": false, "max_fps": 15.0 }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = call(&state, "GET", "/v1/recordings/settings", None).await;
    assert_eq!((body["compression"].clone(), body["max_fps"].clone()), (serde_json::json!(false), serde_json::json!(15.0)));
    assert_eq!(body["reserve_bytes"], 2 * (1u64 << 30), "fields not given keep their defaults");
}

#[tokio::test]
async fn listing_shows_no_recording_and_the_storage() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    call(&state, "PUT", "/v1/recordings/settings", Some(with_storage(dir.path()))).await;
    let (status, body) = call(&state, "GET", "/v1/recordings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["active"].is_null());
    assert_eq!(body["stored"], serde_json::json!([]));
    assert_eq!(body["storage"]["dir"], dir.path().join("rec").display().to_string());
    assert_eq!(body["storage"]["reserve_bytes"], 0);
    assert!(body["storage_error"].is_null());
}

#[tokio::test]
async fn stored_parts_list_download_and_delete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    call(&state, "PUT", "/v1/recordings/settings", Some(with_storage(dir.path()))).await;
    let rec = dir.path().join("rec");
    std::fs::create_dir_all(&rec).expect("dir");
    std::fs::write(rec.join("board-cam-20261019T101530Z-abc.mcap"), b"MCAP-bytes").expect("mcap");
    let summary = serde_json::json!({
        "session": "board-cam-20261019T101530Z-abc", "camera": "cam", "file": "board-cam-20261019T101530Z-abc.mcap", "index": 1,
        "started_at_ms": 1, "duration_ms": 2500, "bytes": 10, "frames_written": 75, "frames_dropped": 2,
        "messages_written": 4, "messages_dropped": 0, "stop_reason": "requested"
    });
    std::fs::write(rec.join("board-cam-20261019T101530Z-abc.summary.json"), summary.to_string()).expect("summary");

    let (status, body) = call(&state, "GET", "/v1/recordings", None).await;
    assert_eq!(status, StatusCode::OK);
    let stored = &body["stored"][0];
    assert_eq!(stored["id"], "board-cam-20261019T101530Z-abc");
    assert_eq!(stored["bytes"], 10);
    assert_eq!((stored["summary"]["duration_ms"].clone(), stored["summary"]["frames_dropped"].clone()), (serde_json::json!(2500), serde_json::json!(2)));

    let (status, content_type, bytes) = raw(&state, "GET", "/v1/recordings/board-cam-20261019T101530Z-abc", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/octet-stream");
    assert_eq!(bytes, b"MCAP-bytes");

    let (status, _) = call(&state, "DELETE", "/v1/recordings/board-cam-20261019T101530Z-abc", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!rec.join("board-cam-20261019T101530Z-abc.summary.json").exists(), "the summary goes too");
    let (status, _) = call(&state, "GET", "/v1/recordings/board-cam-20261019T101530Z-abc", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&state, "DELETE", "/v1/recordings/board-cam-20261019T101530Z-abc", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_ids_are_refused_before_any_file_is_touched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, _) = call(&state, "GET", "/v1/recordings/..%2Fsecret", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&state, "DELETE", "/v1/recordings/has%20space", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&state, "POST", "/v1/recordings/has%20space/stop", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stopping_with_nothing_running_is_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "POST", "/v1/recordings/board-cam-x/stop", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn starting_without_orion_is_unavailable_and_checks_the_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    call(&state, "PUT", "/v1/recordings/settings", Some(with_storage(dir.path()))).await;
    let (status, body) = call(&state, "POST", "/v1/recordings", Some(serde_json::json!({ "camera": "cam0" }))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], "backend_unavailable");
    let (status, _) = call(&state, "POST", "/v1/recordings", Some(serde_json::json!({ "camera": "cam0", "fps": 5 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown fields are refused");
    let (status, _) = call(&state, "POST", "/v1/recordings", Some(serde_json::json!({ "camera": "cam 0" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn capture_last_is_refused_unless_the_settings_enable_a_ring() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "POST", "/v1/recordings/capture-last", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("no camera has a capture-last ring"), "{body}");
    call(&state, "PUT", "/v1/recordings/settings", Some(serde_json::json!({ "capture_last_cameras": ["cam0"] }))).await;
    let (status, body) = call(&state, "POST", "/v1/recordings/capture-last", Some(serde_json::json!({ "camera": "cam1" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.to_string().contains("not enabled for cam1"), "{body}");
}

#[tokio::test]
async fn capture_last_checks_the_window_and_needs_the_camera_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    call(&state, "PUT", "/v1/recordings/settings", Some(serde_json::json!({ "capture_last_cameras": ["cam0"], "capture_last_seconds": 10 }))).await;
    let (status, body) = call(&state, "POST", "/v1/recordings/capture-last", Some(serde_json::json!({ "seconds": 30 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = call(&state, "POST", "/v1/recordings/capture-last", Some(serde_json::json!({ "seconds": 0 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&state, "POST", "/v1/recordings/capture-last", Some(serde_json::json!({ "bogus": 1 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown fields are refused");
    // Without Orion the camera cannot be found, so the ring cannot run.
    let (status, body) = call(&state, "POST", "/v1/recordings/capture-last", Some(serde_json::json!({ "seconds": 5 }))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
}

#[tokio::test]
async fn the_ring_settings_are_checked_and_listed_with_the_recordings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, _) = call(&state, "PUT", "/v1/recordings/settings", Some(serde_json::json!({ "capture_last_seconds": 0 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = call(&state, "GET", "/v1/recordings/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((body["capture_last_seconds"].as_u64(), body["frames"].as_str()), (Some(10), Some("raw")), "{body}");
    assert_eq!(body["max_fps"].as_f64(), Some(30.0), "{body}");
    let (status, body) = call(&state, "GET", "/v1/recordings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rings"], serde_json::json!([]), "{body}");
}

#[tokio::test]
async fn an_eidos_export_needs_a_stored_readable_recording() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    std::fs::create_dir_all(dir.path().join("rec")).expect("dir");
    call(&state, "PUT", "/v1/recordings/settings", Some(with_storage(dir.path()))).await;
    let (status, body) = call(&state, "POST", "/v1/recordings/board-cam-20261019T101530Z-abc/export/eidos", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    std::fs::write(dir.path().join("rec").join("board-cam-x.mcap"), b"not a recording").expect("write");
    let (status, body) = call(&state, "POST", "/v1/recordings/board-cam-x/export/eidos", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = call(&state, "POST", "/v1/recordings/..%2Fsecret/export/eidos", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
