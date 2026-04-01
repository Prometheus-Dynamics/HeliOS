use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{
    EncoderSettings, EngineErrorCode, FrameRate, JsonWire, ResolvedStreamConfig, ResolutionHint, StreamManifest, StreamPipelineGridSlot, StreamPipelineLayout, StreamStatus,
    normalize_requested_stream_decoder, normalize_requested_stream_encoder,
};
use lib_ipc::client::ClientTransportError;
use std::io;
use std::time::Duration;
use styx::prelude::FourCc;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::streams_persist;

use super::CALIBRATION_MODE_PIPELINE_UUID;
use super::RAW_PIPELINE_UUID;
use super::types::{EngineErrorBody, StreamInfo};

pub(crate) fn map_client_error(err: ClientTransportError) -> Response {
    let status = StatusCode::BAD_GATEWAY;
    let body = Json(engine_error_body(Some(EngineErrorCode::Internal), err.to_string()));
    (status, body).into_response()
}

pub(crate) fn engine_error_body(code: Option<EngineErrorCode>, error: impl Into<String>) -> EngineErrorBody {
    let label = code.map(engine_error_code_label).unwrap_or("engine_error");
    EngineErrorBody { code: label.to_string(), engine_code: code, error: error.into() }
}

pub(crate) fn list_streams_timeout() -> Duration {
    const DEFAULT_RELEASE_MS: u64 = 1500;
    const DEFAULT_DEBUG_MS: u64 = 5000;
    const MIN_MS: u64 = 100;
    const MAX_MS: u64 = 20_000;

    let default_ms = if cfg!(debug_assertions) { DEFAULT_DEBUG_MS } else { DEFAULT_RELEASE_MS };
    let ms = std::env::var("HELIOS_API_LIST_STREAMS_TIMEOUT_MS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);

    Duration::from_millis(ms.clamp(MIN_MS, MAX_MS))
}

pub(crate) fn is_engine_unavailable(err: &ClientTransportError) -> bool {
    let io_err = match err {
        ClientTransportError::Io(err) => err,
        _ => return false,
    };
    // Treat only transport-level disconnects as "engine unavailable".
    //
    // Timeouts can happen when the engine is overloaded or graph-building takes longer than the
    // current RPC deadline; in those cases, silently persisting manifests would desync API state
    // from the live running stream (and can make streams appear "broken" until restart).
    matches!(io_err.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset | io::ErrorKind::NotConnected)
        || io_err.to_string().contains("engine connection unavailable")
}

fn engine_error_code_label(code: EngineErrorCode) -> &'static str {
    match code {
        EngineErrorCode::Unimplemented => "unimplemented",
        EngineErrorCode::InvalidState => "invalid_state",
        EngineErrorCode::InvalidInput => "invalid_input",
        EngineErrorCode::NotFound => "not_found",
        EngineErrorCode::Conflict => "conflict",
        EngineErrorCode::Timeout => "timeout",
        EngineErrorCode::Busy => "busy",
        EngineErrorCode::Internal => "internal",
    }
}

pub(crate) fn fourcc_to_format(fourcc: FourCc) -> &'static str {
    match &fourcc.to_u32().to_le_bytes() {
        b"MJPG" | b"JPEG" => "mjpeg",
        b"H264" => "h264",
        b"H265" | b"HEVC" => "h265",
        _ => "unknown",
    }
}

