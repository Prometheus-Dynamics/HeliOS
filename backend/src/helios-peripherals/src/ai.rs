use crate::dto::{AiModelDescriptor, AiModelFormat, AiModelHealth, AiModelHealthStatus, AiModelId, AiModelMetadata, AiModelTensorMetadata, AiModelUpload, AiTensorElementType, AiTensorQuantization};
use crate::error::{Error, Result};
use chrono::Utc;
use lib_ai::backend::AiBackend;
use lib_ai::backend::{coral::CoralBackend, tflite::TfliteBackend};
use lib_ai::model::{
    ModelLoadRequest, ModelSource,
    introspect::{ModelInspection, inspect_model},
};
use lib_ai::registry::Registry;
use lib_ai::runtime::Runtime;
use lib_ipc::types::Timestamp;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::str;
use tokio::sync::{OnceCell, RwLock};
use tracing::{info, warn};

const MANIFEST_NAME: &str = "manifest.json";

struct AiModelEntry {
    descriptor: AiModelDescriptor,
    artifact: String,
    label_artifact: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct AiModelManifest {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    models: Vec<AiModelManifestEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

pub struct AiModelManager {
    registry: Registry<dyn AiBackend>,
    storage_dir: PathBuf,
    manifest_path: PathBuf,
    initialized: OnceCell<()>,
    models: RwLock<HashMap<AiModelId, AiModelEntry>>,
}

impl AiModelManager {
    pub fn with_default_path() -> Self {
        Self::new(resolve_storage_dir())
    }

    pub fn new(storage_dir: PathBuf) -> Self {
        if let Err(err) = fs::create_dir_all(&storage_dir) {
            warn!(path = %storage_dir.display(), %err, "failed to create AI model directory");
        }

        let manifest_path = storage_dir.join(MANIFEST_NAME);
        let registry = build_registry();

        Self { registry, storage_dir, manifest_path, initialized: OnceCell::new(), models: RwLock::new(HashMap::new()) }
    }

    pub async fn list_models(&self) -> Result<Vec<AiModelDescriptor>> {
        self.ensure_initialized().await?;
        let models = self.models.read().await;
        Ok(models.values().map(|entry| entry.descriptor.clone()).collect())
    }

    pub async fn upload_model(&self, upload: AiModelUpload) -> Result<AiModelDescriptor> {
        self.ensure_initialized().await?;

        if upload.bytes.is_empty() {
            return Err(Error::InvalidState("model upload payload must contain bytes".into()));
        }

        let AiModelUpload { id, format, mut metadata, bytes, label_bytes } = upload;

        let id = id.unwrap_or_else(AiModelId::new);
        let artifact = artifact_name(&id, &format);
        let path = self.storage_dir.join(&artifact);
        fs::write(&path, &bytes)?;

        let lib_format = to_lib_model_format(&format);
        let inspection = inspect_model(&bytes, &lib_format);
        Self::hydrate_metadata(&mut metadata, &inspection);

        let label_artifact = if let Some(label_bytes) = label_bytes {
            let label_name = format!("{}.labels.json", id.0);
            let label_path = self.storage_dir.join(&label_name);
            fs::write(&label_path, &label_bytes)?;
            if let Err(err) = Self::hydrate_metadata_from_label_bytes(&mut metadata, &label_bytes) {
                warn!(model_id = %id.0, error = %err, "failed to parse AI model label metadata");
            }
            Some(label_name)
        } else {
            None
        };

        let request = ModelLoadRequest { id: to_lib_model_id(&id), format: lib_format.clone(), source: ModelSource::File(path.clone()), metadata: to_lib_model_metadata(&metadata) };

        let runtime = Runtime::new(&self.registry);
        let loaded = runtime.load_model(&request).await?;

        let created_at = Utc::now();
        let health = AiModelHealth::ready(created_at);
        let descriptor = AiModelDescriptor { id: id.clone(), format, metadata, backend: Some(loaded.backend_name.clone()), health: health.clone(), created_at };

        let mut models = self.models.write().await;
        models.insert(id.clone(), AiModelEntry { descriptor: descriptor.clone(), artifact, label_artifact });
        self.save_manifest_locked(&models)?;

        info!(model_id = %descriptor.id.0, backend = descriptor.backend.as_deref().unwrap_or_default(), "registered AI model");

        Ok(descriptor)
    }

    pub async fn delete_model(&self, model_id: &AiModelId) -> Result<bool> {
        self.ensure_initialized().await?;
        let mut models = self.models.write().await;
        if let Some(entry) = models.remove(model_id) {
            let path = self.storage_dir.join(&entry.artifact);
            match fs::remove_file(&path) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    warn!(%err, model_id = %model_id.0, path = %path.display(), "failed to remove AI model artifact");
                }
                _ => {}
            }
            if let Some(label_artifact) = entry.label_artifact {
                let label_path = self.storage_dir.join(&label_artifact);
                match fs::remove_file(&label_path) {
                    Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                        warn!(%err, model_id = %model_id.0, path = %label_path.display(), "failed to remove AI model label artifact");
                    }
                    _ => {}
                }
            }
            self.save_manifest_locked(&models)?;
            info!(model_id = %model_id.0, "deleted AI model");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn ensure_initialized(&self) -> Result<()> {
        self.initialized.get_or_try_init(|| async { self.reload_from_manifest().await }).await?;
        Ok(())
    }

    async fn reload_from_manifest(&self) -> Result<()> {
        let manifest = self.read_manifest()?;
        if manifest.models.is_empty() {
            return Ok(());
        }

        let runtime = Runtime::new(&self.registry);
        let mut restored = HashMap::new();

        for record in manifest.models {
            let path = self.storage_dir.join(&record.artifact);
            if !path.exists() {
                warn!(artifact = %record.artifact, model_id = %record.id.0, "skipping missing AI model artifact");
                continue;
            }

            let request =
                ModelLoadRequest { id: to_lib_model_id(&record.id), format: to_lib_model_format(&record.format), source: ModelSource::File(path), metadata: to_lib_model_metadata(&record.metadata) };
            match runtime.load_model(&request).await {
                Ok(loaded) => {
                    let mut descriptor = AiModelDescriptor {
                        id: record.id.clone(),
                        format: record.format.clone(),
                        metadata: record.metadata.clone(),
                        backend: Some(loaded.backend_name.clone()),
                        health: record.health.clone(),
                        created_at: record.created_at,
                    };
                    if matches!(descriptor.health.status, AiModelHealthStatus::Unknown) {
                        descriptor.health.status = AiModelHealthStatus::Ready;
                        descriptor.health.last_checked_at = Some(Utc::now());
                    }
                    if let Some(label_artifact) = record.label_artifact.clone() {
                        let label_path = self.storage_dir.join(&label_artifact);
                        match fs::read(&label_path) {
                            Ok(bytes) => {
                                if let Err(err) = Self::hydrate_metadata_from_label_bytes(&mut descriptor.metadata, &bytes) {
                                    warn!(model_id = %record.id.0, artifact = %label_artifact, error = %err, "failed to hydrate label metadata during reload");
                                }
                            }
                            Err(err) => warn!(model_id = %record.id.0, artifact = %label_artifact, %err, "failed to read model label metadata"),
                        }
                        restored.insert(record.id.clone(), AiModelEntry { descriptor: descriptor.clone(), artifact: record.artifact.clone(), label_artifact: Some(label_artifact) });
                    } else {
                        restored.insert(record.id.clone(), AiModelEntry { descriptor: descriptor.clone(), artifact: record.artifact.clone(), label_artifact: None });
                    }
                }
                Err(err) => {
                    warn!(model_id = %record.id.0, artifact = %record.artifact, %err, "failed to restore AI model");
                }
            }
        }

        let mut models = self.models.write().await;
        *models = restored;
        self.save_manifest_locked(&models)?;
        Ok(())
    }

    fn hydrate_metadata(metadata: &mut AiModelMetadata, inspection: &ModelInspection) {
        if metadata.tags.is_empty() && !inspection.suggested_tags.is_empty() {
            metadata.tags = inspection.suggested_tags.clone();
        }
        if metadata.inputs.is_empty() && !inspection.inputs.is_empty() {
            metadata.inputs = inspection.inputs.iter().cloned().map(ai_model_tensor_metadata_from_lib).collect();
        }
        if metadata.outputs.is_empty() && !inspection.outputs.is_empty() {
            metadata.outputs = inspection.outputs.iter().cloned().map(ai_model_tensor_metadata_from_lib).collect();
        }
    }

    fn hydrate_metadata_from_label_bytes(metadata: &mut AiModelMetadata, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let text = str::from_utf8(bytes).map_err(|err| Error::InvalidState(format!("label file is not valid UTF-8: {err}")))?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        if let Ok(parsed) = serde_json::from_str::<AiModelMetadata>(trimmed) {
            Self::merge_metadata(metadata, parsed);
            return Ok(());
        }
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Ok(meta) = serde_json::from_value::<AiModelMetadata>(value.clone()) {
                Self::merge_metadata(metadata, meta);
                return Ok(());
            }
            if let Some(labels_value) = value.get("labels")
                && let Some(array) = labels_value.as_array()
            {
                let labels: Vec<String> = array.iter().filter_map(|entry| entry.as_str().map(|s| s.to_string())).collect();
                if !labels.is_empty() {
                    Self::merge_metadata(metadata, AiModelMetadata { labels, ..AiModelMetadata::default() });
                }
                return Ok(());
            }
            if let Some(array) = value.as_array() {
                let labels: Vec<String> = array.iter().filter_map(|entry| entry.as_str().map(|s| s.to_string())).collect();
                if !labels.is_empty() {
                    Self::merge_metadata(metadata, AiModelMetadata { labels, ..AiModelMetadata::default() });
                }
                return Ok(());
            }
        }
        let labels: Vec<String> = trimmed.lines().map(|line| line.trim()).filter(|line| !line.is_empty()).map(|line| line.to_string()).collect();
        if !labels.is_empty() {
            Self::merge_metadata(metadata, AiModelMetadata { labels, ..AiModelMetadata::default() });
        }
        Ok(())
    }

