use libcamera::camera_manager::CameraManager;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build a manager without auto-starting, then start it.
    let mut mgr = CameraManager::new_unstarted()?;
    println!("started? {}", mgr.is_started());
    mgr.start()?;
    println!("started after start()? {}", mgr.is_started());

    // Hold on to a camera to show try_stop() refusing to stop while handles exist.
    let first_camera_id = {
        let cameras = mgr.cameras();
        println!("found {} cameras", cameras.len());
        if let Some(cam) = cameras.get(0) {
            println!("first camera id: {}", cam.id());
            match mgr.try_stop() {
                Ok(()) => println!("unexpectedly stopped with live camera"),
                Err(e) => println!("try_stop() while camera alive -> {}", e),
            }
            Some(cam.id().to_string())
        } else {
            None
        }
        // cameras drops here; any tracked handles go away too.
    };

    // Now it is safe to stop.
    mgr.try_stop()?;
    println!("stopped cleanly, started? {}", mgr.is_started());

    // Demonstrate restart: stop (succeeds because no cameras alive) then start again.
    mgr.restart()?;
    println!("restarted, started? {}", mgr.is_started());

    if let Some(id) = first_camera_id {
        if let Some(cam) = mgr.get(&id) {
            println!("camera {} still present after restart", cam.id());
        } else {
            println!("camera {} not found after restart", id);
        }
    }

    Ok(())
}