pub(crate) fn default_encoder_settings_for_codec(fourcc: FourCc, implementation: &str) -> Option<EncoderSettings> {
    if implementation.eq_ignore_ascii_case("turbojpeg") {
        return Some(EncoderSettings::Turbojpeg { quality: Some(85) });
    }
    if implementation.eq_ignore_ascii_case("mozjpeg") {
        return Some(EncoderSettings::Mozjpeg { quality: Some(85) });
    }
    if !implementation.eq_ignore_ascii_case("ffmpeg") {
        return None;
    }

    let default_framerate = Some(FrameRate { numerator: 60, denominator: 1 });
    let default_output_resolution = Some(ResolutionHint { width: 854, height: 480 });

    match &fourcc.to_u32().to_le_bytes() {
        b"MJPG" | b"JPEG" => Some(EncoderSettings::FfmpegMjpeg {
            bitrate: Some(4_000_000),
            gop: None,
            framerate: default_framerate,
            thread_count: None,
            output_resolution: default_output_resolution,
        }),
        b"H264" => Some(EncoderSettings::H264 {
            bitrate: Some(4_000_000),
            gop: None,
            framerate: default_framerate,
            thread_count: None,
            output_resolution: default_output_resolution,
        }),
        b"H265" | b"HEVC" => Some(EncoderSettings::H265 {
            bitrate: Some(4_000_000),
            gop: None,
            framerate: default_framerate,
            thread_count: None,
            output_resolution: default_output_resolution,
        }),
        _ => None,
    }
}

pub(crate) fn build_stream_info(id: Uuid, descriptor: CaptureDescriptor, resolved: ResolvedStreamConfig, status: Option<StreamStatus>) -> StreamInfo {
    let manifest = resolved.to_requested_manifest();
    StreamInfo { id, descriptor, manifest, resolved, status }
}

pub(crate) fn normalize_stream_encoder_manifest(manifest: &mut StreamManifest) {
    normalize_requested_stream_encoder(manifest);
    normalize_requested_stream_decoder(manifest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use helios_engine::capture::CaptureConfig;
    use helios_engine::identity::DeviceIdentity;
    use helios_engine::ipc::{RequestedDecoderConfig, RequestedEncoderConfig, default_stream_encoder_selector};
    use std::collections::BTreeMap;
    use styx::prelude::{ColorSpace, MediaFormat, Resolution};
    use styx::{BackendHandle, BackendKind};

    fn sample_manifest() -> StreamManifest {
        StreamManifest {
            schema_version: helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
            identity: DeviceIdentity { id: None, alias: Some("camera".to_string()), hardware_id: None },
            capture: CaptureConfig {
                device_keys: vec!["cam".to_string()],
                backend: BackendKind::Libcamera,
                handle: BackendHandle::Libcamera { id: "cam0".to_string() },
                mode: helios_engine::capture::ModeId { format: MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(1280, 800).unwrap(), ColorSpace::Srgb), interval: None },
                target_fps: None,
                interval: None,
                controls: Vec::new(),
                enable_tdn_output: false,
            },
            host_buffer: 2,
            internal: false,
            pipeline_enabled: false,
            pipelines: Vec::new(),
            active_pipeline_id: None,
            active_pipeline_output: None,
            pipeline_layout: None,
            pipeline_wires: Vec::new(),
            pipeline_host_inputs: BTreeMap::new(),
            calibration: None,
            pose: None,
            encoder: RequestedEncoderConfig::default(),
            decoder: RequestedDecoderConfig::default(),
            preview_jpeg_quality: 30,
            recording_mode: helios_engine::ipc::default_recording_mode(),
            start_on_boot: false,
        }
    }

    #[test]
    fn resolve_stream_state_applies_default_encoder_and_decoder() {
        let resolved = sample_manifest().resolve();
        assert!(resolved.encoder.enabled);
        assert_eq!(resolved.encoder.codec_id.as_deref(), default_stream_encoder_selector().as_deref());
        assert!(resolved.decoder.enabled);
        assert_eq!(
            resolved.decoder.codec_id.as_deref(),
            helios_engine::ipc::default_decoder_selector_for_capture_format(FourCc::new(*b"NV12")).as_deref()
        );
    }

    #[test]
    fn resolve_stream_state_honors_explicit_disable() {
        let mut manifest = sample_manifest();
        manifest.encoder = RequestedEncoderConfig::disabled();
        manifest.decoder = RequestedDecoderConfig::disabled();

        let resolved = manifest.resolve();
        assert!(!resolved.encoder.enabled);
        assert!(resolved.encoder.codec_id.is_none());
        assert!(!resolved.encoder.settings_present);
        assert!(!resolved.decoder.enabled);
        assert!(resolved.decoder.codec_id.is_none());
    }
}

