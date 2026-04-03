use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadUpdateResponse {
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub image_url: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadUpdateError {
    pub error: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct StageUpdateRequest {
    pub image_url: String,
    #[serde(default)]
    pub artifact_kind: Option<ApplyArtifactKind>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default = "default_true")]
    pub delete_image_after_apply: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateAckResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_id: Option<String>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyUpdateRequest {
    #[serde(default)]
    pub requested_by: Option<String>,
    #[serde(default)]
    pub update_id: Option<String>,
    #[serde(default)]
    pub artifact_kind: Option<ApplyArtifactKind>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default = "default_true")]
    pub delete_image_after_apply: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PreflightUpdateRequest {
    #[serde(default)]
    pub update_id: Option<String>,
    #[serde(default)]
    pub artifact_kind: Option<ApplyArtifactKind>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApplyArtifactKind {
    DiskImage,
    FrontendBundle,
    ServiceBundle,
}

impl ApplyArtifactKind {
    pub fn manifest_kind(self) -> &'static str {
        match self {
            Self::DiskImage => "disk-image",
            Self::FrontendBundle => "frontend-bundle",
            Self::ServiceBundle => "service-bundle",
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CancelUpdateRequest {
    #[serde(default)]
    pub update_id: Option<String>,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateStateResponse {
    #[serde(default)]
    pub state: Option<Value>,
    pub cache_usage_bytes: u64,
    pub storage: OtaStorageReport,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtaStorageReport {
    pub uploads: OtaUploadStorageReport,
    pub cache: OtaDirectoryStorageReport,
    pub work: OtaDirectoryStorageReport,
    pub service_releases: OtaDirectoryStorageReport,
    pub frontend_releases: OtaDirectoryStorageReport,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtaUploadStorageReport {
    pub path: String,
    pub usage_bytes: u64,
    pub quota_bytes: u64,
    pub available_bytes: u64,
    pub remaining_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtaDirectoryStorageReport {
    pub path: String,
    #[serde(default)]
    pub usage_bytes: u64,
    #[serde(default)]
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PreflightUpdateResponse {
    pub update_id: String,
    pub ready: bool,
    #[serde(default)]
    pub transient: bool,
    pub report: PreflightReport,
}

#[derive(Debug, Serialize)]
pub struct ApplyConflictResponse {
    pub update_id: String,
    pub error: String,
    pub preflight: PreflightReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreflightRequestMode<'a> {
    Active,
    Existing(Uuid),
    Transient { image_url: &'a str, artifact_kind: ApplyArtifactKind, size_bytes: Option<u64>, checksum: Option<&'a str> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApplyRequestMode<'a> {
    Existing(Uuid),
    Transient { image_url: &'a str, artifact_kind: ApplyArtifactKind, size_bytes: Option<u64>, checksum: Option<&'a str>, delete_image_after_apply: bool },
}

pub(super) const fn default_true() -> bool {
    true
}

impl IntoResponse for UploadUpdateError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::SERVICE_UNAVAILABLE, Json(self)).into_response()
    }
}
