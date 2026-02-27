//! Demonstrates constructing an OwnedFrameBuffer from user-provided DMABUFs and using cookies.
use std::{
    ffi::CString,
    io,
    os::fd::{FromRawFd, OwnedFd},
};

use libc::{c_char, ftruncate};
use libcamera::framebuffer::{AsFrameBuffer, FrameBufferPlane, OwnedFrameBuffer};

fn create_memfd(name: &str, size: usize) -> io::Result<OwnedFd> {
    // Safe: passing null-terminated name and flags, checked for errors.
    let fd = unsafe { libc::memfd_create(CString::new(name).unwrap().as_ptr() as *const c_char, libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let ret = unsafe { ftruncate(fd, size as i64) };
    if ret < 0 {
        unsafe { libc::close(fd) };
        return Err(io::Error::last_os_error());
    }

    // Safety: fd is valid and now owned by OwnedFd.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn main() -> io::Result<()> {
    const SIZE: usize = 1024 * 1024;
    let plane_fd = create_memfd("libcamera_owned_fb", SIZE)?;

    let fb = OwnedFrameBuffer::new(
        vec![FrameBufferPlane {
            fd: plane_fd,
            offset: 0,
            length: SIZE as u32,
        }],
        Some(42),
    )?;

    println!("Initial cookie: {}", fb.cookie());
    fb.set_cookie(1337);
    println!("Updated cookie: {}", fb.cookie());
    println!("Plane descriptors: {:?}", fb.planes());

    Ok(())
}
