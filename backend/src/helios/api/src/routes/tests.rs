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
        ota_dir: dir.join("ota"),
        updater_dir: dir.join("updater"),
        upload_dir: dir.join("uploads"),
        pd_identity_path: dir.join("identity.json"),
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
    for uri in ["/v1/pipelines", "/v1/cameras", "/v1/resources", "/v1/peripherals", "/v1/outputs", "/v1/nodes", "/v1/plugins"] {
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
        ("GET", "/v1/cameras/cam0/preview"),
        ("POST", "/v1/cameras/cam0/calibration"),
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
    assert_eq!(body["helios"]["source"], "pd-device");
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
async fn update_status_works_without_orion() {
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
async fn raw_upload_then_apply_reaches_orion() {
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
    // Not a disk image: preparing it fails before Orion is contacted... unless it is a raw image,
    // in which case submission fails on the missing Orion socket. Either way it is not accepted.
    let (status, _) = call(&state, "POST", "/v1/update/apply", Some(serde_json::json!({ "upload_id": upload["id"] }))).await;
    assert!(status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::SERVICE_UNAVAILABLE, "{status}");
    let (status, _) = call(&state, "DELETE", &format!("/v1/update/uploads/{}", upload["id"].as_str().expect("id")), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn bad_ids_are_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (status, body) = call(&test_state(dir.path()), "GET", "/v1/pipelines/bad%20id", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
}
