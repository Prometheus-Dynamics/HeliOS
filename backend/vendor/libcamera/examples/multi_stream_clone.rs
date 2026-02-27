//! Demonstrates building a multi-stream configuration by cloning a validated stream config.
use libcamera::{camera_manager::CameraManager, stream::StreamRole};

fn main() {
    let cm = CameraManager::new().expect("camera manager");
    let cams = cm.cameras();
    let Some(cam) = cams.get(0) else {
        eprintln!("No cameras available.");
        return;
    };

    let mut config = cam
        .generate_configuration(&[StreamRole::ViewFinder])
        .expect("generate base config");
    let template_cfg = cam
        .generate_configuration(&[StreamRole::ViewFinder])
        .expect("generate template config");

    // Clone the first stream config to append another stream (for example, a second viewfinder).
    let base = template_cfg.get(0).expect("base stream");
    let added = config.add_configurations_like(&[base.value()]);
    println!("Appended {} cloned stream(s).", added.len());

    let status = config.validate();
    println!("validate() => {status:?}");
    println!("Final configuration: {}", config.to_string_repr());
}
