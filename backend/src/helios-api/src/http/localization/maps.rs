use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use tokio::fs;
use uuid::Uuid;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::media::{MediaMetadata, write_media_metadata};
use super::super::upload_integrity;
use super::super::validation::validation_error_response;
use super::super::{json_store, storage};
use super::validation::validate_limelight_fmap_payload;
use helios_engine::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapOverlay, FieldMapSource, FieldMapSummary, FieldQuaternion, aruco_bits_for_family, hydrate_map_document};
use tracing::{info, warn};

const DEFAULT_MEDIA_SEED_DIR: &str = "/usr/share/helios/media";
const MEDIA_SEED_DIR_ENV: &str = "HELIOS_API_MEDIA_SEED_DIR";

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_maps)).route("/:id", get(fetch_map)).route("/upload", post(upload_limelight_fmap)).route_layer(DefaultBodyLimit::disable())
}

pub(crate) async fn load_map_document(id: &str) -> ApiResult<FieldMapDocument> {
    let mut doc = read_map(id).await?;
    hydrate_map_document(&mut doc);
    maybe_backfill_overlay_from_media(id, &mut doc).await;
    Ok(doc)
}

pub(crate) async fn seed_bundled_field_maps() {
    let seed_dir = std::env::var(MEDIA_SEED_DIR_ENV).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).unwrap_or_else(|| DEFAULT_MEDIA_SEED_DIR.to_string());

    let mut seed_entries = match fs::read_dir(&seed_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn!(path = %seed_dir, error = %err, "failed to read field-map seed directory");
            return;
        }
    };

    let media_dir = match storage::ensure_subdir_async("media").await {
        Ok(dir) => dir,
        Err(err) => {
            warn!(error = %err, "failed to prepare media directory while seeding field maps");
            return;
        }
    };
    let map_dir = match storage::ensure_subdir_async("localization/maps").await {
        Ok(dir) => dir,
        Err(err) => {
            warn!(error = %err, "failed to prepare localization map directory while seeding field maps");
            return;
        }
    };

    let mut existing_sources = load_existing_map_source_filenames(&map_dir).await;
    let mut seeded_maps = 0usize;
    let mut copied_media = 0usize;
    let mut skipped_invalid = 0usize;

    loop {
        let entry = match seed_entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                warn!(path = %seed_dir, error = %err, "failed while scanning field-map seed directory");
                break;
            }
        };
        let path = entry.path();
        let is_fmap = path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("fmap")).unwrap_or(false);
        if !is_fmap {
            continue;
        }

        let raw_filename = entry.file_name().to_string_lossy().to_string();
        let Some(filename) = storage::sanitize_name(&raw_filename) else {
            warn!(raw_filename, "skipping seeded field map with invalid file name");
            continue;
        };

        let bytes = match fs::read(&path).await {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => {
                warn!(filename, "skipping empty seeded field map");
                skipped_invalid = skipped_invalid.saturating_add(1);
                continue;
            }
            Err(err) => {
                warn!(filename, path = %path.display(), error = %err, "failed to read seeded field map");
                skipped_invalid = skipped_invalid.saturating_add(1);
                continue;
            }
        };

        let raw: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(err) => {
                warn!(filename, error = %err, "invalid seeded .fmap json");
                skipped_invalid = skipped_invalid.saturating_add(1);
                continue;
            }
        };
        let semantic_validation = validate_limelight_fmap_payload(&raw);
        let map_warnings = match semantic_validation {
            Ok(warnings) => warnings,
            Err(err) => {
                warn!(
                    filename,
                    issue_count = err.issues.len(),
                    warning_count = err.warnings.len(),
                    issues = ?err.issues,
                    warnings = ?err.warnings,
                    "seeded field map failed semantic validation"
                );
                skipped_invalid = skipped_invalid.saturating_add(1);
                continue;
            }
        };
        if !map_warnings.is_empty() {
            info!(
                filename,
                warning_count = map_warnings.len(),
                warnings = ?map_warnings,
                "seeded field map required semantic sanitization"
            );
        }

        let fmap: LimelightFmap = match serde_json::from_value(raw.clone()) {
            Ok(value) => value,
            Err(err) => {
                warn!(filename, error = %err, "seeded .fmap shape is invalid");
                skipped_invalid = skipped_invalid.saturating_add(1);
                continue;
            }
        };

        let media_path = media_dir.join(&filename);
        let should_copy = match fs::metadata(&media_path).await {
            Ok(meta) => meta.len() == 0,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                warn!(filename, path = %media_path.display(), error = %err, "failed to inspect seeded media destination");
                true
            }
        };
        if should_copy {
            if let Err(err) = fs::write(&media_path, &bytes).await {
                warn!(filename, path = %media_path.display(), error = %err, "failed to store seeded field map in media library");
                continue;
            }
            copied_media = copied_media.saturating_add(1);
        }

        let map_name = derive_map_name(&filename);
        if let Err(err) = ensure_field_map_media_metadata(&filename, &map_name, &filename).await {
            warn!(filename, error = %err, "failed to register seeded field map media metadata");
        }

        if existing_sources.contains(filename.as_str()) {
            continue;
        }

        let map_id = seeded_map_id_for_filename(&filename);
        let overlay = extract_overlay(&raw);
        let doc = match convert_limelight_fmap(&map_id, &map_name, &filename, fmap, overlay) {
            Ok(doc) => doc,
            Err(err) => {
                warn!(filename, error = %err, "failed to convert seeded field map");
                skipped_invalid = skipped_invalid.saturating_add(1);
                continue;
            }
        };

        let map_path = map_dir.join(format!("{map_id}.json"));
        if let Err(err) = json_store::write_json(map_path, &doc).await {
            warn!(filename, error = %err, "failed to persist seeded field map document");
            continue;
        }
        existing_sources.insert(filename);
        seeded_maps = seeded_maps.saturating_add(1);
    }

    if seeded_maps > 0 || copied_media > 0 || skipped_invalid > 0 {
        info!(seed_dir = %seed_dir, seeded_maps, copied_media, skipped_invalid, "field-map startup seed pass completed");
    }

    match bootstrap_maps_from_media_library(&mut existing_sources).await {
        Ok(registered) if registered > 0 => info!(registered, "registered .fmap assets from media library"),
        Ok(_) => {}
        Err(err) => warn!(error = %err, "failed to register media-library .fmap assets"),
    }
}

