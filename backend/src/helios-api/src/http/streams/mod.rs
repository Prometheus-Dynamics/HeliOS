mod autostart;
pub(crate) mod bench;
pub(crate) mod bench_util;
pub(crate) mod calibration;
pub(crate) mod controls;
pub(crate) mod lifecycle;
pub(crate) mod mjpeg;
pub(crate) mod pipeline;
pub(crate) mod pose;
mod preview;
mod profiling;
pub(crate) mod recording;
pub(crate) mod replay;
pub(crate) mod replay_bundle;
pub(crate) mod sensor_bench;
pub(crate) mod snapshot;
pub(crate) mod types;
pub(crate) mod util;
pub(crate) mod validation;
mod wait;

use axum::{
    Json, Router,
    extract::Query,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::AppState;
use super::device::rig as rig_device;
use super::device::rig::UpdateCameraPoseRequest;
use super::error::ApiError;
use crate::http::pipelines;
use helios_engine::capture::{CaptureControl, CaptureControlValue};
use helios_engine::ipc::{EngineErrorCode, EngineEvent, GraphOutputPortDescriptor, StreamManifest, StreamPipelineBinding, normalize_pipeline_output_selection};

use self::types::{CodecInfo, StartStreamResponse, StreamFormatInfo, StreamInfo, StreamInspectInfo, UpdateStreamResponse};
use self::validation::{StreamCapabilitiesResponse, StreamValidateResponse, stream_capabilities, validate_stream_manifest_with_runtime};

pub(crate) use helios_engine::contracts::stream_ids::{CALIBRATION_MODE_PIPELINE_UUID, RAW_PIPELINE_UUID};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_streams).post(start_stream))
        .route("/inspect", get(list_stream_inspect))
        .route("/validate", post(validate_stream))
        .route("/capabilities", get(stream_capabilities_handler))
        .route("/replay/media", post(replay::start_media_replay_stream))
        .route("/backends", get(list_backends))
        .route("/bench/formats", post(bench_formats))
        .route("/bench/sensor", post(sensor_bench::start_sensor_benchmark).get(sensor_bench::list_sensor_benchmarks))
        .route("/bench/sensor/{id}", get(sensor_bench::get_sensor_benchmark))
        .route("/bench/sensor/{id}/cancel", post(sensor_bench::cancel_sensor_benchmark))
        .route("/codecs", get(list_codecs))
        .route("/{id}", get(get_stream).put(update_stream).delete(delete_stream))
        .route("/{id}/inspect", get(get_stream_inspect))
        .route("/{id}/controls", get(get_controls))
        .route("/{id}/controls/{control_id}", post(set_control))
        .route("/{id}/metrics", get(get_metrics))
        .route("/{id}/format", get(stream_format))
        .route("/{id}/pipeline/output", post(pipeline::set_pipeline_output))
        .route("/{id}/pipeline/layout", post(pipeline::set_pipeline_layout))
        .route("/{id}/pipeline/wires", post(pipeline::set_pipeline_wires))
        .route("/{id}/pipeline/outputs", get(pipeline::list_pipeline_outputs))
        .route("/{id}/pipeline/outputs/{port}/sample", get(pipeline::get_pipeline_output_sample))
        .route("/{id}/pipeline/smoke", get(pipeline::smoke_pipeline_graph))
        .route("/{id}/pipeline/graph", post(pipeline::set_pipeline_graph))
        .route("/{id}/pipeline/graph/patch", post(pipeline::set_pipeline_graph_patch))
        .route("/{id}/pipeline/inputs", post(pipeline::set_pipeline_inputs))
        .route("/{id}/pipeline/profile", post(profiling::profile_pipeline))
        .route("/{id}/pipeline/perf", post(profiling::set_pipeline_perf))
        .route("/{id}/pipeline/metrics/reset", post(profiling::reset_pipeline_metrics))
        .route("/{id}/pipeline/flamegraph.svg", get(profiling::download_pipeline_flamegraph_svg))
        .route("/{id}/preview", get(preview_stream))
        .route("/{id}/frame", get(frame_jpeg))
        .route("/{id}/snapshot", post(snapshot::capture_snapshot))
        .route("/{id}/crop", post(snapshot::set_crop))
        .route("/{id}/crosshair", post(snapshot::set_crosshair))
        .route("/{id}/ordering", post(snapshot::set_ordering))
        .route("/{id}/pipeline/input-usage", get(snapshot::get_input_usage))
        .route("/{id}/calibration/board", get(calibration::board_png))
        .route("/{id}/calibration/board.pdf", get(calibration::board_pdf))
        .route("/{id}/recording/start", post(recording::start_recording))
        .route("/{id}/recording/stop", post(recording::stop_recording))
        .route("/{id}/recording/capture", post(recording::capture_shadow_recording))
        .route("/{id}/replay/start", post(replay_bundle::start_replay_bundle))
        .route("/{id}/replay/stop", post(replay_bundle::stop_replay_bundle))
        .route("/{id}/calibration/apply", post(calibration::apply_calibration))
        .route("/{id}/calibration/save", post(calibration::save_calibration))
        .route("/{id}/calibration/mode", post(calibration::set_calibration_mode))
        .route("/{id}/pose", put(pose::update_stream_pose).delete(pose::clear_stream_pose))
}

