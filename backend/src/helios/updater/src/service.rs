use crate::config::UpdaterConfig;
use crate::coordinator::lease::CoordinatorLease;
use crate::engine::UpdateEngine;
use crate::manifest::UpdateManifest;
use crate::model::{AssignedUpdateExecution, InvocationSource, SlotLayout, UpdateExecutionPlan, UpdateRunState, UpdateWorkload};
use crate::plans::rollout::UpdateRolloutPlan;
use crate::provider::UpdaterSnapshot;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterRuntimeReport {
    pub node_id: String,
    pub state_dir: PathBuf,
    pub staging_dir: PathBuf,
    pub run_state: UpdateRunState,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid node id '{value}'")]
pub struct InvalidNodeId {
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct UpdaterRuntimeService {
    node_id: String,
    state_dir: PathBuf,
    staging_dir: PathBuf,
    engine: UpdateEngine,
}

impl UpdaterRuntimeService {
    pub fn new(config: &UpdaterConfig) -> Result<Self, InvalidNodeId> {
        validate_node_id(&config.node_id)?;
        let node_id = config.node_id.clone();
        Ok(Self { node_id, state_dir: config.updater_state_dir.clone(), staging_dir: config.updater_staging_dir.clone(), engine: UpdateEngine::new() })
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn plan_rollout(&self, revision: impl Into<String>) -> UpdateRolloutPlan {
        UpdateRolloutPlan { revision: revision.into() }
    }

    pub fn plan_manifest(&self, manifest: &UpdateManifest, invocation: InvocationSource, slots: Option<&SlotLayout>) -> Result<UpdateExecutionPlan, crate::engine::UpdatePlanError> {
        self.engine.plan(manifest, invocation, slots)
    }

    pub fn coordinator_lease(&self, expires_at_ms: u64) -> CoordinatorLease {
        CoordinatorLease { leader: self.node_id.clone(), expires_at_ms }
    }

    pub fn report(&self) -> UpdaterRuntimeReport {
        UpdaterRuntimeReport { node_id: self.node_id.clone(), state_dir: self.state_dir.clone(), staging_dir: self.staging_dir.clone(), run_state: UpdateRunState::idle() }
    }

    pub fn snapshot(&self, assigned_workloads: Vec<UpdateWorkload>, executions: Vec<AssignedUpdateExecution>, run_state: UpdateRunState) -> UpdaterSnapshot {
        UpdaterSnapshot { node_id: self.node_id.clone(), run_state, assigned_workloads, executions }
    }
}

fn validate_node_id(value: &str) -> Result<(), InvalidNodeId> {
    if value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('_') {
        return Err(InvalidNodeId { value: value.to_string() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_service_validates_node_id() {
        let error = UpdaterRuntimeService::new(&UpdaterConfig { node_id: "bad id".into(), ..UpdaterConfig::default() }).expect_err("invalid node id should fail");
        assert_eq!(error, InvalidNodeId { value: "bad id".to_string() });
    }

    #[test]
    fn updater_service_creates_rollout_and_lease() {
        let service = UpdaterRuntimeService::new(&UpdaterConfig::default()).expect("service");
        assert_eq!(service.plan_rollout("rev-1").revision, "rev-1");
        assert_eq!(service.coordinator_lease(10).leader, *service.node_id());
        assert_eq!(service.report().node_id, *service.node_id());
        assert_eq!(service.report().run_state.phase, crate::model::UpdatePhase::Idle);
    }
}
