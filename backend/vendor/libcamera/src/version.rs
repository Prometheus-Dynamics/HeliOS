//! Compile-time libcamera version information (from libcamera/version.h) and runtime version string.
use std::ffi::CStr;

use libcamera_sys::{
    libcamera_version_string, LIBCAMERA_VERSION_MAJOR, LIBCAMERA_VERSION_MINOR, LIBCAMERA_VERSION_PATCH,
};

/// Compile-time libcamera version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Version of libcamera headers linked at build time.
pub const VERSION: Version = Version {
    major: LIBCAMERA_VERSION_MAJOR,
    minor: LIBCAMERA_VERSION_MINOR,
    patch: LIBCAMERA_VERSION_PATCH,
};

impl Version {
    /// Returns the compile-time version as a struct.
    pub const fn current() -> Version {
        VERSION
    }
}

/// Runtime libcamera version string reported by `CameraManager::version()`.
///
/// This does not require creating or starting a `CameraManager`.
pub fn runtime_version() -> &'static str {
    unsafe { CStr::from_ptr(libcamera_version_string()) }.to_str().unwrap()
}
