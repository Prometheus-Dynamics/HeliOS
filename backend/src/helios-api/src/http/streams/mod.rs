mod autostart;
pub(crate) mod bench;
pub(crate) mod bench_util;
pub(crate) mod calibration;
pub(crate) mod controls;
pub(crate) mod lifecycle;
pub(crate) mod mjpeg;
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
/// Reserved internal pipeline UUID used by the engine for transient calibration-mode graphs.
///
/// This must never be persisted or exposed as a user-selectable pipeline; it has no on-disk
/// graph document and will break stream restart/apply if it leaks into saved layouts.

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
        .route("/{id}/pipeline/output", post(set_pipeline_output))
        .route("/{id}/pipeline/layout", post(set_pipeline_layout))
        .route("/{id}/pipeline/wires", post(set_pipeline_wires))
        .route("/{id}/pipeline/outputs", get(list_pipeline_outputs))
        .route("/{id}/pipeline/outputs/{port}/sample", get(get_pipeline_output_sample))
        .route("/{id}/pipeline/smoke", get(smoke_pipeline_graph))
        .route("/{id}/pipeline/graph", post(set_pipeline_graph))
        .route("/{id}/pipeline/graph/patch", post(set_pipeline_graph_patch))
        .route("/{id}/pipeline/inputs", post(set_pipeline_inputs))
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
        .route("/{id}/pose", put(update_stream_pose).delete(clear_stream_pose))
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct SetPipelineOutputRequest {
    #[serde(default)]
    output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct SetPipelineGraphRequest {
    graph: serde_json::Value,
    #[serde(default)]
    pipeline_id: Option<Uuid>,
    #[serde(default)]
    output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct SetPipelineGraphPatchRequest {
    patch: serde_json::Value,
    #[serde(default)]
    pipeline_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct SetPipelineInputsRequest {
    #[serde(default)]
    pipeline_id: Option<Uuid>,
    inputs: std::collections::BTreeMap<String, Option<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct SetPipelineLayoutRequest {
    #[serde(default)]
    pipeline_layout: Option<helios_engine::ipc::StreamPipelineLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
struct SetPipelineWiresRequest {
    #[serde(default)]
    wires: Vec<helios_engine::ipc::StreamPipelineWire>,
}

fn normalize_output_for_pipeline(output: Option<String>, pipeline_id: Option<Uuid>) -> Option<String> {
    let normalized = output.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    });
    match normalize_pipeline_output_selection(normalized.as_deref(), pipeline_id) {
        Ok(output) => output,
        Err(_) => normalized,
    }
}

fn single_view_slot_pipeline_id(manifest: &helios_engine::ipc::StreamManifest) -> Option<Uuid> {
    let layout = manifest.pipeline_layout.as_ref()?;
    if layout.rows != 1 || layout.columns != 1 {
        return None;
    }
    layout.slots.iter().find(|slot| slot.row == 0 && slot.column == 0).and_then(|slot| slot.pipeline_id).or_else(|| layout.slots.iter().find_map(|slot| slot.pipeline_id))
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/output",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelineOutputRequest,
    responses((status = 204, description = "Pipeline output updated"))
)]
async fn set_pipeline_output(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelineOutputRequest>) -> impl IntoResponse {
    let requested_output = req.output.clone();
    let requested_output_for_manifest = requested_output.clone();
    let apply_output = |manifest: &mut helios_engine::ipc::StreamManifest| {
        let active_pipeline_id = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|binding| binding.pipeline_id));
        let view_pipeline_id = single_view_slot_pipeline_id(manifest).or(active_pipeline_id);
        let output_targets_active_pipeline = view_pipeline_id == active_pipeline_id;
        let normalized_output = normalize_output_for_pipeline(requested_output_for_manifest.clone(), view_pipeline_id);

        let implicit_raw_view = view_pipeline_id.is_none() && manifest.pipelines.is_empty();
        if (view_pipeline_id == Some(RAW_PIPELINE_UUID) || implicit_raw_view)
            && normalized_output.as_deref().is_some_and(|v| v.trim().eq_ignore_ascii_case("undistorted"))
            && !manifest.pipeline_enabled
        {
            // Allow RAW stream view selection to re-enable pipelines.
            manifest.pipeline_enabled = true;
        }

        if output_targets_active_pipeline {
            manifest.active_pipeline_output = normalized_output.clone();
        }
        if let Some(target_id) = view_pipeline_id {
            if let Some(binding) = manifest.pipelines.iter_mut().find(|p| p.pipeline_id == target_id) {
                binding.pipeline_output = normalized_output.clone();
            }
        } else if let Some(binding) = manifest.pipelines.first_mut() {
            binding.pipeline_output = normalized_output.clone();
            manifest.active_pipeline_id = Some(binding.pipeline_id);
        }

        if let Some(layout) = manifest.pipeline_layout.as_mut()
            && layout.rows == 1
            && layout.columns == 1
            && let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0)
        {
            slot.output_key = normalized_output.clone();
        }
    };

    match state.engine.set_graph_output(id, requested_output).await {
        Ok(EngineEvent::Ack { .. }) => {
            if let Err(err) = util::persist_live_stream_manifest_update(&state, id, |manifest| {
                apply_output(manifest);
            })
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, apply_output).await {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };

            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) if util::is_engine_unavailable(&err) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, apply_output).await {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };

            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Err(err) => util::map_client_error(err),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/layout",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelineLayoutRequest,
    responses((status = 204, description = "Pipeline layout updated"))
)]
async fn set_pipeline_layout(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelineLayoutRequest>) -> impl IntoResponse {
    let apply_layout = |manifest: &mut helios_engine::ipc::StreamManifest| {
        manifest.pipeline_layout = req.pipeline_layout.clone();
        if manifest.pipeline_layout.is_some() {
            manifest.pipeline_enabled = true;
        }
        if let Some(layout) = manifest.pipeline_layout.as_mut() {
            let previous_active_pipeline_id = manifest.active_pipeline_id;
            // Never persist the internal calibration-mode pipeline into user layouts.
            for slot in &mut layout.slots {
                if slot.pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
                    slot.pipeline_id = None;
                    slot.output_key = None;
                }
                slot.output_key = normalize_output_for_pipeline(slot.output_key.clone(), slot.pipeline_id);
            }

            let mut existing: std::collections::BTreeSet<Uuid> = manifest.pipelines.iter().map(|binding| binding.pipeline_id).collect();
            let mut layout_selected_ids = std::collections::BTreeSet::new();
            for slot in &layout.slots {
                let Some(pipeline_id) = slot.pipeline_id else { continue };
                if pipeline_id == CALIBRATION_MODE_PIPELINE_UUID {
                    continue;
                }
                layout_selected_ids.insert(pipeline_id);
                if pipeline_id == RAW_PIPELINE_UUID {
                    continue;
                }
                if existing.insert(pipeline_id) {
                    manifest.pipelines.push(helios_engine::ipc::StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: None });
                }
            }
            let single_slot_layout = layout.rows == 1 && layout.columns == 1;
            let active_in_layout = manifest.active_pipeline_id.is_some_and(|id| layout_selected_ids.contains(&id));
            let should_retarget_active = single_slot_layout || !active_in_layout;
            let selected_slot = layout
                .slots
                .iter()
                .find(|slot| slot.row == 0 && slot.column == 0 && slot.pipeline_id.is_some() && slot.pipeline_id != Some(CALIBRATION_MODE_PIPELINE_UUID))
                .or_else(|| layout.slots.iter().find(|slot| slot.pipeline_id.is_some() && slot.pipeline_id != Some(CALIBRATION_MODE_PIPELINE_UUID)))
                .and_then(|slot| slot.pipeline_id.map(|pipeline_id| (pipeline_id, slot.output_key.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string()))));
            if should_retarget_active {
                if let Some((active_id, slot_output)) = selected_slot {
                    manifest.active_pipeline_id = Some(active_id);
                    let active_changed = previous_active_pipeline_id != Some(active_id);
                    if active_id == RAW_PIPELINE_UUID {
                        // Preserve RAW output selection (`raw` vs `undistorted`) when present.
                        if let Some(slot_output) = slot_output {
                            manifest.active_pipeline_output = Some(slot_output);
                        } else if active_changed {
                            manifest.active_pipeline_output = manifest.pipelines.iter().find(|p| p.pipeline_id == active_id).and_then(|binding| binding.pipeline_output.clone());
                        }
                    } else if active_changed || manifest.active_pipeline_output.is_none() {
                        manifest.active_pipeline_output = manifest.pipelines.iter().find(|p| p.pipeline_id == active_id).and_then(|binding| binding.pipeline_output.clone());
                    }
                } else {
                    manifest.active_pipeline_id = None;
                    manifest.active_pipeline_output = None;
                }
            }
        }
    };
    match state.engine.set_pipeline_layout(id, req.pipeline_layout.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            if let Err(err) = util::persist_live_stream_manifest_update(&state, id, |manifest| {
                apply_layout(manifest);
            })
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, |manifest| {
                apply_layout(manifest);
            })
            .await
            {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };
            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) if util::is_engine_unavailable(&err) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, |manifest| {
                apply_layout(manifest);
            })
            .await
            {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };
            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Err(err) => util::map_client_error(err),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/wires",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelineWiresRequest,
    responses((status = 204, description = "Pipeline wires updated"))
)]
async fn set_pipeline_wires(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelineWiresRequest>) -> impl IntoResponse {
    let mut wires = req.wires.clone();
    // Never allow internal calibration-mode pipeline IDs in persisted wiring.
    wires.retain(|wire| wire.from.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID && wire.to.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);

    let apply_wires = |manifest: &mut helios_engine::ipc::StreamManifest| {
        manifest.pipeline_wires = wires.clone();
        if !manifest.pipeline_wires.is_empty() {
            manifest.pipeline_enabled = true;
        }
    };

    match state.engine.set_pipeline_wires(id, wires.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            if let Err(err) = util::persist_live_stream_manifest_update(&state, id, |manifest| apply_wires(manifest)).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, |manifest| apply_wires(manifest)).await {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };
            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) if util::is_engine_unavailable(&err) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, |manifest| apply_wires(manifest)).await {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };
            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Err(err) => util::map_client_error(err),
    }
}

