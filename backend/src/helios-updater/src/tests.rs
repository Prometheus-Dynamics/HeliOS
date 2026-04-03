#![cfg(feature = "updater-ipc")]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::ipc::{UpdateStage, UpdaterCommand, UpdaterEvent};
use chrono::Utc;
use tempfile::tempdir;
use tokio::time::{sleep, timeout};
use url::Url;
use uuid::Uuid;

use crate::apply::{self, set_fake_apply_for_tests};
use crate::artifact::{ManifestArtifact, ReleaseManifest, StagedArtifact, StagedMetadata};
use crate::client::{UpdaterClient, UpdaterClientConfig};
use crate::service::UpdaterService;
use crate::{UpdaterConfig, UpdaterRuntime};

async fn wait_for_socket(path: &Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("socket was not created in time");
}

#[lib_test::tokio_test]
async fn runtime_exits_when_token_cancelled() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("updater.sock");
    let journal_path = dir.path().join("updater.journal");
    let data_dir = dir.path().join("data");

    let config = UpdaterConfig::new(&socket_path, &journal_path).with_data_dir(&data_dir);
    let runtime = UpdaterRuntime::from_config(config);
    let shutdown = runtime.shutdown_token();

    let handle = tokio::spawn(async move {
        let mut runtime = runtime;
        runtime.start().await.expect("runtime start");
        runtime
    });

    wait_for_socket(&socket_path).await;
    sleep(Duration::from_millis(25)).await;
    shutdown.cancel();

    let runtime = timeout(Duration::from_secs(1), handle).await.expect("runtime did not exit in time").expect("runtime join failed");

    assert_eq!(runtime.config().socket_path(), socket_path.as_path());
}

