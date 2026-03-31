use crate::http::streams::validation::{StreamValidationResult, normalize_stream_manifest, validate_stream_manifest};
use crate::http::{json_store, storage};
use crate::ipc::IpcHandles;
use chrono::Utc;
use helios_engine::ipc::{ResolvedStreamConfig, RigPose, StreamManifest};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{
    collections::HashMap,
    io,
};
use tokio::fs;
use tokio::sync::OnceCell;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine_guard;
pub type AppState = std::sync::Arc<IpcHandles>;

const RAW_PIPELINE_UUID: Uuid = Uuid::from_u128(0x000000000000000000000000000000aa);
const LEGACY_RAW_PIPELINE_UUID: Uuid = Uuid::from_u128(0x000000000000000000000000000000ab);
const CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION: u32 = 1;
const LEGACY_PERSISTED_STREAM_RECORD_SCHEMA_VERSION: u32 = 0;

fn migrate_legacy_raw_pipeline_uuid(manifest: &mut StreamManifest) {
    for binding in &mut manifest.pipelines {
        if binding.pipeline_id == LEGACY_RAW_PIPELINE_UUID {
            binding.pipeline_id = RAW_PIPELINE_UUID;
        }
    }
    if manifest.active_pipeline_id == Some(LEGACY_RAW_PIPELINE_UUID) {
        manifest.active_pipeline_id = Some(RAW_PIPELINE_UUID);
    }
    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for slot in &mut layout.slots {
            if slot.pipeline_id == Some(LEGACY_RAW_PIPELINE_UUID) {
                slot.pipeline_id = Some(RAW_PIPELINE_UUID);
            }
        }
    }
    for wire in &mut manifest.pipeline_wires {
        if wire.from.pipeline_id == LEGACY_RAW_PIPELINE_UUID {
            wire.from.pipeline_id = RAW_PIPELINE_UUID;
        }
        if wire.to.pipeline_id == LEGACY_RAW_PIPELINE_UUID {
            wire.to.pipeline_id = RAW_PIPELINE_UUID;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedStreamRecord {
    #[serde(default)]
    pub schema_version: u32,
    pub camera_id: String,
    #[serde(default)]
    pub last_stream_id: Option<Uuid>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub manifest: Option<StreamManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_config: Option<ResolvedStreamConfig>,
}

impl Default for PersistedStreamRecord {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION,
            camera_id: String::new(),
            last_stream_id: None,
            updated_at: None,
            manifest: None,
            resolved_config: None,
        }
    }
}

type PersistedStreamRecordMigration = fn(PersistedStreamRecord) -> Result<PersistedStreamRecord, String>;

fn migrate_persisted_stream_record_v0_to_v1(mut record: PersistedStreamRecord) -> Result<PersistedStreamRecord, String> {
    record.schema_version = CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
    Ok(record)
}

const PERSISTED_STREAM_RECORD_MIGRATIONS: &[(u32, PersistedStreamRecordMigration)] =
    &[(LEGACY_PERSISTED_STREAM_RECORD_SCHEMA_VERSION, migrate_persisted_stream_record_v0_to_v1)];

fn migrate_persisted_stream_record(mut record: PersistedStreamRecord) -> Result<PersistedStreamRecord, String> {
    let mut version = record.schema_version;
    if version > CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION {
        return Err(format!(
            "unsupported persisted stream record schema_version {}; current version is {}",
            version, CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION
        ));
    }

    while version < CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION {
        let Some((_, migration)) = PERSISTED_STREAM_RECORD_MIGRATIONS.iter().find(|(from, _)| *from == version) else {
            return Err(format!(
                "no persisted stream record migration registered from schema_version {} to {}",
                version,
                version + 1
            ));
        };
        record = migration(record)?;
        version = record.schema_version.max(version + 1);
    }

    record.schema_version = CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
    Ok(record)
}

