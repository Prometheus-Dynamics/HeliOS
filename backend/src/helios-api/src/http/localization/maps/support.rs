use std::collections::BTreeSet;

use lib_runtime_policy::HELIOS_API_LOCALIZATION_POLICY;
use tokio::fs;
use uuid::Uuid;

use super::super::super::error::{ApiError, ApiResult};
use super::super::super::media::{MediaMetadata, load_media_metadata, write_media_metadata};
use super::super::super::storage;

pub(super) fn field_map_seed_dir() -> String {
    HELIOS_API_LOCALIZATION_POLICY.resolve().media_seed_dir.display().to_string()
}

pub(super) fn seeded_map_id_for_filename(filename: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("helios:seeded-field-map:{filename}").as_bytes()).to_string()
}

pub(super) async fn ensure_field_map_media_metadata(media_name: &str, map_name: &str, source_filename: &str) -> ApiResult<()> {
    let meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| ApiError::internal(format!("failed to open media metadata storage: {err}")))?;
    let mut metadata: MediaMetadata = load_media_metadata(&meta_dir, media_name).await.unwrap_or_default();

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

pub(super) async fn store_map_media_copy(id: &str, name: &str, filename: &str, bytes: &[u8]) -> ApiResult<String> {
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| ApiError::internal(format!("failed to open media storage: {err}")))?;
    let suggested = format!("field-map-{id}.fmap");
    let media_name = storage::sanitize_name(&suggested).unwrap_or_else(|| format!("field-map-{id}.fmap"));
    let path = media_dir.join(&media_name);
    fs::write(&path, bytes).await.map_err(|err| ApiError::internal(format!("failed to store map in media library: {err}")))?;

    ensure_field_map_media_metadata(&media_name, name, filename).await?;
    Ok(media_name)
}

pub(super) fn derive_map_name(filename: &str) -> String {
    let sanitized = storage::sanitize_name(filename).unwrap_or_else(|| "field.fmap".to_string());
    let base = sanitized.trim_end_matches(".fmap").trim_end_matches(".json").trim();
    if base.is_empty() { "Field map".to_string() } else { base.to_string() }
}

pub(super) fn max_upload_bytes() -> u64 {
    HELIOS_API_LOCALIZATION_POLICY.resolve().max_map_upload_bytes
}

#[cfg(test)]
mod tests {
    use super::{derive_map_name, seeded_map_id_for_filename};

    #[test]
    fn derive_map_name_trims_known_suffixes() {
        assert_eq!(derive_map_name("2025-field.fmap"), "2025-field");
        assert_eq!(derive_map_name("2025-field.json"), "2025-field");
    }

    #[test]
    fn seeded_map_id_is_stable_for_filename() {
        let first = seeded_map_id_for_filename("frc-2025.fmap");
        let second = seeded_map_id_for_filename("frc-2025.fmap");
        assert_eq!(first, second);
    }
}
