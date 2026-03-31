use chrono::Utc;
use helios_peripherals::{AiModelFormat, AiModelHealth, AiModelId, AiModelMetadata, AiModelTensorMetadata};
use lib_ipc::types::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::Path as StdPath;
use tokio::fs;
use uuid::Uuid;

use crate::{api_tools_client, http::error::ApiError};

use super::{
    support::{load_media_label_bytes, map_io_error},
    types::MediaMetadata,
};

const AI_MODEL_MANIFEST_NAME: &str = "manifest.json";

pub(super) fn looks_like_model(filename: &str, content_type: &str) -> bool {
    if content_type == "application/octet-stream"
        && let Some(ext) = filename.split('.').next_back()
    {
        return matches!(ext.to_lowercase().as_str(), "tflite" | "onnx");
    }
    filename.to_lowercase().ends_with(".tflite") || filename.to_lowercase().ends_with(".onnx")
}

pub(super) fn guess_model_format(filename: &str) -> Option<AiModelFormat> {
    let ext = filename.split('.').next_back()?.to_lowercase();
    match ext.as_str() {
        "tflite" => Some(AiModelFormat::TensorFlowLite),
        "onnx" => Some(AiModelFormat::Onnx),
        _ => None,
    }
}

pub(super) async fn hydrate_model_metadata(meta: &mut MediaMetadata, path: &std::path::Path, format: AiModelFormat) {
    let bytes = match fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let inspection = match api_tools_client::model_inspect(&bytes, format).await {
        Ok(inspection) => inspection,
        Err(_) => return,
    };
    if meta.model_tensor_spec.is_none() {
        meta.model_tensor_spec = Some(format_tensor_spec(&inspection.inputs, &inspection.outputs));
    }
    if meta.model_input_resolution.is_none() {
        meta.model_input_resolution = derive_input_resolution(&inspection.inputs);
    }
    if !inspection.suggested_tags.is_empty() {
        merge_model_affinity_tags(&mut meta.tags, &inspection.suggested_tags);
    }
}

