use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::ipc::UpdaterEvent;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use futures::StreamExt;
use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast::Sender;
use tracing::warn;
use url::Url;
use uuid::Uuid;

use crate::config::{SignaturePolicy, UpdaterConfig};
use crate::error::{Error, Result};
use crate::ipc::UrlArtifact;

const SIGNATURE_CONTEXT: &[u8] = b"helios-ota-v1\0";
const CURRENT_RELEASE_MANIFEST_METADATA_SCHEMA_VERSION: u32 = 1;
const CURRENT_STAGED_METADATA_SCHEMA_VERSION: u32 = 1;

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    #[serde(default)]
    pub update_id: Option<Uuid>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<ManifestArtifact>,
    /// Optional JSON metadata as a string.
    ///
    /// This is kept as a plain string because our IPC layer uses tagged binary envelopes and
    /// `serde_json::Value` deserialization requires `deserialize_any`, which is
    /// not supported cleanly by the transport serializer (causing the updater IPC
    /// command decode to fail).
    #[serde(default)]
    pub metadata_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseManifestMetadata {
    pub schema_version: u32,
    #[serde(default = "default_true")]
    pub auto_apply: bool,
    #[serde(default = "default_true")]
    pub delete_image_after_apply: bool,
    #[serde(default)]
    pub source_artifact_path: Option<String>,
}

impl Default for ReleaseManifestMetadata {
    fn default() -> Self {
        Self { schema_version: CURRENT_RELEASE_MANIFEST_METADATA_SCHEMA_VERSION, auto_apply: true, delete_image_after_apply: true, source_artifact_path: None }
    }
}

impl ReleaseManifestMetadata {
    #[must_use]
    pub fn manual_stage(delete_image_after_apply: bool, source_artifact_path: Option<String>) -> Self {
        Self { auto_apply: false, delete_image_after_apply, source_artifact_path, ..Self::default() }
    }

    pub fn encode_json(&self) -> core::result::Result<String, String> {
        let mut canonical = self.clone();
        canonical.schema_version = CURRENT_RELEASE_MANIFEST_METADATA_SCHEMA_VERSION;
        serde_json::to_string(&canonical).map_err(|err| format!("failed to encode release manifest metadata: {err}"))
    }

