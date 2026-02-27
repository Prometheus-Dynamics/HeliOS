use std::{
    io,
    mem::ManuallyDrop,
    os::fd::{FromRawFd, IntoRawFd, OwnedFd},
    ptr::NonNull,
};

use libcamera_sys::*;

/// A wrapper around libcamera::Fence for synchronizing buffer access.
pub struct Fence {
    ptr: NonNull<libcamera_fence_t>,
}

impl Fence {
    /// Create a Fence from an owned file descriptor.
    ///
    /// The fd is consumed; on failure it is closed.
    pub fn from_fd(fd: OwnedFd) -> io::Result<Self> {
        let raw = fd.into_raw_fd();
        let ptr = unsafe { libcamera_fence_from_fd(raw) };
        match NonNull::new(ptr) {
            Some(ptr) => Ok(Self { ptr }),
            None => {
                unsafe { libc::close(raw) };
                Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fence fd"))
            }
        }
    }

    /// Duplicate the fence fd.
    pub fn to_owned_fd(&self) -> io::Result<OwnedFd> {
        let fd = unsafe { libcamera_fence_fd(self.ptr.as_ptr()) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }
    }

    /// Consume the fence and return a duplicated fd.
    pub fn into_owned_fd(self) -> io::Result<OwnedFd> {
        let fd = self.to_owned_fd();
        // Manually drop to avoid double-destroy after forgetting self.
        let ptr = self.ptr;
        let _ = ManuallyDrop::new(self);
        unsafe {
            libcamera_fence_destroy(ptr.as_ptr());
        }
        fd
    }

    pub(crate) fn into_raw(self) -> *mut libcamera_fence_t {
        let ptr = self.ptr.as_ptr();
        std::mem::forget(self);
        ptr
    }

    pub(crate) unsafe fn from_ptr(ptr: *mut libcamera_fence_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }
}

impl Drop for Fence {
    fn drop(&mut self) {
        unsafe { libcamera_fence_destroy(self.ptr.as_ptr()) }
    }
}

unsafe impl Send for Fence {}
