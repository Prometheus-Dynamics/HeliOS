use std::path::PathBuf;

use daedalus::data::model::Value as DaedalusValue;
use serde_json::Value as JsonValue;
use tokio::fs;
use uuid::Uuid;

use crate::http::pipelines::PipelineDocument;
use crate::http::storage;

pub(crate) async fn load_pipeline_doc(pipeline_id: Uuid) -> Result<PipelineDocument, String> {
    let path = pipeline_path(pipeline_id).await.map_err(|err| format!("failed to resolve pipeline path: {err}"))?;
    let data = fs::read(&path).await.map_err(|err| format!("failed to read pipeline: {err}"))?;
    serde_json::from_slice::<PipelineDocument>(&data).map_err(|err| format!("failed to decode pipeline: {err}"))
}

pub(crate) async fn save_pipeline_doc(pipeline_id: Uuid, doc: &PipelineDocument) -> Result<(), String> {
    let path = pipeline_path(pipeline_id).await.map_err(|err| format!("failed to resolve pipeline path: {err}"))?;
    let data = serde_json::to_vec_pretty(doc).map_err(|err| format!("failed to encode pipeline: {err}"))?;
    fs::write(&path, data).await.map_err(|err| format!("failed to write pipeline: {err}"))
}

pub(crate) async fn pipeline_path(pipeline_id: Uuid) -> std::io::Result<PathBuf> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    Ok(dir.join(format!("{pipeline_id}.json")))
}

pub(super) fn encode_daedalus_value(value: &JsonValue) -> DaedalusValue {
    match value {
        JsonValue::Null => DaedalusValue::Unit,
        JsonValue::Bool(b) => DaedalusValue::Bool(*b),
        JsonValue::Number(num) => {
            if let Some(i) = num.as_i64() {
                DaedalusValue::Int(i)
            } else if let Some(f) = num.as_f64() {
                DaedalusValue::Float(f)
            } else {
                DaedalusValue::Unit
            }
        }
        JsonValue::String(s) => DaedalusValue::String(s.to_string().into()),
        JsonValue::Array(items) => DaedalusValue::List(items.iter().map(encode_daedalus_value).collect()),
        JsonValue::Object(map) => {
            let fields = map
                .iter()
                .filter(|(key, _)| !key.trim().is_empty())
                .map(|(name, value)| daedalus::data::model::StructFieldValue { name: name.to_string(), value: encode_daedalus_value(value) })
                .collect();
            DaedalusValue::Struct(fields)
        }
    }
}