async fn load_existing_map_source_filenames(map_dir: &std::path::Path) -> HashSet<String> {
    let mut sources = HashSet::new();
    let mut reader = match fs::read_dir(map_dir).await {
        Ok(reader) => reader,
        Err(err) => {
            warn!(path = %map_dir.display(), error = %err, "failed to scan map directory for seeded source matching");
            return sources;
        }
    };

    loop {
        let entry = match reader.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                warn!(path = %map_dir.display(), error = %err, "failed while reading map directory entry");
                break;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let bytes = match fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let doc: FieldMapDocument = match serde_json::from_slice(&bytes) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        if let FieldMapSource::LimelightFmap { original_file_name: Some(file), .. } = doc.source
            && let Some(name) = storage::sanitize_name(&file)
        {
            sources.insert(name);
        }
    }

    sources
}

fn seeded_map_id_for_filename(filename: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("helios:seeded-field-map:{filename}").as_bytes()).to_string()
}

async fn bootstrap_maps_from_media_library(existing_sources: &mut HashSet<String>) -> ApiResult<usize> {
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| ApiError::internal(format!("failed to open media storage: {err}")))?;
    let mut entries = fs::read_dir(&media_dir).await.map_err(|err| ApiError::internal(format!("failed to list media storage: {err}")))?;
    let mut registered = 0usize;

    while let Some(entry) = entries.next_entry().await.map_err(|err| ApiError::internal(format!("failed to scan media storage: {err}")))? {
        let is_file = entry.file_type().await.map(|ty| ty.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let raw_name = entry.file_name().to_string_lossy().to_string();
        if !raw_name.to_ascii_lowercase().ends_with(".fmap") {
            continue;
        }
        let Some(media_name) = storage::sanitize_name(&raw_name) else {
            continue;
        };
        let bytes = match fs::read(entry.path()).await {
            Ok(bytes) if !bytes.is_empty() => bytes,
            _ => continue,
        };
        let already_registered = existing_sources.contains(media_name.as_str());
        if ensure_map_registered_from_media_file(&media_name, &bytes).await.is_ok() {
            if !already_registered {
                registered = registered.saturating_add(1);
            }
            existing_sources.insert(media_name);
        }
    }

    Ok(registered)
}

