//! A camera's status chain: each stage between the camera and what its pipelines make of it (tags,
//! calibration, the live map, depth, features), with its state, why, what to do about it, and
//! progress where it is a count. The UI shows the chain wherever a camera has nothing to show
//! (`GET /v1/cameras/{id}/status`, and each camera in `GET /v1/cameras`); a pipeline's own line
//! of it is `Pipeline::status`.
//!
//! The rules are a pure function of the facts the routes gather (`derive`, `pipeline_stage`), so
//! each one is a unit test. The facts only the device knows come from the camera service (a
//! capture's last error) and from `/proc` (another process holding the camera's device node).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::routes::{cameras::CaptureLive, pipelines::Pipeline};

/// The tag family FRC fields are printed with. A pipeline that decodes another family never sees
/// the field's tags, whatever their size.
pub const FIELD_TAG_FAMILY: &str = "apriltag_36h11";
/// Eidos's default `calibration_frames`: the frames the live mapper fits its lens from.
pub const DEFAULT_CALIBRATION_FRAMES: u64 = 150;
/// The analogue gain (x) from which the image is noisy enough to say so.
const NOISY_GAIN: f64 = 8.0;
/// How many tag ids a stage names before it counts the rest.
const IDS_NAMED: usize = 8;

/// The state of a stage. `off` stages do not apply (no pipeline needs them); `waiting` is progress
/// toward ok (bootstrapping, no frames yet); `blocked` stops the stages after it.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StageState {
    Ok,
    Waiting,
    Blocked,
    Off,
}

impl StageState {
    /// The order the worst state of several is picked by.
    fn rank(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Ok => 1,
            Self::Waiting => 2,
            Self::Blocked => 3,
        }
    }
}

/// How far a stage is toward its goal, in its own unit (`frames` of a calibration's `target`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Progress {
    pub value: u64,
    pub target: u64,
    pub unit: &'static str,
}

/// One stage of the chain.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Stage {
    /// The stable id: `camera`, `service`, `frames`, `pipeline`, `detections`, `calibration`, `map`,
    /// `depth` or `features`.
    pub stage: &'static str,
    pub label: &'static str,
    pub state: StageState,
    /// What the stage is doing, or why it is not working.
    pub reason: String,
    /// What to do about it, when there is something to do.
    pub hint: Option<String>,
    pub progress: Option<Progress>,
}

impl Stage {
    fn new(stage: &'static str, label: &'static str, state: StageState, reason: impl Into<String>) -> Self {
        Self { stage, label, state, reason: reason.into(), hint: None, progress: None }
    }

    pub fn off(stage: &'static str, label: &'static str, reason: impl Into<String>) -> Self {
        Self::new(stage, label, StageState::Off, reason)
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn progress(mut self, value: u64, target: u64, unit: &'static str) -> Self {
        self.progress = Some(Progress { value, target, unit });
        self
    }
}

/// A camera's chain, and the stage it stops at.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraStatus {
    pub stages: Vec<Stage>,
    /// The first stage that is waiting or blocked (`null` when every stage that applies is ok).
    pub blocking: Option<&'static str>,
    /// One line for the UI: the blocking stage's label and reason.
    pub summary: String,
}

/// What the camera service and the device say about one camera.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraFacts {
    /// The camera has a Styx camera service (a frames endpoint).
    pub service: bool,
    /// Whether the API's control client is connected to the service (`None`: it keeps none).
    pub service_online: Option<bool>,
    /// Why the service's facts could not be read.
    pub live_error: Option<String>,
    /// The camera's first capture in the service: its frames, rate and 3A.
    pub capture: Option<CaptureLive>,
    /// The device node (`/dev/videoN`), when the camera has one.
    pub device: Option<PathBuf>,
    /// Other programs holding the device node, by name (empty when none, or not looked for).
    pub holders: Vec<String>,
    /// The calibration stored for the frame size the pipelines take.
    pub calibration: Option<CalibrationFact>,
    /// Why the last self-calibration fit was not stored, when one was refused.
    pub calibration_note: Option<String>,
}

/// A calibration the camera uses: the user's, or one the live mapper fitted.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationFact {
    pub self_calibrated: bool,
    pub rms_px: Option<f64>,
    pub views: Option<u32>,
    pub width: u32,
    pub height: u32,
}

/// One pipeline that takes the camera, as the chain reads it. Built from the pipeline's DTO.
#[derive(Debug, Clone, Default)]
pub struct PipelineFacts<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub state: &'a str,
    pub graph_error: Option<&'a str>,
    /// The session's status and message (for a running pipeline the message is its telemetry).
    pub session: Option<(&'a str, Option<&'a str>)>,
    pub telemetry: Option<&'a serde_json::Value>,
    /// The tag family the detector decodes (`apriltag_36h11`).
    pub dictionary: Option<&'a str>,
    /// Whether the pipeline's multi-tag pose solves against a field layout.
    pub field_layout: bool,
    /// The pyramid level the camera's input takes (the detector reads that level of the frame).
    pub pyramid: Option<u8>,
    /// The pose status (`calibrated`, `uncalibrated`, `image_size_mismatch`).
    pub pose_status: Option<&'a str>,
    /// The latest value of each host output, by port.
    pub outputs: BTreeMap<&'a str, &'a serde_json::Value>,
    /// The graph has the live mapper, the dense mapper, or the feature tracker.
    pub live_map: bool,
    pub dense: bool,
    pub features: bool,
    /// The live mapper's `calibration_frames` (its window), from the graph.
    pub calibration_frames: Option<u64>,
}

