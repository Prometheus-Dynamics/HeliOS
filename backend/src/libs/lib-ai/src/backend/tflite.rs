use super::{AiBackend, AiModel, BackendCapabilities, BackendFeature, BackendHealth, BackendKind};
#[cfg(feature = "docs")]
use crate::docs::{BackendDoc, DocumentedBackend};
use crate::{
    backend::{
        tflite_runtime,
        util::{guard_edge_tpu_support, read_model_bytes, tensor_into_tract, tract_tensor_into_tensor},
    },
    error::{AiError, Result},
    model::{ModelFormat, ModelId, ModelLoadRequest, ModelMetadata},
    registry::{self, Registerable},
    tensor::{Tensor, TensorElementType},
};
use async_trait::async_trait;
use std::{cell::RefCell, collections::HashMap, ptr::NonNull, sync::Arc};
use tflite_runtime::{TfLiteType, TfliteError};
use tracing::warn;
use tract_core::prelude::*;
use tract_tflite::tflite as tract_tflite_loader;

type TflitePlan = TypedSimplePlan<TypedModel>;

#[derive(Debug, Default)]
pub struct TfliteBackend;

#[async_trait]
impl AiBackend for TfliteBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Cpu
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::Cpu,
            hardware_accelerated: false,
            supported_model_formats: vec![ModelFormat::TensorFlowLite],
            supported_precisions: vec![TensorElementType::F32, TensorElementType::I8, TensorElementType::U8],
            max_batch_size: None,
            features: vec![BackendFeature::Batching],
        }
    }

    async fn load(&self, request: &ModelLoadRequest) -> Result<Arc<dyn AiModel>> {
        let bytes = read_model_bytes(&request.source)?;

        let plan = match build_plan(&bytes) {
            Ok(plan) => TfliteExecutionPlan::Tract(Arc::new(plan)),
            Err(err) => {
                if matches!(err, AiError::Unsupported { .. }) {
                    return Err(err);
                }
                warn!(model_id = %request.id.0, error = %err, "tract TFLite load failed; falling back to TFLite runtime");
                TfliteExecutionPlan::Runtime(TfliteRuntimeModel::new(Arc::new(bytes))?)
            }
        };

        let model = TfliteModel { id: request.id.clone(), metadata: request.metadata.clone(), plan };

        Ok(Arc::new(model))
    }

    async fn health(&self) -> Result<BackendHealth> {
        Ok(BackendHealth::Ready)
    }
}

impl Registerable<dyn AiBackend> for TfliteBackend {
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
        vec![TensorElementType::F32, TensorElementType::I8, TensorElementType::U8]
    }

    fn create(_params: Vec<(String, serde_json::Value)>) -> registry::Result<Arc<dyn AiBackend>> {
        Ok(Arc::new(TfliteBackend))
    }

    fn get_create_params() -> Vec<(String, String)> {
        Vec::new()
    }

    fn features() -> Vec<BackendFeature> {
        vec![BackendFeature::Batching]
    }
}

#[cfg(feature = "docs")]
impl DocumentedBackend for TfliteBackend {
    fn docs() -> BackendDoc {
        BackendDoc {
            display_name: "TFLite CPU Inference".into(),
            summary: "TensorFlow Lite interpreter running on the host CPU.".into(),
            description: "Executes TensorFlow Lite models using the tract runtime on the CPU.".into(),
            tags: vec!["cpu".into(), "tflite".into(), "software".into()],
        }
    }
}

#[derive(Debug)]
struct TfliteModel {
    id: ModelId,
    metadata: ModelMetadata,
    plan: TfliteExecutionPlan,
}

#[derive(Debug)]
enum TfliteExecutionPlan {
    Tract(Arc<TflitePlan>),
    Runtime(TfliteRuntimeModel),
}

#[async_trait]
impl AiModel for TfliteModel {
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
        match &self.plan {
            TfliteExecutionPlan::Tract(plan) => {
                let tract_inputs: TVec<TValue> = inputs.into_iter().map(tensor_into_tract).collect::<Result<Vec<_>>>()?.into_iter().map(|tensor| tensor.into_tvalue()).collect();

                let outputs = plan.run(tract_inputs).map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?;

                outputs
                    .into_iter()
                    .enumerate()
                    .map(|(idx, value)| {
                        let tensor = value.into_tensor();
                        tract_tensor_into_tensor(Some(&format!("output_{idx}")), tensor)
                    })
                    .collect()
            }
            TfliteExecutionPlan::Runtime(model) => model.run_inference(inputs),
        }
    }
}

