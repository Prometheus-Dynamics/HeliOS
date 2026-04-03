pub mod introspect;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::tensor::{TensorElementType, TensorShape};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModelFormat {
    TensorFlowLite,
    Onnx,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TensorQuantization {
    #[serde(default)]
    pub zero_point: Vec<i64>,
    #[serde(default)]
    pub scale: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ModelId(pub Uuid);

impl ModelId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ModelId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelMetadata {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub preferred_batch_size: Option<usize>,
    #[serde(default)]
    pub inputs: Vec<ModelTensorMetadata>,
    #[serde(default)]
    pub outputs: Vec<ModelTensorMetadata>,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTensorMetadata {
    pub name: Option<String>,
    pub element_type: TensorElementType,
    #[serde(default)]
    pub shape: TensorShape,
    #[serde(default)]
    pub quantization: Option<TensorQuantization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelSource {
    File(PathBuf),
    Bytes(Vec<u8>),
    RegistryReference(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLoadRequest {
    pub id: ModelId,
    pub format: ModelFormat,
    pub source: ModelSource,
    pub metadata: ModelMetadata,
}

impl ModelLoadRequest {
    pub fn from_path(path: impl Into<PathBuf>, format: ModelFormat) -> Self {
        Self { id: ModelId::new(), format, source: ModelSource::File(path.into()), metadata: ModelMetadata::default() }
    }
}
