//! The Eidos export of a small synthetic recording: 16x8 raw (BYR2) frames at 30 fps with their
//! native metadata, a pipeline output per frame, the camera's calibration and mount, and a field
//! layout. The recording is written by helios-recorder's own part writer (the same MCAP, records
//! and attachments as a recording), so the export reads what a recording holds.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use helios_recorder::{
    RecordingInfo,
    host::{self, TOPIC_OUTPUT_PREFIX},
    info::{ATTACHMENT_FIELD, Attachment},
    writer::{self, PartWriter},
};
use styx::{
    core::prelude::{BackendFrameMeta, FrameLease, NativeFrameMeta},
    prelude::{ColorSpace, FourCc, MediaFormat, Resolution},
};

const WIDTH: u32 = 16;
const HEIGHT: u32 = 8;
const FRAMES: u64 = 4;
const FRAME_NS: u64 = 33_333_333;

fn monotonic_ns() -> u64 {
    let now = nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC).expect("clock");
    now.tv_sec() as u64 * 1_000_000_000 + now.tv_nsec() as u64
}

/// A raw frame of `WIDTH` x `HEIGHT` 16-bit samples, a mid-grey Bayer mosaic.
fn raw_frame(sof: u64, sequence: u32) -> FrameLease {
    let format = MediaFormat::new(FourCc::new(*b"BYR2"), Resolution::new(WIDTH, HEIGHT).expect("resolution"), ColorSpace::Srgb);
    let bytes: Vec<u8> = (0..WIDTH * HEIGHT).flat_map(|_| 0xC000u16.to_le_bytes()).collect();
    let mut lease = FrameLease::from_visible_bytes(format, sof, &bytes).expect("lease");
    // Exposure 10 ms, readout 5 ms: the frame's mid-exposure is 2.5 ms before the readout's middle.
    lease.meta_mut().backend =
        Some(BackendFrameMeta::Native(NativeFrameMeta { sequence, exposure_ns: 10_000_000, analog_gain: 2.0, digital_gain: 1.0, frame_end_ns: sof + 5_000_000, ..NativeFrameMeta::default() }));
    lease
}

fn info(calibrated: bool) -> RecordingInfo {
    let camera = if calibrated { r#"[{"width":16,"height":8,"model":"fisheye","fx":8.0,"fy":8.0,"cx":8.0,"cy":4.0,"distortion":{"k1":0.1}}]"# } else { "null" };
    let mount = if calibrated { r#"{"x":0.3,"y":0.0,"z":0.2,"roll":0.0,"pitch":-15.0,"yaw":0.0}"# } else { "null" };
    let field = br#"{"id":"test","name":"Test","format":"json","builtin":false,"saved_at_ms":0,"layout":{"length_m":16.5,"width_m":8.0,"tags":[{"id":7,"side_m":0.165,"field_from_tag":{"translation":{"x":1.0,"y":2.0,"z":0.5},"rotation":{"w":1.0,"x":0.0,"y":0.0,"z":0.0}}}]}}"#;
    RecordingInfo {
        session: BTreeMap::from([("board".into(), "\"test-board\"".into()), ("camera".into(), "\"cam\"".into())]),
        camera: BTreeMap::from([("calibration".into(), camera.into()), ("mount".into(), mount.into())]),
        versions: BTreeMap::from([("helios_api".into(), "\"test\"".into())]),
        field: Some(Attachment { name: ATTACHMENT_FIELD.into(), media_type: "application/json".into(), data: field.to_vec() }),
        pipelines: Vec::new(),
    }
}

/// Writes a recording of `FRAMES` frames into `dir`; returns its path.
fn write_recording(dir: &Path, calibrated: bool) -> PathBuf {
    let header = writer::header_for("cam", FourCc::new(*b"BYR2"), (WIDTH, HEIGHT), Some(30.0)).expect("header");
    let options = writer::options_for(false, 8 << 20);
    let mut part = PartWriter::open(dir, "session", "cam", 1, &header, options, &info(calibrated)).expect("open");
    let start = monotonic_ns();
    let boot = host::boottime_ns();
    for i in 0..FRAMES {
        let sof = start + i * FRAME_NS;
        part.record(&raw_frame(sof, i as u32)).expect("frame");
        // The output of this frame, recorded a few milliseconds after its readout.
        let log_time = boot + i * FRAME_NS + 8_000_000;
        let output = host::output_json("p1", "tags", &serde_json::json!([{ "id": 7, "corners": [] }]), 0);
        part.write(&helios_recorder::host::HostMessage { topic: format!("{TOPIC_OUTPUT_PREFIX}tags"), log_time_ns: log_time, data: output }).expect("output");
    }
    part.finish(None, 0).expect("finish");
    dir.join("session.mcap")
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).expect("read")).expect("json")
}

