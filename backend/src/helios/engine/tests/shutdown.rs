//! SIGTERM stops helios-engine within the shutdown deadline, and logs the start and the end.
//!
//! No Orion runs here, so the engine waits to register with it (logging and retrying): a stop
//! must end it from there too.

use std::process::Command;
use std::time::Duration;

use helios_shutdown::{DEADLINE, testing::stop_on_sigterm};

#[test]
fn sigterm_stops_helios_engine_within_the_deadline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut command = Command::new(env!("CARGO_BIN_EXE_helios-engine"));
    command
        .env("RUST_LOG", "helios_engine=info,helios_shutdown=info")
        .env("HELIOS_NODE_ID", "node-stop-test")
        .env("HELIOS_ENGINE_LIVE_SOCKET", dir.path().join("engine-live.sock"))
        .env("ENGINE_JOURNAL_PATH", dir.path().join("engine.log"))
        .env("HELIOS_ENGINE_LIVE_SOCKET", dir.path().join("live.sock"))
        .env("HELIOS_ENGINE_CATALOG_PATH", dir.path().join("catalog.json"))
        .env("HELIOS_NT4_SETTINGS", dir.path().join("nt4.json"))
        .env("HELIOS_CONTROLLER_FACTS", dir.path().join("controller.json"))
        .env("HELIOS_DAEDALUS_PLUGIN_DIRS", dir.path().join("plugins"))
        .env("ORION_NODE_IPC_SOCKET", dir.path().join("orion.sock"))
        .env("ORION_NODE_IPC_STREAM_SOCKET", dir.path().join("orion-stream.sock"));
    let report = stop_on_sigterm(command, "failed to register provider/executor identities", Duration::from_secs(20)).expect("the engine stops");
    assert!(report.status.success(), "exit status {:?}:\n{}", report.status, report.log);
    assert!(report.elapsed < DEADLINE, "the stop took {:?}:\n{}", report.elapsed, report.log);
    assert!(report.log.contains("shutdown requested") && report.log.contains("duration_ms"), "no start and end lines:\n{}", report.log);
}
