use crate::BoundedU64Policy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiUploadPolicy {
    pub media_upload_mb: BoundedU64Policy,
    pub ota_upload_mb: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedApiUploadPolicy {
    pub media_upload_bytes: u64,
    pub ota_upload_bytes: u64,
}

impl ApiUploadPolicy {
    pub fn resolve(self) -> ResolvedApiUploadPolicy {
        ResolvedApiUploadPolicy { media_upload_bytes: self.media_upload_mb.resolve().saturating_mul(1024 * 1024), ota_upload_bytes: self.ota_upload_mb.resolve().saturating_mul(1024 * 1024) }
    }
}

pub const HELIOS_API_UPLOAD_POLICY: ApiUploadPolicy = ApiUploadPolicy {
    media_upload_mb: BoundedU64Policy { env_var: "HELIOS_API_MAX_UPLOAD_MB", default: 512, min: 1, max: 16 * 1024 },
    ota_upload_mb: BoundedU64Policy { env_var: "HELIOS_OTA_MAX_UPLOAD_MB", default: 2 * 1024, min: 1, max: 64 * 1024 },
};
