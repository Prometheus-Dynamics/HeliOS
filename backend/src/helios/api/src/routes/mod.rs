//! The `/v1` routes.

pub mod auth;
pub mod cameras;
pub mod logs;
pub mod pipelines;
pub mod resources;
pub mod system;
pub mod update;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};

use crate::{SharedState, error::ApiError, events};

pub fn router(state: SharedState) -> Router {
    let uploads = Router::new().route("/v1/update/uploads", post(update::upload_raw).get(update::list_uploads)).route("/v1/ota/upload", post(update::atlas_upload)).layer(DefaultBodyLimit::disable());

    Router::new()
        // Device, identity, system
        .route("/v1/health", get(system::health))
        .route("/v1/identity", get(system::identity))
        // Device security (open by default; see auth.rs)
        .route("/v1/auth/status", get(auth::status))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/logout", post(auth::logout))
        .route("/v1/auth/enable", post(auth::enable))
        .route("/v1/auth/disable", post(auth::disable))
        .route("/v1/auth/password", post(auth::change_password))
        .route("/v1/auth/tokens", get(auth::list_tokens).post(auth::create_token))
        .route("/v1/auth/tokens/{id}", delete(auth::revoke_token))
        .route("/v1/device", get(system::device))
        .route("/v1/device/os", get(system::device_os))
        .route("/v1/nodes", get(system::nodes))
        .route("/v1/metrics", get(system::metrics))
        .route("/v1/system/health", get(system::health_report))
        .route("/v1/system/services", get(system::services))
        .route("/v1/system/services/{unit}/restart", post(system::restart_service))
        .route("/v1/system/reboot", post(system::reboot))
        .route("/v1/system/safe-mode", post(system::safe_mode))
        .route("/v1/system/processes", get(system::processes))
        .route("/v1/system/processes/{pid}/signal", post(system::signal_process))
        .route("/v1/system/processes/{pid}/affinity", put(system::process_affinity))
        .route("/v1/system/processes/{pid}/nice", put(system::process_nice))
        // Logs and events
        .route("/v1/logs", get(logs::query))
        .route("/v1/logs/stream", get(logs::stream))
        .route("/v1/events", get(events::events))
        // Cameras
        .route("/v1/cameras", get(cameras::list))
        .route("/v1/cameras/{id}", get(cameras::get_one))
        .route("/v1/cameras/{id}/settings", get(cameras::get_settings).patch(cameras::set_settings))
        .route("/v1/cameras/{id}/mount", get(cameras::get_mount).put(cameras::put_mount).delete(cameras::delete_mount))
        .route("/v1/cameras/{id}/preview", get(cameras::preview))
        .route("/v1/cameras/{id}/calibration", get(cameras::calibration).post(cameras::calibration))
        // Pipelines (Daedalus graphs run by helios-engine) and their outputs
        .route("/v1/catalog", get(pipelines::catalog))
        .route("/v1/plugins", get(pipelines::plugins))
        .route("/v1/pipelines", get(pipelines::list).post(pipelines::create))
        .route("/v1/pipelines/{id}", get(pipelines::get_one).put(pipelines::put).delete(pipelines::remove))
        .route("/v1/pipelines/{id}/start", post(pipelines::start))
        .route("/v1/pipelines/{id}/stop", post(pipelines::stop))
        .route("/v1/pipelines/{id}/restart", post(pipelines::restart))
        .route("/v1/pipelines/{id}/rollback", post(pipelines::rollback))
        .route("/v1/pipelines/{id}/revisions", get(pipelines::revisions))
        .route("/v1/pipelines/{id}/bindings/{input}", put(pipelines::bind_input))
        .route("/v1/pipelines/{id}/outputs", get(pipelines::pipeline_outputs))
        .route("/v1/outputs", get(pipelines::outputs))
        // Resources and peripherals
        .route("/v1/resources", get(resources::list))
        .route("/v1/resources/{id}", get(resources::get_one))
        .route("/v1/peripherals", get(resources::peripherals))
        .route("/v1/peripherals/{id}/actions", post(resources::action))
        .route("/v1/peripherals/fan", get(resources::fan).put(resources::fan))
        .route("/v1/peripherals/leds", get(resources::leds).put(resources::leds))
        .route("/v1/peripherals/imu", get(resources::imu).put(resources::imu))
        // Updates (OTA)
        .route("/v1/update/uploads/{id}", delete(update::delete_upload))
        .route("/v1/update/apply", post(update::apply))
        .route("/v1/update/status", get(update::status))
        .route("/v1/update/events", get(update::events))
        .route("/v1/update/slots/switch", post(update::switch_slot))
        // Atlas's HTTP OTA path (legacy Atlas client contract)
        .route("/v1/ota/apply", post(update::atlas_apply))
        .route("/v1/ota/state", get(update::atlas_state))
        .merge(uploads)
        .fallback(not_found)
        // CORS is outermost so preflights are answered before auth.
        .layer(middleware::from_fn_with_state(state.clone(), auth::middleware))
        .layer(middleware::from_fn_with_state(state.clone(), cors))
        .with_state(state)
}

async fn not_found(request: Request) -> ApiError {
    ApiError::not_found(format!("no route for {} {}", request.method(), request.uri().path()))
}

/// Optional CORS for a UI served from another origin (`HELIOS_API_CORS_ORIGIN`).
async fn cors(State(state): State<SharedState>, request: Request, next: Next) -> Response {
    let Some(origin) = state.config.cors_origin.clone() else {
        return next.run(request).await;
    };
    let preflight = request.method() == Method::OPTIONS;
    let mut response = if preflight { StatusCode::NO_CONTENT.into_response() } else { next.run(request).await };
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("authorization, content-type, x-helios-csrf, x-helios-sha256, x-helios-filename, x-helios-version"));
    response
}

/// A path id: letters, digits, `.`, `_`, `-`, `:`; 1 to 128 characters.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
}

pub fn check_id(id: &str) -> Result<(), ApiError> {
    if valid_id(id) { Ok(()) } else { Err(ApiError::bad_request(format!("invalid id {id:?}"))) }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod auth_tests;
