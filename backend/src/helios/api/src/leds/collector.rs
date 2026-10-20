//! The collector: every tick it turns what the device knows into the director's [`Facts`]. Fast
//! facts (the tags, the field pose, the calibration's views) come from the event stream, which
//! the collector subscribes to while the LEDs are on. Slow facts (Orion's engine workloads and
//! the engine's runtime labels, the robot's NetworkTables connection, the calibration sessions,
//! the recorder, the calibrations) are read once a second.
//!
//! Two inputs are not published yet and read as absent: the robot's enable state
//! (`helios.robot.enabled=true` on `engine.runtime.<node>`) and the camera's lux
//! (`helios.camera.lux=<lux>` on the same resource). helios-engine publishes them once the robot's
//! NetworkTables subscription and the camera's AAA metadata are wired (see the docs' LED page).

use std::time::Duration;

use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use super::detections::{Detections, multi_tag_valid, parse_detections, pick_calibration, tag_observations};
use super::settings::LedSettings;
use super::states::{CALIBRATION_VIEWS, Calibration, Facts};
use crate::SharedState;
use crate::camera_context::{CameraCalibration, Orientation};
use crate::events::ApiEvent;
use crate::host::now_ms;
use crate::orion::{enum_name, label_values};
use crate::routes::pipelines::ENGINE_RUNTIME;

const TICK: Duration = Duration::from_millis(100);
/// Slow facts are read every this many ticks (one second).
const SLOW_EVERY: u64 = 10;
/// Detections older than this are gone from the frame.
const DETECTIONS_FRESH_MS: u64 = 700;
/// A valid field pose holds the lock for this long after its last valid solve.
const LOCK_HOLD_MS: u64 = 2000;
/// After start-up this long, with no engine running, the director stops saying "starting".
const STARTING_MS: u64 = 120_000;

/// Starts the collector (once; `leds::start`). It loads the saved settings into `settings`.
pub fn spawn(state: SharedState, facts: watch::Sender<Facts>, settings: watch::Sender<LedSettings>) -> JoinHandle<()> {
    tokio::spawn(collect(state, facts, settings))
}

/// The slow facts, sampled once a second.
#[derive(Debug, Default)]
struct Slow {
    engine_running: bool,
    engine_failed: bool,
    robot_missing: bool,
    robot_enabled: bool,
    lux: Option<f64>,
    recording: bool,
    calibration: Option<u32>,
    calibrations: Vec<CameraCalibration>,
}

/// The fast facts: the latest detections, and when the field pose was last valid.
#[derive(Debug, Default)]
struct Fast {
    detections: Option<(u64, Detections)>,
    locked_until: u64,
}

async fn collect(state: SharedState, facts: watch::Sender<Facts>, settings: watch::Sender<LedSettings>) {
    let started = now_ms();
    if let Ok(saved) = state.store.led_settings().await {
        settings.send_replace(saved);
    }
    let mut ticker = tokio::time::interval(TICK);
    let mut subscription: Option<broadcast::Receiver<ApiEvent>> = None;
    let mut fast = Fast::default();
    let mut slow = Slow::default();
    let mut tick: u64 = 0;
    loop {
        ticker.tick().await;
        if tick.is_multiple_of(SLOW_EVERY) {
            slow = sample_slow(&state).await;
        }
        tick += 1;
        let now = now_ms();
        let current = settings.borrow().clone();
        if current.enabled {
            let receiver = subscription.get_or_insert_with(|| state.events.subscribe());
            drain(receiver, &mut fast, now);
        } else {
            subscription = None;
            fast = Fast::default();
        }
        facts.send_replace(build(&slow, &fast, &current, now, started));
    }
}

/// The facts for this tick.
fn build(slow: &Slow, fast: &Fast, settings: &LedSettings, now: u64, started: u64) -> Facts {
    let detections = fast.detections.as_ref().filter(|(at, _)| now.saturating_sub(*at) < DETECTIONS_FRESH_MS).map(|(_, detections)| detections);
    let tags = detections
        .map(|detections| {
            let calibration = pick_calibration(&slow.calibrations, detections.width, detections.height);
            tag_observations(detections, calibration, settings.fov_deg)
        })
        .unwrap_or_default();
    Facts {
        starting: !slow.engine_running && now.saturating_sub(started) < STARTING_MS,
        error: slow.engine_failed,
        calibration: slow.calibration.map(|views| Calibration { progress: (f64::from(views) / f64::from(CALIBRATION_VIEWS)).min(1.0), views }),
        recording: slow.recording,
        robot_missing: slow.robot_missing,
        robot_enabled: slow.robot_enabled,
        searching: slow.engine_running && tags.is_empty(),
        tags,
        locked: now < fast.locked_until,
        lux: slow.lux,
    }
}

