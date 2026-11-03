//! SIGTERM stops helios-api within the shutdown deadline, and logs the start and the end.
//!
//! The API serves with no Orion, no engine and no UI: its stop is the HTTP server's (its grace
//! for open connections) and the runtime's (its blocking work), both bounded.

use std::process::Command;
use std::time::Duration;

use helios_shutdown::{DEADLINE, testing::stop_on_sigterm};

#[test]
fn sigterm_stops_helios_api_within_the_deadline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut command = Command::new(env!("CARGO_BIN_EXE_helios-api"));
    command
        .env("RUST_LOG", "helios_api=info,helios_shutdown=info")
        .env("HELIOS_API_BIND", "127.0.0.1:0")
        .env("HELIOS_API_STATE_DIR", dir.path().join("state"))
        .env("HELIOS_API_UPLOAD_DIR", dir.path().join("uploads"))
        .env("HELIOS_API_AUTH_FILE", dir.path().join("auth.json"))
        .env("HELIOS_API_UI_DIR", "off")
        .env("HELIOS_API_STARTUP_FILE", dir.path().join("startup.toml"))
        .env("HELIOS_API_PLAYBACK_STREAM_DIR", dir.path().join("streams"))
        .env("HELIOS_NODE_ID", "node-stop-test")
        .env("HELIOS_ORION_IPC_SOCKET", dir.path().join("orion.sock"))
        .env("HELIOS_ORION_IPC_STREAM_SOCKET", dir.path().join("orion-stream.sock"))
        .env("HELIOS_LEMNOSD_SOCKET", dir.path().join("lemnosd.sock"))
        .env("HELIOS_ENGINE_LIVE_SOCKET", dir.path().join("live.sock"))
        .env("HELIOS_BOARD_IDENTITY_PATH", dir.path().join("identity.json"))
        .env("HELIOS_BOARD_CAMERA_FILE", dir.path().join("no-camera.toml"));
    let report = stop_on_sigterm(command, "helios-api listening", Duration::from_secs(30)).expect("the API stops");
    assert!(report.status.success(), "exit status {:?}:\n{}", report.status, report.log);
    assert!(report.elapsed < DEADLINE, "the stop took {:?}:\n{}", report.elapsed, report.log);
    assert!(report.log.contains("shutdown requested") && report.log.contains("duration_ms"), "no start and end lines:\n{}", report.log);
}