pub(crate) fn apply_effective_pipeline_layout(manifest: &mut StreamManifest) {
    if manifest.pipeline_layout.is_some() {
        return;
    }
    if !manifest.pipeline_enabled {
        return;
    }
    if manifest.pipelines.len() <= 1 {
        return;
    }

    const MULTIPLEX_DIMENSION_MAX: u32 = 6;
    let target = manifest.pipelines.len().min((MULTIPLEX_DIMENSION_MAX * MULTIPLEX_DIMENSION_MAX) as usize);
    if target == 0 {
        return;
    }
    let size = ((target as f64).sqrt().ceil() as u32).clamp(1, MULTIPLEX_DIMENSION_MAX);
    let mut slots = Vec::with_capacity((size * size) as usize);
    for (index, pipeline) in manifest.pipelines.iter().take((size * size) as usize).enumerate() {
        let row = (index as u32) / size;
        let col = (index as u32) % size;
        slots.push(StreamPipelineGridSlot { row: row as u8, column: col as u8, pipeline_id: Some(pipeline.pipeline_id), output_key: None });
    }
    manifest.pipeline_layout = Some(StreamPipelineLayout { rows: size as u8, columns: size as u8, slots });
}

pub(crate) fn camera_id_for_manifest(manifest: &StreamManifest) -> String {
    // File-based streams commonly use a generic device key like `media-file`, which is not
    // unique per stream and will collide if used as the persisted camera_id. Persist file streams
    // under their alias (or UUID) instead. If neither is available, derive from the first media
    // path rather than a shared placeholder token.
    if manifest.capture.backend == styx::BackendKind::File {
        if let Some(alias) = manifest.identity.alias.clone() {
            return alias;
        }
        if let Some(id) = manifest.identity.id {
            return id.to_string();
        }
        if let Some(hardware_id) = manifest.identity.hardware_id.clone() {
            return hardware_id;
        }
        if let helios_engine::capture::BackendHandle::File { paths, .. } = &manifest.capture.handle
            && let Some(path) = paths.iter().find_map(|path| path.to_str())
        {
            return path.to_string();
        }
        return "media-file-unknown".to_string();
    }

    let mut keys = manifest.capture.device_keys.clone();
    if keys.is_empty() {
        return manifest.identity.hardware_id.clone().or_else(|| manifest.identity.alias.clone()).unwrap_or_else(|| "unknown".to_string());
    }
    if let Some(key) = keys.iter().find(|key| key.contains('/')) {
        return key.clone();
    }
    if let Some(key) = keys.iter().find(|key| key.contains(':')) {
        return key.clone();
    }
    keys.sort();
    keys.into_iter().next().unwrap_or_else(|| "unknown".to_string())
}

