use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use chrono::Utc;
use helios_updater::ipc::{UpdateStage, UpdateState, UpdaterEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpdateLedMode {
    Idle = 0,
    Updating = 1,
    Error = 2,
    Rebooting = 3,
}

impl UpdateLedMode {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Updating,
            2 => Self::Error,
            3 => Self::Rebooting,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UpdateLedState {
    mode: AtomicU8,
}

impl UpdateLedState {
    pub(super) fn mode(&self) -> UpdateLedMode {
        UpdateLedMode::from_u8(self.mode.load(Ordering::Relaxed))
    }

    pub(super) fn set_mode(&self, mode: UpdateLedMode) {
        self.mode.store(mode as u8, Ordering::Relaxed);
    }
}

pub(super) fn update_mode_from_event(event: &UpdaterEvent, reboot_grace: Duration) -> Option<UpdateLedMode> {
    match event {
        UpdaterEvent::StateSnapshot { active_update, .. } => Some(update_mode_from_state(active_update.as_ref(), reboot_grace)),
        UpdaterEvent::ApplyComplete { reboot_required, .. } => Some(if *reboot_required { UpdateLedMode::Rebooting } else { UpdateLedMode::Idle }),
        UpdaterEvent::StageProgress { .. } | UpdaterEvent::StageComplete { .. } | UpdaterEvent::ApplyScheduled { .. } => Some(UpdateLedMode::Updating),
        UpdaterEvent::RollbackTriggered { reason, .. } => Some(rollback_mode_from_reason(reason)),
        _ => None,
    }
}

fn rollback_mode_from_reason(reason: &str) -> UpdateLedMode {
    if reason.trim().eq_ignore_ascii_case("manual rollback") { UpdateLedMode::Idle } else { UpdateLedMode::Error }
}

fn update_mode_from_state(state: Option<&UpdateState>, reboot_grace: Duration) -> UpdateLedMode {
    match state.map(|value| &value.stage) {
        Some(UpdateStage::Idle) | None => UpdateLedMode::Idle,
        Some(UpdateStage::RolledBack) => {
            let has_error = state.and_then(|value| value.last_error.as_deref()).map(|reason| !reason.trim().is_empty() && !reason.trim().eq_ignore_ascii_case("manual rollback")).unwrap_or(false);
            if has_error { UpdateLedMode::Error } else { UpdateLedMode::Idle }
        }
        Some(UpdateStage::Complete) => state
            .and_then(|value| value.finished_at)
            .map(|finished_at| {
                let age = Utc::now().signed_duration_since(finished_at);
                if age.num_seconds() >= 0 && age.to_std().map(|duration| duration <= reboot_grace).unwrap_or(false) { UpdateLedMode::Rebooting } else { UpdateLedMode::Idle }
            })
            .unwrap_or(UpdateLedMode::Idle),
        Some(_) => UpdateLedMode::Updating,
    }
}
