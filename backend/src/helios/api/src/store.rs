//! What only the API owns, persisted under its state directory (`/var/lib/helios/api`, on the
//! data partition, so it survives reboots and OTA updates): the revision history of each
//! pipeline (for rollback), where each camera is mounted on the robot, each camera's
//! calibrations, and the camera control values set through the API.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::Mutex;

use crate::{
    camera_context::CameraCalibration,
    error::{ApiError, ApiResult},
};

/// Revisions kept per pipeline.
pub const MAX_REVISIONS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineRevision {
    pub revision: u64,
    pub saved_at_ms: u64,
    pub spec: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PipelineHistory {
    pub revisions: Vec<PipelineRevision>,
}

impl PipelineHistory {
    pub fn latest(&self) -> Option<&PipelineRevision> {
        self.revisions.last()
    }

    pub fn next_revision(&self) -> u64 {
        self.latest().map_or(1, |latest| latest.revision + 1)
    }
}

/// Robot-frame camera mount: metres and degrees, x forward, y left, z up.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CameraMount {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
}

impl CameraMount {
    pub fn validate(&self) -> ApiResult<()> {
        let values = [self.x, self.y, self.z, self.roll, self.pitch, self.yaw];
        if values.iter().any(|v| !v.is_finite()) {
            return Err(ApiError::unprocessable("mount values must be finite numbers"));
        }
        if [self.x, self.y, self.z].iter().any(|v| v.abs() > 10.0) {
            return Err(ApiError::unprocessable("mount translation must be within 10 m of the robot origin"));
        }
        if [self.roll, self.pitch, self.yaw].iter().any(|v| v.abs() > 360.0) {
            return Err(ApiError::unprocessable("mount angles are degrees within ±360"));
        }
        Ok(())
    }
}

