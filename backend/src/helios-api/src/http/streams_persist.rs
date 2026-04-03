use crate::http::streams::validation::{StreamValidationResult, normalize_stream_manifest, validate_stream_manifest};
use crate::http::{json_store, storage};
use chrono::Utc;
use helios_engine::capture::{CaptureDescriptor, canonicalize_capture_config};
use helios_engine::ipc::{ResolvedStreamConfig, RigPose, StreamManifest};
use std::path::PathBuf;
use std::{collections::HashMap, io};
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

mod listing;
mod pose;
mod record;
use listing::list_records;
#[cfg(test)]
use record::CURRENT_PERSISTED_STREAM_RECORD_SCHEMA_VERSION;
pub(crate) use record::{PersistedStreamReconcileStatus, PersistedStreamRecord, derived_stream_id};
use record::{canonicalize_resolved_capture_identity, materialize_descriptor_snapshot, parse_persisted_stream_record};
pub(crate) use record::{descriptor_snapshot_for_record, ensure_descriptor_snapshot_has_mode, synthesize_descriptor_snapshot_from_manifest};

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

async fn remove_other_stream_records(camera_id: &str, stream_id: Option<Uuid>) -> io::Result<()> {
    let Some(stream_id) = stream_id else {
        return Ok(());
    };

    for record in list_records().await {
        let matches = record.stream_id() == Some(stream_id) || record.last_stream_id == Some(stream_id) || derived_stream_id(&record.camera_id) == stream_id;
        if !matches || record.camera_id == camera_id {
            continue;
        }
        let path = record_path(&record.camera_id).await?;
        match fs::remove_file(path).await {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }

    Ok(())
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

async fn persist_resolved_config_impl(camera_id: &str, stream_id: Option<Uuid>, mut resolved: ResolvedStreamConfig, descriptor_snapshot: Option<CaptureDescriptor>) -> std::io::Result<()> {
    resolved = canonicalize_resolved_capture_identity(resolved);
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
        record.descriptor_snapshot = Some(materialize_descriptor_snapshot(&record, &resolved, descriptor_snapshot));
        record.resolved_config = Some(resolved);
        record.updated_at = Some(now_rfc3339());
        record
    })
    .await
    .map(|_| ())
}