impl<'a> PipelineFacts<'a> {
    /// The facts of a pipeline's DTO.
    pub fn of(pipeline: &'a Pipeline) -> Self {
        let graph = pipeline.graph.as_ref();
        let node = |id: &str| graph_node(graph, id);
        Self {
            name: &pipeline.name,
            enabled: pipeline.enabled,
            state: &pipeline.state,
            graph_error: pipeline.graph_error.as_deref(),
            session: pipeline.session.as_ref().map(|s| (s.status.as_str(), s.message.as_deref())),
            telemetry: pipeline.telemetry.as_ref(),
            dictionary: pipeline.dictionary.as_deref(),
            field_layout: pipeline.field_layout.is_some(),
            pyramid: pipeline.bindings.values().filter_map(|b| b.pyramid).max(),
            pose_status: pipeline.pose.as_ref().and_then(|p| p.get("status")).and_then(|s| s.as_str()),
            outputs: pipeline.outputs.iter().map(|o| (o.port.as_str(), &o.value)).collect(),
            live_map: node(crate::field_layouts::LIVE_MAP_NODE).is_some(),
            dense: node(DENSE_NODE).is_some(),
            features: node(FEATURES_NODE).is_some(),
            calibration_frames: node(crate::field_layouts::LIVE_MAP_NODE).and_then(|n| const_int(n, "calibration_frames")),
        }
    }

    fn output(&self, port: &str) -> Option<&'a serde_json::Value> {
        self.outputs.get(port).copied()
    }

    fn map_pose(&self) -> Option<&'a serde_json::Value> {
        self.output("map_pose")
    }
}

const DENSE_NODE: &str = "eidos:dense.map";
const FEATURES_NODE: &str = "eidos:features.track";

/// The node of a graph document whose id is `id`.
fn graph_node<'a>(graph: Option<&'a serde_json::Value>, id: &str) -> Option<&'a serde_json::Value> {
    graph?.get("graph")?.get("nodes")?.as_array()?.iter().find(|node| node.get("id").and_then(|v| v.as_str()) == Some(id))
}

/// An integer constant of a graph node (`const_inputs: [[name, {type, value}]]`).
fn const_int(node: &serde_json::Value, name: &str) -> Option<u64> {
    let consts = node.get("const_inputs")?.as_array()?;
    let value = consts.iter().find(|entry| entry.get(0).and_then(|n| n.as_str()) == Some(name))?.get(1)?.get("value")?;
    value.as_u64()
}

/// The length of a list in an output's value: the value itself, or its `key` member.
fn list_len(value: &serde_json::Value, key: &str) -> Option<usize> {
    value.as_array().or_else(|| value.get(key).and_then(|v| v.as_array())).map(Vec::len)
}

