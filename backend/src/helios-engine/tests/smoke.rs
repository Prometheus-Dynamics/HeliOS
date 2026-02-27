#[test]
fn virtual_capture_starts() {
    use helios_engine::capture::{default_virtual_device, start_from_config, CaptureConfig};

    let device = default_virtual_device();
    let backend = device.backends.first().expect("virtual backend present");
    let mode = backend.descriptor.modes.first().expect("virtual mode present").id.clone();
    let config = CaptureConfig { device_keys: vec![], backend: backend.kind, handle: backend.handle.clone(), mode, target_fps: None, interval: None, controls: vec![], enable_tdn_output: false };
    let devices = vec![device];
    let handle = start_from_config(&config, &devices).expect("capture should start");
    let _metrics = handle.metrics();
    handle.stop();
}