pub(crate) async fn prepare_manifest_for_persistence_checked(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamValidationResult> {
    let mut hydrated = hydrate_manifest(manifest).await;
    hydrated.capture = canonicalize_capture_config(&hydrated.capture);
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
    persist_resolved_config_impl(camera_id, stream_id, prepared.resolved, None).await?;
    Ok(prepared.manifest)
}

pub async fn persist_manifest_auto_camera_id_checked(stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<String> {
    let prepared = prepare_manifest_for_persistence_checked("<auto>", stream_id, manifest).await?;
    let camera_id = crate::http::streams::util::camera_id_for_manifest(&prepared.manifest);
    persist_resolved_config_impl(&camera_id, stream_id, prepared.resolved, None).await?;
    remove_other_stream_records(&camera_id, stream_id).await?;
    Ok(camera_id)
}

pub async fn persist_manifest_prepared_auto_camera_id_checked(stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<StreamManifest> {
    let prepared = prepare_manifest_for_persistence_checked("<auto>", stream_id, manifest).await?;
    let camera_id = crate::http::streams::util::camera_id_for_manifest(&prepared.manifest);
    persist_resolved_config_impl(&camera_id, stream_id, prepared.resolved, None).await?;
    remove_other_stream_records(&camera_id, stream_id).await?;
    Ok(prepared.manifest)
}

pub async fn persist_manifest_quick_auto_camera_id_checked(stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<String> {
    persist_manifest_auto_camera_id_checked(stream_id, manifest).await
}

pub async fn persist_resolved_config_auto_camera_id_checked(stream_id: Option<Uuid>, resolved: ResolvedStreamConfig) -> std::io::Result<String> {
    let resolved = canonicalize_resolved_capture_identity(resolved);
    let camera_id = crate::http::streams::util::camera_id_for_manifest(&resolved.to_requested_manifest());
    persist_resolved_config_impl(&camera_id, stream_id, resolved, None).await?;
    remove_other_stream_records(&camera_id, stream_id).await?;
    Ok(camera_id)
}

pub async fn persist_resolved_config_with_descriptor_auto_camera_id_checked(
    stream_id: Option<Uuid>,
    resolved: ResolvedStreamConfig,
    descriptor_snapshot: CaptureDescriptor,
) -> std::io::Result<String> {
    let resolved = canonicalize_resolved_capture_identity(resolved);
    let camera_id = crate::http::streams::util::camera_id_for_manifest(&resolved.to_requested_manifest());
    persist_resolved_config_impl(&camera_id, stream_id, resolved, Some(descriptor_snapshot)).await?;
    remove_other_stream_records(&camera_id, stream_id).await?;
    Ok(camera_id)
}

pub async fn persist_manifest_checked(camera_id: &str, stream_id: Option<Uuid>, manifest: StreamManifest) -> std::io::Result<()> {
    persist_manifest_impl(camera_id, stream_id, manifest).await.map(|_| ())
}

pub async fn persist_resolved_config_checked(camera_id: &str, stream_id: Option<Uuid>, resolved: ResolvedStreamConfig) -> std::io::Result<()> {
    persist_resolved_config_impl(camera_id, stream_id, resolved, None).await
}

pub async fn persist_resolved_config_with_descriptor_checked(camera_id: &str, stream_id: Option<Uuid>, resolved: ResolvedStreamConfig, descriptor_snapshot: CaptureDescriptor) -> std::io::Result<()> {
    persist_resolved_config_impl(camera_id, stream_id, resolved, Some(descriptor_snapshot)).await
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
        let mut resolved = canonicalize_resolved_capture_identity(resolved);
        if let Some(id) = stream_id {
            resolved.identity.id = Some(id);
            record.last_stream_id = Some(id);
        }
        if resolved.pose.is_none()
            && let Some(existing_pose) = record.resolved_config.as_ref().and_then(|config| config.pose.clone())
        {
            resolved.pose = Some(existing_pose);
        }
        record.descriptor_snapshot = Some(materialize_descriptor_snapshot(&record, &resolved, None));
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

pub async fn list_persisted_records() -> Vec<PersistedStreamRecord> {
    listing::list_persisted_records().await
}

pub async fn update_manifest_pose_by_camera_id(camera_id: &str, pose: Option<RigPose>) -> std::io::Result<bool> {
    pose::update_manifest_pose_by_camera_id(camera_id, pose).await
}

pub async fn list_pose_map() -> HashMap<String, RigPose> {
    pose::list_pose_map().await
}

pub async fn pose_for_stream_id(stream_id: Uuid) -> Option<RigPose> {
    pose::pose_for_stream_id(stream_id).await
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
    a.capture.matches_capture_target(&b.capture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::streams::util::build_stream_info;
    use crate::http::streams::validation::validate_stream_manifest;
    use helios_engine::services::StreamManager;
    use serde::Deserialize;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

    #[derive(Debug, Clone, Deserialize)]
    struct RoundTripFixtureFile {
        cases: Vec<RoundTripFixtureCase>,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct RoundTripFixtureCase {
        name: String,
        requested_manifest: StreamManifest,
        #[serde(default)]
        canonical_manifest: Option<StreamManifest>,
    }

    impl RoundTripFixtureCase {
        fn canonical_manifest(&self) -> &StreamManifest {
            self.canonical_manifest.as_ref().unwrap_or(&self.requested_manifest)
        }
    }

    fn sample_manifest_json() -> serde_json::Value {
        json!({
            "schema_version": 1,
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
    async fn parse_persisted_record_rejects_missing_schema_version() {
        let bytes = serde_json::to_vec(&json!({
            "camera_id": "camera-a",
            "manifest": sample_manifest_json()
        }))
        .expect("encode record");

        let err = parse_persisted_stream_record(&bytes).await.expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
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
    async fn parse_persisted_record_rejects_pre_reset_schema_versions() {
        for schema_version in [0, 2, 3] {
            let bytes = serde_json::to_vec(&json!({
                "schema_version": schema_version,
                "metadata": {
                    "camera_id": "camera-a",
                    "last_stream_id": Uuid::nil()
                },
                "stream": {
                    "resolved_config": sample_manifest().resolve()
                }
            }))
            .expect("encode old record");

            let err = parse_persisted_stream_record(&bytes).await.expect_err("old record should fail");
            assert!(err.contains("unsupported persisted stream record schema_version"), "unexpected error for schema_version={schema_version}: {err}");
        }
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
            descriptor_snapshot: None,
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
        assert!(encoded.get("stream").and_then(|value| value.get("descriptor_snapshot")).is_some());
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
        assert!(persisted_json.pointer("/stream/descriptor_snapshot").is_some());
        let loaded = load_resolved_config(&camera_id).await.expect("load resolved config");
        assert_eq!(loaded.identity.id, stream_id);
        let mut expected = canonicalize_resolved_capture_identity(resolved);
        expected.identity.id = stream_id;
        assert_eq!(serde_json::to_value(loaded).expect("encode loaded"), serde_json::to_value(expected).expect("encode resolved"));
    }

    #[tokio::test]
    async fn persist_resolved_config_auto_camera_id_checked_migrates_legacy_camera_id_for_same_stream() {
        let _root = test_data_root();
        let legacy_camera_id = format!("legacy-camera-{}", Uuid::new_v4());
        let stable_camera_id = format!("libcamera:front-{}", Uuid::new_v4());
        let stream_id = Some(Uuid::new_v4());

        let mut manifest = sample_manifest();
        manifest.capture.backend = styx::BackendKind::Libcamera;
        manifest.capture.handle = styx::BackendHandle::Libcamera { id: format!("missing-handle-{}", Uuid::new_v4()) };
        manifest.capture.device_keys = vec!["ov9782".to_string()];
        manifest.capture.device_identity = Some(helios_engine::capture::CaptureDeviceIdentity {
            display: Some("Front Camera".to_string()),
            primary_key: Some(stable_camera_id.clone()),
            keys: vec![stable_camera_id.clone(), "ov9782".to_string()],
        });

        let resolved = manifest.resolve();
        persist_resolved_config_checked(&legacy_camera_id, stream_id, resolved.clone()).await.expect("persist legacy camera id record");

        let legacy_path = record_path(&legacy_camera_id).await.expect("legacy record path");
        assert!(fs::try_exists(&legacy_path).await.expect("legacy record exists before migration"));

        let persisted_camera_id = persist_resolved_config_auto_camera_id_checked(stream_id, resolved.clone()).await.expect("persist canonical camera id record");
        assert_eq!(persisted_camera_id, stable_camera_id);

        let stable_path = record_path(&stable_camera_id).await.expect("stable record path");
        assert!(fs::try_exists(&stable_path).await.expect("stable record exists after migration"));
        assert!(!fs::try_exists(&legacy_path).await.expect("legacy record removed after migration"));

        let loaded = load_resolved_config(&stable_camera_id).await.expect("load stable resolved config");
        assert_eq!(loaded.identity.id, stream_id);
        assert_eq!(loaded.capture.device_identity.as_ref().and_then(|identity| identity.camera_id()), Some(stable_camera_id.clone()));
    }

    #[tokio::test]
    async fn persist_resolved_config_with_descriptor_auto_camera_id_checked_migrates_legacy_camera_id_for_same_stream() {
        let _root = test_data_root();
        let legacy_camera_id = format!("legacy-camera-{}", Uuid::new_v4());
        let stable_camera_id = format!("libcamera:front-{}", Uuid::new_v4());
        let stream_id = Some(Uuid::new_v4());

        let mut manifest = sample_manifest();
        manifest.capture.backend = styx::BackendKind::Libcamera;
        manifest.capture.handle = styx::BackendHandle::Libcamera { id: format!("missing-handle-{}", Uuid::new_v4()) };
        manifest.capture.device_keys = vec!["ov9782".to_string()];
        manifest.capture.device_identity = Some(helios_engine::capture::CaptureDeviceIdentity {
            display: Some("Front Camera".to_string()),
            primary_key: Some(stable_camera_id.clone()),
            keys: vec![stable_camera_id.clone(), "ov9782".to_string()],
        });

        let resolved = manifest.resolve();
        let descriptor_snapshot = synthesize_descriptor_snapshot_from_manifest(&resolved.to_requested_manifest());
        persist_resolved_config_checked(&legacy_camera_id, stream_id, resolved.clone()).await.expect("persist legacy camera id record");

        let legacy_path = record_path(&legacy_camera_id).await.expect("legacy record path");
        assert!(fs::try_exists(&legacy_path).await.expect("legacy record exists before migration"));

        let persisted_camera_id =
            persist_resolved_config_with_descriptor_auto_camera_id_checked(stream_id, resolved, descriptor_snapshot.clone()).await.expect("persist canonical camera id record with descriptor");
        assert_eq!(persisted_camera_id, stable_camera_id);

        let stable_path = record_path(&stable_camera_id).await.expect("stable record path");
        assert!(fs::try_exists(&stable_path).await.expect("stable record exists after migration"));
        assert!(!fs::try_exists(&legacy_path).await.expect("legacy record removed after migration"));

        let record = list_persisted_records().await.into_iter().find(|record| record.camera_id == stable_camera_id).expect("find stable record");
        let persisted_descriptor = record.descriptor_snapshot.expect("descriptor snapshot");
        assert_eq!(serde_json::to_value(&persisted_descriptor).expect("encode descriptor"), serde_json::to_value(&descriptor_snapshot).expect("encode expected descriptor"));
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

    fn round_trip_fixtures() -> &'static [RoundTripFixtureCase] {
        static FIXTURES: OnceLock<Vec<RoundTripFixtureCase>> = OnceLock::new();
        FIXTURES
            .get_or_init(|| serde_json::from_str::<RoundTripFixtureFile>(include_str!("../../../../../testdata/stream_config_roundtrip.json")).expect("decode round-trip fixture file").cases)
            .as_slice()
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
        assert!(persisted_json.get("stream").and_then(|value| value.get("descriptor_snapshot")).is_some());
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
        assert!(record.descriptor_snapshot.is_some());
    }

    #[tokio::test]
    async fn persist_manifest_checked_writes_descriptor_snapshot_with_requested_mode() {
        let _root = test_data_root();
        let camera_id = format!("camera-{}", Uuid::new_v4());
        let stream_id = Some(Uuid::new_v4());
        let manifest = sample_manifest();

        persist_manifest_checked(&camera_id, stream_id, manifest.clone()).await.expect("persist manifest");

        let record = list_persisted_records().await.into_iter().find(|record| record.camera_id == camera_id).expect("find persisted record");
        let descriptor = record.descriptor_snapshot.expect("descriptor snapshot");
        assert!(descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode));
    }

    #[test]
    fn descriptor_snapshot_for_record_synthesizes_when_snapshot_missing() {
        let manifest = sample_manifest();
        let record = PersistedStreamRecord { camera_id: "camera-a".to_string(), resolved_config: Some(manifest.resolve()), ..Default::default() };

        let descriptor = descriptor_snapshot_for_record(&record).expect("descriptor snapshot");

        assert!(descriptor.controls.is_empty());
        assert_eq!(descriptor.modes.len(), 1);
        assert_eq!(descriptor.modes[0].id, manifest.capture.mode);
    }

    #[test]
    fn descriptor_snapshot_for_record_rehydrates_requested_mode_into_existing_snapshot() {
        let manifest = sample_manifest();
        let alternate_format = MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(2, 2).unwrap(), ColorSpace::Srgb);
        let record = PersistedStreamRecord {
            camera_id: "camera-a".to_string(),
            resolved_config: Some(manifest.resolve()),
            descriptor_snapshot: Some(helios_engine::capture::CaptureDescriptor {
                modes: vec![helios_engine::capture::CaptureMode {
                    id: helios_engine::capture::ModeId { format: alternate_format, interval: None },
                    format: alternate_format,
                    intervals: Default::default(),
                    interval_stepwise: None,
                }],
                controls: Vec::new(),
            }),
            ..Default::default()
        };

        let descriptor = descriptor_snapshot_for_record(&record).expect("descriptor snapshot");

        assert_eq!(descriptor.modes.len(), 2);
        assert!(descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode));
        assert!(descriptor.modes.iter().any(|mode| mode.id.format == alternate_format));
    }

    #[test]
    fn canonicalize_for_write_clears_descriptor_without_resolved_config() {
        let alternate_format = MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(2, 2).unwrap(), ColorSpace::Srgb);
        let record = PersistedStreamRecord {
            camera_id: "camera-a".to_string(),
            descriptor_snapshot: Some(helios_engine::capture::CaptureDescriptor {
                modes: vec![helios_engine::capture::CaptureMode {
                    id: helios_engine::capture::ModeId { format: alternate_format, interval: None },
                    format: alternate_format,
                    intervals: Default::default(),
                    interval_stepwise: None,
                }],
                controls: Vec::new(),
            }),
            ..Default::default()
        }
        .canonicalize_for_write();

        assert!(record.descriptor_snapshot.is_none());
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

    #[tokio::test]
    async fn stream_config_fixtures_round_trip_through_validation_persistence_runtime_and_readback() {
        let _root = test_data_root();

        for fixture in round_trip_fixtures() {
            let requested = fixture.requested_manifest.clone();
            let expected_requested = serde_json::to_value(&requested).expect("encode requested fixture");
            let expected_canonical = serde_json::to_value(fixture.canonical_manifest()).expect("encode canonical fixture");
            let stream_id = requested.identity.id.expect("fixture stream id");
            let camera_id = format!("roundtrip-{}-{}", fixture.name, Uuid::new_v4());

            let validated = validate_stream_manifest(requested.clone()).await.unwrap_or_else(|err| panic!("fixture {} should validate: {err:?}", fixture.name));
            assert_eq!(serde_json::to_value(&validated.manifest).expect("encode validated manifest"), expected_requested);
            assert_eq!(serde_json::to_value(validated.resolved.to_requested_manifest()).expect("encode round-tripped manifest"), expected_requested);

            persist_manifest_checked(&camera_id, Some(stream_id), requested.clone()).await.unwrap_or_else(|err| panic!("fixture {} should persist: {err}", fixture.name));

            let loaded_requested = load_manifest(&camera_id).await.expect("load requested manifest");
            assert_eq!(serde_json::to_value(&loaded_requested).expect("encode loaded requested manifest"), expected_canonical);

            let loaded_resolved = load_resolved_config(&camera_id).await.expect("load resolved config");
            assert_eq!(serde_json::to_value(loaded_resolved.to_requested_manifest()).expect("encode loaded resolved manifest"), expected_canonical);

            let manager = StreamManager::new();
            let (started_id, descriptor) = manager.start_stream(loaded_resolved.clone()).await.unwrap_or_else(|err| panic!("fixture {} should start: {err}", fixture.name));
            assert_eq!(started_id, stream_id);

            let mut summaries = manager.list_streams().await;
            assert_eq!(summaries.len(), 1, "fixture {} should expose one active stream", fixture.name);
            let summary = summaries.pop().expect("summary");
            assert_eq!(summary.stream_id, stream_id);
            assert_eq!(serde_json::to_value(&summary.manifest).expect("encode runtime summary manifest"), serde_json::to_value(&loaded_resolved).expect("encode expected runtime manifest"));

            let readback = build_stream_info(summary.stream_id, descriptor.clone(), summary.manifest.clone(), Some(summary.status.clone()), Some(summary.runtime.clone()));
            assert_eq!(serde_json::to_value(&readback.manifest).expect("encode readback manifest"), expected_canonical);
            assert_eq!(serde_json::to_value(readback.resolved.to_requested_manifest()).expect("encode readback resolved manifest"), expected_canonical);

            manager.stop_stream(stream_id).await.unwrap_or_else(|err| panic!("fixture {} should stop cleanly: {err}", fixture.name));
            assert!(manager.list_streams().await.is_empty(), "fixture {} should leave no active streams", fixture.name);
        }
    }
}
