use crate::{
    backend::{AiBackend, AiModel},
    error::{AiError, Result},
    model::ModelLoadRequest,
    registry::Registry,
    tensor::Tensor,
};
use std::sync::Arc;
use tokio::runtime::Handle;

#[derive(Debug, Clone)]
pub struct BackendSelection {
    pub preferred: Vec<String>,
}

impl Default for BackendSelection {
    fn default() -> Self {
        let mut preferred = Vec::new();

        #[cfg(feature = "backend-coral")]
        {
            preferred.push("CoralBackend".to_string());
        }

        #[cfg(feature = "backend-tflite")]
        {
            preferred.push("TfliteCpuBackend".to_string());
        }

        #[cfg(feature = "backend-onnx")]
        {
            preferred.push("OnnxCpuBackend".to_string());
        }

        Self { preferred }
    }
}

pub struct LoadedModel {
    pub backend_name: String,
    pub model: Arc<dyn AiModel>,
}

pub struct Runtime<'a> {
    registry: &'a Registry<dyn AiBackend>,
    selection: BackendSelection,
}

impl<'a> Runtime<'a> {
    pub fn new(registry: &'a Registry<dyn AiBackend>) -> Self {
        Self { registry, selection: BackendSelection::default() }
    }

    pub fn with_selection(registry: &'a Registry<dyn AiBackend>, selection: BackendSelection) -> Self {
        Self { registry, selection }
    }

    pub async fn load_model(&self, request: &ModelLoadRequest) -> Result<LoadedModel> {
        let mut errors = Vec::new();

        for backend_name in &self.selection.preferred {
            let backend = match self.registry.create_backend_default(backend_name) {
                Ok(backend) => backend,
                Err(err) => {
                    errors.push(format!("{}: {}", backend_name, err));
                    continue;
                }
            };

            match backend.load(request).await {
                Ok(model) => {
                    return Ok(LoadedModel { backend_name: backend_name.clone(), model });
                }
                Err(err) => {
                    errors.push(format!("{}: {}", backend_name, err));
                }
            }
        }

        let reason = if errors.is_empty() { "no backends available".to_string() } else { errors.join("; ") };

        Err(AiError::ModelLoadFailed { reason })
    }

    pub fn load_model_blocking(&self, request: &ModelLoadRequest) -> Result<LoadedModel> {
        block_on(self.load_model(request))?
    }
}

pub async fn run_inference(model: &dyn AiModel, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
    model.infer(inputs).await
}

pub fn run_inference_blocking(model: &dyn AiModel, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
    block_on(run_inference(model, inputs))?
}

fn block_on<F: std::future::Future>(fut: F) -> Result<F::Output> {
    if let Ok(handle) = Handle::try_current() {
        Ok(handle.block_on(fut))
    } else {
        let rt = tokio::runtime::Runtime::new().map_err(|e| AiError::Other(e.into()))?;
        Ok(rt.block_on(fut))
    }
}
