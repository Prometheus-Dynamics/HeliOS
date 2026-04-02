use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde_json::Value;
use tokio::fs;

use super::super::super::error::{ApiError, ApiResult};
use super::super::super::{json_store, storage};
use super::overlay::extract_overlay;
use helios_engine::localization::maps::{FieldMapDocument, FieldMapSource, FieldMapSummary, hydrate_map_document, parse_field_map_document};
use tracing::warn;

pub(crate) async fn load_map_document(id: &str) -> ApiResult<FieldMapDocument> {
    let mut doc = read_map(id).await?;
    hydrate_map_document(&mut doc);
    maybe_backfill_overlay_from_media(id, &mut doc).await;
    Ok(doc)
}

pub(super) async fn load_existing_map_source_filenames(map_dir: &Path) -> HashSet<String> {
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
        let doc: FieldMapDocument = match parse_field_map_document(&bytes) {
            Ok((doc, _)) => doc,
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

pub(super) async fn find_map_id_for_source_file(filename: &str) -> ApiResult<Option<String>> {
    let dir = map_storage_dir().await?;
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
        let doc: FieldMapDocument = match parse_field_map_document(&bytes) {
            Ok((doc, _)) => doc,
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

pub(super) async fn list_map_summaries() -> ApiResult<Vec<FieldMapSummary>> {
    let dir = map_storage_dir().await?;
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
        let doc: FieldMapDocument = match parse_field_map_document(&bytes) {
            Ok((doc, _)) => doc,
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
    Ok(out)
}

async fn read_map(id: &str) -> ApiResult<FieldMapDocument> {
    let dir = map_storage_dir().await?;
    let path = dir.join(format!("{id}.json"));
    let bytes = fs::read(&path).await.map_err(|_| ApiError::not_found("field map not found"))?;
    let (doc, dirty) = parse_field_map_document(&bytes).map_err(|err| ApiError::internal(format!("invalid stored map: {err}")))?;
    if dirty && let Err(err) = json_store::write_json(path.clone(), &doc).await {
        warn!(path = %path.display(), error = %err, "failed to rewrite canonical field map document");
    }
    Ok(doc)
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

async fn map_storage_dir() -> ApiResult<PathBuf> {
    storage::ensure_subdir_async("localization/maps").await.map_err(|err| ApiError::internal(format!("failed to open map storage: {err}")))
}
