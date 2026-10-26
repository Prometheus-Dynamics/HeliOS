//! What only the API owns, persisted under its state directory (`/var/lib/helios/api`, on the
//! data partition, so it survives reboots and OTA updates): the revision history of each
//! pipeline (for rollback), where each camera is mounted on the robot, each camera's
//! calibrations, the camera control values set through the API, the uploaded field layouts
//! with the one selected for every pipeline that names none, and the first-boot decision about
//! the startup file's default pipeline (`startup.rs`).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::Mutex;

use crate::{
    camera_context::CameraCalibration,
    error::{ApiError, ApiResult},
    events::EventHub,
    field_layouts::StoredLayout,
};
use std::sync::Arc;

/// Revisions kept per pipeline.
pub const MAX_REVISIONS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineRevision {
    pub revision: u64,
    pub saved_at_ms: u64,
    pub spec: serde_json::Value,
}

/// The first-boot decision about the startup file's default pipeline. Kept once made, so that a
/// pipeline the user deletes is not created again.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct DefaultPipelineRecord {
    /// Whether the default pipeline was created (false: the board already had pipelines).
    pub created: bool,
    pub decided_at_ms: u64,
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
    pub(crate) dir: PathBuf,
    pub(crate) lock: Mutex<()>,
    /// The recording settings until they are saved: the defaults of this device.
    recording_defaults: helios_recorder::RecorderSettings,
    /// Where every write is announced (`settings` events); none in tests that do not listen.
    events: Option<Arc<EventHub>>,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), lock: Mutex::new(()), recording_defaults: helios_recorder::RecorderSettings::default(), events: None }
    }

    /// The recording settings a device without saved ones starts with (its layout's folders).
    pub fn with_recording_defaults(mut self, defaults: helios_recorder::RecorderSettings) -> Self {
        self.recording_defaults = defaults;
        self
    }

    /// Announce every write on `events`.
    pub fn with_events(mut self, events: Arc<EventHub>) -> Self {
        self.events = Some(events);
        self
    }

    /// Write `value` to `path` (the file is replaced whole), then announce the change of `domain`
    /// and `key` to every client. Every settings write goes through here, so a new setting is live
    /// on every open client without any extra wiring.
    pub(crate) async fn save<T: Serialize>(&self, domain: &str, key: &str, path: &Path, value: &T) -> ApiResult<()> {
        write_json(path, value).await?;
        self.notify_changed(domain, key);
        Ok(())
    }

    /// Announce that `domain`'s `key` changed: one `settings` event, `{domain, key}`.
    pub(crate) fn notify_changed(&self, domain: &str, key: &str) {
        if let Some(events) = &self.events {
            events.publish("settings", serde_json::json!({ "domain": domain, "key": key }));
        }
    }

    fn pipeline_path(&self, id: &str) -> PathBuf {
        self.dir.join("pipelines").join(format!("{}.json", file_stem(id)))
    }

    fn nt4_path(&self) -> PathBuf {
        self.dir.join("nt4.json")
    }

    /// NetworkTables settings (helios-engine reads the same file).
    pub async fn nt4_settings(&self) -> ApiResult<helios_robot::Nt4Settings> {
        let _guard = self.lock.lock().await;
        read_json(&self.nt4_path()).await.map(Option::unwrap_or_default)
    }

    pub async fn put_nt4_settings(&self, settings: &helios_robot::Nt4Settings) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("nt4", "settings", &self.nt4_path(), settings).await
    }

    /// The LED director's settings (`leds.json`); the defaults until saved.
    pub async fn led_settings(&self) -> ApiResult<crate::leds::settings::LedSettings> {
        let _guard = self.lock.lock().await;
        read_json(&self.dir.join("leds.json")).await.map(Option::unwrap_or_default)
    }

    pub async fn put_led_settings(&self, settings: &crate::leds::settings::LedSettings) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("leds", "settings", &self.dir.join("leds.json"), settings).await
    }

    fn recording_settings_path(&self) -> PathBuf {
        self.dir.join("recording.json")
    }

    /// Session recording settings (helios-recorder's, `recording.json`); the defaults until saved.
    pub async fn recording_settings(&self) -> ApiResult<helios_recorder::RecorderSettings> {
        let _guard = self.lock.lock().await;
        match read_json::<serde_json::Value>(&self.recording_settings_path()).await? {
            Some(stored) => self.recording_settings_from(stored).map_err(|error| ApiError::internal(format!("recording.json: {error}"))),
            None => Ok(self.recording_defaults.clone()),
        }
    }

    /// Recording settings from a partial object: the fields it leaves out take this device's
    /// defaults (its layout's folders), not the image's.
    pub fn recording_settings_from(&self, partial: serde_json::Value) -> Result<helios_recorder::RecorderSettings, String> {
        let serde_json::Value::Object(fields) = partial else { return Err("recording settings must be an object".into()) };
        let mut merged = serde_json::to_value(&self.recording_defaults).map_err(|error| error.to_string())?;
        merged.as_object_mut().expect("settings serialize to an object").extend(fields);
        serde_json::from_value(merged).map_err(|error| error.to_string())
    }

    pub async fn put_recording_settings(&self, settings: &helios_recorder::RecorderSettings) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("recording", "settings", &self.recording_settings_path(), settings).await
    }

    fn mounts_path(&self) -> PathBuf {
        self.dir.join("camera-mounts.json")
    }

    fn default_pipeline_path(&self) -> PathBuf {
        self.dir.join("default-pipeline.json")
    }

    pub async fn default_pipeline_record(&self) -> ApiResult<Option<DefaultPipelineRecord>> {
        let _guard = self.lock.lock().await;
        read_json(&self.default_pipeline_path()).await
    }

    pub async fn record_default_pipeline(&self, created: bool, decided_at_ms: u64) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("default_pipeline", "record", &self.default_pipeline_path(), &DefaultPipelineRecord { created, decided_at_ms }).await
    }

    pub async fn history(&self, id: &str) -> ApiResult<PipelineHistory> {
        let _guard = self.lock.lock().await;
        read_json(&self.pipeline_path(id)).await.map(Option::unwrap_or_default)
    }

    /// Append revision `revision` (trimming old ones) and return its number. The number is the one
    /// the caller wrote to Orion, so the history and the device agree; it is never below the
    /// history's own next number, so the history stays strictly increasing.
    pub async fn push_revision(&self, id: &str, revision: u64, spec: serde_json::Value, saved_at_ms: u64) -> ApiResult<u64> {
        let _guard = self.lock.lock().await;
        let path = self.pipeline_path(id);
        let mut history: PipelineHistory = read_json(&path).await?.unwrap_or_default();
        let revision = revision.max(history.next_revision());
        history.revisions.push(PipelineRevision { revision, saved_at_ms, spec });
        let excess = history.revisions.len().saturating_sub(MAX_REVISIONS);
        history.revisions.drain(..excess);
        self.save("pipeline", id, &path, &history).await?;
        Ok(revision)
    }

    /// The revision before the newest one (what a rollback restores), without changing the history.
    pub async fn previous_revision(&self, id: &str) -> ApiResult<PipelineRevision> {
        let _guard = self.lock.lock().await;
        let history: PipelineHistory = read_json(&self.pipeline_path(id)).await?.unwrap_or_default();
        let count = history.revisions.len();
        if count < 2 {
            return Err(ApiError::conflict(format!("pipeline {id} has no earlier revision to roll back to")));
        }
        Ok(history.revisions[count - 2].clone())
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
        self.save("pipeline", id, &path, &history).await?;
        Ok(previous)
    }

    pub async fn forget(&self, id: &str) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        match tokio::fs::remove_file(self.pipeline_path(id)).await {
            Ok(()) => {
                self.notify_changed("pipeline", id);
                Ok(())
            }
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
        self.save("mounts", camera, &path, &mounts).await
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
            self.save("camera_settings", camera, &path, &all).await?;
        }
        Ok(stored)
    }

    /// Forget the values stored for `camera` under `keys` (the others are kept), and return what
    /// is still stored for it.
    pub async fn forget_camera_settings(&self, camera: &str, keys: impl IntoIterator<Item = String>) -> ApiResult<BTreeMap<String, serde_json::Value>> {
        let _guard = self.lock.lock().await;
        let path = self.camera_settings_path();
        let mut all: BTreeMap<String, BTreeMap<String, serde_json::Value>> = read_json(&path).await?.unwrap_or_default();
        let Some(stored) = all.get_mut(camera) else {
            return Ok(BTreeMap::new());
        };
        let before = stored.len();
        for key in keys {
            stored.remove(&key);
        }
        let remaining = stored.clone();
        let changed = remaining.len() != before;
        if remaining.is_empty() {
            all.remove(camera);
        }
        if changed {
            self.save("camera_settings", camera, &path, &all).await?;
        }
        Ok(remaining)
    }

    /// Forget every value stored for `camera` (back to the camera's defaults on its next start).
    pub async fn clear_camera_settings(&self, camera: &str) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        let path = self.camera_settings_path();
        let mut all: BTreeMap<String, BTreeMap<String, serde_json::Value>> = read_json(&path).await?.unwrap_or_default();
        if all.remove(camera).is_some() {
            self.save("camera_settings", camera, &path, &all).await?;
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
        self.save("calibration", camera, &path, &all).await?;
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
        self.save("calibration", camera, &path, &all).await?;
        Ok(left)
    }
}

