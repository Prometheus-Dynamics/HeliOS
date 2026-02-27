use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use utoipa::ToSchema;

use super::types::PipelineOutputSample;

const DEFAULT_PIPELINE_LABEL: &str = "External";

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

#[derive(Debug, Clone)]
struct ExternalLocalizationEntry {
    meta: ExternalLocalizationSource,
    sample: Option<PipelineOutputSample>,
}

static EXTERNAL_SOURCES: OnceLock<RwLock<HashMap<String, ExternalLocalizationEntry>>> = OnceLock::new();

fn external_sources() -> &'static RwLock<HashMap<String, ExternalLocalizationEntry>> {
    EXTERNAL_SOURCES.get_or_init(|| RwLock::new(HashMap::new()))
}

pub async fn list_sources() -> Vec<ExternalLocalizationSource> {
    let sources = external_sources().read().await;
    sources.values().map(|entry| entry.meta.clone()).collect()
}

pub async fn upsert_source(id: &str, payload: ExternalLocalizationSourceUpsert) -> Result<ExternalLocalizationSource, String> {
    let output_key = payload.output_key.trim().to_string();
    if output_key.is_empty() {
        return Err("output_key is required".to_string());
    }
    let mut sources = external_sources().write().await;
    let entry = sources.entry(id.to_string()).or_insert_with(|| ExternalLocalizationEntry {
        meta: ExternalLocalizationSource {
            id: id.to_string(),
            label: id.to_string(),
            output_key: output_key.clone(),
            camera_uid: None,
            camera_path: None,
            pipeline_label: Some(DEFAULT_PIPELINE_LABEL.to_string()),
            data_type: None,
            last_sample_ms: None,
        },
        sample: None,
    });

    entry.meta.label = payload.label.clone().unwrap_or_else(|| entry.meta.label.clone());
    entry.meta.output_key = output_key;
    entry.meta.camera_uid = payload.camera_uid.clone();
    entry.meta.camera_path = payload.camera_path.clone();
    entry.meta.pipeline_label = payload.pipeline_label.clone().or_else(|| entry.meta.pipeline_label.clone());
    if payload.data_type.is_some() {
        entry.meta.data_type = payload.data_type.clone();
    }

    Ok(entry.meta.clone())
}

pub async fn delete_source(id: &str) {
    let mut sources = external_sources().write().await;
    sources.remove(id);
}

pub async fn update_sample(id: &str, payload: ExternalLocalizationSampleRequest) -> Result<ExternalLocalizationSample, String> {
    let mut sources = external_sources().write().await;
    let entry = sources.entry(id.to_string()).or_insert_with(|| ExternalLocalizationEntry {
        meta: ExternalLocalizationSource {
            id: id.to_string(),
            label: id.to_string(),
            output_key: payload.output_key.clone().unwrap_or_else(|| "pose".to_string()),
            camera_uid: None,
            camera_path: None,
            pipeline_label: Some(DEFAULT_PIPELINE_LABEL.to_string()),
            data_type: None,
            last_sample_ms: None,
        },
        sample: None,
    });

    let output_key = payload.output_key.clone().unwrap_or_else(|| entry.meta.output_key.clone()).trim().to_string();
    if output_key.is_empty() {
        return Err("output_key is required".to_string());
    }
    entry.meta.output_key = output_key;

    if payload.data_type.is_some() {
        entry.meta.data_type = payload.data_type.clone();
    }
    let data_type = payload.data_type.clone().or_else(|| entry.meta.data_type.clone());
    let sample = PipelineOutputSample { data_type, value: payload.value.clone() };
    entry.sample = Some(sample.clone());

    let updated_at_ms = now_ms();
    entry.meta.last_sample_ms = Some(updated_at_ms);

    Ok(ExternalLocalizationSample { data_type: sample.data_type, value: sample.value, updated_at_ms })
}

pub async fn fetch_sample(source_id: &str, output_key: &str) -> Result<PipelineOutputSample, String> {
    let sources = external_sources().read().await;
    let entry = sources.get(source_id).ok_or_else(|| "external source not found".to_string())?;
    if entry.meta.output_key != output_key {
        return Err("unsupported output key".to_string());
    }
    entry.sample.clone().ok_or_else(|| "no sample available".to_string())
}

pub async fn fetch_value(source_id: &str, output_key: &str) -> Result<serde_json::Value, String> {
    let sample = fetch_sample(source_id, output_key).await?;
    Ok(sample.value)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|dur| dur.as_millis() as u64).unwrap_or(0)
}