/// Normalize pipeline-related manifest fields so `pipelines` is the single source of truth.
pub(crate) fn normalize_pipeline_manifest(manifest: &mut StreamManifest) {
    // Internal/system pipeline IDs may temporarily appear in engine manifests (e.g. calibration mode),
    // but they must never leak into persisted user configs or be treated as normal pipelines by the API.
    strip_reserved_pipeline_ids(manifest);
    canonicalize_raw_output_aliases(manifest);

    if !manifest.pipeline_enabled {
        manifest.pipelines.clear();
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        manifest.pipeline_layout = None;
        manifest.pipeline_wires.clear();
        return;
    }

    // Clients can transiently send stale layout/wire/active references during rapid UI edits.
    // Treat `pipelines` (plus reserved RAW pipeline) as the source of truth and drop dangling refs.
    let pruned_layout_slots = prune_dangling_pipeline_references(manifest);

    if !manifest.pipelines.is_empty() && manifest.active_pipeline_id.is_none() {
        manifest.active_pipeline_id = Some(manifest.pipelines[0].pipeline_id);
    }

    // If a stale single-view slot was pruned, keep the stream visible by re-targeting the
    // output cell to the active (or first) pipeline instead of leaving an explicit blank layout.
    if pruned_layout_slots {
        recover_single_view_slot_from_active(manifest);
    }

    let previous_active_pipeline_id = manifest.active_pipeline_id;
    let previous_active_pipeline_output = manifest.active_pipeline_output.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string());

    // In single-view layout, keep active selection aligned with the visible slot.
    if let Some(layout) = manifest.pipeline_layout.as_ref()
        && layout.rows == 1
        && layout.columns == 1
    {
        let slot = layout.slots.iter().find(|slot| slot.row == 0 && slot.column == 0).or_else(|| layout.slots.iter().find(|slot| slot.pipeline_id.is_some()));
        if let Some(slot) = slot {
            if slot.pipeline_id.is_some() {
                manifest.active_pipeline_id = slot.pipeline_id;
            }
            let slot_output = slot.output_key.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string());
            if let Some(slot_output) = slot_output {
                manifest.active_pipeline_output = Some(slot_output);
            } else if let Some(slot_pipeline_id) = slot.pipeline_id {
                let active_changed = previous_active_pipeline_id != Some(slot_pipeline_id);
                if active_changed {
                    manifest.active_pipeline_output = manifest
                        .pipelines
                        .iter()
                        .find(|binding| binding.pipeline_id == slot_pipeline_id)
                        .and_then(|binding| binding.pipeline_output.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string()));
                } else if previous_active_pipeline_output.is_some() {
                    manifest.active_pipeline_output = previous_active_pipeline_output.clone();
                }
            }
        }
    }

    // Re-canonicalize after any active/slot-derived output updates.
    canonicalize_raw_output_aliases(manifest);

    for binding in &mut manifest.pipelines {
        binding.pipeline_graph = None;
    }

    // Missing graph payloads are handled by the graph builder with explicit errors.
}

fn normalized_output_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    })
}

pub(crate) fn preferred_pipeline_output(manifest: &StreamManifest, pipeline_id: Uuid) -> Option<String> {
    if let Some(layout) = manifest.pipeline_layout.as_ref() {
        let slot = layout
            .slots
            .iter()
            .find(|slot| slot.row == 0 && slot.column == 0 && slot.pipeline_id == Some(pipeline_id))
            .or_else(|| layout.slots.iter().find(|slot| slot.pipeline_id == Some(pipeline_id)));
        if let Some(slot_output) = slot.and_then(|slot| normalized_output_value(slot.output_key.clone())) {
            return Some(slot_output);
        }
    }

    if manifest.active_pipeline_id == Some(pipeline_id)
        && let Some(output) = normalized_output_value(manifest.active_pipeline_output.clone())
    {
        return Some(output);
    }

    manifest.pipelines.iter().find(|binding| binding.pipeline_id == pipeline_id).and_then(|binding| normalized_output_value(binding.pipeline_output.clone()))
}

pub(crate) fn promote_single_view_pipeline_selection(manifest: &mut StreamManifest, pipeline_id: Uuid, output: Option<String>) {
    if pipeline_id == RAW_PIPELINE_UUID || pipeline_id == CALIBRATION_MODE_PIPELINE_UUID {
        return;
    }

    let resolved_output = normalized_output_value(output).or_else(|| preferred_pipeline_output(manifest, pipeline_id));
    let active_is_raw = manifest.active_pipeline_id == Some(RAW_PIPELINE_UUID);

    let Some(layout) = manifest.pipeline_layout.as_mut() else {
        if active_is_raw {
            manifest.active_pipeline_id = Some(pipeline_id);
            manifest.active_pipeline_output = resolved_output;
        }
        return;
    };

    if layout.rows != 1 || layout.columns != 1 {
        if active_is_raw {
            manifest.active_pipeline_id = Some(pipeline_id);
            manifest.active_pipeline_output = resolved_output;
        }
        return;
    }

    let slot_index = layout.slots.iter().position(|slot| slot.row == 0 && slot.column == 0).or_else(|| (!layout.slots.is_empty()).then_some(0));

    let slot_pipeline_id = slot_index.and_then(|idx| layout.slots.get(idx).and_then(|slot| slot.pipeline_id));
    let should_promote = active_is_raw || slot_pipeline_id.is_none() || slot_pipeline_id == Some(RAW_PIPELINE_UUID);
    if !should_promote {
        return;
    }

    if let Some(slot) = slot_index.and_then(|idx| layout.slots.get_mut(idx)) {
        slot.pipeline_id = Some(pipeline_id);
        slot.output_key = resolved_output.clone();
    } else {
        layout.slots.push(StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(pipeline_id), output_key: resolved_output.clone() });
    }

    manifest.active_pipeline_id = Some(pipeline_id);
    manifest.active_pipeline_output = resolved_output;
}

