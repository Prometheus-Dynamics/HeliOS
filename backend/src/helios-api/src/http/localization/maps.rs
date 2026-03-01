use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use tokio::fs;
use uuid::Uuid;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::media::{MediaMetadata, write_media_metadata};
use super::super::{json_store, storage};
use helios_engine::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapOverlay, FieldMapSource, FieldMapSummary, FieldQuaternion, aruco_bits_for_family, hydrate_map_document};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_maps)).route("/:id", get(fetch_map)).route("/upload", post(upload_limelight_fmap)).route_layer(DefaultBodyLimit::disable())
}

pub(crate) async fn load_map_document(id: &str) -> ApiResult<FieldMapDocument> {
    let mut doc = read_map(id).await?;
    hydrate_map_document(&mut doc);
    maybe_backfill_overlay_from_media(id, &mut doc).await;
    Ok(doc)
}

pub(crate) async fn bootstrap_seeded_field_maps() -> ApiResult<usize> {
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| ApiError::internal(format!("failed to open media storage: {err}")))?;
    let mut entries = fs::read_dir(&media_dir).await.map_err(|err| ApiError::internal(format!("failed to list media storage: {err}")))?;
    let mut registered = 0usize;

    while let Some(entry) = entries.next_entry().await.map_err(|err| ApiError::internal(format!("failed to scan media storage: {err}")))? {
        let is_file = entry.file_type().await.map(|ty| ty.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.to_ascii_lowercase().ends_with(".fmap") {
            continue;
        }
        let bytes = match fs::read(entry.path()).await {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => continue,
            Err(_) => continue,
        };
        if ensure_map_registered_from_media_file(&name, &bytes).await.is_ok() {
            registered += 1;
        }
    }

    Ok(registered)
}

