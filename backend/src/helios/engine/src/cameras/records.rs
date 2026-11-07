//! The cameras as Orion resources (`camera.device`), one per camera Styx finds, with its
//! service's `styx-frames+unix://` endpoint when it is served.
//!
//! A camera's resource id is `capture_device_<node>_<local>`, where `<local>` is its device
//! node's file name (`video0`) or, for a camera without one (a native CSI sensor), `camera-` and a
//! hash of its stable name. Saved pipelines bind cameras by this id, so it keeps the form
//! helios-peripherals gave it.

use orion::{
    client::ProviderResource,
    control_plane::{AvailabilityState, HealthState, LeaseState, ProviderRecord, ResourceCapability, ResourceOwnershipMode, ResourceRecord},
    core::{CapabilityId, NodeId, ProviderId, ResourceId},
};
use styx::{BackendHandle, ProbedBackend, ProbedDevice};
use styx_camerad::{Endpoint, StartError};

/// The resource type of a camera.
pub const CAMERA_RESOURCE_TYPE: &str = "camera.device";
/// Scheme of the endpoint a camera advertises for its service's socket.
pub const STYX_FRAMES_ENDPOINT_SCHEME: &str = styx_camerad::TRANSPORT;

pub fn provider_id_for(node_id: &str) -> String {
    format!("provider.cameras.{node_id}")
}

pub fn provider_record(node_id: &str) -> ProviderRecord {
    ProviderRecord::builder(ProviderId::new(provider_id_for(node_id)), NodeId::new(node_id)).resource_type(CAMERA_RESOURCE_TYPE).build()
}

/// One resource per camera in `devices`: served ones carry their `endpoints` entry's socket, and
/// ones whose service failed to start are degraded (with the error as a label).
pub fn camera_records(node_id: &str, devices: &[ProbedDevice], endpoints: &[Endpoint], errors: &[StartError]) -> Vec<ResourceRecord> {
    let provider = ProviderId::new(provider_id_for(node_id));
    devices
        .iter()
        .filter_map(|device| {
            let id = resource_id(node_id, device)?;
            let primary = device.backends.first()?;
            let endpoint = endpoints.iter().find(|endpoint| endpoint.name == device.identity.display && endpoint.keys == device.identity.keys);
            let error = errors.iter().find(|error| error.camera == device.identity.display);
            let mut builder = ProviderResource::new(id, CAMERA_RESOURCE_TYPE, provider.clone())
                .ownership_mode(ResourceOwnershipMode::SharedRead)
                .health(if endpoint.is_some() { HealthState::Healthy } else { HealthState::Degraded })
                .availability(AvailabilityState::Available)
                .lease_state(LeaseState::Unleased)
                .label(format!("helios.display_name={}", device.identity.display))
                .label("helios.kind=capture_device")
                .label("helios.label.capture_role=camera_stream")
                .label(format!("helios.label.styx.backend={}", primary.kind))
                .capability(ResourceCapability::new(CapabilityId::new("capture")).with_detail("styx"))
                .capability(ResourceCapability::new(CapabilityId::new("capture.configurable")).with_detail("styx"));
            if let Some(product) = &device.identity.product {
                builder = builder.label(format!("helios.label.styx.product={product}"));
            }
            if let Some(path) = device_path(primary) {
                builder = builder.endpoint(format!("dev://{path}"));
                if path.starts_with("/dev/video") {
                    builder = builder.endpoint(format!("v4l2://{path}"));
                }
            }
            if let Some(endpoint) = endpoint {
                builder = builder.endpoint(format!("{STYX_FRAMES_ENDPOINT_SCHEME}://{}", endpoint.socket.display()));
            }
            if let Some(error) = error {
                builder = builder.label(format!("helios.camera.error={}", error.error.replace('\n', " ")));
            }
            Some(builder.build())
        })
        .collect()
}

fn resource_id(node_id: &str, device: &ProbedDevice) -> Option<ResourceId> {
    ResourceId::try_new(format!("capture_device_{node_id}_{}", local_name(device))).ok()
}

/// The camera's id segment: its device node's file name, else `camera-` and a hash of its name.
fn local_name(device: &ProbedDevice) -> String {
    if let Some(path) = device.backends.first().and_then(device_path)
        && let Some(name) = std::path::Path::new(path).file_name().and_then(|value| value.to_str())
    {
        return sanitize(name, "camera");
    }
    format!("camera-{}", short_fingerprint(&device.identity.display))
}