#[utoipa::path(
    get,
    path = "/streams/{id}/pipeline/outputs",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Pipeline graph host outputs", body = [helios_engine::ipc::GraphOutputPortDescriptor]))
)]
async fn list_pipeline_outputs(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match state.engine.list_graph_outputs_event(id).await {
        Ok(EngineEvent::GraphOutputs { mut outputs, .. }) => {
            if stream_uses_raw_pipeline(&state, id).await {
                ensure_raw_undistorted_outputs(&mut outputs);
            }
            strip_raw_frame_alias_output(&mut outputs);
            Json(outputs).into_response()
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => util::map_client_error(err),
    }
}

async fn stream_uses_raw_pipeline(state: &AppState, stream_id: Uuid) -> bool {
    let Ok(streams) = state.engine.list_streams().await else {
        return false;
    };
    let Some(stream) = streams.iter().find(|entry| entry.stream_id == stream_id) else {
        return false;
    };
    let manifest = &stream.manifest;
    if manifest.active_pipeline_id == Some(RAW_PIPELINE_UUID) {
        return true;
    }
    manifest.pipeline_layout.as_ref().is_some_and(|layout| layout.slots.iter().any(|slot| slot.pipeline_id == Some(RAW_PIPELINE_UUID)))
}

fn ensure_raw_undistorted_outputs(outputs: &mut Vec<GraphOutputPortDescriptor>) {
    let raw_template = outputs.iter().find(|entry| entry.name.eq_ignore_ascii_case("raw")).cloned();
    let has_undistorted = outputs.iter().any(|entry| entry.name.eq_ignore_ascii_case("undistorted"));
    if let Some(raw) = raw_template
        && !has_undistorted
    {
        outputs.push(GraphOutputPortDescriptor { name: "undistorted".to_string(), ty: raw.ty, previewable: raw.previewable });
    }
}

fn strip_raw_frame_alias_output(outputs: &mut Vec<GraphOutputPortDescriptor>) {
    let has_canonical_raw = outputs.iter().any(|entry| entry.name.eq_ignore_ascii_case("raw") || entry.name.eq_ignore_ascii_case("undistorted"));
    if has_canonical_raw {
        outputs.retain(|entry| !entry.name.eq_ignore_ascii_case("frame"));
    }
}

#[utoipa::path(
    get,
    path = "/streams/{id}/pipeline/outputs/{port}/sample",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID"), ("port" = String, Path, description = "Host output port name")),
    responses((status = 200, description = "Latest JSON sample captured from the port", body = serde_json::Value))
)]
async fn get_pipeline_output_sample(State(state): State<AppState>, Path((id, port)): Path<(Uuid, String)>) -> impl IntoResponse {
    match state.engine.get_graph_output_sample_event(id, port.clone()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => Json(value.0).into_response(),
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => util::map_client_error(err),
    }
}

