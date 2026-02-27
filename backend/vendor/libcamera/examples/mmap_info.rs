//! Demonstrates writable mappings and mapped length reporting for OwnedFrameBuffer.
use std::{
    io::Write,
    os::fd::{FromRawFd, OwnedFd},
};

use libcamera::{
    framebuffer::{AsFrameBuffer, FrameBufferPlane, OwnedFrameBuffer},
    framebuffer_map::MemoryMappedFrameBuffer,
};

fn main() {
    // Create a simple memfd-backed plane
    let fd: OwnedFd = unsafe {
        let name = std::ffi::CString::new("libcamera-mmap-info").unwrap();
        let fd = libc::memfd_create(name.as_ptr(), 0);
        if fd < 0 {
            panic!("memfd_create failed: {}", std::io::Error::last_os_error());
        }
        let mut f = std::fs::File::from_raw_fd(fd);
        f.write_all(&vec![0u8; 4096]).expect("write memfd");
        f.into()
    };

    let plane = FrameBufferPlane {
        fd,
        offset: 0,
        length: 4096,
    };

    let fb = OwnedFrameBuffer::new(vec![plane], None).expect("owned framebuffer");
    let mut mapped = MemoryMappedFrameBuffer::new_writable(fb).expect("map");
    println!("is_writable: {}", mapped.is_writable());
    let fd = mapped.planes().into_iter().next().unwrap().fd();
    println!("mapped_len for fd {}: {:?}", fd, mapped.mapped_len(fd));

    // Mutate the buffer through the mapping
    if let Ok(mut planes) = mapped.data_mut() {
        planes[0][0] = 0xAB;
        println!("first byte after write: {:#04x}", planes[0][0]);
    }
}
