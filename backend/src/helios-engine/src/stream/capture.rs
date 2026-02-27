#[cfg(feature = "runtime")]
use super::StreamRunner;
#[cfg(feature = "runtime")]
use crate::capture::{discover_devices, BackendKind, CaptureConfig};
#[cfg(feature = "runtime")]
use styx::prelude::FourCc;

#[cfg(feature = "runtime")]
impl StreamRunner {
    pub(super) fn capture_input_fourcc(&self) -> Option<FourCc> {
        if let Some(cc) = self.capture_fourcc {
            return Some(cc);
        }
        let CaptureConfig { device_keys, backend, mode, .. } = &self.capture_config;
        if *backend == BackendKind::File {
            return Some(FourCc::new(*b"RGBA"));
        }
        let backend = *backend;
        let mode = mode.clone();
        let devices = discover_devices();
        for dev in devices {
            if !device_keys.is_empty() && dev.identity.keys.iter().all(|k| !device_keys.contains(k)) {
                continue;
            }
            let backend = dev.backends.iter().find(|b| b.kind == backend)?;
            let mode = backend.descriptor.modes.iter().find(|m| m.id == mode)?;
            return Some(mode.format.code);
        }
        // Fall back to the requested mode for synthetic backends like netcam/file.
        Some(mode.format.code)
    }
}
