use std::time::Duration;

use helios_updater::ipc::{UpdateStage, UpdateState, UpdaterEvent};
use uuid::Uuid;

use super::state::{UpdateLedMode, update_mode_from_event};

fn sample_state(stage: UpdateStage, last_error: Option<&str>) -> UpdateState {
    UpdateState { update_id: Uuid::new_v4(), stage, progress_percent: None, last_error: last_error.map(|value| value.to_string()), started_at: None, finished_at: None, artifacts: Vec::new() }
}

#[test]
fn rollback_failure_maps_to_error_mode() {
    let event = UpdaterEvent::RollbackTriggered { update_id: Uuid::new_v4(), reason: "checksum verification failed".into() };
    assert_eq!(update_mode_from_event(&event, Duration::from_secs(45)), Some(UpdateLedMode::Error));
}

#[test]
fn manual_rollback_does_not_map_to_error_mode() {
    let event = UpdaterEvent::RollbackTriggered { update_id: Uuid::new_v4(), reason: "manual rollback".into() };
    assert_eq!(update_mode_from_event(&event, Duration::from_secs(45)), Some(UpdateLedMode::Idle));
}

#[test]
fn rolled_back_state_with_error_maps_to_error_mode() {
    let state = sample_state(UpdateStage::RolledBack, Some("artifact verification failed"));
    let event = UpdaterEvent::StateSnapshot { active_update: Some(state), cache_usage_bytes: 0 };
    assert_eq!(update_mode_from_event(&event, Duration::from_secs(45)), Some(UpdateLedMode::Error));
}
