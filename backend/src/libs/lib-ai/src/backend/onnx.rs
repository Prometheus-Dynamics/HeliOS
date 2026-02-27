use super::{AiBackend, AiModel, BackendCapabilities, BackendFeature, BackendHealth, BackendKind};
#[cfg(feature = "docs")]
use crate::docs::{BackendDoc, DocumentedBackend};
use crate::{
    backend::util::{read_model_bytes, tensor_into_tract, tract_tensor_into_tensor},
    error::{AiError, Result},
    model::{ModelFormat, ModelId, ModelLoadRequest, ModelMetadata},
    registry::{self, Registerable},
    tensor::{Tensor, TensorElementType},
};
use async_trait::async_trait;
use std::sync::Arc;
use tract_core::prelude::*;
use tract_onnx::{onnx, prelude::InferenceModelExt};

type OnnxPlan = TypedSimplePlan<TypedModel>;

#[derive(Debug, Default)]
pub struct OnnxBackend;

#[async_trait]
impl AiBackend for OnnxBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Cpu
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::Cpu,
            hardware_accelerated: false,
            supported_model_formats: vec![ModelFormat::Onnx],
            supported_precisions: vec![TensorElementType::F32],
            max_batch_size: None,
            features: vec![BackendFeature::Batching],
        }
    }

    async fn load(&self, request: &ModelLoadRequest) -> Result<Arc<dyn AiModel>> {
        let bytes = read_model_bytes(&request.source)?;
        let plan = build_plan(&bytes)?;

        let model = OnnxCpuModel { id: request.id.clone(), metadata: request.metadata.clone(), plan: Arc::new(plan) };

        Ok(Arc::new(model))
    }

    async fn health(&self) -> Result<BackendHealth> {
        Ok(BackendHealth::Ready)
    }
}

impl Registerable<dyn AiBackend> for OnnxBackend {
    fn backend_kind() -> BackendKind {
        BackendKind::Cpu
    }

    fn hardware_accelerated() -> bool {
        false
    }

    fn supported_model_formats() -> Vec<ModelFormat> {
        vec![ModelFormat::Onnx]
    }

    fn supported_precisions() -> Vec<TensorElementType> {
        vec![TensorElementType::F32]
    }

    fn create(_params: Vec<(String, serde_json::Value)>) -> registry::Result<Arc<dyn AiBackend>> {
        Ok(Arc::new(OnnxBackend))
    }

    fn get_create_params() -> Vec<(String, String)> {
        Vec::new()
    }

    fn features() -> Vec<BackendFeature> {
        vec![BackendFeature::Batching]
    }
}

#[cfg(feature = "docs")]
impl DocumentedBackend for OnnxBackend {
    fn docs() -> BackendDoc {
        BackendDoc {
            display_name: "ONNX CPU Inference".into(),
            summary: "ONNX graph execution using the tract runtime on the CPU.".into(),
            description: "Executes ONNX models on the host CPU via the tract inference runtime.".into(),
            tags: vec!["cpu".into(), "onnx".into(), "software".into()],
        }
    }
}

#[derive(Debug)]
struct OnnxCpuModel {
    id: ModelId,
    metadata: ModelMetadata,
    plan: Arc<OnnxPlan>,
}

#[async_trait]
impl AiModel for OnnxCpuModel {
    fn id(&self) -> &ModelId {
        &self.id
    }

    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::Cpu
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn infer(&self, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
        let tract_inputs: TVec<TValue> = inputs.into_iter().map(tensor_into_tract).collect::<Result<Vec<_>>>()?.into_iter().map(|tensor| tensor.into_tvalue()).collect();

        let outputs = self.plan.run(tract_inputs).map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?;

        outputs
            .into_iter()
            .enumerate()
            .map(|(idx, value)| {
                let tensor = value.into_tensor();
                tract_tensor_into_tensor(Some(&format!("output_{idx}")), tensor)
            })
            .collect()
    }
}

fn build_plan(bytes: &[u8]) -> Result<OnnxPlan> {
    let mut reader = std::io::Cursor::new(bytes);
    let model = onnx().model_for_read(&mut reader).and_then(|model| model.into_typed()).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;

    model.into_optimized().and_then(|model| model.into_runnable()).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })
}
