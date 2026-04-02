use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;

use super::{CURRENT_STARTUP_PRESET_SCHEMA_VERSION, StartupPresetDocument, StartupPresetMarker};
use crate::http::storage;

#[cfg(test)]
mod tests;

const DEFAULT_STARTUP_PRESET_TOML_PATH: &str = "/var/lib/helios/startup.toml";
const STARTUP_PRESET_FILE_ENV: &str = "HELIOS_STARTUP_PRESET_FILE";
const STARTUP_PRESET_MARKER_ENV: &str = "HELIOS_STARTUP_PRESET_MARKER";
const STARTUP_PRESET_MARKER_NAME: &str = ".startup-preset-applied-v1.json";

fn startup_preset_path() -> PathBuf {
    match std::env::var_os(STARTUP_PRESET_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(DEFAULT_STARTUP_PRESET_TOML_PATH),
    }
}

pub(super) fn startup_marker_path() -> PathBuf {
    std::env::var_os(STARTUP_PRESET_MARKER_ENV).map(PathBuf::from).unwrap_or_else(|| storage::data_root_path().join(STARTUP_PRESET_MARKER_NAME))
}

pub(super) async fn write_marker(path: &Path, marker: StartupPresetMarker) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec_pretty(&marker).map_err(io::Error::other)?;
    fs::write(path, bytes).await
}

fn decode_startup_preset_with_dirty(path: &Path, bytes: &[u8]) -> Result<(StartupPresetDocument, bool), String> {
    let ext = path.extension().and_then(OsStr::to_str).map(|value| value.to_ascii_lowercase());

    match ext.as_deref() {
        Some("toml") => decode_startup_preset_toml(&String::from_utf8_lossy(bytes))
            .or_else(|toml_err| decode_startup_preset_json(bytes).map_err(|json_err| format!("toml parse error: {toml_err}; json parse error: {json_err}"))),
        Some("json") => decode_startup_preset_json(bytes)
            .or_else(|json_err| decode_startup_preset_toml(&String::from_utf8_lossy(bytes)).map_err(|toml_err| format!("json parse error: {json_err}; toml parse error: {toml_err}"))),
        _ => decode_startup_preset_json(bytes).or_else(|_| decode_startup_preset_toml(&String::from_utf8_lossy(bytes))),
    }
}

async fn write_canonical_startup_preset(path: &Path, preset: &StartupPresetDocument) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut canonical = preset.clone();
    canonical.schema_version = CURRENT_STARTUP_PRESET_SCHEMA_VERSION;
    let encoded = toml::to_string_pretty(&canonical).map_err(io::Error::other)?;
    fs::write(path, encoded).await
}

const STARTUP_PRESET_JSON_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> =
    SyncSchemaPlan { document_name: "startup preset document", legacy_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, current_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, migrations: &[] };

const STARTUP_PRESET_TOML_SCHEMA_PLAN: SyncSchemaPlan<toml::Value> =
    SyncSchemaPlan { document_name: "startup preset document", legacy_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, current_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, migrations: &[] };

fn decode_startup_preset_json(bytes: &[u8]) -> Result<(StartupPresetDocument, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| err.to_string())?;
    let migrated = migrate_to_current(raw.clone(), &STARTUP_PRESET_JSON_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value(migrated.clone()).map_err(|err| err.to_string())?;
    Ok((parsed, migrated != raw))
}

fn decode_startup_preset_toml(raw: &str) -> Result<(StartupPresetDocument, bool), String> {
    let value = toml::from_str::<toml::Value>(raw).map_err(|err| err.to_string())?;
    let migrated = migrate_to_current(value.clone(), &STARTUP_PRESET_TOML_SCHEMA_PLAN)?;
    let parsed = migrated.clone().try_into().map_err(|err: toml::de::Error| err.to_string())?;
    Ok((parsed, migrated != value))
}

pub(super) async fn load_startup_preset() -> io::Result<Option<(PathBuf, StartupPresetDocument)>> {
    let configured_path = startup_preset_path();
    load_startup_preset_from_path(&configured_path).await
}

async fn load_startup_preset_from_path(configured_path: &Path) -> io::Result<Option<(PathBuf, StartupPresetDocument)>> {
    match fs::read(configured_path).await {
        Ok(bytes) => {
            let (preset, dirty) = decode_startup_preset_with_dirty(configured_path, &bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            if dirty {
                write_canonical_startup_preset(configured_path, &preset).await?;
            }
            return Ok(Some((configured_path.to_path_buf(), preset)));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    Ok(None)
}

pub(super) async fn pipeline_document_count() -> io::Result<usize> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    let mut entries = fs::read_dir(dir).await?;
    let mut count = 0usize;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await.map(|ty| ty.is_file()).unwrap_or(false) {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        count += 1;
    }
    Ok(count)
}
