use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use lib_ipc::protocol::ControlEvent;
use tokio::fs;
use tokio::time::{Instant, sleep, timeout};
use url::Url;
use uuid::Uuid;

use crate::client::{CommandId, UpdaterClient, UpdaterClientConfig, UpdaterSession};
use crate::ipc::{MaintenanceWindow, PreflightReport, UpdateStage, UpdateState, UpdaterCommand, UpdaterEvent};
use crate::{ManifestArtifact, ReleaseManifest, ReleaseManifestMetadata};

#[cfg(test)]
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateArtifactKind {
    DiskImage,
    FrontendBundle,
    ServiceBundle,
}

impl UpdateArtifactKind {
    #[must_use]
    pub const fn manifest_kind(self) -> &'static str {
        match self {
            Self::DiskImage => "disk-image",
            Self::FrontendBundle => "frontend-bundle",
            Self::ServiceBundle => "service-bundle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManualUpdateArtifact {
    pub url: Url,
    pub artifact_kind: UpdateArtifactKind,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
    pub delete_source_after_apply: bool,
    pub source_artifact_path: Option<String>,
}

impl ManualUpdateArtifact {
    #[must_use]
    pub fn new(url: Url, artifact_kind: UpdateArtifactKind) -> Self {
        Self { url, artifact_kind, size_bytes: None, checksum: None, delete_source_after_apply: true, source_artifact_path: None }
    }

    #[must_use]
    pub fn with_size_bytes(mut self, size_bytes: Option<u64>) -> Self {
        self.size_bytes = size_bytes;
        self
    }

    #[must_use]
    pub fn with_checksum(mut self, checksum: Option<String>) -> Self {
        self.checksum = checksum;
        self
    }

    #[must_use]
    pub fn with_delete_source_after_apply(mut self, delete_source_after_apply: bool) -> Self {
        self.delete_source_after_apply = delete_source_after_apply;
        self
    }

