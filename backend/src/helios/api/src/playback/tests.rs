//! Player lifecycle tests, on a small MCAP that Styx's `StreamRecorder` writes from a virtual
//! camera (no Orion: the publisher records its sets).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use styx::prelude::*;

use super::{
    PlaybackSettings,
    player::StartOptions,
    registry::Players,
    request::{PlaybackSource, SourceKind},
    source::{Roots, resolve},
    transport::{
        CONTROL_DURATION_MS, CONTROL_FRAME, CONTROL_FRAME_COUNT, CONTROL_LOOP, CONTROL_PLAYING, CONTROL_POSITION_MS, CONTROL_SEEK_FRAME, CONTROL_SEEK_MS, CONTROL_SPEED, CONTROL_STATE, CONTROL_STEP,
    },
};

/// Records `frames` frames of a virtual camera into `dir/<id>.mcap`, with the `helios.camera`
/// metadata `camera`.
fn make_recording(dir: &Path, id: &str, frames: usize, camera: BTreeMap<String, String>) -> PathBuf {
    let device = CaptureRequest::virtual_source(VirtualSourceConfig::new()).into_device();
    let handle = CaptureRequest::new(&device).start().expect("virtual camera starts");
    let path = dir.join(format!("{id}.mcap"));
    let mut recorder = StreamRecorder::create(&path, &device, &handle).expect("recorder");
    recorder.add_metadata("helios.camera", camera).expect("metadata");
    let mut written = 0;
    let mut waits = 0;
    while written < frames && waits < 200 {
        match handle.recv_blocking(Duration::from_millis(50)) {
            RecvOutcome::Data(frame) => {
                recorder.record(&frame).expect("record");
                written += 1;
            }
            _ => waits += 1,
        }
    }
    assert_eq!(written, frames, "the virtual camera delivered its frames");
    recorder.finish().expect("finish");
    handle.stop();
    path
}

fn camera_facts() -> BTreeMap<String, String> {
    BTreeMap::from([("calibration".into(), "null".into()), ("mount".into(), "null".into()), ("mode".into(), "\"virtual\"".into())])
}

fn roots(dir: &Path) -> Roots {
    Roots { recordings: dir.to_path_buf(), extra: Vec::new() }
}

fn settings(dir: &Path, max_players: usize) -> PlaybackSettings {
    PlaybackSettings { max_players, stream_dir: dir.join("streams"), roots: Vec::new() }
}

fn options() -> StartOptions {
    StartOptions { looping: false, speed: 1.0, fps: 30 }
}

#[tokio::test]
async fn a_recording_opens_as_a_camera_and_closes_with_its_resource() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_recording(dir.path(), "rec-1", 5, camera_facts());
    let players = Players::recorded(settings(dir.path(), 2));

    let resolved = resolve(&roots(dir.path()), &PlaybackSource::Recording { recording: "rec-1".into() }).expect("resolves");
    assert_eq!(resolved.facts.mode.as_deref(), Some("virtual"), "the recording's camera facts are read");
    let view = players.open(resolved, options(), 1).await.expect("opens");
    assert_eq!(view.kind, SourceKind::Recording);
    assert!(view.resource_id.starts_with("camera.playback."));
    assert!(view.frames_endpoint.starts_with("styx-frames+unix://"));

    // The player's service serves the replayed camera on its socket, as a live camera's.
    let socket = players.socket(&view.id).await.expect("socket");
    assert!(socket.exists());
    let cameras = styx::ipc::FrameClient::options(&socket).timeout(Duration::from_secs(3)).cameras().expect("the replayed camera lists");
    assert_eq!(cameras.len(), 1);

    // Its transport is Styx's replay controls, under the names the API writes and reads.
    let control = styx::ipc::ControlClient::options(&socket).timeout(Duration::from_secs(3)).controls().expect("a control client");
    let names: Vec<String> = control.controls().expect("the controls").into_iter().map(|control| control.meta.name).collect();
    for name in
        [CONTROL_PLAYING, CONTROL_SPEED, CONTROL_LOOP, CONTROL_SEEK_MS, CONTROL_SEEK_FRAME, CONTROL_STEP, CONTROL_POSITION_MS, CONTROL_FRAME, CONTROL_DURATION_MS, CONTROL_FRAME_COUNT, CONTROL_STATE]
    {
        assert!(names.iter().any(|listed| listed == name), "{name} is listed in {names:?}");
    }

    // Published to Orion as a playback camera: one resource in the provider's set.
    let set = players.published().pop().expect("a set was published");
    assert_eq!(set.len(), 1);
    assert_eq!(set[0].resource_id.as_str(), view.resource_id);
    assert!(set[0].labels.iter().any(|label| label == "helios.label.capture_role=playback"));

    // Closing stops the service, removes its socket and publishes the empty set.
    let closed = players.close(&view.id).await.expect("closes").expect("it was open");
    assert_eq!(closed.id, view.id);
    assert!(!socket.exists());
    assert!(players.published().pop().expect("a set").is_empty());
    assert!(players.close(&view.id).await.expect("still ok").is_none());
}

