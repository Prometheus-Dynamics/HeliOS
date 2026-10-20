//! Eidos's replay input v1 (`docs/docs/recording-design.md`, "Offline use"): the per-frame lines of
//! `NAME.frames.jsonl` and the document of `NAME.replay.json`. Times are monotonic nanoseconds at
//! the middle of each frame's exposure; the camera is its stored calibration for the frame size
//! (`calibrated`) or a nominal pinhole (`nominal`); the mount is `rig_from_camera`; the field's tags
//! are `reference.tags`. The word the design keeps out of the output (the field) never appears in
//! it; the field layout enters as its tags' poses only.

use std::collections::BTreeMap;

use helios_field::{CameraMount, FieldLayout, Pose};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use styx::core::prelude::{BackendFrameMeta, FrameLease, FrameMeta};

use super::facts::{ClockSample, OutputSample};

/// Detections are matched to a frame within this window: outputs are stamped when they are
/// received, not with the frame that made them (a known limit of the recording).
pub const DETECTION_WINDOW_NS: i64 = 100_000_000;

/// A frame's timing, from its metadata: the start of its readout (SOF, the frame timestamp), the
/// end of its readout (EOF, the native frame's `frame_end_ns`), its exposure and its gains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTiming {
    pub sof_ns: u64,
    pub eof_ns: u64,
    pub exposure_ns: u64,
    pub analog_gain: f32,
    pub digital_gain: f32,
    pub sequence: u32,
}

impl FrameTiming {
    pub fn from_frame(frame: &FrameLease) -> Self {
        Self::from_meta(frame.meta())
    }

    pub fn from_meta(meta: &FrameMeta) -> Self {
        let sof_ns = meta.timestamp;
        match meta.backend.as_ref().and_then(BackendFrameMeta::as_native) {
            Some(native) => {
                Self { sof_ns, eof_ns: native.frame_end_ns, exposure_ns: native.exposure_ns, analog_gain: native.analog_gain, digital_gain: native.digital_gain, sequence: native.sequence }
            }
            None => Self { sof_ns, eof_ns: 0, exposure_ns: 0, analog_gain: 1.0, digital_gain: 1.0, sequence: 0 },
        }
    }

    /// The middle of the exposure. A rolling shutter exposes each row until it is read, so the
    /// frame's middle row is exposed up to the middle of its readout, `exposure / 2` before it.
    pub fn mid_exposure_ns(&self) -> u64 {
        let centre = if self.eof_ns > self.sof_ns { (self.sof_ns + self.eof_ns) / 2 } else { self.sof_ns };
        centre.saturating_sub(self.exposure_ns / 2)
    }
}

/// Relates the frame clock (monotonic) to the host's (boot time), from Styx's clock samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    offset_ns: i64,
    source: &'static str,
}

impl Clock {
    pub fn from_samples(samples: &[ClockSample]) -> Self {
        match samples.first() {
            Some(sample) => Self { offset_ns: sample.boottime_ns as i64 - sample.monotonic_ns as i64, source: "styx_clock_samples" },
            None => Self { offset_ns: 0, source: "assumed_equal" },
        }
    }

    pub fn boottime_ns(&self, monotonic_ns: u64) -> i64 {
        monotonic_ns as i64 + self.offset_ns
    }

    pub fn source(&self) -> &'static str {
        self.source
    }
}

/// The pipeline outputs closest in time to `boottime_ns` (within `window_ns`), one per port, each
/// with its value and how far after the frame it was recorded (`dt_ms`).
pub fn detections_at(outputs: &[OutputSample], boottime_ns: i64, window_ns: i64) -> Map<String, Value> {
    let lo = outputs.partition_point(|output| (output.log_time_ns as i64) < boottime_ns - window_ns);
    let hi = outputs.partition_point(|output| (output.log_time_ns as i64) <= boottime_ns + window_ns);
    let mut nearest: BTreeMap<&str, (i64, &OutputSample)> = BTreeMap::new();
    for output in &outputs[lo..hi] {
        let distance = (output.log_time_ns as i64 - boottime_ns).abs();
        let closer = nearest.get(output.port.as_str()).is_none_or(|(best, _)| distance < *best);
        if closer {
            nearest.insert(output.port.as_str(), (distance, output));
        }
    }
    nearest.into_iter().map(|(port, (_, output))| (port.to_string(), json!({ "value": output.value, "dt_ms": (output.log_time_ns as f64 - boottime_ns as f64) / 1e6 }))).collect()
}

/// One line of `NAME.frames.jsonl`: frame `index`, its mid-exposure time and timing, and the
/// detections for comparison.
pub fn frame_line(index: u64, timing: &FrameTiming, t_ns: u64, detections: Map<String, Value>) -> Value {
    json!({
        "i": index,
        "t_ns": t_ns,
        "sof_ns": timing.sof_ns,
        "eof_ns": timing.eof_ns,
        "exposure_ns": timing.exposure_ns,
        "gain": { "analog": timing.analog_gain, "digital": timing.digital_gain },
        "sequence": timing.sequence,
        "detections": detections,
    })
}

