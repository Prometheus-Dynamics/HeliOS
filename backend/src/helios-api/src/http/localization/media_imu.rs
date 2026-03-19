use serde_json::{Map as JsonMap, Value as JsonValue};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult};
use crate::http::media::MediaMetadata;
use crate::http::storage;
use crate::media_read_model::MediaImuSelectionInput;
use helios_engine::ipc::StreamSummary;
use helios_engine::localization::types::{LocalizationPipelineSource, PipelineOutputSample};

const MEDIA_IMU_EXTERNAL_PREFIX: &str = "media-imu-";
pub(crate) const MEDIA_IMU_OUTPUT_KEY: &str = "imu_pose";
pub(crate) const MEDIA_IMU_OUTPUT_KEY_LEGACY: &str = "pose";

#[derive(Debug, Clone)]
struct MediaImuBinding {
    stream_id: Uuid,
    stream_label: String,
    loop_forever: bool,
    playback_fps: Option<f64>,
    media_name: String,
    sidecar_path: PathBuf,
    frame_ts_path: Option<PathBuf>,
    playback_duration_ms: Option<i64>,
}

impl MediaImuBinding {
    fn signature(&self) -> String {
        let frame_ts = self.frame_ts_path.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        format!("{}|{}|{}|{}|{:.6}|{}", self.sidecar_path.display(), self.media_name, self.loop_forever, self.playback_duration_ms.unwrap_or(0), self.playback_fps.unwrap_or(0.0), frame_ts)
    }
}

pub(crate) fn is_media_imu_source_id(source_id: &str) -> bool {
    parse_media_imu_stream_id(source_id).is_some()
}

pub(crate) async fn source_for_stream(state: &AppState, stream: &StreamSummary, media_meta_dir: &Path) -> Option<LocalizationPipelineSource> {
    let binding = resolve_binding_for_stream(state, stream, media_meta_dir).await?;
    let stream_id = binding.stream_id.to_string();
    let camera_uid = stream.manifest.identity.hardware_id.clone().or(stream.manifest.identity.alias.clone()).unwrap_or_else(|| stream_id.clone());
    let camera_path = match &stream.manifest.capture.handle {
        styx::BackendHandle::V4l2 { path } => path.clone(),
        styx::BackendHandle::Libcamera { id } => id.clone(),
        styx::BackendHandle::Netcam { url, .. } => url.clone(),
        styx::BackendHandle::File { paths, .. } => paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(","),
        other => format!("{other:?}"),
    };
    Some(LocalizationPipelineSource {
        id: format!("{stream_id}:{MEDIA_IMU_OUTPUT_KEY}"),
        stream_id: stream_id.clone(),
        stream_label: binding.stream_label,
        camera_uid,
        camera_path,
        pipeline_id: "media-imu".to_string(),
        pipeline_label: "Media IMU".to_string(),
        output_key: MEDIA_IMU_OUTPUT_KEY.to_string(),
        data_type: None,
    })
}

pub(crate) async fn fetch_media_imu_sample_for_stream(state: &AppState, stream_id: Uuid, output_key: &str) -> ApiResult<PipelineOutputSample> {
    let source_id = format!("{MEDIA_IMU_EXTERNAL_PREFIX}{stream_id}");
    fetch_media_imu_sample(state, &source_id, output_key).await
}

