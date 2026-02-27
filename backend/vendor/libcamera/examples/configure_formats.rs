//! Configure a stream using a common pixel format (NV12 if available) and inspect the result.
use libcamera::{camera_manager::CameraManager, geometry::Size, pixel_format::PixelFormat, stream::StreamRole};

fn main() {
    let mgr = CameraManager::new().expect("camera manager");
    let cameras = mgr.cameras();
    let Some(cam) = cameras.get(0) else {
        eprintln!("No cameras available");
        return;
    };
    let mut cam = cam.acquire().expect("acquire");

    let mut config = cam
        .generate_configuration(&[StreamRole::ViewFinder])
        .expect("generate config");

    if let Some(mut cfg) = config.get_mut(0) {
        let nv12 = PixelFormat::parse("NV12").unwrap_or_else(|| {
            eprintln!("NV12 not available; leaving pixel format unchanged");
            cfg.get_pixel_format()
        });
        cfg.set_pixel_format(nv12);
        cfg.set_size(Size::new(640, 480));
    }

    let status = config.validate();
    println!("validate status: {status:?}");
    cam.configure(&mut config).expect("configure");
    if let Some(cfg) = config.get(0) {
        println!(
            "configured: {:?}, stride={} frame_size={}",
            cfg,
            cfg.get_stride(),
            cfg.get_frame_size()
        );
    }
}
