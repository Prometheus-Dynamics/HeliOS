//! Introspect a request and run a single capture to observe live status.
use std::time::Duration;

use libcamera::{
    camera_manager::CameraManager, framebuffer_allocator::FrameBufferAllocator, request::ReuseFlag, stream::StreamRole,
};

fn main() {
    let mgr = CameraManager::new().expect("camera manager");
    let cameras = mgr.cameras();
    let mut cam = cameras.iter().next().expect("no cameras").acquire().unwrap();

    let mut cfg = cam.generate_configuration(&[StreamRole::ViewFinder]).expect("config");
    cfg.validate();
    cam.configure(&mut cfg).expect("configure");
    let stream = cfg.get(0).and_then(|c| c.stream()).expect("stream");

    let mut alloc = FrameBufferAllocator::new(&cam);
    let buf = alloc.alloc(&stream).expect("alloc").pop().expect("buffer");

    let mut req = cam.create_request(Some(99)).expect("request");
    req.add_buffer(&stream, buf).expect("attach buffer");

    println!("Before queue: {}", req.to_string_repr());
    for (_s, fb_ptr) in req.buffers_iter() {
        println!("  buf_ptr {:?}", fb_ptr);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    cam.on_request_completed(move |r| {
        let _ = tx.send(r);
    });

    cam.start(None).expect("start");
    cam.queue_request(req).map_err(|(_, e)| e).expect("queue");

    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(mut r) => {
            println!("Completed: {}", r.to_string_repr());
            println!("has_pending_buffers = {}", r.has_pending_buffers());
            r.reuse(ReuseFlag::REUSE_BUFFERS);
        }
        Err(_) => eprintln!("Timed out waiting for request completion"),
    }

    cam.stop().ok();
}
