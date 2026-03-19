use super::super::error::{ApiError, ApiResult};
use crate::http::AppState;
use crate::http::revision::{apply_revision_headers, matches_if_none_match, not_modified_response};
use crate::logs::LogSource;
use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use tokio_util::io::ReaderStream;

#[utoipa::path(
    get,
    path = "/device/logs",
    tag = "Device",
    responses((status = 200, description = "Logs placeholder", body = [String]))
)]
pub async fn logs(State(state): State<AppState>, Query(params): Query<LogParams>) -> ApiResult<Json<Vec<String>>> {
    let source = params.source.unwrap_or_else(|| "unit:helios-engine.service".to_string());
    let lines = params.lines.unwrap_or(250).clamp(1, 10_000);

    let lines = state.services.system.read_log_lines(&source, lines).await.map_err(|err| if err == "unknown log source" { ApiError::bad_request(err) } else { ApiError::internal(err) })?;
    Ok(Json(lines))
}

#[utoipa::path(
    get,
    path = "/device/logs/download",
    operation_id = "device_logs_download",
    tag = "Device",
    params(
        ("source" = Option<String>, Query, description = "Log source ID"),
        ("lines" = Option<u64>, Query, description = "Optional line limit; omit for full output")
    ),
    responses((status = 200, description = "Log download", content_type = "text/plain"))
)]
pub async fn download(State(state): State<AppState>, Query(params): Query<LogParams>) -> Response {
    let source = params.source.unwrap_or_else(|| "unit:helios-engine.service".to_string());
    let lines = params.lines.map(|value| value.clamp(1, 200_000) as usize);

    let Some(spec) = state.services.system.resolve_log_source(&source).await else {
        return (StatusCode::BAD_REQUEST, "unknown log source").into_response();
    };

    let (mut child, suggested_filename) = match state.services.system.spawn_log_download(&spec, lines) {
        Ok(value) => value,
        Err(err) => return (StatusCode::BAD_GATEWAY, err).into_response(),
    };

    let Some(stdout) = child.stdout.take() else {
        return (StatusCode::BAD_GATEWAY, "log download unavailable").into_response();
    };

    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "text/plain; charset=utf-8".parse().unwrap());
    headers.insert(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{suggested_filename}\"").parse().unwrap());
    (headers, body).into_response()
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct LogParams {
    pub source: Option<String>,
    pub lines: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/device/logs/sources",
    tag = "Device",
    responses((status = 200, description = "Available log sources", body = [LogSource]))
)]
pub async fn sources(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let (payload, revision) = state.services.system.load_log_sources_snapshot().await;
    if matches_if_none_match(&headers, revision) {
        return Ok(not_modified_response(revision));
    }

    let mut response = Json(payload).into_response();
    apply_revision_headers(response.headers_mut(), revision);
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/device/console",
    tag = "Device",
    responses((status = 200, description = "Console placeholder", body = [String]))
)]
pub async fn console() -> ApiResult<Json<Vec<String>>> {
    Ok(Json(Vec::new()))
}
