use crate::http::streams::validation::{StreamValidationResult, normalize_stream_manifest, validate_stream_manifest};
use crate::http::{json_store, storage};
use chrono::Utc;
use futures::future::BoxFuture;
use helios_engine::ipc::{ResolvedStreamConfig, RigPose, StreamManifest};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::path::PathBuf;
use std::{collections::HashMap, io};
use tokio::fs;
use tokio::sync::OnceCell;
use tracing::warn;
use uuid::Uuid;

const RAW_PIPELINE_UUID: Uuid = Uuid::from_u128(0x000000000000000000000000000000aa);
const LEGACY_RAW_PIPELINE_UUID: Uuid = Uuid::from_u128(0x000000000000000000000000000000ab);
const CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION: u32 = 2;
const FLAT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION: u32 = 1;
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedStreamMetadata {
    pub camera_id: String,
    #[serde(default)]
    pub last_stream_id: Option<Uuid>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconcile_status: Option<PersistedStreamReconcileStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconcile_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciled_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PersistedStreamReconcileStatus {
    Ready,
    Running,
    Invalid,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedStreamConfigPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_config: Option<ResolvedStreamConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStreamRecordWire {
    #[serde(default)]
    pub schema_version: u32,
    pub metadata: PersistedStreamMetadata,
    pub stream: PersistedStreamConfigPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedStreamCompatPayload {
    #[serde(default)]
    pub manifest: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_config: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedStreamCompatRecordWire {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub camera_id: String,
    #[serde(default)]
    pub last_stream_id: Option<Uuid>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub metadata: PersistedStreamMetadata,
    #[serde(default)]
    pub stream: PersistedStreamCompatPayload,
}

#[derive(Debug, Clone)]
pub struct PersistedStreamRecord {
    pub schema_version: u32,
    pub camera_id: String,
    pub last_stream_id: Option<Uuid>,
    pub updated_at: Option<String>,
    pub reconcile_status: Option<PersistedStreamReconcileStatus>,
    pub reconcile_error: Option<String>,
    pub reconciled_at: Option<String>,
    pub resolved_config: Option<ResolvedStreamConfig>,
}

#[derive(Debug)]
struct ParsedPersistedStreamRecord {
    record: PersistedStreamRecord,
    dirty: bool,
}

impl From<PersistedStreamRecordWire> for PersistedStreamRecord {
    fn from(value: PersistedStreamRecordWire) -> Self {
        Self {
            schema_version: value.schema_version,
            camera_id: value.metadata.camera_id,
            last_stream_id: value.metadata.last_stream_id,
            updated_at: value.metadata.updated_at,
            reconcile_status: value.metadata.reconcile_status,
            reconcile_error: value.metadata.reconcile_error,
            reconciled_at: value.metadata.reconciled_at,
            resolved_config: value.stream.resolved_config,
        }
    }
}

impl From<PersistedStreamRecord> for PersistedStreamRecordWire {
    fn from(value: PersistedStreamRecord) -> Self {
        Self {
            schema_version: value.schema_version,
            metadata: PersistedStreamMetadata {
                camera_id: value.camera_id,
                last_stream_id: value.last_stream_id,
                updated_at: value.updated_at,
                reconcile_status: value.reconcile_status,
                reconcile_error: value.reconcile_error,
                reconciled_at: value.reconciled_at,
            },
            stream: PersistedStreamConfigPayload { resolved_config: value.resolved_config },
        }
    }
}

impl Serialize for PersistedStreamRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PersistedStreamRecordWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PersistedStreamRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        PersistedStreamRecordWire::deserialize(deserializer).map(Into::into)
    }
}

impl PersistedStreamRecord {
    pub(crate) fn requested_manifest(&self) -> Option<StreamManifest> {
        self.resolved_config.as_ref().map(ResolvedStreamConfig::to_requested_manifest)
    }

    pub(crate) fn stream_id(&self) -> Option<Uuid> {
        self.resolved_config.as_ref().and_then(|resolved| resolved.identity.id).or(self.last_stream_id).or_else(|| (!self.camera_id.is_empty()).then(|| derived_stream_id(&self.camera_id)))
    }

    fn canonicalize_for_write(mut self) -> Self {
        self.schema_version = CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
        if self.last_stream_id.is_none() {
            self.last_stream_id = self.resolved_config.as_ref().and_then(|resolved| resolved.identity.id);
        }
        self
    }
}

impl Default for PersistedStreamRecord {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION,
            camera_id: String::new(),
            last_stream_id: None,
            updated_at: None,
            reconcile_status: None,
            reconcile_error: None,
            reconciled_at: None,
            resolved_config: None,
        }
    }
}

