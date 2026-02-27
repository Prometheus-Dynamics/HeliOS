use std::time::Duration;

use libcamera::camera_manager::CameraManager;

fn main() {
    let mut mgr = CameraManager::new().expect("camera manager");
    let rx = mgr.subscribe_hotplug_events();

    println!("Waiting for hotplug events. Press Ctrl+C to exit.");
    loop {
        if let Ok(evt) = rx.recv_timeout(Duration::from_secs(1)) {
            println!("Event: {:?}", evt);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
