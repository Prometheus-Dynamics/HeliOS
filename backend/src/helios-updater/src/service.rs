use std::sync::Arc;

use crate::ipc::{MaintenanceWindow, UpdateStage, UpdaterCommand, UpdaterEvent};
use chrono::Utc;
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::apply;
use crate::artifact::{self, ReleaseManifest};
use crate::cleanup;
use crate::config::{SignaturePolicy, UpdaterConfig};
use crate::error::{Error, Result};
use crate::state::ServiceState;

const EVENT_BUS_CAPACITY: usize = 128;

pub struct UpdaterService {
    config: Arc<UpdaterConfig>,
    state: Arc<RwLock<ServiceState>>,
    client: reqwest::Client,
    command_lock: Mutex<()>,
    event_bus: broadcast::Sender<UpdaterEvent>,
    apply_tasks: Mutex<Vec<JoinHandle<()>>>,
    stage_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl UpdaterService {
    pub fn new(config: Arc<UpdaterConfig>) -> Result<Self> {
        let client = reqwest::Client::builder().user_agent(config.user_agent().to_string()).build()?;
        let (event_bus, _rx) = broadcast::channel(EVENT_BUS_CAPACITY);
        let state = Arc::new(RwLock::new(ServiceState::default()));
        spawn_state_sync(Arc::clone(&state), event_bus.subscribe());

        Ok(Self { config, state, client, command_lock: Mutex::new(()), event_bus, apply_tasks: Mutex::new(Vec::new()), stage_tasks: Mutex::new(Vec::new()) })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<UpdaterEvent> {
        self.event_bus.subscribe()
    }

    pub async fn run_post_boot_cleanup(&self) -> Result<bool> {
        cleanup::run_post_boot_cleanup(&self.config).await
    }

    pub async fn handle_command(&self, command: UpdaterCommand) -> Result<()> {
        let _guard = self.command_lock.lock().await;

        match command {
            UpdaterCommand::StageRelease { command_id: _, update_id, manifest } => self.queue_stage_release(update_id, manifest).await,
            UpdaterCommand::Cancel { command_id: _, update_id } => self.cancel_update(update_id).await,
            UpdaterCommand::ApplyRelease { command_id: _, update_id, window } => self.apply_release(update_id, window).await,
            UpdaterCommand::Rollback { command_id: _, update_id } => self.rollback(update_id).await,
            UpdaterCommand::QueryState { .. } => {
                self.publish_snapshot().await;
                Ok(())
            }
        }
    }

    async fn queue_stage_release(&self, update_id: Uuid, mut manifest: ReleaseManifest) -> Result<()> {
        manifest.update_id.get_or_insert(update_id);
        let first_url = manifest.artifacts.first().map(|a| a.url.as_str()).unwrap_or("");
        info!(%update_id, url = %first_url, "staging release");
        self.validate_manifest(update_id, &manifest)?;

        {
            let mut state = self.state.write().await;
            if let Some(active) = state.active_update.as_ref()
                && active.update_id != update_id
                && !matches!(active.state.stage, UpdateStage::Idle | UpdateStage::Complete | UpdateStage::RolledBack)
            {
                return Err(Error::InvalidState(format!("update {} is already in progress (stage: {:?})", active.update_id, active.state.stage)));
            }
            state.ensure_active_update(update_id);
            if let Some(active) = state.active_update.as_mut() {
                active.state.stage = UpdateStage::Downloading;
                active.state.progress_percent = Some(0);
                active.state.last_error = None;
                active.state.started_at = Some(Utc::now());
            }
        }

        self.publish_snapshot().await;

        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        let state = Arc::clone(&self.state);
        let events = self.event_bus.clone();
        let handle = tokio::spawn(async move {
            if let Err(err) = stage_release_job(client, config, state, events.clone(), update_id, manifest).await {
                let _ = events.send(UpdaterEvent::RollbackTriggered { update_id, reason: err.to_string() });
            }
        });
        self.track_stage_task(handle).await;
        Ok(())
    }

    async fn track_stage_task(&self, handle: JoinHandle<()>) {
        let mut tasks = self.stage_tasks.lock().await;
        tasks.retain(|task| !task.is_finished());
        tasks.push(handle);
    }

    async fn cancel_update(&self, update_id: Uuid) -> Result<()> {
        info!(%update_id, "cancelling staged update");
        self.purge_update_dirs(update_id).await?;
        let cache_usage = artifact::cache_usage_bytes(self.config.cache_dir()).await?;

        {
            let mut state = self.state.write().await;
            if matches!(state.active_update.as_ref().map(|a| a.update_id), Some(id) if id == update_id) {
                state.update_progress(UpdateStage::Idle, Some(0), None);
                state.clear_active_update();
            }
            state.cache_usage_bytes = cache_usage;
        }

        self.publish_snapshot().await;
        Ok(())
    }

    async fn apply_release(&self, update_id: Uuid, window: MaintenanceWindow) -> Result<()> {
        self.ensure_update_is_ready(update_id).await?;
        info!(%update_id, window_start = %window.start, "apply release requested");

        {
            let mut state = self.state.write().await;
            state.update_progress(UpdateStage::Applying, Some(0), None);
        }
        self.publish_snapshot().await;

        let handle = apply::spawn_apply_job(Arc::clone(&self.config), Arc::clone(&self.state), self.event_bus.clone(), update_id);
        self.track_task(handle).await;
        self.publish_event(UpdaterEvent::ApplyScheduled { update_id, eta: window.start });
        Ok(())
    }

    pub(crate) async fn rollback(&self, update_id: Uuid) -> Result<()> {
        self.ensure_update_is_ready(update_id).await?;
        info!(%update_id, "rollback requested");
        self.purge_update_dirs(update_id).await?;

        let cache_usage = artifact::cache_usage_bytes(self.config.cache_dir()).await?;
        {
            let mut state = self.state.write().await;
            state.cache_usage_bytes = cache_usage;
            state.update_progress(UpdateStage::RolledBack, Some(100), None);
        }
        self.publish_event(UpdaterEvent::RollbackTriggered { update_id, reason: "manual rollback".into() });
        self.publish_snapshot().await;
        Ok(())
    }

    pub async fn snapshot_event(&self) -> UpdaterEvent {
        let state = self.state.read().await;
        let (active, cache_usage) = state.snapshot();
        UpdaterEvent::StateSnapshot { active_update: active, cache_usage_bytes: cache_usage }
    }

    pub(crate) async fn publish_snapshot(&self) {
        let event = self.snapshot_event().await;
        self.publish_event(event);
    }

    pub(crate) fn publish_event(&self, event: UpdaterEvent) {
        if let Err(err) = self.event_bus.send(event) {
            warn!(?err, "failed to publish updater event");
        }
    }

    async fn ensure_update_is_ready(&self, update_id: Uuid) -> Result<()> {
        let state = self.state.read().await;
        match state.active_update.as_ref() {
            Some(active) if active.update_id == update_id && active.state.stage == UpdateStage::AwaitingWindow => Ok(()),
            Some(active) if active.update_id == update_id => Err(Error::InvalidState(format!("update {} is not ready to apply (stage: {:?})", update_id, active.state.stage))),
            Some(active) => Err(Error::InvalidState(format!("apply called for update {} but {} is active", update_id, active.update_id))),
            None => Err(Error::InvalidState("no staged update is active".into())),
        }
    }

    fn validate_manifest(&self, update_id: Uuid, manifest: &ReleaseManifest) -> Result<()> {
        if let Some(manifest_id) = manifest.update_id
            && manifest_id != update_id
        {
            return Err(Error::Manifest(format!("manifest update_id {} does not match command update_id {}", manifest_id, update_id)));
        }

        if manifest.artifacts.is_empty() {
            return Err(Error::Manifest("manifest contains no artifacts".into()));
        }

        if matches!(self.config.signature_policy(), SignaturePolicy::Required(_)) {
            let missing = manifest.artifacts.iter().filter(|artifact| artifact.signature.is_none()).count();
            if missing > 0 {
                return Err(Error::Manifest(format!("manifest has {missing} unsigned artifacts but signatures are required")));
            }
        }

        Ok(())
    }

    async fn track_task(&self, handle: JoinHandle<()>) {
        let mut tasks = self.apply_tasks.lock().await;
        tasks.retain(|task| !task.is_finished());
        tasks.push(handle);
    }

    #[cfg(test)]
    pub(crate) fn state_handle(&self) -> Arc<RwLock<ServiceState>> {
        Arc::clone(&self.state)
    }

    async fn purge_update_dirs(&self, update_id: Uuid) -> Result<()> {
        let cache_dir = self.config.cache_dir().join(update_id.to_string());
        if tokio::fs::metadata(&cache_dir).await.is_ok() {
            tokio::fs::remove_dir_all(&cache_dir).await?;
        }

        let work_dir = self.config.work_dir().join(update_id.to_string());
        if tokio::fs::metadata(&work_dir).await.is_ok() {
            tokio::fs::remove_dir_all(&work_dir).await?;
        }

        Ok(())
    }
}

async fn stage_release_job(
    client: reqwest::Client,
    config: Arc<UpdaterConfig>,
    state: Arc<RwLock<ServiceState>>,
    events: broadcast::Sender<UpdaterEvent>,
    update_id: Uuid,
    manifest: ReleaseManifest,
) -> Result<()> {
    let auto_apply = manifest_auto_apply(&manifest);
    let staged_files = artifact::stage_release(&client, &config, update_id, &manifest, &events).await?;

    let artifacts_summary = artifact::staged_to_url_artifacts(&staged_files);
    let cache_usage = artifact::cache_usage_bytes(config.cache_dir()).await?;

    {
        let mut guard = state.write().await;
        guard.ensure_active_update(update_id);
        guard.set_artifacts(artifacts_summary);
        if auto_apply {
            guard.update_progress(UpdateStage::Applying, Some(0), None);
        } else {
            guard.update_progress(UpdateStage::AwaitingWindow, Some(100), None);
        }
        guard.cache_usage_bytes = cache_usage;
    }

    if !auto_apply {
        let _ = events.send(UpdaterEvent::StageComplete { update_id });
    }
    let snapshot = {
        let guard = state.read().await;
        let (active, usage) = guard.snapshot();
        UpdaterEvent::StateSnapshot { active_update: active, cache_usage_bytes: usage }
    };
    let _ = events.send(snapshot);
    info!(%update_id, auto_apply, "stage complete");

    if auto_apply {
        apply::spawn_apply_job(Arc::clone(&config), Arc::clone(&state), events.clone(), update_id);
    }
    Ok(())
}

#[derive(Deserialize)]
struct ManifestMetadata {
    #[serde(default = "default_true")]
    auto_apply: bool,
}

const fn default_true() -> bool {
    true
}

fn manifest_auto_apply(manifest: &ReleaseManifest) -> bool {
    let metadata = manifest.metadata_json.trim();
    if metadata.is_empty() || metadata == "{}" {
        return true;
    }
    serde_json::from_str::<ManifestMetadata>(metadata).map(|value| value.auto_apply).unwrap_or(true)
}

fn spawn_state_sync(state: Arc<RwLock<ServiceState>>, mut rx: broadcast::Receiver<UpdaterEvent>) {
    tokio::spawn(async move {
        loop {
            let event = match rx.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            };

            match event {
                UpdaterEvent::StageProgress { update_id, percent, .. } => {
                    let mut guard = state.write().await;
                    guard.ensure_active_update(update_id);
                    if let Some(active) = guard.active_update.as_mut()
                        && active.update_id == update_id
                        && matches!(active.state.stage, UpdateStage::Idle | UpdateStage::Downloading)
                    {
                        active.state.stage = UpdateStage::Downloading;
                        active.state.progress_percent = Some(percent);
                        active.state.last_error = None;
                        active.state.finished_at = None;
                    }
                }
                UpdaterEvent::RollbackTriggered { update_id, reason } => {
                    let mut guard = state.write().await;
                    guard.ensure_active_update(update_id);
                    guard.update_progress(UpdateStage::RolledBack, Some(0), Some(reason));
                }
                _ => {}
            }
        }
    });
}
