//! Fetch a camera by ID using CameraManager::get and print its model property.
use libcamera::{camera_manager::CameraManager, properties::Model};

fn main() {
    let mgr = CameraManager::new().expect("camera manager");
    let cameras = mgr.cameras();

    let first = match cameras.iter().next() {
        Some(cam) => cam,
        None => {
            eprintln!("No cameras found");
            return;
        }
    };

    let id = first.id().to_string();
    println!("First camera id: {}", id);

    let cam = match mgr.get(&id) {
        Some(cam) => cam,
        None => {
            eprintln!("Camera with id {} not found via get()", id);
            return;
        }
    };

    match cam.properties().get::<Model>() {
        Ok(model) => println!("Model: {}", model.0),
        Err(e) => println!("Model property not available: {e}"),
    }
}
