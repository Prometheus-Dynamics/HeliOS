//! Keeps a live mapper's self-calibration. When Eidos's live mapper (`eidos:mapping.live_map`) has
//! fitted the lens of a camera without a calibration and converged, the fit is stored as that
//! camera's calibration for the pipeline's camera binding's frame size, so the next boot starts
//! calibrated. The fit's `provenance` says it was self-calibrated (and its views and revision).
//!
//! The mapper's poses come through the live relay (`live_outputs`): this task holds a subscription
//! only while a live mapper is deployed, because the engine serializes its outputs for the relay
//! only while someone listens. Every pose line is first checked by substring for
//! `"converged":true`, and parsed only then.
//!
//! Rules: a user's calibration at that size always wins. A stored self-calibration is replaced
//! only by a fit with more views or a lower rms. A fit is stored once per Eidos revision (without
//! a revision: the first converged pose of a pipeline since the API started), so the stored
//! calibration changing the mapper's start does not store again.

use std::{collections::{BTreeMap, HashMap}, sync::Arc, time::Duration};

use serde::Deserialize;
use tokio::sync::broadcast;

use crate::{
    SharedState,
    camera_context::{CameraCalibration, Distortion, LensModel, Orientation, Provenance},
    camera_uncertainty::CalibrationUncertainty,
    host,
    live_outputs::LiveReceiver,
    routes::pipelines::{self, LiveMapper},
};

/// `provenance.method` of a calibration fitted by a live mapper.
pub const METHOD: &str = "self-calibration";
/// The live relay's ports of the mapper's pose: `map_pose` (Eidos's templates), `pose` (graphs
/// stored before them).
const POSE_PORTS: [&str; 2] = ["map_pose", "pose"];
/// How often the live mappers are looked up in Orion.
const SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// The `calibration` block of Eidos's map pose (`eidos:map_pose`): the lens the mapper fitted.
/// Eidos's `distortion` is OpenCV's eight terms (`k1` to `k6`, `p1`, `p2`); a fisheye uses `k1`
/// to `k4`. `revision`, `converged`, `width` and `height` may be absent from older Eidos; `width`
/// and `height` are the frame size the fit is at (0 before the mapper has seen a frame).
#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq)]
pub struct Fit {
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub converged: bool,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub fx: Option<f64>,
    #[serde(default)]
    pub fy: Option<f64>,
    #[serde(default)]
    pub cx: Option<f64>,
    #[serde(default)]
    pub cy: Option<f64>,
    #[serde(default)]
    pub distortion: Vec<f64>,
    #[serde(default)]
    pub rms_px: Option<f64>,
    #[serde(default)]
    pub views: Option<u32>,
    #[serde(default)]
    pub frames: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// One pose line of the live relay (`{pipeline, port, observed_at_ms, frame_ts_ns, value}`).
#[derive(Deserialize)]
struct PoseLine {
    pipeline: String,
    port: String,
    value: PoseValue,
}

#[derive(Deserialize)]
struct PoseValue {
    calibration: Option<Fit>,
}

/// The frame size a fit says it is at, when it says one.
fn fit_size(fit: &Fit) -> Option<(u32, u32)> {
    match (fit.width?, fit.height?) {
        (0, _) | (_, 0) => None,
        size => Some(size),
    }
}

/// The calibration a converged fit is, at `width` x `height` in the camera's current `orientation`
/// (the frames the mapper saw were read out that way). Errors name what is wrong with the fit.
pub fn calibration_of(fit: &Fit, width: u32, height: u32, orientation: Orientation, pipeline: &str) -> Result<CameraCalibration, String> {
    if !(fit.estimated && fit.converged) {
        return Err("the fit is not an estimate that converged".into());
    }
    let model = match fit.model.as_str() {
        "pinhole" => LensModel::Pinhole,
        "fisheye" => LensModel::Fisheye,
        "" => return Err("the fit names no lens model".into()),
        other => return Err(format!("the fit's lens model `{other}` is not one HeliOS knows")),
    };
    let (Some(fx), Some(fy), Some(cx), Some(cy)) = (fit.fx, fit.fy, fit.cx, fit.cy) else {
        return Err("the fit's intrinsics are missing".into());
    };
    let rms = fit.rms_px.ok_or("the fit's rms_px is missing")?;
    let views = fit.views.filter(|views| *views > 0).ok_or("the fit has no views")?;
    let need = match model {
        LensModel::Pinhole => 8,
        LensModel::Fisheye => 4,
    };
    if fit.distortion.len() < need {
        return Err(format!("the fit has {} distortion terms; a {} lens has {need}", fit.distortion.len(), model.name()));
    }
    let d = &fit.distortion;
    let distortion = match model {
        LensModel::Pinhole => Distortion { k1: d[0], k2: d[1], k3: d[2], k4: d[3], k5: d[4], k6: d[5], p1: d[6], p2: d[7] },
        // The fisheye's terms are k1 to k4; the others are zero (validation requires it).
        LensModel::Fisheye => Distortion { k1: d[0], k2: d[1], k3: d[2], k4: d[3], ..Distortion::default() },
    };
    let calibration = CameraCalibration {
        width,
        height,
        model,
        fx,
        fy,
        cx,
        cy,
        distortion,
        rms_px: Some(rms),
        // No covariance from the mapper: Eidos maps the rms and views to its conservative sigmas.
        uncertainty: Some(CalibrationUncertainty::solved(&[], rms, views as usize, model)),
        source: None,
        orientation,
        saved_at_ms: 0,
        provenance: Some(Provenance { method: METHOD.into(), views, frames: fit.frames, revision: fit.revision, pipeline: Some(pipeline.into()) }),
    };
    calibration.validate().map_err(|error| error.message)?;
    Ok(calibration)
}

/// What to do with a fit, given the calibration stored at its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Store,
    Keep(String),
}

