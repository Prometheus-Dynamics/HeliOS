//! Router-level tests: no Orion, no systemd. Orion-backed routes must answer 503, missing
//! backends 501, and everything with the one error shape.

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

async fn call(state: &crate::SharedState, method: &str, uri: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
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
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn health_reports_the_api() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&test_state(dir.path()), "GET", "/v1/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["service"], "helios-api");
    assert_eq!(body["api_version"], "v1");
}

#[tokio::test]
async fn orion_backed_routes_answer_503_without_orion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    for uri in ["/v1/pipelines", "/v1/cameras", "/v1/cameras/cam0/preview", "/v1/resources", "/v1/peripherals", "/v1/outputs", "/v1/nodes", "/v1/plugins"] {
        let (status, body) = call(&state, "GET", uri, None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        assert_eq!(body["error"]["code"], "backend_unavailable", "{uri}");
    }
}

#[tokio::test]
async fn missing_backends_answer_501_with_what_they_need() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let cases = [
        ("POST", "/v1/cameras/cam0/calibration/capture"),
        ("GET", "/v1/catalog"),
        ("POST", "/v1/system/safe-mode"),
        ("PUT", "/v1/system/processes/100/affinity"),
        ("GET", "/v1/peripherals/fan"),
        ("GET", "/v1/peripherals/leds"),
        ("GET", "/v1/peripherals/imu"),
        ("POST", "/v1/update/slots/switch"),
    ];
    for (method, uri) in cases {
        let (status, body) = call(&state, method, uri, None).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{method} {uri}");
        assert_eq!(body["error"]["code"], "not_available", "{method} {uri}");
        assert!(body["error"]["needs"].as_str().is_some_and(|needs| !needs.is_empty()), "{method} {uri}");
    }
}

#[tokio::test]
async fn unknown_routes_are_json_404s() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&test_state(dir.path()), "GET", "/v1/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn identity_merges_the_device_package_document() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    std::fs::write(
        dir.path().join("identity.json"),
        r#"{"contract":1,"model":"raze","rev":"gen1","serial":"10000000abcdef01","hostname":"raze-abcdef01","os":{"name":"helios","version":"2026.4.0"},"device_package":{"version":"1.0.10","commit":null},"update_methods":["image-write","ab-tryboot"],"manage_url":null,"macs":{}}"#,
    )
    .expect("write");
    let (status, body) = call(&state, "GET", "/v1/identity", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["model"], "raze");
    assert_eq!(body["serial"], "10000000abcdef01");
    assert_eq!(body["helios"]["source"], "board");
    assert_eq!(body["helios"]["api_version"], "v1");
    let methods: Vec<&str> = body["update_methods"].as_array().expect("methods").iter().filter_map(|m| m.as_str()).collect();
    assert_eq!(methods, vec!["image-write", "ab-tryboot", "helios-ota"]);
}

#[tokio::test]
async fn identity_falls_back_to_system_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&test_state(dir.path()), "GET", "/v1/identity", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["contract"], 1);
    assert_eq!(body["helios"]["source"], "helios-api");
    assert!(body["update_methods"].as_array().expect("methods").iter().any(|m| m == "image-write"));
}

#[tokio::test]
async fn update_status_works_without_the_writer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "GET", "/v1/update/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["phase"], "unknown");
    let (status, body) = call(&state, "GET", "/v1/ota/state", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["state"].is_object());
}

#[tokio::test]
async fn raw_upload_then_apply_needs_the_device_package_writer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let request = Request::builder().method("POST").uri("/v1/update/uploads?filename=helios-raze-v9.img").body(Body::from(vec![7u8; 1024])).expect("request");
    let response = router(state.clone()).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);
    let upload: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.expect("body")).expect("json");
    assert_eq!(upload["size_bytes"], 1024);
    let (status, list) = call(&state, "GET", "/v1/update/uploads", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().map(Vec::len), Some(1));
    // No /usr/lib/board/update here: applying is not available, and the upload stays.
    let (status, body) = call(&state, "POST", "/v1/update/apply", Some(serde_json::json!({ "upload_id": upload["id"] }))).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    assert_eq!(body["error"]["code"], "not_available");
    let (status, _) = call(&state, "DELETE", &format!("/v1/update/uploads/{}", upload["id"].as_str().expect("id")), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn ui_is_served_outside_v1() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(&ui).expect("mkdir");
    std::fs::write(ui.join("index.html"), "<!doctype html><title>HeliOS</title>").expect("write");
    let mut config = test_state(dir.path()).config.clone();
    config.ui_dir = Some(ui);
    let state = AppState::new(config);
    for path in ["/", "/pipelines/42"] {
        let response = router(state.clone()).oneshot(Request::builder().uri(path).body(Body::empty()).expect("request")).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers()["content-type"].to_str().expect("type").starts_with("text/html"));
    }
    let (status, body) = call(&state, "GET", "/v1/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    let (status, _) = call(&state, "GET", "/missing.js", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_ids_are_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&test_state(dir.path()), "GET", "/v1/pipelines/bad%20id", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
}

#[tokio::test]
async fn field_layouts_list_upload_and_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (status, body) = call(&state, "GET", "/v1/field-layouts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["selected"], "frc2026-andymark");
    assert_eq!(body["layouts"][0]["builtin"], true);
    assert_eq!(body["layouts"][0]["tags"], 32);

    let wpilib = serde_json::json!({
        "tags": [{ "ID": 7, "pose": { "translation": { "x": 1.0, "y": 2.0, "z": 0.5 }, "rotation": { "quaternion": { "W": 1.0, "X": 0.0, "Y": 0.0, "Z": 0.0 } } } }],
        "field": { "length": 16.5, "width": 8.0 }
    });
    let (status, body) = call(&state, "POST", "/v1/field-layouts?name=Practice%20field", Some(wpilib)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["id"], "practice-field");
    assert_eq!(body["format"], "wpilib");
    assert_eq!(body["tags"], 1);

    let (status, body) = call(&state, "GET", "/v1/field-layouts/practice-field", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["layout"]["tags"][0]["id"], 7);
    assert!((body["layout"]["tags"][0]["side_m"].as_f64().expect("side") - 0.1651).abs() < 1e-12, "FRC tag size by default");
    // Eidos's tag frame for a tag facing +X: q = (1/2, -1/2, -1/2, 1/2).
    assert!((body["known_tags"][0]["reference_from_tag"]["rotation"]["y"].as_f64().expect("y") + 0.5).abs() < 1e-12, "{body}");
    let (status, body) = call(&state, "GET", "/v1/field-layouts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["layouts"].as_array().map(Vec::len), Some(2), "{body}");

    let (status, _) = call(&state, "POST", "/v1/field-layouts?name=x", Some(serde_json::json!({ "nope": 1 }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(&state, "POST", "/v1/field-layouts?name=x&id=frc2026-andymark", Some(serde_json::json!({ "fiducials": [] }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "the built-in layout cannot be replaced");
    let (status, _) = call(&state, "DELETE", "/v1/field-layouts/frc2026-andymark", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(&state, "GET", "/v1/field-layouts/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&state, "PUT", "/v1/field-layouts/selected", Some(serde_json::json!({ "id": "nope" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
