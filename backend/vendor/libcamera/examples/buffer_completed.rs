//! Demonstrates listening for per-buffer completion and disconnected callbacks.
use std::{sync::mpsc, time::Duration};

use libcamera::{
    camera_manager::CameraManager, framebuffer_allocator::FrameBufferAllocator, request::ReuseFlag, stream::StreamRole,
};

fn main() {
    let mgr = CameraManager::new().expect("camera manager");
    let cameras = mgr.cameras();
    let mut cam = cameras.iter().next().expect("no cameras").acquire().unwrap();

    let (tx_req, rx_req) = mpsc::channel();
    let (tx_buf, rx_buf) = mpsc::channel();
    let (tx_disc, rx_disc) = mpsc::channel();

    cam.on_request_completed(move |req| {
        tx_req.send(req).unwrap();
    });

    cam.on_buffer_completed(move |req, stream| {
        // Buffer cookies/metadata can be inspected here; print sequence for demo.
        tx_buf.send((req.sequence(), stream)).unwrap();
        // For reuse, do nothing; the request will complete later.
    });

    cam.on_disconnected(move || {
        let _ = tx_disc.send(());
    });

    // Configure a simple viewfinder stream.
    let roles = [StreamRole::ViewFinder];
    let mut cfg = cam.generate_configuration(&roles).expect("config");
    cfg.validate();
    cam.configure(&mut cfg).expect("configure");

    let stream = cfg.get(0).and_then(|c| c.stream()).expect("stream");
    let mut alloc = FrameBufferAllocator::new(&cam);
    let buffers = alloc.alloc(&stream).expect("alloc");

    let mut reqs = buffers
        .into_iter()
        .enumerate()
        .map(|(i, buf)| {
            let mut req = cam.create_request(Some(i as u64)).unwrap();
            req.add_buffer(&stream, buf).unwrap();
            req
        })
        .collect::<Vec<_>>();

    cam.start(None).expect("start");
    for req in reqs.drain(..) {
        cam.queue_request(req).map_err(|(_, e)| e).unwrap();
    }

    let mut completed = 0;
    while completed < 5 {
        if let Ok((seq, _stream)) = rx_buf.recv_timeout(Duration::from_secs(2)) {
            println!("bufferCompleted for request seq {}", seq);
        }
        if let Ok(mut req) = rx_req.recv_timeout(Duration::from_millis(500)) {
            println!("requestCompleted seq {}", req.sequence());
            req.reuse(ReuseFlag::REUSE_BUFFERS);
            cam.queue_request(req).map_err(|(_, e)| e).unwrap();
            completed += 1;
        }
        if rx_disc.try_recv().is_ok() {
            println!("Camera disconnected");
            break;
        }
    }

    cam.stop().ok();
    println!("Done.");
}