    #[must_use]
    pub fn with_source_artifact_path(mut self, source_artifact_path: Option<String>) -> Self {
        self.source_artifact_path = source_artifact_path;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpdateSource<'a> {
    Existing(Uuid),
    Manual(&'a ManualUpdateArtifact),
}

#[derive(Debug, Clone)]
pub struct PreparedUpdate {
    pub update_id: Uuid,
    pub preflight: PreflightReport,
    pub staged_manually: bool,
}

#[derive(Debug, Clone)]
pub enum PreparedApply {
    Ready(PreparedUpdate),
    Blocked(PreparedUpdate),
}

#[derive(Debug, Clone)]
pub struct TransientPreflight {
    pub update_id: Uuid,
    pub report: PreflightReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCoreError {
    message: String,
}

impl UpdateCoreError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl Display for UpdateCoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UpdateCoreError {}

impl From<String> for UpdateCoreError {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for UpdateCoreError {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[async_trait]
pub trait UpdateCoreBackend: Send + Sync {
    async fn stage_release(&self, update_id: Uuid, manifest: ReleaseManifest) -> Result<(), UpdateCoreError>;
    async fn cancel_update(&self, update_id: Uuid) -> Result<(), UpdateCoreError>;
    async fn apply_release(&self, update_id: Uuid, window: MaintenanceWindow) -> Result<(), UpdateCoreError>;
    async fn rollback_update(&self, update_id: Uuid) -> Result<(), UpdateCoreError>;
    async fn fetch_state(&self) -> Result<(Option<UpdateState>, u64), UpdateCoreError>;
    async fn fetch_preflight(&self, update_id: Uuid) -> Result<PreflightReport, UpdateCoreError>;
}

pub async fn build_manual_release_manifest(request: &ManualUpdateArtifact, update_id: Uuid) -> Result<ReleaseManifest, UpdateCoreError> {
    let mut artifact = ManifestArtifact {
        url: request.url.clone(),
        filename: None,
        size_bytes: request.size_bytes,
        sha256: request.checksum.clone(),
        signature: None,
        kind: Some(request.artifact_kind.manifest_kind().to_string()),
    };

    if artifact.size_bytes.is_none() && request.url.scheme() == "file" {
        let path = request.url.to_file_path().map_err(|_| UpdateCoreError::new("invalid image path"))?;
        if let Ok(meta) = fs::metadata(&path).await {
            artifact.size_bytes = Some(meta.len());
        }
    }

    let metadata_json = ReleaseManifestMetadata::manual_stage(request.delete_source_after_apply, request.source_artifact_path.clone()).encode_json().map_err(UpdateCoreError::new)?;

    Ok(ReleaseManifest { update_id: Some(update_id), version: None, artifacts: vec![artifact], metadata_json })
}

pub async fn stage_manual_update<B>(backend: &B, request: &ManualUpdateArtifact) -> Result<Uuid, UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    let update_id = Uuid::new_v4();
    let manifest = build_manual_release_manifest(request, update_id).await?;
    backend.stage_release(update_id, manifest).await?;
    if let Err(err) = wait_for_staged_update(backend, update_id).await {
        let _ = backend.cancel_update(update_id).await;
        return Err(err);
    }
    Ok(update_id)
}

pub async fn prepare_update_for_apply<B>(backend: &B, source: UpdateSource<'_>) -> Result<PreparedApply, UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    let (update_id, staged_manually) = match source {
        UpdateSource::Existing(update_id) => (update_id, false),
        UpdateSource::Manual(request) => (stage_manual_update(backend, request).await?, true),
    };

    let preflight = match backend.fetch_preflight(update_id).await {
        Ok(report) => report,
        Err(err) => {
            let _ = backend.cancel_update(update_id).await;
            return Err(err);
        }
    };

    let prepared = PreparedUpdate { update_id, preflight, staged_manually };
    if prepared.preflight.ready {
        Ok(PreparedApply::Ready(prepared))
    } else {
        let _ = backend.cancel_update(update_id).await;
        Ok(PreparedApply::Blocked(prepared))
    }
}

pub async fn apply_prepared_update<B>(backend: &B, update_id: Uuid) -> Result<(), UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    backend.apply_release(update_id, MaintenanceWindow { start: Utc::now(), duration: Duration::from_secs(1) }).await
}

pub async fn rollback_update<B>(backend: &B, update_id: Uuid) -> Result<(), UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    backend.rollback_update(update_id).await
}

pub async fn run_transient_preflight<B>(backend: &B, request: &ManualUpdateArtifact) -> Result<TransientPreflight, UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    let transient_request = request.clone().with_delete_source_after_apply(false);
    let update_id = stage_manual_update(backend, &transient_request).await?;
    let preflight_result = backend.fetch_preflight(update_id).await;
    let cleanup_result = ensure_transient_preflight_cleared(backend, update_id).await;

    match (preflight_result, cleanup_result) {
        (Ok(report), Ok(())) => Ok(TransientPreflight { update_id, report }),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
        (Err(err), Err(cleanup_err)) => Err(UpdateCoreError::new(format!("{err}; cleanup failed: {cleanup_err}"))),
    }
}

pub async fn wait_for_staged_update<B>(backend: &B, update_id: Uuid) -> Result<UpdateState, UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (active, _) = backend.fetch_state().await?;
        if let Some(active) = active {
            if active.update_id != update_id {
                return Err(UpdateCoreError::new(format!("different update {} became active while waiting for {}", active.update_id, update_id)));
            }
            match active.stage {
                UpdateStage::AwaitingWindow => return Ok(active),
                UpdateStage::RolledBack => {
                    return Err(UpdateCoreError::new(active.last_error.unwrap_or_else(|| format!("staging update {update_id} failed"))));
                }
                UpdateStage::Applying | UpdateStage::Rebooting | UpdateStage::Complete => {
                    return Err(UpdateCoreError::new(format!("update {} unexpectedly entered {:?} before apply was authorized", update_id, active.stage)));
                }
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            return Err(UpdateCoreError::new(format!("timed out waiting for staged update {update_id}")));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

pub async fn wait_for_update_cleared<B>(backend: &B, update_id: Uuid) -> Result<(), UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (active, _) = backend.fetch_state().await?;
        if !matches!(active, Some(current) if current.update_id == update_id) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(UpdateCoreError::new(format!("timed out waiting for transient preflight cleanup of update {update_id}")));
        }
        sleep(Duration::from_millis(200)).await;
    }
}