pub(crate) async fn fetch_media_imu_sample(state: &AppState, source_id: &str, output_key: &str) -> ApiResult<PipelineOutputSample> {
    let output = output_key.trim();
    if !output.eq_ignore_ascii_case(MEDIA_IMU_OUTPUT_KEY) && !output.eq_ignore_ascii_case(MEDIA_IMU_OUTPUT_KEY_LEGACY) {
        return Err(ApiError::bad_request("unsupported output key"));
    }

    let Some(stream_id) = parse_media_imu_stream_id(source_id) else {
        return Err(ApiError::not_found("external source not found"));
    };

    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(err.to_string()))?;
    let Some(stream) = streams.into_iter().find(|entry| entry.stream_id == stream_id) else {
        return Err(ApiError::not_found("stream not found"));
    };

    let media_meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| ApiError::bad_gateway(format!("failed to access media metadata: {err}")))?;
    let Some(binding) = resolve_binding_for_stream(state, &stream, &media_meta_dir).await else {
        return Err(ApiError::not_found("IMU sidecar not found"));
    };

    let events = state.services.media.load_media_imu_events_cached(binding.sidecar_path.clone()).await.map_err(|err| ApiError::bad_gateway(format!("failed to read IMU sidecar: {err}")))?;
    let frame_timeline = match binding.frame_ts_path.clone() {
        Some(path) => match state.services.media.load_media_frame_timeline_cached(path).await {
            Ok(values) => Some(values),
            Err(err) => {
                tracing::warn!(stream_id = %binding.stream_id, error = %err, "failed to read frame timestamp sidecar");
                None
            }
        },
        None => None,
    };
    let stream_started_at_ms = stream.status.started_at_ms.map(|value| value as i64).filter(|value| *value > 0);
    let replay_frame_clock = state.services.media.read_stream_frame_clock(binding.stream_id).await;
    let Some(idx) = state
        .services
        .media
        .select_media_imu_event_index(
            MediaImuSelectionInput {
                stream_id: binding.stream_id,
                signature: &binding.signature(),
                loop_forever: binding.loop_forever,
                playback_fps: binding.playback_fps,
                replay_started_at_ms: stream_started_at_ms,
                playback_duration_ms: binding.playback_duration_ms,
                replay_frame_clock,
                frame_timeline: frame_timeline.as_deref().map(Vec::as_slice),
            },
            events.as_ref(),
        )
        .await
    else {
        return Err(ApiError::not_found("no IMU samples available"));
    };
    let Some(sample) = imu_status_to_pose_sample(&events[idx].imu, events[idx].t_ms) else {
        return Err(ApiError::not_found("IMU orientation unavailable"));
    };
    Ok(sample)
}

fn parse_media_imu_stream_id(source_id: &str) -> Option<Uuid> {
    source_id.strip_prefix(MEDIA_IMU_EXTERNAL_PREFIX).and_then(|value| Uuid::parse_str(value).ok())
}

async fn resolve_binding_for_stream(state: &AppState, stream: &StreamSummary, media_meta_dir: &Path) -> Option<MediaImuBinding> {
    let (paths, loop_forever, playback_fps) = match &stream.manifest.capture.handle {
        styx::BackendHandle::File { paths, loop_forever, fps } => (paths, *loop_forever, Some(*fps as f64)),
        _ => return None,
    };

    let stream_label = stream.manifest.identity.alias.clone().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| stream.stream_id.to_string());

    for path in paths {
        for media_name in media_name_candidates(path) {
            let Some(metadata) = load_media_metadata(media_meta_dir, &media_name).await else {
                continue;
            };
            let Some(sidecar_name) = metadata.imu_data_file_name.as_deref().and_then(storage::sanitize_name) else {
                continue;
            };
            let sidecar_path = media_meta_dir.join(&sidecar_name);
            if !tokio::fs::metadata(&sidecar_path).await.ok().is_some_and(|meta| meta.is_file()) {
                continue;
            }
            let frame_ts_path = match metadata.frame_timestamps_file_name.as_deref().and_then(storage::sanitize_name) {
                Some(name) => resolve_frame_ts_path(media_meta_dir, &name).await,
                None => None,
            };
            let playback_duration_ms = state.services.media.playback_duration_ms_cached(path.clone()).await;
            return Some(MediaImuBinding {
                stream_id: stream.stream_id,
                stream_label: stream_label.clone(),
                loop_forever,
                playback_fps,
                media_name,
                sidecar_path,
                frame_ts_path,
                playback_duration_ms,
            });
        }
    }

    None
}

async fn resolve_frame_ts_path(media_meta_dir: &Path, sidecar_name: &str) -> Option<PathBuf> {
    let meta_candidate = media_meta_dir.join(sidecar_name);
    if tokio::fs::metadata(&meta_candidate).await.ok().is_some_and(|meta| meta.is_file()) {
        return Some(meta_candidate);
    }
    let media_dir = storage::ensure_subdir_async("media").await.ok()?;
    let media_candidate = media_dir.join(sidecar_name);
    tokio::fs::metadata(&media_candidate).await.ok().and_then(|meta| meta.is_file().then_some(media_candidate))
}

