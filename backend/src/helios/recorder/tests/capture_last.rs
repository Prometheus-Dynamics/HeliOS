//! Capture-last rings end to end: a virtual raw camera (BYR2 at 60 fps, the recorder keeps 30), a
//! ring on it, a clip written while a recording keeps running, and the clip read back. The camera
//! is virtual (Styx's `make_virtual_device`); on a device the ring gets the same raw companion
//! frames the camera service serves for recordings.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use helios_recorder::{ClipRequest, RecordFrames, Recorder, RecorderSettings, RecordingInfo, RecordingRequest, RecordingState, RingRequest, RingStatus};
use mcap::{read::ChunkFlattener, records::Record};
use styx::{
    ipc::{CameraService, CameraServiceHandle},
    prelude::{ColorSpace, FourCc, Interval, MediaFormat, Mode, Resolution},
};

const MIB: u64 = 1 << 20;

fn raw_camera(socket: &Path) -> CameraServiceHandle {
    let mode = Mode::with_interval(MediaFormat::new(FourCc::new(*b"BYR2"), Resolution::new(320, 200).expect("resolution"), ColorSpace::Srgb), Interval::from_fps(60).expect("interval"));
    let device = styx::capture_api::make_virtual_device("raw", [mode]);
    CameraService::new(device).keep_streaming().spare_clients(4).serve(socket).expect("serve raw camera")
}

fn info() -> RecordingInfo {
    RecordingInfo {
        session: BTreeMap::from([("board".into(), "test-board".into())]),
        camera: BTreeMap::from([("calibration".into(), "null".into())]),
        versions: BTreeMap::from([("helios_api".into(), "test".into())]),
        field: None,
        pipelines: Vec::new(),
    }
}

fn settings(dir: &Path) -> RecorderSettings {
    RecorderSettings {
        storage_dir: Some(dir.to_path_buf()),
        reserve_bytes: 16 * MIB,
        capture_last_seconds: 3,
        capture_last_cameras: vec!["cam".into()],
        frames: RecordFrames::Raw,
        ..RecorderSettings::default()
    }
}

fn clip(info: &RecordingInfo) -> ClipRequest<'_> {
    ClipRequest { camera_id: "cam", board: "test-board", camera_label: "cam", commit: "abc1234", info, compression: true, queue_bytes: 64 * MIB as usize, max_fps: Some(30.0) }
}

/// Frames and metadata names of an MCAP part.
fn frames_and_metadata(path: &Path) -> (u64, Vec<String>) {
    let bytes = std::fs::read(path).expect("read mcap");
    let mut frames = 0;
    let mut metadata = Vec::new();
    let mut image_channels: HashMap<u16, bool> = HashMap::new();
    for record in ChunkFlattener::new(&bytes).expect("reader") {
        match record.expect("record") {
            Record::Channel(channel) => {
                image_channels.insert(channel.id, channel.topic == "/styx/image");
            }
            Record::Message { header, .. } => {
                if image_channels.get(&header.channel_id) == Some(&true) {
                    frames += 1;
                }
            }
            Record::Metadata(entry) => metadata.push(entry.name),
            _ => {}
        }
    }
    (frames, metadata)
}

fn ring_of(rings: &[RingStatus]) -> &RingStatus {
    rings.iter().find(|ring| ring.camera == "cam").expect("the ring")
}

#[test]
fn a_clip_is_written_from_the_ring_while_a_recording_keeps_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("raw.styx.sock");
    let _camera = raw_camera(&socket);
    let recordings = dir.path().join("recordings");
    let recorder = Recorder::default();
    let stored = settings(&recordings);

    let rings = recorder.sync_rings(&[RingRequest { camera_id: "cam".into(), frames_socket: socket.clone(), settings: stored.clone() }]);
    assert_eq!(ring_of(&rings).state, "running", "{rings:?}");
    // The first frames come a moment later; the format is known once one arrives.
    std::thread::sleep(Duration::from_millis(300));
    let rings = recorder.rings();
    assert!(ring_of(&rings).format.as_deref().is_some_and(|f| f.starts_with("BYR2 ")), "{rings:?}");
    assert!(ring_of(&rings).frames > 0, "{rings:?}");

    let request = RecordingRequest {
        camera_id: "cam".into(),
        frames_socket: socket.clone(),
        board: "test-board".into(),
        camera_label: "cam".into(),
        commit: "abc1234".into(),
        label: None,
        settings: stored.clone(),
        info: info(),
        lemnos: None,
    };
    let started = recorder.start(request).expect("start");
    std::thread::sleep(Duration::from_millis(1500));

    let info = info();
    let summary = recorder.capture_last(None, &clip(&info), &recordings).expect("clip");
    assert!(summary.file.ends_with("-last.mcap"), "{summary:?}");
    assert_eq!(summary.stop_reason.as_deref(), Some("capture_last"));
    // About 1.5 s at 30 fps (the ring started a moment before the recording).
    assert!((30..=60).contains(&summary.frames_written), "{summary:?}");
    assert_eq!(summary.frames_dropped, 0, "{summary:?}");

    // The recording was not stopped by the clip.
    assert!(recorder.status().is_some_and(|status| status.state.is_active()));
    recorder.stop(&started.id).expect("stop");
    let mut done = recorder.status().expect("status");
    let end = Instant::now() + Duration::from_secs(30);
    while done.state.is_active() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
        done = recorder.status().expect("status");
    }
    assert_eq!(done.state, RecordingState::Stopped, "{done:?}");

    let clip_path = recordings.join(format!("{}.mcap", summary.session));
    let (frames, metadata) = frames_and_metadata(&clip_path);
    assert_eq!(frames, summary.frames_written);
    for name in ["helios.session", "helios.camera", "helios.versions", "helios.part"] {
        assert!(metadata.iter().any(|m| m == name), "{name} in {metadata:?}");
    }

    // The last half second is a shorter clip, and the ring's window is the configured seconds.
    let short = recorder.capture_last(Some(Duration::from_millis(500)), &clip(&info), &recordings).expect("short clip");
    assert!(short.frames_written < summary.frames_written, "{short:?} vs {summary:?}");
    let window = ring_of(&recorder.rings()).window_seconds;
    assert!(window <= 3.0 + 1e-6, "window {window}");
}

#[test]
fn a_clip_needs_a_running_ring_for_its_camera() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recordings = dir.path().join("recordings");
    let recorder = Recorder::default();
    let info = info();
    let refused = recorder.capture_last(None, &clip(&info), &recordings);
    assert!(matches!(refused, Err(helios_recorder::RecorderError::NotFound(_))), "{refused:?}");

    // A camera that is not there: its ring fails, with the reason, and the clip says so.
    let missing = PathBuf::from(dir.path()).join("missing.sock");
    let rings = recorder.sync_rings(&[RingRequest { camera_id: "cam".into(), frames_socket: missing, settings: settings(&recordings) }]);
    assert_eq!(ring_of(&rings).state, "failed", "{rings:?}");
    assert!(ring_of(&rings).error.is_some());
    let failed = recorder.capture_last(None, &clip(&info), &recordings);
    assert!(matches!(failed, Err(helios_recorder::RecorderError::Camera(_))), "{failed:?}");

    // Asked to stop: the ring goes.
    let none = recorder.sync_rings(&[]);
    assert!(none.is_empty());
    assert!(recorder.rings().is_empty());
}
