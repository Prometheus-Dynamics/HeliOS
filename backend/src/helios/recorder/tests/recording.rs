//! Recordings end to end: a Styx camera service (virtual camera, luma, 320x200 at 30 fps) on a
//! socket, a `Recorder` making recordings from it, and the MCAP files read back.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use helios_recorder::{
    Attachment, Recorder, RecorderError, RecorderSettings, RecordingInfo, RecordingRequest, RecordingState, RecordingStatus, catalog, frames,
    host::{self, TOPIC_EVENTS, TOPIC_OUTPUT_PREFIX},
    info::{ATTACHMENT_FIELD, ATTACHMENT_PIPELINE_PREFIX},
    storage::StorageError,
};
use mcap::{read::ChunkFlattener, records::Record};
use styx::{
    ipc::{CameraService, CameraServiceHandle},
    prelude::{ColorSpace, FourCc, Interval, MediaFormat, Mode, Resolution},
};

const MIB: u64 = 1 << 20;

/// A virtual camera on `socket`, luma 320x200 at 30 fps, served until the handle drops.
fn luma_camera(socket: &Path) -> CameraServiceHandle {
    let mode = Mode::with_interval(MediaFormat::new(FourCc::GREY, Resolution::new(320, 200).expect("resolution"), ColorSpace::Srgb), Interval::from_fps(30).expect("interval"));
    let device = styx::capture_api::make_virtual_device("cam", [mode]);
    CameraService::new(device).keep_streaming().serve(socket).expect("serve camera")
}

fn info() -> RecordingInfo {
    RecordingInfo {
        session: BTreeMap::from([("board".into(), "test-board".into())]),
        camera: BTreeMap::from([("calibration".into(), "null".into()), ("mount".into(), "null".into())]),
        versions: BTreeMap::from([("helios_api".into(), "test".into())]),
        field: Some(Attachment { name: ATTACHMENT_FIELD.into(), media_type: "application/json".into(), data: b"{\"tags\":[]}".to_vec() }),
        pipelines: vec![Attachment { name: format!("{ATTACHMENT_PIPELINE_PREFIX}p1"), media_type: "application/json".into(), data: b"{}".to_vec() }],
    }
}

fn request(socket: &Path, settings: RecorderSettings) -> RecordingRequest {
    RecordingRequest {
        camera_id: "cam".into(),
        frames_socket: socket.to_path_buf(),
        frames: frames::luma_request(),
        board: "test-board".into(),
        camera_label: "cam".into(),
        commit: "abc1234".into(),
        settings,
        info: info(),
        lemnos: None,
    }
}

fn settings(dir: &Path) -> RecorderSettings {
    RecorderSettings { storage_dir: Some(dir.to_path_buf()), ..RecorderSettings::default() }
}

/// Polls until the recording is no longer active (or `limit` passes).
fn wait_idle(recorder: &Recorder, limit: Duration) -> RecordingStatus {
    let end = Instant::now() + limit;
    loop {
        let status = recorder.status().expect("a recording");
        if !status.state.is_active() {
            return status;
        }
        assert!(Instant::now() < end, "recording did not end: {status:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What an MCAP part holds, by topic.
#[derive(Debug, Default)]
struct Contents {
    messages: BTreeMap<String, u64>,
    channel_metadata: BTreeMap<String, BTreeMap<String, String>>,
    metadata: BTreeMap<String, BTreeMap<String, String>>,
    attachments: Vec<String>,
}

impl Contents {
    /// Frames: one image message each (`/styx/image`); the frame's metadata is on
    /// `/styx/frame_meta`, one message per frame too.
    fn frames(&self) -> u64 {
        let image = self.messages.get("/styx/image").copied().unwrap_or_default();
        let meta = self.messages.get("/styx/frame_meta").copied().unwrap_or_default();
        assert_eq!(image, meta, "one metadata message per image message");
        image
    }
}

fn read(path: &Path) -> Contents {
    let bytes = std::fs::read(path).expect("read mcap");
    let mut contents = Contents::default();
    let mut topics: HashMap<u16, String> = HashMap::new();
    for record in ChunkFlattener::new(&bytes).expect("reader") {
        match record.expect("record") {
            Record::Channel(channel) => {
                topics.insert(channel.id, channel.topic.clone());
                contents.channel_metadata.insert(channel.topic, channel.metadata);
            }
            Record::Message { header, .. } => {
                let topic = topics.get(&header.channel_id).cloned().unwrap_or_default();
                *contents.messages.entry(topic).or_default() += 1;
            }
            Record::Metadata(metadata) => {
                contents.metadata.insert(metadata.name, metadata.metadata);
            }
            Record::Attachment { header, .. } => contents.attachments.push(header.name),
            _ => {}
        }
    }
    contents
}

fn mcaps(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir).expect("dir").filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "mcap")).collect();
    found.sort();
    found
}

