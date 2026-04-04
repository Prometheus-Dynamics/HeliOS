use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLocalizationSource {
    pub id: String,
    pub label: String,
    pub output_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sample_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLocalizationSourceUpsert {
    #[serde(default)]
    pub label: Option<String>,
    pub output_key: String,
    #[serde(default)]
    pub camera_uid: Option<String>,
    #[serde(default)]
    pub camera_path: Option<String>,
    #[serde(default)]
    pub pipeline_label: Option<String>,
    #[serde(default)]
    pub data_type: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLocalizationSampleRequest {
    #[serde(default)]
    pub output_key: Option<String>,
    #[serde(default)]
    pub data_type: Option<serde_json::Value>,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLocalizationSample {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<serde_json::Value>,
    pub value: serde_json::Value,
    pub updated_at_ms: u64,
}
