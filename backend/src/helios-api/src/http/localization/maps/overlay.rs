use serde_json::Value;

use helios_engine::localization::maps::FieldMapOverlay;

pub(super) fn extract_overlay(raw: &Value) -> Option<FieldMapOverlay> {
    let obj = raw.as_object()?;
    let (container, image_value) = find_overlay_image(obj)?;
    let (data, mime_from_value) = extract_image_data(image_value)?;
    let mime = mime_from_value.or_else(|| read_string(container, &["mimeType", "mime", "contentType", "type", "format"]).and_then(normalize_mime));
    let data_url = normalize_data_url(&data, mime.as_deref());
    Some(FieldMapOverlay {
        data_url,
        mime_type: mime,
        opacity: read_f64(container, &["opacity", "alpha", "fieldImageOpacity", "fieldimageopacity", "overlayOpacity"]),
        width_m: read_f64(container, &["widthM", "fieldWidthM", "overlayWidthM", "fieldImageWidthM", "fieldimagewidthm"]),
        depth_m: read_f64(container, &["depthM", "fieldDepthM", "overlayDepthM", "fieldImageDepthM", "fieldimagedepthm"]),
        offset_x_m: read_f64(container, &["offsetXM", "offsetX", "fieldImageOffsetXM", "fieldimageoffsetx", "offset_x_m"]),
        offset_z_m: read_f64(container, &["offsetZM", "offsetZ", "fieldImageOffsetZM", "fieldimageoffsetz", "offset_z_m"]),
        rotation_deg: read_f64(container, &["rotationDeg", "rotation", "fieldImageRotationDeg", "fieldimagerotationdeg"]),
    })
}

fn find_overlay_image(obj: &serde_json::Map<String, Value>) -> Option<(&serde_json::Map<String, Value>, &Value)> {
    if let Some(value) = find_image_value(obj) {
        return Some((obj, value));
    }
    for key in ["overlay", "fieldOverlay", "fieldImage", "fieldimage", "field_image", "fieldMapImage", "field_map_image", "texture", "floorImage", "floor_image"] {
        let Some(value) = obj.get(key) else { continue };
        if let Some(map) = value.as_object() {
            if let Some(inner) = find_image_value(map) {
                return Some((map, inner));
            }
        } else if value.is_string() {
            return Some((obj, value));
        }
    }
    None
}

fn find_image_value(obj: &serde_json::Map<String, Value>) -> Option<&Value> {
    // Limelight .fmap exports commonly use `pngBase64`.
    for key in ["dataUrl", "dataURL", "image", "imageData", "image_data", "base64", "data", "pngBase64", "png_base64", "fieldImage", "fieldimage", "field_image"] {
        if let Some(value) = obj.get(key)
            && value.is_string()
        {
            return Some(value);
        }
    }
    None
}

fn extract_image_data(value: &Value) -> Option<(String, Option<String>)> {
    if let Some(data) = value.as_str() {
        return Some((data.trim().to_string(), None));
    }
    let obj = value.as_object()?;
    if let Some(data) = read_string(obj, &["dataUrl", "dataURL", "image", "imageData", "image_data", "base64", "pngBase64", "png_base64", "data"]) {
        let mime = read_string(obj, &["mimeType", "mime", "contentType", "type", "format"]).and_then(normalize_mime);
        return Some((data, mime));
    }
    None
}

fn normalize_data_url(raw: &str, mime: Option<&str>) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("data:") || trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("blob:") {
        return trimmed.to_string();
    }
    let mime = mime.unwrap_or("image/png");
    format!("data:{mime};base64,{trimmed}")
}

fn normalize_mime(value: String) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with("image/") {
        return Some(normalized);
    }
    let mapped = match normalized.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    Some(mapped.to_string())
}

fn read_string(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = obj.get(*key)
            && let Some(raw) = value.as_str()
        {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn read_f64(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(value) = obj.get(*key) {
            if let Some(number) = value.as_f64() {
                return Some(number);
            }
            if let Some(raw) = value.as_str()
                && let Ok(parsed) = raw.trim().parse::<f64>()
            {
                return Some(parsed);
            }
        }
    }
    None
}