pub async fn ensure_transient_preflight_cleared<B>(backend: &B, update_id: Uuid) -> Result<(), UpdateCoreError>
where
    B: UpdateCoreBackend,
{
    let active = backend.fetch_state().await?.0;
    if !matches!(active, Some(current) if current.update_id == update_id) {
        return Ok(());
    }
    backend.cancel_update(update_id).await?;
    wait_for_update_cleared(backend, update_id).await
}

pub struct IpcUpdateCoreBackend {
    client: Arc<UpdaterClient>,
}

impl IpcUpdateCoreBackend {
    pub fn new(config: UpdaterClientConfig) -> Result<Self, UpdateCoreError> {
        let client = Arc::new(UpdaterClient::new(config).map_err(|err| UpdateCoreError::new(err.to_string()))?);
        Ok(Self { client })
    }

    async fn checkout_session(&self) -> Result<UpdaterSession, UpdateCoreError> {
        match timeout(Duration::from_secs(5), self.client.handshake()).await {
            Ok(result) => result.map_err(|err| UpdateCoreError::new(err.to_string())),
            Err(_) => Err(UpdateCoreError::new("updater handshake timed out")),
        }
    }

    async fn recycle_session(&self, session: UpdaterSession) {
        drop(session);
    }

    async fn send_command_wait_ack(&self, command: UpdaterCommand) -> Result<(), UpdateCoreError> {
        let command_id = command_id(&command);
        let journal = self.client.journal();
        let mut session = self.checkout_session().await?;

        journal.append(&command).map_err(|err| UpdateCoreError::new(err.to_string()))?;
        session.send_command(journal, &command).await.map_err(|err| UpdateCoreError::new(err.to_string()))?;

        let mut attempts = 0;
        let result = loop {
            attempts += 1;
            let event = match timeout(Duration::from_secs(10), session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err(UpdateCoreError::new("updater closed connection")),
                Ok(Err(err)) => break Err(UpdateCoreError::new(err.to_string())),
                Err(_) => break Err(UpdateCoreError::new("timed out waiting for updater response")),
            };

            match event {
                UpdaterEvent::Control(ControlEvent::Ack(ack)) if ack.command_id == command_id => break Ok(()),
                UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == command_id => break Err(UpdateCoreError::new(nack.reason)),
                _ => {
                    if attempts > 32 {
                        break Err(UpdateCoreError::new("updater did not acknowledge command"));
                    }
                }
            }
        };

        if result.is_ok() {
            self.recycle_session(session).await;
        }
        result
    }
}

#[async_trait]
impl UpdateCoreBackend for IpcUpdateCoreBackend {
    async fn stage_release(&self, update_id: Uuid, manifest: ReleaseManifest) -> Result<(), UpdateCoreError> {
        self.send_command_wait_ack(UpdaterCommand::StageRelease { command_id: CommandId::new(), update_id, manifest }).await
    }

    async fn cancel_update(&self, update_id: Uuid) -> Result<(), UpdateCoreError> {
        self.send_command_wait_ack(UpdaterCommand::Cancel { command_id: CommandId::new(), update_id }).await
    }

    async fn apply_release(&self, update_id: Uuid, window: MaintenanceWindow) -> Result<(), UpdateCoreError> {
        self.send_command_wait_ack(UpdaterCommand::ApplyRelease { command_id: CommandId::new(), update_id, window }).await
    }

    async fn rollback_update(&self, update_id: Uuid) -> Result<(), UpdateCoreError> {
        self.send_command_wait_ack(UpdaterCommand::Rollback { command_id: CommandId::new(), update_id }).await
    }

    async fn fetch_state(&self) -> Result<(Option<UpdateState>, u64), UpdateCoreError> {
        let command = UpdaterCommand::QueryState { command_id: CommandId::new() };
        let command_id = command_id(&command);
        let journal = self.client.journal();
        let mut session = self.checkout_session().await?;

        journal.append(&command).map_err(|err| UpdateCoreError::new(err.to_string()))?;
        session.send_command(journal, &command).await.map_err(|err| UpdateCoreError::new(err.to_string()))?;

        let result = loop {
            let event = match timeout(Duration::from_secs(5), session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err(UpdateCoreError::new("updater closed connection")),
                Ok(Err(err)) => break Err(UpdateCoreError::new(err.to_string())),
                Err(_) => break Err(UpdateCoreError::new("timed out waiting for updater state")),
            };

            match event {
                UpdaterEvent::StateSnapshot { active_update, cache_usage_bytes } => break Ok((active_update, cache_usage_bytes)),
                UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == command_id => break Err(UpdateCoreError::new(nack.reason)),
                _ => continue,
            }
        };

        if result.is_ok() {
            self.recycle_session(session).await;
        }
        result
    }