pub struct Store {
    dir: PathBuf,
    lock: Mutex<()>,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), lock: Mutex::new(()) }
    }

    fn pipeline_path(&self, id: &str) -> PathBuf {
        self.dir.join("pipelines").join(format!("{}.json", file_stem(id)))
    }

    fn mounts_path(&self) -> PathBuf {
        self.dir.join("camera-mounts.json")
    }

    pub async fn history(&self, id: &str) -> ApiResult<PipelineHistory> {
        let _guard = self.lock.lock().await;
        read_json(&self.pipeline_path(id)).await.map(Option::unwrap_or_default)
    }

    /// Append a revision (trimming old ones) and return its number.
    pub async fn push_revision(&self, id: &str, spec: serde_json::Value, saved_at_ms: u64) -> ApiResult<u64> {
        let _guard = self.lock.lock().await;
        let path = self.pipeline_path(id);
        let mut history: PipelineHistory = read_json(&path).await?.unwrap_or_default();
        let revision = history.next_revision();
        history.revisions.push(PipelineRevision { revision, saved_at_ms, spec });
        let excess = history.revisions.len().saturating_sub(MAX_REVISIONS);
        history.revisions.drain(..excess);
        write_json(&path, &history).await?;
        Ok(revision)
    }

    /// Drop the newest revision and return the one before it (now the newest).
    pub async fn pop_revision(&self, id: &str) -> ApiResult<PipelineRevision> {
        let _guard = self.lock.lock().await;
        let path = self.pipeline_path(id);
        let mut history: PipelineHistory = read_json(&path).await?.unwrap_or_default();
        if history.revisions.len() < 2 {
            return Err(ApiError::conflict(format!("pipeline {id} has no earlier revision to roll back to")));
        }
        history.revisions.pop();
        let previous = history.revisions.last().cloned().expect("at least one revision left");
        write_json(&path, &history).await?;
        Ok(previous)
    }

    pub async fn forget(&self, id: &str) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        match tokio::fs::remove_file(self.pipeline_path(id)).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn mounts(&self) -> ApiResult<BTreeMap<String, CameraMount>> {
        let _guard = self.lock.lock().await;
        read_json(&self.mounts_path()).await.map(Option::unwrap_or_default)
    }

    pub async fn set_mount(&self, camera: &str, mount: Option<CameraMount>) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        let path = self.mounts_path();
        let mut mounts: BTreeMap<String, CameraMount> = read_json(&path).await?.unwrap_or_default();
        match mount {
            Some(mount) => {
                mounts.insert(camera.to_string(), mount);
            }
            None => {
                mounts.remove(camera);
            }
        }
        write_json(&path, &mounts).await
    }

    /// The control values set through the API for `camera` (standard keys and the camera's
    /// control names), re-applied whenever its camera service appears.
    pub async fn camera_settings(&self, camera: &str) -> ApiResult<BTreeMap<String, serde_json::Value>> {
        let _guard = self.lock.lock().await;
        let mut all: BTreeMap<String, BTreeMap<String, serde_json::Value>> = read_json(&self.camera_settings_path()).await?.unwrap_or_default();
        Ok(all.remove(camera).unwrap_or_default())
    }

    /// Remember `values` for `camera` (replacing earlier values of the same keys) and return
    /// everything stored for it.
    pub async fn persist_camera_settings(&self, camera: &str, values: impl IntoIterator<Item = (String, serde_json::Value)>) -> ApiResult<BTreeMap<String, serde_json::Value>> {
        let _guard = self.lock.lock().await;
        let path = self.camera_settings_path();
        let mut all: BTreeMap<String, BTreeMap<String, serde_json::Value>> = read_json(&path).await?.unwrap_or_default();
        let stored = all.entry(camera.to_string()).or_default();
        let before = stored.clone();
        stored.extend(values);
        let stored = stored.clone();
        if stored != before {
            write_json(&path, &all).await?;
        }
        Ok(stored)
    }

    /// Forget every value stored for `camera` (back to the camera's defaults on its next start).
    pub async fn clear_camera_settings(&self, camera: &str) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        let path = self.camera_settings_path();
        let mut all: BTreeMap<String, BTreeMap<String, serde_json::Value>> = read_json(&path).await?.unwrap_or_default();
        if all.remove(camera).is_some() {
            write_json(&path, &all).await?;
        }
        Ok(())
    }

    fn camera_settings_path(&self) -> PathBuf {
        self.dir.join("camera-settings.json")
    }

    fn calibrations_path(&self) -> PathBuf {
        self.dir.join("camera-calibration.json")
    }

    /// Every camera's calibrations (one per calibrated image size).
    pub async fn calibrations(&self) -> ApiResult<BTreeMap<String, Vec<CameraCalibration>>> {
        let _guard = self.lock.lock().await;
        read_json(&self.calibrations_path()).await.map(Option::unwrap_or_default)
    }

    /// Keep `calibration` for `camera`, replacing the one of the same image size; returns the
    /// camera's calibrations.
    pub async fn put_calibration(&self, camera: &str, calibration: CameraCalibration) -> ApiResult<Vec<CameraCalibration>> {
        let _guard = self.lock.lock().await;
        let path = self.calibrations_path();
        let mut all: BTreeMap<String, Vec<CameraCalibration>> = read_json(&path).await?.unwrap_or_default();
        let list = all.entry(camera.to_string()).or_default();
        list.retain(|c| (c.width, c.height) != (calibration.width, calibration.height));
        list.push(calibration);
        list.sort_by_key(|c| (c.width, c.height));
        let list = list.clone();
        write_json(&path, &all).await?;
        Ok(list)
    }

    /// Forget `camera`'s calibration at `size`, or all of them; returns what is left.
    pub async fn delete_calibration(&self, camera: &str, size: Option<(u32, u32)>) -> ApiResult<Vec<CameraCalibration>> {
        let _guard = self.lock.lock().await;
        let path = self.calibrations_path();
        let mut all: BTreeMap<String, Vec<CameraCalibration>> = read_json(&path).await?.unwrap_or_default();
        let left = match size {
            Some(size) => {
                let list = all.entry(camera.to_string()).or_default();
                list.retain(|c| (c.width, c.height) != size);
                list.clone()
            }
            None => Vec::new(),
        };
        if left.is_empty() {
            all.remove(camera);
        }
        write_json(&path, &all).await?;
        Ok(left)
    }
}

fn file_stem(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()
}

