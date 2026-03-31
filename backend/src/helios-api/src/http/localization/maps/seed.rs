use std::collections::HashSet;

use serde_json::Value;
use tokio::fs;

use super::super::super::error::{ApiError, ApiResult};
use super::super::super::{json_store, storage};
use super::super::validation::validate_limelight_fmap_payload;
use super::{
    convert::{LimelightFmap, convert_limelight_fmap},
    overlay::extract_overlay,
    storage::{find_map_id_for_source_file, load_existing_map_source_filenames},
    support::{derive_map_name, ensure_field_map_media_metadata, field_map_seed_dir, seeded_map_id_for_filename},
};
use tracing::{info, warn};

pub(crate) async fn seed_bundled_field_maps() {
    let seed_dir = field_map_seed_dir();

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
