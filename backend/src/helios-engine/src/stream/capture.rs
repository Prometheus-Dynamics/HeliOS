#[cfg(feature = "runtime")]
use super::StreamRunner;
#[cfg(feature = "runtime")]
use crate::capture::{discover_devices, find_backend_for_config, BackendKind};
#[cfg(feature = "runtime")]
use styx::prelude::FourCc;

#[cfg(feature = "runtime")]
impl StreamRunner {
    pub(super) fn capture_input_fourcc(&self) -> Option<FourCc> {
        if let Some(cc) = self.capture_fourcc {
            return Some(cc);
        }
        if self.capture_config.backend == BackendKind::File {
            return Some(FourCc::new(*b"RGBA"));
        }
        let devices = discover_devices();
        let mode = self.capture_config.mode.clone();
        let backend = find_backend_for_config(&self.capture_config, &devices)?;
        let mode = backend.descriptor.modes.iter().find(|candidate| candidate.id == mode)?;
        Some(mode.format.code)
    }
}