    pub fn decode_json(raw: &str) -> core::result::Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "{}" {
            return Ok(Self::default());
        }
        let value = serde_json::from_str::<serde_json::Value>(trimmed).map_err(|err| format!("failed to decode release manifest metadata: {err}"))?;
        let migrated = migrate_to_current(value, &RELEASE_MANIFEST_METADATA_SCHEMA_PLAN)?;
        serde_json::from_value(migrated).map_err(|err| format!("failed to parse release manifest metadata: {err}"))
    }

    pub fn from_manifest(manifest: &ReleaseManifest) -> core::result::Result<Self, String> {
        Self::decode_json(&manifest.metadata_json)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestArtifact {
    pub url: Url,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

impl ManifestArtifact {
    pub fn resolved_filename(&self, index: usize) -> String {
        if let Some(name) = &self.filename
            && !name.is_empty()
        {
            return name.clone();
        }
        self.url.path_segments().and_then(|mut segments| segments.next_back().map(|s| s.to_string())).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("artifact-{index:02}.bin"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedArtifact {
    pub url: Url,
    pub local_path: PathBuf,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub staged_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StagedMetadata {
    pub schema_version: u32,
    pub manifest: ReleaseManifest,
    pub artifacts: Vec<StagedArtifact>,
    pub staged_at: DateTime<Utc>,
}

const RELEASE_MANIFEST_METADATA_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan {
    document_name: "release manifest metadata",
    legacy_version: CURRENT_RELEASE_MANIFEST_METADATA_SCHEMA_VERSION,
    current_version: CURRENT_RELEASE_MANIFEST_METADATA_SCHEMA_VERSION,
    migrations: &[],
};

const STAGED_METADATA_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> =
    SyncSchemaPlan { document_name: "staged metadata", legacy_version: CURRENT_STAGED_METADATA_SCHEMA_VERSION, current_version: CURRENT_STAGED_METADATA_SCHEMA_VERSION, migrations: &[] };

fn parse_staged_metadata(bytes: &[u8]) -> Result<StagedMetadata> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(Error::SerdeJson)?;
    let migrated = migrate_to_current(raw, &STAGED_METADATA_SCHEMA_PLAN).map_err(Error::InvalidState)?;
    serde_json::from_value(migrated).map_err(Error::SerdeJson)
}

struct DownloadContext {
    update_id: Uuid,
    artifact_index: usize,
    total_artifacts: usize,
}

pub async fn stage_release(client: &Client, config: &UpdaterConfig, update_id: Uuid, manifest: &ReleaseManifest, event_tx: &Sender<UpdaterEvent>) -> Result<Vec<StagedArtifact>> {
    let release_dir = config.cache_dir().join(update_id.to_string());
    let work_dir = config.work_dir().join(update_id.to_string());
    fs::create_dir_all(&release_dir).await?;
    fs::create_dir_all(&work_dir).await?;

    let started_at = Utc::now();
    let total_artifacts = manifest.artifacts.len().max(1);
    let mut staged_files = Vec::with_capacity(manifest.artifacts.len());

    for (index, artifact) in manifest.artifacts.iter().enumerate() {
        let filename = artifact.resolved_filename(index);
        let tmp_path = work_dir.join(format!("{}.partial", filename));
        let final_path = release_dir.join(&filename);

        emit_event(
            event_tx,
            UpdaterEvent::StageProgress { update_id, percent: ramp_percent(index, total_artifacts, 0), detail: Some(format!("downloading {} ({}/{})", filename, index + 1, total_artifacts)) },
        );

        let staged = download_artifact(client, config, artifact, (&tmp_path, &final_path), event_tx, DownloadContext { update_id, artifact_index: index, total_artifacts }).await?;

        emit_event(event_tx, UpdaterEvent::StageProgress { update_id, percent: ramp_percent(index, total_artifacts, 100), detail: Some(format!("stored {}", filename)) });

        staged_files.push(staged);
    }

    persist_metadata(&release_dir, manifest.clone(), staged_files.clone(), started_at).await?;
    Ok(staged_files)
}

fn ramp_percent(artifact_index: usize, total: usize, within: u8) -> u8 {
    if total == 0 {
        return within.min(100);
    }
    let start = ((artifact_index as f64) / (total as f64) * 100.0).floor() as u8;
    let end = (((artifact_index + 1) as f64) / (total as f64) * 100.0).ceil().min(100.0) as u8;
    let span = end.saturating_sub(start).max(1);
    start.saturating_add(((span as u16 * within as u16) / 100) as u8).min(100)
}

async fn download_artifact(
    client: &Client,
    config: &UpdaterConfig,
    artifact: &ManifestArtifact,
    paths: (&Path, &Path),
    event_tx: &Sender<UpdaterEvent>,
    ctx: DownloadContext,
) -> Result<StagedArtifact> {
    let mut hasher = Sha256::new();
    let written: u64;

    if artifact.url.scheme() == "file" {
        let source = artifact.url.to_file_path().map_err(|_| Error::InvalidState(format!("invalid file url for artifact: {}", artifact.url)))?;
        let total_len = tokio::fs::metadata(&source).await.ok().map(|meta| meta.len()).filter(|len| *len > 0);
        let mut last_within = 0u8;

        // Avoid copying large local images into the OTA cache. Many deployments
        // have a small /var/lib/helios DATA partition, and duplicating a
        // near-1GB disk image would exhaust space and break the updater (even
        // for QueryState journal appends).
        //
        // We still compute and verify the sha256/signature, then reference the
        // existing path directly.
        let mut reader = tokio::fs::File::open(&source).await.map_err(Error::Io)?;
        let mut buf = [0u8; 128 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = reader.read(&mut buf).await.map_err(Error::Io)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;

            if let Some(len) = total_len {
                let within = ((total as f64 / len as f64) * 100.0).clamp(0.0, 100.0).round() as u8;
                if within > last_within {
                    last_within = within;
                    emit_event(
                        event_tx,
                        UpdaterEvent::StageProgress {
                            update_id: ctx.update_id,
                            percent: ramp_percent(ctx.artifact_index, ctx.total_artifacts, within),
                            detail: Some(format!("verifying {} ({}%)", paths.1.file_name().unwrap_or_default().to_string_lossy(), within)),
                        },
                    );
                }
            }
        }
        written = total;
    } else {
        let response = client.get(artifact.url.clone()).send().await?.error_for_status()?;
        let declared_len = artifact.size_bytes.or(response.content_length());
        let mut stream = response.bytes_stream();
        let mut file = fs::File::create(paths.0).await?;
        let mut written_acc: u64 = 0;
        let mut last_percent = 0u8;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            hasher.update(&chunk);
            written_acc += chunk.len() as u64;

            if let Some(total) = declared_len
                && total > 0
            {
                let within = ((written_acc as f64 / total as f64) * 100.0).clamp(0.0, 100.0).round() as u8;
                if within > last_percent {
                    last_percent = within;
                    emit_event(
                        event_tx,
                        UpdaterEvent::StageProgress {
                            update_id: ctx.update_id,
                            percent: ramp_percent(ctx.artifact_index, ctx.total_artifacts, within),
                            detail: Some(format!("downloading {} ({}%)", paths.1.file_name().unwrap_or_default().to_string_lossy(), within)),
                        },
                    );
                }
            }
        }

        file.flush().await?;
        written = written_acc;
    }

    let digest = hasher.finalize();
    let digest_hex = hex::encode(digest.as_slice());
    if let Some(expected) = &artifact.sha256
        && !expected.trim().eq_ignore_ascii_case(&digest_hex)
    {
        return Err(Error::ArtifactVerificationFailed(format!("sha256 mismatch for {} (expected {}, got {})", paths.1.display(), expected, digest_hex)));
    }

    verify_signature(config.signature_policy(), digest.as_slice(), artifact)?;

    if artifact.url.scheme() == "file" {
        let source = artifact.url.to_file_path().map_err(|_| Error::InvalidState(format!("invalid file url for artifact: {}", artifact.url)))?;
        let filename = source.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| paths.1.file_name().unwrap_or_default().to_string_lossy().into_owned());
        return Ok(StagedArtifact { url: artifact.url.clone(), local_path: source, filename, size_bytes: written, sha256: digest_hex, staged_at: Utc::now() });
    }

    if fs::metadata(paths.1).await.is_ok() {
        fs::remove_file(paths.1).await?;
    }
    fs::rename(paths.0, paths.1).await?;

    let staged = StagedArtifact {
        url: artifact.url.clone(),
        local_path: paths.1.to_path_buf(),
        filename: paths.1.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "artifact".into()),
        size_bytes: written,
        sha256: digest_hex,
        staged_at: Utc::now(),
    };

    Ok(staged)
}

async fn persist_metadata(dir: &Path, manifest: ReleaseManifest, artifacts: Vec<StagedArtifact>, staged_at: DateTime<Utc>) -> Result<()> {
    let metadata = StagedMetadata { schema_version: CURRENT_STAGED_METADATA_SCHEMA_VERSION, manifest, artifacts, staged_at };
    let path = dir.join("metadata.json");
    let serialized = serde_json::to_vec_pretty(&metadata)?;
    fs::write(path, serialized).await?;
    Ok(())
}

fn verify_signature(policy: &SignaturePolicy, digest: &[u8], artifact: &ManifestArtifact) -> Result<()> {
    if !policy.is_enabled() {
        return Ok(());
    }

    let signature_b64 = match &artifact.signature {
        Some(sig) => sig,
        None if policy.is_required() => {
            return Err(Error::ArtifactVerificationFailed(format!("signature required for {}", artifact.url)));
        }
        None => return Ok(()),
    };

    let decoded = base64::engine::general_purpose::STANDARD.decode(signature_b64)?;
    let signature = Signature::from_slice(&decoded)?;

    let mut message = SIGNATURE_CONTEXT.to_vec();
    message.extend_from_slice(digest);

    for key in policy.keys() {
        if key.verify_strict(&message, &signature).is_ok() {
            return Ok(());
        }
    }

    Err(Error::ArtifactVerificationFailed(format!("signature verification failed for {}", artifact.url)))
}

fn emit_event(tx: &Sender<UpdaterEvent>, event: UpdaterEvent) {
    if let Err(err) = tx.send(event) {
        warn!(?err, "failed to send updater event");
    }
}

pub async fn cache_usage_bytes(dir: &Path) -> Result<u64> {
    let mut total: u64 = 0;
    let mut stack = vec![dir.to_path_buf()];

    while let Some(path) = stack.pop() {
        let mut entries = match fs::read_dir(&path).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.into()),
        };

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }

    Ok(total)
}

