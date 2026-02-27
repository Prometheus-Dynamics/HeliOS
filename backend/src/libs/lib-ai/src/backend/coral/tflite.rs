#![allow(unsafe_code)]

use libloading::Library;
use std::{env, ffi::c_void, fmt, os::raw::c_int, path::PathBuf, ptr::NonNull, sync::OnceLock};

use super::{bundled_coral_lib_dir, runtime::TfLiteDelegate};

type ModelCreateFn = unsafe extern "C" fn(*const c_void, usize) -> *mut TfLiteModel;
type ModelDeleteFn = unsafe extern "C" fn(*mut TfLiteModel);
type OptionsCreateFn = unsafe extern "C" fn() -> *mut TfLiteInterpreterOptions;
type OptionsDeleteFn = unsafe extern "C" fn(*mut TfLiteInterpreterOptions);
type OptionsSetThreadsFn = unsafe extern "C" fn(*mut TfLiteInterpreterOptions, c_int);
type OptionsAddDelegateFn = unsafe extern "C" fn(*mut TfLiteInterpreterOptions, *mut TfLiteDelegate);
type InterpreterCreateFn = unsafe extern "C" fn(*const TfLiteModel, *const TfLiteInterpreterOptions) -> *mut TfLiteInterpreter;
type InterpreterDeleteFn = unsafe extern "C" fn(*mut TfLiteInterpreter);
type InterpreterAllocateFn = unsafe extern "C" fn(*mut TfLiteInterpreter) -> TfLiteStatus;
type InterpreterInvokeFn = unsafe extern "C" fn(*mut TfLiteInterpreter) -> TfLiteStatus;
type InterpreterInputCountFn = unsafe extern "C" fn(*const TfLiteInterpreter) -> c_int;
type InterpreterOutputCountFn = unsafe extern "C" fn(*const TfLiteInterpreter) -> c_int;
type InterpreterGetInputFn = unsafe extern "C" fn(*mut TfLiteInterpreter, c_int) -> *mut TfLiteTensor;
type InterpreterGetOutputFn = unsafe extern "C" fn(*const TfLiteInterpreter, c_int) -> *const TfLiteTensor;
type TensorTypeFn = unsafe extern "C" fn(*const TfLiteTensor) -> TfLiteType;
type TensorNumDimsFn = unsafe extern "C" fn(*const TfLiteTensor) -> c_int;
type TensorDimFn = unsafe extern "C" fn(*const TfLiteTensor, c_int) -> c_int;
type TensorByteSizeFn = unsafe extern "C" fn(*const TfLiteTensor) -> usize;
type TensorCopyFromFn = unsafe extern "C" fn(*mut TfLiteTensor, *const c_void, usize) -> TfLiteStatus;
type TensorCopyToFn = unsafe extern "C" fn(*const TfLiteTensor, *mut c_void, usize) -> TfLiteStatus;

#[repr(C)]
pub struct TfLiteModel;
#[repr(C)]
pub struct TfLiteInterpreter;
#[repr(C)]
pub struct TfLiteInterpreterOptions;
#[repr(C)]
pub struct TfLiteTensor;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TfLiteStatus {
    Ok = 0,
    Error = 1,
    DelegateError = 2,
    ApplicationError = 3,
}

