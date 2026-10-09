//! Device security at the router: open mode allows everything; secured mode needs a session
//! (with CSRF for mutations) or an API token everywhere but health, identity and auth status.

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::json;
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

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: serde_json::Value,
}

async fn send(state: &crate::SharedState, method: &str, uri: &str, headers: &[(&str, &str)], body: Option<serde_json::Value>) -> Reply {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = match body {
        Some(json) => {
            request = request.header("content-type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state.clone()).oneshot(request.body(body).expect("request")).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    Reply { status, headers, body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null) }
}

/// `helios_session=<value>` from a Set-Cookie header.
fn session_cookie(reply: &Reply) -> String {
    let set_cookie = reply.headers.get("set-cookie").expect("set-cookie").to_str().expect("ascii");
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Strict"), "{set_cookie}");
    set_cookie.split(';').next().expect("pair").to_string()
}

/// Secure the device through the API; returns (cookie, csrf).
async fn secure(state: &crate::SharedState) -> (String, String) {
    let reply = send(state, "POST", "/v1/auth/enable", &[], Some(json!({ "password": "correct horse" }))).await;
    assert_eq!(reply.status, StatusCode::OK, "{:?}", reply.body);
    assert_eq!(reply.body["mode"], "secured");
    assert_eq!(reply.body["via"], "session");
    (session_cookie(&reply), reply.body["csrf_token"].as_str().expect("csrf").to_string())
}

#[tokio::test]
async fn open_mode_allows_everything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let reply = send(&state, "GET", "/v1/auth/status", &[], None).await;
    assert_eq!(reply.body["mode"], "open");
    assert_eq!(reply.body["authenticated"], true);
    assert_eq!(reply.body["via"], "open");
    // Mutations and reads go through without credentials and reach their handlers.
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[], None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&state, "POST", "/v1/update/slots/switch", &[], None).await.status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(send(&state, "GET", "/v1/update/uploads", &[], None).await.status, StatusCode::OK);
    assert_eq!(send(&state, "GET", "/v1/update/status", &[], None).await.status, StatusCode::OK);
    // Token management needs a secured device.
    assert_eq!(send(&state, "POST", "/v1/auth/tokens", &[], Some(json!({ "label": "x" }))).await.status, StatusCode::CONFLICT);
    let identity = send(&state, "GET", "/v1/identity", &[], None).await;
    assert_eq!(identity.body["helios"]["auth"]["mode"], "open");
}

#[tokio::test]
async fn secured_mode_rejects_unauthenticated_requests() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    secure(&state).await;
    let protected = [
        ("POST", "/v1/system/reboot"),
        ("POST", "/v1/system/services/helios-engine.service/restart"),
        ("POST", "/v1/system/processes/1/signal"),
        ("POST", "/v1/update/uploads"),
        ("POST", "/v1/update/apply"),
        ("POST", "/v1/ota/upload"),
        ("POST", "/v1/ota/apply"),
        ("DELETE", "/v1/cameras/cam0/mount"),
        ("PUT", "/v1/pipelines/p"),
        ("POST", "/v1/auth/disable"),
        ("POST", "/v1/auth/password"),
        ("POST", "/v1/auth/tokens"),
        ("GET", "/v1/auth/tokens"),
        ("GET", "/v1/logs"),
        ("GET", "/v1/logs/stream"),
        ("GET", "/v1/events"),
        ("GET", "/v1/metrics"),
        ("GET", "/v1/device"),
        ("GET", "/v1/update/status"),
        ("GET", "/v1/nope"),
    ];
    for (method, uri) in protected {
        let reply = send(&state, method, uri, &[], None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(reply.body["error"]["code"], "unauthorized", "{method} {uri}");
        assert!(reply.headers.contains_key("www-authenticate"), "{method} {uri}");
    }
    // A made-up bearer token or session cookie is no better.
    assert_eq!(send(&state, "POST", "/v1/system/reboot", &[("authorization", "Bearer helios_0000")], None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&state, "POST", "/v1/system/reboot", &[("cookie", "helios_session=abc")], None).await.status, StatusCode::UNAUTHORIZED);

    // Health, a trimmed identity and the auth status stay public.
    assert_eq!(send(&state, "GET", "/v1/health", &[], None).await.status, StatusCode::OK);
    let status = send(&state, "GET", "/v1/auth/status", &[], None).await;
    assert_eq!(status.body["mode"], "secured");
    assert_eq!(status.body["authenticated"], false);
    assert!(status.body.get("tokens").is_none());
    let identity = send(&state, "GET", "/v1/identity", &[], None).await;
    assert_eq!(identity.status, StatusCode::OK);
    assert_eq!(identity.body["helios"]["auth"]["mode"], "secured");
    assert!(identity.body.get("macs").is_none(), "MACs are not public on a secured device");
    assert!(identity.body.get("model").is_some());
}

#[tokio::test]
async fn sessions_need_the_csrf_header_for_mutations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    secure(&state).await;
    assert_eq!(send(&state, "POST", "/v1/auth/login", &[], Some(json!({ "password": "wrong password" }))).await.status, StatusCode::UNAUTHORIZED);
    let login = send(&state, "POST", "/v1/auth/login", &[], Some(json!({ "password": "correct horse" }))).await;
    assert_eq!(login.status, StatusCode::OK);
    let cookie = session_cookie(&login);
    let csrf = login.body["csrf_token"].as_str().expect("csrf").to_string();

    let status = send(&state, "GET", "/v1/auth/status", &[("cookie", &cookie)], None).await;
    assert_eq!(status.body["authenticated"], true);
    assert_eq!(status.body["csrf_token"], csrf.as_str());
    assert_eq!(send(&state, "GET", "/v1/update/uploads", &[("cookie", &cookie)], None).await.status, StatusCode::OK);
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[("cookie", &cookie)], None).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[("cookie", &cookie), ("x-helios-csrf", "nope")], None).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[("cookie", &cookie), ("x-helios-csrf", &csrf)], None).await.status, StatusCode::NO_CONTENT);

    let logout = send(&state, "POST", "/v1/auth/logout", &[("cookie", &cookie)], None).await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&state, "GET", "/v1/update/uploads", &[("cookie", &cookie)], None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_tokens_work_until_revoked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (cookie, csrf) = secure(&state).await;
    let created = send(&state, "POST", "/v1/auth/tokens", &[("cookie", &cookie), ("x-helios-csrf", &csrf)], Some(json!({ "label": "Atlas" }))).await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let token = created.body["token"].as_str().expect("token").to_string();
    let id = created.body["id"].as_str().expect("id").to_string();
    let bearer = format!("Bearer {token}");

    // Tokens need no CSRF header and reach every route.
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[("authorization", &bearer)], None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&state, "GET", "/v1/update/status", &[("authorization", &bearer)], None).await.status, StatusCode::OK);
    assert_eq!(send(&state, "GET", "/v1/auth/status", &[("authorization", &bearer)], None).await.body["via"], "token");
    let identity = send(&state, "GET", "/v1/identity", &[("authorization", &bearer)], None).await;
    assert!(identity.body["helios"]["endpoints"].is_object());

    let list = send(&state, "GET", "/v1/auth/tokens", &[("authorization", &bearer)], None).await;
    let listed = list.body.as_array().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["label"], "Atlas");
    assert!(listed[0].get("token").is_none(), "the secret is shown once");
    assert!(listed[0]["last_used_at_ms"].as_u64().is_some());

    assert_eq!(send(&state, "DELETE", &format!("/v1/auth/tokens/{id}"), &[("cookie", &cookie), ("x-helios-csrf", &csrf)], None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&state, "GET", "/v1/update/status", &[("authorization", &bearer)], None).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn console_reset_returns_to_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    secure(&state).await;
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[], None).await.status, StatusCode::UNAUTHORIZED);
    // What `helios-api auth reset` does on the device.
    assert!(crate::auth_state::reset(&dir.path().join("auth").join("auth.json")).expect("reset"));
    assert_eq!(send(&state, "GET", "/v1/auth/status", &[], None).await.body["mode"], "open");
    assert_eq!(send(&state, "DELETE", "/v1/cameras/cam0/mount", &[], None).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn disabling_security_requires_auth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (cookie, csrf) = secure(&state).await;
    let session = [("cookie", cookie.as_str()), ("x-helios-csrf", csrf.as_str())];

    assert_eq!(send(&state, "POST", "/v1/auth/disable", &[], Some(json!({ "password": "correct horse" }))).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&state, "POST", "/v1/auth/disable", &session, Some(json!({}))).await.status, StatusCode::FORBIDDEN, "a session re-enters the password");
    assert_eq!(send(&state, "POST", "/v1/auth/disable", &session, Some(json!({ "password": "wrong password" }))).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&state, "GET", "/v1/auth/status", &[], None).await.body["mode"], "secured");
    assert_eq!(send(&state, "POST", "/v1/auth/enable", &session, Some(json!({ "password": "another one" }))).await.status, StatusCode::CONFLICT);

    let disabled = send(&state, "POST", "/v1/auth/disable", &session, Some(json!({ "password": "correct horse" }))).await;
    assert_eq!(disabled.status, StatusCode::OK);
    assert_eq!(disabled.body["mode"], "open");
    assert!(!dir.path().join("auth").join("auth.json").exists());

    // An API token alone can turn it off too.
    let (cookie, csrf) = secure(&state).await;
    let created = send(&state, "POST", "/v1/auth/tokens", &[("cookie", &cookie), ("x-helios-csrf", &csrf)], Some(json!({ "label": "script" }))).await;
    let bearer = format!("Bearer {}", created.body["token"].as_str().expect("token"));
    assert_eq!(send(&state, "POST", "/v1/auth/disable", &[("authorization", &bearer)], None).await.status, StatusCode::OK);
    assert_eq!(send(&state, "GET", "/v1/auth/status", &[], None).await.body["mode"], "open");
}

#[tokio::test]
async fn password_change_ends_other_sessions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(dir.path());
    let (cookie, csrf) = secure(&state).await;
    let other = session_cookie(&send(&state, "POST", "/v1/auth/login", &[], Some(json!({ "password": "correct horse" }))).await);
    let body = json!({ "current_password": "correct horse", "new_password": "battery staple" });
    let changed = send(&state, "POST", "/v1/auth/password", &[("cookie", &cookie), ("x-helios-csrf", &csrf)], Some(body)).await;
    assert_eq!(changed.status, StatusCode::OK, "{:?}", changed.body);
    let fresh = session_cookie(&changed);
    assert_eq!(send(&state, "GET", "/v1/update/uploads", &[("cookie", &other)], None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&state, "GET", "/v1/update/uploads", &[("cookie", &fresh)], None).await.status, StatusCode::OK);
    assert_eq!(send(&state, "POST", "/v1/auth/login", &[], Some(json!({ "password": "battery staple" }))).await.status, StatusCode::OK);
}
