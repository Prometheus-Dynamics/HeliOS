//! Demonstrates reading back sensor configuration after validation.
use libcamera::{camera_manager::CameraManager, geometry::Rectangle, stream::StreamRole};

fn main() {
    let mgr = CameraManager::new().expect("camera manager");
    let cameras = mgr.cameras();
    let cam = match cameras.iter().next() {
        Some(c) => c,
        None => {
            eprintln!("No cameras found");
            return;
        }
    };

    let mut cfg = cam.generate_configuration(&[StreamRole::ViewFinder]).expect("config");
    cfg.validate();
    if let Some(stream_cfg) = cfg.get(0) {
        println!("Stream cfg: {}", stream_cfg.to_string_repr());
    }
    println!("Camera cfg: {}", cfg.to_string_repr());

    if let Some(sensor_cfg) = cfg.sensor_configuration() {
        println!("Sensor config valid: {}", sensor_cfg.is_valid());
        println!("Bit depth: {}", sensor_cfg.bit_depth());
        println!("Output size: {:?}", sensor_cfg.output_size());
        println!("Analog crop: {:?}", sensor_cfg.analog_crop());
        println!("Binning: {:?}", sensor_cfg.binning());
        println!("Skipping: {:?}", sensor_cfg.skipping());
    } else {
        println!("No sensor configuration available");
    }

    // Example of setting analog crop then reading it back.
    let mut sensor = cam
        .generate_configuration(&[StreamRole::StillCapture])
        .unwrap()
        .sensor_configuration()
        .unwrap_or_default();
    sensor.set_analog_crop(Rectangle {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    });
    println!("Set analog crop to {:?}", sensor.analog_crop());
}