/// `DefaultHasher` (SipHash-1-3 with zero keys) of the name, low 32 bits: the hash
/// helios-peripherals used, so ids stay what saved pipelines name.
fn short_fingerprint(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

fn device_path(backend: &ProbedBackend) -> Option<&str> {
    backend.properties.iter().find(|(key, _)| key == "devnode" || key == "device").map(|(_, value)| value.as_str()).or(match &backend.handle {
        BackendHandle::V4l2 { path } => Some(path.as_str()),
        _ => None,
    })
}

/// A name readable as one id segment: ASCII letters and digits in lower case, `/ . _ :` and
/// spaces as one `-` each, everything else dropped; `fallback` when nothing is left.
fn sanitize(value: &str, fallback: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            cleaned.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '/' | '.' | '_' | ':' | ' ') && !cleaned.ends_with('-') {
            cleaned.push('-');
        }
    }
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() { fallback.into() } else { cleaned.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn device(display: &str, handle: BackendHandle, kind: styx::BackendKind, keys: &[&str]) -> ProbedDevice {
        ProbedDevice {
            identity: styx::DeviceIdentity { display: display.into(), product: Some("OV9782".into()), keys: keys.iter().map(|k| k.to_string()).collect() },
            backends: vec![ProbedBackend { kind, handle, descriptor: styx::capture::CaptureDescriptor { modes: Vec::new(), controls: Vec::new() }, properties: Vec::new() }],
        }
    }

    fn v4l2(display: &str, path: &str) -> ProbedDevice {
        device(display, BackendHandle::V4l2 { path: path.into() }, styx::BackendKind::V4l2, &[path])
    }

    fn endpoint(device: &ProbedDevice, socket: &str) -> Endpoint {
        Endpoint {
            name: device.identity.display.clone(),
            product: device.identity.product.clone(),
            keys: device.identity.keys.clone(),
            socket: PathBuf::from(socket),
            transport: styx_camerad::TRANSPORT,
        }
    }

    #[test]
    fn a_served_camera_keeps_its_peripherals_id_and_advertises_its_socket() {
        let camera = v4l2("USB Camera", "/dev/video0");
        let records = camera_records("node1", std::slice::from_ref(&camera), &[endpoint(&camera, "/run/s/dev_video0.sock")], &[]);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.resource_id.as_str(), "capture_device_node1_video0");
        assert_eq!(record.resource_type.as_str(), CAMERA_RESOURCE_TYPE);
        assert_eq!(record.provider_id.as_str(), "provider.cameras.node1");
        assert_eq!(record.health, HealthState::Healthy);
        for label in ["helios.display_name=USB Camera", "helios.kind=capture_device", "helios.label.capture_role=camera_stream", "helios.label.styx.backend=v4l2", "helios.label.styx.product=OV9782"] {
            assert!(record.labels.iter().any(|l| l == label), "{label} in {:?}", record.labels);
        }
        assert!(record.endpoints.iter().any(|e| e == "dev:///dev/video0"));
        assert!(record.endpoints.iter().any(|e| e == "v4l2:///dev/video0"));
        assert!(record.endpoints.iter().any(|e| e == "styx-frames+unix:///run/s/dev_video0.sock"));
    }

    #[test]
    fn a_sensor_without_a_device_node_is_named_by_the_hash_of_its_name() {
        // Raze #1's OV9782, as helios-peripherals named it.
        let sensor = device("ov9782 (kernel driver i2c 10-0060)", BackendHandle::Native { key: "bridge:/dev/v4l-subdev2".into() }, styx::BackendKind::Native, &["sensor:/dev/v4l-subdev2"]);
        let id = resource_id("raze-37bc", &sensor).expect("id");
        assert_eq!(id.as_str(), "capture_device_raze-37bc_camera-2c206880");
    }

    #[test]
    fn a_camera_whose_service_failed_is_degraded_without_a_frames_endpoint() {
        let camera = v4l2("USB Camera", "/dev/video0");
        let error = StartError { camera: "USB Camera".into(), error: "busy\nretry".into() };
        let records = camera_records("node1", &[camera], &[], &[error]);
        assert_eq!(records[0].health, HealthState::Degraded);
        assert!(records[0].endpoints.iter().all(|e| !e.starts_with("styx-frames+unix://")));
        assert!(records[0].labels.iter().any(|l| l == "helios.camera.error=busy retry"));
    }
}