#[derive(Debug, Clone, Deserialize, IntoParams, ToSchema)]
struct SmokePipelineQuery {
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
struct SmokePipelineResponse {
    ok: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/streams/{id}/pipeline/smoke",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID"), SmokePipelineQuery),
    responses((status = 200, description = "Runtime pipeline health snapshot", body = SmokePipelineResponse))
)]
async fn smoke_pipeline_graph(State(state): State<AppState>, Path(id): Path<Uuid>, Query(query): Query<SmokePipelineQuery>) -> impl IntoResponse {
    let timeout_ms = query.timeout_ms.unwrap_or(500).clamp(0, 5_000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let metrics = match state.engine.get_metrics(id).await {
            Ok(EngineEvent::Metrics { metrics, .. }) => metrics,
            Ok(EngineEvent::Nack { code, reason, .. }) => return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
            Ok(_) => return (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
            Err(err) => return util::map_client_error(err),
        };

        let mut errors = Vec::new();
        if let Some(pipeline) = metrics.pipeline {
            for (id, node) in pipeline.nodes {
                if let Some(err) = node.last_error {
                    let label = node.node_label.or(node.node_type).unwrap_or_else(|| id.clone());
                    errors.push(format!("{label}: {err}"));
                }
            }
        }
        if errors.is_empty() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        }
        return Json(SmokePipelineResponse { ok: errors.is_empty(), errors }).into_response();
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/graph",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelineGraphRequest,
    responses((status = 204, description = "Pipeline graph updated"))
)]
async fn set_pipeline_graph(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelineGraphRequest>) -> impl IntoResponse {
    let Some(pipeline_id) = req.pipeline_id else {
        return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), "pipeline_id is required"))).into_response();
    };
    if pipeline_id == RAW_PIPELINE_UUID || pipeline_id == CALIBRATION_MODE_PIPELINE_UUID {
        return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), "reserved pipeline ids cannot be set via /streams/{id}/pipeline/graph"))).into_response();
    }
    match pipelines::load_graph_document(pipeline_id).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), format!("pipeline {pipeline_id} is not persisted under /pipelines/graphs"))))
                .into_response();
        }
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to load pipeline {pipeline_id}: {err}")))).into_response();
        }
    }
    match state.engine.set_graph(id, req.graph.clone(), Some(pipeline_id), req.output.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            if let Err(err) = util::persist_live_stream_manifest_update(&state, id, |manifest| {
                // `/streams/:id/pipeline/graph` updates (or inserts) a single pipeline binding.
                // Importantly: do not clear existing multiplex layout state, since users can
                // assign/tune pipelines while a multiplex grid is configured.
                let mut updated = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        if req.output.is_some() {
                            binding.pipeline_output = req.output.clone();
                        }
                        binding.pipeline_patch = None;
                        updated = true;
                        break;
                    }
                }
                if !updated {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: req.output.clone(), pipeline_patch: None });
                }
                manifest.pipeline_enabled = true;
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
                if req.output.is_some() && manifest.active_pipeline_id == Some(pipeline_id) {
                    manifest.active_pipeline_output = req.output.clone();
                }
                util::promote_single_view_pipeline_selection(manifest, pipeline_id, req.output.clone());
            })
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, |manifest| {
                let mut replaced = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        if req.output.is_some() {
                            binding.pipeline_output = req.output.clone();
                        }
                        binding.pipeline_patch = None;
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: req.output.clone(), pipeline_patch: None });
                }

                manifest.pipeline_enabled = true;
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
                if req.output.is_some() && manifest.active_pipeline_id == Some(pipeline_id) {
                    manifest.active_pipeline_output = req.output.clone();
                }
                util::promote_single_view_pipeline_selection(manifest, pipeline_id, req.output.clone());
            })
            .await
            {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };

            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => util::map_client_error(err),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/graph/patch",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelineGraphPatchRequest,
    responses((status = 204, description = "Pipeline graph patch applied"))
)]
async fn set_pipeline_graph_patch(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelineGraphPatchRequest>) -> impl IntoResponse {
    let Some(pipeline_id) = req.pipeline_id else {
        return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), "pipeline_id is required"))).into_response();
    };
    if pipeline_id == RAW_PIPELINE_UUID || pipeline_id == CALIBRATION_MODE_PIPELINE_UUID {
        return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), "reserved pipeline ids cannot be patched via /streams/{id}/pipeline/graph/patch")))
            .into_response();
    }
    match pipelines::load_graph_document(pipeline_id).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), format!("pipeline {pipeline_id} is not persisted under /pipelines/graphs"))))
                .into_response();
        }
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to load pipeline {pipeline_id}: {err}")))).into_response();
        }
    }
    let patch = req.patch.clone();
    match state.engine.set_graph_patch(id, patch.clone(), Some(pipeline_id)).await {
        Ok(EngineEvent::Ack { .. }) => {
            if let Err(err) = util::persist_live_stream_manifest_update(&state, id, |manifest| {
                let mut updated = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        binding.pipeline_patch = Some(helios_engine::ipc::JsonWire(patch.clone()));
                        updated = true;
                        break;
                    }
                }
                if !updated {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: Some(helios_engine::ipc::JsonWire(patch.clone())) });
                }
                manifest.pipeline_enabled = true;
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
                util::promote_single_view_pipeline_selection(manifest, pipeline_id, None);
            })
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code: EngineErrorCode::NotFound, .. }) => {
            let updated = match util::update_persisted_manifest_by_stream_id_checked(id, |manifest| {
                let mut replaced = false;
                for binding in &mut manifest.pipelines {
                    if binding.pipeline_id == pipeline_id {
                        binding.pipeline_patch = Some(helios_engine::ipc::JsonWire(patch.clone()));
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    manifest.pipelines.push(StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: Some(helios_engine::ipc::JsonWire(patch.clone())) });
                }

                manifest.pipeline_enabled = true;
                if manifest.active_pipeline_id.is_none() {
                    manifest.active_pipeline_id = Some(pipeline_id);
                }
                util::promote_single_view_pipeline_selection(manifest, pipeline_id, None);
            })
            .await
            {
                Ok(updated) => updated,
                Err(err) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist stream manifest: {err}")))).into_response();
                }
            };
            if updated.is_some() { StatusCode::NO_CONTENT.into_response() } else { StatusCode::NOT_FOUND.into_response() }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(code), format!("engine rejected patch: {reason}")))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), "unexpected engine response"))).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::InvalidState), format!("engine error: {err}")))).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/inputs",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelineInputsRequest,
    responses((status = 204, description = "Pipeline inputs updated"))
)]
async fn set_pipeline_inputs(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelineInputsRequest>) -> impl IntoResponse {
    let inputs = req.inputs.clone();
    match state.engine.set_pipeline_inputs(id, req.pipeline_id, inputs.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            if let Err(err) = util::persist_live_stream_manifest_update(&state, id, |manifest| {
                util::apply_pipeline_host_inputs_update(manifest, &inputs);
            })
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(util::engine_error_body(Some(code), reason))).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), err.to_string()))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(util::engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
    }
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
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses(
        (status = 200, description = "Live stream preview (MJPEG multipart or length-prefixed encoded)", content_type = "application/octet-stream")
    )
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