#[test]
fn records_frames_host_channels_and_start_records_compressed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.styx.sock");
    let _camera = luma_camera(&socket);
    let recordings = dir.path().join("recordings");
    let recorder = Recorder::default();

    let started = recorder.start(request(&socket, settings(&recordings))).expect("start");
    assert!(matches!(started.state, RecordingState::Starting | RecordingState::Recording), "{started:?}");
    assert!(started.file.as_deref().is_some_and(|f| f.ends_with(".mcap")));
    assert!(started.id.ends_with("-abc1234"), "{}", started.id);
    let host = recorder.host().expect("host sender while recording");
    for port in 0..5u32 {
        host.send_at(&format!("{TOPIC_OUTPUT_PREFIX}tags"), u64::from(port) + 1, host::output_json("p1", "tags", &serde_json::json!([port]), 9));
    }
    for _ in 0..3 {
        host.send(TOPIC_EVENTS, host::event_json("pipeline", 1, &serde_json::json!({})));
    }
    std::thread::sleep(Duration::from_millis(1500));

    let asked = Instant::now();
    let stopping = recorder.stop(&started.id).expect("stop");
    assert!(asked.elapsed() < Duration::from_secs(1), "stop returns at once");
    assert!(!stopping.state.is_active() || stopping.state == RecordingState::Finishing, "{stopping:?}");
    let done = wait_idle(&recorder, Duration::from_secs(30));
    assert_eq!(done.state, RecordingState::Stopped, "{done:?}");
    assert_eq!(done.stop_reason.as_deref(), Some("requested"));
    assert_eq!(done.parts.len(), 1);
    assert!(done.frames_written > 10, "{done:?}");
    assert_eq!(done.frames_dropped, 0, "{done:?}");
    assert!(recorder.host().is_none(), "no host sender once stopped");

    let files = mcaps(&recordings);
    assert_eq!(files.len(), 1, "{files:?}");
    let contents = read(&files[0]);
    assert_eq!(contents.frames(), done.frames_written, "frame messages match the count");
    assert_eq!(contents.messages.get(&format!("{TOPIC_OUTPUT_PREFIX}tags")), Some(&5));
    assert_eq!(contents.messages.get(TOPIC_EVENTS), Some(&3));
    assert_eq!(contents.channel_metadata[&format!("{TOPIC_OUTPUT_PREFIX}tags")]["source"], "helios-api");
    for name in ["helios.session", "helios.camera", "helios.versions", "helios.part"] {
        assert!(contents.metadata.contains_key(name), "metadata {name}: {:?}", contents.metadata.keys().collect::<Vec<_>>());
    }
    assert_eq!(contents.metadata["helios.session"]["board"], "test-board");
    assert_eq!(contents.metadata["helios.camera"]["calibration"], "null");
    assert!(contents.attachments.contains(&ATTACHMENT_FIELD.to_string()));
    assert!(contents.attachments.contains(&format!("{ATTACHMENT_PIPELINE_PREFIX}p1")));

    let stored = catalog::list(&recordings).expect("list");
    assert_eq!(stored.len(), 1);
    let summary = stored[0].summary.as_ref().expect("summary beside the part");
    assert_eq!((summary.frames_written, summary.messages_written), (done.frames_written, done.messages_written));
    assert_eq!(summary.stop_reason.as_deref(), Some("requested"));
}

#[test]
fn refuses_to_start_below_the_reserve_and_names_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.styx.sock");
    let _camera = luma_camera(&socket);
    let recorder = Recorder::default();
    let refused = recorder.start(request(&socket, RecorderSettings { reserve_bytes: u64::MAX, ..settings(&dir.path().join("rec")) }));
    assert!(matches!(refused, Err(RecorderError::Storage(StorageError::ReserveNotMet { .. }))), "{refused:?}");
    assert!(recorder.status().is_none());
    assert!(mcaps(&dir.path().join("rec")).is_empty());
}

#[test]
fn a_camera_service_that_is_not_there_refuses_the_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorder = Recorder::default();
    let refused = recorder.start(request(&dir.path().join("none.sock"), settings(&dir.path().join("rec"))));
    assert!(matches!(refused, Err(RecorderError::Camera(_))), "{refused:?}");
}

#[test]
fn parts_rotate_at_the_size_cap_and_each_is_complete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.styx.sock");
    let _camera = luma_camera(&socket);
    let recordings = dir.path().join("rec");
    let recorder = Recorder::default();
    let uncompressed = RecorderSettings { compression: false, max_file_bytes: MIB, ..settings(&recordings) };
    let started = recorder.start(request(&socket, uncompressed)).expect("start");

    std::thread::sleep(Duration::from_millis(2600));
    recorder.stop(&started.id).expect("stop");
    let done = wait_idle(&recorder, Duration::from_secs(30));
    assert_eq!(done.state, RecordingState::Stopped, "{done:?}");
    assert!(done.parts.len() >= 2, "rotated: {:?}", done.parts);
    assert_eq!(done.parts[0].stop_reason.as_deref(), Some("cap"));
    assert_eq!(done.parts.iter().map(|p| p.index).collect::<Vec<_>>(), (1..=done.parts.len() as u32).collect::<Vec<_>>());
    let files = mcaps(&recordings);
    assert_eq!(files.len(), done.parts.len(), "{files:?}");
    assert!(files.iter().any(|f| f.to_string_lossy().ends_with(&format!("{}-part2.mcap", started.id))), "{files:?}");
    for file in &files {
        let contents = read(file);
        assert!(contents.metadata.contains_key("helios.session"), "every part carries the start records: {file:?}");
    }
    let frames: u64 = files.iter().map(|f| read(f).frames()).sum();
    assert_eq!(frames, done.frames_written);
}

#[test]
fn a_second_recording_is_refused_while_one_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("cam.styx.sock");
    let _camera = luma_camera(&socket);
    let recorder = Recorder::default();
    let first = recorder.start(request(&socket, settings(&dir.path().join("rec")))).expect("start");
    let busy = recorder.start(request(&socket, settings(&dir.path().join("rec"))));
    assert!(matches!(busy, Err(RecorderError::Busy(ref id)) if *id == first.id), "{busy:?}");
    recorder.stop(&first.id).expect("stop");
    wait_idle(&recorder, Duration::from_secs(30));
    let again = recorder.start(request(&socket, settings(&dir.path().join("rec")))).expect("a new recording after the first ended");
    assert_ne!(again.id, first.id, "a new file, not the first one's");
    recorder.stop(&again.id).expect("stop");
    wait_idle(&recorder, Duration::from_secs(30));
}