#[test]
fn a_calibrated_recording_exports_grey_frames_the_replay_and_the_detections() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recording = write_recording(dir.path(), true);
    let out = dir.path().join("eidos");
    let summary = helios_recorder::export::export_eidos(&recording, &out).expect("export");
    assert_eq!((summary.frames, summary.width, summary.height, summary.camera_status), (FRAMES, WIDTH, HEIGHT, "calibrated"));

    // Grey8, one frame after another.
    let raw = std::fs::read(out.join("session.raw")).expect("raw");
    assert_eq!(raw.len() as u64, FRAMES * u64::from(WIDTH) * u64::from(HEIGHT));
    // The mosaic is mid-high grey (0xC000 per sample), so its luma is not zero.
    assert!(raw.iter().any(|&grey| grey > 0), "the grey frames are not zero");

    // One line per frame: monotonic mid-exposure times, the exposure, the gain, the detections.
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(out.join("session.frames.jsonl")).expect("lines").lines().map(|line| serde_json::from_str(line).expect("line")).collect();
    assert_eq!(lines.len() as u64, FRAMES);
    let times: Vec<u64> = lines.iter().map(|line| line["t_ns"].as_u64().expect("t_ns")).collect();
    assert!(times.windows(2).all(|pair| pair[1] > pair[0]), "{times:?}");
    assert_eq!(lines[0]["exposure_ns"], 10_000_000);
    assert_eq!(lines[0]["gain"]["analog"], 2.0);
    assert_eq!(lines[0]["eof_ns"].as_u64().map(|eof| eof - lines[0]["sof_ns"].as_u64().unwrap_or(0)), Some(5_000_000));
    // Mid-exposure: readout centre (SOF + 2.5 ms) less half the exposure (5 ms): SOF - 2.5 ms.
    assert_eq!(times[0] + 2_500_000, lines[0]["sof_ns"].as_u64().expect("sof"));
    let tags = &lines[0]["detections"]["tags"];
    assert_eq!(tags["value"][0]["id"], 7, "{}", lines[0]);
    assert!(tags["dt_ms"].as_f64().is_some_and(|dt| dt.abs() < 100.0), "{}", lines[0]);

    // The replay document: the camera and its mount, the field's tags, and the time base.
    let text = std::fs::read_to_string(out.join("session.replay.json")).expect("replay");
    assert!(!text.to_lowercase().contains("field"), "the word field must not appear in the replay document");
    let replay: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(replay["format"], "eidos-replay");
    assert_eq!(replay["image"]["file"], "session.raw");
    assert_eq!((replay["camera"]["model"].as_str(), replay["camera"]["status"].as_str()), (Some("fisheye"), Some("calibrated")));
    assert_eq!(replay["camera"]["fx"], 8.0);
    assert_eq!(replay["camera"]["distortion"]["k1"], 0.1);
    assert_eq!(replay["time"]["stamp"], "exposure_mid");
    assert_eq!(replay["source"]["camera"], "cam");
    assert_eq!(replay["mount"], "calibrated");
    assert!((replay["rig_from_camera"]["translation"]["x"].as_f64().unwrap_or_default() - 0.3).abs() < 1e-9);
    assert_eq!(replay["rig_from_camera"]["rotation_wxyz"].as_array().map(Vec::len), Some(4));
    let tags = replay["reference"]["tags"].as_array().expect("tags");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0]["id"], 7);
    assert_eq!(replay["relative_motion"]["status"], "todo", "IMU constraints are a stated TODO for v1");
}

#[test]
fn an_uncalibrated_recording_is_nominal_with_an_unknown_mount() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recording = write_recording(dir.path(), false);
    let out = dir.path().join("nominal");
    let summary = helios_recorder::export::export_eidos(&recording, &out).expect("export");
    assert_eq!(summary.camera_status, "nominal");
    let replay = read_json(&out.join("session.replay.json"));
    assert_eq!((replay["camera"]["status"].as_str(), replay["camera"]["model"].as_str()), (Some("nominal"), Some("pinhole")));
    assert_eq!(replay["camera"]["fx"].as_f64(), Some(f64::from(WIDTH) / 2.0), "a 90 degree field, until calibrated");
    assert_eq!(replay["mount"], "unknown");
    assert!((replay["rig_from_camera"]["translation"]["x"].as_f64().unwrap_or(1.0)).abs() < 1e-12, "the origin without a mount");
}

#[test]
fn a_file_that_is_not_a_recording_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bogus = dir.path().join("bogus.mcap");
    std::fs::write(&bogus, b"not an mcap file at all").expect("write");
    let refused = helios_recorder::export::export_eidos(&bogus, &dir.path().join("out"));
    assert!(refused.is_err(), "{refused:?}");
}