async fn read_json<T: DeserializeOwned>(path: &Path) -> ApiResult<Option<T>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| ApiError::internal(format!("{} is corrupt: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn write_json<T: Serialize>(path: &Path, value: &T) -> ApiResult<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| ApiError::internal(error.to_string()))?;
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn revisions_push_pop_and_trim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        assert_eq!(store.push_revision("tags/front", serde_json::json!({"n": 1}), 1).await.expect("push"), 1);
        assert_eq!(store.push_revision("tags/front", serde_json::json!({"n": 2}), 2).await.expect("push"), 2);
        let previous = store.pop_revision("tags/front").await.expect("pop");
        assert_eq!(previous.revision, 1);
        assert!(store.pop_revision("tags/front").await.is_err(), "nothing before revision 1");
        for n in 0..(MAX_REVISIONS + 5) {
            store.push_revision("x", serde_json::json!(n), 0).await.expect("push");
        }
        let history = store.history("x").await.expect("history");
        assert_eq!(history.revisions.len(), MAX_REVISIONS);
        assert_eq!(history.latest().expect("latest").revision, (MAX_REVISIONS + 5) as u64);
    }

    #[tokio::test]
    async fn mounts_round_trip_and_validate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        let mount = CameraMount { x: 0.3, y: 0.0, z: 0.25, roll: 0.0, pitch: -15.0, yaw: 0.0 };
        mount.validate().expect("valid");
        store.set_mount("camera.a", Some(mount)).await.expect("set");
        assert_eq!(store.mounts().await.expect("mounts").get("camera.a"), Some(&mount));
        store.set_mount("camera.a", None).await.expect("clear");
        assert!(store.mounts().await.expect("mounts").is_empty());
        assert!(CameraMount { x: 50.0, ..mount }.validate().is_err());
    }

    #[tokio::test]
    async fn calibrations_are_kept_per_size_across_a_new_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        let full = CameraCalibration {
            width: 1280,
            height: 800,
            model: Default::default(),
            fx: 900.0,
            fy: 900.0,
            cx: 640.0,
            cy: 400.0,
            distortion: Default::default(),
            rms_px: None,
            source: None,
            saved_at_ms: 1,
        };
        store.put_calibration("camera.a", full.clone()).await.expect("put");
        store.put_calibration("camera.a", CameraCalibration { width: 640, height: 400, ..full.clone() }).await.expect("put");
        let replaced = store.put_calibration("camera.a", CameraCalibration { fx: 910.0, ..full.clone() }).await.expect("replace");
        assert_eq!(replaced.len(), 2);
        let store = Store::new(dir.path());
        assert_eq!(store.calibrations().await.expect("read")["camera.a"][1].fx, 910.0);
        assert_eq!(store.delete_calibration("camera.a", Some((640, 400))).await.expect("delete").len(), 1);
        assert!(store.delete_calibration("camera.a", None).await.expect("delete all").is_empty());
        assert!(store.calibrations().await.expect("read").is_empty());
    }

    #[tokio::test]
    async fn camera_settings_merge_survive_a_new_store_and_clear() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        assert!(store.camera_settings("camera.a").await.expect("empty").is_empty());
        store.persist_camera_settings("camera.a", [("exposure_us".to_string(), serde_json::json!(8000)), ("ae".to_string(), serde_json::json!(false))]).await.expect("persist");
        let merged = store.persist_camera_settings("camera.a", [("exposure_us".to_string(), serde_json::json!(4000))]).await.expect("persist");
        assert_eq!(merged, BTreeMap::from([("ae".to_string(), serde_json::json!(false)), ("exposure_us".to_string(), serde_json::json!(4000))]));
        store.persist_camera_settings("camera.b", [("gain".to_string(), serde_json::json!(2.0))]).await.expect("persist");

        // As after a reboot: a new store on the same directory.
        let store = Store::new(dir.path());
        assert_eq!(store.camera_settings("camera.a").await.expect("settings"), merged);
        store.clear_camera_settings("camera.a").await.expect("clear");
        assert!(store.camera_settings("camera.a").await.expect("cleared").is_empty());
        assert_eq!(store.camera_settings("camera.b").await.expect("other camera kept").len(), 1);
    }
}