/// Takes the events waiting on the subscription. A lagging receiver skips what it missed; a closed
/// one (the watcher ended) is dropped by the next tick's `subscribe`.
fn drain(receiver: &mut broadcast::Receiver<ApiEvent>, fast: &mut Fast, now: u64) {
    loop {
        match receiver.try_recv() {
            Ok(event) => absorb(fast, &event, now),
            Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
}

/// Folds one event into the fast facts: `output` events carry a pipeline's port as `value`.
fn absorb(fast: &mut Fast, event: &ApiEvent, now: u64) {
    if event.kind != "output" {
        return;
    }
    let Some(output) = event.data.get("value") else { return };
    match output.get("port").and_then(|port| port.as_str()) {
        Some("detections") => {
            if let Some(detections) = output.get("value").and_then(parse_detections) {
                fast.detections = Some((now, detections));
            }
        }
        Some("multi_tag_pose") if output.get("value").is_some_and(multi_tag_valid) => {
            fast.locked_until = now + LOCK_HOLD_MS;
        }
        _ => {}
    }
}

async fn sample_slow(state: &SharedState) -> Slow {
    let mut slow = Slow::default();
    let nt4 = state.store.nt4_settings().await.unwrap_or_default();
    // Each camera's calibrations, turned into the orientation its frames are read out in now.
    for (camera, calibrations) in state.store.calibrations().await.unwrap_or_default() {
        let orientation = Orientation::from_settings(&state.store.camera_settings(&camera).await.unwrap_or_default());
        slow.calibrations.extend(calibrations.iter().map(|calibration| calibration.oriented(orientation)));
    }
    slow.calibration = state.calibrations.most_views().await;
    slow.recording = state.recorder.status().is_some_and(|status| status.state.is_active());
    // Without Orion the engine is not running, and robot code is not connected (if it is wanted).
    slow.robot_missing = nt4.host().is_some();
    let Ok(view) = state.orion.view().await else { return slow };
    let runtime = view.resources.get(&format!("engine.runtime.{}", state.config.node_id));
    let labels: &[String] = runtime.map(|resource| resource.labels.as_slice()).unwrap_or(&[]);
    slow.robot_missing = nt4.host().is_some() && label_values(labels, "helios.nt4.connected").first() != Some(&"true");
    slow.robot_enabled = label_values(labels, "helios.robot.enabled").first() == Some(&"true");
    slow.lux = label_values(labels, "helios.camera.lux").first().and_then(|lux| lux.parse().ok());
    let engines = view.workloads.values().filter(|workload| workload.runtime_type.as_str() == ENGINE_RUNTIME);
    let mut running = false;
    let mut failed = false;
    for workload in engines {
        match enum_name(workload.observed_state).as_str() {
            "running" => running = true,
            "failed" => failed = true,
            _ => {}
        }
    }
    slow.engine_running = running;
    slow.engine_failed = failed;
    slow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> LedSettings {
        LedSettings::default()
    }

    fn output(port: &str, value: serde_json::Value) -> ApiEvent {
        ApiEvent {
            kind: "output".into(),
            at_ms: 0,
            data: serde_json::json!({ "id": format!("p/{port}"), "change": "updated", "value": { "pipeline": "p", "port": port, "value": value, "observed_at_ms": 0 } }),
        }
    }

    fn detections_value() -> serde_json::Value {
        serde_json::json!({ "frame_width": 640, "frame_height": 400, "detections": [
            { "id": 3, "corners": [{ "x": 100.0, "y": 100.0 }, { "x": 140.0, "y": 100.0 }, { "x": 140.0, "y": 140.0 }, { "x": 100.0, "y": 140.0 }] }
        ] })
    }

    #[test]
    fn detections_become_tags_while_fresh_and_then_go() {
        let mut fast = Fast::default();
        absorb(&mut fast, &output("detections", detections_value()), 1000);
        let slow = Slow::default();
        let fresh = build(&slow, &fast, &settings(), 1200, 0);
        assert_eq!(fresh.tags.len(), 1);
        assert_eq!(fresh.tags[0].id, 3);
        assert!(fresh.tags[0].bearing_deg < 0.0, "the tag is left of centre (x = 120 of 640)");
        let stale = build(&slow, &fast, &settings(), 1000 + DETECTIONS_FRESH_MS, 0);
        assert!(stale.tags.is_empty());
    }

    #[test]
    fn a_valid_field_pose_holds_the_lock_for_a_while() {
        let mut fast = Fast::default();
        absorb(&mut fast, &output("multi_tag_pose", serde_json::json!({ "valid": true })), 1000);
        assert!(build(&Slow::default(), &fast, &settings(), 2000, 0).locked);
        assert!(!build(&Slow::default(), &fast, &settings(), 1000 + LOCK_HOLD_MS, 0).locked);
        absorb(&mut fast, &output("multi_tag_pose", serde_json::json!({ "valid": false })), 1500);
        assert!(build(&Slow::default(), &fast, &settings(), 2000, 0).locked, "an invalid solve does not drop the lock early");
    }

    #[test]
    fn removed_outputs_and_other_ports_are_ignored() {
        let mut fast = Fast::default();
        let removed = ApiEvent { kind: "output".into(), at_ms: 0, data: serde_json::json!({ "id": "p/detections", "change": "removed" }) };
        absorb(&mut fast, &removed, 1000);
        absorb(&mut fast, &output("refined_corners", detections_value()), 1000);
        assert!(fast.detections.is_none());
    }

    #[test]
    fn the_engine_states_map_onto_the_facts() {
        let slow = Slow { engine_running: true, robot_missing: true, calibration: Some(6), ..Slow::default() };
        let facts = build(&slow, &Fast::default(), &settings(), 5000, 0);
        assert!(facts.searching && facts.robot_missing && !facts.starting);
        assert_eq!(facts.calibration.map(|c| c.views), Some(6));
        assert!((facts.calibration.map_or(0.0, |c| c.progress) - 0.5).abs() < 1e-9);
        let booting = build(&Slow::default(), &Fast::default(), &settings(), 5000, 0);
        assert!(booting.starting && !booting.searching);
        let late = build(&Slow::default(), &Fast::default(), &settings(), STARTING_MS + 1, 0);
        assert!(!late.starting, "after the start-up window, with no engine, it is idle");
    }
}