pub async fn reconcile_startup_streams(state: AppState, reason: &'static str) {
    autostart::reconcile_startup_streams(state, reason).await;
}

pub(crate) async fn restart_stream_with_manifest(state: AppState, manifest: StreamManifest) -> Result<(), StatusCode> {
    let response = match manifest.identity.id {
        Some(stream_id) => lifecycle::update_stream(state, stream_id, manifest).await,
        None => lifecycle::start_stream(state, manifest).await,
    };
    let status = response.status();
    if status == StatusCode::OK { Ok(()) } else { Err(status) }
}

#[utoipa::path(
    get,
    path = "/streams",
    tag = "EngineStreams",
    responses((status = 200, description = "List active streams", body = [StreamInfo]))
)]
async fn list_streams(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    lifecycle::list_streams(state, headers).await
}

#[utoipa::path(
    get,
    path = "/streams/inspect",
    tag = "EngineStreams",
    responses((status = 200, description = "Inspect active streams with resolved config, descriptor, codec chain, and demand state", body = [StreamInspectInfo]))
)]
async fn list_stream_inspect(State(state): State<AppState>, headers: axum::http::HeaderMap) -> impl IntoResponse {
    lifecycle::list_stream_inspect(state, headers).await
}

#[utoipa::path(
    post,
    path = "/streams",
    tag = "EngineStreams",
    request_body = StreamManifest,
    responses(
        (status = 200, description = "Stream started", body = StartStreamResponse),
        (status = 422, description = "Semantic validation failure", body = crate::http::validation::ValidationErrorBody)
    )
)]
async fn start_stream(State(state): State<AppState>, Json(manifest): Json<StreamManifest>) -> impl IntoResponse {
    lifecycle::start_stream(state, manifest).await
}

#[utoipa::path(
    put,
    path = "/streams/{id}",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = StreamManifest,
    responses(
        (status = 200, description = "Stream updated", body = UpdateStreamResponse),
        (status = 404, description = "Stream not found"),
        (status = 422, description = "Semantic validation failure", body = crate::http::validation::ValidationErrorBody)
    )
)]
async fn update_stream(State(state): State<AppState>, Path(id): Path<Uuid>, Json(manifest): Json<StreamManifest>) -> impl IntoResponse {
    lifecycle::update_stream(state, id, manifest).await
}

#[utoipa::path(
    post,
    path = "/streams/validate",
    tag = "EngineStreams",
    request_body = StreamManifest,
    responses(
        (status = 200, description = "Validated + canonicalized stream manifest", body = StreamValidateResponse),
        (status = 502, description = "Runtime capability inventory unavailable", body = crate::http::streams::types::EngineErrorBody),
        (status = 422, description = "Semantic validation failure", body = crate::http::validation::ValidationErrorBody)
    )
)]
async fn validate_stream(State(state): State<AppState>, Json(manifest): Json<StreamManifest>) -> impl IntoResponse {
    let runtime = match util::resolve_stream_runtime_capabilities(&state).await {
        Ok(runtime) => runtime,
        Err(body) => return (StatusCode::BAD_GATEWAY, Json(body)).into_response(),
    };
    match validate_stream_manifest_with_runtime(manifest, &runtime).await {
        Ok(result) => Json(StreamValidateResponse { manifest: result.manifest, resolved: result.resolved, warnings: result.warnings }).into_response(),
        Err(err) => crate::http::validation::validation_error_response("stream manifest failed semantic validation", err.issues, err.warnings),
    }
}

