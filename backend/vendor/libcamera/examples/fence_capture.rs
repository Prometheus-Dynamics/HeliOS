//! Demonstrates using acquire fences when queuing requests and releasing fences from framebuffers.
use std::{os::fd::AsRawFd, time::Duration};

use libc::{poll, pollfd, POLLIN};
use libcamera::{
    camera_manager::CameraManager,
    fence::Fence,
    framebuffer::AsFrameBuffer,
    framebuffer_allocator::{FrameBuffer, FrameBufferAllocator},
    framebuffer_map::MemoryMappedFrameBuffer,
    logging::LoggingLevel,
    request::ReuseFlag,
    stream::StreamRole,
};

fn main() {
    let mgr = CameraManager::new().expect("manager");
    let cameras = mgr.cameras();
    let mut cam = cameras.iter().next().expect("no cameras").acquire().unwrap();
    // Set global logging to reduce noise.
    // Note: log_set_level is on CameraManager.
    mgr.log_set_level("Camera", LoggingLevel::Error);

    // Try roles in order until we get a usable stream.
    let roles_to_try = [
        StreamRole::VideoRecording,
        StreamRole::ViewFinder,
        StreamRole::StillCapture,
    ];
    let (mut cfgs, _role) = cam
        .generate_first_supported_configuration(&roles_to_try)
        .unwrap_or_else(|| {
            eprintln!("No usable stream found for any role; exiting.");
            std::process::exit(1);
        });
    cfgs.validate();

    cam.configure(&mut cfgs).expect("configure");

    let mut alloc = FrameBufferAllocator::new(&cam);
    let stream = cfgs.get(0).and_then(|cfg| cfg.stream()).expect("stream");
    let buffers = alloc.alloc(&stream).expect("alloc buffers");
    println!("Allocated {} buffers", buffers.len());

    // Wrap buffers to get mmap access
    let buffers = buffers
        .into_iter()
        .map(|buf| MemoryMappedFrameBuffer::new(buf).unwrap())
        .collect::<Vec<_>>();

    // Create requests with fence-capable path (no acquire fences in this example).
    let mut reqs = buffers
        .into_iter()
        .enumerate()
        .map(|(i, buf)| {
            let mut req = cam.create_request(Some(i as u64)).unwrap();
            req.add_buffer_with_fence(&stream, buf, None::<Fence>).unwrap();
            req
        })
        .collect::<Vec<_>>();

    // Subscribe to completion
    let (tx, rx) = std::sync::mpsc::channel();
    cam.on_request_completed(move |req| {
        tx.send(req).unwrap();
    });

    cam.start(None).unwrap();

    for req in reqs.drain(..) {
        cam.queue_request(req).map_err(|(_, e)| e).unwrap();
    }

    // Recycle buffers a few times to show waiting on release fences before reuse.
    for _ in 0..5 {
        println!("Waiting for camera request execution");
        let mut req = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                eprintln!("Timed out waiting for a request; camera may not be delivering frames.");
                break;
            }
        };
        println!("Request {} completed", req.sequence());
        let fb: &MemoryMappedFrameBuffer<FrameBuffer> = req.buffer(&stream).unwrap();
        println!("Metadata: {:#?}", fb.metadata());

        if let Some(fence) = fb.release_fence() {
            let fence_fd = fence.into_owned_fd().expect("dup release fence");
            let fd = fence_fd.as_raw_fd();
            let mut pfd = [pollfd {
                fd,
                events: POLLIN,
                revents: 0,
            }];
            println!("Waiting on release fence fd {}", fd);
            unsafe {
                let _ = poll(pfd.as_mut_ptr(), 1, 2000);
            }
        } else {
            println!("No release fence for this buffer");
        }

        // Reuse the same request to demonstrate fence-aware recycling.
        req.reuse(ReuseFlag::REUSE_BUFFERS);
        cam.queue_request(req).map_err(|(_, e)| e).unwrap();
    }
}
