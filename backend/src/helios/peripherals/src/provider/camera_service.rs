use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use orion::control_plane::CustomEndpointScheme;
use styx::{
    BackendHandle, ProbedBackend, ProbedDevice,
    ipc::{CameraService, CameraServiceHandle},
};
use tracing::{info, warn};

use crate::{
    config::PeripheralConfig,
    model::{ResourceDescriptor, ResourceKind, ResourceStatus},
    resources::probe_capture_devices,
};

/// Scheme of the endpoint a camera resource advertises for its Styx `CameraService` socket.
pub const STYX_FRAMES_ENDPOINT_SCHEME: &str = "styx-frames+unix";
const CAMERA_SERVICE_SOCKET_SUFFIX: &str = ".styx.sock";
const CAMERA_SERVICE_SOCKET_MODE: u32 = 0o660;

/// `styx-frames+unix://<absolute socket path>`: where Styx `FrameClient`s reach a camera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyxFramesEndpoint {
    pub socket_path: PathBuf,
}

impl CustomEndpointScheme for StyxFramesEndpoint {
    const SCHEME: &'static str = STYX_FRAMES_ENDPOINT_SCHEME;
    const TYPE_NAME: &'static str = "styx frames";

    fn from_payload(payload: &str) -> Option<Self> {
        let socket_path = PathBuf::from(payload);
        socket_path.is_absolute().then_some(Self { socket_path })
    }
}

pub fn serves_camera_frames(resource: &ResourceDescriptor) -> bool {
    resource.kind == ResourceKind::CaptureDevice && resource.status != ResourceStatus::Missing && resource.label("capture_role").unwrap_or("camera_stream") == "camera_stream"
}

pub fn camera_service_socket_path(stream_dir: &Path, resource: &ResourceDescriptor) -> PathBuf {
    let stream_dir = std::path::absolute(stream_dir).unwrap_or_else(|_| stream_dir.to_path_buf());
    stream_dir.join(format!("{}{CAMERA_SERVICE_SOCKET_SUFFIX}", socket_file_stem(resource.id.as_str())))
}

/// One running Styx `CameraService` per served camera resource, keyed by resource id. Dropping a
/// handle stops its service and removes its socket.
#[derive(Default)]
pub struct CameraServices {
    services: BTreeMap<String, CameraServiceHandle>,
}

impl CameraServices {
    /// Starts a service for each camera resource that has none and stops services whose camera is
    /// gone, so hotplug refreshes keep the set current.
    pub fn sync(&mut self, config: &PeripheralConfig, resources: &[ResourceDescriptor]) {
        self.sync_with_probe(config, resources, probe_capture_devices);
    }

    fn sync_with_probe(&mut self, config: &PeripheralConfig, resources: &[ResourceDescriptor], probe: impl FnOnce() -> Vec<ProbedDevice>) {
        let wanted = resources.iter().filter(|resource| serves_camera_frames(resource)).map(|resource| resource.id.as_str()).collect::<BTreeSet<_>>();
        self.services.retain(|resource_id, handle| {
            let keep = wanted.contains(resource_id.as_str());
            if !keep {
                info!(resource_id = resource_id.as_str(), socket = %handle.path().display(), "stopping camera service");
            }
            keep
        });

        let missing = resources.iter().filter(|resource| serves_camera_frames(resource) && !self.services.contains_key(resource.id.as_str())).collect::<Vec<_>>();
        if missing.is_empty() {
            return;
        }
        if let Err(error) = fs::create_dir_all(&config.stream_dir) {
            warn!(path = %config.stream_dir.display(), error = %error, "failed to create camera service socket directory");
            return;
        }
        let devices = probe();
        for resource in missing {
            let Some(device) = find_capture_device(&devices, resource) else {
                warn!(resource_id = resource.id.as_str(), display_name = %resource.display_name, "no Styx capture device matched camera resource");
                continue;
            };
            let socket_path = camera_service_socket_path(&config.stream_dir, resource);
            match CameraService::new(device.clone()).pause_when_idle(Duration::from_millis(config.camera_idle_pause_ms)).socket_mode(CAMERA_SERVICE_SOCKET_MODE).serve(&socket_path) {
                Ok(handle) => {
                    info!(resource_id = resource.id.as_str(), socket = %socket_path.display(), "camera service started");
                    self.services.insert(resource.id.as_str().to_string(), handle);
                }
                Err(error) => warn!(resource_id = resource.id.as_str(), socket = %socket_path.display(), error = %error, "failed to start camera service"),
            }
        }
    }
}

