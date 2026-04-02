use crate::error::AiError;
use crate::model::{ModelFormat, ModelId, ModelMetadata};
use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use serde::{Deserialize, de::DeserializeOwned};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::{env, fs};
use thiserror::Error;

pub const DEFAULT_MODEL_DIR: &str = "/var/lib/helios/ai-models";
pub const MANIFEST_NAME: &str = "manifest.json";
pub const CURRENT_AI_MODEL_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct StoredModel {
    pub id: ModelId,
    pub format: ModelFormat,
    pub metadata: ModelMetadata,
    pub artifact_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ModelStorageError {
    #[error("model {0:?} not found in manifest")]
    MissingModel(ModelId),
    #[error("manifest parse error: {0}")]
    Manifest(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ModelStorageError> for AiError {
    fn from(value: ModelStorageError) -> Self {
        match value {
            ModelStorageError::MissingModel(id) => AiError::ModelLoadFailed { reason: format!("model {} not found", id.0) },
            ModelStorageError::Manifest(err) => AiError::ModelLoadFailed { reason: format!("invalid manifest: {err}") },
            ModelStorageError::Io(err) => AiError::ModelLoadFailed { reason: format!("storage error: {err}") },
        }
    }
}

const AI_MODEL_MANIFEST_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> =
    SyncSchemaPlan { document_name: "ai model manifest", legacy_version: CURRENT_AI_MODEL_MANIFEST_SCHEMA_VERSION, current_version: CURRENT_AI_MODEL_MANIFEST_SCHEMA_VERSION, migrations: &[] };

pub fn decode_manifest<T>(raw: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| format!("failed to decode ai model manifest: {err}"))?;
    let migrated = migrate_to_current(value, &AI_MODEL_MANIFEST_SCHEMA_PLAN)?;
    serde_json::from_value(migrated).map_err(|err| format!("failed to parse ai model manifest: {err}"))
}

pub fn default_model_dir() -> PathBuf {
    if let Some(value) = env_var_any(&["PERIPHERALS_AI_MODEL_DIR", "SENSORS_AI_MODEL_DIR"]) {
        return PathBuf::from(value);
    }
    if let Some(value) = env_var_any(&["PERIPHERALS_STATE_DIR", "SENSORS_STATE_DIR"]) {
        let mut path = PathBuf::from(value);
        path.push("ai-models");
        return path;
    }
    PathBuf::from(DEFAULT_MODEL_DIR)
}

fn env_var_any(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = env::var(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

pub fn load_model(dir: impl AsRef<Path>, model_id: &ModelId) -> std::result::Result<StoredModel, ModelStorageError> {
    let dir = dir.as_ref();
    let manifest = read_manifest(dir)?;
    let entry = manifest.models.into_iter().find(|entry| &entry.id == model_id).ok_or_else(|| ModelStorageError::MissingModel(model_id.clone()))?;

    let artifact_path = dir.join(entry.artifact);
    Ok(StoredModel { id: entry.id, format: entry.format, metadata: entry.metadata, artifact_path })
}

pub fn list_models(dir: impl AsRef<Path>) -> std::result::Result<Vec<StoredModel>, ModelStorageError> {
    let dir = dir.as_ref();
    let manifest = match read_manifest(dir) {
        Ok(manifest) => manifest,
        Err(ModelStorageError::Io(err)) if err.kind() == ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(err) => return Err(err),
    };
    let models = manifest
        .models
        .into_iter()
        .filter_map(|entry| {
            let artifact_path = dir.join(&entry.artifact);
            if !artifact_path.exists() {
                return None;
            }
            Some(StoredModel { id: entry.id, format: entry.format, metadata: entry.metadata, artifact_path })
        })
        .collect();
    Ok(models)
}

fn read_manifest(dir: &Path) -> std::result::Result<RawManifest, ModelStorageError> {
    let manifest_path = dir.join(MANIFEST_NAME);
    let data = fs::read_to_string(&manifest_path)?;
    let manifest = decode_manifest::<RawManifest>(&data).map_err(ModelStorageError::Manifest)?;
    let _ = manifest.schema_version;
    Ok(manifest)
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    models: Vec<RawModelEntry>,
}

#[derive(Debug, Deserialize)]
struct RawModelEntry {
    id: ModelId,
    format: ModelFormat,
    metadata: ModelMetadata,
    artifact: String,
}

#[cfg(test)]
mod tests {
    use super::{CURRENT_AI_MODEL_MANIFEST_SCHEMA_VERSION, RawManifest, decode_manifest};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn decode_manifest_rejects_missing_schema_version() {
        let raw = json!({
            "models": [
                {
                    "id": Uuid::nil(),
                    "format": "raw",
                    "metadata": {},
                    "artifact": "model.bin"
                }
            ]
        });

        let err = decode_manifest::<RawManifest>(&serde_json::to_string(&raw).expect("encode")).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn decode_manifest_rejects_future_schema_version() {
        let raw = json!({
            "schema_version": CURRENT_AI_MODEL_MANIFEST_SCHEMA_VERSION + 1,
            "models": []
        });

        let err = decode_manifest::<RawManifest>(&serde_json::to_string(&raw).expect("encode")).expect_err("future schema should fail");
        assert!(err.contains("unsupported ai model manifest schema_version"));
    }
}