#[lib_test::tokio_test]
async fn runtime_accepts_handshake_and_ack() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("updater.sock");
    let journal_path = dir.path().join("updater.journal");
    let data_dir = dir.path().join("data");

    let config = UpdaterConfig::new(&socket_path, &journal_path).with_data_dir(&data_dir).with_server_info("updater-test", "0.0.0");
    let runtime = UpdaterRuntime::from_config(config);
    let shutdown = runtime.shutdown_token();

    let handle = tokio::spawn(async move {
        let mut runtime = runtime;
        runtime.start().await.expect("runtime start");
        runtime
    });

    wait_for_socket(&socket_path).await;

    let client_dir = tempdir().expect("client tempdir");
    let client_config = UpdaterClientConfig::new(&socket_path, client_dir.path().join("updater-client.journal"));
    let client = UpdaterClient::new(client_config).expect("client");
    let mut session = client.handshake().await.expect("handshake");

    let command_id = lib_ipc::types::CommandId::new();
    let command = UpdaterCommand::QueryState { command_id };
    let entry = session.send_command(client.journal(), &command).await.expect("send command");

    let mut saw_ack = false;
    for _ in 0..5 {
        if let Some(event) = session.next_event().await.expect("event result") {
            match event {
                crate::ipc::UpdaterEvent::Ack { command_id: ack_id, .. } => {
                    assert_eq!(ack_id, command_id);
                    saw_ack = true;
                    break;
                }
                crate::ipc::UpdaterEvent::StateSnapshot { .. } => {
                    // keep waiting for ack
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
    }

    assert!(saw_ack, "expected ack event");
    client.journal().truncate_through(&entry).expect("truncate journal");

    shutdown.cancel();
    drop(session);

    let runtime = timeout(Duration::from_secs(1), handle).await.expect("runtime join timed out").expect("runtime task failed");
    assert_eq!(runtime.config().socket_path(), socket_path.as_path());
}

fn sample_metadata(config: &UpdaterConfig, update_id: Uuid) -> StagedMetadata {
    let url = Url::parse("https://example.com/update.bin").expect("url");
    let manifest_artifact =
        ManifestArtifact { url: url.clone(), filename: Some("update.bin".into()), size_bytes: Some(1024), sha256: Some("deadbeef".into()), signature: None, kind: Some("rootfs".into()) };
    let staged_artifact = StagedArtifact {
        url,
        local_path: config.cache_dir().join(update_id.to_string()).join("update.bin"),
        filename: "update.bin".into(),
        size_bytes: 1024,
        sha256: "deadbeef".into(),
        staged_at: Utc::now(),
    };
    StagedMetadata {
        schema_version: 1,
        manifest: ReleaseManifest { update_id: Some(update_id), version: Some("1.2.3".into()), artifacts: vec![manifest_artifact], metadata_json: "{}".into() },
        artifacts: vec![staged_artifact],
        staged_at: Utc::now(),
    }
}

async fn write_metadata(config: &UpdaterConfig, update_id: Uuid, metadata: &StagedMetadata) {
    let cache_dir = config.cache_dir().join(update_id.to_string());
    tokio::fs::create_dir_all(&cache_dir).await.expect("create cache dir");
    let encoded = serde_json::to_vec_pretty(metadata).expect("serialize metadata");
    tokio::fs::write(cache_dir.join("metadata.json"), encoded).await.expect("write metadata");
}

#[lib_test::tokio_test]
async fn apply_job_transitions_to_complete() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("updater.sock");
    let journal_path = dir.path().join("updater.journal");
    let data_dir = dir.path().join("data");

    let config = Arc::new(UpdaterConfig::new(&socket_path, &journal_path).with_data_dir(&data_dir));
    let service = UpdaterService::new(Arc::clone(&config)).expect("service");
    let update_id = Uuid::new_v4();

    set_fake_apply_for_tests(true);

    let state_handle = service.state_handle();
    {
        let mut state = state_handle.write().await;
        state.ensure_active_update(update_id);
        state.update_progress(UpdateStage::AwaitingWindow, Some(100), None);
    }

    let metadata = sample_metadata(&config, update_id);
    write_metadata(&config, update_id, &metadata).await;

    let (tx, _) = tokio::sync::broadcast::channel::<UpdaterEvent>(16);
    let mut rx = tx.subscribe();
    let handle = apply::spawn_apply_job(Arc::clone(&config), Arc::clone(&state_handle), tx.clone(), update_id);
    handle.await.expect("apply job");

    let mut saw_complete = false;
    for _ in 0..5 {
        match timeout(Duration::from_millis(50), rx.recv()).await {
            Ok(Ok(event)) => {
                if let UpdaterEvent::ApplyComplete { update_id: id, .. } = event
                    && id == update_id
                {
                    saw_complete = true;
                    break;
                }
            }
            _ => break,
        }
    }

    assert!(saw_complete, "expected apply complete event");

    let state = state_handle.read().await;
    let (active, _) = state.snapshot();
    let snapshot = active.expect("active update");
    assert_eq!(snapshot.stage, UpdateStage::Complete);
    assert_eq!(snapshot.progress_percent, Some(100));

    set_fake_apply_for_tests(false);
}

#[lib_test::tokio_test]
async fn rollback_transitions_to_rolled_back() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("updater.sock");
    let journal_path = dir.path().join("updater.journal");
    let data_dir = dir.path().join("data");

    let config = Arc::new(UpdaterConfig::new(&socket_path, &journal_path).with_data_dir(&data_dir));
    let service = UpdaterService::new(Arc::clone(&config)).expect("service");
    let update_id = Uuid::new_v4();

    set_fake_apply_for_tests(true);

    let state_handle = service.state_handle();
    {
        let mut state = state_handle.write().await;
        state.ensure_active_update(update_id);
        state.update_progress(UpdateStage::AwaitingWindow, Some(100), None);
    }

    let metadata = sample_metadata(&config, update_id);
    write_metadata(&config, update_id, &metadata).await;

    let mut rx = service.subscribe();
    service.rollback(update_id).await.expect("rollback");

    let mut saw_rollback = false;
    let mut saw_snapshot = false;
    for _ in 0..2 {
        if let Ok(event) = timeout(Duration::from_millis(50), rx.recv()).await.expect("event") {
            match event {
                UpdaterEvent::RollbackTriggered { update_id: id, .. } => {
                    assert_eq!(id, update_id);
                    saw_rollback = true;
                }
                UpdaterEvent::StateSnapshot { active_update: Some(state), .. } => {
                    saw_snapshot = true;
                    assert_eq!(state.stage, UpdateStage::RolledBack);
                }
                _ => {}
            }
        }
    }

    assert!(saw_rollback, "expected rollback event");
    assert!(saw_snapshot, "expected snapshot event");

    set_fake_apply_for_tests(false);
}