/// Store `fit` unless the camera has a calibration of its own at that size (which always wins), or
/// the stored self-calibration is at least as good: a fit replaces it only with a rms no higher and
/// either more views or a lower rms, so more views never buy a worse fit.
pub fn decide(stored: Option<&CameraCalibration>, fit: &CameraCalibration) -> Decision {
    let Some(stored) = stored else { return Decision::Store };
    let Some(old) = &stored.provenance else {
        return Decision::Keep("the camera has a calibration of its own at this size, which always wins".into());
    };
    if old.method != METHOD {
        return Decision::Keep(format!("the stored calibration came from `{}`", old.method));
    }
    let Some(new) = &fit.provenance else {
        return Decision::Keep("the fit is not a self-calibration".into());
    };
    let old_rms = stored.rms_px.unwrap_or(f64::INFINITY);
    let new_rms = fit.rms_px.unwrap_or(f64::INFINITY);
    if new_rms <= old_rms && (new.views > old.views || new_rms < old_rms) {
        Decision::Store
    } else {
        Decision::Keep(format!("the stored self-calibration ({} views at {old_rms:.3} px) is at least as good as the fit ({} views at {new_rms:.3} px)", old.views, new.views))
    }
}

/// Which fits were handled per pipeline: the revision last seen (`None` without one).
#[derive(Debug, Default)]
pub struct Handled(HashMap<String, Option<u64>>);

impl Handled {
    /// Whether this fit is new for its pipeline, and records it as seen.
    fn is_new(&mut self, pipeline: &str, revision: Option<u64>) -> bool {
        match self.0.get(pipeline) {
            Some(seen) if *seen == revision => false,
            _ => {
                self.0.insert(pipeline.to_string(), revision);
                true
            }
        }
    }
}

/// Why each camera's last fit was not stored, for the camera's status chain: the reason a fit was
/// refused, cleared when one is stored. In memory: a restart starts without notes.
#[derive(Debug, Default)]
pub struct Notes(std::sync::Mutex<BTreeMap<String, String>>);

impl Notes {
    /// Sets (or with `None`, clears) the camera's note.
    pub fn set(&self, camera: &str, reason: Option<String>) {
        let mut notes = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match reason {
            Some(reason) => notes.insert(camera.to_string(), reason),
            None => notes.remove(camera),
        };
    }