fn prune_dangling_pipeline_references(manifest: &mut StreamManifest) -> bool {
    let mut known_pipeline_ids: std::collections::BTreeSet<Uuid> = manifest.pipelines.iter().map(|binding| binding.pipeline_id).collect();
    known_pipeline_ids.insert(RAW_PIPELINE_UUID);
    let mut pruned_layout_slots = false;

    if manifest.active_pipeline_id.is_some_and(|pipeline_id| !known_pipeline_ids.contains(&pipeline_id)) {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for slot in &mut layout.slots {
            let Some(pipeline_id) = slot.pipeline_id else { continue };
            if known_pipeline_ids.contains(&pipeline_id) {
                continue;
            }
            slot.pipeline_id = None;
            slot.output_key = None;
            pruned_layout_slots = true;
        }
    }

    manifest.pipeline_wires.retain(|wire| known_pipeline_ids.contains(&wire.from.pipeline_id) && known_pipeline_ids.contains(&wire.to.pipeline_id));
    pruned_layout_slots
}

fn recover_single_view_slot_from_active(manifest: &mut StreamManifest) {
    let Some(layout) = manifest.pipeline_layout.as_mut() else {
        return;
    };
    if layout.rows != 1 || layout.columns != 1 {
        return;
    }
    if layout.slots.iter().any(|slot| slot.pipeline_id.is_some()) {
        return;
    }

    let pipeline_id = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|binding| binding.pipeline_id));
    let Some(pipeline_id) = pipeline_id else {
        return;
    };

    if let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0) {
        slot.pipeline_id = Some(pipeline_id);
        if slot.output_key.as_deref().map(str::trim).is_none_or(|value| value.is_empty()) {
            slot.output_key = manifest.active_pipeline_output.clone();
        }
        return;
    }

    layout.slots.push(StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(pipeline_id), output_key: manifest.active_pipeline_output.clone() });
}

fn canonicalize_raw_output_aliases(manifest: &mut StreamManifest) {
    if manifest.active_pipeline_id == Some(RAW_PIPELINE_UUID) {
        manifest.active_pipeline_output = canonicalize_raw_output_value(manifest.active_pipeline_output.take());
    }

    for binding in &mut manifest.pipelines {
        if binding.pipeline_id != RAW_PIPELINE_UUID {
            continue;
        }
        binding.pipeline_output = canonicalize_raw_output_value(binding.pipeline_output.take());
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for slot in &mut layout.slots {
            if slot.pipeline_id != Some(RAW_PIPELINE_UUID) {
                continue;
            }
            slot.output_key = canonicalize_raw_output_value(slot.output_key.take());
        }
    }

    for wire in &mut manifest.pipeline_wires {
        if wire.from.pipeline_id == RAW_PIPELINE_UUID {
            wire.from.output_key = canonicalize_raw_output_value(wire.from.output_key.take());
        }
    }
}

fn canonicalize_raw_output_value(value: Option<String>) -> Option<String> {
    let value = value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    })?;
    if value.eq_ignore_ascii_case("undistorted") {
        Some("undistorted".to_string())
    } else {
        // RAW pipeline only supports `raw` and `undistorted`.
        // Coerce legacy/invalid values (e.g. `frame`, `overlay`) to `raw`.
        Some("raw".to_string())
    }
}

