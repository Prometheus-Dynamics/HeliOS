use crate::model::ResourceKind;
use crate::resources::{DiscoveryContext, DiscoveryError, DiscoveryProbe, DiscoverySnapshot, ResourceBuilder};
use styx::{BackendHandle, probe_all_with_errors};

#[derive(Default)]
pub struct CaptureProbe;

impl DiscoveryProbe for CaptureProbe {
    fn name(&self) -> &'static str {
        "styx-capture"
    }

    fn discover(&self, context: &DiscoveryContext) -> Result<DiscoverySnapshot, DiscoveryError> {
        let mut result = probe_all_with_errors();
        for error in result.errors {
            tracing::warn!(error = %error, "styx camera probe error");
        }
        let devices = styx::prelude::probe_libcamera();
        for device in devices {
            let device_id = device.id.clone();
            let already_present = result.devices.iter().any(|existing| existing.backends.iter().any(|backend| matches!(&backend.handle, BackendHandle::Libcamera { id } if id == &device_id)));
            if already_present {
                continue;
            }
            result.devices.push(styx::ProbedDevice {
                identity: styx::DeviceIdentity { display: device_id.clone(), keys: vec![device_id.clone()] },
                backends: vec![styx::ProbedBackend {
                    kind: styx::BackendKind::Libcamera,
                    handle: BackendHandle::Libcamera { id: device_id },
                    descriptor: device.descriptor,
                    properties: device.properties,
                }],
            });
        }
        tracing::warn!(devices = result.devices.len(), "styx capture probe discovered devices after merge");
        let mut resources = Vec::new();
        for device in result.devices {
            let local = local_name_for_device(&device);
            let primary_backend =
                device.backends.first().ok_or_else(|| DiscoveryError::ProbeFailed { probe: self.name().into(), message: format!("styx device '{}' has no backends", device.identity.display) })?;
            let mut builder = ResourceBuilder::new(context.local_node_id.clone(), ResourceKind::CaptureDevice, local, device.identity.display.clone())
                .map_err(|error| DiscoveryError::ProbeFailed { probe: self.name().into(), message: error.to_string() })?
                .capability("capture", Some("styx"))
                .capability("capture.configurable", Some("styx"))
                .label("capture_role", "camera_stream")
                .label("styx.backend", backend_kind_name(primary_backend));
            if let Some(path) = device_path(primary_backend) {
                builder = builder.endpoint("dev", path.to_string());
                if path.starts_with("/dev/video") {
                    builder = builder.endpoint("v4l2", path.to_string());
                }
            }
            resources.push(builder.build());
        }
        Ok(DiscoverySnapshot::new(resources))
    }
}

fn local_name_for_device(device: &styx::ProbedDevice) -> String {
    if let Some(path) = device.backends.first().and_then(device_path)
        && let Some(name) = std::path::Path::new(path).file_name().and_then(|value| value.to_str())
    {
        return sanitize_local_component(name);
    }
    format!("camera-{}", short_fingerprint(&device.identity.display))
}

fn sanitize_local_component(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            cleaned.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '/' | '.' | '_' | ':' | ' ') && !cleaned.ends_with('-') {
            cleaned.push('-');
        }
    }
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() { "camera".into() } else { cleaned.into() }
}

fn short_fingerprint(value: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

fn backend_kind_name(backend: &styx::ProbedBackend) -> &'static str {
    match backend.kind {
        styx::BackendKind::V4l2 => "v4l2",
        styx::BackendKind::Libcamera => "libcamera",
        styx::BackendKind::Virtual => "virtual",
        styx::BackendKind::Netcam => "netcam",
        styx::BackendKind::File => "file",
        styx::BackendKind::Simulation => "simulation",
    }
}

fn device_path(backend: &styx::ProbedBackend) -> Option<&str> {
    backend.properties.iter().find(|(candidate, _)| candidate == "devnode" || candidate == "device").map(|(_, value)| value.as_str()).or_else(|| match &backend.handle {
        BackendHandle::V4l2 { path } => Some(path.as_str()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_local_component_normalizes_id_values() {
        assert_eq!(sanitize_local_component("/dev/video0"), "dev-video0");
        assert_eq!(sanitize_local_component("CSI Camera: Front"), "csi-camera-front");
        assert_eq!(sanitize_local_component(""), "camera");
    }
}