    pub fn get(&self, camera: &str) -> Option<String> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(camera).cloned()
    }
}

/// The task: looks up the live mappers every few seconds, holds the relay's subscription while
/// there are any, and stores their converged fits.
pub fn spawn(state: SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut mappers: BTreeMap<String, LiveMapper> = BTreeMap::new();
        let mut handled = Handled::default();
        let mut lines: Option<LiveReceiver> = None;
        let mut scan = tokio::time::interval(SCAN_INTERVAL);
        scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = scan.tick() => {
                    if let Ok(view) = state.orion.view().await {
                        mappers = pipelines::live_mappers(&view);
                    }
                    match (mappers.is_empty(), lines.is_some()) {
                        (true, _) => lines = None,
                        (false, false) => lines = Some(state.live.subscribe()),
                        (false, true) => {}
                    }
                }
                received = next(&mut lines) => match received {
                    Ok(chunk) => handle_chunk(&state, &mappers, &mut handled, &chunk).await,
                    // Behind the engine: the newer chunks carry the same fits.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => lines = None,
                },
            }
        }
    })
}

async fn next(lines: &mut Option<LiveReceiver>) -> Result<Arc<str>, broadcast::error::RecvError> {
    match lines {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Stores the converged fits in a chunk of relay lines from a live mapper.
pub async fn handle_chunk(state: &SharedState, mappers: &BTreeMap<String, LiveMapper>, handled: &mut Handled, chunk: &str) {
    for line in chunk.lines() {
        // Cheap checks first: most lines are not a converged pose.
        if !line.contains("\"converged\":true") || !POSE_PORTS.iter().any(|port| line.contains(&format!("\"port\":\"{port}\""))) {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<PoseLine>(line) else { continue };
        if !POSE_PORTS.contains(&parsed.port.as_str()) {
            continue;
        }
        let Some(mapper) = mappers.get(&parsed.pipeline) else { continue };
        let Some(fit) = parsed.value.calibration else { continue };
        if !(fit.estimated && fit.converged) || !handled.is_new(&parsed.pipeline, fit.revision) {
            continue;
        }
        if let Err(error) = store(state, &parsed.pipeline, mapper, &fit).await {
            tracing::warn!(pipeline = %parsed.pipeline, camera = %mapper.camera, error = %error.message, "self-calibration not stored");
        }
    }
}

/// Stores one fit for the mapper's camera at the fit's own frame size (the binding's size from an
/// older Eidos), if the rules allow it, and hands the camera's inputs to its pipelines.
async fn store(state: &SharedState, pipeline: &str, mapper: &LiveMapper, fit: &Fit) -> crate::error::ApiResult<()> {
    let Some((width, height)) = fit_size(fit).or(mapper.size) else {
        let reason = "neither the fit nor the mapper's input says the frame size";
        tracing::info!(pipeline, camera = %mapper.camera, reason, "self-calibration not stored");
        state.self_calibration_notes.set(&mapper.camera, Some(reason.into()));
        return Ok(());
    };
    let orientation = Orientation::from_settings(&state.store.camera_settings(&mapper.camera).await?);
    let mut calibration = match calibration_of(fit, width, height, orientation, pipeline) {
        Ok(calibration) => calibration,
        Err(reason) => {
            tracing::warn!(pipeline, camera = %mapper.camera, reason, "self-calibration not stored");
            state.self_calibration_notes.set(&mapper.camera, Some(reason));
            return Ok(());
        }
    };
    let stored = state.store.calibrations().await?.remove(&mapper.camera).unwrap_or_default();
    let at_size = stored.iter().find(|c| (c.width, c.height) == (width, height));
    match decide(at_size, &calibration) {
        Decision::Keep(reason) => {
            tracing::info!(pipeline, camera = %mapper.camera, reason, "self-calibration kept out");
            state.self_calibration_notes.set(&mapper.camera, Some(reason));
            return Ok(());
        }
        Decision::Store => {}
    }
    state.self_calibration_notes.set(&mapper.camera, None);
    calibration.saved_at_ms = host::now_ms();
    state.store.put_calibration(&mapper.camera, calibration.clone()).await?;
    state.events.publish("camera", serde_json::json!({ "id": mapper.camera, "change": "self_calibration", "pipeline": pipeline, "calibration": calibration }));
    match pipelines::refresh_camera_inputs(state, &mapper.camera).await {
        Ok(updated) => tracing::info!(pipeline, camera = %mapper.camera, views = ?calibration.provenance.as_ref().map(|p| p.views), pipelines = ?updated, "self-calibration stored"),
        Err(error) => tracing::warn!(pipeline, camera = %mapper.camera, error = %error.message, "self-calibration stored, not handed to its pipelines"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiConfig, AppState, camera_context::LensModel};

    fn fit(model: &str) -> Fit {
        Fit {
            estimated: true,
            converged: true,
            revision: Some(3),
            model: model.into(),
            fx: Some(566.5),
            fy: Some(566.4),
            cx: Some(627.3),
            cy: Some(351.8),
            distortion: vec![-0.0156, 0.0021, -0.0004, 0.0001, 0.0, 0.0, 0.0002, -0.0003],
            rms_px: Some(0.31),
            views: Some(24),
            frames: Some(180),
            width: Some(1280),
            height: Some(800),
        }
    }

    #[test]
    fn a_fit_is_stored_at_its_own_frame_size() {
        assert_eq!(fit_size(&fit("pinhole")), Some((1280, 800)));
        assert_eq!(fit_size(&Fit { width: Some(0), height: Some(0), ..fit("pinhole") }), None, "no frame seen yet");
        assert_eq!(fit_size(&Fit { width: None, height: None, ..fit("pinhole") }), None, "an older Eidos");
        let line: PoseLine = serde_json::from_value(serde_json::json!({
            "pipeline": "p", "port": "pose",
            "value": { "calibration": { "estimated": true, "converged": true, "width": 640, "height": 400, "model": "pinhole" } }
        }))
        .expect("pose line");
        assert_eq!(line.value.calibration.as_ref().and_then(fit_size), Some((640, 400)));
    }

    fn stored(views: u32, rms: f64) -> CameraCalibration {
        CameraCalibration {
            rms_px: Some(rms),
            provenance: Some(Provenance { method: METHOD.into(), views, frames: None, revision: Some(1), pipeline: Some("live-map".into()) }),
            ..calibration_of(&fit("pinhole"), 1280, 800, Orientation::default(), "live-map").expect("valid")
        }
    }

    #[test]
    fn a_pinhole_fit_maps_all_eight_terms_in_opencv_order() {
        let c = calibration_of(&fit("pinhole"), 1280, 800, Orientation::default(), "live-map").expect("valid");
        assert_eq!(c.model, LensModel::Pinhole);
        assert_eq!((c.fx, c.fy, c.cx, c.cy), (566.5, 566.4, 627.3, 351.8));
        assert_eq!((c.distortion.k1, c.distortion.k2, c.distortion.k3), (-0.0156, 0.0021, -0.0004));
        assert_eq!((c.distortion.k4, c.distortion.k5, c.distortion.k6), (0.0001, 0.0, 0.0));
        assert_eq!((c.distortion.p1, c.distortion.p2), (0.0002, -0.0003));
        assert_eq!(c.rms_px, Some(0.31));
        let provenance = c.provenance.expect("provenance");
        assert_eq!((provenance.method.as_str(), provenance.views, provenance.frames, provenance.revision), (METHOD, 24, Some(180), Some(3)));
        assert_eq!(c.uncertainty.as_ref().map(|u| (u.views, u.rms_px)), Some((24, 0.31)));
    }

    #[test]
    fn a_fisheye_fit_takes_four_terms_and_zeroes_the_rest() {
        let mut f = fit("fisheye");
        f.distortion = vec![-0.02, 0.003, -0.001, 0.0004, 9.0, 9.0, 9.0, 9.0];
        let c = calibration_of(&f, 1280, 800, Orientation::default(), "live-map").expect("valid");
        assert_eq!(c.model, LensModel::Fisheye);
        assert_eq!((c.distortion.k1, c.distortion.k4), (-0.02, 0.0004));
        assert_eq!((c.distortion.k5, c.distortion.k6, c.distortion.p1, c.distortion.p2), (0.0, 0.0, 0.0, 0.0), "validation needs the fisheye's others zero");
        assert_eq!(c.uncertainty.as_ref().map(|u| u.sigma.distortion.len()), Some(4));
        f.distortion.truncate(3);
        assert!(calibration_of(&f, 1280, 800, Orientation::default(), "live-map").is_err(), "a fisheye needs four terms");
    }

    #[test]
    fn a_fit_is_stored_only_when_it_is_a_converged_estimate_with_good_values() {
        assert!(calibration_of(&Fit { converged: false, ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "not converged");
        assert!(calibration_of(&Fit { estimated: false, ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "given, not estimated");
        assert!(calibration_of(&fit("catadioptric"), 1280, 800, Orientation::default(), "p").is_err(), "unknown model");
        assert!(calibration_of(&fit(""), 1280, 800, Orientation::default(), "p").is_err(), "no model");
        assert!(calibration_of(&Fit { fx: Some(0.0), ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "focal length");
        assert!(calibration_of(&Fit { fy: None, ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "missing focal length");
        assert!(calibration_of(&Fit { rms_px: None, ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "no rms");
        assert!(calibration_of(&Fit { rms_px: Some(f64::NAN), ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "NaN rms");
        assert!(calibration_of(&Fit { views: Some(0), ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "no views");
        assert!(calibration_of(&Fit { distortion: vec![0.0; 3], ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "too few terms");
        assert!(calibration_of(&Fit { distortion: vec![f64::INFINITY; 8], ..fit("pinhole") }, 1280, 800, Orientation::default(), "p").is_err(), "infinite term");
    }

    #[test]
    fn the_fit_is_stored_in_the_orientation_it_was_seen_in() {
        let flipped = Orientation { hflip: true, vflip: false };
        let c = calibration_of(&fit("pinhole"), 1280, 800, flipped, "live-map").expect("valid");
        assert_eq!(c.orientation, flipped);
        // Read back in the camera's orientation, the fit is unchanged; in another one it mirrors.
        assert_eq!(c.oriented(flipped).cx, c.cx);
        assert_eq!(c.oriented(Orientation::default()).cx, 1279.0 - 627.3);
    }

    #[test]
    fn a_user_calibration_always_wins() {
        let user = CameraCalibration { provenance: None, rms_px: Some(9.0), ..stored(1, 9.0) };
        let better = calibration_of(&fit("pinhole"), 1280, 800, Orientation::default(), "p").expect("valid");
        assert!(matches!(decide(Some(&user), &better), Decision::Keep(reason) if reason.contains("its own")));
        assert_eq!(decide(None, &better), Decision::Store, "nothing stored: store");
    }

    #[test]
    fn a_stored_self_calibration_is_replaced_only_by_a_fit_no_worse_and_better_on_one_count() {
        let fit_now = calibration_of(&fit("pinhole"), 1280, 800, Orientation::default(), "p").expect("valid"); // 24 views, 0.31 px
        assert_eq!(decide(Some(&stored(12, 0.40)), &fit_now), Decision::Store, "fewer views");
        assert_eq!(decide(Some(&stored(30, 0.50)), &fit_now), Decision::Store, "a higher rms (the fit is better)");
        assert!(matches!(decide(Some(&stored(24, 0.31)), &fit_now), Decision::Keep(_)), "the same fit: no store, no loop");
        assert!(matches!(decide(Some(&stored(30, 0.20)), &fit_now), Decision::Keep(_)), "more views and a lower rms already stored");
        assert!(matches!(decide(Some(&stored(12, 0.25)), &fit_now), Decision::Keep(_)), "more views never buy a worse rms");
        assert_eq!(decide(Some(&stored(24, 0.40)), &fit_now), Decision::Store, "the same views at a lower rms");
    }

    #[test]
    fn stored_calibrations_without_provenance_load_as_before() {
        let json = serde_json::json!({ "width": 1280, "height": 800, "model": "pinhole", "fx": 600.0, "fy": 600.0, "cx": 639.5, "cy": 399.5, "saved_at_ms": 7 });
        let loaded: CameraCalibration = serde_json::from_value(json).expect("loads");
        assert_eq!(loaded.provenance, None);
        assert!(!serde_json::to_value(&loaded).expect("json").as_object().expect("object").contains_key("provenance"));
        let with: CameraCalibration = serde_json::from_value(serde_json::to_value(stored(24, 0.3)).expect("json")).expect("round trip");
        assert_eq!(with.provenance.map(|p| (p.method, p.views)), Some((METHOD.to_string(), 24)));
    }

    fn pose_line(pipeline: &str, fit: &Fit) -> String {
        let value = serde_json::json!({ "frame": 10, "state": "tracking", "calibration": fit });
        let line = serde_json::json!({ "pipeline": pipeline, "port": "pose", "observed_at_ms": 5, "frame_ts_ns": 42, "value": value });
        format!("{line}\n")
    }

    fn test_state(dir: &std::path::Path) -> SharedState {
        AppState::new(ApiConfig {
            orion_socket: dir.join("no-orion.sock"),
            orion_stream_socket: dir.join("no-orion-stream.sock"),
            state_dir: dir.join("state"),
            ..ApiConfig::default()
        })
    }

    fn mappers() -> BTreeMap<String, LiveMapper> {
        BTreeMap::from([("live-map".to_string(), LiveMapper { camera: "camera.front".into(), size: Some((1280, 800)) })])
    }

    #[tokio::test]
    async fn a_converged_pose_on_the_relay_is_stored_once_per_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(dir.path());
        let mut handled = Handled::default();
        // Another pipeline's and a not-converged pose are ignored; the converged one is stored.
        let not_yet = Fit { converged: false, revision: Some(1), ..fit("pinhole") };
        let chunk = format!("{}{}{}", pose_line("live-map", &not_yet), pose_line("other", &fit("pinhole")), pose_line("live-map", &fit("pinhole")));
        handle_chunk(&state, &mappers(), &mut handled, &chunk).await;
        let stored = state.store.calibrations().await.expect("read").remove("camera.front").expect("stored");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].provenance.as_ref().map(|p| p.views), Some(24));
        let first_saved = stored[0].saved_at_ms;
        // The same revision again: not stored again.
        handle_chunk(&state, &mappers(), &mut handled, &pose_line("live-map", &fit("pinhole"))).await;
        let again = state.store.calibrations().await.expect("read").remove("camera.front").expect("stored");
        assert_eq!(again[0].saved_at_ms, first_saved);
    }

    #[tokio::test]
    async fn a_user_calibration_on_the_camera_is_not_replaced_by_the_relay() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(dir.path());
        let user = CameraCalibration { provenance: None, ..stored(1, 2.0) };
        state.store.put_calibration("camera.front", CameraCalibration { saved_at_ms: 1, ..user.clone() }).await.expect("put");
        handle_chunk(&state, &mappers(), &mut Handled::default(), &pose_line("live-map", &fit("pinhole"))).await;
        let kept = state.store.calibrations().await.expect("read").remove("camera.front").expect("kept");
        assert_eq!(kept, vec![CameraCalibration { saved_at_ms: 1, ..user }]);
    }

    #[tokio::test]
    async fn a_fit_and_mapper_without_a_frame_size_store_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(dir.path());
        let sizeless = BTreeMap::from([("live-map".to_string(), LiveMapper { camera: "camera.front".into(), size: None })]);
        let older_eidos = Fit { width: None, height: None, ..fit("pinhole") };
        handle_chunk(&state, &sizeless, &mut Handled::default(), &pose_line("live-map", &older_eidos)).await;
        assert!(state.store.calibrations().await.expect("read").is_empty());
        handle_chunk(&state, &sizeless, &mut Handled::default(), &pose_line("live-map", &fit("pinhole"))).await;
        let stored = state.store.calibrations().await.expect("read").remove("camera.front").unwrap_or_default();
        assert_eq!(stored.iter().map(|c| (c.width, c.height)).collect::<Vec<_>>(), vec![(1280, 800)], "the fit's own size");
    }
}