/// The ids of the detections in a `detections` output (distinct, in order).
fn tag_ids(value: &serde_json::Value) -> Vec<u64> {
    let mut ids: Vec<u64> = value.get("detections").and_then(|d| d.as_array()).into_iter().flatten().filter_map(|d| d.get("id").and_then(|id| id.as_u64())).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn is_busy(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("busy") || text.contains("ebusy")
}

/// A program's name as the chain shows it, with what it is for when it is one we know.
pub fn holder_label(name: &str) -> String {
    match name {
        "visionserver" => "visionserver (Limelight vision)".into(),
        "photonvision" => "PhotonVision".into(),
        other => other.into(),
    }
}

/// The processes holding `device` open, by name. Only processes this user can read are seen (the
/// API runs as root on a device). HeliOS's own services (`helios-*`, the camera service is in
/// helios-peripherals) are not holders the chain reports.
pub fn device_holders(device: &Path) -> Vec<String> {
    let Ok(target) = fs::canonicalize(device) else { return Vec::new() };
    let Ok(procs) = fs::read_dir("/proc") else { return Vec::new() };
    let mut names: Vec<String> = procs
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().is_some_and(|pid| pid.bytes().all(|b| b.is_ascii_digit())))
        .filter_map(|entry| {
            let pid = entry.path();
            let name = fs::read_to_string(pid.join("comm")).ok()?.trim().to_string();
            if name.starts_with("helios-") {
                return None;
            }
            let holds = fs::read_dir(pid.join("fd")).ok()?.filter_map(Result::ok).any(|fd| fs::read_link(fd.path()).is_ok_and(|link| link == target));
            holds.then_some(name)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The status of one pipeline: its own line of the chain (graph, run state, errors, source).
pub fn pipeline_stage(p: &PipelineFacts) -> Stage {
    const STAGE: (&str, &str) = ("pipeline", "Pipeline");
    let stage = |state, reason: String| Stage::new(STAGE.0, STAGE.1, state, reason);
    if let Some(error) = p.graph_error {
        return stage(StageState::Blocked, format!("The graph does not load: {error}")).hint("Open the pipeline's graph and fix it, or deploy a template again.");
    }
    if !p.enabled || p.state == "stopped" {
        return stage(StageState::Blocked, "Stopped".into()).hint("Start it in Pipelines.");
    }
    match p.state {
        "failed" => {
            let message = match p.session {
                Some(("failed" | "error", Some(message))) => message.to_string(),
                _ => telemetry_text(p, "last_error").unwrap_or_else(|| "the engine stopped it".into()),
            };
            stage(StageState::Blocked, format!("Failed: {message}")).hint("Restart it. If it fails again, its error is in the Logs.")
        }
        "running" => running_stage(p),
        other => stage(StageState::Waiting, format!("Starting ({other})")),
    }
}

fn telemetry_text(p: &PipelineFacts, key: &str) -> Option<String> {
    p.telemetry?.get(key)?.as_str().map(str::to_string)
}

fn running_stage(p: &PipelineFacts) -> Stage {
    const STAGE: (&str, &str) = ("pipeline", "Pipeline");
    let telemetry = p.telemetry;
    let failed_ticks = telemetry.and_then(|t| t.get("ticks_failed")).and_then(|v| v.as_u64()).unwrap_or(0);
    if failed_ticks > 0
        && let Some(error) = telemetry_text(p, "last_error")
    {
        return Stage::new(STAGE.0, STAGE.1, StageState::Blocked, format!("Frames fail: {error}")).hint("Each frame is dropped until this is fixed. The error is in the Logs.");
    }
    if telemetry.and_then(|t| t.get("source_connected")).and_then(|v| v.as_bool()) == Some(false) {
        return Stage::new(STAGE.0, STAGE.1, StageState::Waiting, "Waiting for the camera's frames").hint("The pipeline reconnects to the camera service on its own.");
    }
    let fps = telemetry.and_then(|t| t.get("fps")).and_then(|v| v.as_f64()).filter(|fps| *fps > 0.0);
    let reason = match fps {
        Some(fps) => format!("Running at {fps:.0} fps"),
        None => "Running".into(),
    };
    Stage::new(STAGE.0, STAGE.1, StageState::Ok, reason)
}

/// The pipelines' own line for the chain: the worst of them, named.
fn pipelines_stage(pipelines: &[PipelineFacts]) -> Stage {
    if pipelines.is_empty() {
        return Stage::off("pipeline", "Pipeline", "No pipeline uses this camera")
            .hint("The view shows the camera's frames as they are. Bind the camera to a pipeline's camera input to get tags, poses or a map.");
    }
    let worst = pipelines.iter().map(|p| (p, pipeline_stage(p))).reduce(|worst, next| if next.1.state.rank() > worst.1.state.rank() { next } else { worst });
    let (p, stage) = worst.expect("not empty");
    Stage { reason: format!("{}: {}", p.name, stage.reason), ..stage }
}

fn camera_stage(camera: &CameraFacts) -> Stage {
    if !camera.service {
        return Stage::new("camera", "Camera", StageState::Blocked, "The camera has no camera service, so it has no frames")
            .hint("Check the camera is plugged in and listed under Peripherals. Only cameras with a camera service show frames here.");
    }
    let reason = match &camera.device {
        Some(device) => format!("Connected at {}", device.display()),
        None => "Connected".into(),
    };
    Stage::new("camera", "Camera", StageState::Ok, reason)
}

fn service_stage(camera: &CameraFacts) -> Stage {
    if !camera.service {
        return Stage::off("service", "Camera service", "There is no camera service");
    }
    if let Some(error) = &camera.live_error {
        return Stage::new("service", "Camera service", StageState::Blocked, format!("The camera service did not answer: {error}")).hint("Look at the camera service under System > Services, and at the Logs.");
    }
    if camera.service_online == Some(false) {
        return Stage::new("service", "Camera service", StageState::Blocked, "The API has lost the camera service").hint("It is restarting or stopped. Check System > Services.");
    }
    Stage::new("service", "Camera service", StageState::Ok, "Online")
}

fn frames_stage(camera: &CameraFacts) -> Stage {
    const STAGE: (&str, &str) = ("frames", "Frames");
    let stage = |state, reason: String| Stage::new(STAGE.0, STAGE.1, state, reason);
    if !camera.service {
        return Stage::off(STAGE.0, STAGE.1, "No frames without a camera service");
    }
    let device = camera.device.as_ref().map_or_else(|| "the camera".to_string(), |d| d.display().to_string());
    let Some(capture) = &camera.capture else {
        if !camera.holders.is_empty() {
            return busy_stage(STAGE, camera, &device);
        }
        return stage(StageState::Waiting, "No capture yet".into()).hint("The capture starts when the camera view or a pipeline uses the camera.");
    };
    let fps = capture.fps_measured.filter(|fps| *fps > 0.0);
    if fps.is_some() || capture.frames_delivered > 0 {
        let reason = fps.map_or_else(|| "Frames flowing".into(), |fps| format!("{fps:.1} fps"));
        return stage(StageState::Ok, reason);
    }
    let busy = !camera.holders.is_empty() || capture.last_error.as_deref().is_some_and(is_busy);
    if busy {
        return busy_stage(STAGE, camera, &device);
    }
    match &capture.last_error {
        Some(error) => stage(StageState::Blocked, format!("The capture failed: {error}")).hint("Check the camera's USB connection, and that its mode is one it supports (Camera settings)."),
        None => stage(StageState::Waiting, "Capture started, no frames yet".into()).hint("Wait a moment. If it stays this way, check the camera's USB connection."),
    }
}

fn busy_stage(stage: (&'static str, &'static str), camera: &CameraFacts, device: &str) -> Stage {
    if camera.holders.is_empty() {
        return Stage::new(stage.0, stage.1, StageState::Blocked, format!("The device is busy: another program has {device} open"))
            .hint(format!("Find it with `fuser {device}` and stop its capture. HeliOS takes the camera once it is free."));
    }
    let holders: Vec<String> = camera.holders.iter().map(|name| holder_label(name)).collect();
    Stage::new(stage.0, stage.1, StageState::Blocked, format!("In use by {}", holders.join(", ")))
        .hint("Turn off that program's capture of this camera. HeliOS takes the camera once it is free.")
}

fn detections_stage(camera: &CameraFacts, pipelines: &[PipelineFacts]) -> Stage {
    const STAGE: (&str, &str) = ("detections", "Tags");
    let detectors: Vec<&PipelineFacts> = pipelines.iter().filter(|p| p.dictionary.is_some()).collect();
    let Some(first) = detectors.first() else {
        return Stage::off(STAGE.0, STAGE.1, "No pipeline on this camera detects tags");
    };
    let ids = {
        let mut ids: Vec<u64> = detectors.iter().filter_map(|p| p.output("detections")).flat_map(tag_ids).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    if !ids.is_empty() {
        let named: Vec<String> = ids.iter().take(IDS_NAMED).map(u64::to_string).collect();
        let more = ids.len().saturating_sub(IDS_NAMED);
        let list = if more > 0 { format!("{} and {more} more", named.join(", ")) } else { named.join(", ") };
        return Stage::new(STAGE.0, STAGE.1, StageState::Ok, format!("{} tags in view: ids {list}", ids.len()));
    }
    let dictionary = first.dictionary.unwrap_or_default();
    if first.field_layout && dictionary != FIELD_TAG_FAMILY {
        return Stage::new(STAGE.0, STAGE.1, StageState::Blocked, format!("No tags: the pipeline decodes {dictionary}, and the field's tags are {FIELD_TAG_FAMILY}"))
            .hint("Set the pipeline's tag family to apriltag_36h11 in its settings. Tags of another family are never decoded.");
    }
    let stage = Stage::new(STAGE.0, STAGE.1, StageState::Waiting, "No tags seen yet");
    let rejected = first.output("rejected").and_then(|value| list_len(value, "rejected")).unwrap_or(0);
    let gain = camera.capture.as_ref().and_then(|c| c.analogue_gain).filter(|gain| *gain >= NOISY_GAIN);
    let searching = camera.capture.as_ref().and_then(|c| c.ae_state.as_deref()) == Some("searching");
    if rejected > 0 {
        return stage.hint(format!("{rejected} outlines were found and rejected: check the focus, the exposure and that the tags are printed with a black border."));
    }
    if let Some(gain) = gain {
        return stage.hint(format!("The gain is {gain:.1}x, so the image is noisy. Add light, or lower the gain in the camera settings."));
    }
    if searching {
        return stage.hint("The exposure is still adjusting. Hold a tag in view for a moment.");
    }
    if first.pyramid.is_some_and(|level| level > 0) {
        return stage.hint(format!("The detector reads pyramid level {}, a 1/{} scale of the frame. A tag must still span several cells at that size.", first.pyramid.unwrap_or(0), 1u32 << first.pyramid.unwrap_or(0)));
    }
    stage.hint(format!("Point the camera at a tag, and check it is the {dictionary} family. Tags must be printed with a black border."))
}

fn calibration_stage(camera: &CameraFacts, pipelines: &[PipelineFacts]) -> Stage {
    const STAGE: (&str, &str) = ("calibration", "Calibration");
    let stage = |state, reason: String| Stage::new(STAGE.0, STAGE.1, state, reason);
    if pipelines.is_empty() && camera.calibration.is_none() {
        return Stage::off(STAGE.0, STAGE.1, "No pipeline uses this camera");
    }
    if pipelines.iter().any(|p| p.pose_status == Some("image_size_mismatch")) {
        return stage(StageState::Blocked, "The calibration is for another frame size".into()).hint("Calibrate at this size in the Calibration pane, or change the camera input's size.");
    }
    if let Some(c) = &camera.calibration {
        let rms = c.rms_px.map_or(String::new(), |rms| format!(", {rms:.2} px rms"));
        let reason = if c.self_calibrated {
            format!("Self-calibrated from {} views{rms} (at {}x{})", c.views.unwrap_or(0), c.width, c.height)
        } else {
            format!("Calibrated by you{rms} (at {}x{})", c.width, c.height)
        };
        return stage(StageState::Ok, reason);
    }
    let note = camera.calibration_note.as_ref().map(|note| format!(" Last fit not stored: {note}."));
    let mapper = pipelines.iter().find(|p| p.map_pose().is_some() || p.live_map);
    let pose = mapper.and_then(|p| p.map_pose());
    if let Some(p) = mapper {
        let target = p.calibration_frames.unwrap_or(DEFAULT_CALIBRATION_FRAMES);
        let frames = pose.and_then(|pose| pose.pointer("/calibration/frames")).and_then(|v| v.as_u64()).unwrap_or(0);
        let views = pose.and_then(|pose| pose.pointer("/calibration/views")).and_then(|v| v.as_u64()).unwrap_or(0);
        let hint = format!("Move the camera so the tags are seen from many sides. The lens is fitted from what the mapper sees.{}", note.unwrap_or_default());
        return stage(StageState::Waiting, format!("Self-calibrating: {frames} of {target} frames, {views} views so far")).progress(frames.min(target), target, "frames").hint(hint);
    }
    stage(StageState::Waiting, "Uncalibrated: poses are not in metres".into())
        .hint(format!("Capture a calibration in the Calibration pane, or add a live map to fit one from the tags.{}", note.unwrap_or_default()))
}

fn map_stage(pipelines: &[PipelineFacts]) -> Stage {
    const STAGE: (&str, &str) = ("map", "Map");
    let stage = |state, reason: String| Stage::new(STAGE.0, STAGE.1, state, reason);
    let Some(mapper) = pipelines.iter().find(|p| p.live_map || p.map_pose().is_some()) else {
        return Stage::off(STAGE.0, STAGE.1, "No live map on this camera");
    };
    let Some(pose) = mapper.map_pose() else {
        return stage(StageState::Waiting, "The mapper has no pose yet".into()).hint("It starts from the first tags it sees. Check the Tags stage.");
    };
    let state = pose.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let reason = pose.get("reason").and_then(|v| v.as_str()).unwrap_or("").trim();
    let with_reason = |text: &str| if reason.is_empty() { text.to_string() } else { format!("{text}: {reason}") };
    match state {
        "tracking" | "relocalized" => stage(StageState::Ok, if state == "tracking" { "Tracking".into() } else { "Relocalized".into() }),
        "bootstrapping" => stage(StageState::Waiting, with_reason("Bootstrapping")).hint("The map starts from the first calibrated tags. Show a tag, or the field, to the camera."),
        "coasting" => stage(StageState::Waiting, with_reason("Coasting, no tags this frame")).hint("The pose is predicted until a tag is seen again."),
        "lost" => stage(StageState::Blocked, with_reason("Lost")).hint("Show the tags again to relocalize. If it stays lost, restart the pipeline."),
        other => stage(StageState::Waiting, with_reason(&format!("Mapper state {other}"))),
    }
}

fn depth_stage(pipelines: &[PipelineFacts]) -> Stage {
    const STAGE: (&str, &str) = ("depth", "Depth and mesh");
    let Some(dense) = pipelines.iter().find(|p| p.dense || p.output("dense_depth").is_some()) else {
        return Stage::off(STAGE.0, STAGE.1, "No dense mapper on this camera");
    };
    if dense.output("dense_depth").is_some() {
        let keyframes = dense.output("dense_mesh").and_then(|mesh| mesh.get("keyframes")).and_then(|v| v.as_u64());
        let reason = keyframes.map_or_else(|| "Depth is being built".into(), |k| format!("Depth from {k} keyframes"));
        return Stage::new(STAGE.0, STAGE.1, StageState::Ok, reason);
    }
    let keyframes = dense.output("dense_mesh").and_then(|mesh| mesh.get("keyframes")).and_then(|v| v.as_u64()).unwrap_or(0);
    let tracking = dense.map_pose().and_then(|pose| pose.get("state")).and_then(|v| v.as_str()) == Some("tracking");
    let hint = if tracking { "Depth needs motion: move the camera slowly so keyframes are taken from new views." } else { "The dense mapper needs the live map tracking first. See the Map stage." };
    Stage::new(STAGE.0, STAGE.1, StageState::Waiting, format!("No depth yet: {keyframes} keyframes")).hint(hint)
}

fn features_stage(pipelines: &[PipelineFacts]) -> Stage {
    const STAGE: (&str, &str) = ("features", "Features");
    let Some(tracker) = pipelines.iter().find(|p| p.features || p.output("tracks").is_some()) else {
        return Stage::off(STAGE.0, STAGE.1, "No feature tracker on this camera");
    };
    match tracker.output("tracks").and_then(|value| list_len(value, "tracks")) {
        Some(tracks) if tracks > 0 => Stage::new(STAGE.0, STAGE.1, StageState::Ok, format!("{tracks} feature tracks")),
        _ => Stage::new(STAGE.0, STAGE.1, StageState::Waiting, "No feature tracks yet").hint("Features need texture in view and camera motion. They do not need tags."),
    }
}

/// The chain of a camera from its facts and the facts of the pipelines that take it.
pub fn derive(camera: &CameraFacts, pipelines: &[PipelineFacts]) -> CameraStatus {
    let stages = vec![
        camera_stage(camera),
        service_stage(camera),
        frames_stage(camera),
        pipelines_stage(pipelines),
        detections_stage(camera, pipelines),
        calibration_stage(camera, pipelines),
        map_stage(pipelines),
        depth_stage(pipelines),
        features_stage(pipelines),
    ];
    let blocking = stages.iter().find(|s| matches!(s.state, StageState::Blocked | StageState::Waiting)).map(|s| s.stage);
    let summary = match stages.iter().find(|s| Some(s.stage) == blocking) {
        Some(stage) => format!("{}: {}", stage.label, stage.reason),
        None if pipelines.is_empty() => "Frames flow. No pipeline uses this camera.".into(),
        None => "Working: every stage that applies is running.".into(),
    };
    CameraStatus { stages, blocking, summary }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn capture(fps: Option<f64>, delivered: u64, last_error: Option<&str>) -> CaptureLive {
        CaptureLive {
            name: "046d:0825".into(),
            backend: "v4l2".into(),
            mode: "MJPG 1280x960".into(),
            fps_configured: Some(30.0),
            fps_measured: fps,
            frames_delivered: delivered,
            drops: 0,
            latency_p50_ms: None,
            latency_p95_ms: None,
            cpu_per_frame_us: None,
            exposure_us: None,
            analogue_gain: None,
            digital_gain: None,
            ae_state: None,
            last_error: last_error.map(str::to_string),
        }
    }

    fn served(capture: Option<CaptureLive>) -> CameraFacts {
        CameraFacts { service: true, service_online: Some(true), capture, device: Some(PathBuf::from("/dev/video0")), ..CameraFacts::default() }
    }

    fn stage<'a>(status: &'a CameraStatus, id: &str) -> &'a Stage {
        status.stages.iter().find(|s| s.stage == id).expect("stage")
    }

    fn running() -> serde_json::Value {
        json!({ "fps": 24.0, "ticks_failed": 0, "source_connected": true, "last_error": null })
    }

    #[test]
    fn a_camera_with_no_pipeline_shows_frames_and_nothing_downstream() {
        let status = derive(&served(Some(capture(Some(24.0), 900, None))), &[]);
        assert_eq!(stage(&status, "frames").state, StageState::Ok);
        assert_eq!(stage(&status, "frames").reason, "24.0 fps");
        assert_eq!(stage(&status, "pipeline").state, StageState::Off);
        assert_eq!(stage(&status, "detections").state, StageState::Off);
        assert_eq!(status.blocking, None);
        assert_eq!(status.summary, "Frames flow. No pipeline uses this camera.");
    }

    #[test]
    fn a_camera_held_by_another_program_is_blocked_at_frames_and_names_it() {
        let camera = CameraFacts { holders: vec!["visionserver".into()], ..served(Some(capture(None, 0, Some("ioctl VIDIOC_STREAMON: Device or resource busy (EBUSY)")))) };
        let status = derive(&camera, &[]);
        let frames = stage(&status, "frames");
        assert_eq!(frames.state, StageState::Blocked);
        assert_eq!(frames.reason, "In use by visionserver (Limelight vision)");
        assert!(frames.hint.as_deref().unwrap_or_default().contains("Turn off that program's capture"));
        assert_eq!(status.blocking, Some("frames"));
        assert_eq!(status.summary, "Frames: In use by visionserver (Limelight vision)");
    }

    #[test]
    fn a_busy_device_with_no_known_holder_says_so_and_how_to_find_it() {
        let camera = served(Some(capture(None, 0, Some("device is busy: /dev/video0"))));
        let frames = stage(&derive(&camera, &[]), "frames").clone();
        assert_eq!(frames.state, StageState::Blocked);
        assert!(frames.reason.starts_with("The device is busy"), "{}", frames.reason);
        assert!(frames.hint.unwrap_or_default().contains("fuser /dev/video0"));
    }

    #[test]
    fn a_capture_that_fails_without_being_busy_is_blocked_with_its_error() {
        let frames = stage(&derive(&served(Some(capture(None, 0, Some("no such mode")))), &[]), "frames").clone();
        assert_eq!((frames.state, frames.reason.as_str()), (StageState::Blocked, "The capture failed: no such mode"));
    }

    #[test]
    fn a_camera_the_service_has_not_started_is_waiting_not_broken() {
        let frames = stage(&derive(&served(None), &[]), "frames").clone();
        assert_eq!(frames.state, StageState::Waiting);
        assert!(frames.hint.unwrap_or_default().contains("camera view or a pipeline"));
    }

    #[test]
    fn a_camera_service_that_does_not_answer_blocks_the_chain() {
        let camera = CameraFacts { live_error: Some("camera service did not answer in time".into()), ..served(None) };
        let status = derive(&camera, &[]);
        assert_eq!(stage(&status, "service").state, StageState::Blocked);
        assert_eq!(status.blocking, Some("service"));
    }

    #[test]
    fn a_tag_family_other_than_the_fields_is_blocked_with_the_fix() {
        let telemetry = running();
        let p = PipelineFacts { name: "default", enabled: true, state: "running", telemetry: Some(&telemetry), dictionary: Some("apriltag_16h5"), field_layout: true, ..PipelineFacts::default() };
        let status = derive(&served(Some(capture(Some(24.0), 1, None))), std::slice::from_ref(&p));
        let tags = stage(&status, "detections");
        assert_eq!(tags.state, StageState::Blocked);
        assert_eq!(tags.reason, "No tags: the pipeline decodes apriltag_16h5, and the field's tags are apriltag_36h11");
        assert_eq!(status.blocking, Some("detections"));
    }

    #[test]
    fn tags_seen_are_named_and_counted() {
        let telemetry = running();
        let detections = json!({ "detections": [{ "id": 7, "corners": [] }, { "id": 1, "corners": [] }, { "id": 7, "corners": [] }] });
        let p = PipelineFacts {
            name: "default",
            enabled: true,
            state: "running",
            telemetry: Some(&telemetry),
            dictionary: Some("apriltag_36h11"),
            field_layout: true,
            outputs: BTreeMap::from([("detections", &detections)]),
            ..PipelineFacts::default()
        };
        let tags = stage(&derive(&served(Some(capture(Some(24.0), 1, None))), std::slice::from_ref(&p)), "detections").clone();
        assert_eq!((tags.state, tags.reason.as_str()), (StageState::Ok, "2 tags in view: ids 1, 7"));
    }

    #[test]
    fn no_tags_hints_at_the_pyramid_and_the_family() {
        let telemetry = running();
        let rejected = json!({ "rejected": [{ "corners": [] }, { "corners": [] }] });
        let p = PipelineFacts {
            name: "default",
            enabled: true,
            state: "running",
            telemetry: Some(&telemetry),
            dictionary: Some("apriltag_36h11"),
            pyramid: Some(3),
            outputs: BTreeMap::from([("rejected", &rejected)]),
            ..PipelineFacts::default()
        };
        let tags = stage(&derive(&served(Some(capture(Some(24.0), 1, None))), std::slice::from_ref(&p)), "detections").clone();
        assert_eq!(tags.state, StageState::Waiting);
        assert!(tags.hint.unwrap_or_default().starts_with("2 outlines were found and rejected"));
    }

    #[test]
    fn a_failed_pipeline_says_why_not_just_quarantined() {
        let p = PipelineFacts { name: "tags", enabled: true, state: "failed", session: Some(("failed", Some("nodes: detector panicked"))), ..PipelineFacts::default() };
        let stage = pipeline_stage(&p);
        assert_eq!((stage.state, stage.reason.as_str()), (StageState::Blocked, "Failed: nodes: detector panicked"));
    }

    #[test]
    fn a_graph_that_does_not_load_is_the_reason() {
        let p = PipelineFacts { name: "tags", enabled: true, state: "failed", graph_error: Some("graph.inline is not JSON"), ..PipelineFacts::default() };
        assert_eq!(pipeline_stage(&p).reason, "The graph does not load: graph.inline is not JSON");
    }

    #[test]
    fn a_running_pipeline_that_fails_its_ticks_says_the_error() {
        let telemetry = json!({ "fps": 0.0, "ticks_failed": 4, "last_error": "frame size changed" });
        let p = PipelineFacts { name: "tags", enabled: true, state: "running", telemetry: Some(&telemetry), ..PipelineFacts::default() };
        assert_eq!(pipeline_stage(&p).reason, "Frames fail: frame size changed");
    }

    #[test]
    fn self_calibration_shows_its_progress_toward_the_window() {
        let pose = json!({ "state": "bootstrapping", "reason": "", "valid": false, "calibration": { "estimated": false, "rms_px": 0.0, "views": 4, "frames": 40 } });
        let telemetry = running();
        let p = PipelineFacts {
            name: "live-map",
            enabled: true,
            state: "running",
            telemetry: Some(&telemetry),
            dictionary: Some("apriltag_36h11"),
            live_map: true,
            calibration_frames: Some(150),
            outputs: BTreeMap::from([("map_pose", &pose)]),
            ..PipelineFacts::default()
        };
        let calibration = stage(&derive(&served(Some(capture(Some(24.0), 1, None))), std::slice::from_ref(&p)), "calibration").clone();
        assert_eq!(calibration.state, StageState::Waiting);
        assert_eq!(calibration.progress, Some(Progress { value: 40, target: 150, unit: "frames" }));
        assert!(calibration.reason.contains("40 of 150 frames, 4 views"));
    }

    #[test]
    fn a_refused_self_calibration_fit_is_in_the_hint() {
        let p = PipelineFacts { name: "live-map", enabled: true, state: "running", live_map: true, ..PipelineFacts::default() };
        let camera = CameraFacts { calibration_note: Some("the camera has a calibration of its own".into()), ..served(Some(capture(Some(24.0), 1, None))) };
        let calibration = stage(&derive(&camera, std::slice::from_ref(&p)), "calibration").clone();
        assert!(calibration.hint.unwrap_or_default().contains("Last fit not stored: the camera has a calibration of its own."));
    }

    #[test]
    fn a_user_calibration_is_ok() {
        let camera = CameraFacts { calibration: Some(CalibrationFact { self_calibrated: false, rms_px: Some(0.31), views: None, width: 1280, height: 960 }), ..served(None) };
        let calibration = stage(&derive(&camera, &[]), "calibration").clone();
        assert_eq!((calibration.state, calibration.reason.as_str()), (StageState::Ok, "Calibrated by you, 0.31 px rms (at 1280x960)"), "a stored calibration is ok with no pipeline");
        let p = PipelineFacts { name: "tags", enabled: true, state: "running", dictionary: Some("apriltag_36h11"), ..PipelineFacts::default() };
        let calibration = stage(&derive(&camera, std::slice::from_ref(&p)), "calibration").clone();
        assert_eq!((calibration.state, calibration.reason.as_str()), (StageState::Ok, "Calibrated by you, 0.31 px rms (at 1280x960)"));
    }

    #[test]
    fn a_lost_mapper_is_blocked_with_its_reason() {
        let pose = json!({ "state": "lost", "reason": "no tag for 60 frames", "valid": false });
        let p = PipelineFacts { name: "live-map", enabled: true, state: "running", live_map: true, outputs: BTreeMap::from([("map_pose", &pose)]), ..PipelineFacts::default() };
        let map = stage(&derive(&served(None), std::slice::from_ref(&p)), "map").clone();
        assert_eq!((map.state, map.reason.as_str()), (StageState::Blocked, "Lost: no tag for 60 frames"));
    }

    #[test]
    fn depth_says_it_needs_motion_when_the_map_tracks_but_no_depth_came() {
        let pose = json!({ "state": "tracking", "reason": "", "valid": true });
        let mesh = json!({ "keyframes": 2 });
        let p = PipelineFacts { name: "dense", enabled: true, state: "running", dense: true, outputs: BTreeMap::from([("map_pose", &pose), ("dense_mesh", &mesh)]), ..PipelineFacts::default() };
        let depth = stage(&derive(&served(None), std::slice::from_ref(&p)), "depth").clone();
        assert_eq!(depth.state, StageState::Waiting);
        assert_eq!(depth.reason, "No depth yet: 2 keyframes");
        assert!(depth.hint.unwrap_or_default().contains("Depth needs motion"));
    }

    #[test]
    fn the_first_stage_that_is_not_ok_is_the_blocking_one() {
        let pose = json!({ "state": "bootstrapping", "reason": "waiting for a calibrated tag", "valid": false });
        let p = PipelineFacts { name: "live-map", enabled: true, state: "running", live_map: true, outputs: BTreeMap::from([("map_pose", &pose)]), ..PipelineFacts::default() };
        let status = derive(&served(Some(capture(Some(24.0), 1, None))), std::slice::from_ref(&p));
        assert_eq!(status.blocking, Some("calibration"), "calibration bootstraps before the map does");
    }

    #[test]
    fn holder_labels_name_the_known_programs_and_keep_the_rest() {
        assert_eq!(holder_label("visionserver"), "visionserver (Limelight vision)");
        assert_eq!(holder_label("gst-launch"), "gst-launch");
    }

    #[test]
    fn the_device_scan_finds_a_process_holding_a_file() {
        let dir = std::env::temp_dir().join(format!("helios-holders-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let node = dir.join("node");
        fs::write(&node, b"x").expect("write");
        let held = fs::File::open(&node).expect("open");
        let holders = device_holders(&node);
        drop(held);
        fs::remove_dir_all(&dir).ok();
        assert_eq!(holders.len(), 1, "this test process holds it: {holders:?}");
    }
}