#[utoipa::path(
    get,
    path = "/localization/maps",
    tag = "Localization",
    responses((status = 200, description = "Available field maps", body = [FieldMapSummary]))
)]
async fn list_maps() -> ApiResult<Json<Vec<FieldMapSummary>>> {
    let dir = storage::ensure_subdir_async("localization/maps").await.map_err(|err| ApiError::internal(format!("failed to open map storage: {err}")))?;
    let mut reader = fs::read_dir(&dir).await.map_err(|err| ApiError::internal(format!("failed to list map storage: {err}")))?;
    let mut out = Vec::new();
    while let Some(entry) = reader.next_entry().await.map_err(|err| ApiError::internal(format!("failed to scan map storage: {err}")))? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let bytes = match fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let doc: FieldMapDocument = match serde_json::from_slice(&bytes) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        out.push(FieldMapSummary {
            id: stem.to_string(),
            name: doc.name,
            width_m: doc.width_m,
            depth_m: doc.depth_m,
            marker_count: doc.markers.len(),
            source_kind: match doc.source {
                FieldMapSource::LimelightFmap { .. } => "limelight-fmap".to_string(),
            },
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/localization/maps/{id}",
    tag = "Localization",
    params(("id" = String, Path, description = "Field map id")),
    responses(
        (status = 200, description = "Field map document", body = FieldMapDocument),
        (status = 404, description = "Map not found", body = super::super::error::ErrorBody)
    )
)]
async fn fetch_map(Path(id): Path<String>) -> ApiResult<Json<FieldMapDocument>> {
    Ok(Json(load_map_document(&id).await?))
}

#[utoipa::path(
    post,
    path = "/localization/maps/upload",
    tag = "Localization",
    request_body(content = String, description = "Multipart form-data with exactly one .fmap (JSON) file part"),
    responses(
        (status = 201, description = "Map uploaded", body = FieldMapSummary),
        (status = 400, description = "Invalid upload", body = super::super::error::ErrorBody),
        (status = 413, description = "Upload too large", body = super::super::error::ErrorBody)
    )
)]
async fn upload_limelight_fmap(mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
    let max_bytes = max_upload_bytes();
    let mut uploaded: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart.next_field().await.map_err(|err| ApiError::bad_request(format!("failed to read upload payload: {err}")))? {
        if uploaded.is_some() {
            return Err(ApiError::bad_request("only one file may be uploaded per request"));
        }
        let filename = field.file_name().map(|name| name.to_string()).unwrap_or_else(|| "field.fmap".to_string());
        let data = field.bytes().await.map_err(|err| ApiError::bad_request(format!("failed to read upload bytes: {err}")))?;
        if data.is_empty() {
            return Err(ApiError::bad_request("empty upload"));
        }
        if data.len() as u64 > max_bytes {
            return Err(ApiError::payload_too_large(format!("upload exceeds limit of {} bytes", max_bytes)));
        }
        uploaded = Some((filename, data.to_vec()));
    }

    let Some((filename, bytes)) = uploaded else {
        return Err(ApiError::bad_request("multipart payload missing file part"));
    };

    let (raw, fmap) = parse_limelight_fmap_bytes(&bytes)?;
    let id = Uuid::new_v4().to_string();
    let name = derive_map_name(&filename);
    let overlay = extract_overlay(&raw);
    let doc = convert_limelight_fmap(&id, &name, &filename, fmap, overlay)?;

    let dir = storage::ensure_subdir_async("localization/maps").await.map_err(|err| ApiError::internal(format!("failed to open map storage: {err}")))?;
    let path = dir.join(format!("{id}.json"));
    json_store::write_json(path, &doc).await.map_err(|err| ApiError::internal(format!("failed to store map: {err}")))?;
    store_map_media_copy(&id, &name, &filename, &bytes).await?;

    let summary = FieldMapSummary { id: id.clone(), name, width_m: doc.width_m, depth_m: doc.depth_m, marker_count: doc.markers.len(), source_kind: "limelight-fmap".to_string() };
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn store_map_media_copy(id: &str, name: &str, filename: &str, bytes: &[u8]) -> ApiResult<String> {
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| ApiError::internal(format!("failed to open media storage: {err}")))?;
    let suggested = format!("field-map-{id}.fmap");
    let media_name = storage::sanitize_name(&suggested).unwrap_or_else(|| format!("field-map-{id}.fmap"));
    let path = media_dir.join(&media_name);
    fs::write(&path, bytes).await.map_err(|err| ApiError::internal(format!("failed to store map in media library: {err}")))?;

    ensure_field_map_media_metadata(&media_name, name, filename).await?;
    Ok(media_name)
}

async fn ensure_map_registered_from_media_file(media_name: &str, bytes: &[u8]) -> ApiResult<String> {
    let (raw, fmap) = parse_limelight_fmap_bytes(bytes)?;
    let map_name = derive_map_name(media_name);
    let map_id = if let Some(existing_id) = find_map_id_for_source_file(media_name).await? {
        existing_id
    } else {
        let map_id = deterministic_seed_map_id(media_name);
        let overlay = extract_overlay(&raw);
        let doc = convert_limelight_fmap(&map_id, &map_name, media_name, fmap, overlay)?;
        let dir = storage::ensure_subdir_async("localization/maps").await.map_err(|err| ApiError::internal(format!("failed to open map storage: {err}")))?;
        let path = dir.join(format!("{map_id}.json"));
        json_store::write_json(path, &doc).await.map_err(|err| ApiError::internal(format!("failed to store map: {err}")))?;
        map_id
    };
    ensure_field_map_media_metadata(media_name, &map_name, media_name).await?;
    Ok(map_id)
}

async fn ensure_field_map_media_metadata(media_name: &str, map_name: &str, source_filename: &str) -> ApiResult<()> {
    let meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| ApiError::internal(format!("failed to open media metadata: {err}")))?;
    let meta_path = meta_dir.join(format!("{media_name}.json"));
    let mut meta: MediaMetadata = match fs::read(&meta_path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => MediaMetadata::default(),
    };

    meta.kind = Some("field-map".to_string());
    if meta.description.as_deref().map(str::trim).is_none_or(|value| value.is_empty()) {
        meta.description = Some(format!("Field map: {map_name} (source: {source_filename})"));
    }

    let mut tags = BTreeSet::new();
    for tag in meta.tags {
        let trimmed = tag.trim();
        if !trimmed.is_empty() {
            tags.insert(trimmed.to_string());
        }
    }
    tags.insert("field-map".to_string());
    tags.insert("localization".to_string());
    meta.tags = tags.into_iter().collect();

    write_media_metadata(media_name, meta).await
}

