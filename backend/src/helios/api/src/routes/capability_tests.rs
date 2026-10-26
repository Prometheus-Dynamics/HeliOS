//! Route tests for device capabilities: a guest (SystemCore) has no updates, slots, reboot or
//! security, and says so; an owner (the Raze image) has them. See `capabilities.rs`.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::{ApiConfig, AppState, router};

/// A guest package under `dir`: no board contract, the guest's layout, no lemnosd devices.
fn guest_state(dir: &std::path::Path) -> crate::SharedState {
    let mut config = ApiConfig::from_vars([("HELIOS_LAYOUT_PROFILE", "guest")]).expect("guest config");
    config.orion_socket = dir.join("no-orion.sock");
    config.orion_stream_socket = dir.join("no-orion-stream.sock");
    config.state_dir = dir.join("state");
    config.upload_dir = dir.join("uploads");
    config.auth_file = dir.join("auth").join("auth.json");
    config.ui_dir = None;
    config.test_lemnos_classes = Some(Default::default());
    AppState::new(config)
}

/// The owner image under `dir`, with lemnosd's status ring.
fn owner_state(dir: &std::path::Path) -> crate::SharedState {
    AppState::new(ApiConfig {
        orion_socket: dir.join("no-orion.sock"),
        orion_stream_socket: dir.join("no-orion-stream.sock"),
        state_dir: dir.join("state"),
        board: Some(helios_layout::BoardContract { identity_file: dir.join("identity.json"), update_tool: dir.join("no-board-update"), update_status: dir.join("update.json"), update_progress: dir.join("update-progress"), ..helios_layout::BoardContract::image() }),
        board_update_systemd_run: false,
        ui_dir: None,
        upload_dir: dir.join("uploads"),
        auth_file: dir.join("auth").join("auth.json"),
        test_lemnos_classes: Some(["light".to_string()].into()),
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
async fn a_guest_reports_its_profile_and_what_it_lacks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&guest_state(dir.path()), "GET", "/v1/device", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["profile"], "guest");
    assert_eq!(body["features"]["updates"], false);
    assert_eq!(body["features"]["reboot"], false);
    assert_eq!(body["features"]["security"], false);
    assert_eq!(body["features"]["leds"], false);
    assert!(body["reasons"]["reboot"].is_string(), "the reason is given: {body}");
    assert!(body["host"]["kind"].is_string());
}

#[tokio::test]
async fn an_owner_reports_the_board_features_and_its_hardware() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&owner_state(dir.path()), "GET", "/v1/device", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["profile"], "owner");
    for name in ["updates", "slots", "reboot", "recovery_ladder", "security", "console", "leds"] {
        assert_eq!(body["features"][name], true, "{name} on the owner: {body}");
    }
    assert_eq!(body["features"]["fan"], false, "no fan reported");
    assert!(body["reasons"].get("reboot").is_none(), "nothing off that the owner has");
}

#[tokio::test]
async fn a_guest_identity_offers_no_helios_ota() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&guest_state(dir.path()), "GET", "/v1/identity", None).await;
    assert_eq!(status, StatusCode::OK);
    let methods: Vec<&str> = body["update_methods"].as_array().expect("methods").iter().filter_map(Value::as_str).collect();
    assert!(!methods.contains(&"helios-ota"), "{methods:?}");
    assert!(!methods.contains(&"image-write"), "{methods:?}");
    assert!(methods.contains(&"host-package"), "{methods:?}");
}

#[tokio::test]
async fn a_guest_refuses_updates_slots_reboot_safe_mode_and_security() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = guest_state(dir.path());
    for (method, uri, body, name) in [
        ("POST", "/v1/update/apply", Some(json!({ "upload_id": "x" })), "updates"),
        ("POST", "/v1/update/slots/switch", None, "slots"),
        ("POST", "/v1/system/reboot", None, "reboot"),
        ("POST", "/v1/system/safe-mode", None, "safe_mode"),
        ("POST", "/v1/auth/enable", Some(json!({ "password": "a-long-password" })), "security"),
        ("POST", "/v1/ota/apply", Some(json!({ "image_url": "file:///x" })), "updates"),
    ] {
        let (status, error) = call(&state, method, uri, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {error}");
        assert!(error["error"]["message"].as_str().unwrap_or_default().starts_with(&format!("{name} is not on this device")), "{method} {uri}: {error}");
    }
}

#[tokio::test]
async fn a_guest_status_has_no_slot_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&guest_state(dir.path()), "GET", "/v1/update/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["slots"]["layout"], "none");
}