fn find_capture_device<'a>(devices: &'a [ProbedDevice], resource: &ResourceDescriptor) -> Option<&'a ProbedDevice> {
    devices.iter().find(|device| device.identity.display == resource.display_name.as_ref() || device.backends.iter().any(|backend| backend_matches_resource(backend, resource)))
}

fn backend_matches_resource(backend: &ProbedBackend, resource: &ResourceDescriptor) -> bool {
    let dev_endpoint = resource.endpoint("dev");
    match (&backend.handle, dev_endpoint) {
        (BackendHandle::V4l2 { path }, Some(dev)) => path == dev,
        _ => backend.properties.iter().any(|(key, value)| (key == "devnode" || key == "device") && Some(value.as_str()) == dev_endpoint),
    }
}

fn socket_file_stem(resource_id: &str) -> String {
    resource_id.chars().map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') { ch } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::NodeId, resources::ResourceBuilder};

    #[test]
    fn camera_service_socket_path_is_absolute_and_named_after_the_resource() {
        let resource = ResourceBuilder::new(NodeId::new("node1"), ResourceKind::CaptureDevice, "video0", "Camera 0").expect("camera").build();
        let path = camera_service_socket_path(Path::new("/run/helios/streams"), &resource);
        assert_eq!(path, PathBuf::from("/run/helios/streams/capture_device_node1_video0.styx.sock"));
        let relative = camera_service_socket_path(Path::new("streams"), &resource);
        assert!(relative.is_absolute());
    }

    #[test]
    fn styx_frames_endpoint_round_trips_absolute_socket_paths() {
        let endpoint = StyxFramesEndpoint::endpoint_string("/run/helios/streams/cam.styx.sock");
        assert_eq!(endpoint, "styx-frames+unix:///run/helios/streams/cam.styx.sock");
        assert_eq!(StyxFramesEndpoint::from_payload("/run/helios/streams/cam.styx.sock").map(|endpoint| endpoint.socket_path), Some(PathBuf::from("/run/helios/streams/cam.styx.sock")));
        assert_eq!(StyxFramesEndpoint::from_payload("relative.sock"), None);
    }

    #[test]
    fn only_present_camera_stream_resources_serve_frames() {
        let owner = NodeId::new("node1");
        let camera = ResourceBuilder::new(owner.clone(), ResourceKind::CaptureDevice, "video0", "Camera 0").expect("camera").build();
        let missing = ResourceBuilder::new(owner.clone(), ResourceKind::CaptureDevice, "video1", "Camera 1").expect("camera").status(ResourceStatus::Missing).build();
        let imu = ResourceBuilder::new(owner, ResourceKind::Fan, "imu", "IMU").expect("imu").build();
        assert!(serves_camera_frames(&camera));
        assert!(!serves_camera_frames(&missing));
        assert!(!serves_camera_frames(&imu));
    }

    #[test]
    fn sync_serves_cameras_and_stops_when_they_disappear() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = PeripheralConfig { stream_dir: dir.path().join("streams"), ..PeripheralConfig::default() };
        let resource = ResourceBuilder::new(NodeId::new("node1"), ResourceKind::CaptureDevice, "video0", "Camera 0").expect("camera").endpoint("dev", "/dev/video0").build();
        let device = ProbedDevice {
            identity: styx::DeviceIdentity { display: "Camera 0".into(), keys: vec!["/dev/video0".into()] },
            backends: vec![ProbedBackend {
                kind: styx::BackendKind::V4l2,
                handle: BackendHandle::V4l2 { path: "/dev/video0".into() },
                descriptor: styx::capture::CaptureDescriptor { modes: Vec::new(), controls: Vec::new() },
                properties: Vec::new(),
            }],
        };
        let socket_path = camera_service_socket_path(&config.stream_dir, &resource);
        let mut services = CameraServices::default();

        services.sync_with_probe(&config, std::slice::from_ref(&resource), || vec![device]);
        assert!(services.services.contains_key(resource.id.as_str()));
        assert!(socket_path.exists());

        services.sync_with_probe(&config, &[], || panic!("no camera is missing, nothing to probe"));
        assert!(services.services.is_empty());
        assert!(!socket_path.exists());
    }
}