    fn merge_metadata(target: &mut AiModelMetadata, incoming: AiModelMetadata) {
        if target.display_name.is_none() && incoming.display_name.is_some() {
            target.display_name = incoming.display_name;
        }
        if target.description.is_none() && incoming.description.is_some() {
            target.description = incoming.description;
        }
        if target.tags.is_empty() && !incoming.tags.is_empty() {
            target.tags = incoming.tags;
        }
        if target.preferred_batch_size.is_none() && incoming.preferred_batch_size.is_some() {
            target.preferred_batch_size = incoming.preferred_batch_size;
        }
        if !incoming.inputs.is_empty() {
            target.inputs = incoming.inputs;
        }
        if !incoming.outputs.is_empty() {
            target.outputs = incoming.outputs;
        }
        if !incoming.labels.is_empty() {
            target.labels = incoming.labels;
        }
    }

    fn read_manifest(&self) -> Result<AiModelManifest> {
        match fs::read_to_string(&self.manifest_path) {
            Ok(contents) => lib_ai::storage::decode_manifest(&contents).map_err(|err| Error::InvalidState(format!("failed to parse AI model manifest: {err}"))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AiModelManifest::default()),
            Err(err) => Err(err.into()),
        }
    }

    fn save_manifest_locked(&self, models: &HashMap<AiModelId, AiModelEntry>) -> Result<()> {
        let manifest = AiModelManifest {
            schema_version: lib_ai::storage::CURRENT_AI_MODEL_MANIFEST_SCHEMA_VERSION,
            models: models
                .values()
                .map(|entry| AiModelManifestEntry {
                    id: entry.descriptor.id.clone(),
                    format: entry.descriptor.format.clone(),
                    metadata: entry.descriptor.metadata.clone(),
                    artifact: entry.artifact.clone(),
                    label_artifact: entry.label_artifact.clone(),
                    created_at: entry.descriptor.created_at,
                    health: entry.descriptor.health.clone(),
                })
                .collect(),
        };

        if let Some(parent) = self.manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&manifest).map_err(|err| Error::InvalidState(format!("failed to serialize AI model manifest: {err}")))?;
        fs::write(&self.manifest_path, json)?;
        Ok(())
    }
}

