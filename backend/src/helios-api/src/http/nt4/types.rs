use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct Nt4TopicsRequest {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub scan_ms: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4TopicInfo {
    pub name: String,
    pub data_type: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub properties: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4TopicsResponse {
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub topics: Vec<Nt4TopicInfo>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct Nt4ValueRequest {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    pub topic: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Nt4ValueResponse {
    pub host: String,
    pub port: u16,
    pub topic: String,
    #[serde(default)]
    pub data_type: Option<String>,
    pub value: serde_json::Value,
}