type PersistedStreamRecordMigration = fn(JsonValue) -> BoxFuture<'static, Result<JsonValue, String>>;

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

async fn load_record_from_path(path: PathBuf, camera_id_hint: Option<&str>) -> std::io::Result<Option<PersistedStreamRecord>> {
    let bytes = match fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };

    let parsed = match parse_persisted_stream_record(&bytes).await {
        Ok(parsed) => parsed,
        Err(err) => {
            if let Some(camera_id) = camera_id_hint {
                warn!(camera_id, error = %err, "failed to load persisted stream record");
            } else {
                warn!(path = %path.display(), error = %err, "failed to load persisted stream record");
            }
            return Ok(None);
        }
    };

    let mut record = parsed.record;
    let mut dirty = parsed.dirty;
    if record.camera_id.is_empty()
        && let Some(camera_id) = camera_id_hint
    {
        record.camera_id = camera_id.to_string();
        dirty = true;
    }

    if dirty {
        json_store::write_json(path.clone(), &record).await?;
    }

    Ok(Some(record))
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

fn schema_version_from_value(value: &JsonValue) -> Result<u32, String> {
    let Some(object) = value.as_object() else {
        return Err("persisted stream record must be a JSON object".to_string());
    };
    let Some(version) = object.get("schema_version") else {
        return Ok(LEGACY_PERSISTED_STREAM_RECORD_SCHEMA_VERSION);
    };
    let Some(version) = version.as_u64() else {
        return Err("persisted stream record `schema_version` must be an unsigned integer".to_string());
    };
    u32::try_from(version).map_err(|_| format!("persisted stream record schema_version {version} does not fit in u32"))
}

fn migrate_legacy_resolved_config_value(value: &mut JsonValue) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Err("persisted resolved_config must be a JSON object".to_string());
    };

    let legacy_recording_mode = object.remove("shadowRecorderEnabled").or_else(|| object.remove("shadow_recorder_enabled"));
    let current_recording_mode = object.contains_key("recordingMode") || object.contains_key("recording_mode");
    match (current_recording_mode, legacy_recording_mode) {
        (true, Some(_)) => Err("persisted resolved_config may not mix `recordingMode` with legacy `shadowRecorderEnabled`".to_string()),
        (false, Some(JsonValue::Bool(enabled))) => {
            object.insert("recordingMode".to_string(), JsonValue::Bool(enabled));
            Ok(())
        }
        (false, Some(_)) => Err("persisted resolved_config legacy `shadowRecorderEnabled` must be a boolean".to_string()),
        _ => Ok(()),
    }
}

async fn resolve_legacy_manifest_record(camera_id: &str, stream_id: Option<Uuid>, manifest_value: JsonValue) -> Result<ResolvedStreamConfig, String> {
    let manifest = serde_json::from_value::<StreamManifest>(manifest_value).map_err(|err| format!("failed to decode persisted stream manifest: {err}"))?;
    let prepared =
        prepare_manifest_for_persistence_checked(camera_id, stream_id, manifest).await.map_err(|err| format!("persisted stream manifest for `{camera_id}` failed migration validation: {err}"))?;
    Ok(prepared.resolved)
}

