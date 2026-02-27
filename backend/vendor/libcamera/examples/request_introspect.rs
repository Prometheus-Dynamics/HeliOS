//! Introspect request buffer map and to_string().
use std::io;

use libcamera::{
    camera_manager::CameraManager, framebuffer_allocator::FrameBufferAllocator, request::ReuseFlag, stream::StreamRole,
};

fn main() -> io::Result<()> {
    let mgr = CameraManager::new().expect("camera manager");
    let cameras = mgr.cameras();
    let mut cam = cameras.iter().next().expect("no cameras").acquire().unwrap();

    let mut cfg = cam.generate_configuration(&[StreamRole::ViewFinder]).expect("config");
    cfg.validate();
    cam.configure(&mut cfg).expect("configure");
    let stream = cfg.get(0).and_then(|c| c.stream()).expect("stream");

    let mut alloc = FrameBufferAllocator::new(&cam);
    let mut bufs = alloc.alloc(&stream).expect("alloc buffers");
    let buf = bufs.pop().expect("at least one buffer");

    let mut req = cam.create_request(Some(99)).expect("request");
    req.add_buffer(&stream, buf).expect("attach buffer");

    println!("Request before queue:");
    println!("has_pending_buffers = {}", req.has_pending_buffers());
    println!("find_buffer is Some: {}", req.find_buffer(&stream).is_some());
    println!("to_string: {}", req.to_string_repr());

    println!("Buffer map entries:");
    for (_s, fb_ptr) in req.buffers_iter() {
        println!("  buffer ptr {:?}", fb_ptr);
    }

    // Demonstrate reuse without queueing.
    req.reuse(ReuseFlag::REUSE_BUFFERS);
    println!("After reuse, has_pending_buffers = {}", req.has_pending_buffers());

    // Drop request/buffer without queueing to keep example simple.
    Ok(())
}
