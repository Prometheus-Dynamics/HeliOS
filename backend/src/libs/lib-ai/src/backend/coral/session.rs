use super::*;
use crate::backend::tflite_runtime::library as tflite_library;
use runtime::{EdgeTpuLib, EdgeTpuOption, TfLiteDelegate, enumerate_devices, library_handle as edge_library};
use std::{
    cell::RefCell,
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    ptr::NonNull,
};
use tflite_runtime::{TfLiteType, TfliteError};

#[derive(Debug)]
pub(super) struct EdgeTpuCompiledModel {
    bytes: Arc<Vec<u8>>,
}

impl EdgeTpuCompiledModel {
    pub(super) fn new(_: ModelId, _: ModelMetadata, bytes: Vec<u8>) -> Result<Self> {
        tflite_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        edge_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        Ok(Self { bytes: Arc::new(bytes) })
    }

    pub(super) fn run_inference(&self, inputs: Vec<Tensor>, preferred_device: Option<String>) -> Result<Vec<Tensor>> {
        let key = EdgeTpuSessionKey::new(&self.bytes, preferred_device.clone());
        EDGE_TPU_SESSION_CACHE.with(|store| {
            let mut sessions = store.borrow_mut();
            if !sessions.contains_key(&key) {
                let session = EdgeTpuSession::new(&self.bytes, preferred_device.clone())?;
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
struct EdgeTpuSessionKey {
    model_ptr: usize,
    preferred_device: Option<String>,
}

impl EdgeTpuSessionKey {
    fn new(model_bytes: &Arc<Vec<u8>>, preferred_device: Option<String>) -> Self {
        Self { model_ptr: Arc::as_ptr(model_bytes) as usize, preferred_device }
    }
}

thread_local! {
    static EDGE_TPU_SESSION_CACHE: RefCell<HashMap<EdgeTpuSessionKey, EdgeTpuSession>> = RefCell::new(HashMap::new());
}

pub fn clear_edge_tpu_session_cache() {
    EDGE_TPU_SESSION_CACHE.with(|store| store.borrow_mut().clear());
}

struct EdgeTpuSession {
    _model: TfliteModelHandle<'static>,
    _options: InterpreterOptionsHandle<'static>,
    _delegate: EdgeTpuDelegateHandle<'static>,
    interpreter: InterpreterHandle<'static>,
}

impl EdgeTpuSession {
    fn new(model_bytes: &Arc<Vec<u8>>, preferred_device: Option<String>) -> Result<Self> {
        let tflite = tflite_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        let edge = edge_library().map_err(|err| AiError::NotReady { reason: err.to_string() })?;

        let model = TfliteModelHandle::new(tflite, model_bytes)?;
        let options = InterpreterOptionsHandle::new(tflite)?;

        let runtime_devices = enumerate_devices().ok();
        let matched_device = preferred_device.as_deref().and_then(|path| runtime_devices.as_ref().and_then(|devices| devices.iter().find(|record| record.path == path).cloned()));
        if preferred_device.is_some() && matched_device.is_none() {
            warn!(preferred_device = ?preferred_device, "edge tpu preferred device not found in runtime list; falling back to first available device");
        }
        let delegate_device = matched_device.clone().or_else(|| runtime_devices.as_ref().and_then(|devices| devices.first().cloned()));
        let delegate_path = matched_device.as_ref().map(|record| record.path.as_str());

        let delegate = EdgeTpuDelegateHandle::new(edge, delegate_device, delegate_path)?;
        tflite.options_add_delegate(options.ptr.as_ptr(), delegate.ptr.cast());
        let interpreter = InterpreterHandle::new(tflite, model.ptr.as_ptr(), options.ptr.as_ptr())?;
        tflite.allocate_tensors(interpreter.ptr.as_ptr()).map_err(|err| AiError::NotReady { reason: err.to_string() })?;

        Ok(Self { _model: model, _options: options, _delegate: delegate, interpreter })
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

struct EdgeTpuDelegateHandle<'a> {
    lib: &'a EdgeTpuLib,
    ptr: *mut TfLiteDelegate,
}

impl<'a> EdgeTpuDelegateHandle<'a> {
    fn new(lib: &'a EdgeTpuLib, device: Option<RuntimeDeviceRecord>, preferred_path: Option<&str>) -> Result<Self> {
        let (device_type, path_owned) = if let Some(path) = preferred_path {
            (RuntimeDeviceType::Usb, Some(path.to_string()))
        } else if let Some(record) = device {
            (record.device_type, Some(record.path))
        } else {
            (RuntimeDeviceType::Usb, None)
        };
        let ptr = lib.create_delegate(device_type, path_owned.as_deref(), &[] as &[EdgeTpuOption]).map_err(|err| AiError::NotReady { reason: err.to_string() })?;
        Ok(Self { lib, ptr })
    }
}

impl Drop for EdgeTpuDelegateHandle<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.lib.free_delegate(self.ptr);
        }
    }
}

pub fn device_hardware_key(device: &CoralUsbDevice) -> String {
    if let Some(serial) = device.serial_number.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return format!("USB::SER::{serial}");
    }
    if let Some(path) = device.path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return format!("USB::PATH::{path}");
    }
    if !device.port_path.is_empty() {
        let ports = device.port_path.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(".");
        return format!("USB::PORT::{}-{}", device.bus_number, ports);
    }
    format!("USB::BUS::{:03}-{:03}", device.bus_number, device.address)
}

pub fn alias_identity_from_key(hardware_key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    hardware_key.hash(&mut hasher);
    let hash = hasher.finish();
    format!("edge-tpu-{:04x}", (hash & 0xFFFF) as u16)
}

pub fn alias_display_from_identity(identity_alias: &str) -> String {
    let suffix = identity_alias.split('-').next_back().unwrap_or("tpu").to_uppercase();
    format!("Edge TPU {suffix}")
}

pub fn device_alias_identity(device: &CoralUsbDevice) -> String {
    alias_identity_from_key(&device_hardware_key(device))
}

pub fn device_alias_display(device: &CoralUsbDevice) -> String {
    alias_display_from_identity(&device_alias_identity(device))
}
