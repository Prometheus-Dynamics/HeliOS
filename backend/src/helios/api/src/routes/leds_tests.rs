//! Route tests for the LED director: settings, locate and preview. The ring thread is not started
//! here, so nothing is written to lemnosd.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::{ApiConfig, AppState, router};

fn test_state(dir: &std::path::Path) -> crate::SharedState {
    AppState::new(ApiConfig {
        orion_socket: dir.join("no-orion.sock"),
        orion_stream_socket: dir.join("no-orion-stream.sock"),
        state_dir: dir.join("state"),
        board_update_tool: dir.join("no-board-update"),
        board_update_status: dir.join("update.json"),
        board_update_progress: dir.join("update-progress"),
        board_update_systemd_run: false,
        ui_dir: None,
        upload_dir: dir.join("uploads"),
        board_identity_path: dir.join("identity.json"),
        auth_file: dir.join("auth").join("auth.json"),
        ..ApiConfig::default()
    })
}

async fn call(state: &crate::SharedState, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
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
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let json = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, json)
}

#[tokio::test]
async fn the_settings_are_read_validated_and_saved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "GET", "/v1/device/leds", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["settings"]["brightness"], 0.5);
    assert_eq!(body["settings"]["direction"], "cw");
    assert_eq!(body["live"]["ring_leds"], 16);
    assert_eq!(body["states"].as_array().map(Vec::len), Some(12));
    assert_eq!(body["states"][0]["name"], "locate");

    let (status, _) = call(&state, "PUT", "/v1/device/leds", Some(json!({ "brightness": 1.5 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&state, "PUT", "/v1/device/leds", Some(json!({ "forward_led": 16 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = call(&state, "PUT", "/v1/device/leds", Some(json!({ "brightness": 0.8, "direction": "ccw", "forward_led": 3 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["settings"]["brightness"], 0.8);
    assert_eq!(body["settings"]["forward_led"], 3);
    assert!(body["settings"]["tally"].as_bool().unwrap_or(false), "the rest keep their defaults");

    let (_, body) = call(&state, "GET", "/v1/device/leds", None).await;
    assert_eq!(body["settings"]["direction"], "ccw", "the saved settings are read back");
    assert!(dir.path().join("state").join("leds.json").exists());
}

#[tokio::test]
async fn an_unknown_setting_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, _) = call(&state, "PUT", "/v1/device/leds", Some(json!({ "colour": "red" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn locate_is_timed_and_bounded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "POST", "/v1/device/leds/locate", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["locate_until_ms"].is_u64(), "the default locate runs for a while: {body}");
    let (status, _) = call(&state, "POST", "/v1/device/leds/locate", Some(json!({ "seconds": 0 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&state, "POST", "/v1/device/leds/locate", Some(json!({ "seconds": 61 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = call(&state, "POST", "/v1/device/leds/locate", Some(json!({ "seconds": 5 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["locate_until_ms"].is_u64());
}

#[tokio::test]
async fn a_preview_names_a_state_and_stops_on_null() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, _) = call(&state, "POST", "/v1/device/leds/preview", Some(json!({ "state": "nope" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&state, "POST", "/v1/device/leds/preview", Some(json!({ "state": "tags", "seconds": 0 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = call(&state, "POST", "/v1/device/leds/preview", Some(json!({ "state": "tags-locked", "seconds": 3 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["preview"]["state"], "tags-locked");
    assert!(body["preview"]["until_ms"].is_u64());
    let (_, body) = call(&state, "GET", "/v1/device/leds", None).await;
    assert_eq!(body["preview"]["state"], "tags-locked", "the next read shows it too");

    let (status, body) = call(&state, "POST", "/v1/device/leds/preview", Some(json!({ "state": null }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["preview"].is_null());
}

#[tokio::test]
async fn the_device_is_not_reachable_before_the_ring_thread_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (_, body) = call(&state, "GET", "/v1/device/leds", None).await;
    assert_eq!(body["device"]["reachable"], false);
    assert_eq!(body["live"]["state"], "idle");
}