/// A stored calibration (the API's `CameraCalibration`, as the recording's metadata holds it).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CalibrationJson {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub model: String,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    #[serde(default)]
    pub distortion: Distortion,
}

/// OpenCV's distortion coefficients.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Distortion {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub k4: f64,
    pub k5: f64,
    pub k6: f64,
    pub p1: f64,
    pub p2: f64,
}

/// The calibration of the frame size among the camera's stored ones (`metadata` JSON, a list).
pub fn calibration_for(json: &str, width: u32, height: u32) -> Option<CalibrationJson> {
    let all: Vec<CalibrationJson> = serde_json::from_str(json).ok()?;
    all.into_iter().find(|calibration| calibration.width == width && calibration.height == height)
}

/// The camera's mount as the API stores it: metres, and degrees (x forward, y left, z up).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct MountJson {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
}

/// `rig_from_camera` for a mount (the optical frame on the robot). Without a mount the camera is
/// taken at the robot's origin, facing forward, and the mount reads `unknown`.
pub fn rig_from_camera(mount: Option<&MountJson>) -> (Pose, &'static str) {
    let flu = mount.copied().unwrap_or(MountJson { x: 0.0, y: 0.0, z: 0.0, roll: 0.0, pitch: 0.0, yaw: 0.0 });
    let camera = CameraMount { x_m: flu.x, y_m: flu.y, z_m: flu.z, roll_rad: flu.roll.to_radians(), pitch_rad: flu.pitch.to_radians(), yaw_rad: flu.yaw.to_radians() };
    let pose = Pose::from_se3(&camera.extrinsics().rig_from_camera);
    (pose, if mount.is_some() { "calibrated" } else { "unknown" })
}

/// The field layout of a `helios.field` attachment (a stored layout; its `layout` is the field).
pub fn field_layout(bytes: &[u8]) -> Option<FieldLayout> {
    #[derive(Deserialize)]
    struct Stored {
        layout: FieldLayout,
    }
    serde_json::from_slice::<Stored>(bytes).ok().map(|stored| stored.layout)
}

/// What the replay document is made from.
pub struct DocumentInput<'a> {
    /// The base name of the export's files.
    pub name: &'a str,
    /// The recording's file name.
    pub recording: &'a str,
    pub camera: &'a str,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub calibration: Option<&'a CalibrationJson>,
    pub mount: Option<&'a MountJson>,
    pub field: Option<&'a FieldLayout>,
    /// How the frame clock relates to the host's (see [`Clock::source`]).
    pub clock: &'a str,
}

/// `NAME.replay.json`.
pub fn document(input: &DocumentInput<'_>) -> Value {
    let (model, status, fx, fy, cx, cy, distortion) = match input.calibration {
        Some(c) => {
            let model = if c.model.is_empty() { "pinhole" } else { c.model.as_str() };
            (model.to_string(), "calibrated", c.fx, c.fy, c.cx, c.cy, c.distortion)
        }
        // A pinhole with a 90 degree horizontal field and no distortion, until the camera is calibrated.
        None => {
            let f = f64::from(input.width) / 2.0;
            ("pinhole".to_string(), "nominal", f, f, f64::from(input.width) / 2.0, f64::from(input.height) / 2.0, Distortion::default())
        }
    };
    let (pose, mount) = rig_from_camera(input.mount);
    let tags = input.field.map(FieldLayout::known_tags_value).unwrap_or_else(|| json!([]));
    let rotation = pose.rotation;
    json!({
        "format": "eidos-replay",
        "version": 1,
        "name": input.name,
        "source": { "recording": input.recording, "camera": input.camera },
        "image": {
            "width": input.width,
            "height": input.height,
            "format": "grey8",
            "file": format!("{}.raw", input.name),
            "frame_bytes": u64::from(input.width) * u64::from(input.height),
            "frames": input.frames,
            "frames_file": format!("{}.frames.jsonl", input.name),
        },
        "time": { "clock": "monotonic", "stamp": "exposure_mid", "clock_relation": input.clock },
        "camera": {
            "model": model,
            "status": status,
            "width": input.width,
            "height": input.height,
            "fx": fx,
            "fy": fy,
            "cx": cx,
            "cy": cy,
            "distortion": { "k1": distortion.k1, "k2": distortion.k2, "k3": distortion.k3, "k4": distortion.k4, "k5": distortion.k5, "k6": distortion.k6, "p1": distortion.p1, "p2": distortion.p2 },
        },
        "rig_from_camera": {
            "translation": { "x": pose.translation.x, "y": pose.translation.y, "z": pose.translation.z },
            "rotation_wxyz": [rotation.w, rotation.x, rotation.y, rotation.z],
        },
        "mount": mount,
        "reference": { "tags": tags },
        "relative_motion": { "status": "todo", "note": "IMU relative-motion constraints are not exported in v1" },
    })
}
