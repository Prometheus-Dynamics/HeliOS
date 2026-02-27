use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use helios_engine::{
    capture::{BackendHandle, BackendKind, ControlAssignment},
    ipc::StreamManifest,
};
use serde::{Deserialize, Serialize};
use styx::prelude::FourCc;
use utoipa::ToSchema;

use crate::http::AppState;

use super::bench_util::{
    StartStreamArgs, codec_impl_cache, discover_devices, find_matching_backend, identity_for_keys, restore_streams_best_effort, sample_stream_metrics, start_stream_for_mode, stop_conflicting_streams,
    stop_stream_best_effort,
};

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BenchFormatsRequest {
    pub backend: BackendKind,
    pub handle: BackendHandle,
    #[serde(default)]
    pub device_keys: Vec<String>,
    /// Optional explicit mode list to benchmark (skips device probing on the server).
    ///
    /// When present, the server will benchmark exactly these mode ids and render each group using
    /// the provided `format` (FOURCC) string.
    #[serde(default)]
    pub modes: Vec<BenchModeRef>,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_target_fps")]
    pub target_fps: u32,
    #[serde(default = "default_sample_ms")]
    pub sample_ms: u64,
    /// Stop conflicting streams while benchmarking, then restore them.
    #[serde(default = "default_restore_existing")]
    pub restore_existing: bool,
    /// Capture controls to apply during benchmark runs (e.g., exposure).
    #[serde(default)]
    pub controls: Vec<ControlAssignment>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BenchModeRef {
    /// `ModeId` from `/streams/backends` for the selected resolution.
    pub id: styx::capture::ModeId,
}

fn default_target_fps() -> u32 {
    120
}

fn default_sample_ms() -> u64 {
    1500
}

fn default_restore_existing() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BenchCodecStat {
    pub implementation: String,
    pub avg_ms: f64,
    pub avg_fps: f64,
    #[serde(default)]
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BenchFormatGroup {
    pub format: String,
    #[serde(default)]
    pub decoders: Vec<BenchCodecStat>,
    #[serde(default)]
    pub encoders: Vec<BenchCodecStat>,
    /// Capture stage throughput for this format (context).
    pub capture_avg_fps: f64,
    pub host_avg_fps: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BenchFormatsResponse {
    pub formats: Vec<BenchFormatGroup>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

pub async fn bench_formats(State(state): State<AppState>, Json(req): Json<BenchFormatsRequest>) -> axum::response::Response {
    let (keys, backend) = if req.device_keys.is_empty() || req.modes.is_empty() {
        let devices = discover_devices().await;
        let Some((device, backend)) = find_matching_backend(&devices, req.backend, &req.handle, &req.device_keys) else {
            return (StatusCode::NOT_FOUND, Json(BenchFormatsResponse { formats: vec![], warnings: vec!["device/backend not found".into()] })).into_response();
        };
        let keys = if req.device_keys.is_empty() { device.identity.keys.clone() } else { req.device_keys.clone() };
        (keys, Some(backend))
    } else {
        (req.device_keys.clone(), None)
    };
    let identity = identity_for_keys(&keys);

    let mut warnings: Vec<String> = Vec::new();
    let mut restored: Vec<StreamManifest> = Vec::new();

    if req.restore_existing {
        // Stop any currently running streams that share device keys (libcamera can't be opened twice).
        restored = stop_conflicting_streams(&state, &keys).await;
    }

    let modes: Vec<(styx::capture::ModeId, FourCc, String)> = if !req.modes.is_empty() {
        req.modes
            .iter()
            .map(|m| {
                let fourcc = m.id.format.code;
                (m.id.clone(), fourcc, fourcc.to_string())
            })
            .collect()
    } else {
        let Some(backend) = backend.as_ref() else {
            return (StatusCode::NOT_FOUND, Json(BenchFormatsResponse { formats: vec![], warnings: vec!["device/backend not found".into()] })).into_response();
        };
        backend
            .descriptor
            .modes
            .iter()
            .filter(|m| {
                let r = &m.format.resolution;
                r.width.get() == req.width && r.height.get() == req.height
            })
            .map(|m| (m.id.clone(), m.format.code, m.format.code.to_string()))
            .collect()
    };

    if modes.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(BenchFormatsResponse { formats: vec![], warnings: vec![format!("no modes for {}x{}", req.width, req.height)] })).into_response();
    }

    let codec_cache = codec_impl_cache();

    let mut groups: Vec<BenchFormatGroup> = Vec::new();

    for (mode_id, fourcc, format) in modes {
        let decoder_impls = codec_cache.decoders_by_input.get(&fourcc).cloned().unwrap_or_default();

        let stream_id = match start_stream_for_mode(StartStreamArgs {
            state: &state,
            identity: &identity,
            keys: &keys,
            backend: req.backend,
            handle: req.handle.clone(),
            mode_id: &mode_id,
            target_fps: req.target_fps,
            controls: req.controls.clone(),
        })
        .await
        {
            Ok(id) => id,
            Err(err) => {
                warnings.push(format!("{format}: start failed: {err}"));
                continue;
            }
        };

        // Baseline metrics (capture/host context) with no codec selection.
        let baseline_metrics = match sample_stream_metrics(&state, stream_id, req.sample_ms, false).await {
            Ok(m) => m,
            Err(err) => {
                warnings.push(format!("{format}: baseline metrics failed: {err}"));
                stop_stream_best_effort(&state, stream_id).await;
                continue;
            }
        };

        let mut decoder_stats: Vec<BenchCodecStat> = Vec::new();
        for impl_name in decoder_impls {
            if let Err(err) = state.engine.set_codecs(stream_id, Some(impl_name.clone()), None).await {
                warnings.push(format!("{format}: decoder {impl_name} set failed: {err}"));
                continue;
            }
            match sample_stream_metrics(&state, stream_id, req.sample_ms, false).await {
                Ok(metrics) => {
                    if let Some(dec) = metrics.decoder.as_ref() {
                        decoder_stats.push(BenchCodecStat { implementation: impl_name, avg_ms: dec.work_average_time_ms, avg_fps: dec.fps, errors: dec.errors });
                    }
                }
                Err(err) => warnings.push(format!("{format}: decoder {impl_name} metrics failed: {err}")),
            }
        }

        let mut encoder_stats: Vec<BenchCodecStat> = Vec::new();
        for impl_name in &codec_cache.encoders_rg24 {
            if let Err(err) = state.engine.set_codecs(stream_id, None, Some(impl_name.clone())).await {
                warnings.push(format!("{format}: encoder {impl_name} set failed: {err}"));
                continue;
            }
            match sample_stream_metrics(&state, stream_id, req.sample_ms, true).await {
                Ok(metrics) => {
                    if let Some(enc) = metrics.encoder.as_ref() {
                        encoder_stats.push(BenchCodecStat { implementation: impl_name.clone(), avg_ms: enc.work_average_time_ms, avg_fps: enc.fps, errors: enc.errors });
                    }
                }
                Err(err) => warnings.push(format!("{format}: encoder {impl_name} metrics failed: {err}")),
            }
        }

        decoder_stats.sort_by(|a, b| b.avg_fps.total_cmp(&a.avg_fps));
        encoder_stats.sort_by(|a, b| b.avg_fps.total_cmp(&a.avg_fps));

        groups.push(BenchFormatGroup { format, decoders: decoder_stats, encoders: encoder_stats, capture_avg_fps: baseline_metrics.capture.fps, host_avg_fps: baseline_metrics.host.fps });

        stop_stream_best_effort(&state, stream_id).await;
    }

    // Restore previous streams if requested.
    if req.restore_existing {
        restore_streams_best_effort(&state, restored, &mut warnings).await;
    }

    (StatusCode::OK, Json(BenchFormatsResponse { formats: groups, warnings })).into_response()
}
