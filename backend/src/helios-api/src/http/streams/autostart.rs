use futures::StreamExt;
use helios_engine::ipc::EngineEvent;
use std::collections::HashSet;
use std::sync::Arc;
use styx::BackendKind;
use tokio::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine_guard;
use crate::http::AppState;

use super::lifecycle::persist_effective_stream_manifest;
use super::util::camera_id_for_manifest;
use super::validation::validate_stream_manifest;
use super::wait::wait_for_stream_started;
use crate::http::streams_persist::manifests_conflict;

pub(super) async fn restore_autostart_streams(state: AppState) {
    if engine_guard::safe_mode_active() {
        warn!("engine crash guard active; skipping autostart streams restore");
        return;
    }

    let mut manifests = Vec::new();
    for record in crate::http::streams_persist::list_persisted_records().await {
        let Some(mut manifest) = record.requested_manifest() else {
            continue;
        };
        if manifest.internal || !manifest.start_on_boot {
            continue;
        }
        manifest.identity.id = manifest.identity.id.or(record.stream_id());
        manifests.push((record.camera_id, manifest));
    }

    if manifests.is_empty() {
        return;
    }

    let concurrency = std::env::var("HELIOS_AUTOSTART_CONCURRENCY").ok().and_then(|raw| raw.parse::<usize>().ok()).map(|v| v.clamp(1, 8)).unwrap_or(2);

    let running = state.engine.list_streams().await.unwrap_or_default();
    let mut running_keys = HashSet::new();
    for stream in &running {
        running_keys.extend(stream.manifest.capture.device_keys.iter().cloned());
    }
    let reserved_keys = Arc::new(tokio::sync::Mutex::new(running_keys));

    let state = state;
    futures::stream::iter(manifests)
        .for_each_concurrent(Some(concurrency), |(camera_id, mut manifest)| {
            let state = state.clone();
            let reserved_keys = reserved_keys.clone();
            async move {
                let prepared = match validate_stream_manifest(manifest).await {
                    Ok(validated) => {
                        if !validated.warnings.is_empty() {
                            warn!(
                                camera_id,
                                warning_count = validated.warnings.len(),
                                warnings = ?validated.warnings,
                                "autostart manifest required semantic sanitization"
                            );
                        }
                        validated
                    }
                    Err(err) => {
                        warn!(
                            camera_id,
                            issue_count = err.issues.len(),
                            warning_count = err.warnings.len(),
                            issues = ?err.issues,
                            warnings = ?err.warnings,
                            "skipping autostart manifest that failed semantic validation"
                        );
                        return;
                    }
                };
                manifest = prepared.manifest;

                let mut device_keys = manifest.capture.device_keys.clone();
                if manifest.capture.backend == BackendKind::File || device_keys.iter().any(|k| k == "media-file") {
                    device_keys.clear();
                }
                let mut reserved = Vec::new();
                if !device_keys.is_empty() {
                    let mut guard = reserved_keys.lock().await;
                    if device_keys.iter().any(|k| guard.contains(k)) {
                        return;
                    }
                    for key in &device_keys {
                        if guard.insert(key.clone()) {
                            reserved.push(key.clone());
                        }
                    }
                }

                let requested_id = *manifest.identity.id.get_or_insert_with(Uuid::new_v4);
                let start_result = state.engine.start_stream(prepared.resolved).await;
                let started = match start_result {
                    Ok(EngineEvent::Started { stream_id, .. }) => {
                        let _ = persist_effective_stream_manifest(&state, &camera_id, stream_id, &manifest).await;
                        true
                    }
                    Ok(EngineEvent::Nack { reason, .. }) => {
                        let reason_lc = reason.to_ascii_lowercase();
                        if reason_lc.contains("stream already exists") || reason_lc.contains("already in use") || reason_lc.contains("conflict") {
                            if let Ok(active) = state.engine.list_streams().await {
                                let desired_alias = manifest.identity.alias.as_deref().map(str::trim).filter(|alias| !alias.is_empty());
                                if let Some(existing) = active.into_iter().find(|stream| {
                                    manifests_conflict(&stream.manifest, &manifest)
                                        || (manifest.identity.id.is_some() && stream.manifest.identity.id == manifest.identity.id)
                                        || desired_alias.is_some_and(|alias| stream.manifest.identity.alias.as_deref().map(str::trim) == Some(alias))
                                }) {
                                    crate::http::streams_persist::persist_resolved_config(&camera_id, Some(existing.stream_id), existing.manifest.clone()).await;
                                    info!(camera_id, stream_id = %existing.stream_id, "autostart stream already running; reconciled record");
                                    true
                                } else {
                                    warn!(camera_id, error = %reason, "autostart stream was rejected by engine");
                                    false
                                }
                            } else {
                                warn!(camera_id, error = %reason, "autostart stream was rejected by engine");
                                false
                            }
                        } else {
                            warn!(camera_id, error = %reason, "autostart stream was rejected by engine");
                            false
                        }
                    }
                    Ok(_) | Err(_) => match wait_for_stream_started(&state, requested_id, Duration::from_secs(20)).await {
                        Ok(Some(_descriptor)) => {
                            let _ = persist_effective_stream_manifest(&state, &camera_id, requested_id, &manifest).await;
                            true
                        }
                        Ok(None) => {
                            warn!(camera_id, "engine did not confirm autostart stream start");
                            false
                        }
                        Err(err) => {
                            warn!(camera_id, error = %err, "failed to start autostart stream");
                            false
                        }
                    },
                };

                if !started {
                    if !reserved.is_empty() {
                        let mut guard = reserved_keys.lock().await;
                        for key in reserved {
                            guard.remove(&key);
                        }
                    }
                    return;
                }

                crate::http::streams_persist::persist_manifest(&camera_id_for_manifest(&manifest), manifest.identity.id, manifest).await;
            }
        })
        .await;
}
