//! The calibration and mount a run on a player uses. A recording's own (from its `helios.camera`
//! metadata) when it has them, else the live camera's the pipeline was bound to. They are kept
//! under the player's resource id in the store, where the pipelines read a camera's calibration
//! and mount (`pipelines::camera_inputs_for`), so the run needs no change to the pipeline code.

use serde::Serialize;

use super::{facts::RecordingFacts, is_player};
use crate::{error::ApiResult, store::Store};

/// Where a run's calibration or mount came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ContextFrom {
    /// The recording's own metadata.
    Recording,
    /// The live camera the pipeline was bound to (the fallback).
    Camera { camera: String },
    /// Neither the recording nor the live camera has one.
    None,
}

/// What a run uses: the calibration and the mount, and where each came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunContext {
    pub calibration: ContextFrom,
    pub mount: ContextFrom,
}

/// Keeps the run's calibration and mount under `player` (its resource id), replacing what it had.
pub async fn take(store: &Store, player: &str, facts: &RecordingFacts, live_camera: &str) -> ApiResult<RunContext> {
    let live_calibrations = store.calibrations().await?.remove(live_camera).unwrap_or_default();
    let (calibrations, calibration_from) = match &facts.calibration {
        Some(recorded) if !recorded.is_empty() => (recorded.clone(), ContextFrom::Recording),
        _ if !live_calibrations.is_empty() => (live_calibrations, ContextFrom::Camera { camera: live_camera.to_string() }),
        _ => (Vec::new(), ContextFrom::None),
    };
    store.delete_calibration(player, None).await?;
    for calibration in calibrations {
        store.put_calibration(player, calibration).await?;
    }

    let live_mount = store.mounts().await?.remove(live_camera);
    let (mount, mount_from) = match (facts.mount, live_mount) {
        (Some(recorded), _) => (Some(recorded), ContextFrom::Recording),
        (None, Some(live)) => (Some(live), ContextFrom::Camera { camera: live_camera.to_string() }),
        (None, None) => (None, ContextFrom::None),
    };
    store.set_mount(player, mount).await?;
    Ok(RunContext { calibration: calibration_from, mount: mount_from })
}

/// Forgets what was kept for `player` (when it closes).
pub async fn forget(store: &Store, player: &str) -> ApiResult<()> {
    store.delete_calibration(player, None).await?;
    store.set_mount(player, None).await?;
    store.clear_camera_settings(player).await
}

/// Forgets what players kept in the store and that are not open now (left by an earlier run).
pub async fn forget_stale(store: &Store, open: &[String]) -> ApiResult<()> {
    let stale = |camera: &String| is_player(camera) && !open.contains(camera);
    let calibrated: Vec<String> = store.calibrations().await?.into_keys().filter(stale).collect();
    let mounted: Vec<String> = store.mounts().await?.into_keys().filter(stale).collect();
    for camera in calibrated.iter().chain(mounted.iter()).collect::<std::collections::BTreeSet<_>>() {
        forget(store, camera).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{camera_context::CameraCalibration, store::CameraMount};

    fn calibration(width: u32) -> CameraCalibration {
        CameraCalibration {
            width,
            height: 800,
            model: Default::default(),
            fx: 1000.0,
            fy: 1000.0,
            cx: f64::from(width) / 2.0,
            cy: 400.0,
            distortion: Default::default(),
            rms_px: None,
            uncertainty: None,
            source: None,
            orientation: Default::default(),
            saved_at_ms: 0,
            provenance: None,
        }
    }

    fn mount(x: f64) -> CameraMount {
        CameraMount { x, y: 0.0, z: 0.3, roll: 0.0, pitch: 0.0, yaw: 0.0 }
    }

    #[tokio::test]
    async fn a_run_uses_the_recording_then_the_live_camera_and_keeps_them_for_the_player() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(dir.path());
        let live = "camera.node-local.front";
        store.put_calibration(live, calibration(1280)).await.expect("live calibration");
        store.set_mount(live, Some(mount(0.2))).await.expect("live mount");
        let player = "camera.playback.00000001";

        // A recording with its own calibration and no mount: its calibration, the live mount.
        let recorded = RecordingFacts { calibration: Some(vec![calibration(640)]), mount: None, mode: None };
        let context = take(&store, player, &recorded, live).await.expect("take");
        assert_eq!(context, RunContext { calibration: ContextFrom::Recording, mount: ContextFrom::Camera { camera: live.into() } });
        assert_eq!(store.calibrations().await.expect("calibrations")[player], vec![calibration(640)]);
        assert_eq!(store.mounts().await.expect("mounts")[player], mount(0.2));

        // A recording with neither: the live camera's calibration and mount.
        let bare = RecordingFacts::default();
        let context = take(&store, player, &bare, live).await.expect("take");
        assert_eq!(context, RunContext { calibration: ContextFrom::Camera { camera: live.into() }, mount: ContextFrom::Camera { camera: live.into() } });
        assert_eq!(store.calibrations().await.expect("calibrations")[player], vec![calibration(1280)]);

        // Forgotten when the player closes; a stale player's values go on the next start.
        forget(&store, player).await.expect("forget");
        assert!(!store.calibrations().await.expect("calibrations").contains_key(player));
        assert!(!store.mounts().await.expect("mounts").contains_key(player));
        take(&store, "camera.playback.00000002", &recorded, live).await.expect("take");
        forget_stale(&store, &[]).await.expect("forget stale");
        assert!(!store.calibrations().await.expect("calibrations").contains_key("camera.playback.00000002"));
        assert!(store.calibrations().await.expect("calibrations").contains_key(live), "live cameras are kept");
    }

    #[test]
    fn a_run_context_says_where_each_part_came_from() {
        let context = RunContext { calibration: ContextFrom::Recording, mount: ContextFrom::Camera { camera: "camera.node-local.front".into() } };
        let json = serde_json::to_value(&context).expect("json");
        assert_eq!(json["calibration"]["from"], "recording");
        assert_eq!(json["mount"], serde_json::json!({"from": "camera", "camera": "camera.node-local.front"}));
        assert_eq!(serde_json::to_value(ContextFrom::None).expect("json")["from"], "none");
    }
}