impl Store {
    fn field_layouts_dir(&self) -> PathBuf {
        self.dir.join("field-layouts")
    }

    fn field_layout_selection_path(&self) -> PathBuf {
        self.dir.join("field-layout.json")
    }

    /// The uploaded field layouts (the built-in ones are not stored).
    pub async fn field_layouts(&self) -> ApiResult<Vec<StoredLayout>> {
        let _guard = self.lock.lock().await;
        let mut layouts = Vec::new();
        let mut entries = match tokio::fs::read_dir(self.field_layouts_dir()).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(layouts),
            Err(error) => return Err(error.into()),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(layout) = read_json::<StoredLayout>(&path).await?
            {
                layouts.push(layout);
            }
        }
        layouts.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(layouts)
    }

    pub async fn put_field_layout(&self, layout: &StoredLayout) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("field_layout", &layout.id, &self.field_layouts_dir().join(format!("{}.json", file_stem(&layout.id))), layout).await
    }

    /// Forget an uploaded layout; false when there was none.
    pub async fn delete_field_layout(&self, id: &str) -> ApiResult<bool> {
        let _guard = self.lock.lock().await;
        match tokio::fs::remove_file(self.field_layouts_dir().join(format!("{}.json", file_stem(id)))).await {
            Ok(()) => {
                self.notify_changed("field_layout", id);
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// The layout selected for pipelines that name none; `None` until one is selected.
    pub async fn selected_field_layout(&self) -> ApiResult<Option<String>> {
        let _guard = self.lock.lock().await;
        let selection: Option<FieldLayoutSelection> = read_json(&self.field_layout_selection_path()).await?;
        Ok(selection.map(|s| s.selected))
    }

    pub async fn select_field_layout(&self, id: &str) -> ApiResult<()> {
        let _guard = self.lock.lock().await;
        self.save("field_layout", "selected", &self.field_layout_selection_path(), &FieldLayoutSelection { selected: id.to_string() }).await
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FieldLayoutSelection {
    selected: String,
}

fn file_stem(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()
}

pub(crate) async fn read_json<T: DeserializeOwned>(path: &Path) -> ApiResult<Option<T>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| ApiError::internal(format!("{} is corrupt: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn write_json<T: Serialize>(path: &Path, value: &T) -> ApiResult<()> {
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
        assert_eq!(store.push_revision("tags/front", 1, serde_json::json!({"n": 1}), 1).await.expect("push"), 1);
        assert_eq!(store.push_revision("tags/front", 2, serde_json::json!({"n": 2}), 2).await.expect("push"), 2);
        assert_eq!(store.previous_revision("tags/front").await.expect("previous").revision, 1);
        let previous = store.pop_revision("tags/front").await.expect("pop");
        assert_eq!(previous.revision, 1);
        assert!(store.pop_revision("tags/front").await.is_err(), "nothing before revision 1");
        assert!(store.previous_revision("tags/front").await.is_err(), "nothing before revision 1");
        // The number Orion was given wins when it is ahead of the history (a deploy whose record was
        // cut short); a stale number never takes the history backwards.
        assert_eq!(store.push_revision("tags/front", 5, serde_json::json!({"n": 5}), 3).await.expect("push"), 5);
        assert_eq!(store.push_revision("tags/front", 2, serde_json::json!({"n": 6}), 4).await.expect("push"), 6);
        for n in 0..(MAX_REVISIONS + 5) {
            let next = store.history("x").await.expect("history").next_revision();
            store.push_revision("x", next, serde_json::json!(n), 0).await.expect("push");
        }
        let history = store.history("x").await.expect("history");
        assert_eq!(history.revisions.len(), MAX_REVISIONS);
        assert_eq!(history.latest().expect("latest").revision, (MAX_REVISIONS + 5) as u64);
    }

    #[tokio::test]
    async fn default_pipeline_decision_is_kept_across_a_new_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        assert_eq!(store.default_pipeline_record().await.expect("read"), None);
        store.record_default_pipeline(true, 7).await.expect("record");
        let store = Store::new(dir.path());
        assert_eq!(store.default_pipeline_record().await.expect("read"), Some(DefaultPipelineRecord { created: true, decided_at_ms: 7 }));
    }

    #[tokio::test]
    async fn recording_settings_fill_missing_fields_from_the_device_layout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let guest = helios_layout::Layout::guest("/usr/local");
        let store = Store::new(dir.path()).with_recording_defaults(helios_recorder::RecorderSettings::for_layout(&guest));
        let settings = store.recording_settings_from(serde_json::json!({"max_fps": 15.0})).expect("partial settings");
        assert_eq!(settings.max_fps, Some(15.0));
        assert_eq!(settings.fallback_dir, guest.fallback_recordings_dir(), "a guest never falls back to the image's /data");
        assert_eq!(settings.usb_roots, guest.removable_media_roots());
        // A stored file written before a field existed reads the same way.
        tokio::fs::write(dir.path().join("recording.json"), r#"{"compression": false}"#).await.expect("write");
        let stored = store.recording_settings().await.expect("read");
        assert!(!stored.compression);
        assert_eq!(stored.fallback_dir, guest.fallback_recordings_dir());
        assert!(store.recording_settings_from(serde_json::json!({"no_such_field": 1})).is_err(), "unknown fields are still refused");
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
            uncertainty: None,
            source: None,
            orientation: Default::default(),
            saved_at_ms: 1,
            provenance: None,
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

    #[tokio::test]
    async fn forgetting_removes_only_the_listed_keys_and_survives_a_new_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        store
            .persist_camera_settings("camera.a", [("exposure_us".to_string(), serde_json::json!(8000)), ("ae".to_string(), serde_json::json!(false)), ("sharpness".to_string(), serde_json::json!(4))])
            .await
            .expect("persist");
        store.persist_camera_settings("camera.b", [("exposure_us".to_string(), serde_json::json!(2000))]).await.expect("persist");

        // Forgetting a key that is not stored changes nothing; the others stay.
        let remaining = store.forget_camera_settings("camera.a", ["exposure_us".to_string(), "gain".to_string()]).await.expect("forget");
        assert_eq!(remaining, BTreeMap::from([("ae".to_string(), serde_json::json!(false)), ("sharpness".to_string(), serde_json::json!(4))]));

        // As after a reboot: a new store on the same directory.
        let store = Store::new(dir.path());
        assert_eq!(store.camera_settings("camera.a").await.expect("settings"), remaining);
        assert_eq!(store.camera_settings("camera.b").await.expect("other camera kept"), BTreeMap::from([("exposure_us".to_string(), serde_json::json!(2000))]));

        // Forgetting the last keys drops the camera's entry, and an unknown camera is empty.
        store.forget_camera_settings("camera.a", ["ae".to_string(), "sharpness".to_string()]).await.expect("forget");
        assert!(store.camera_settings("camera.a").await.expect("empty").is_empty());
        assert!(store.forget_camera_settings("camera.zzz", ["ae".to_string()]).await.expect("unknown").is_empty());
        assert_eq!(store.camera_settings("camera.b").await.expect("other camera kept").len(), 1);
    }
}