pub fn staged_to_url_artifacts(staged: &[StagedArtifact]) -> Vec<UrlArtifact> {
    staged.iter().map(|artifact| UrlArtifact { url: artifact.url.clone(), size_bytes: Some(artifact.size_bytes), checksum: Some(artifact.sha256.clone()) }).collect()
}

pub async fn load_metadata(config: &UpdaterConfig, update_id: Uuid) -> Result<StagedMetadata> {
    let path = config.cache_dir().join(update_id.to_string()).join("metadata.json");
    let bytes = match fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Err(Error::InvalidState(format!("staged metadata missing for update {}", update_id))),
        Err(err) => return Err(Error::Io(err)),
    };
    parse_staged_metadata(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ReleaseManifest {
        ReleaseManifest {
            update_id: Some(Uuid::nil()),
            version: Some("test".into()),
            artifacts: vec![ManifestArtifact {
                url: Url::parse("https://example.invalid/update.bin").expect("url"),
                filename: Some("update.bin".into()),
                size_bytes: Some(42),
                sha256: None,
                signature: None,
                kind: Some("disk-image".into()),
            }],
            metadata_json: "{}".into(),
        }
    }

    #[test]
    fn release_manifest_metadata_defaults_for_empty_payload() {
        let parsed = ReleaseManifestMetadata::decode_json("").expect("parse");
        assert_eq!(parsed, ReleaseManifestMetadata { schema_version: CURRENT_RELEASE_MANIFEST_METADATA_SCHEMA_VERSION, auto_apply: true, delete_image_after_apply: true, source_artifact_path: None });
    }

    #[test]
    fn release_manifest_metadata_rejects_missing_schema_version() {
        let err = ReleaseManifestMetadata::decode_json(r#"{"auto_apply":false,"delete_image_after_apply":true,"source_artifact_path":"/var/lib/helios/api-data/media/update.tar"}"#)
            .expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn release_manifest_metadata_rejects_future_schema() {
        let err = ReleaseManifestMetadata::decode_json(r#"{"schema_version":2,"auto_apply":true}"#).expect_err("future schema should fail");
        assert!(err.contains("unsupported release manifest metadata schema_version"));
    }

    #[test]
    fn parse_staged_metadata_rejects_missing_schema_version() {
        let raw = serde_json::json!({
            "manifest": sample_manifest(),
            "artifacts": [],
            "staged_at": "2026-01-01T00:00:00Z"
        });

        let err = parse_staged_metadata(&serde_json::to_vec(&raw).expect("encode")).expect_err("missing schema version should fail");
        match err {
            Error::InvalidState(message) => assert!(message.contains("missing required schema_version")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parse_staged_metadata_rejects_future_schema() {
        let raw = serde_json::json!({
            "schema_version": CURRENT_STAGED_METADATA_SCHEMA_VERSION + 1,
            "manifest": sample_manifest(),
            "artifacts": [],
            "staged_at": "2026-01-01T00:00:00Z"
        });

        let err = parse_staged_metadata(&serde_json::to_vec(&raw).expect("encode")).expect_err("future schema should fail");
        match err {
            Error::InvalidState(message) => assert!(message.contains("unsupported staged metadata schema_version")),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