async fn find_map_id_for_source_file(filename: &str) -> ApiResult<Option<String>> {
    let dir = storage::ensure_subdir_async("localization/maps").await.map_err(|err| ApiError::internal(format!("failed to open map storage: {err}")))?;
    let mut entries = fs::read_dir(&dir).await.map_err(|err| ApiError::internal(format!("failed to list map storage: {err}")))?;
    let sanitized = storage::sanitize_name(filename).unwrap_or_else(|| filename.to_string());
    while let Some(entry) = entries.next_entry().await.map_err(|err| ApiError::internal(format!("failed to scan map storage: {err}")))? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let bytes = match fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let doc: FieldMapDocument = match serde_json::from_slice(&bytes) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        if let FieldMapSource::LimelightFmap { original_file_name: Some(original), .. } = doc.source
            && original == sanitized
        {
            return Ok(Some(stem.to_string()));
        }
    }
    Ok(None)
}

fn deterministic_seed_map_id(filename: &str) -> String {
    let key = storage::sanitize_name(filename).unwrap_or_else(|| filename.to_string());
    Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("helios:seeded-field-map:{key}").as_bytes()).to_string()
}

fn parse_limelight_fmap_bytes(bytes: &[u8]) -> Result<(Value, LimelightFmap), ApiError> {
    let raw: Value = serde_json::from_slice(bytes).map_err(|err| ApiError::bad_request(format!("invalid .fmap json: {err}")))?;
    let fmap: LimelightFmap = serde_json::from_value(raw.clone()).map_err(|err| ApiError::bad_request(format!("invalid .fmap json: {err}")))?;
    Ok((raw, fmap))
}

fn derive_map_name(filename: &str) -> String {
    let sanitized = storage::sanitize_name(filename).unwrap_or_else(|| "field.fmap".to_string());
    let base = sanitized.trim_end_matches(".fmap").trim_end_matches(".json").trim();
    if base.is_empty() { "Field map".to_string() } else { base.to_string() }
}

async fn read_map(id: &str) -> ApiResult<FieldMapDocument> {
    let dir = storage::ensure_subdir_async("localization/maps").await.map_err(|err| ApiError::internal(format!("failed to open map storage: {err}")))?;
    let path = dir.join(format!("{id}.json"));
    let bytes = fs::read(&path).await.map_err(|_| ApiError::not_found("field map not found"))?;
    serde_json::from_slice(&bytes).map_err(|err| ApiError::internal(format!("invalid stored map: {err}")))
}

async fn maybe_backfill_overlay_from_media(id: &str, doc: &mut FieldMapDocument) {
    if doc.overlay.is_some() {
        return;
    }
    let Ok(media_dir) = storage::ensure_subdir_async("media").await else {
        return;
    };
    let mut candidates: Vec<String> = Vec::new();
    candidates.push(format!("field-map-{id}.fmap"));
    if let FieldMapSource::LimelightFmap { original_file_name: Some(name), .. } = &doc.source {
        candidates.push(name.clone());
    }
    candidates.dedup();

    for filename in candidates {
        let path = media_dir.join(&filename);
        let bytes = match fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let raw: Value = match serde_json::from_slice(&bytes) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        if let Some(overlay) = extract_overlay(&raw) {
            doc.overlay = Some(overlay);
            return;
        }
    }
}

fn max_upload_bytes() -> u64 {
    const DEFAULT_MB: u64 = 5;
    std::env::var("HELIOS_API_MAX_MAP_UPLOAD_MB").ok().and_then(|raw| raw.parse::<u64>().ok()).filter(|v| *v > 0).map(|mb| mb.saturating_mul(1024 * 1024)).unwrap_or(DEFAULT_MB * 1024 * 1024)
}