async fn canonicalize_current_record_value(value: JsonValue) -> Result<JsonValue, String> {
    let compat = serde_json::from_value::<PersistedStreamCompatRecordWire>(value).map_err(|err| format!("failed to decode persisted stream record: {err}"))?;
    let metadata = PersistedStreamMetadata {
        camera_id: if compat.metadata.camera_id.is_empty() { compat.camera_id } else { compat.metadata.camera_id },
        last_stream_id: compat.metadata.last_stream_id.or(compat.last_stream_id),
        updated_at: compat.metadata.updated_at.or(compat.updated_at),
        reconcile_status: compat.metadata.reconcile_status,
        reconcile_error: compat.metadata.reconcile_error,
        reconciled_at: compat.metadata.reconciled_at,
    };

    let resolved_config = match (compat.stream.resolved_config, compat.stream.manifest) {
        (Some(mut resolved_value), _) => {
            migrate_legacy_resolved_config_value(&mut resolved_value)?;
            serde_json::from_value::<ResolvedStreamConfig>(resolved_value).map_err(|err| format!("failed to decode persisted resolved_config: {err}"))?
        }
        (None, Some(manifest_value)) => resolve_legacy_manifest_record(&metadata.camera_id, metadata.last_stream_id, manifest_value).await?,
        (None, None) => return Err("persisted stream record missing `stream.resolved_config` payload".to_string()),
    };

    let record = PersistedStreamRecord {
        schema_version: CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION,
        camera_id: metadata.camera_id,
        last_stream_id: metadata.last_stream_id,
        updated_at: metadata.updated_at,
        reconcile_status: metadata.reconcile_status,
        reconcile_error: metadata.reconcile_error,
        reconciled_at: metadata.reconciled_at,
        resolved_config: Some(resolved_config),
    }
    .canonicalize_for_write();

    serde_json::to_value(PersistedStreamRecordWire::from(record)).map_err(|err| format!("failed to encode migrated persisted stream record: {err}"))
}

fn migrate_persisted_stream_record_v0_to_v1(mut value: JsonValue) -> BoxFuture<'static, Result<JsonValue, String>> {
    Box::pin(async move {
        let Some(object) = value.as_object_mut() else {
            return Err("persisted stream record must be a JSON object".to_string());
        };
        object.insert("schema_version".to_string(), JsonValue::from(FLAT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION));
        Ok(value)
    })
}

fn migrate_persisted_stream_record_v1_to_v2(value: JsonValue) -> BoxFuture<'static, Result<JsonValue, String>> {
    Box::pin(async move {
        let JsonValue::Object(mut object) = value else {
            return Err("persisted stream record must be a JSON object".to_string());
        };

        let camera_id = object.remove("camera_id").unwrap_or_else(|| JsonValue::String(String::new()));
        let last_stream_id = object.remove("last_stream_id").filter(|value| !value.is_null());
        let updated_at = object.remove("updated_at").filter(|value| !value.is_null());
        let manifest = object.remove("manifest").filter(|value| !value.is_null());
        let resolved_config = object.remove("resolved_config").filter(|value| !value.is_null());

        let mut metadata = JsonMap::new();
        metadata.insert("camera_id".to_string(), camera_id);
        if let Some(last_stream_id) = last_stream_id {
            metadata.insert("last_stream_id".to_string(), last_stream_id);
        }
        if let Some(updated_at) = updated_at {
            metadata.insert("updated_at".to_string(), updated_at);
        }

        let mut stream = JsonMap::new();
        if let Some(resolved_config) = resolved_config {
            stream.insert("resolved_config".to_string(), resolved_config);
        }
        if let Some(manifest) = manifest {
            stream.insert("manifest".to_string(), manifest);
        }

        let mut migrated = JsonMap::new();
        migrated.insert("schema_version".to_string(), JsonValue::from(CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION));
        migrated.insert("metadata".to_string(), JsonValue::Object(metadata));
        migrated.insert("stream".to_string(), JsonValue::Object(stream));
        Ok(JsonValue::Object(migrated))
    })
}

