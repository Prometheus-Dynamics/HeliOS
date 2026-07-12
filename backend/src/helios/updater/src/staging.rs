use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::StreamExt;
use reqwest::Certificate;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::OnceCell;
use url::Url;

use crate::{
    config::UpdaterConfig,
    manifest::{BootAssetSpec, PayloadTargetSpec, UpdateManifest, UpdatePayload},
    releases::{ServiceReleaseError, ServiceReleaseManager},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedArtifact {
    OsImage { path: PathBuf, boot_assets: Vec<StagedBootAsset> },
    PayloadUpdate { staged_targets: Vec<StagedPayloadTarget> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPayloadTarget {
    pub name: String,
    pub revision: String,
    pub staged_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBootAsset {
    pub relative_path: PathBuf,
    pub staged_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedManifest {
    pub artifact_id: String,
    pub artifact: StagedArtifact,
}

#[derive(Debug, Clone)]
pub struct ArtifactStager {
    staging_dir: PathBuf,
    release_manager: ServiceReleaseManager,
    http_client: Arc<OnceCell<reqwest::Client>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ArtifactStageError {
    #[error("unsupported artifact url scheme '{scheme}' for {field}")]
    UnsupportedUrlScheme { field: &'static str, scheme: String },
    #[error("artifact source path does not exist: {path}")]
    MissingSourcePath { path: PathBuf },
    #[error("artifact {artifact_id} size mismatch: expected {expected} bytes, got {actual}")]
    SizeMismatch { artifact_id: String, expected: u64, actual: u64 },
    #[error("artifact {artifact_id} sha256 mismatch")]
    Sha256Mismatch { artifact_id: String },
    #[error("payload target {name} is missing an artifact_path")]
    MissingPayloadTargetPath { name: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Release(#[from] ServiceReleaseError),
}

impl ArtifactStager {
    pub fn from_config(config: &UpdaterConfig) -> Result<Self, ArtifactStageError> {
        Ok(Self {
            staging_dir: config.updater_staging_dir.clone(),
            release_manager: ServiceReleaseManager::new(config.service_releases_dir.clone(), config.service_bin_dir.clone()),
            http_client: Arc::new(OnceCell::new()),
        })
    }

    pub async fn stage_manifest(&self, manifest: &UpdateManifest) -> Result<StagedManifest, ArtifactStageError> {
        fs::create_dir_all(&self.staging_dir)?;

        let artifact = match &manifest.payload {
            UpdatePayload::OsImage(payload) => {
                let artifact_dir = self.staging_dir.join(sanitize_component(&manifest.artifact_id));
                fs::create_dir_all(&artifact_dir)?;
                let staged_path = artifact_dir.join("image");
                self.stage_url_to_path("helios.update.image_url", &payload.image_url, &staged_path).await?;
                validate_file(&manifest.artifact_id, &staged_path, payload.size_bytes, payload.sha256.as_deref())?;
                let mut boot_assets = Vec::with_capacity(payload.boot_assets.len());
                for asset in &payload.boot_assets {
                    boot_assets.push(self.stage_boot_asset(&artifact_dir, asset).await?);
                }
                StagedArtifact::OsImage { path: staged_path, boot_assets }
            }
            UpdatePayload::PayloadUpdate(payload) => {
                let mut staged_targets = Vec::with_capacity(payload.targets.len());
                for target in &payload.targets {
                    staged_targets.push(self.stage_payload_target(target).await?);
                }
                StagedArtifact::PayloadUpdate { staged_targets }
            }
        };

        Ok(StagedManifest { artifact_id: manifest.artifact_id.clone(), artifact })
    }

    async fn stage_payload_target(&self, target: &PayloadTargetSpec) -> Result<StagedPayloadTarget, ArtifactStageError> {
        if target.artifact_path.trim().is_empty() {
            return Err(ArtifactStageError::MissingPayloadTargetPath { name: target.name.clone() });
        }

        let temp_dir = self.staging_dir.join("payloads").join(sanitize_component(&target.revision));
        fs::create_dir_all(&temp_dir)?;
        let temp_path = temp_dir.join(sanitize_component(&target.name));

        self.stage_source_to_path("payload_target.artifact_path", &target.artifact_path, &temp_path).await?;
        let staged_path = self.release_manager.stage(&target.name, &target.revision, &temp_path)?;

        Ok(StagedPayloadTarget { name: target.name.clone(), revision: target.revision.clone(), staged_path })
    }

    async fn stage_boot_asset(&self, artifact_dir: &Path, asset: &BootAssetSpec) -> Result<StagedBootAsset, ArtifactStageError> {
        let relative_path = PathBuf::from(&asset.path);
        let dest = artifact_dir.join("boot").join(&relative_path);
        self.stage_source_to_path("boot_asset.source", &asset.source, &dest).await?;
        validate_file(&asset.path, &dest, asset.size_bytes, asset.sha256.as_deref())?;
        Ok(StagedBootAsset { relative_path, staged_path: dest })
    }

    async fn stage_url_to_path(&self, field: &'static str, source: &str, dest: &Path) -> Result<(), ArtifactStageError> {
        let url = Url::parse(source)?;
        match url.scheme() {
            "file" => {
                let path = url.to_file_path().map_err(|_| ArtifactStageError::UnsupportedUrlScheme { field, scheme: url.scheme().to_string() })?;
                copy_file(&path, dest)?;
                Ok(())
            }
            "http" | "https" => {
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let client = self.http_client().await?;
                let response = client.get(url).send().await?.error_for_status()?;
                let mut file = tokio::fs::File::create(dest).await?;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    file.write_all(&chunk?).await?;
                }
                file.flush().await?;
                Ok(())
            }
            scheme => Err(ArtifactStageError::UnsupportedUrlScheme { field, scheme: scheme.to_string() }),
        }
    }

    async fn stage_source_to_path(&self, field: &'static str, source: &str, dest: &Path) -> Result<(), ArtifactStageError> {
        if source.starts_with("file://") || source.starts_with("http://") || source.starts_with("https://") {
            return self.stage_url_to_path(field, source, dest).await;
        }

        let path = PathBuf::from(source);
        copy_file(&path, dest)?;
        Ok(())
    }

    async fn http_client(&self) -> Result<&reqwest::Client, ArtifactStageError> {
        self.http_client
            .get_or_try_init(|| async {
                let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().map(|cert| Certificate::from_der(cert.as_ref())).collect::<Result<Vec<_>, _>>()?;

                reqwest::Client::builder().use_rustls_tls().tls_certs_only(roots).build()
            })
            .await
            .map_err(ArtifactStageError::Http)
    }
}

fn copy_file(source: &Path, dest: &Path) -> Result<(), ArtifactStageError> {
    if !source.is_file() {
        return Err(ArtifactStageError::MissingSourcePath { path: source.to_path_buf() });
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    if fs::hard_link(source, dest).is_err() {
        fs::copy(source, dest)?;
    }
    Ok(())
}

fn validate_file(artifact_id: &str, path: &Path, expected_size: Option<u64>, expected_sha256: Option<&str>) -> Result<(), ArtifactStageError> {
    let metadata = fs::metadata(path)?;
    if let Some(expected_size) = expected_size {
        let actual = metadata.len();
        if actual != expected_size {
            return Err(ArtifactStageError::SizeMismatch { artifact_id: artifact_id.to_string(), expected: expected_size, actual });
        }
    }
    if let Some(expected_sha256) = expected_sha256 {
        let actual = sha256_hex(path)?;
        if actual != expected_sha256.to_ascii_lowercase() {
            return Err(ArtifactStageError::Sha256Mismatch { artifact_id: artifact_id.to_string() });
        }
    }
    Ok(())
}

fn sha256_hex(path: &Path) -> Result<String, ArtifactStageError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sanitize_component(value: &str) -> String {
    value.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        manifest::{CompatibilityRule, OsImagePayload, PayloadUpdatePayload, UpdatePayload},
        model::{HookPhase, UpdateArtifactClass},
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn stages_os_image_from_file_url() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("helios.img");
        fs::write(&source, b"image-bytes").expect("source");
        let digest = sha256_hex(&source).expect("sha");
        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.os".into(),
            version: "2026.2.0".into(),
            artifact_class: UpdateArtifactClass::OsImage,
            compatibility: CompatibilityRule { requires_ab_rootfs: true, ..CompatibilityRule::default() },
            payload: UpdatePayload::OsImage(OsImagePayload {
                image_url: format!("file://{}", source.display()),
                size_bytes: Some(11),
                sha256: Some(digest),
                inactive_slot_min_bytes: Some(1024),
                boot_assets: vec![],
            }),
            hooks: vec![crate::manifest::ManifestHook { phase: HookPhase::Postboot, command: vec!["/bin/true".into()], timeout_secs: None }],
        };
        let config = UpdaterConfig {
            updater_staging_dir: temp.path().join("staging"),
            service_releases_dir: temp.path().join("releases"),
            service_bin_dir: temp.path().join("bin"),
            ..UpdaterConfig::default()
        };
        let stager = ArtifactStager::from_config(&config).expect("stager");

        let staged = stager.stage_manifest(&manifest).await.expect("staged");
        match staged.artifact {
            StagedArtifact::OsImage { path, boot_assets } => {
                assert!(path.is_file());
                assert_eq!(fs::read(path).expect("read"), b"image-bytes");
                assert!(boot_assets.is_empty());
            }
            _ => panic!("expected os image"),
        }
    }

    #[tokio::test]
    async fn stages_payload_targets_into_release_manager() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("helios-engine");
        fs::write(&source, b"bin").expect("source");
        let manifest = UpdateManifest {
            manifest_version: "v1".into(),
            artifact_id: "artifact.payload".into(),
            version: "rev-1".into(),
            artifact_class: UpdateArtifactClass::PayloadUpdate,
            compatibility: CompatibilityRule { requires_ab_rootfs: false, ..CompatibilityRule::default() },
            payload: UpdatePayload::PayloadUpdate(PayloadUpdatePayload {
                targets: vec![PayloadTargetSpec { name: "helios-engine".into(), revision: "rev-1".into(), artifact_path: source.display().to_string() }],
            }),
            hooks: vec![],
        };
        let config = UpdaterConfig {
            updater_staging_dir: temp.path().join("staging"),
            service_releases_dir: temp.path().join("releases"),
            service_bin_dir: temp.path().join("bin"),
            ..UpdaterConfig::default()
        };
        let stager = ArtifactStager::from_config(&config).expect("stager");

        let staged = stager.stage_manifest(&manifest).await.expect("staged");
        match staged.artifact {
            StagedArtifact::PayloadUpdate { staged_targets } => {
                assert_eq!(staged_targets.len(), 1);
                assert!(staged_targets[0].staged_path.is_file());
                assert_eq!(fs::read(&staged_targets[0].staged_path).expect("read"), b"bin");
            }
            _ => panic!("expected payload update"),
        }
    }
}
