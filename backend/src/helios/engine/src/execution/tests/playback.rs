//! A workload bound to a playback player's endpoint ticks on the replayed frames: the player is a
//! Styx `CameraService` serving a recording (as helios-api opens it), published as a
//! `camera.device` whose endpoint the engine binds like a live camera's.

use super::*;
use styx::prelude::RecvOutcome;

#[test]
fn replayed_recording_drives_workload_through_its_player_endpoint() {
    use styx::{
        capture_api::{CaptureRequest, VirtualSourceConfig},
        ipc::CameraService,
        replay::{ReplaySourceConfig, ReplayTimestamps, StreamRecorder},
    };

    // A short recording, written from a virtual camera.
    let temp = tempfile::tempdir().expect("tempdir");
    let recording = temp.path().join("recording.mcap");
    let camera = CaptureRequest::virtual_source(VirtualSourceConfig::new().name("virtual-front").format(FourCc::GREY).resolution(320, 180).fps(60)).into_device();
    let capture = CaptureRequest::new(&camera).start().expect("virtual camera starts");
    let mut recorder = StreamRecorder::create(&recording, &camera, &capture).expect("recorder");
    let (mut written, mut waits) = (0, 0);
    while written < 30 && waits < 400 {
        match capture.recv_blocking(Duration::from_millis(20)) {
            RecvOutcome::Data(frame) => {
                recorder.record(&frame).expect("record");
                written += 1;
            }
            _ => waits += 1,
        }
    }
    recorder.finish().expect("recording finishes");
    capture.stop();
    assert_eq!(written, 30, "the virtual camera delivered the recording's frames");

    // The player: the recording held at its last frame, with delivery times as timestamps.
    let player = CaptureRequest::replay_source(ReplaySourceConfig::new(&recording).hold_at_end(true).timestamps(ReplayTimestamps::Rebased)).expect("replay source").into_device();
    let socket = temp.path().join("playback-e2e.styx.sock");
    let service = CameraService::new(player).serve(&socket).expect("serve the player");

    // The engine binds the player's resource like a live camera's: a frames endpoint, labelled as
    // playback.
    let plugins = TestPlugins::load();
    let mut resource = ResourceRecord::builder("camera.playback.e2e0001", "camera.device", "provider.playback.node-local").build();
    resource.endpoints = vec![format!("{STYX_FRAMES_ENDPOINT_SCHEME}://{}", socket.display())];
    resource.labels = vec!["helios.label.capture_role=playback".into()];
    let state = state_with(vec![resource]);
    let workloads = [workload("workload.playback", ARUCO_GRAPH_DOCUMENT.into(), vec![binding("frame", "camera.playback.e2e0001")])];
    let execution = plugins.execution();
    let mut resident = ResidentExecutionSet::default();

    let last = sync_until(&mut resident, &execution, &workloads, Some(&state), "replayed frames to tick the workload", |snapshot| {
        snapshot.sessions[0].status == ExecutionSessionStatus::Running && artifact_json(&snapshot.artifacts, "execution.telemetry")["ticks_processed"].as_u64().is_some_and(|ticks| ticks >= 3)
    });
    let telemetry = artifact_json(&last.artifacts, "execution.telemetry");
    assert_eq!(telemetry["source_connected"], true, "{telemetry}");
    assert!(telemetry["frames_received"].as_u64().is_some_and(|frames| frames >= 3), "{telemetry}");
    assert!(telemetry["last_frame_width"].is_u64(), "{telemetry}");
    assert!(last.artifacts.iter().any(|artifact| artifact.kind == "host_output:detections"), "{:?}", last.artifacts);
    assert_eq!(service.stats().clients, 1);

    // Removing the workload disconnects it from the player.
    let removed = resident.sync_workloads(&execution, &[], Some(&state), 0);
    assert!(removed.sessions.is_empty());
    wait_for("the player's client to disconnect", || service.stats().clients == 0);
}
