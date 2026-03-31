use super::timeouts::{scale_timeout, timeout_scale_for_streams};
use super::{EngineCommand, JournalMode};
use std::time::Duration;

#[test]
fn localization_commands_use_ephemeral_journal_mode() {
    let command = EngineCommand::SolveLocalization { command_id: lib_ipc::types::CommandId::new(), request: helios_engine::ipc::JsonWire(serde_json::json!({})) };
    assert_eq!(JournalMode::for_command(&command), JournalMode::Ephemeral);
}

#[test]
fn scale_timeout_applies_timeout_multiplier() {
    assert_eq!(scale_timeout(Duration::from_millis(200), 1_500_000), Duration::from_millis(300));
}

#[test]
fn timeout_scale_grows_with_stream_count() {
    assert!(timeout_scale_for_streams(4) >= timeout_scale_for_streams(0));
}
