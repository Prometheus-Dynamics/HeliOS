use serde_json::Value as JsonValue;
use url::Url;

use crate::http::peers::{PeerIntegrationArucoMapping, PeerIntegrationMapping, PeerIntegrationPoseMapping};

pub(crate) fn apply_custom_mapping(payload: &JsonValue, mapping: &PeerIntegrationMapping) -> Option<JsonValue> {
    if let Some(pose) = mapping.pose.as_ref()
        && let Some(value) = map_pose(payload, pose)
    {
        return Some(value);
    }
    if let Some(aruco) = mapping.aruco.as_ref()
        && let Some(value) = map_aruco(payload, aruco)
    {
        return Some(value);
    }
    None
}

pub(crate) fn resolve_custom_base_url(base: &str, endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    if trimmed.contains("://") {
        return trimmed.to_string();
    }
    if let Ok(base_url) = Url::parse(base)
        && let Ok(joined) = base_url.join(trimmed)
    {
        return joined.to_string();
    }
    trimmed.to_string()
}

pub(crate) fn build_custom_url(base: &str, output_key: &str) -> Result<String, String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return Err("custom api endpoint missing".to_string());
    }
    if trimmed.contains("{output_key}") {
        return Ok(trimmed.replace("{output_key}", output_key));
    }
    if output_key.trim().is_empty() {
        return Ok(trimmed.to_string());
    }
    if trimmed.contains('?') {
        return Ok(format!("{trimmed}&output_key={}", url::form_urlencoded::byte_serialize(output_key.as_bytes()).collect::<String>()));
    }
    if trimmed.ends_with('/') {
        return Ok(format!("{trimmed}{output_key}"));
    }
    Ok(format!("{trimmed}/{}", output_key))
}

fn map_pose(payload: &JsonValue, mapping: &PeerIntegrationPoseMapping) -> Option<JsonValue> {
    let translation = mapping.translation.as_ref().and_then(|axis| {
        let x = resolve_number(payload, axis.x.as_deref());
        let y = resolve_number(payload, axis.y.as_deref());
        let z = resolve_number(payload, axis.z.as_deref());
        if x.is_none() && y.is_none() && z.is_none() {
            None
        } else {
            Some(serde_json::json!({
                "x": x.unwrap_or(0.0),
                "y": y.unwrap_or(0.0),
                "z": z.unwrap_or(0.0),
            }))
        }
    });

    let rotation = mapping.rotation.as_ref().and_then(|axis| {
        let roll = resolve_number(payload, axis.x.as_deref());
        let pitch = resolve_number(payload, axis.y.as_deref());
        let yaw = resolve_number(payload, axis.z.as_deref());
        if roll.is_none() && pitch.is_none() && yaw.is_none() {
            None
        } else {
            Some(serde_json::json!({
                "roll": roll.unwrap_or(0.0),
                "pitch": pitch.unwrap_or(0.0),
                "yaw": yaw.unwrap_or(0.0),
            }))
        }
    });

    if translation.is_none() && rotation.is_none() {
        return None;
    }

    let mut pose = serde_json::json!({});
    if let Some(value) = translation {
        pose["translation"] = value;
    }
    if let Some(value) = rotation {
        pose["rotation"] = value;
    }

    if let Some(timestamp) = resolve_value(payload, mapping.timestamp.as_deref()) {
        pose["timestamp"] = timestamp.clone();
    }
    if let Some(latency) = resolve_value(payload, mapping.latency_ms.as_deref()) {
        pose["latencyMs"] = latency.clone();
    }

    Some(serde_json::json!({ "pose": pose }))
}

fn map_aruco(payload: &JsonValue, mapping: &PeerIntegrationArucoMapping) -> Option<JsonValue> {
    let list = if let Some(path) = mapping.list_path.as_deref() { resolve_value(payload, Some(path)) } else { Some(payload) };
    let list = list.and_then(|value| value.as_array());
    let empty: &[JsonValue] = &[];
    let list = list.map_or(empty, |value| value);

    let mut detections = Vec::new();
    for entry in list {
        let id = mapping.id.as_deref().and_then(|path| resolve_number(entry, Some(path))).map(|value| value.round() as i64);

        let translation = mapping.translation.as_ref().and_then(|axis| {
            let x = resolve_number(entry, axis.x.as_deref());
            let y = resolve_number(entry, axis.y.as_deref());
            let z = resolve_number(entry, axis.z.as_deref());
            if x.is_none() && y.is_none() && z.is_none() {
                None
            } else {
                Some(serde_json::json!({
                    "x": x.unwrap_or(0.0),
                    "y": y.unwrap_or(0.0),
                    "z": z.unwrap_or(0.0),
                }))
            }
        });

        let rotation = mapping.rotation.as_ref().and_then(|axis| {
            let roll = resolve_number(entry, axis.x.as_deref());
            let pitch = resolve_number(entry, axis.y.as_deref());
            let yaw = resolve_number(entry, axis.z.as_deref());
            if roll.is_none() && pitch.is_none() && yaw.is_none() {
                None
            } else {
                Some(serde_json::json!({
                    "roll": roll.unwrap_or(0.0),
                    "pitch": pitch.unwrap_or(0.0),
                    "yaw": yaw.unwrap_or(0.0),
                }))
            }
        });

        if translation.is_none() && rotation.is_none() && id.is_none() {
            continue;
        }

        let mut det = serde_json::json!({});
        if let Some(id) = id {
            det["id"] = serde_json::json!(id);
        }
        if let Some(value) = translation {
            det["translation"] = value;
        }
        if let Some(value) = rotation {
            det["rotation"] = value;
        }
        detections.push(det);
    }

    if detections.is_empty() {
        return None;
    }

    Some(serde_json::json!({ "detections": detections }))
}

fn resolve_number(payload: &JsonValue, path: Option<&str>) -> Option<f64> {
    let value = resolve_value(payload, path)?;
    match value {
        JsonValue::Number(number) => number.as_f64(),
        JsonValue::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
}

fn resolve_value<'a>(payload: &'a JsonValue, path: Option<&str>) -> Option<&'a JsonValue> {
    let path = path.unwrap_or("").trim();
    if path.is_empty() {
        return None;
    }
    let normalized = path.replace(['[', ']'], ".").replace("..", ".");
    let segments = normalized.split('.').filter(|segment| !segment.is_empty());
    let mut cursor = payload;
    for segment in segments {
        match cursor {
            JsonValue::Object(map) => {
                cursor = map.get(segment)?;
            }
            JsonValue::Array(list) => {
                let index = segment.parse::<usize>().ok()?;
                cursor = list.get(index)?;
            }
            _ => return None,
        }
    }
    Some(cursor)
}