impl TfLiteStatus {
    fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::Ok,
            1 => Self::Error,
            2 => Self::DelegateError,
            3 => Self::ApplicationError,
            _ => Self::Error,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TfLiteType {
    NoType = 0,
    Float32 = 1,
    Int32 = 2,
    UInt8 = 3,
    Int64 = 4,
    String = 5,
    Bool = 6,
    Int16 = 7,
    Complex64 = 8,
    Int8 = 9,
    Float16 = 10,
    Float64 = 11,
    Complex128 = 12,
    UInt64 = 13,
    Resource = 14,
    Variant = 15,
    UInt32 = 16,
    UInt16 = 17,
    Int4 = 18,
}

impl TfLiteType {
    fn from_raw(value: i32) -> Self {
        match value {
            0 => Self::NoType,
            1 => Self::Float32,
            2 => Self::Int32,
            3 => Self::UInt8,
            4 => Self::Int64,
            5 => Self::String,
            6 => Self::Bool,
            7 => Self::Int16,
            8 => Self::Complex64,
            9 => Self::Int8,
            10 => Self::Float16,
            11 => Self::Float64,
            12 => Self::Complex128,
            13 => Self::UInt64,
            14 => Self::Resource,
            15 => Self::Variant,
            16 => Self::UInt32,
            17 => Self::UInt16,
            18 => Self::Int4,
            _ => Self::NoType,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TfliteError {
    LibraryNotFound(String),
    SymbolLoad(String),
    OperationFailed(String),
}

impl fmt::Display for TfliteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TfliteError::LibraryNotFound(msg) => write!(f, "TensorFlow Lite runtime not found: {msg}"),
            TfliteError::SymbolLoad(msg) => write!(f, "failed to resolve TensorFlow Lite symbol: {msg}"),
            TfliteError::OperationFailed(msg) => write!(f, "TensorFlow Lite operation failed: {msg}"),
        }
    }
}

impl std::error::Error for TfliteError {}

pub struct TfliteLib {
    model_create: ModelCreateFn,
    model_delete: ModelDeleteFn,
    options_create: OptionsCreateFn,
    options_delete: OptionsDeleteFn,
    options_set_threads: OptionsSetThreadsFn,
    options_add_delegate: OptionsAddDelegateFn,
    interpreter_create: InterpreterCreateFn,
    interpreter_delete: InterpreterDeleteFn,
    interpreter_allocate: InterpreterAllocateFn,
    interpreter_invoke: InterpreterInvokeFn,
    interpreter_input_count: InterpreterInputCountFn,
    interpreter_output_count: InterpreterOutputCountFn,
    interpreter_get_input: InterpreterGetInputFn,
    interpreter_get_output: InterpreterGetOutputFn,
    tensor_type: TensorTypeFn,
    tensor_num_dims: TensorNumDimsFn,
    tensor_dim: TensorDimFn,
    tensor_byte_size: TensorByteSizeFn,
    tensor_copy_from: TensorCopyFromFn,
    tensor_copy_to: TensorCopyToFn,
    _library: Library,
}

static LIB: OnceLock<Result<TfliteLib, TfliteError>> = OnceLock::new();

const TFLITE_RUNTIME_LIBRARY_ENV: &str = "LIBAI_TFLITE_RUNTIME_LIBRARY";
const TFLITE_RUNTIME_LIBRARY_DIRS_ENV: &str = "LIBAI_TFLITE_RUNTIME_LIBRARY_DIRS";
const TFLITE_RUNTIME_DEFAULT_CANDIDATES: &[&str] = &["libtensorflowlite_c.so.2", "libtensorflowlite_c.so"];

pub fn library() -> Result<&'static TfliteLib, TfliteError> {
    LIB.get_or_init(TfliteLib::load).as_ref().map_err(Clone::clone)
}

impl TfliteLib {
    fn load() -> Result<Self, TfliteError> {
        let mut last_error = String::new();
        for candidate in library_candidates() {
            match unsafe { Library::new(&candidate) } {
                Ok(lib) => return unsafe { Self::from_library(lib) },
                Err(err) => {
                    last_error = format!("{}: {err}", candidate.display());
                }
            }
        }
        Err(TfliteError::LibraryNotFound(last_error))
    }

