use std::collections::HashMap;

use tokio::sync::RwLock;

use helios_engine::localization::external::{ExternalLocalizationSample, ExternalLocalizationSampleRequest, ExternalLocalizationSource, ExternalLocalizationSourceUpsert};
use helios_engine::localization::types::PipelineOutputSample;

#[derive(Debug, Clone)]
struct ExternalLocalizationEntry {
    meta: ExternalLocalizationSource,
    sample: Option<PipelineOutputSample>,
}

#[derive(Default)]
pub(crate) struct LocalizationExternalSourceRegistry {
    sources: RwLock<HashMap<String, ExternalLocalizationEntry>>,
}

impl LocalizationExternalSourceRegistry {
    pub(crate) async fn list_sources(&self) -> Vec<ExternalLocalizationSource> {
        let sources = self.sources.read().await;
        sources.values().map(|entry| entry.meta.clone()).collect()
    }

    pub(crate) async fn upsert_source(&self, id: &str, payload: ExternalLocalizationSourceUpsert) -> Result<ExternalLocalizationSource, String> {
        let output_key = payload.output_key.trim().to_string();
        if output_key.is_empty() {
            return Err("output_key is required".to_string());
        }

        let mut sources = self.sources.write().await;
        let entry = sources.entry(id.to_string()).or_insert_with(|| ExternalLocalizationEntry {
            meta: ExternalLocalizationSource {
                id: id.to_string(),
                label: id.to_string(),
                output_key: output_key.clone(),
                camera_uid: None,
                camera_path: None,
                pipeline_label: Some("External".to_string()),
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

    pub(crate) async fn delete_source(&self, id: &str) {
        let mut sources = self.sources.write().await;
        sources.remove(id);
    }

    pub(crate) async fn update_sample(&self, id: &str, payload: ExternalLocalizationSampleRequest) -> Result<ExternalLocalizationSample, String> {
        let mut sources = self.sources.write().await;
        let entry = sources.entry(id.to_string()).or_insert_with(|| ExternalLocalizationEntry {
            meta: ExternalLocalizationSource {
                id: id.to_string(),
                label: id.to_string(),
                output_key: payload.output_key.clone().unwrap_or_else(|| "pose".to_string()),
                camera_uid: None,
                camera_path: None,
                pipeline_label: Some("External".to_string()),
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

    pub(crate) async fn fetch_sample(&self, source_id: &str, output_key: &str) -> Result<PipelineOutputSample, String> {
        let sources = self.sources.read().await;
        let entry = sources.get(source_id).ok_or_else(|| "external source not found".to_string())?;
        if entry.meta.output_key != output_key {
            return Err("unsupported output key".to_string());
        }
        entry.sample.clone().ok_or_else(|| "no sample available".to_string())
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|dur| dur.as_millis() as u64).unwrap_or(0)
}
