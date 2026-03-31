use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IdeInfo {
    pub enabled: bool,
    pub url: Option<String>,
    pub port: Option<u16>,
    pub workspace_dir: Option<String>,
    pub artifacts_dir: Option<String>,
    pub projects_dir: Option<String>,
    pub tools_dir: Option<String>,
    pub sdk_dir: Option<String>,
    pub bind_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IdeProjectEntry {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IdeProjectsResponse {
    pub projects: Vec<IdeProjectEntry>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateIdeProjectRequest {
    pub name: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CreateIdeProjectResponse {
    pub name: String,
    pub path: String,
}
