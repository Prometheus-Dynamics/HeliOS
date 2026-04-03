use super::timeouts::{scale_timeout, timeout_scale_for_streams};
use super::{EngineCommand, EngineConnectionMetrics, JournalMode};
use std::time::Duration;

#[test]
fn localization_commands_use_ephemeral_journal_mode() {
    let command = EngineCommand::SolveLocalization { command_id: lib_ipc::types::CommandId::new(), request: helios_engine::ipc::JsonWire::from(serde_json::json!({})) };
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

#[test]
fn engine_connection_metrics_track_high_water_and_disconnects() {
    let metrics = EngineConnectionMetrics::new(8);
    metrics.record_queue_enqueue();
    metrics.record_queue_enqueue();
    metrics.record_queue_dequeue();
    metrics.set_pending_len(3);
    metrics.record_roundtrip(Duration::from_millis(45));
    metrics.record_disconnect(2);

    assert_eq!(metrics.request_queue_depth.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(metrics.request_queue_high_water.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert_eq!(metrics.pending_requests.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(metrics.pending_requests_high_water.load(std::sync::atomic::Ordering::Relaxed), 3);
    assert_eq!(metrics.completed_roundtrips.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(metrics.roundtrip_total_ms.load(std::sync::atomic::Ordering::Relaxed), 45);
    assert_eq!(metrics.roundtrip_max_ms.load(std::sync::atomic::Ordering::Relaxed), 45);
    assert_eq!(metrics.disconnect_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(metrics.disconnected_pending_requests.load(std::sync::atomic::Ordering::Relaxed), 2);
}