fn build_registry() -> Registry<dyn AiBackend> {
    let registry = Registry::<dyn AiBackend>::new();
    registry.register_backend::<CoralBackend>("CoralBackend");
    registry.register_backend::<TfliteBackend>("TfliteCpuBackend");
    registry
}

fn resolve_storage_dir() -> PathBuf {
    lib_ai::storage::default_model_dir()
}

fn artifact_name(id: &AiModelId, format: &AiModelFormat) -> String {
    let extension = match format {
        AiModelFormat::TensorFlowLite => "tflite",
        AiModelFormat::Onnx => "onnx",
        AiModelFormat::Raw => "bin",
    };
    format!("{}.{}", id.0, extension)
}

fn to_lib_model_id(id: &AiModelId) -> lib_ai::model::ModelId {
    lib_ai::model::ModelId(id.0)
}

fn to_lib_model_format(format: &AiModelFormat) -> lib_ai::model::ModelFormat {
    match format {
        AiModelFormat::TensorFlowLite => lib_ai::model::ModelFormat::TensorFlowLite,
        AiModelFormat::Onnx => lib_ai::model::ModelFormat::Onnx,
        AiModelFormat::Raw => lib_ai::model::ModelFormat::Raw,
    }
}

fn to_lib_tensor_element_type(element_type: &AiTensorElementType) -> lib_ai::tensor::TensorElementType {
    match element_type {
        AiTensorElementType::U8 => lib_ai::tensor::TensorElementType::U8,
        AiTensorElementType::I8 => lib_ai::tensor::TensorElementType::I8,
        AiTensorElementType::I16 => lib_ai::tensor::TensorElementType::I16,
        AiTensorElementType::I32 => lib_ai::tensor::TensorElementType::I32,
        AiTensorElementType::F16 => lib_ai::tensor::TensorElementType::F16,
        AiTensorElementType::F32 => lib_ai::tensor::TensorElementType::F32,
    }
}