fn format_tensor_spec(inputs: &[AiModelTensorMetadata], outputs: &[AiModelTensorMetadata]) -> String {
    let format_tensors = |prefix: &str, tensors: &[AiModelTensorMetadata]| -> String {
        let entries = tensors
            .iter()
            .enumerate()
            .map(|(idx, tensor)| {
                let name = tensor.name.clone().unwrap_or_else(|| format!("{prefix}{idx}"));
                let shape = tensor.shape.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("x");
                format!("{name}:{shape}")
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{prefix}s:{entries}")
    };
    let input_section = format_tensors("input", inputs);
    let output_section = format_tensors("output", outputs);
    format!("{input_section}|{output_section}")
}

fn derive_input_resolution(inputs: &[AiModelTensorMetadata]) -> Option<String> {
    for tensor in inputs {
        let shape = &tensor.shape;
        if shape.len() == 4 {
            let h = shape[1];
            let w = shape[2];
            if h > 0 && w > 0 {
                return Some(format!("{w}×{h}"));
            }
        } else if shape.len() == 3 {
            let h = shape[0];
            let w = shape[1];
            if h > 0 && w > 0 {
                return Some(format!("{w}×{h}"));
            }
        }
    }
    None
}

fn merge_model_affinity_tags(existing: &mut Vec<String>, suggested: &[String]) {
    for tag in suggested {
        if !is_model_affinity_tag(tag) {
            continue;
        }
        if existing.iter().any(|value| value.eq_ignore_ascii_case(tag)) {
            continue;
        }
        existing.push(tag.clone());
    }
}

fn is_model_affinity_tag(tag: &str) -> bool {
    let normalized = tag.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    if normalized.starts_with("runtime:") || normalized.starts_with("precision:") {
        return true;
    }
    matches!(normalized.as_str(), "quantized" | "edge-tpu" | "edgetpu" | "requires-edge-tpu")
}

pub(super) fn needs_model_affinity_tags(tags: &[String]) -> bool {
    !tags.iter().any(|tag| is_model_affinity_tag(tag))
}

fn parse_label_bytes(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = serde_json::from_str::<AiModelMetadata>(trimmed)
        && !parsed.labels.is_empty()
    {
        return Some(parsed.labels);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Ok(meta) = serde_json::from_value::<AiModelMetadata>(value.clone())
            && !meta.labels.is_empty()
        {
            return Some(meta.labels);
        }
        if let Some(array) = value.get("labels").and_then(|v| v.as_array()) {
            let labels: Vec<String> = array.iter().filter_map(|entry| entry.as_str().map(|s| s.to_string())).collect();
            if !labels.is_empty() {
                return Some(labels);
            }
        }
        if let Some(array) = value.as_array() {
            let labels: Vec<String> = array.iter().filter_map(|entry| entry.as_str().map(|s| s.to_string())).collect();
            if !labels.is_empty() {
                return Some(labels);
            }
        }
    }

    let labels: Vec<String> = trimmed.lines().map(|line| line.trim()).filter(|line| !line.is_empty()).map(|line| line.to_string()).collect();
    if labels.is_empty() { None } else { Some(labels) }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AiModelManifest {
    #[serde(default)]
    models: Vec<AiModelManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiModelManifestEntry {
    id: AiModelId,
    format: AiModelFormat,
    metadata: AiModelMetadata,
    artifact: String,
    #[serde(default)]
    label_artifact: Option<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    created_at: Timestamp,
    #[serde(default)]
    health: AiModelHealth,
}

pub(super) async fn register_ai_model_for_media(filename: &str, path: &StdPath, content_type: &str, meta: &mut MediaMetadata) -> Result<(), ApiError> {
    if !looks_like_model(filename, content_type) {
        return Ok(());
    }

    let Some(format) = guess_model_format(filename) else {
        return Ok(());
    };

    let model_dir = ai_model_dir();
    fs::create_dir_all(&model_dir).await.map_err(|err| map_io_error(err, "failed to prepare AI model directory"))?;
    let manifest_path = model_dir.join(AI_MODEL_MANIFEST_NAME);

    let mut manifest = load_ai_model_manifest(&manifest_path).await?;
    let model_uuid = meta.model_id.unwrap_or_else(Uuid::new_v4);
    let model_id = AiModelId(model_uuid);

    let label_bytes = load_media_label_bytes(filename).await?;
    let labels = label_bytes.as_deref().and_then(parse_label_bytes);

    if let Some(existing_idx) = manifest.models.iter().position(|entry| entry.id == model_id) {
        if manifest.models[existing_idx].label_artifact.is_none() {
            if let Some(labels) = labels.clone() {
                manifest.models[existing_idx].metadata.labels = labels;
            }
            if let Some(label_bytes) = label_bytes.as_ref() {
                let label_name = format!("{}.labels.json", model_id.0);
                let label_path = model_dir.join(&label_name);
                if !fs::try_exists(&label_path).await.unwrap_or(false) {
                    fs::write(&label_path, label_bytes).await.map_err(|err| map_io_error(err, "failed to store AI model label artifact"))?;
                }
                manifest.models[existing_idx].label_artifact = Some(label_name);
                save_ai_model_manifest(&manifest_path, &manifest).await?;
            }
        }
        meta.model_id = Some(model_uuid);
        return Ok(());
    }

    let artifact = artifact_name(&model_id, &format);
    let artifact_path = model_dir.join(&artifact);
    if !fs::try_exists(&artifact_path).await.unwrap_or(false) {
        fs::copy(path, &artifact_path).await.map_err(|err| map_io_error(err, "failed to store AI model artifact"))?;
    }

    let bytes = fs::read(path).await.map_err(|err| map_io_error(err, "failed to read AI model payload"))?;
    let inspection = api_tools_client::model_inspect(&bytes, format.clone()).await?;

    let mut metadata = AiModelMetadata { display_name: Some(model_display_name(filename)), inputs: inspection.inputs, outputs: inspection.outputs, ..Default::default() };
    if !inspection.suggested_tags.is_empty() {
        metadata.tags = inspection.suggested_tags;
    }
    if let Some(labels) = labels {
        metadata.labels = labels;
    }

    let label_artifact = if let Some(label_bytes) = label_bytes.as_ref() {
        let label_name = format!("{}.labels.json", model_id.0);
        let label_path = model_dir.join(&label_name);
        fs::write(&label_path, label_bytes).await.map_err(|err| map_io_error(err, "failed to store AI model label artifact"))?;
        Some(label_name)
    } else {
        None
    };

    let created_at = Utc::now();
    let health = AiModelHealth::ready(created_at);
    manifest.models.push(AiModelManifestEntry { id: model_id, format, metadata, artifact, label_artifact, created_at, health });

    save_ai_model_manifest(&manifest_path, &manifest).await?;
    meta.model_id = Some(model_uuid);
    Ok(())
}

pub(super) async fn remove_ai_model(model_id: Uuid) -> Result<(), ApiError> {
    let model_dir = ai_model_dir();
    let manifest_path = model_dir.join(AI_MODEL_MANIFEST_NAME);
    let mut manifest = load_ai_model_manifest(&manifest_path).await?;
    let target = AiModelId(model_id);

    if let Some(idx) = manifest.models.iter().position(|entry| entry.id == target) {
        let entry = manifest.models.remove(idx);
        let artifact_path = model_dir.join(&entry.artifact);
        let _ = fs::remove_file(&artifact_path).await;
        if let Some(label_artifact) = entry.label_artifact {
            let _ = fs::remove_file(model_dir.join(label_artifact)).await;
        }
        save_ai_model_manifest(&manifest_path, &manifest).await?;
    }

    Ok(())
}

async fn load_ai_model_manifest(path: &StdPath) -> Result<AiModelManifest, ApiError> {
    match fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| ApiError::internal(format!("invalid AI model manifest: {err}"))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AiModelManifest::default()),
        Err(err) => Err(map_io_error(err, "failed to read AI model manifest")),
    }
}

async fn save_ai_model_manifest(path: &StdPath, manifest: &AiModelManifest) -> Result<(), ApiError> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|err| ApiError::internal(format!("failed to serialize AI model manifest: {err}")))?;
    fs::write(path, bytes).await.map_err(|err| map_io_error(err, "failed to write AI model manifest"))?;
    Ok(())
}