    async fn fetch_preflight(&self, update_id: Uuid) -> Result<PreflightReport, UpdateCoreError> {
        let command = UpdaterCommand::PreflightRelease { command_id: CommandId::new(), update_id };
        let command_id = command_id(&command);
        let journal = self.client.journal();
        let mut session = self.checkout_session().await?;

        journal.append(&command).map_err(|err| UpdateCoreError::new(err.to_string()))?;
        session.send_command(journal, &command).await.map_err(|err| UpdateCoreError::new(err.to_string()))?;

        let result = loop {
            let event = match timeout(Duration::from_secs(60), session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err(UpdateCoreError::new("updater closed connection")),
                Ok(Err(err)) => break Err(UpdateCoreError::new(err.to_string())),
                Err(_) => break Err(UpdateCoreError::new("timed out waiting for updater preflight")),
            };

            match event {
                UpdaterEvent::PreflightReport { report } if report.update_id == update_id => break Ok(report),
                UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == command_id => break Err(UpdateCoreError::new(nack.reason)),
                _ => continue,
            }
        };

        if result.is_ok() {
            self.recycle_session(session).await;
        }
        result
    }
}

fn command_id(command: &UpdaterCommand) -> CommandId {
    match command {
        UpdaterCommand::StageRelease { command_id, .. }
        | UpdaterCommand::Cancel { command_id, .. }
        | UpdaterCommand::ApplyRelease { command_id, .. }
        | UpdaterCommand::Rollback { command_id, .. }
        | UpdaterCommand::QueryState { command_id }
        | UpdaterCommand::QueryStorage { command_id }
        | UpdaterCommand::PreflightRelease { command_id, .. } => *command_id,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct FakeBackend {
        state_responses: Mutex<VecDeque<Result<(Option<UpdateState>, u64), UpdateCoreError>>>,
        preflight_responses: Mutex<VecDeque<Result<PreflightReport, UpdateCoreError>>>,
        stage_calls: Mutex<Vec<(Uuid, ReleaseManifest)>>,
        cancel_calls: Mutex<Vec<Uuid>>,
        apply_calls: Mutex<Vec<(Uuid, MaintenanceWindow)>>,
        rollback_calls: Mutex<Vec<Uuid>>,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                state_responses: Mutex::new(VecDeque::new()),
                preflight_responses: Mutex::new(VecDeque::new()),
                stage_calls: Mutex::new(Vec::new()),
                cancel_calls: Mutex::new(Vec::new()),
                apply_calls: Mutex::new(Vec::new()),
                rollback_calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl FakeBackend {
        async fn push_state(&self, state: Result<(Option<UpdateState>, u64), UpdateCoreError>) {
            self.state_responses.lock().await.push_back(state);
        }

        async fn push_preflight(&self, report: Result<PreflightReport, UpdateCoreError>) {
            self.preflight_responses.lock().await.push_back(report);
        }
    }

    #[async_trait]
    impl UpdateCoreBackend for FakeBackend {
        async fn stage_release(&self, update_id: Uuid, manifest: ReleaseManifest) -> Result<(), UpdateCoreError> {
            self.stage_calls.lock().await.push((update_id, manifest));
            Ok(())
        }

        async fn cancel_update(&self, update_id: Uuid) -> Result<(), UpdateCoreError> {
            self.cancel_calls.lock().await.push(update_id);
            Ok(())
        }

        async fn apply_release(&self, update_id: Uuid, window: MaintenanceWindow) -> Result<(), UpdateCoreError> {
            self.apply_calls.lock().await.push((update_id, window));
            Ok(())
        }

        async fn rollback_update(&self, update_id: Uuid) -> Result<(), UpdateCoreError> {
            self.rollback_calls.lock().await.push(update_id);
            Ok(())
        }

        async fn fetch_state(&self) -> Result<(Option<UpdateState>, u64), UpdateCoreError> {
            self.state_responses.lock().await.pop_front().unwrap_or_else(|| Err(UpdateCoreError::new("missing fake state response")))
        }

        async fn fetch_preflight(&self, _update_id: Uuid) -> Result<PreflightReport, UpdateCoreError> {
            self.preflight_responses.lock().await.pop_front().unwrap_or_else(|| Err(UpdateCoreError::new("missing fake preflight response")))
        }
    }

    fn staged_state(update_id: Uuid) -> UpdateState {
        UpdateState { update_id, stage: UpdateStage::AwaitingWindow, progress_percent: Some(100), last_error: None, started_at: Some(Utc::now()), finished_at: None, artifacts: Vec::new() }
    }

    fn blocked_preflight(update_id: Uuid, ready: bool) -> PreflightReport {
        PreflightReport {
            update_id,
            ready,
            verdict: if ready { crate::ipc::PreflightVerdict::Ready } else { crate::ipc::PreflightVerdict::ClearDataDir },
            summary: if ready { "ready".into() } else { "blocked".into() },
            artifact_kind: Some("disk-image".into()),
            target_device: Some("/dev/mmcblk0p2".into()),
            image_size_bytes: Some(1024),
            target_size_bytes: Some(2048),
            gap_after_bytes: None,
            additional_from_data_bytes: None,
            data_dir_available_bytes: None,
            work_dir_available_bytes: None,
            clear_bytes: None,
            single_slot: false,
        }
    }

    fn tempdir_for_small_artifacts() -> tempfile::TempDir {
        tempfile::tempdir_in("/dev/shm").or_else(|_| tempfile::tempdir()).expect("tempdir")
    }

    #[tokio::test]
    async fn build_manual_release_manifest_preserves_cleanup_metadata() {
        let temp = tempdir_for_small_artifacts();
        let image = temp.path().join("image.img");
        std::fs::write(&image, vec![0_u8; 11]).expect("write image");

        let request = ManualUpdateArtifact::new(Url::from_file_path(&image).expect("file url"), UpdateArtifactKind::DiskImage)
            .with_delete_source_after_apply(true)
            .with_source_artifact_path(Some(image.display().to_string()));
        let update_id = Uuid::new_v4();

        let manifest = build_manual_release_manifest(&request, update_id).await.expect("manifest");

        assert_eq!(manifest.update_id, Some(update_id));
        assert_eq!(manifest.artifacts.len(), 1);
        assert_eq!(manifest.artifacts[0].size_bytes, Some(11));
        assert_eq!(manifest.artifacts[0].kind.as_deref(), Some("disk-image"));
        let metadata = ReleaseManifestMetadata::decode_json(&manifest.metadata_json).expect("decode metadata");
        assert!(!metadata.auto_apply);
        assert!(metadata.delete_image_after_apply);
        assert_eq!(metadata.source_artifact_path.as_deref(), Some(image.display().to_string().as_str()));
    }

    #[tokio::test]
    async fn stage_manual_update_cancels_when_waiting_for_window_fails() {
        let backend = FakeBackend::default();
        let request = ManualUpdateArtifact::new(Url::parse("https://example.invalid/update.swu").expect("url"), UpdateArtifactKind::ServiceBundle);

        let update_id = Uuid::new_v4();
        backend
            .push_state(Ok((
                Some(UpdateState { update_id, stage: UpdateStage::Applying, progress_percent: Some(25), last_error: None, started_at: Some(Utc::now()), finished_at: None, artifacts: Vec::new() }),
                0,
            )))
            .await;

        let result = {
            let mut guard = backend.stage_calls.lock().await;
            guard.clear();
            drop(guard);
            stage_manual_update_with_forced_id(&backend, &request, update_id).await
        };

        let err = result.expect_err("expected stage failure");
        assert!(err.to_string().contains("unexpectedly entered"));
        assert_eq!(backend.cancel_calls.lock().await.as_slice(), &[update_id]);
    }

    #[tokio::test]
    async fn transient_preflight_cleans_up_staged_update() {
        let backend = FakeBackend::default();
        let request = ManualUpdateArtifact::new(Url::parse("https://example.invalid/update.swu").expect("url"), UpdateArtifactKind::ServiceBundle);
        let update_id = Uuid::new_v4();

        backend.push_state(Ok((Some(staged_state(update_id)), 0))).await;
        backend.push_preflight(Ok(blocked_preflight(update_id, true))).await;
        backend.push_state(Ok((Some(staged_state(update_id)), 0))).await;
        backend.push_state(Ok((None, 0))).await;

        let response = run_transient_preflight_with_forced_id(&backend, &request, update_id).await.expect("transient preflight");

        assert_eq!(response.update_id, update_id);
        assert!(response.report.ready);
        assert_eq!(backend.cancel_calls.lock().await.as_slice(), &[update_id]);
    }

    #[tokio::test]
    async fn prepare_update_for_apply_returns_blocked_and_cancels() {
        let backend = FakeBackend::default();
        let request = ManualUpdateArtifact::new(Url::parse("https://example.invalid/update.swu").expect("url"), UpdateArtifactKind::ServiceBundle);
        let update_id = Uuid::new_v4();

        backend.push_state(Ok((Some(staged_state(update_id)), 0))).await;
        backend.push_preflight(Ok(blocked_preflight(update_id, false))).await;

        let prepared = prepare_update_for_apply_with_forced_id(&backend, UpdateSource::Manual(&request), update_id).await.expect("prepared apply");

        match prepared {
            PreparedApply::Blocked(prepared) => {
                assert_eq!(prepared.update_id, update_id);
                assert!(!prepared.preflight.ready);
                assert!(prepared.staged_manually);
            }
            PreparedApply::Ready(_) => panic!("expected blocked preflight"),
        }

        assert_eq!(backend.cancel_calls.lock().await.as_slice(), &[update_id]);
    }

    async fn stage_manual_update_with_forced_id<B>(backend: &B, request: &ManualUpdateArtifact, update_id: Uuid) -> Result<Uuid, UpdateCoreError>
    where
        B: UpdateCoreBackend,
    {
        let manifest = build_manual_release_manifest(request, update_id).await?;
        backend.stage_release(update_id, manifest).await?;
        if let Err(err) = wait_for_staged_update(backend, update_id).await {
            let _ = backend.cancel_update(update_id).await;
            return Err(err);
        }
        Ok(update_id)
    }

    async fn run_transient_preflight_with_forced_id<B>(backend: &B, request: &ManualUpdateArtifact, update_id: Uuid) -> Result<TransientPreflight, UpdateCoreError>
    where
        B: UpdateCoreBackend,
    {
        let transient_request = request.clone().with_delete_source_after_apply(false);
        let _ = stage_manual_update_with_forced_id(backend, &transient_request, update_id).await?;
        let preflight_result = backend.fetch_preflight(update_id).await;
        let cleanup_result = ensure_transient_preflight_cleared(backend, update_id).await;

        match (preflight_result, cleanup_result) {
            (Ok(report), Ok(())) => Ok(TransientPreflight { update_id, report }),
            (Err(err), Ok(())) => Err(err),
            (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
            (Err(err), Err(cleanup_err)) => Err(UpdateCoreError::new(format!("{err}; cleanup failed: {cleanup_err}"))),
        }
    }

    async fn prepare_update_for_apply_with_forced_id<B>(backend: &B, source: UpdateSource<'_>, update_id: Uuid) -> Result<PreparedApply, UpdateCoreError>
    where
        B: UpdateCoreBackend,
    {
        let (update_id, staged_manually) = match source {
            UpdateSource::Existing(existing) => (existing, false),
            UpdateSource::Manual(request) => (stage_manual_update_with_forced_id(backend, request, update_id).await?, true),
        };

        let preflight = match backend.fetch_preflight(update_id).await {
            Ok(report) => report,
            Err(err) => {
                let _ = backend.cancel_update(update_id).await;
                return Err(err);
            }
        };

        let prepared = PreparedUpdate { update_id, preflight, staged_manually };
        if prepared.preflight.ready {
            Ok(PreparedApply::Ready(prepared))
        } else {
            let _ = backend.cancel_update(update_id).await;
            Ok(PreparedApply::Blocked(prepared))
        }
    }
}
