use serde_json::{Map as JsonMap, Value as JsonValue};
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult};
use crate::http::storage;
use crate::media_read_model::MediaImuSelectionInput;
use helios_engine::localization::types::PipelineOutputSample;
use lib_sensors::dto::{ImuAxesPayload, ImuOrientationPayload, ImuStatusPayload};

use super::binding::{MEDIA_IMU_EXTERNAL_PREFIX, MEDIA_IMU_OUTPUT_KEY, MEDIA_IMU_OUTPUT_KEY_LEGACY, parse_media_imu_stream_id, resolve_binding_for_stream};

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

    imu_status_to_pose_sample(&events[idx].imu, events[idx].t_ms).ok_or_else(|| ApiError::not_found("IMU orientation unavailable"))
}

pub(super) fn imu_status_to_pose_sample(status: &ImuStatusPayload, t_ms: i64) -> Option<PipelineOutputSample> {
    let orientation = status.orientation.as_ref()?;
    let mut payload = JsonMap::new();
    payload.insert("t_ms".to_string(), JsonValue::from(t_ms));
    payload.insert("rotation".to_string(), rotation_value(orientation));

    insert_optional_axes(&mut payload, "translation", status.position_world.as_ref());
    insert_optional_axes(&mut payload, "velocity_world", status.velocity_world.as_ref());
    insert_optional_axes(&mut payload, "velocity_delta_world", status.velocity_delta_world.as_ref());
    insert_optional_axes(&mut payload, "angular_velocity_dps", status.angular_velocity_dps.as_ref());
    insert_optional_axes(&mut payload, "gyro_bias_dps", status.gyro_bias_dps.as_ref());
    insert_optional_axes(&mut payload, "corrected_world_accel_mps2", status.corrected_world_accel_mps2.as_ref());
    insert_optional_f64(&mut payload, "linear_speed_mps", status.linear_speed_mps);
    insert_optional_f64(&mut payload, "linear_speed_normalized", status.linear_speed_normalized);
    insert_optional_f64(&mut payload, "angular_speed_dps", status.angular_speed_dps);
    insert_optional_f64(&mut payload, "angular_speed_normalized", status.angular_speed_normalized);
    insert_optional_f64(&mut payload, "stillness_confidence", status.stillness_confidence);
    insert_optional_bool(&mut payload, "is_still", status.is_still);
    insert_optional_bool(&mut payload, "rotation_contaminated", status.rotation_contaminated);
    insert_optional_f64(&mut payload, "dr_confidence", status.dr_confidence);
    insert_optional_f64(&mut payload, "motion_fast_g", status.motion_fast_g);
    insert_optional_bool(&mut payload, "is_moving_fast", status.is_moving_fast);

    Some(PipelineOutputSample { data_type: None, value: JsonValue::Object(payload) })
}

fn rotation_value(orientation: &ImuOrientationPayload) -> JsonValue {
    if let Some(quat) = orientation.quaternion.as_ref() {
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
    }
}

fn axes_value(value: &ImuAxesPayload) -> JsonValue {
    serde_json::json!({
        "x": value.x,
        "y": value.y,
        "z": value.z,
    })
}

fn insert_optional_axes(payload: &mut JsonMap<String, JsonValue>, key: &str, value: Option<&ImuAxesPayload>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), axes_value(value));
    }
}

fn insert_optional_bool(payload: &mut JsonMap<String, JsonValue>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), serde_json::json!(value));
    }
}

fn insert_optional_f64(payload: &mut JsonMap<String, JsonValue>, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), serde_json::json!(value));
    }
}