fn artifact_name(id: &AiModelId, format: &AiModelFormat) -> String {
    let extension = match format {
        AiModelFormat::TensorFlowLite => "tflite",
        AiModelFormat::Onnx => "onnx",
        AiModelFormat::Raw => "bin",
    };
    format!("{}.{}", id.0, extension)
}

fn model_display_name(filename: &str) -> String {
    StdPath::new(filename).file_stem().and_then(|stem| stem.to_str()).map(|value| value.trim()).filter(|value| !value.is_empty()).unwrap_or(filename).to_string()
}

fn ai_model_dir() -> std::path::PathBuf {
    if let Some(value) = std::env::var_os("PERIPHERALS_AI_MODEL_DIR").filter(|value| !value.is_empty()) {
        return value.into();
    }
    if let Some(value) = std::env::var_os("SENSORS_AI_MODEL_DIR").filter(|value| !value.is_empty()) {
        return value.into();
    }
    if let Some(value) = std::env::var_os("PERIPHERALS_STATE_DIR").filter(|value| !value.is_empty()) {
        return StdPath::new(&value).join("ai-models");
    }
    if let Some(value) = std::env::var_os("SENSORS_STATE_DIR").filter(|value| !value.is_empty()) {
        return StdPath::new(&value).join("ai-models");
    }
    StdPath::new("/var/lib/helios/ai-models").to_path_buf()
}