#[utoipa::path(
    get,
    path = "/streams/capabilities",
    tag = "EngineStreams",
    responses(
        (status = 200, description = "Stream validation constraints and defaults", body = StreamCapabilitiesResponse),
        (status = 502, description = "Runtime capability inventory unavailable", body = crate::http::streams::types::EngineErrorBody)
    )
)]
async fn stream_capabilities_handler(State(state): State<AppState>) -> impl IntoResponse {
    match util::resolve_stream_runtime_capabilities(&state).await {
        Ok(runtime) => Json(stream_capabilities(&runtime)).into_response(),
        Err(body) => (StatusCode::BAD_GATEWAY, Json(body)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/streams/bench/formats",
    tag = "EngineStreams",
    request_body = bench::BenchFormatsRequest,
    responses((status = 200, description = "Benchmark results", body = bench::BenchFormatsResponse))
)]
async fn bench_formats(State(state): State<AppState>, Json(req): Json<bench::BenchFormatsRequest>) -> impl IntoResponse {
    bench::bench_formats(State(state), Json(req)).await
}

#[utoipa::path(
    delete,
    path = "/streams/{id}",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 204, description = "Stream stopped"))
)]
async fn delete_stream(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    lifecycle::delete_stream(state, id).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}/inspect",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Inspect one stream with resolved config, descriptor, codec chain, and demand state", body = StreamInspectInfo))
)]
async fn get_stream_inspect(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    lifecycle::get_stream_inspect(state, id).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}/controls",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Current controls", body = [CaptureControl]))
)]
async fn get_controls(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    controls::get_controls(state, id).await
}

#[utoipa::path(
    post,
    path = "/streams/{id}/controls/{control_id}",
    tag = "EngineStreams",
    params(
        ("id" = Uuid, Path, description = "Stream ID"),
        ("control_id" = u32, Path, description = "Control ID")
    ),
    request_body = CaptureControlValue,
    responses((status = 204, description = "Control updated"))
)]
async fn set_control(State(state): State<AppState>, Path((id, control_id)): Path<(Uuid, u32)>, Json(value): Json<CaptureControlValue>) -> impl IntoResponse {
    controls::set_control(state, id, control_id, value).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}/metrics",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Stream metrics", body = helios_engine::stream::StreamMetrics))
)]
async fn get_metrics(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    controls::get_metrics(state, id).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}/format",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Latest encoded payload format info", body = StreamFormatInfo))
)]
async fn stream_format(Path(id): Path<Uuid>, Query(query): Query<preview::PreviewSelectionQuery>) -> impl IntoResponse {
    preview::stream_format(id, query).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}/preview",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID"), preview::PreviewSelectionQuery),
    responses((status = 200, description = "Live stream preview (MJPEG multipart or length-prefixed encoded)", content_type = "application/octet-stream"))
)]
async fn preview_stream(State(state): State<AppState>, Path(id): Path<Uuid>, Query(query): Query<preview::PreviewSelectionQuery>) -> impl IntoResponse {
    preview::preview_stream(state, id, query).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}/frame",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Latest JPEG frame", content_type = "image/jpeg"))
)]
async fn frame_jpeg(State(state): State<AppState>, Path(id): Path<Uuid>, Query(query): Query<preview::PreviewSelectionQuery>) -> impl IntoResponse {
    preview::frame_jpeg(state, id, query).await
}

#[utoipa::path(
    get,
    path = "/streams/{id}",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Stream info", body = StreamInfo), (status = 404, description = "Stream not found"))
)]
async fn get_stream(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    lifecycle::get_stream(state, id).await
}

#[utoipa::path(
    get,
    path = "/streams/backends",
    tag = "EngineStreams",
    responses((status = 200, description = "Available capture backends/devices", body = [helios_engine::capture::DiscoveredDevice]))
)]
async fn list_backends(State(state): State<AppState>) -> impl IntoResponse {
    lifecycle::list_backends(state).await
}

#[utoipa::path(
    get,
    path = "/streams/codecs",
    tag = "EngineStreams",
    responses(
        (status = 200, description = "Available codecs (encoders + decoders)", body = [CodecInfo]),
        (status = 502, description = "Runtime capability inventory unavailable", body = crate::http::streams::types::EngineErrorBody)
    )
)]
async fn list_codecs(State(state): State<AppState>) -> impl IntoResponse {
    lifecycle::list_codecs(state).await
}