async fn resolve_camera_uid(state: &AppState, stream_id: Uuid) -> Result<String, ApiError> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(err.to_string()))?;
    let stream = streams.iter().find(|summary| summary.stream_id == stream_id).ok_or_else(|| ApiError::not_found("stream not found"))?;
    let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref()).unwrap_or("");
    let camera_uid = rig_device::camera_uid_from_keys(&stream.manifest.capture.device_keys, Some(fallback)).unwrap_or_else(|| stream_id.to_string());
    Ok(camera_uid)
}

#[utoipa::path(
    put,
    path = "/streams/{id}/pose",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = UpdateCameraPoseRequest,
    responses((status = 204, description = "Pose updated"))
)]
pub async fn update_stream_pose(State(state): State<AppState>, Path(id): Path<Uuid>, body: Json<UpdateCameraPoseRequest>) -> Response {
    let Json(req) = body;
    match resolve_camera_uid(&state, id).await {
        Ok(camera_uid) => rig_device::update_camera_pose(State(state.clone()), Path(camera_uid), Json(req)).await.into_response(),
        Err(err) => err.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/streams/{id}/pose",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 204, description = "Pose cleared"))
)]
pub async fn clear_stream_pose(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    match resolve_camera_uid(&state, id).await {
        Ok(camera_uid) => rig_device::clear_camera_pose(State(state.clone()), Path(camera_uid)).await.into_response(),
        Err(err) => err.into_response(),
    }
}
