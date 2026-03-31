use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

const LOCALIZATION_STREAM_SAMPLE_REFRESH_MS: u64 = 1_000;

fn localization_stream_sample_refreshes() -> &'static Mutex<BTreeMap<String, u64>> {
    static STATE: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|dur| dur.as_millis() as u64).unwrap_or(0)
}

pub(crate) async fn localization_stream_sample_needs_refresh(stream_id: Uuid, output_key: &str) -> bool {
    let key = format!("{stream_id}:{}", output_key.trim().to_ascii_lowercase());
    let now = now_ms();
    let mut guard = localization_stream_sample_refreshes().lock().await;
    guard.retain(|_, refreshed_at_ms| now.saturating_sub(*refreshed_at_ms) <= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS.saturating_mul(8));
    let needs_refresh = guard.get(&key).copied().map(|refreshed_at_ms| now.saturating_sub(refreshed_at_ms) >= LOCALIZATION_STREAM_SAMPLE_REFRESH_MS).unwrap_or(true);
    if needs_refresh {
        guard.insert(key, now);
    }
    needs_refresh
}

pub(super) fn is_media_imu_output_key(output_key: &str) -> bool {
    output_key.eq_ignore_ascii_case(super::super::media_imu::MEDIA_IMU_OUTPUT_KEY) || output_key.eq_ignore_ascii_case(super::super::media_imu::MEDIA_IMU_OUTPUT_KEY_LEGACY)
}

pub(super) fn is_localization_compatible_output(output_key: &str, data_type: Option<&JsonValue>) -> bool {
    let key = output_key.trim();
    if key.is_empty() {
        return false;
    }
    if key.eq_ignore_ascii_case("frame") {
        return false;
    }

    let looks_detection = output_key_looks_detection(key);
    let looks_pose = output_key_looks_pose(key);
    let looks_image = output_key_looks_image(key);
    let type_localization = data_type_looks_localization(data_type);
    let type_image = data_type_looks_image(data_type);

    if looks_detection || looks_pose {
        if looks_image && !type_localization {
            return false;
        }
        return true;
    }

    type_localization && !type_image
}

fn output_key_looks_detection(output_key: &str) -> bool {
    let key = output_key.trim().to_ascii_lowercase();
    key.contains("aruco") || key.contains("detect") || key.contains("detection") || key.contains("tag_poses") || key.contains("tag_pose")
}

fn output_key_looks_pose(output_key: &str) -> bool {
    let key = output_key.trim().to_ascii_lowercase();
    key.starts_with("solver:")
        || key.starts_with("tag_in_")
        || key.starts_with("camera_in_")
        || key.starts_with("robot_in_")
        || key.contains("imu_pose")
        || key.contains(" pose")
        || key.contains("_pose")
        || key.ends_with("pose")
}

fn output_key_looks_image(output_key: &str) -> bool {
    let key = output_key.trim().to_ascii_lowercase();
    key == "frame" || key == "raw" || key == "undistorted" || key.contains("image") || key.contains("frame")
}

fn data_type_text(data_type: Option<&JsonValue>) -> String {
    data_type.and_then(|value| serde_json::to_string(value).ok()).unwrap_or_default().to_ascii_lowercase()
}

fn data_type_looks_localization(data_type: Option<&JsonValue>) -> bool {
    let text = data_type_text(data_type);
    if text.is_empty() {
        return false;
    }
    text.contains("localization")
        || text.contains("detection")
        || text.contains("aruco")
        || text.contains("tag_pose")
        || text.contains("tag_poses")
        || text.contains("tag_in_")
        || text.contains("camera_in_")
        || text.contains("robot_in_")
        || text.contains("imu")
        || text.contains("pose")
}

fn data_type_looks_image(data_type: Option<&JsonValue>) -> bool {
    let text = data_type_text(data_type);
    if text.is_empty() {
        return false;
    }
    text.contains("image") || text.contains("frame") || text.contains("rgb") || text.contains("bgr") || text.contains("nv12") || text.contains("yuv") || text.contains("jpeg") || text.contains("png")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::is_localization_compatible_output;

    #[test]
    fn rejects_frame_outputs_even_when_named_loosely() {
        assert!(!is_localization_compatible_output("frame", None));
        assert!(!is_localization_compatible_output("camera_pose_image", None));
    }

    #[test]
    fn accepts_pose_outputs_with_localization_types() {
        assert!(is_localization_compatible_output("robot_in_field", Some(&json!({"kind":"localization_pose"}))));
    }
}
