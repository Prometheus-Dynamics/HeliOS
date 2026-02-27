#![allow(unsafe_code)]

use libloading::Library;
use std::{
    env,
    ffi::CStr,
    fmt,
    os::raw::{c_char, c_int},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::bundled_coral_lib_dir;

type ListDevicesFn = unsafe extern "C" fn(*mut usize) -> *mut EdgeTpuDevice;
type FreeDevicesFn = unsafe extern "C" fn(*mut EdgeTpuDevice);
type VersionFn = unsafe extern "C" fn() -> *const c_char;
type CreateDelegateFn = unsafe extern "C" fn(c_int, *const c_char, *const EdgeTpuOption, usize) -> *mut TfLiteDelegate;
type FreeDelegateFn = unsafe extern "C" fn(*mut TfLiteDelegate);

#[repr(C)]
pub struct EdgeTpuOption {
    name: *const c_char,
    value: *const c_char,
}

#[repr(C)]
pub struct TfLiteDelegate {
    _private: [u8; 0],
}

#[repr(C)]
struct EdgeTpuDevice {
    device_type: c_int,
    path: *const c_char,
}

#[derive(Debug)]
pub struct EdgeTpuLib {
    list_devices: ListDevicesFn,
    free_devices: FreeDevicesFn,
    create_delegate: CreateDelegateFn,
    free_delegate: FreeDelegateFn,
    _version: VersionFn,
    _library: Library,
}

static LIB: OnceLock<Result<EdgeTpuLib, RuntimeError>> = OnceLock::new();

const EDGETPU_RUNTIME_LIBRARY_ENV: &str = "LIBAI_EDGETPU_RUNTIME_LIBRARY";
const EDGETPU_RUNTIME_LIBRARY_DIRS_ENV: &str = "LIBAI_EDGETPU_RUNTIME_LIBRARY_DIRS";
const EDGETPU_RUNTIME_DEFAULT_CANDIDATES: &[&str] = &["libedgetpu.so.1", "libedgetpu.so"];

#[derive(Debug, Clone)]
pub enum RuntimeError {
    LibraryNotFound(String),
    SymbolLoad(String),
    OperationFailed(String),
    InvalidInput(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::LibraryNotFound(msg) => write!(f, "Edge TPU runtime not found: {msg}"),
            RuntimeError::SymbolLoad(msg) => write!(f, "failed to resolve libedgetpu symbol: {msg}"),
            RuntimeError::OperationFailed(msg) => write!(f, "Edge TPU operation failed: {msg}"),
            RuntimeError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone)]
pub struct RuntimeDeviceRecord {
    pub device_type: RuntimeDeviceType,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeDeviceType {
    Usb,
    Pci,
    Other(i32),
}

impl RuntimeDeviceType {
    fn from_raw(value: c_int) -> Self {
        match value {
            0 => RuntimeDeviceType::Pci,
            1 => RuntimeDeviceType::Usb,
            other => RuntimeDeviceType::Other(other),
        }
    }
}

pub fn enumerate_devices() -> Result<Vec<RuntimeDeviceRecord>, RuntimeError> {
    let lib = library()?;

    unsafe {
        let mut count: usize = 0;
        let devices_ptr = (lib.list_devices)(&mut count as *mut usize);
        if devices_ptr.is_null() || count == 0 {
            return Ok(Vec::new());
        }

        let devices = std::slice::from_raw_parts(devices_ptr, count);
        let mut records = Vec::with_capacity(devices.len());

        for device in devices {
            if device.path.is_null() {
                continue;
            }

            let path = CStr::from_ptr(device.path).to_string_lossy().into_owned();
            let device_type = RuntimeDeviceType::from_raw(device.device_type);

            records.push(RuntimeDeviceRecord { device_type, path });
        }

        (lib.free_devices)(devices_ptr);

        Ok(records)
    }
}

fn library() -> Result<&'static EdgeTpuLib, RuntimeError> {
    LIB.get_or_init(EdgeTpuLib::load).as_ref().map_err(Clone::clone)
}

pub fn library_handle() -> Result<&'static EdgeTpuLib, RuntimeError> {
    library()
}

impl EdgeTpuLib {
    fn load() -> Result<Self, RuntimeError> {
        let mut last_error = String::new();

        for candidate in library_candidates() {
            match unsafe { Library::new(&candidate) } {
                Ok(lib) => return unsafe { Self::from_library(lib) },
                Err(err) => last_error = format!("{}: {err}", candidate.display()),
            }
        }

        Err(RuntimeError::LibraryNotFound(last_error))
    }

