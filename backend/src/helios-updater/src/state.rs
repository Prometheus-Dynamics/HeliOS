use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::ipc::{UpdateStage, UpdateState, UrlArtifact};

#[derive(Debug, Clone)]
pub struct ActiveUpdate {
    pub update_id: Uuid,
    pub state: UpdateState,
}

impl ActiveUpdate {
    pub fn new(update_id: Uuid, started_at: DateTime<Utc>, artifacts: Vec<UrlArtifact>) -> Self {
        let state = UpdateState { update_id, stage: UpdateStage::Idle, progress_percent: Some(0), last_error: None, started_at: Some(started_at), finished_at: None, artifacts };
        Self { update_id, state }
    }
}

#[derive(Debug, Default)]
pub struct ServiceState {
    pub active_update: Option<ActiveUpdate>,
    pub cache_usage_bytes: u64,
}

impl ServiceState {
    pub fn snapshot(&self) -> (Option<UpdateState>, u64) {
        (self.active_update.as_ref().map(|a| a.state.clone()), self.cache_usage_bytes)
    }

    pub fn ensure_active_update(&mut self, update_id: Uuid) {
        let needs_new = match self.active_update.as_ref() {
            Some(active) => active.update_id != update_id,
            None => true,
        };

        if needs_new {
            let now = Utc::now();
            self.active_update = Some(ActiveUpdate::new(update_id, now, Vec::new()));
        }
    }

    pub fn clear_active_update(&mut self) {
        self.active_update = None;
    }

    pub fn update_progress(&mut self, stage: UpdateStage, percent: Option<u8>, error: Option<String>) {
        if let Some(active) = &mut self.active_update {
            active.state.stage = stage.clone();
            active.state.progress_percent = percent;
            active.state.last_error = error;
            if matches!(stage, UpdateStage::Complete | UpdateStage::RolledBack) {
                active.state.finished_at = Some(Utc::now());
            } else if percent.unwrap_or_default() < 100 {
                active.state.finished_at = None;
            }
        }
    }

    pub fn set_artifacts(&mut self, artifacts: Vec<UrlArtifact>) {
        if let Some(active) = &mut self.active_update {
            active.state.artifacts = artifacts;
        }
    }

    pub fn set_started_at_if_missing(&mut self, started_at: DateTime<Utc>) {
        if let Some(active) = &mut self.active_update
            && active.state.started_at.is_none()
        {
            active.state.started_at = Some(started_at);
        }
    }
}