#[derive(Debug)]
struct TfliteRuntimeModel {
    bytes: Arc<Vec<u8>>,
}

impl TfliteRuntimeModel {
    fn new(bytes: Arc<Vec<u8>>) -> Result<Self> {
        tflite_runtime::library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        Ok(Self { bytes })
    }

    fn run_inference(&self, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
        let key = TfliteCpuSessionKey::new(&self.bytes);
        TFLITE_CPU_SESSION_CACHE.with(|store| {
            let mut sessions = store.borrow_mut();
            if !sessions.contains_key(&key) {
                let session = TfliteCpuSession::new(&self.bytes)?;
                sessions.insert(key.clone(), session);
            }

            let result = sessions.get_mut(&key).expect("session just inserted").run(inputs);
            if result.is_err() {
                sessions.remove(&key);
            }
            result
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TfliteCpuSessionKey {
    model_ptr: usize,
}

impl TfliteCpuSessionKey {
    fn new(model_bytes: &Arc<Vec<u8>>) -> Self {
        Self { model_ptr: Arc::as_ptr(model_bytes) as usize }
    }
}

thread_local! {
    static TFLITE_CPU_SESSION_CACHE: RefCell<HashMap<TfliteCpuSessionKey, TfliteCpuSession>> = RefCell::new(HashMap::new());
}

struct TfliteCpuSession {
    _model: TfliteModelHandle<'static>,
    _options: InterpreterOptionsHandle<'static>,
    interpreter: InterpreterHandle<'static>,
}

impl TfliteCpuSession {
    fn new(model_bytes: &Arc<Vec<u8>>) -> Result<Self> {
        let tflite = tflite_runtime::library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        let model = TfliteModelHandle::new(tflite, model_bytes)?;
        let options = InterpreterOptionsHandle::new(tflite)?;
        let interpreter = InterpreterHandle::new(tflite, model.ptr.as_ptr(), options.ptr.as_ptr())?;
        tflite.allocate_tensors(interpreter.ptr.as_ptr()).map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        Ok(Self { _model: model, _options: options, interpreter })
    }

    fn run(&mut self, inputs: Vec<Tensor>) -> Result<Vec<Tensor>> {
        if inputs.is_empty() {
            return Err(AiError::InvalidInput { reason: "no tensors provided".into() });
        }
        let tflite = self.interpreter.lib;
        let interpreter_ptr = self.interpreter.ptr.as_ptr();

        let input_count = tflite.input_tensor_count(interpreter_ptr);
        if input_count != inputs.len() {
            return Err(AiError::InvalidInput { reason: format!("model expects {input_count} inputs but received {}", inputs.len()) });
        }

        for (idx, tensor) in inputs.into_iter().enumerate() {
            let tensor_ptr = tflite.input_tensor(interpreter_ptr, idx).map_err(map_tflite_not_ready)?;
            encode_input_tensor(tflite, tensor_ptr.as_ptr(), tensor)?;
        }

        tflite.invoke(interpreter_ptr).map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?;

        let output_count = tflite.output_tensor_count(interpreter_ptr);
        let mut outputs = Vec::with_capacity(output_count);
        for idx in 0..output_count {
            let tensor_ptr = tflite.output_tensor(interpreter_ptr, idx).map_err(map_tflite_not_ready)?;
            outputs.push(decode_output_tensor(tflite, tensor_ptr.as_ptr(), idx)?);
        }

        Ok(outputs)
    }
}

fn encode_input_tensor(tflite: &tflite_runtime::TfliteLib, tensor_ptr: *mut tflite_runtime::TfLiteTensor, tensor: Tensor) -> Result<()> {
    let bytes = tensor.bytes.clone().ok_or_else(|| AiError::InvalidInput { reason: format!("tensor {} missing bytes", tensor.name) })?;
    let expected_type = tflite.tensor_type(tensor_ptr.cast());
    let element_type = map_tflite_type(expected_type)?;
    if element_type != tensor.element_type {
        return Err(AiError::InvalidInput { reason: format!("tensor {} expected {:?} but received {:?}", tensor.name, element_type, tensor.element_type) });
    }
    let byte_size = tflite.tensor_byte_size(tensor_ptr.cast());
    if bytes.len() != byte_size {
        return Err(AiError::InvalidInput { reason: format!("tensor {} expected {} bytes but received {}", tensor.name, byte_size, bytes.len()) });
    }
    tflite.tensor_copy_from_buffer(tensor_ptr, bytes.as_ptr(), bytes.len()).map_err(|err| AiError::InvalidInput { reason: err.to_string() })
}

fn decode_output_tensor(tflite: &tflite_runtime::TfliteLib, tensor_ptr: *mut tflite_runtime::TfLiteTensor, idx: usize) -> Result<Tensor> {
    let element_type = map_tflite_type(tflite.tensor_type(tensor_ptr.cast()))?;
    let byte_size = tflite.tensor_byte_size(tensor_ptr.cast());
    let mut bytes = vec![0u8; byte_size];
    tflite.tensor_copy_to_buffer(tensor_ptr.cast(), bytes.as_mut_ptr(), bytes.len()).map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?;
    let dims = tflite.tensor_num_dims(tensor_ptr.cast());
    let mut shape = Vec::with_capacity(dims);
    for dim in 0..dims {
        let value = tflite.tensor_dim(tensor_ptr.cast(), dim);
        if value <= 0 {
            return Err(AiError::InferenceFailed { reason: format!("tensor output_{idx} has invalid dimension {value}") });
        }
        shape.push(value as usize);
    }
    Ok(Tensor::new(format!("output_{idx}"), element_type, shape).with_bytes(bytes))
}

fn map_tflite_type(value: TfLiteType) -> Result<TensorElementType> {
    match value {
        TfLiteType::UInt8 => Ok(TensorElementType::U8),
        TfLiteType::Int8 => Ok(TensorElementType::I8),
        TfLiteType::Float32 => Ok(TensorElementType::F32),
        other => Err(AiError::InvalidInput { reason: format!("unsupported TF Lite tensor type: {other:?}") }),
    }
}

fn map_tflite_not_ready(err: TfliteError) -> AiError {
    AiError::NotReady { reason: err.to_string() }
}

struct TfliteModelHandle<'a> {
    lib: &'a tflite_runtime::TfliteLib,
    ptr: NonNull<tflite_runtime::TfLiteModel>,
}

impl<'a> TfliteModelHandle<'a> {
    fn new(lib: &'a tflite_runtime::TfliteLib, bytes: &Arc<Vec<u8>>) -> Result<Self> {
        let ptr = lib.create_model(bytes.as_ptr() as *const _, bytes.len()).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        Ok(Self { lib, ptr })
    }
}

impl Drop for TfliteModelHandle<'_> {
    fn drop(&mut self) {
        self.lib.delete_model(self.ptr.as_ptr());
    }
}

struct InterpreterOptionsHandle<'a> {
    lib: &'a tflite_runtime::TfliteLib,
    ptr: NonNull<tflite_runtime::TfLiteInterpreterOptions>,
}

impl<'a> InterpreterOptionsHandle<'a> {
    fn new(lib: &'a tflite_runtime::TfliteLib) -> Result<Self> {
        let ptr = lib.create_options().map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        lib.options_set_threads(ptr.as_ptr(), 1);
        Ok(Self { lib, ptr })
    }
}

impl Drop for InterpreterOptionsHandle<'_> {
    fn drop(&mut self) {
        self.lib.delete_options(self.ptr.as_ptr());
    }
}

struct InterpreterHandle<'a> {
    lib: &'a tflite_runtime::TfliteLib,
    ptr: NonNull<tflite_runtime::TfLiteInterpreter>,
}

impl<'a> InterpreterHandle<'a> {
    fn new(lib: &'a tflite_runtime::TfliteLib, model: *const tflite_runtime::TfLiteModel, options: *const tflite_runtime::TfLiteInterpreterOptions) -> Result<Self> {
        let ptr = lib.create_interpreter(model, options).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        Ok(Self { lib, ptr })
    }
}

impl Drop for InterpreterHandle<'_> {
    fn drop(&mut self) {
        self.lib.delete_interpreter(self.ptr.as_ptr());
    }
}

fn build_plan(bytes: &[u8]) -> Result<TflitePlan> {
    guard_edge_tpu_support(bytes)?;

    let mut reader = std::io::Cursor::new(bytes);
    let model = tract_tflite_loader().model_for_read(&mut reader).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;

    model.into_optimized().and_then(|model| model.into_runnable()).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })
}