    pub fn load_from(path: &Path) -> Result<Self, RuntimeError> {
        let lib = unsafe { Library::new(path) }.map_err(|err| RuntimeError::LibraryNotFound(err.to_string()))?;
        unsafe { Self::from_library(lib) }
    }

    unsafe fn from_library(lib: Library) -> Result<Self, RuntimeError> {
        let list_devices = unsafe { *lib.get::<ListDevicesFn>(b"edgetpu_list_devices\0").map_err(|err| RuntimeError::SymbolLoad(err.to_string()))? };
        let free_devices = unsafe { *lib.get::<FreeDevicesFn>(b"edgetpu_free_devices\0").map_err(|err| RuntimeError::SymbolLoad(err.to_string()))? };
        let create_delegate = unsafe { *lib.get::<CreateDelegateFn>(b"edgetpu_create_delegate\0").map_err(|err| RuntimeError::SymbolLoad(err.to_string()))? };
        let free_delegate = unsafe { *lib.get::<FreeDelegateFn>(b"edgetpu_free_delegate\0").map_err(|err| RuntimeError::SymbolLoad(err.to_string()))? };
        let version = unsafe { *lib.get::<VersionFn>(b"edgetpu_version\0").map_err(|err| RuntimeError::SymbolLoad(err.to_string()))? };

        Ok(EdgeTpuLib { list_devices, free_devices, create_delegate, free_delegate, _version: version, _library: lib })
    }

    pub fn flash_device(&self, device_path: Option<&str>) -> Result<(), RuntimeError> {
        use std::ffi::CString;
        let c_path = match device_path.map(str::trim).filter(|value| !value.is_empty()) {
            Some(path) => Some(CString::new(path).map_err(|_| RuntimeError::InvalidInput("device path contained null byte".into()))?),
            None => None,
        };
        let path_ptr = c_path.as_ref().map(|value| value.as_ptr()).unwrap_or(std::ptr::null());
        unsafe {
            let delegate = (self.create_delegate)(1, path_ptr, std::ptr::null(), 0);
            if delegate.is_null() {
                let suffix = device_path.map(|p| format!(" for {p}")).unwrap_or_default();
                return Err(RuntimeError::OperationFailed(format!("failed to initialize delegate{suffix}")));
            }
            (self.free_delegate)(delegate);
        }
        Ok(())
    }
}

pub fn flash_with_library(path: &Path, device_path: Option<&str>) -> Result<(), RuntimeError> {
    let lib = EdgeTpuLib::load_from(path)?;
    lib.flash_device(device_path)
}

fn library_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(explicit) = env::var(EDGETPU_RUNTIME_LIBRARY_ENV)
        && !explicit.trim().is_empty()
    {
        candidates.push(PathBuf::from(explicit));
    }

    let mut directories = Vec::new();
    if let Ok(dir_list) = env::var(EDGETPU_RUNTIME_LIBRARY_DIRS_ENV) {
        directories.extend(env::split_paths(&dir_list));
    }
    if let Some(bundled) = bundled_coral_lib_dir() {
        directories.push(bundled);
    }
    if let Ok(ld_paths) = env::var("LD_LIBRARY_PATH") {
        directories.extend(env::split_paths(&ld_paths));
    }

    for dir in directories {
        for name in EDGETPU_RUNTIME_DEFAULT_CANDIDATES {
            candidates.push(dir.join(name));
        }
    }

    for name in EDGETPU_RUNTIME_DEFAULT_CANDIDATES {
        candidates.push(PathBuf::from(name));
    }

    candidates
}

impl EdgeTpuLib {
    pub fn create_delegate(&self, device_type: RuntimeDeviceType, device_path: Option<&str>, options: &[EdgeTpuOption]) -> Result<*mut TfLiteDelegate, RuntimeError> {
        use std::ffi::CString;
        let path = if let Some(path) = device_path { Some(CString::new(path).map_err(|_| RuntimeError::InvalidInput("device path contained null byte".into()))?) } else { None };
        let path_ptr = path.as_ref().map(|value| value.as_ptr()).unwrap_or(std::ptr::null());
        let delegate = unsafe { (self.create_delegate)(device_type.into_raw(), path_ptr, options.as_ptr(), options.len()) };
        if delegate.is_null() {
            return Err(RuntimeError::OperationFailed("failed to create Edge TPU delegate".into()));
        }
        Ok(delegate)
    }

    pub fn free_delegate(&self, delegate: *mut TfLiteDelegate) {
        unsafe {
            (self.free_delegate)(delegate);
        }
    }
}

impl RuntimeDeviceType {
    pub fn into_raw(self) -> c_int {
        match self {
            RuntimeDeviceType::Usb => 1,
            RuntimeDeviceType::Pci => 0,
            RuntimeDeviceType::Other(value) => value,
        }
    }
}
