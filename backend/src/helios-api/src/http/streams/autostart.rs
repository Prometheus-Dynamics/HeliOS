use helios_engine::ipc::{EngineEvent, StreamManifest, StreamSummary};
use tokio::time::Duration;
use tracing::{info, warn};

use crate::engine_guard;
use crate::http::AppState;
use crate::http::streams_persist::{self, PersistedStreamReconcileStatus};

use super::lifecycle::persist_effective_stream_manifest;
use super::wait::wait_for_stream_started;

fn startup_priority(record: &streams_persist::PersistedStreamRecord) -> (u8, &str) {
    let autostart = record.requested_manifest().as_ref().is_some_and(|manifest| manifest.start_on_boot);
    (u8::from(!autostart), record.camera_id.as_str())
}

fn matching_running_stream<'a>(running: &'a [StreamSummary], manifest: &StreamManifest) -> Option<&'a StreamSummary> {
    let desired_alias = manifest.identity.alias.as_deref().map(str::trim).filter(|alias| !alias.is_empty());
    running.iter().find(|stream| {
        streams_persist::manifests_conflict(&stream.manifest, manifest)
            || (manifest.identity.id.is_some() && stream.manifest.identity.id == manifest.identity.id)
            || desired_alias.is_some_and(|alias| stream.manifest.identity.alias.as_deref().map(str::trim) == Some(alias))
    })
}

async fn mark_reconcile_status(camera_id: &str, status: PersistedStreamReconcileStatus, error: Option<String>) {
    if let Err(update_err) = streams_persist::persist_reconcile_status_checked(camera_id, status, error.clone()).await {
        warn!(camera_id, error = %update_err, reconcile_error = ?error, "failed to persist startup reconcile status");
    }
}

async fn mark_running_record(state: &AppState, camera_id: &str, stream_id: uuid::Uuid, manifest: &StreamManifest) {
    if let Err(err) = persist_effective_stream_manifest(state, Some(camera_id), stream_id, manifest).await {
        warn!(camera_id, stream_id = %stream_id, error = %err, "failed to persist effective startup stream manifest");
    }
    mark_reconcile_status(camera_id, PersistedStreamReconcileStatus::Running, None).await;
}

pub(super) async fn reconcile_startup_streams(state: AppState, reason: &'static str) {
    if engine_guard::safe_mode_active() {
        warn!(reason, "engine crash guard active; skipping startup stream reconcile");
        return;
    }

    let mut records = streams_persist::list_persisted_records().await;
    if records.is_empty() {
        return;
    }
    records.sort_by(|a, b| startup_priority(a).cmp(&startup_priority(b)));

    let mut running = state.engine.list_streams().await.unwrap_or_default();
    for record in records {
        let Some(mut manifest) = record.requested_manifest() else {
            mark_reconcile_status(&record.camera_id, PersistedStreamReconcileStatus::Invalid, Some("persisted stream record is missing a requested manifest".to_string())).await;
            continue;
        };
        if manifest.internal {
            continue;
        }

        let requested_id = record.stream_id().unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        manifest.identity.id = Some(requested_id);

        let prepared = match streams_persist::prepare_manifest_for_persistence_checked(&record.camera_id, Some(requested_id), manifest).await {
            Ok(validated) => {
                if !validated.warnings.is_empty() {
                    warn!(
                        reason,
                        camera_id = %record.camera_id,
                        warning_count = validated.warnings.len(),
                        warnings = ?validated.warnings,
                        "startup stream required semantic sanitization"
                    );
                }
                validated
            }
            Err(err) => {
                let detail = err.to_string();
                mark_reconcile_status(&record.camera_id, PersistedStreamReconcileStatus::Invalid, Some(detail.clone())).await;
                warn!(reason, camera_id = %record.camera_id, error = %detail, "persisted startup stream failed semantic validation");
                continue;
            }
        };

        let manifest = prepared.manifest;
        let resolved = prepared.resolved;
        if let Err(err) = streams_persist::persist_reconciled_resolved_config_checked(&record.camera_id, Some(requested_id), resolved.clone(), PersistedStreamReconcileStatus::Ready, None).await {
            warn!(reason, camera_id = %record.camera_id, error = %err, "failed to persist reconciled startup stream config");
        }

        if let Some(existing) = matching_running_stream(&running, &manifest) {
            let existing_stream_id = existing.stream_id;
            let existing_manifest = existing.manifest.clone();
            if let Err(err) =
                streams_persist::persist_reconciled_resolved_config_checked(&record.camera_id, Some(existing_stream_id), existing_manifest, PersistedStreamReconcileStatus::Running, None).await
            {
                warn!(reason, camera_id = %record.camera_id, stream_id = %existing_stream_id, error = %err, "failed to reconcile running startup stream");
            }
            info!(reason, camera_id = %record.camera_id, stream_id = %existing_stream_id, "startup stream already running; reconciled record");
            continue;
        }

        match state.engine.start_stream(resolved.clone()).await {
            Ok(EngineEvent::Started { stream_id, .. }) => {
                mark_running_record(&state, &record.camera_id, stream_id, &manifest).await;
                running = state.engine.list_streams().await.unwrap_or(running);
            }
            Ok(EngineEvent::Nack { reason: nack_reason, .. }) => {
                let reason_lc = nack_reason.to_ascii_lowercase();
                if (reason_lc.contains("stream already exists") || reason_lc.contains("already in use") || reason_lc.contains("conflict"))
                    && let Ok(active) = state.engine.list_streams().await
                    && let Some(existing) = matching_running_stream(&active, &manifest)
                {
                    let existing_stream_id = existing.stream_id;
                    let existing_manifest = existing.manifest.clone();
                    if let Err(err) =
                        streams_persist::persist_reconciled_resolved_config_checked(&record.camera_id, Some(existing_stream_id), existing_manifest, PersistedStreamReconcileStatus::Running, None).await
                    {
                        warn!(reason, camera_id = %record.camera_id, stream_id = %existing_stream_id, error = %err, "failed to persist conflict-reconciled startup stream");
                    }
                    info!(reason, camera_id = %record.camera_id, stream_id = %existing_stream_id, "startup stream conflict resolved to existing running stream");
                    running = active;
                    continue;
                }

                mark_reconcile_status(&record.camera_id, PersistedStreamReconcileStatus::Error, Some(nack_reason.clone())).await;
                warn!(reason, camera_id = %record.camera_id, error = %nack_reason, "startup stream was rejected by engine");
            }
            Ok(_) | Err(_) => match wait_for_stream_started(&state, requested_id, Duration::from_secs(20)).await {
                Ok(Some(_descriptor)) => {
                    mark_running_record(&state, &record.camera_id, requested_id, &manifest).await;
                    running = state.engine.list_streams().await.unwrap_or(running);
                }
                Ok(None) => {
                    let detail = "engine did not confirm startup stream start".to_string();
                    mark_reconcile_status(&record.camera_id, PersistedStreamReconcileStatus::Error, Some(detail.clone())).await;
                    warn!(reason, camera_id = %record.camera_id, "engine did not confirm startup stream start");
                }
                Err(err) => {
                    let detail = err.to_string();
                    mark_reconcile_status(&record.camera_id, PersistedStreamReconcileStatus::Error, Some(detail.clone())).await;
                    warn!(reason, camera_id = %record.camera_id, error = %detail, "failed to start startup stream");
                }
            },
        }
    }
}
