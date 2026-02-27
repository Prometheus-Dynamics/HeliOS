//! Import a DMABUF framebuffer (memfd) and capture a single frame.
use std::{
    convert::TryInto,
    io,
    os::fd::{FromRawFd, OwnedFd},
    time::Duration,
};

use libcamera::{
    camera_manager::CameraManager,
    framebuffer::{AsFrameBuffer, FrameBufferPlane, OwnedFrameBuffer},
    framebuffer_allocator::FrameBufferAllocator,
    request::ReuseFlag,
    stream::StreamRole,
};

fn main() -> io::Result<()> {
    let mgr = CameraManager::new().expect("manager");
    let cameras = mgr.cameras();
    let mut cam = cameras.iter().next().expect("no cameras").acquire().unwrap();

    // Configure viewfinder stream for simplicity.
    let mut cfg = cam.generate_configuration(&[StreamRole::ViewFinder]).expect("config");
    cfg.validate();

    cam.configure(&mut cfg).expect("configure");
    let stream = cfg.get(0).and_then(|c| c.stream()).expect("stream");

    // Allocate a native buffer, then duplicate its DMABUF fds to create an imported framebuffer.
    let mut alloc = FrameBufferAllocator::new(&cam);
    let mut bufs = alloc.alloc(&stream).expect("alloc buffer");
    let src = bufs.pop().expect("at least one buffer");
    let planes = src.planes();

    let mut plane_desc = Vec::new();
    for p in planes.into_iter() {
        let fd = unsafe { libc::dup(p.fd()) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let len: u32 = p
            .len()
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "plane length overflow"))?;
        plane_desc.push(FrameBufferPlane {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            offset: p.offset().unwrap_or(0) as u32,
            length: len,
        });
    }

    let fb = OwnedFrameBuffer::new(plane_desc, Some(123))?;

    let mut req = cam.create_request(Some(1)).expect("request");
    req.add_buffer(&stream, fb).expect("attach buffer");

    let (tx, rx) = std::sync::mpsc::channel();
    cam.on_request_completed(move |r| {
        let _ = tx.send(r);
    });

    cam.start(None)?;
    cam.queue_request(req).map_err(|(_, e)| e)?;

    if let Ok(mut r) = rx.recv_timeout(Duration::from_secs(3)) {
        println!("Captured request seq {}", r.sequence());
        if let Some(buf) = r.buffer_mut::<OwnedFrameBuffer>(&stream) {
            println!("Cookie: {}", buf.cookie());
            if let Some(meta) = buf.metadata() {
                println!("Metadata: {:?}", meta);
            }
        }
        // Ensure fences cleaned up before dropping
        r.reuse(ReuseFlag::REUSE_BUFFERS);
    } else {
        eprintln!("Timed out waiting for capture");
    }

    cam.stop().ok();
    Ok(())
}