async fn ensure_map_registered_from_media_file(media_name: &str, bytes: &[u8]) -> ApiResult<String> {
    let raw: Value = serde_json::from_slice(bytes).map_err(|err| ApiError::bad_request(format!("invalid .fmap json: {err}")))?;
    let semantic_validation = validate_limelight_fmap_payload(&raw).map_err(|err| {
        let detail = err.issues.first().map(|issue| issue.message.clone()).unwrap_or_else(|| "semantic map validation failed".to_string());
        ApiError::bad_request(format!("invalid .fmap payload: {detail}"))
    })?;
    if !semantic_validation.is_empty() {
        info!(
            media_name,
            warning_count = semantic_validation.len(),
            warnings = ?semantic_validation,
            "media-library field map required semantic sanitization"
        );
    }
    let fmap: LimelightFmap = serde_json::from_value(raw.clone()).map_err(|err| ApiError::bad_request(format!("invalid .fmap json: {err}")))?;

    let map_name = derive_map_name(media_name);
    let map_id = if let Some(existing_id) = find_map_id_for_source_file(media_name).await? {
        existing_id
    } else {
        let map_id = seeded_map_id_for_filename(media_name);
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
    let meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| ApiError::internal(format!("failed to open media metadata storage: {err}")))?;
    let meta_path = meta_dir.join(format!("{media_name}.json"));
    let mut metadata: MediaMetadata = match fs::read(&meta_path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => MediaMetadata::default(),
    };

    metadata.kind = Some("field-map".to_string());
    if metadata.description.as_deref().map(str::trim).is_none_or(|value| value.is_empty()) {
        metadata.description = Some(format!("Field map: {map_name} (source: {source_filename})"));
    }

    let mut tags = BTreeSet::new();
    for tag in metadata.tags {
        let trimmed = tag.trim();
        if !trimmed.is_empty() {
            tags.insert(trimmed.to_string());
        }
    }
    tags.insert("field-map".to_string());
    tags.insert("localization".to_string());
    metadata.tags = tags.into_iter().collect();

    write_media_metadata(media_name, metadata).await
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
        (status = 422, description = "Semantic validation failure", body = super::super::validation::ValidationErrorBody),
        (status = 413, description = "Upload too large", body = super::super::error::ErrorBody)
    )
)]
async fn upload_limelight_fmap(headers: HeaderMap, mut multipart: Multipart) -> axum::response::Response {
    let max_bytes = max_upload_bytes();
    let expected_upload_bytes = match upload_integrity::expected_upload_bytes(&headers) {
        Ok(value) => value,
        Err(err) => return ApiError::bad_request(err).into_response(),
    };
    let mut uploaded: Option<(String, Vec<u8>)> = None;

    loop {
        let next = match multipart.next_field().await {
            Ok(value) => value,
            Err(err) => return ApiError::bad_request(format!("failed to read upload payload: {err}")).into_response(),
        };
        let Some(field) = next else {
            break;
        };
        if uploaded.is_some() {
            return ApiError::bad_request("only one file may be uploaded per request").into_response();
        }
        let filename = field.file_name().map(|name| name.to_string()).unwrap_or_else(|| "field.fmap".to_string());
        let data = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(err) => return ApiError::bad_request(format!("failed to read upload bytes: {err}")).into_response(),
        };
        if data.is_empty() {
            return ApiError::bad_request("empty upload").into_response();
        }
        if data.len() as u64 > max_bytes {
            return ApiError::payload_too_large(format!("upload exceeds limit of {} bytes", max_bytes)).into_response();
        }
        if let Err(err) = upload_integrity::validate_expected_upload_bytes(data.len() as u64, expected_upload_bytes) {
            return ApiError::bad_request(err).into_response();
        }
        uploaded = Some((filename, data.to_vec()));
    }

    let Some((filename, bytes)) = uploaded else {
        return ApiError::bad_request("multipart payload missing file part").into_response();
    };

    let raw: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(err) => return ApiError::bad_request(format!("invalid .fmap json: {err}")).into_response(),
    };
    let semantic_validation = validate_limelight_fmap_payload(&raw);
    let map_warnings = match semantic_validation {
        Ok(warnings) => warnings,
        Err(err) => {
            warn!(
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "localization map upload rejected by semantic validator"
            );
            return validation_error_response("uploaded fmap failed semantic validation", err.issues, err.warnings);
        }
    };
    if !map_warnings.is_empty() {
        info!(
            warning_count = map_warnings.len(),
            warnings = ?map_warnings,
            "localization map upload required semantic sanitization"
        );
    }

    let fmap: LimelightFmap = match serde_json::from_value(raw.clone()) {
        Ok(value) => value,
        Err(err) => return ApiError::bad_request(format!("invalid .fmap json: {err}")).into_response(),
    };
    let id = Uuid::new_v4().to_string();
    let name = derive_map_name(&filename);
    let overlay = extract_overlay(&raw);
    let doc = match convert_limelight_fmap(&id, &name, &filename, fmap, overlay) {
        Ok(doc) => doc,
        Err(err) => return err.into_response(),
    };

    let dir = match storage::ensure_subdir_async("localization/maps").await {
        Ok(dir) => dir,
        Err(err) => return ApiError::internal(format!("failed to open map storage: {err}")).into_response(),
    };
    let path = dir.join(format!("{id}.json"));
    if let Err(err) = json_store::write_json(path, &doc).await {
        return ApiError::internal(format!("failed to store map: {err}")).into_response();
    }
    if let Err(err) = store_map_media_copy(&id, &name, &filename, &bytes).await {
        return err.into_response();
    }

    let summary = FieldMapSummary { id: id.clone(), name, width_m: doc.width_m, depth_m: doc.depth_m, marker_count: doc.markers.len(), source_kind: "limelight-fmap".to_string() };
    (StatusCode::CREATED, Json(summary)).into_response()
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