#[derive(Debug, Clone, Deserialize)]
struct LimelightFmap {
    fieldlength: f64,
    fieldwidth: f64,
    #[serde(default)]
    fiducials: Vec<LimelightFiducial>,
    #[serde(default)]
    r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LimelightFiducial {
    family: String,
    id: u32,
    size: f64,
    transform: Vec<f64>,
    #[serde(default)]
    unique: bool,
}

fn convert_limelight_fmap(id: &str, name: &str, filename: &str, fmap: LimelightFmap, overlay: Option<FieldMapOverlay>) -> Result<FieldMapDocument, Box<ApiError>> {
    use nalgebra::{Matrix3, Matrix4, Rotation3, UnitQuaternion, Vector3};

    // Limelight .fmap uses FRC/WPILib-style field coordinates:
    // +X forward (field length), +Y left (field width), +Z up.
    //
    // The localization viewer uses a Three.js-friendly basis:
    // +X left, +Y up, +Z forward.
    //
    // So: viewer = [wpilib_y, wpilib_z, wpilib_x]
    let width_m = fmap.fieldwidth;
    let depth_m = fmap.fieldlength;
    if !(width_m.is_finite() && width_m > 0.0 && depth_m.is_finite() && depth_m > 0.0) {
        return Err(Box::new(ApiError::bad_request("invalid field dimensions")));
    }

    // Basis change matrix from WPILib -> viewer.
    let basis = Matrix4::new(0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
    let basis_inv = basis.transpose();

    let mut markers = Vec::new();
    for fiducial in fmap.fiducials {
        if fiducial.transform.len() < 16 {
            continue;
        }
        let mut field_from_tag = Matrix4::<f64>::zeros();
        for i in 0..16 {
            field_from_tag[(i / 4, i % 4)] = fiducial.transform[i];
        }
        let viewer_from_tag = basis * field_from_tag * basis_inv;

        let position = [viewer_from_tag[(0, 3)], viewer_from_tag[(1, 3)], viewer_from_tag[(2, 3)]];

        let rot = Matrix3::from_row_slice(&[
            viewer_from_tag[(0, 0)],
            viewer_from_tag[(0, 1)],
            viewer_from_tag[(0, 2)],
            viewer_from_tag[(1, 0)],
            viewer_from_tag[(1, 1)],
            viewer_from_tag[(1, 2)],
            viewer_from_tag[(2, 0)],
            viewer_from_tag[(2, 1)],
            viewer_from_tag[(2, 2)],
        ]);
        let rot = Rotation3::from_matrix_unchecked(rot);
        let quat = UnitQuaternion::from_rotation_matrix(&rot);
        let q = quat.quaternion();

        let normal = quat.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
        let heading_deg = normal.x.atan2(normal.z).to_degrees();

        let size_m = fiducial.size / 1000.0;

        let tag_bits = aruco_bits_for_family(&normalize_family_label_for_bits(&fiducial.family), fiducial.id);

        markers.push(FieldMapMarker {
            id: fiducial.id,
            family: fiducial.family,
            size_m,
            position,
            quaternion: FieldQuaternion { x: q.i, y: q.j, z: q.k, w: q.w },
            heading_deg,
            tag_bits,
            unique: fiducial.unique,
        });
    }

    markers.sort_by_key(|marker| marker.id);

    Ok(FieldMapDocument {
        schema_version: 1,
        id: id.to_string(),
        name: name.to_string(),
        width_m,
        depth_m,
        markers,
        source: FieldMapSource::LimelightFmap { original_file_name: storage::sanitize_name(filename), map_type: fmap.r#type },
        overlay,
    })
}

fn normalize_family_label_for_bits(raw: &str) -> String {
    let value = raw.trim().to_ascii_lowercase();
    for label in ["36h11", "36h10", "25h9", "16h5"] {
        if value == label || value.contains(label) {
            return label.to_string();
        }
    }
    raw.trim().to_string()
}

fn extract_overlay(raw: &Value) -> Option<FieldMapOverlay> {
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