fn to_lib_tensor_quantization(quantization: &AiTensorQuantization) -> lib_ai::model::TensorQuantization {
    lib_ai::model::TensorQuantization { zero_point: quantization.zero_point.clone(), scale: quantization.scale.clone() }
}

fn to_lib_model_tensor_metadata(tensor: &AiModelTensorMetadata) -> lib_ai::model::ModelTensorMetadata {
    lib_ai::model::ModelTensorMetadata {
        name: tensor.name.clone(),
        element_type: to_lib_tensor_element_type(&tensor.element_type),
        shape: tensor.shape.clone(),
        quantization: tensor.quantization.as_ref().map(to_lib_tensor_quantization),
    }
}

fn to_lib_model_metadata(metadata: &AiModelMetadata) -> lib_ai::model::ModelMetadata {
    lib_ai::model::ModelMetadata {
        display_name: metadata.display_name.clone(),
        description: metadata.description.clone(),
        tags: metadata.tags.clone(),
        preferred_batch_size: metadata.preferred_batch_size,
        inputs: metadata.inputs.iter().map(to_lib_model_tensor_metadata).collect(),
        outputs: metadata.outputs.iter().map(to_lib_model_tensor_metadata).collect(),
        labels: metadata.labels.clone(),
    }
}

fn ai_tensor_element_type_from_lib(element_type: lib_ai::tensor::TensorElementType) -> AiTensorElementType {
    match element_type {
        lib_ai::tensor::TensorElementType::U8 => AiTensorElementType::U8,
        lib_ai::tensor::TensorElementType::I8 => AiTensorElementType::I8,
        lib_ai::tensor::TensorElementType::I16 => AiTensorElementType::I16,
        lib_ai::tensor::TensorElementType::I32 => AiTensorElementType::I32,
        lib_ai::tensor::TensorElementType::F16 => AiTensorElementType::F16,
        lib_ai::tensor::TensorElementType::F32 => AiTensorElementType::F32,
    }
}

fn ai_tensor_quantization_from_lib(quantization: lib_ai::model::TensorQuantization) -> AiTensorQuantization {
    AiTensorQuantization { zero_point: quantization.zero_point, scale: quantization.scale }
}

fn ai_model_tensor_metadata_from_lib(tensor: lib_ai::model::ModelTensorMetadata) -> AiModelTensorMetadata {
    AiModelTensorMetadata {
        name: tensor.name,
        element_type: ai_tensor_element_type_from_lib(tensor.element_type),
        shape: tensor.shape,
        quantization: tensor.quantization.map(ai_tensor_quantization_from_lib),
    }
}