#[tokio::test]
async fn the_limit_refuses_another_player_until_one_closes() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_recording(dir.path(), "rec-1", 2, camera_facts());
    let players = Players::recorded(settings(dir.path(), 2));
    let mut opened = Vec::new();
    for _ in 0..2 {
        let resolved = resolve(&roots(dir.path()), &PlaybackSource::Recording { recording: "rec-1".into() }).expect("resolves");
        opened.push(players.open(resolved, options(), 1).await.expect("within the limit"));
    }
    let resolved = resolve(&roots(dir.path()), &PlaybackSource::Recording { recording: "rec-1".into() }).expect("resolves");
    let refused = players.open(resolved.clone(), options(), 1).await.expect_err("the third is over the limit");
    assert_eq!(refused.code.status(), axum::http::StatusCode::CONFLICT);

    players.close(&opened[0].id).await.expect("closes");
    players.open(resolved, options(), 1).await.expect("a slot is free again");
    assert_eq!(players.views().await.len(), 2);
}

#[tokio::test]
async fn an_image_folder_opens_as_an_image_player_with_its_frame_rate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let frames = dir.path().join("frames");
    std::fs::create_dir(&frames).expect("folder");
    for index in 0..3 {
        std::fs::write(frames.join(format!("{index:04}.png")), b"not decoded until a frame is asked for").expect("image");
    }
    let players = Players::recorded(settings(dir.path(), 2));
    let resolved = resolve(&roots(dir.path()), &PlaybackSource::Path { path: frames.to_string_lossy().into_owned() }).expect("resolves");
    let view = players.open(resolved, StartOptions { fps: 12, ..options() }, 1).await.expect("opens");
    assert_eq!(view.kind, SourceKind::Images);
    assert_eq!(view.fps, Some(12));
    assert!(view.note.is_some(), "image sequences say what they cannot do yet");
    assert!(players.socket(&view.id).await.expect("socket").exists());
    players.close(&view.id).await.expect("closes");
}

#[tokio::test]
async fn a_fresh_start_publishes_no_players_so_stale_ones_disappear() {
    let dir = tempfile::tempdir().expect("tempdir");
    make_recording(dir.path(), "rec-1", 2, camera_facts());
    let before = Players::recorded(settings(dir.path(), 2));
    let resolved = resolve(&roots(dir.path()), &PlaybackSource::Recording { recording: "rec-1".into() }).expect("resolves");
    let view = before.open(resolved, options(), 1).await.expect("opens");

    // A restarted helios-api has no players; its startup publishes the empty set.
    let after = Players::recorded(settings(dir.path(), 2));
    assert!(after.views().await.is_empty());
    after.sync().await.expect("sync");
    assert_eq!(after.published(), vec![Vec::new()]);
    assert!(after.get(&view.id).await.is_none());
    before.close(&view.id).await.expect("closes");
}