fn parse_persisted_stream_record(bytes: &[u8]) -> Result<PersistedStreamRecord, String> {
    let record = serde_json::from_slice::<PersistedStreamRecord>(bytes).map_err(|err| format!("failed to decode persisted stream record: {err}"))?;
    migrate_persisted_stream_record(record)
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn record_key(camera_id: &str) -> String {
    blake3::hash(camera_id.as_bytes()).to_hex().to_string()
}

async fn records_dir() -> std::io::Result<PathBuf> {
    storage::ensure_subdir_async("streams").await
}

async fn record_path(camera_id: &str) -> std::io::Result<PathBuf> {
    let dir = records_dir().await?;
    Ok(dir.join(format!("{}.json", record_key(camera_id))))
}

async fn hydrate_manifest(mut manifest: StreamManifest) -> StreamManifest {
    migrate_legacy_raw_pipeline_uuid(&mut manifest);
    // Migration/normalization: libcamera FPS should be expressed via `capture.target_fps` rather
    // than a persisted `capture.interval` or a raw FrameDurationLimits control assignment (id 30).
    if manifest.capture.backend == styx::BackendKind::Libcamera {
        // If `interval` was persisted from older clients, convert it to `target_fps`.
        if manifest.capture.target_fps.is_none() {
            if let Some(interval) = manifest.capture.interval.take() {
                let num = interval.numerator.get();
                let den = interval.denominator.get();
                if num > 0 {
                    // fps = den / num (seconds per frame -> frames per second)
                    manifest.capture.target_fps = Some(den / num);
                }
            }
        } else {
            // Avoid persisting interval for libcamera; we drive FPS via controls internally.
            manifest.capture.interval = None;
        }

        // If a target FPS exists, drop raw FrameDurationLimits assignments so we don't "lock" the
        // stream to an old duration while the FPS knob changes.
        if manifest.capture.target_fps.is_some() {
            manifest.capture.controls.retain(|c| c.id != 30);
        }
    }

    // NOTE: Persisted manifests must never embed inline pipeline graphs.
    // Pipelines are referenced by ID and loaded from `/pipelines/graphs` by the engine when needed.
    for binding in &mut manifest.pipelines {
        binding.pipeline_graph = None;
    }

    if manifest.capture.backend == styx::BackendKind::File && manifest.capture.mode.format.code == styx::prelude::FourCc::new(*b"RGBA") {
        manifest.capture.mode.format.code = styx::prelude::FourCc::new(*b"RG24");
    }
    normalize_stream_manifest(manifest).manifest
}

async fn hydrate_record(mut record: PersistedStreamRecord) -> PersistedStreamRecord {
    record.schema_version = CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
    if let Some(resolved) = record.resolved_config.clone() {
        record.manifest = Some(resolved.to_requested_manifest());
        return record;
    }

    if let Some(manifest) = record.manifest.take() {
        let manifest = hydrate_manifest(manifest).await;
        match validate_stream_manifest(manifest.clone()).await {
            Ok(prepared) => {
                record.manifest = Some(prepared.manifest);
                record.resolved_config = Some(prepared.resolved);
            }
            Err(err) => {
                warn!(
                    camera_id = record.camera_id,
                    issue_count = err.issues.len(),
                    warning_count = err.warnings.len(),
                    issues = ?err.issues,
                    warnings = ?err.warnings,
                    "persisted stream record remains manifest-only because semantic validation failed"
                );
                record.manifest = Some(manifest);
                record.resolved_config = None;
            }
        }
    }

    record
}

async fn update_record<F, Fut>(camera_id: &str, updater: F) -> std::io::Result<PersistedStreamRecord>
where
    F: FnOnce(PersistedStreamRecord) -> Fut,
    Fut: std::future::Future<Output = PersistedStreamRecord>,
{
    let path = record_path(camera_id).await?;
    json_store::update_json(path, move |mut current: PersistedStreamRecord| async move {
        current = match migrate_persisted_stream_record(current) {
            Ok(record) => record,
            Err(err) => {
                warn!(camera_id, error = %err, "failed to migrate persisted stream record during update; rewriting with current schema");
                PersistedStreamRecord::default()
            }
        };
        if current.camera_id.is_empty() {
            current.camera_id = camera_id.to_string();
        }
        let mut next = updater(current).await;
        next.schema_version = CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
        next
    })
    .await
}

async fn persist_resolved_config_impl(camera_id: &str, stream_id: Option<Uuid>, mut resolved: ResolvedStreamConfig) -> std::io::Result<()> {
    if let Some(id) = stream_id {
        resolved.identity.id = Some(id);
    }
    if resolved.capture.backend == styx::BackendKind::File && !resolved.start_on_boot {
        resolved.start_on_boot = true;
    }
    update_record(camera_id, move |mut record| async move {
        if let Some(id) = stream_id {
            record.last_stream_id = Some(id);
        }
        if resolved.pose.is_none()
            && let Some(existing_pose) = record.resolved_config.as_ref().and_then(|config| config.pose.clone()).or_else(|| record.manifest.as_ref().and_then(|manifest| manifest.pose.clone()))
        {
            resolved.pose = Some(existing_pose);
        }
        record.manifest = None;
        record.resolved_config = Some(resolved);
        record.updated_at = Some(now_rfc3339());
        record
    })
    .await
    .map(|_| ())
}

async fn prepare_manifest_for_persistence(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamValidationResult> {
    let mut hydrated = hydrate_manifest(manifest).await;
    if let Some(id) = stream_id {
        hydrated.identity.id = Some(id);
    }
    if hydrated.capture.backend == styx::BackendKind::File && !hydrated.start_on_boot {
        hydrated.start_on_boot = true;
    }

    if hydrated.capture.backend == styx::BackendKind::Libcamera && hydrated.capture.target_fps.is_some() {
        hydrated.capture.interval = None;
        hydrated.capture.controls.retain(|c| c.id != 30);
    }

    validate_stream_manifest(hydrated).await.map_err(|err| {
        let detail = err
            .issues
            .first()
            .map(|issue| format!("{} ({})", issue.message, issue.code))
            .unwrap_or_else(|| "unknown semantic validation failure".to_string());
        io::Error::new(io::ErrorKind::InvalidInput, format!("persisted stream manifest for `{camera_id}` failed semantic validation: {detail}"))
    })
}

async fn persist_manifest_impl(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamManifest> {
    let prepared = prepare_manifest_for_persistence(camera_id, stream_id, manifest).await?;
    persist_resolved_config_impl(camera_id, stream_id, prepared.resolved).await?;
    Ok(prepared.manifest)
}

pub(crate) async fn persist_manifest_prepared_checked(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamManifest> {
    persist_manifest_impl(camera_id, stream_id, manifest).await
}

pub async fn persist_manifest_checked(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<()> {
    persist_manifest_impl(camera_id, stream_id, manifest).await.map(|_| ())
}

pub async fn persist_resolved_config_checked(camera_id: &str, stream_id: Option<Uuid>, resolved: ResolvedStreamConfig) -> std::io::Result<()> {
    persist_resolved_config_impl(camera_id, stream_id, resolved).await
}

pub async fn persist_manifest(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) {
    if let Err(err) = persist_manifest_impl(camera_id, stream_id, manifest).await {
        warn!(camera_id, error = %err, "failed to persist stream manifest");
    }
}

pub async fn persist_resolved_config(camera_id: &str, stream_id: Option<Uuid>, resolved: ResolvedStreamConfig) {
    if let Err(err) = persist_resolved_config_impl(camera_id, stream_id, resolved).await {
        warn!(camera_id, error = %err, "failed to persist resolved stream config");
    }
}

pub async fn persist_manifest_quick_checked(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<()> {
    persist_manifest_impl(camera_id, stream_id, manifest).await.map(|_| ())
}

pub async fn persist_manifest_quick(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) {
    if let Err(err) = persist_manifest_quick_checked(camera_id, stream_id, manifest).await {
        warn!(camera_id, error = %err, "failed to persist stream manifest");
    }
}

pub async fn load_manifest(camera_id: &str) -> Option<StreamManifest> {
    load_resolved_config(camera_id).await.map(|config| config.to_requested_manifest())
}

pub async fn load_resolved_config(camera_id: &str) -> Option<ResolvedStreamConfig> {
    let path = record_path(camera_id).await.ok()?;
    let bytes = fs::read(&path).await.ok()?;
    let record = match parse_persisted_stream_record(&bytes) {
        Ok(record) => record,
        Err(err) => {
            warn!(camera_id, error = %err, "failed to load persisted stream record");
            return None;
        }
    };
    let record = hydrate_record(record).await;
    record.resolved_config
}

async fn list_records() -> Vec<PersistedStreamRecord> {
    let dir = match records_dir().await {
        Ok(dir) => dir,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut entries = match fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let is_file = entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let Ok(bytes) = fs::read(&path).await else {
            continue;
        };
        let Ok(record) = parse_persisted_stream_record(&bytes) else {
            continue;
        };
        out.push(hydrate_record(record).await);
    }

    out
}

async fn migrate_file_camera_ids_once() {
    // Historical bug: File streams were sometimes persisted under a generic device key like
    // `media-file`, which collides across streams and creates duplicate persisted records for the
    // same stream UUID/alias. Canonicalize file streams to be stored under their alias.
    let records = list_records().await;
    for record in records {
        let Some(manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        if manifest.capture.backend != styx::BackendKind::File {
            continue;
        }
        let Some(alias) = manifest.identity.alias.clone() else {
            continue;
        };
        if alias == record.camera_id {
            continue;
        }

        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| derived_stream_id(&record.camera_id));

        // Persist under the canonical alias key.
        persist_manifest_quick(&alias, Some(stream_id), manifest.clone()).await;

        // Remove the stale/colliding record.
        if let Ok(path) = record_path(&record.camera_id).await {
            let _ = fs::remove_file(path).await;
        }
    }
}

pub async fn list_persisted_records() -> Vec<PersistedStreamRecord> {
    static MIGRATED: OnceCell<()> = OnceCell::const_new();
    MIGRATED.get_or_init(|| async { migrate_file_camera_ids_once().await }).await;
    list_records().await
}

pub async fn update_manifest_pose_by_camera_id(camera_id: &str, pose: Option<RigPose>) -> std::io::Result<bool> {
    let path = record_path(camera_id).await?;
    let bytes = match fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let mut record = hydrate_record(parse_persisted_stream_record(&bytes).unwrap_or_default()).await;
    if record.camera_id.is_empty() {
        record.camera_id = camera_id.to_string();
    }
    let Some(mut manifest) = record.manifest else {
        return Ok(false);
    };
    manifest.pose = pose;
    match validate_stream_manifest(manifest.clone()).await {
        Ok(prepared) => {
            record.manifest = Some(prepared.manifest);
            record.resolved_config = Some(prepared.resolved);
        }
        Err(err) => {
            warn!(
                camera_id,
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "persisted stream pose update left manifest unresolved because semantic validation failed"
            );
            record.manifest = Some(manifest);
            record.resolved_config = None;
        }
    }
    record.updated_at = Some(now_rfc3339());

    let data = serde_json::to_vec(&record).map_err(io::Error::other)?;
    fs::write(&path, data).await?;
    Ok(true)
}

pub async fn list_pose_map() -> HashMap<String, RigPose> {
    let records = list_persisted_records().await;
    let mut out = HashMap::new();
    for record in records {
        if record.manifest.as_ref().map(|manifest| manifest.internal).unwrap_or(false) {
            continue;
        }
        if let Some(pose) = record.manifest.and_then(|manifest| manifest.pose) {
            out.insert(record.camera_id.clone(), pose);
        }
    }
    out
}

pub async fn pose_for_stream_id(stream_id: Uuid) -> Option<RigPose> {
    for record in list_persisted_records().await {
        let Some(manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let matches = manifest.identity.id == Some(stream_id) || record.last_stream_id == Some(stream_id) || derived_stream_id(&record.camera_id) == stream_id;
        if !matches {
            continue;
        }
        return manifest.pose;
    }
    None
}

pub fn derived_stream_id(camera_id: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, camera_id.as_bytes())
}

pub async fn remove_record_by_stream_id(stream_id: Uuid) -> io::Result<bool> {
    let records = list_records().await;
    for record in records {
        let Some(manifest) = record.manifest else {
            continue;
        };
        let manifest_id = manifest.identity.id;
        let matches = manifest_id == Some(stream_id) || record.last_stream_id == Some(stream_id) || derived_stream_id(&record.camera_id) == stream_id;
        if !matches {
            continue;
        }
        let path = record_path(&record.camera_id).await?;
        match fs::remove_file(path).await {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(false)
}

pub(crate) fn manifests_conflict(a: &ResolvedStreamConfig, b: &StreamManifest) -> bool {
    if a.capture.device_keys.is_empty() || b.capture.device_keys.is_empty() {
        return false;
    }
    a.capture.device_keys.iter().any(|key| b.capture.device_keys.iter().any(|other| other == key))
}

pub async fn restore_persisted_streams(state: AppState) {
    if engine_guard::safe_mode_active() {
        warn!("engine crash guard active; skipping persisted streams restore");
        return;
    }

    let records = list_records().await;
    if records.is_empty() {
        return;
    }

    let running = state.engine.list_streams().await.unwrap_or_default();
    for record in records {
        let Some(manifest) = record.manifest else {
            continue;
        };
        let manifest = hydrate_manifest(manifest).await;
        // `start_on_boot` manifests are handled by the autostart restore path. Re-processing them
        // here can generate duplicate start attempts + conflict spam on every API restart.
        if manifest.start_on_boot {
            continue;
        }
        // Always attempt to restore persisted streams so they survive rebuilds/uploads.
        if running.iter().any(|s| manifests_conflict(&s.manifest, &manifest)) {
            continue;
        }

        let prepared = match validate_stream_manifest(manifest).await {
            Ok(prepared) => prepared,
            Err(err) => {
                warn!(
                    camera_id = record.camera_id,
                    issue_count = err.issues.len(),
                    warning_count = err.warnings.len(),
                    issues = ?err.issues,
                    warnings = ?err.warnings,
                    "skipping persisted stream restore that failed stream preparation"
                );
                continue;
            }
        };
        let manifest = prepared.manifest;

        match state.engine.start_stream(prepared.resolved).await {
            Ok(helios_engine::ipc::EngineEvent::Started { stream_id, .. }) => {
                persist_manifest(&record.camera_id, Some(stream_id), manifest.clone()).await;
            }
            Ok(helios_engine::ipc::EngineEvent::Nack { reason, .. }) => {
                let reason_lc = reason.to_ascii_lowercase();
                if reason_lc.contains("stream already exists") || reason_lc.contains("already in use") || reason_lc.contains("conflict") {
                    // Best effort reconciliation: if a compatible stream is already running, bind
                    // this persisted record to that stream id and avoid warning on every restart.
                    if let Ok(active) = state.engine.list_streams().await {
                        let desired_alias = manifest.identity.alias.as_deref().map(str::trim).filter(|alias| !alias.is_empty());
                        if let Some(existing) = active.into_iter().find(|stream| {
                            manifests_conflict(&stream.manifest, &manifest)
                                || (manifest.identity.id.is_some() && stream.manifest.identity.id == manifest.identity.id)
                                || desired_alias.is_some_and(|alias| stream.manifest.identity.alias.as_deref().map(str::trim) == Some(alias))
                        }) {
                            persist_resolved_config(&record.camera_id, Some(existing.stream_id), existing.manifest.clone()).await;
                            info!(camera_id = %record.camera_id, stream_id = %existing.stream_id, "persisted stream already running; reconciled record");
                            continue;
                        }
                    }
                }
                warn!(camera_id = %record.camera_id, error = %reason, "persisted stream was rejected by engine");
            }
            Ok(_) => {
                warn!(camera_id = %record.camera_id, "persisted stream returned unexpected engine response");
            }
            Err(err) => {
                warn!(camera_id = %record.camera_id, error = %err, "failed to start persisted stream");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION;
    use serde_json::json;

    fn sample_manifest_json() -> serde_json::Value {
        json!({
            "identity": {},
            "capture": {
                "device_keys": [],
                "backend": "Virtual",
                "handle": { "type": "virtual" },
                "mode": {
                    "format": {
                        "code": "RGB3",
                        "resolution": { "width": 1, "height": 1 },
                        "color": "Srgb"
                    },
                    "interval": null
                },
                "controls": []
            }
        })
    }

    #[test]
    fn parse_persisted_record_migrates_legacy_versionless_record() {
        let bytes = serde_json::to_vec(&json!({
            "camera_id": "camera-a",
            "manifest": sample_manifest_json()
        }))
        .expect("encode legacy record");

        let record = parse_persisted_stream_record(&bytes).expect("migrate legacy record");
        assert_eq!(record.schema_version, CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION);
        assert_eq!(record.manifest.as_ref().map(|manifest| manifest.schema_version), Some(CURRENT_STREAM_CONFIG_SCHEMA_VERSION));
    }

    #[test]
    fn parse_persisted_record_rejects_unknown_future_schema_version() {
        let bytes = serde_json::to_vec(&json!({
            "schema_version": CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION + 1,
            "camera_id": "camera-a"
        }))
        .expect("encode future record");

        let err = parse_persisted_stream_record(&bytes).expect_err("future record should fail");
        assert!(err.contains("unsupported persisted stream record schema_version"));
    }
}
