use helios_engine::capture::{BackendKind, CaptureDescriptor, CaptureMode, canonicalize_capture_config, descriptor_snapshot_for_config, discover_devices};
use helios_engine::ipc::{ResolvedStreamConfig, StreamManifest};
use lib_schema_migration::{AsyncSchemaPlan, normalize_to_current_async};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

pub(crate) const CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION: u32 = 1;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor_snapshot: Option<CaptureDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStreamRecordWire {
    #[serde(default)]
    pub schema_version: u32,
    pub metadata: PersistedStreamMetadata,
    pub stream: PersistedStreamConfigPayload,
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
    pub descriptor_snapshot: Option<CaptureDescriptor>,
}

#[derive(Debug)]
pub(super) struct ParsedPersistedStreamRecord {
    pub(super) record: PersistedStreamRecord,
    pub(super) dirty: bool,
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
            descriptor_snapshot: value.stream.descriptor_snapshot,
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
            stream: PersistedStreamConfigPayload { resolved_config: value.resolved_config, descriptor_snapshot: value.descriptor_snapshot },
        }
    }
}

impl Serialize for PersistedStreamRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PersistedStreamRecordWire::from(self.clone().canonicalize_for_write()).serialize(serializer)
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

    pub(crate) fn canonicalize_for_write(mut self) -> Self {
        self.schema_version = CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
        if self.last_stream_id.is_none() {
            self.last_stream_id = self.resolved_config.as_ref().and_then(|resolved| resolved.identity.id);
        }
        if let Some(resolved) = self.resolved_config.as_ref() {
            let manifest = resolved.to_requested_manifest();
            let mut descriptor = self.descriptor_snapshot.take().unwrap_or_else(|| synthesize_descriptor_snapshot_from_manifest(&manifest));
            ensure_descriptor_snapshot_has_mode(&mut descriptor, &manifest);
            self.descriptor_snapshot = Some(descriptor);
        } else {
            self.descriptor_snapshot = None;
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
            descriptor_snapshot: None,
        }
    }
}

pub(crate) fn synthesize_descriptor_snapshot_from_manifest(manifest: &StreamManifest) -> CaptureDescriptor {
    let mode_id = manifest.capture.mode.clone();
    let format = mode_id.format;
    let intervals = mode_id.interval.into_iter().collect();
    let mode = CaptureMode { id: mode_id, format, intervals, interval_stepwise: None };
    CaptureDescriptor { modes: vec![mode], controls: Vec::new() }
}

pub(crate) fn ensure_descriptor_snapshot_has_mode(descriptor: &mut CaptureDescriptor, manifest: &StreamManifest) {
    if descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode) {
        return;
    }

    let mode_id = manifest.capture.mode.clone();
    let format = mode_id.format;
    let intervals = mode_id.interval.into_iter().collect();
    descriptor.modes.push(CaptureMode { id: mode_id, format, intervals, interval_stepwise: None });
}

fn discover_descriptor_snapshot_for_resolved(resolved: &ResolvedStreamConfig) -> Option<CaptureDescriptor> {
    match resolved.capture.backend {
        BackendKind::Libcamera | BackendKind::V4l2 => {
            let devices = discover_devices();
            descriptor_snapshot_for_config(&resolved.capture, &devices)
        }
        _ => None,
    }
}

pub(super) fn materialize_descriptor_snapshot(record: &PersistedStreamRecord, resolved: &ResolvedStreamConfig, provided: Option<CaptureDescriptor>) -> CaptureDescriptor {
    let manifest = resolved.to_requested_manifest();
    let mut descriptor = provided
        .or_else(|| discover_descriptor_snapshot_for_resolved(resolved))
        .or_else(|| record.resolved_config.as_ref().filter(|existing| existing.capture.matches_capture_target(&resolved.capture)).and(record.descriptor_snapshot.clone()))
        .unwrap_or_else(|| synthesize_descriptor_snapshot_from_manifest(&manifest));
    ensure_descriptor_snapshot_has_mode(&mut descriptor, &manifest);
    descriptor
}

pub(crate) fn descriptor_snapshot_for_record(record: &PersistedStreamRecord) -> Option<CaptureDescriptor> {
    let manifest = record.requested_manifest()?;
    let mut descriptor = record.descriptor_snapshot.clone().unwrap_or_else(|| synthesize_descriptor_snapshot_from_manifest(&manifest));
    ensure_descriptor_snapshot_has_mode(&mut descriptor, &manifest);
    Some(descriptor)
}

pub(crate) fn canonicalize_resolved_capture_identity(mut resolved: ResolvedStreamConfig) -> ResolvedStreamConfig {
    resolved.capture = canonicalize_capture_config(&resolved.capture);
    resolved
}

async fn canonicalize_current_record_value(value: JsonValue) -> Result<JsonValue, String> {
    let wire = serde_json::from_value::<PersistedStreamRecordWire>(value).map_err(|err| format!("failed to decode persisted stream record: {err}"))?;
    let record = PersistedStreamRecord::from(wire).canonicalize_for_write();
    if record.resolved_config.is_none() {
        return Err("persisted stream record missing `stream.resolved_config` payload".to_string());
    }
    serde_json::to_value(PersistedStreamRecordWire::from(record)).map_err(|err| format!("failed to encode canonical persisted stream record: {err}"))
}

const PERSISTED_STREAM_RECORD_SCHEMA_PLAN: AsyncSchemaPlan<JsonValue> = AsyncSchemaPlan::strict("persisted stream record", CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION);

pub(super) async fn parse_persisted_stream_record(bytes: &[u8]) -> Result<ParsedPersistedStreamRecord, String> {
    let mut value = serde_json::from_slice::<JsonValue>(bytes).map_err(|err| format!("failed to decode persisted stream record: {err}"))?;
    let original = value.clone();
    value = normalize_to_current_async(value, &PERSISTED_STREAM_RECORD_SCHEMA_PLAN).await?;
    let canonical = canonicalize_current_record_value(value).await?;
    let dirty = canonical != original;
    let record = serde_json::from_value::<PersistedStreamRecord>(canonical).map_err(|err| format!("failed to decode canonical persisted stream record: {err}"))?;
    Ok(ParsedPersistedStreamRecord { record: record.canonicalize_for_write(), dirty })
}

pub fn derived_stream_id(camera_id: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, camera_id.as_bytes())
}
