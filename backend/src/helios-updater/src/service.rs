use std::sync::Arc;

use crate::ipc::{MaintenanceWindow, StorageDirectoryReport, UpdateStage, UpdaterCommand, UpdaterEvent, UpdaterStorageReport};
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
use crate::util::available_bytes_for_path;

const EVENT_BUS_CAPACITY: usize = 128;

pub struct UpdaterService {
    config: Arc<UpdaterConfig>,
    state: Arc<RwLock<ServiceState>>,
    client: tokio::sync::OnceCell<reqwest::Client>,
    command_lock: Mutex<()>,
    event_bus: broadcast::Sender<UpdaterEvent>,
    apply_tasks: Mutex<Vec<JoinHandle<()>>>,
    stage_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl UpdaterService {
    pub fn new(config: Arc<UpdaterConfig>) -> Result<Self> {
        let (event_bus, _rx) = broadcast::channel(EVENT_BUS_CAPACITY);
        let state = Arc::new(RwLock::new(ServiceState::default()));
        spawn_state_sync(Arc::clone(&state), event_bus.subscribe());

        Ok(Self { config, state, client: tokio::sync::OnceCell::new(), command_lock: Mutex::new(()), event_bus, apply_tasks: Mutex::new(Vec::new()), stage_tasks: Mutex::new(Vec::new()) })
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
            UpdaterCommand::QueryStorage { .. } => self.query_storage().await,
            UpdaterCommand::PreflightRelease { command_id: _, update_id } => self.preflight_release(update_id).await,
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

        let client = self.http_client().await?.clone();
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
        apply::purge_update_dirs(&self.config, update_id).await?;
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

    async fn preflight_release(&self, update_id: Uuid) -> Result<()> {
        let report = apply::preflight_staged_release(&self.config, update_id).await?;
        self.publish_event(UpdaterEvent::PreflightReport { report });
        Ok(())
    }

    pub(crate) async fn rollback(&self, update_id: Uuid) -> Result<()> {
        self.ensure_update_is_ready(update_id).await?;
        info!(%update_id, "rollback requested");
        apply::purge_update_dirs(&self.config, update_id).await?;

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

    async fn query_storage(&self) -> Result<()> {
        let report = self.storage_report().await?;
        self.publish_event(UpdaterEvent::StorageReport { report });
        Ok(())
    }

    pub(crate) async fn storage_report(&self) -> Result<UpdaterStorageReport> {
        Ok(UpdaterStorageReport {
            cache: storage_directory_report(self.config.cache_dir()).await?,
            work: storage_directory_report(self.config.work_dir()).await?,
            service_releases: storage_directory_report(self.config.service_releases_dir()).await?,
            frontend_releases: storage_directory_report(self.config.frontend_releases_dir()).await?,
        })
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

    async fn http_client(&self) -> Result<&reqwest::Client> {
        self.client.get_or_try_init(|| async { build_http_client(&self.config) }).await
    }
}

fn build_http_client(config: &UpdaterConfig) -> Result<reqwest::Client> {
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().map(|cert| reqwest::Certificate::from_der(cert.as_ref())).collect::<core::result::Result<Vec<_>, _>>()?;
    reqwest::Client::builder().user_agent(config.user_agent().to_string()).tls_certs_only(roots).build().map_err(Into::into)
}

async fn storage_directory_report(path: &std::path::Path) -> Result<StorageDirectoryReport> {
    Ok(StorageDirectoryReport { path: path.display().to_string(), usage_bytes: artifact::cache_usage_bytes(path).await?, available_bytes: available_bytes_for_path(path).await? })
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

#[cfg(test)]
mod tests {
    use super::{UpdaterService, storage_directory_report};
    use crate::config::UpdaterConfig;
    use crate::ipc::UpdaterEvent;
    use std::sync::Arc;

    #[tokio::test]
    async fn storage_directory_report_counts_usage() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("cache");
        tokio::fs::create_dir_all(&root).await.expect("create root");
        tokio::fs::write(root.join("artifact.bin"), vec![0u8; 11]).await.expect("write artifact");

        let report = storage_directory_report(&root).await.expect("storage report");

        assert_eq!(report.path, root.display().to_string());
        assert_eq!(report.usage_bytes, 11);
        assert!(report.available_bytes.is_some());
    }

    #[tokio::test]
    async fn updater_service_storage_report_includes_all_roots() {
        let temp = tempfile::tempdir().expect("temp dir");
        let cache_dir = temp.path().join("cache");
        let work_dir = temp.path().join("work");
        let service_releases = temp.path().join("service-releases");
        let service_bin = temp.path().join("bin");
        let frontend_releases = temp.path().join("frontend-releases");
        tokio::fs::create_dir_all(&cache_dir).await.expect("create cache dir");
        tokio::fs::create_dir_all(&work_dir).await.expect("create work dir");
        tokio::fs::create_dir_all(&service_releases).await.expect("create service releases dir");
        tokio::fs::create_dir_all(&service_bin).await.expect("create service bin dir");
        tokio::fs::create_dir_all(&frontend_releases).await.expect("create frontend releases dir");
        tokio::fs::write(cache_dir.join("artifact.bin"), vec![0u8; 13]).await.expect("write cache file");
        tokio::fs::write(work_dir.join("expanded.img"), vec![0u8; 17]).await.expect("write work file");
        tokio::fs::create_dir_all(service_releases.join("r1")).await.expect("create release dir");
        tokio::fs::write(service_releases.join("r1/helios-api"), vec![0u8; 19]).await.expect("write service release");
        tokio::fs::create_dir_all(frontend_releases.join("f1")).await.expect("create frontend dir");
        tokio::fs::write(frontend_releases.join("f1/index.html"), vec![0u8; 23]).await.expect("write frontend release");

        let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log"))
            .with_cache_dir(&cache_dir)
            .with_work_dir(&work_dir)
            .with_service_paths(&service_releases, &service_bin)
            .with_frontend_paths(&frontend_releases, frontend_releases.join("current"));
        let service = UpdaterService::new(Arc::new(config)).expect("service");

        let report = service.storage_report().await.expect("storage report");

        assert_eq!(report.cache.usage_bytes, 13);
        assert_eq!(report.work.usage_bytes, 17);
        assert_eq!(report.service_releases.usage_bytes, 19);
        assert_eq!(report.frontend_releases.usage_bytes, 23);
    }

    #[tokio::test]
    async fn query_storage_publishes_storage_report_event() {
        let temp = tempfile::tempdir().expect("temp dir");
        let cache_dir = temp.path().join("cache");
        let work_dir = temp.path().join("work");
        let service_releases = temp.path().join("service-releases");
        let service_bin = temp.path().join("bin");
        let frontend_releases = temp.path().join("frontend-releases");
        tokio::fs::create_dir_all(&cache_dir).await.expect("create cache dir");
        tokio::fs::create_dir_all(&work_dir).await.expect("create work dir");
        tokio::fs::create_dir_all(&service_releases).await.expect("create service releases dir");
        tokio::fs::create_dir_all(&service_bin).await.expect("create service bin dir");
        tokio::fs::create_dir_all(&frontend_releases).await.expect("create frontend releases dir");
        tokio::fs::write(cache_dir.join("artifact.bin"), vec![0u8; 29]).await.expect("write cache file");

        let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log"))
            .with_cache_dir(&cache_dir)
            .with_work_dir(&work_dir)
            .with_service_paths(&service_releases, &service_bin)
            .with_frontend_paths(&frontend_releases, frontend_releases.join("current"));
        let service = UpdaterService::new(Arc::new(config)).expect("service");
        let mut rx = service.subscribe();

        service.handle_command(crate::ipc::UpdaterCommand::QueryStorage { command_id: crate::client::CommandId::new() }).await.expect("query storage");

        loop {
            match rx.recv().await.expect("event") {
                UpdaterEvent::StorageReport { report } => {
                    assert_eq!(report.cache.usage_bytes, 29);
                    break;
                }
                _ => continue,
            }
        }
    }
}