pub(crate) fn strip_reserved_pipeline_ids(manifest: &mut StreamManifest) {
    let mut removed_calibration_mode = false;

    // Multiplex bindings.
    let before = manifest.pipelines.len();
    manifest.pipelines.retain(|binding| binding.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);
    if manifest.pipelines.len() != before {
        removed_calibration_mode = true;
    }

    if manifest.active_pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
        removed_calibration_mode = true;
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
    }

    // Layout slots may reference pipeline IDs directly; clear reserved internal IDs.
    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for slot in &mut layout.slots {
            if slot.pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
                removed_calibration_mode = true;
                slot.pipeline_id = None;
                slot.output_key = None;
            }
        }
    }

    // Wiring graph: drop any wires involving reserved internal pipelines.
    let before = manifest.pipeline_wires.len();
    manifest.pipeline_wires.retain(|wire| wire.from.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID && wire.to.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);
    if manifest.pipeline_wires.len() != before {
        removed_calibration_mode = true;
    }

    // If calibration-mode leaked and was the only reason pipelines were "enabled", normalize back
    // to raw passthrough when there's no remaining pipeline config.
    if removed_calibration_mode {
        let has_any_config = !manifest.pipelines.is_empty()
            || manifest.pipeline_layout.as_ref().is_some_and(|layout| layout.slots.iter().any(|s| s.pipeline_id.is_some()))
            || manifest.active_pipeline_id.is_some()
            || !manifest.pipeline_wires.is_empty();
        if !has_any_config {
            manifest.pipeline_enabled = false;
        }
    }
}

pub(crate) fn apply_pipeline_host_inputs_update(manifest: &mut StreamManifest, inputs: &std::collections::BTreeMap<String, Option<serde_json::Value>>) {
    for (raw_key, value) in inputs {
        let key = raw_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        if let Some(value) = value {
            manifest.pipeline_host_inputs.insert(key, JsonWire(value.clone()));
        } else {
            manifest.pipeline_host_inputs.remove(&key);
        }
    }
}

pub(crate) async fn update_persisted_manifest_by_stream_id_checked<F>(stream_id: Uuid, updater: F) -> io::Result<Option<StreamManifest>>
where
    F: FnOnce(&mut StreamManifest),
{
    for record in streams_persist::list_persisted_records().await {
        let Some(mut manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let matches = manifest.identity.id == Some(stream_id) || record.last_stream_id == Some(stream_id) || streams_persist::derived_stream_id(&record.camera_id) == stream_id;
        if !matches {
            continue;
        }

        updater(&mut manifest);
        let manifest = streams_persist::persist_manifest_prepared_checked(&record.camera_id, Some(stream_id), manifest).await?;
        return Ok(Some(manifest));
    }

    Ok(None)
}

pub(crate) async fn persist_live_stream_manifest_update<F>(state: &AppState, stream_id: Uuid, updater: F) -> Result<StreamManifest, String>
where
    F: Fn(&mut StreamManifest),
{
    if let Some(manifest) =
        update_persisted_manifest_by_stream_id_checked(stream_id, |manifest| updater(manifest)).await.map_err(|err| format!("updated live state but failed to persist stream manifest: {err}"))?
    {
        state.services.streams.upsert_cached_stream_manifest(stream_id, manifest.clone()).await;
        return Ok(manifest);
    }

    let streams = state.engine.list_streams().await.map_err(|err| format!("updated live state but failed to load running stream for persistence: {err}"))?;
    let Some(stream) = streams.into_iter().find(|stream| stream.stream_id == stream_id) else {
        return Err("updated live state but stream was not found for persistence".to_string());
    };

    let mut manifest = stream.manifest.to_requested_manifest();
    updater(&mut manifest);
    let manifest = streams_persist::persist_manifest_prepared_checked(&camera_id_for_manifest(&manifest), Some(stream_id), manifest)
        .await
        .map_err(|err| format!("updated live state but failed to persist stream manifest: {err}"))?;
    state.services.streams.upsert_cached_stream_manifest(stream_id, manifest.clone()).await;
    Ok(manifest)
}
