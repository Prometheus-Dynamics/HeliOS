//! SIGTERM stops helios-peripherals within the shutdown deadline, and logs the start and the end.
//!
//! No Orion runs here, so the service waits to register with it (logging and retrying): a stop
//! must end it from there too.

use std::process::Command;
use std::time::Duration;

use helios_shutdown::{DEADLINE, testing::stop_on_sigterm};

#[test]
fn sigterm_stops_helios_peripherals_within_the_deadline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut command = Command::new(env!("CARGO_BIN_EXE_helios-peripherals"));
    command
        .env("RUST_LOG", "helios_peripherals=info,helios_shutdown=info")
        .env("HELIOS_NODE_ID", "node-stop-test")
        .env("HELIOS_IPC_DIR", dir.path())
        .env("HELIOS_PERIPHERAL_STREAM_DIR", dir.path().join("streams"))
        .env("ORION_NODE_IPC_SOCKET", dir.path().join("orion.sock"))
        .env("ORION_NODE_IPC_STREAM_SOCKET", dir.path().join("orion-stream.sock"))
        .env("HELIOS_ENABLE_LINUX_PROBES", "false");
    let report = stop_on_sigterm(command, "failed to register provider/executor identities", Duration::from_secs(20)).expect("the service stops");
    assert!(report.status.success(), "exit status {:?}:\n{}", report.status, report.log);
    assert!(report.elapsed < DEADLINE, "the stop took {:?}:\n{}", report.elapsed, report.log);
    assert!(report.log.contains("shutdown requested") && report.log.contains("duration_ms"), "no start and end lines:\n{}", report.log);
}