    unsafe fn from_library(lib: Library) -> Result<Self, TfliteError> {
        macro_rules! load {
            ($symbol:literal, $ty:ty) => {
                unsafe { *lib.get::<$ty>($symbol).map_err(|err| TfliteError::SymbolLoad(err.to_string()))? }
            };
        }

        Ok(Self {
            model_create: load!(b"TfLiteModelCreate\0", ModelCreateFn),
            model_delete: load!(b"TfLiteModelDelete\0", ModelDeleteFn),
            options_create: load!(b"TfLiteInterpreterOptionsCreate\0", OptionsCreateFn),
            options_delete: load!(b"TfLiteInterpreterOptionsDelete\0", OptionsDeleteFn),
            options_set_threads: load!(b"TfLiteInterpreterOptionsSetNumThreads\0", OptionsSetThreadsFn),
            options_add_delegate: load!(b"TfLiteInterpreterOptionsAddDelegate\0", OptionsAddDelegateFn),
            interpreter_create: load!(b"TfLiteInterpreterCreate\0", InterpreterCreateFn),
            interpreter_delete: load!(b"TfLiteInterpreterDelete\0", InterpreterDeleteFn),
            interpreter_allocate: load!(b"TfLiteInterpreterAllocateTensors\0", InterpreterAllocateFn),
            interpreter_invoke: load!(b"TfLiteInterpreterInvoke\0", InterpreterInvokeFn),
            interpreter_input_count: load!(b"TfLiteInterpreterGetInputTensorCount\0", InterpreterInputCountFn),
            interpreter_output_count: load!(b"TfLiteInterpreterGetOutputTensorCount\0", InterpreterOutputCountFn),
            interpreter_get_input: load!(b"TfLiteInterpreterGetInputTensor\0", InterpreterGetInputFn),
            interpreter_get_output: load!(b"TfLiteInterpreterGetOutputTensor\0", InterpreterGetOutputFn),
            tensor_type: load!(b"TfLiteTensorType\0", TensorTypeFn),
            tensor_num_dims: load!(b"TfLiteTensorNumDims\0", TensorNumDimsFn),
            tensor_dim: load!(b"TfLiteTensorDim\0", TensorDimFn),
            tensor_byte_size: load!(b"TfLiteTensorByteSize\0", TensorByteSizeFn),
            tensor_copy_from: load!(b"TfLiteTensorCopyFromBuffer\0", TensorCopyFromFn),
            tensor_copy_to: load!(b"TfLiteTensorCopyToBuffer\0", TensorCopyToFn),
            _library: lib,
        })
    }

    pub fn create_model(&self, data: *const c_void, len: usize) -> Result<NonNull<TfLiteModel>, TfliteError> {
        let model = unsafe { (self.model_create)(data, len) };
        NonNull::new(model).ok_or_else(|| TfliteError::OperationFailed("failed to create TfLite model".into()))
    }

    pub fn delete_model(&self, model: *mut TfLiteModel) {
        unsafe { (self.model_delete)(model) };
    }

    pub fn create_options(&self) -> Result<NonNull<TfLiteInterpreterOptions>, TfliteError> {
        let opts = unsafe { (self.options_create)() };
        NonNull::new(opts).ok_or_else(|| TfliteError::OperationFailed("failed to create interpreter options".into()))
    }

    pub fn delete_options(&self, options: *mut TfLiteInterpreterOptions) {
        unsafe { (self.options_delete)(options) };
    }

    pub fn options_set_threads(&self, options: *mut TfLiteInterpreterOptions, threads: i32) {
        unsafe { (self.options_set_threads)(options, threads as c_int) };
    }

    pub fn options_add_delegate(&self, options: *mut TfLiteInterpreterOptions, delegate: *mut TfLiteDelegate) {
        unsafe { (self.options_add_delegate)(options, delegate) };
    }

    pub fn create_interpreter(&self, model: *const TfLiteModel, options: *const TfLiteInterpreterOptions) -> Result<NonNull<TfLiteInterpreter>, TfliteError> {
        let interpreter = unsafe { (self.interpreter_create)(model, options) };
        NonNull::new(interpreter).ok_or_else(|| TfliteError::OperationFailed("failed to create interpreter".into()))
    }

    pub fn delete_interpreter(&self, interpreter: *mut TfLiteInterpreter) {
        unsafe { (self.interpreter_delete)(interpreter) };
    }

    pub fn allocate_tensors(&self, interpreter: *mut TfLiteInterpreter) -> Result<(), TfliteError> {
        let status = TfLiteStatus::from_raw(unsafe { (self.interpreter_allocate)(interpreter) } as i32);
        match status {
            TfLiteStatus::Ok => Ok(()),
            status => Err(TfliteError::OperationFailed(format!("allocate tensors failed: {status:?}"))),
        }
    }

    pub fn invoke(&self, interpreter: *mut TfLiteInterpreter) -> Result<(), TfliteError> {
        let status = TfLiteStatus::from_raw(unsafe { (self.interpreter_invoke)(interpreter) } as i32);
        match status {
            TfLiteStatus::Ok => Ok(()),
            status => Err(TfliteError::OperationFailed(format!("interpreter invoke failed: {status:?}"))),
        }
    }

