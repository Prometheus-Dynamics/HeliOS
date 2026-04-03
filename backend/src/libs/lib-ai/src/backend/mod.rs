use crate::{
    error::Result,
    model::{ModelFormat, ModelId, ModelLoadRequest, ModelMetadata},
    tensor::{Tensor, TensorElementType},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{any::Any, borrow::Cow, sync::Arc};

#[cfg(feature = "backend-tflite")]
pub mod tflite;

#[cfg(feature = "backend-onnx")]
pub mod onnx;

#[cfg(feature = "backend-coral")]
pub mod coral;

#[cfg(any(feature = "backend-tflite", feature = "backend-coral"))]
pub(crate) mod tflite_runtime;

#[cfg(any(feature = "backend-tflite", feature = "backend-onnx", feature = "backend-coral"))]
pub(crate) mod util;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Cpu,
    Coral,
    Custom(Cow<'static, str>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackendFeature {
    Batching,
    Streaming,
    ZeroCopy,
    Diagnostics,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub kind: BackendKind,
    pub hardware_accelerated: bool,
    pub supported_model_formats: Vec<ModelFormat>,
    pub supported_precisions: Vec<TensorElementType>,
    pub max_batch_size: Option<usize>,
    pub features: Vec<BackendFeature>,
}

impl BackendCapabilities {
    pub fn supports_format(&self, format: &ModelFormat) -> bool {
        self.supported_model_formats.iter().any(|f| f == format)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendHealth {
    Ready,
    Degraded { reason: String },
    Unavailable { reason: String },
}

#[async_trait]
pub trait AiBackend: Send + Sync {
    fn backend_kind(&self) -> BackendKind;
    fn capabilities(&self) -> BackendCapabilities;

    async fn load(&self, request: &ModelLoadRequest) -> Result<Arc<dyn AiModel>>;

    async fn health(&self) -> Result<BackendHealth> {
        Ok(BackendHealth::Ready)
    }
}

#[async_trait]
pub trait AiModel: Send + Sync {
    fn id(&self) -> &ModelId;
    fn metadata(&self) -> &ModelMetadata;
    fn backend_kind(&self) -> BackendKind;
    fn as_any(&self) -> &dyn Any;

    async fn infer(&self, inputs: Vec<Tensor>) -> Result<Vec<Tensor>>;
}