fn media_name_candidates(path: &Path) -> Vec<String> {
    let Some(raw_name) = path.file_name().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let raw_name = raw_name.trim();
    if raw_name.is_empty() {
        return Vec::new();
    }

    let mut names = Vec::new();
    if let Some(name) = storage::sanitize_name(raw_name) {
        names.push(name);
    }
    if let Some(stripped) = raw_name.strip_suffix(".replay.mp4")
        && let Some(name) = storage::sanitize_name(stripped)
        && !names.iter().any(|value| value == &name)
    {
        names.push(name);
    }
    if let Some(no_mp4) = raw_name.strip_suffix(".mp4")
        && let Some((prefix, _)) = no_mp4.rsplit_once(".replay.")
        && let Some(name) = storage::sanitize_name(prefix)
        && !names.iter().any(|value| value == &name)
    {
        names.push(name);
    }
    names
}

async fn load_media_metadata(media_meta_dir: &Path, media_name: &str) -> Option<MediaMetadata> {
    let bytes = tokio::fs::read(media_meta_dir.join(format!("{media_name}.json"))).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn imu_status_to_pose_sample(status: &lib_sensors::dto::ImuStatusPayload, t_ms: i64) -> Option<PipelineOutputSample> {
    let orientation = status.orientation.as_ref()?;

    let rotation = if let Some(quat) = orientation.quaternion.as_ref() {
        serde_json::json!({
            "roll": orientation.roll,
            "pitch": orientation.pitch,
            "yaw": orientation.yaw,
            "quaternion": {
                "x": quat.x,
                "y": quat.y,
                "z": quat.z,
                "w": quat.w,
            }
        })
    } else {
        serde_json::json!({
            "roll": orientation.roll,
            "pitch": orientation.pitch,
            "yaw": orientation.yaw,
        })
    };
    let translation = status.position_world.as_ref().map(|position| {
        serde_json::json!({
            "x": position.x,
            "y": position.y,
            "z": position.z,
        })
    });
    let velocity = status.velocity_world.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let velocity_delta = status.velocity_delta_world.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let angular_velocity = status.angular_velocity_dps.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let gyro_bias = status.gyro_bias_dps.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let corrected_world_accel = status.corrected_world_accel_mps2.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });

    let mut payload = JsonMap::new();
    payload.insert("t_ms".to_string(), JsonValue::from(t_ms));
    payload.insert("rotation".to_string(), rotation);
    if let Some(translation) = translation {
        payload.insert("translation".to_string(), translation);
    }
    if let Some(velocity) = velocity {
        payload.insert("velocity_world".to_string(), velocity);
    }
    if let Some(velocity_delta) = velocity_delta {
        payload.insert("velocity_delta_world".to_string(), velocity_delta);
    }
    if let Some(angular_velocity) = angular_velocity {
        payload.insert("angular_velocity_dps".to_string(), angular_velocity);
    }
    if let Some(gyro_bias) = gyro_bias {
        payload.insert("gyro_bias_dps".to_string(), gyro_bias);
    }
    if let Some(corrected_world_accel) = corrected_world_accel {
        payload.insert("corrected_world_accel_mps2".to_string(), corrected_world_accel);
    }
    if let Some(value) = status.linear_speed_mps {
        payload.insert("linear_speed_mps".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.linear_speed_normalized {
        payload.insert("linear_speed_normalized".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.angular_speed_dps {
        payload.insert("angular_speed_dps".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.angular_speed_normalized {
        payload.insert("angular_speed_normalized".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.stillness_confidence {
        payload.insert("stillness_confidence".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.is_still {
        payload.insert("is_still".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.rotation_contaminated {
        payload.insert("rotation_contaminated".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.dr_confidence {
        payload.insert("dr_confidence".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.motion_fast_g {
        payload.insert("motion_fast_g".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.is_moving_fast {
        payload.insert("is_moving_fast".to_string(), serde_json::json!(value));
    }

    Some(PipelineOutputSample { data_type: None, value: JsonValue::Object(payload) })
}
