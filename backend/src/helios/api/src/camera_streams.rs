//! The board's camera stream list (`/run/board/camera-streams.json`): each camera's MJPEG preview
//! (`GET /v1/cameras/{id}/preview`), as the Raze device package's identity advertises it and Atlas
//! shows it (`[{"name", "url": ":<port>/<path>"}]`, the URL relative to the address Atlas reached
//! the board at). Rewritten when the cameras change; the board's identity refresh picks it up.
//! HeliOS draws its pipeline outputs over the frames in the UI, so each camera has one stream.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;

use crate::{SharedState, routes::cameras};

/// Where the board reads the stream list (`HELIOS_CAMERA_STREAMS_FILE` overrides it).
const DEFAULT_PATH: &str = "/run/board/camera-streams.json";
/// How often the cameras are looked up in Orion.
const SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// One stream as the board's identity lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CameraStream {
    pub name: String,
    pub url: String,
}

/// The stream file's path: `HELIOS_CAMERA_STREAMS_FILE`, else the board's.
pub fn path() -> PathBuf {
    std::env::var_os("HELIOS_CAMERA_STREAMS_FILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
}

/// The stream of each `(id, name)` camera on the API's `port`.
pub fn streams<'a>(cameras: impl IntoIterator<Item = (&'a str, &'a str)>, port: u16) -> Vec<CameraStream> {
    cameras.into_iter().map(|(id, name)| CameraStream { name: name.to_string(), url: format!(":{port}/v1/cameras/{id}/preview") }).collect()
}

/// Keeps the stream file in step with the cameras that serve frames. Off a board (no directory
/// for the file) it writes nothing.
pub fn spawn(state: SharedState, path: PathBuf) -> tokio::task::JoinHandle<()> {
    let port = state.config.bind.port();
    tokio::spawn(async move {
        let mut written: Option<Vec<CameraStream>> = None;
        let mut interval = tokio::time::interval(SCAN_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if !path.parent().is_some_and(Path::is_dir) {
                continue;
            }
            let Ok(view) = state.orion.view().await else {
                continue;
            };
            let list = streams(cameras::streaming_cameras(&view).iter().map(|(id, name)| (id.as_str(), name.as_str())), port);
            if written.as_ref() == Some(&list) {
                continue;
            }
            match write(&path, &list) {
                Ok(()) => written = Some(list),
                Err(error) => tracing::warn!(path = %path.display(), %error, "camera stream list not written"),
            }
        }
    })
}

/// Writes `list` to `path` whole (a temporary file renamed over it), so a reader never sees half.
fn write(path: &Path, list: &[CameraStream]) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(list).map_err(std::io::Error::other)?)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_camera_is_its_preview_on_the_api_port() {
        let list = streams([("camera.raze-front.cam0", "front"), ("camera.usb.0", "Arducam \"B\"")], 5800);
        assert_eq!(
            serde_json::to_value(&list).expect("json"),
            serde_json::json!([
                { "name": "front", "url": ":5800/v1/cameras/camera.raze-front.cam0/preview" },
                { "name": "Arducam \"B\"", "url": ":5800/v1/cameras/camera.usb.0/preview" },
            ])
        );
    }

    #[test]
    fn the_file_is_replaced_whole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("camera-streams.json");
        write(&path, &streams([("camera.a", "a")], 5800)).expect("write");
        write(&path, &[]).expect("rewrite");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "[]");
        assert!(!path.with_extension("json.tmp").exists());
    }
}