    pub fn input_tensor_count(&self, interpreter: *const TfLiteInterpreter) -> usize {
        unsafe { (self.interpreter_input_count)(interpreter) as usize }
    }

    pub fn output_tensor_count(&self, interpreter: *const TfLiteInterpreter) -> usize {
        unsafe { (self.interpreter_output_count)(interpreter) as usize }
    }

    pub fn input_tensor(&self, interpreter: *mut TfLiteInterpreter, index: usize) -> Result<NonNull<TfLiteTensor>, TfliteError> {
        let tensor = unsafe { (self.interpreter_get_input)(interpreter, index as c_int) };
        NonNull::new(tensor).ok_or_else(|| TfliteError::OperationFailed(format!("input tensor {index} unavailable")))
    }

    pub fn output_tensor(&self, interpreter: *const TfLiteInterpreter, index: usize) -> Result<NonNull<TfLiteTensor>, TfliteError> {
        let tensor = unsafe { (self.interpreter_get_output)(interpreter, index as c_int) as *mut TfLiteTensor };
        NonNull::new(tensor).ok_or_else(|| TfliteError::OperationFailed(format!("output tensor {index} unavailable")))
    }

    pub fn tensor_type(&self, tensor: *const TfLiteTensor) -> TfLiteType {
        TfLiteType::from_raw(unsafe { (self.tensor_type)(tensor) } as i32)
    }

    pub fn tensor_num_dims(&self, tensor: *const TfLiteTensor) -> usize {
        unsafe { (self.tensor_num_dims)(tensor) as usize }
    }

    pub fn tensor_dim(&self, tensor: *const TfLiteTensor, idx: usize) -> i32 {
        unsafe { (self.tensor_dim)(tensor, idx as c_int) }
    }

    pub fn tensor_byte_size(&self, tensor: *const TfLiteTensor) -> usize {
        unsafe { (self.tensor_byte_size)(tensor) }
    }

    pub fn tensor_copy_from_buffer(&self, tensor: *mut TfLiteTensor, data: *const u8, len: usize) -> Result<(), TfliteError> {
        let status = TfLiteStatus::from_raw(unsafe { (self.tensor_copy_from)(tensor, data as *const c_void, len) } as i32);
        match status {
            TfLiteStatus::Ok => Ok(()),
            status => Err(TfliteError::OperationFailed(format!("copy into tensor failed: {status:?}"))),
        }
    }

    pub fn tensor_copy_to_buffer(&self, tensor: *const TfLiteTensor, data: *mut u8, len: usize) -> Result<(), TfliteError> {
        let status = TfLiteStatus::from_raw(unsafe { (self.tensor_copy_to)(tensor, data as *mut c_void, len) } as i32);
        match status {
            TfLiteStatus::Ok => Ok(()),
            status => Err(TfliteError::OperationFailed(format!("copy from tensor failed: {status:?}"))),
        }
    }
}

fn library_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(explicit) = env::var(TFLITE_RUNTIME_LIBRARY_ENV)
        && !explicit.trim().is_empty()
    {
        candidates.push(PathBuf::from(explicit));
    }

    let mut directories = Vec::new();
    if let Ok(dir_list) = env::var(TFLITE_RUNTIME_LIBRARY_DIRS_ENV) {
        directories.extend(env::split_paths(&dir_list));
    }
    if let Some(bundled) = bundled_coral_lib_dir() {
        directories.push(bundled);
    }
    if let Ok(ld_paths) = env::var("LD_LIBRARY_PATH") {
        directories.extend(env::split_paths(&ld_paths));
    }

    append_directory_candidates(&mut candidates, &directories);

    for name in TFLITE_RUNTIME_DEFAULT_CANDIDATES {
        candidates.push(PathBuf::from(name));
    }

    candidates
}

fn append_directory_candidates(candidates: &mut Vec<PathBuf>, directories: &[PathBuf]) {
    for dir in directories {
        for name in TFLITE_RUNTIME_DEFAULT_CANDIDATES {
            candidates.push(dir.join(name));
        }
    }
}
