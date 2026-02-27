#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "engine")]
use std::sync::LazyLock;

pub mod backend;
#[cfg(feature = "engine")]
pub mod daedalus_plugin;
pub mod detection;
pub mod docs;
pub mod error;
#[cfg(feature = "engine")]
pub mod inference;
pub mod model;
#[cfg(feature = "engine")]
pub mod overlay_draw;
pub mod registry;
pub mod runtime;
pub mod storage;
pub mod tensor;

pub use backend::{AiBackend, AiModel, BackendCapabilities, BackendFeature, BackendHealth};
pub use detection::{ArucoDetection, NormalizedBoundingBox, VisionDetection2D};
pub use docs::{BackendDoc, DocumentedBackend};
pub use error::{AiError, Result};
pub use model::{ModelFormat, ModelId, ModelLoadRequest, ModelMetadata, ModelSource, ModelTensorMetadata, TensorQuantization};
pub use registry::{BackendInfo, Registerable, Registry};
pub use tensor::{Tensor, TensorElementType, TensorShape};

#[cfg(feature = "engine")]
pub use daedalus_plugin::AiPlugin;

#[cfg(feature = "engine")]
pub static BACKEND_REGISTRY: LazyLock<registry::Registry<dyn AiBackend>> = LazyLock::new(|| {
    let registry = registry::Registry::<dyn AiBackend>::new();

    #[cfg(feature = "backend-tflite")]
    {
        registry.register_backend::<backend::tflite::TfliteBackend>("TfliteCpuBackend");
    }

    #[cfg(feature = "backend-onnx")]
    {
        registry.register_backend::<backend::onnx::OnnxBackend>("OnnxCpuBackend");
    }

    #[cfg(feature = "backend-coral")]
    {
        registry.register_backend::<backend::coral::CoralBackend>("CoralBackend");
    }

    registry
});

#[cfg(feature = "engine")]
pub fn list_backends() -> Vec<registry::BackendInfo> {
    BACKEND_REGISTRY.list_backends()
}

#[cfg(feature = "engine")]
pub fn backend_capabilities(name: &str) -> registry::Result<registry::BackendInfo> {
    BACKEND_REGISTRY.get_backend(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendKind;
    use crate::runtime::{BackendSelection, Runtime, run_inference};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct HappyBackend;

    #[async_trait]
    impl AiBackend for HappyBackend {
        fn backend_kind(&self) -> BackendKind {
            BackendKind::Cpu
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                kind: BackendKind::Cpu,
                hardware_accelerated: false,
                supported_model_formats: vec![ModelFormat::TensorFlowLite],
                supported_precisions: vec![TensorElementType::F32],
                max_batch_size: None,
                features: vec![BackendFeature::Batching],
            }
        }

        async fn load(&self, request: &ModelLoadRequest) -> Result<Arc<dyn AiModel>> {
            Ok(Arc::new(HappyModel { id: request.id.clone(), metadata: request.metadata.clone() }))
        }
    }

    impl Registerable<dyn AiBackend> for HappyBackend {
        fn backend_kind() -> BackendKind {
            BackendKind::Cpu
        }

        fn hardware_accelerated() -> bool {
            false
        }

        fn supported_model_formats() -> Vec<ModelFormat> {
            vec![ModelFormat::TensorFlowLite]
        }

        fn supported_precisions() -> Vec<TensorElementType> {
            vec![TensorElementType::F32]
        }

        fn create(_params: Vec<(String, serde_json::Value)>) -> registry::Result<Arc<dyn AiBackend>> {
            Ok(Arc::new(HappyBackend))
        }

        fn get_create_params() -> Vec<(String, String)> {
            Vec::new()
        }

        fn features() -> Vec<BackendFeature> {
            vec![BackendFeature::Batching]
        }
    }

    impl DocumentedBackend for HappyBackend {
        fn docs() -> BackendDoc {
            BackendDoc { display_name: "HappyBackend".into(), summary: "Test backend".into(), description: "Test backend for registry coverage".into(), tags: vec!["test".into()] }
        }
    }

    struct HappyModel {
        id: ModelId,
        metadata: ModelMetadata,
    }

    #[async_trait]
    impl AiModel for HappyModel {
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

        async fn infer(&self, _inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
            Err(AiError::unsupported("mock backend"))
        }
    }

    fn build_request() -> ModelLoadRequest {
        ModelLoadRequest { id: ModelId::new(), format: ModelFormat::TensorFlowLite, source: ModelSource::Bytes(Vec::new()), metadata: ModelMetadata::default() }
    }

    #[tokio::test]
    async fn runtime_prefers_first_available_backend() {
        let registry = Registry::<dyn AiBackend>::new();
        registry.register_backend::<HappyBackend>("HappyBackend");

        let selection = BackendSelection { preferred: vec!["Missing".into(), "HappyBackend".into()] };
        let runtime = Runtime::with_selection(&registry, selection);
        let request = build_request();

        let loaded = runtime.load_model(&request).await.expect("model to load");
        assert_eq!(loaded.backend_name, "HappyBackend");

        let result = run_inference(loaded.model.as_ref(), Vec::new()).await;
        assert!(matches!(result, Err(AiError::Unsupported { .. })));
    }
}
