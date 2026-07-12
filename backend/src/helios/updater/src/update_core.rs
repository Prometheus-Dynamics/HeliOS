use crate::client::UpdaterClientConfig;
use std::fmt::{Display, Formatter};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateArtifactKind {
    DiskImage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualUpdateArtifact {
    pub image_url: Url,
    pub kind: UpdateArtifactKind,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
    pub delete_source_after_apply: bool,
    pub source_artifact_path: Option<String>,
}

impl ManualUpdateArtifact {
    pub fn new(image_url: Url, kind: UpdateArtifactKind) -> Self {
        Self { image_url, kind, size_bytes: None, checksum: None, delete_source_after_apply: false, source_artifact_path: None }
    }

    pub fn with_size_bytes(mut self, size_bytes: Option<u64>) -> Self {
        self.size_bytes = size_bytes;
        self
    }

    pub fn with_checksum(mut self, checksum: Option<String>) -> Self {
        self.checksum = checksum;
        self
    }

    pub fn with_delete_source_after_apply(mut self, delete_source_after_apply: bool) -> Self {
        self.delete_source_after_apply = delete_source_after_apply;
        self
    }

    pub fn with_source_artifact_path(mut self, source_artifact_path: Option<String>) -> Self {
        self.source_artifact_path = source_artifact_path;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpdateSource<'a> {
    Manual(&'a ManualUpdateArtifact),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePreflight {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedUpdate {
    pub update_id: Uuid,
    pub preflight: UpdatePreflight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedApply {
    Ready(PreparedUpdate),
    Blocked(PreparedUpdate),
}

#[derive(Debug, Clone)]
pub struct IpcUpdateCoreBackend {
    _config: UpdaterClientConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCoreError {
    message: String,
}

impl UpdateCoreError {
    fn blocked(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl Display for UpdateCoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UpdateCoreError {}

impl IpcUpdateCoreBackend {
    pub fn new(config: UpdaterClientConfig) -> Result<Self, UpdateCoreError> {
        Ok(Self { _config: config })
    }
}

pub async fn prepare_update_for_apply(_backend: &IpcUpdateCoreBackend, source: UpdateSource<'_>) -> Result<PreparedApply, UpdateCoreError> {
    let summary = match source {
        UpdateSource::Manual(artifact) => {
            format!("ota apply is not enabled yet for source '{}'", artifact.image_url)
        }
    };
    Ok(PreparedApply::Blocked(PreparedUpdate { update_id: Uuid::new_v4(), preflight: UpdatePreflight { summary } }))
}

pub async fn apply_prepared_update(_backend: &IpcUpdateCoreBackend, _update_id: Uuid) -> Result<(), UpdateCoreError> {
    Err(UpdateCoreError::blocked("ota apply is not enabled yet"))
}