const PERSISTED_STREAM_RECORD_MIGRATIONS: &[(u32, PersistedStreamRecordMigration)] =
    &[(LEGACY_PERSISTED_STREAM_RECORD_SCHEMA_VERSION, migrate_persisted_stream_record_v0_to_v1), (FLAT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION, migrate_persisted_stream_record_v1_to_v2)];

async fn migrate_persisted_stream_record_value(mut value: JsonValue) -> Result<(JsonValue, bool), String> {
    let original = value.clone();
    let mut version = schema_version_from_value(&value)?;
    if version > CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION {
        return Err(format!("unsupported persisted stream record schema_version {}; current version is {}", version, CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION));
    }

    while version < CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION {
        let Some((_, migration)) = PERSISTED_STREAM_RECORD_MIGRATIONS.iter().find(|(from, _)| *from == version) else {
            return Err(format!("no persisted stream record migration registered from schema_version {} to {}", version, version + 1));
        };
        value = migration(value).await?;
        version = schema_version_from_value(&value)?.max(version + 1);
    }

    let canonical = canonicalize_current_record_value(value).await?;
    Ok((canonical.clone(), canonical != original))
}

async fn parse_persisted_stream_record(bytes: &[u8]) -> Result<ParsedPersistedStreamRecord, String> {
    let raw = serde_json::from_slice::<JsonValue>(bytes).map_err(|err| format!("failed to decode persisted stream record: {err}"))?;
    let (value, dirty) = migrate_persisted_stream_record_value(raw).await?;
    let record = serde_json::from_value::<PersistedStreamRecord>(value).map_err(|err| format!("failed to decode migrated persisted stream record: {err}"))?;
    if record.resolved_config.is_none() {
        return Err("migrated persisted stream record is missing `stream.resolved_config`".to_string());
    }
    Ok(ParsedPersistedStreamRecord { record: record.canonicalize_for_write(), dirty })
}

async fn update_record<F, Fut>(camera_id: &str, updater: F) -> std::io::Result<PersistedStreamRecord>
where
    F: FnOnce(PersistedStreamRecord) -> Fut,
    Fut: std::future::Future<Output = PersistedStreamRecord>,
{
    let path = record_path(camera_id).await?;
    let _ = load_record_from_path(path.clone(), Some(camera_id)).await?;
    json_store::update_json(path, move |mut current: PersistedStreamRecord| async move {
        if current.camera_id.is_empty() {
            current.camera_id = camera_id.to_string();
        }
        let next = updater(current).await;
        next.canonicalize_for_write()
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
            && let Some(existing_pose) = record.resolved_config.as_ref().and_then(|config| config.pose.clone())
        {
            resolved.pose = Some(existing_pose);
        }
        record.reconcile_error = None;
        record.resolved_config = Some(resolved);
        record.updated_at = Some(now_rfc3339());
        record
    })
    .await
    .map(|_| ())
}

pub(crate) async fn prepare_manifest_for_persistence_checked(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamValidationResult> {
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
        let detail = err.issues.first().map(|issue| format!("{} ({})", issue.message, issue.code)).unwrap_or_else(|| "unknown semantic validation failure".to_string());
        io::Error::new(io::ErrorKind::InvalidInput, format!("persisted stream manifest for `{camera_id}` failed semantic validation: {detail}"))
    })
}

