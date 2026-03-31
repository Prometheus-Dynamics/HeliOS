use helios_engine::ipc::PluginCompatibility;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginFile {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginListResponse {
    pub installed: Vec<PluginFile>,
    pub disabled: Vec<PluginFile>,
    pub uploads: Vec<PluginFile>,
    pub engine_available: bool,
    pub compatibility: Vec<PluginCompatibility>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginUploadResponse {
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PluginInstallRequest {
    #[serde(default)]
    pub upload_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginInstallResponse {
    pub name: String,
    pub installed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginToggleResponse {
    pub name: String,
    pub enabled: bool,
}
