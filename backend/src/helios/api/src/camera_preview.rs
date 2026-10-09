//! Camera previews for the UI: small JPEG frames from each camera's Styx camera service, served
//! as MJPEG (`GET /v1/cameras/{id}/preview`) or WebSocket messages
//! (`GET /v1/cameras/{id}/preview/ws`).
//!
//! Each camera gets one `styx::preview::Preview::from_service` (Styx `docs/preview.md`), made on
//! the first viewer and shared by every viewer after it. It is a **low-priority** client of the
//! camera service: the service plans the vision client (helios-engine) as if the preview were not
//! there, gives the preview the ISP's free second output only when that changes nothing for it,
//! else a share of the vision client's frames, and never restarts its capture. The preview is
//! connected only while someone watches and disconnects `idle_disconnect` (5 s) after the last
//! viewer left; it encodes at most `max_fps` frames, drops rather than queues, and runs at
//! lower priority (`nice` 10). The API holds no camera frames itself.

use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc};

use styx::{
    ipc::FrameClient,
    preview::{Preview, PreviewConfig},
};
use tokio::sync::Mutex;

use crate::error::{ApiError, ApiResult};

/// Preview size, rate and quality (`HELIOS_API_PREVIEW_SIZE`, `_FPS`, `_QUALITY`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewSettings {
    /// The largest preview; frames are scaled to fit, keeping their aspect ratio, never up.
    pub width: u32,
    pub height: u32,
    pub max_fps: f32,
    /// JPEG quality, 1 to 100.
    pub quality: u8,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self { width: 640, height: 400, max_fps: 15.0, quality: 70 }
    }
}

impl PreviewSettings {
    /// `640x400`.
    pub fn parse_size(value: &str) -> Option<(u32, u32)> {
        let (width, height) = value.trim().split_once(['x', 'X'])?;
        let (width, height) = (width.trim().parse().ok()?, height.trim().parse().ok()?);
        (16..=4096).contains(&width).then_some(())?;
        (16..=4096).contains(&height).then_some(())?;
        Some((width, height))
    }

    fn config(&self, name: &str) -> PreviewConfig {
        PreviewConfig::new().name(name).size(self.width, self.height).max_fps(self.max_fps).quality(self.quality)
    }
}

struct Entry {
    socket: PathBuf,
    preview: Arc<Preview>,
}

/// One preview per camera, made on the first viewer.
pub struct CameraPreviews {
    settings: PreviewSettings,
    previews: Mutex<HashMap<String, Entry>>,
}

impl CameraPreviews {
    pub fn new(settings: PreviewSettings) -> Self {
        Self { settings, previews: Mutex::new(HashMap::new()) }
    }

    pub fn settings(&self) -> PreviewSettings {
        self.settings
    }

    /// The preview of camera `id`, served by the camera service at `socket` (made now if there
    /// is none, or if the camera's service moved to another socket). Never waits for the
    /// service: the preview connects in the background while someone watches.
    pub async fn get(&self, id: &str, socket: &Path) -> ApiResult<Arc<Preview>> {
        let mut previews = self.previews.lock().await;
        if let Some(entry) = previews.get(id)
            && entry.socket == socket
        {
            return Ok(entry.preview.clone());
        }
        let preview = Preview::from_service(FrameClient::options(socket), self.settings.config(id)).map_err(|error| ApiError::internal(format!("camera preview for {id}: {error}")))?;
        let preview = Arc::new(preview);
        // A replaced preview stops once its last viewer is gone.
        previews.insert(id.to_string(), Entry { socket: socket.to_path_buf(), preview: preview.clone() });
        Ok(preview)
    }

    /// Forget previews of cameras that are gone (`keep`: the cameras there are now).
    pub async fn retain(&self, keep: impl Fn(&str) -> bool) {
        self.previews.lock().await.retain(|id, _| keep(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(PreviewSettings::parse_size("640x400"), Some((640, 400)));
        assert_eq!(PreviewSettings::parse_size(" 320 X 200 "), Some((320, 200)));
        assert_eq!(PreviewSettings::parse_size("640"), None);
        assert_eq!(PreviewSettings::parse_size("0x400"), None);
        assert_eq!(PreviewSettings::parse_size("99999x400"), None);
    }

    #[tokio::test]
    async fn a_preview_is_shared_and_needs_no_service() {
        let previews = CameraPreviews::new(PreviewSettings::default());
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("cam.styx.sock");
        let first = previews.get("cam", &socket).await.expect("preview without a service");
        let again = previews.get("cam", &socket).await.expect("preview");
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(first.config().max_size, (640, 400));
        assert_eq!(first.config().quality, 70);
        let moved = previews.get("cam", &dir.path().join("other.sock")).await.expect("preview");
        assert!(!Arc::ptr_eq(&first, &moved));
        previews.retain(|_| false).await;
        assert!(previews.previews.lock().await.is_empty());
    }
}