async fn persist_manifest_impl(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamManifest> {
    let prepared = prepare_manifest_for_persistence_checked(camera_id, stream_id, manifest).await?;
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

pub(crate) async fn persist_reconcile_status_checked(camera_id: &str, status: PersistedStreamReconcileStatus, error: Option<String>) -> std::io::Result<()> {
    update_record(camera_id, move |mut record| async move {
        record.reconcile_status = Some(status);
        record.reconcile_error = error;
        record.reconciled_at = Some(now_rfc3339());
        record
    })
    .await
    .map(|_| ())
}

pub(crate) async fn persist_reconciled_resolved_config_checked(
    camera_id: &str,
    stream_id: Option<Uuid>,
    resolved: ResolvedStreamConfig,
    status: PersistedStreamReconcileStatus,
    error: Option<String>,
) -> std::io::Result<()> {
    update_record(camera_id, move |mut record| async move {
        let mut resolved = resolved;
        if let Some(id) = stream_id {
            resolved.identity.id = Some(id);
            record.last_stream_id = Some(id);
        }
        if resolved.pose.is_none()
            && let Some(existing_pose) = record.resolved_config.as_ref().and_then(|config| config.pose.clone())
        {
            resolved.pose = Some(existing_pose);
        }
        record.resolved_config = Some(resolved);
        record.reconcile_status = Some(status);
        record.reconcile_error = error;
        record.reconciled_at = Some(now_rfc3339());
        record.updated_at = Some(now_rfc3339());
        record
    })
    .await
    .map(|_| ())
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
    load_record_from_path(path, Some(camera_id)).await.ok().flatten()?.resolved_config
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
        let Ok(parsed) = parse_persisted_stream_record(&bytes).await else {
            continue;
        };
        let record = parsed.record;
        let mut dirty = parsed.dirty;
        if record.camera_id.is_empty() {
            dirty = true;
        }
        if dirty {
            let _ = json_store::write_json(path.clone(), &record).await;
        }
        out.push(record);
    }

    out
}

async fn migrate_file_camera_ids_once() {
    // Historical bug: File streams were sometimes persisted under a generic device key like
    // `media-file`, which collides across streams and creates duplicate persisted records for the
    // same stream UUID/alias. Canonicalize file streams to be stored under their alias.
    let records = list_records().await;
    for record in records {
        let Some(manifest) = record.requested_manifest() else {
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

        let stream_id = record.stream_id().unwrap_or_else(|| derived_stream_id(&record.camera_id));

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
    let Some(mut record) = load_record_from_path(path.clone(), Some(camera_id)).await? else {
        return Ok(false);
    };
    if record.camera_id.is_empty() {
        record.camera_id = camera_id.to_string();
    }
    let Some(mut resolved) = record.resolved_config.take() else {
        return Ok(false);
    };
    resolved.pose = pose;
    record.resolved_config = Some(resolved);
    record.updated_at = Some(now_rfc3339());

    json_store::write_json(path, &record.canonicalize_for_write()).await?;
    Ok(true)
}

pub async fn list_pose_map() -> HashMap<String, RigPose> {
    let records = list_persisted_records().await;
    let mut out = HashMap::new();
    for record in records {
        if record.resolved_config.as_ref().map(|resolved| resolved.internal).unwrap_or(false) {
            continue;
        }
        if let Some(pose) = record.resolved_config.and_then(|resolved| resolved.pose) {
            out.insert(record.camera_id.clone(), pose);
        }
    }
    out
}

pub async fn pose_for_stream_id(stream_id: Uuid) -> Option<RigPose> {
    for record in list_persisted_records().await {
        let Some(resolved) = record.resolved_config else {
            continue;
        };
        if resolved.internal {
            continue;
        }
        let matches = resolved.identity.id == Some(stream_id) || record.last_stream_id == Some(stream_id) || derived_stream_id(&record.camera_id) == stream_id;
        if !matches {
            continue;
        }
        return resolved.pose;
    }
    None
}

pub fn derived_stream_id(camera_id: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, camera_id.as_bytes())
}

pub async fn remove_record_by_stream_id(stream_id: Uuid) -> io::Result<bool> {
    let records = list_records().await;
    for record in records {
        let matches = record.stream_id() == Some(stream_id) || record.last_stream_id == Some(stream_id) || derived_stream_id(&record.camera_id) == stream_id;
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

#[cfg(test)]
mod tests {
    use super::*;
    use helios_engine::ipc::{CURRENT_STREAM_CONFIG_SCHEMA_VERSION, StreamRecordingMode, default_shadow_recording_codec};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::OnceLock;

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

    #[tokio::test]
    async fn parse_persisted_record_migrates_legacy_versionless_record() {
        let bytes = serde_json::to_vec(&json!({
            "camera_id": "camera-a",
            "manifest": sample_manifest_json()
        }))
        .expect("encode legacy record");

        let ParsedPersistedStreamRecord { record, dirty } = parse_persisted_stream_record(&bytes).await.expect("migrate legacy record");
        assert!(dirty);
        assert_eq!(record.schema_version, CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION);
        assert_eq!(record.requested_manifest().as_ref().map(|manifest| manifest.schema_version), Some(CURRENT_STREAM_CONFIG_SCHEMA_VERSION));
        assert!(record.resolved_config.is_some());
    }

    #[tokio::test]
    async fn parse_persisted_record_rejects_unknown_future_schema_version() {
        let bytes = serde_json::to_vec(&json!({
            "schema_version": CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION + 1,
            "metadata": {
                "camera_id": "camera-a"
            },
            "stream": {}
        }))
        .expect("encode future record");

        let err = parse_persisted_stream_record(&bytes).await.expect_err("future record should fail");
        assert!(err.contains("unsupported persisted stream record schema_version"));
    }

    #[tokio::test]
    async fn parse_persisted_record_migrates_flat_v1_record_to_current_schema() {
        let mut resolved = serde_json::to_value(sample_manifest().resolve()).expect("encode resolved config");
        let object = resolved.as_object_mut().expect("resolved config object");
        object.remove("recordingMode");
        object.insert("shadowRecorderEnabled".to_string(), serde_json::json!(true));

        let bytes = serde_json::to_vec(&json!({
            "schema_version": FLAT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION,
            "camera_id": "camera-a",
            "last_stream_id": Uuid::nil(),
            "resolved_config": resolved
        }))
        .expect("encode flat v1 record");

        let ParsedPersistedStreamRecord { record, dirty } = parse_persisted_stream_record(&bytes).await.expect("migrate flat v1 record");
        assert!(dirty);
        assert_eq!(record.schema_version, CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION);
        assert_eq!(record.camera_id, "camera-a");
        assert_eq!(record.last_stream_id, Some(Uuid::nil()));
        assert_eq!(record.resolved_config.as_ref().map(|resolved| resolved.recording_mode), Some(StreamRecordingMode::shadow_buffer(default_shadow_recording_codec())));
    }

    #[tokio::test]
    async fn parse_persisted_record_migrates_nested_v2_manifest_only_record() {
        let bytes = serde_json::to_vec(&json!({
            "schema_version": CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION,
            "metadata": {
                "camera_id": "camera-a",
                "last_stream_id": Uuid::nil()
            },
            "stream": {
                "manifest": sample_manifest_json()
            }
        }))
        .expect("encode manifest-only v2 record");

        let ParsedPersistedStreamRecord { record, dirty } = parse_persisted_stream_record(&bytes).await.expect("migrate manifest-only v2 record");
        assert!(dirty);
        assert_eq!(record.schema_version, CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION);
        assert!(record.resolved_config.is_some());
        assert_eq!(record.requested_manifest().as_ref().map(|manifest| manifest.schema_version), Some(CURRENT_STREAM_CONFIG_SCHEMA_VERSION));
    }

    #[test]
    fn persisted_record_serializes_nested_metadata_wrapper() {
        let record = PersistedStreamRecord {
            schema_version: CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION,
            camera_id: "camera-a".to_string(),
            last_stream_id: Some(Uuid::nil()),
            updated_at: Some("2026-03-31T00:00:00Z".to_string()),
            reconcile_status: Some(PersistedStreamReconcileStatus::Running),
            reconcile_error: None,
            reconciled_at: Some("2026-03-31T01:00:00Z".to_string()),
            resolved_config: Some(sample_manifest().resolve()),
        };

        let encoded = serde_json::to_value(record).expect("encode record");
        assert_eq!(encoded.get("schema_version").and_then(serde_json::Value::as_u64), Some(CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION as u64));
        assert!(encoded.get("camera_id").is_none());
        assert!(encoded.get("last_stream_id").is_none());
        assert!(encoded.get("updated_at").is_none());
        assert_eq!(encoded.get("metadata").and_then(|value| value.get("camera_id")).and_then(serde_json::Value::as_str), Some("camera-a"));
        assert_eq!(encoded.get("metadata").and_then(|value| value.get("reconcile_status")).and_then(serde_json::Value::as_str), Some("running"));
        assert_eq!(encoded.get("metadata").and_then(|value| value.get("reconciled_at")).and_then(serde_json::Value::as_str), Some("2026-03-31T01:00:00Z"));
        assert!(encoded.get("stream").and_then(|value| value.get("resolved_config")).is_some());
    }

    #[tokio::test]
    async fn persist_reconciled_resolved_config_checked_updates_reconcile_metadata() {
        let _root = test_data_root();
        let camera_id = format!("camera-{}", Uuid::new_v4());
        let stream_id = Some(Uuid::new_v4());
        let resolved = sample_manifest().resolve();

        persist_reconciled_resolved_config_checked(&camera_id, stream_id, resolved.clone(), PersistedStreamReconcileStatus::Ready, Some("warm-start".to_string()))
            .await
            .expect("persist reconciled config");

        let path = record_path(&camera_id).await.expect("record path");
        let bytes = fs::read(&path).await.expect("read persisted record");
        let persisted_json: serde_json::Value = serde_json::from_slice(&bytes).expect("decode persisted json");
        assert_eq!(persisted_json.pointer("/metadata/reconcile_status").and_then(serde_json::Value::as_str), Some("ready"));
        assert_eq!(persisted_json.pointer("/metadata/reconcile_error").and_then(serde_json::Value::as_str), Some("warm-start"));
        assert!(persisted_json.pointer("/metadata/reconciled_at").and_then(serde_json::Value::as_str).is_some());
        assert_eq!(persisted_json.pointer("/stream/resolved_config/identity/id").and_then(serde_json::Value::as_str), stream_id.map(|id| id.to_string()).as_deref());
        let loaded = load_resolved_config(&camera_id).await.expect("load resolved config");
        assert_eq!(loaded.identity.id, stream_id);
        let mut expected = resolved;
        expected.identity.id = stream_id;
        assert_eq!(serde_json::to_value(loaded).expect("encode loaded"), serde_json::to_value(expected).expect("encode resolved"));
    }

    fn test_data_root() -> PathBuf {
        static ROOT: OnceLock<PathBuf> = OnceLock::new();
        ROOT.get_or_init(|| {
            let path = std::env::temp_dir().join(format!("helios-api-streams-persist-tests-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create test data root");
            crate::http::storage::set_data_root_for_tests(path.clone());
            path
        })
        .clone()
    }

    fn sample_manifest() -> StreamManifest {
        serde_json::from_value(sample_manifest_json()).expect("decode sample manifest")
    }

    #[tokio::test]
    async fn persist_manifest_checked_writes_resolved_config_only() {
        let _root = test_data_root();
        let camera_id = format!("camera-{}", Uuid::new_v4());
        let stream_id = Some(Uuid::new_v4());
        let manifest = sample_manifest();
        let prepared = prepare_manifest_for_persistence_checked(&camera_id, stream_id, manifest.clone()).await.expect("prepare manifest");

        persist_manifest_checked(&camera_id, stream_id, manifest).await.expect("persist manifest");

        let path = record_path(&camera_id).await.expect("record path");
        let bytes = fs::read(&path).await.expect("read persisted record");
        let persisted_json: serde_json::Value = serde_json::from_slice(&bytes).expect("decode persisted json");
        assert_eq!(persisted_json.get("schema_version").and_then(serde_json::Value::as_u64), Some(CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION as u64));
        assert_eq!(persisted_json.get("metadata").and_then(|value| value.get("camera_id")).and_then(serde_json::Value::as_str), Some(camera_id.as_str()));
        assert_eq!(persisted_json.get("metadata").and_then(|value| value.get("last_stream_id")).and_then(serde_json::Value::as_str), stream_id.map(|id| id.to_string()).as_deref());
        assert!(persisted_json.get("camera_id").is_none());
        assert!(persisted_json.get("resolved_config").is_none());
        assert!(persisted_json.get("stream").and_then(|value| value.get("resolved_config")).is_some());
        assert!(
            persisted_json.get("stream").and_then(|value| value.get("manifest")).is_none()
                || persisted_json.get("stream").and_then(|value| value.get("manifest")).is_some_and(serde_json::Value::is_null)
        );

        let loaded = load_resolved_config(&camera_id).await.expect("load resolved config");
        assert_eq!(serde_json::to_value(&loaded).expect("encode loaded"), serde_json::to_value(&prepared.resolved).expect("encode prepared"));

        let record = list_persisted_records().await.into_iter().find(|record| record.camera_id == camera_id).expect("find persisted record");
        assert_eq!(serde_json::to_value(record.resolved_config.as_ref()).expect("encode resolved"), serde_json::to_value(Some(&prepared.resolved)).expect("encode expected resolved"));
        assert_eq!(
            serde_json::to_value(record.requested_manifest()).expect("encode manifest"),
            serde_json::to_value(Some(prepared.resolved.to_requested_manifest())).expect("encode expected manifest")
        );
    }

    #[tokio::test]
    async fn update_manifest_pose_keeps_record_resolved_only_on_disk() {
        let _root = test_data_root();
        let camera_id = format!("camera-{}", Uuid::new_v4());
        let stream_id = Some(Uuid::new_v4());

        persist_manifest_checked(&camera_id, stream_id, sample_manifest()).await.expect("persist manifest");

        let pose = RigPose {
            translation: helios_engine::ipc::PoseVector { x: 1.0, y: 2.0, z: 3.0 },
            rotation: helios_engine::ipc::PoseRotation { roll: 4.0, pitch: 5.0, yaw: 6.0 },
            updated_at: Some("2026-03-31T00:00:00Z".to_string()),
        };

        assert!(update_manifest_pose_by_camera_id(&camera_id, Some(pose.clone())).await.expect("update pose"));

        let path = record_path(&camera_id).await.expect("record path");
        let bytes = fs::read(&path).await.expect("read persisted record");
        let persisted_json: serde_json::Value = serde_json::from_slice(&bytes).expect("decode persisted json");
        assert_eq!(persisted_json.get("schema_version").and_then(serde_json::Value::as_u64), Some(CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION as u64));
        assert_eq!(
            persisted_json.get("stream").and_then(|value| value.get("resolved_config")).and_then(|value| value.get("pose")).cloned().expect("resolved pose"),
            serde_json::to_value(&pose).expect("encode pose")
        );
        assert!(
            persisted_json.get("stream").and_then(|value| value.get("manifest")).is_none()
                || persisted_json.get("stream").and_then(|value| value.get("manifest")).is_some_and(serde_json::Value::is_null)
        );

        assert_eq!(
            serde_json::to_value(list_pose_map().await.get(&camera_id).cloned()).expect("encode pose map entry"),
            serde_json::to_value(Some(pose.clone())).expect("encode expected pose map entry")
        );
        let loaded = load_resolved_config(&camera_id).await.expect("load resolved config");
        assert_eq!(serde_json::to_value(loaded.pose).expect("encode loaded pose"), serde_json::to_value(Some(pose)).expect("encode expected loaded pose"));
    }
}
