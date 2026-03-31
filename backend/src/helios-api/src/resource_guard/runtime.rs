use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use helios_engine::ipc::{EngineEvent, StreamSummary};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};
use tracing::{info, warn};
use uuid::Uuid;

use crate::ipc::IpcHandles;

use super::scoring::{relief_action_for_stream, rough_stream_score, runtime_stream_score};
use super::system::{now_ms, read_mem_available_kb};
use super::{
    COMMAND_TX, DegradedStream, GuardCommand, GuardConfig, GuardRuntimeState, ReliefAction, ReliefCandidate, ResourceGuardAction, ResourceGuardActionKind, ResourceGuardStage, ResourceGuardStatus,
    STATE,
};

pub fn snapshot() -> ResourceGuardStatus {
    let guard = STATE.lock().expect("resource guard state lock");
    guard.snapshot()
}

pub async fn restore_stream(stream_id: Uuid) -> Result<ResourceGuardAction, String> {
    let tx = {
        let guard = COMMAND_TX.lock().expect("resource guard command lock");
        guard.clone()
    }
    .ok_or_else(|| "resource guard command channel unavailable".to_string())?;

    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(GuardCommand::Restore { stream_id, respond_to: reply_tx }).map_err(|_| "resource guard command channel closed".to_string())?;

    match timeout(Duration::from_secs(8), reply_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("resource guard command canceled".to_string()),
        Err(_) => Err("resource guard restore command timed out".to_string()),
    }
}

pub fn spawn_resource_guard_task(handles: Arc<IpcHandles>) {
    let cfg = GuardConfig::from_env();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    {
        let mut guard = COMMAND_TX.lock().expect("resource guard command lock");
        *guard = Some(cmd_tx);
    }
    if !cfg.enabled {
        let mut guard = STATE.lock().expect("resource guard state lock");
        *guard = GuardRuntimeState::disabled();
        info!("resource guard disabled");
        return;
    }

    {
        let mut guard = STATE.lock().expect("resource guard state lock");
        *guard = GuardRuntimeState::from_config(cfg);
    }

    info!(
        poll_ms = cfg.poll_ms,
        mem_low_kb = cfg.mem_low_kb,
        mem_recover_kb = cfg.mem_recover_kb,
        cooldown_ms = cfg.cooldown_ms,
        metrics_top_n = cfg.metrics_top_n,
        allow_stop_fallback = cfg.allow_stop_fallback,
        "resource guard started"
    );

    tokio::spawn(async move {
        run_resource_guard_loop(handles, cfg, cmd_rx).await;
    });
}

async fn run_resource_guard_loop(handles: Arc<IpcHandles>, cfg: GuardConfig, mut cmd_rx: mpsc::UnboundedReceiver<GuardCommand>) {
    let mut degraded: HashMap<Uuid, DegradedStream> = HashMap::new();
    let mut last_action_ms = 0u64;

    loop {
        while let Ok(command) = cmd_rx.try_recv() {
            match command {
                GuardCommand::Restore { stream_id, respond_to } => {
                    let result = handle_restore_command(&handles, &mut degraded, cfg, stream_id).await;
                    let _ = respond_to.send(result);
                }
            }
        }

        sleep(Duration::from_millis(cfg.poll_ms)).await;

        let Some(mem_available_kb) = read_mem_available_kb() else {
            continue;
        };

        update_runtime_state(cfg, mem_available_kb, &degraded);

        if degraded.is_empty() && mem_available_kb > cfg.mem_low_kb {
            continue;
        }

        let running = match handles.engine.list_streams().await {
            Ok(streams) => streams,
            Err(err) => {
                warn!(error = %err, "resource guard: failed to list streams");
                continue;
            }
        };

        degraded.retain(|stream_id, _| running.iter().any(|stream| stream.stream_id == *stream_id));
        update_runtime_state(cfg, mem_available_kb, &degraded);

        if mem_available_kb >= cfg.mem_recover_kb {
            let now = now_ms();
            if !degraded.is_empty() && now.saturating_sub(last_action_ms) >= cfg.cooldown_ms && restore_one_stream(&handles, &running, &mut degraded, cfg, mem_available_kb).await {
                last_action_ms = now_ms();
                update_runtime_state(cfg, mem_available_kb, &degraded);
            }
            continue;
        }

        if mem_available_kb > cfg.mem_low_kb {
            continue;
        }

        let now = now_ms();
        if now.saturating_sub(last_action_ms) < cfg.cooldown_ms {
            continue;
        }

        match relieve_pressure(&handles, &running, &mut degraded, cfg, mem_available_kb).await {
            Ok(true) => {
                last_action_ms = now_ms();
                update_runtime_state(cfg, mem_available_kb, &degraded);
                warn!(mem_available_kb, mem_low_kb = cfg.mem_low_kb, mem_recover_kb = cfg.mem_recover_kb, degraded_streams = degraded.len(), "resource guard applied pressure relief action");
            }
            Ok(false) => {
                warn!(mem_available_kb, mem_low_kb = cfg.mem_low_kb, "resource guard found no actionable stream for pressure relief");
            }
            Err(err) => {
                warn!(error = %err, mem_available_kb, "resource guard pressure relief failed");
            }
        }
    }
}

fn update_runtime_state(cfg: GuardConfig, mem_available_kb: u64, degraded: &HashMap<Uuid, DegradedStream>) {
    let mut guard = STATE.lock().expect("resource guard state lock");
    guard.enabled = cfg.enabled;
    guard.poll_ms = cfg.poll_ms;
    guard.cooldown_ms = cfg.cooldown_ms;
    guard.mem_low_kb = cfg.mem_low_kb;
    guard.mem_recover_kb = cfg.mem_recover_kb;
    guard.update_mem(mem_available_kb);
    guard.sync_degraded(degraded);
}

fn update_runtime_degraded_only(degraded: &HashMap<Uuid, DegradedStream>) {
    let mut guard = STATE.lock().expect("resource guard state lock");
    guard.sync_degraded(degraded);
}

fn record_runtime_action(action: ResourceGuardAction) {
    let mut guard = STATE.lock().expect("resource guard state lock");
    guard.push_action(action);
}

async fn handle_restore_command(handles: &Arc<IpcHandles>, degraded: &mut HashMap<Uuid, DegradedStream>, cfg: GuardConfig, stream_id: Uuid) -> Result<ResourceGuardAction, String> {
    if !cfg.enabled {
        return Err("resource guard is disabled".to_string());
    }
    if !degraded.contains_key(&stream_id) {
        return Err(format!("stream {stream_id} is not currently degraded by resource guard"));
    }

    let mem_available_kb = read_mem_available_kb();
    let reason = match mem_available_kb {
        Some(mem) => format!("Manual restore requested while MemAvailable is {}kB (low watermark {}kB, recover watermark {}kB)", mem, cfg.mem_low_kb, cfg.mem_recover_kb),
        None => "Manual restore requested by operator".to_string(),
    };
    let restored = restore_stream_codecs(handles, degraded, stream_id, reason, mem_available_kb).await?;

    if let Some(mem) = mem_available_kb {
        update_runtime_state(cfg, mem, degraded);
    } else {
        update_runtime_degraded_only(degraded);
    }

    Ok(restored)
}

async fn relieve_pressure(handles: &Arc<IpcHandles>, running: &[StreamSummary], degraded: &mut HashMap<Uuid, DegradedStream>, cfg: GuardConfig, mem_available_kb: u64) -> Result<bool, String> {
    let mut candidates: Vec<ReliefCandidate> = running
        .iter()
        .filter(|stream| !stream.manifest.internal)
        .filter_map(|stream| {
            relief_action_for_stream(stream, degraded, cfg.allow_stop_fallback).map(|action| ReliefCandidate { stream_id: stream.stream_id, score: rough_stream_score(stream), action })
        })
        .collect();

    if candidates.is_empty() {
        return Ok(false);
    }

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

    let top_n = candidates.len().min(cfg.metrics_top_n);
    for candidate in candidates.iter_mut().take(top_n) {
        if let Some(stream) = running.iter().find(|s| s.stream_id == candidate.stream_id)
            && let Some(runtime_score) = runtime_stream_score(handles, stream.stream_id, cfg.metrics_timeout_ms).await
        {
            candidate.score += runtime_score;
        }
    }

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
    let Some(choice) = candidates.first() else {
        return Ok(false);
    };
    let Some(stream) = running.iter().find(|s| s.stream_id == choice.stream_id) else {
        return Ok(false);
    };

    let reason = format!("MemAvailable {}kB is below low watermark {}kB", mem_available_kb, cfg.mem_low_kb);

    match choice.action {
        ReliefAction::DisableDecoder => {
            let encoder_id = stream.manifest.encoder.codec_id.clone();
            match handles.engine.set_codecs(stream.stream_id, None, encoder_id).await {
                Ok(EngineEvent::Ack { .. }) => {
                    let mut original_decoder_id = stream.manifest.decoder.codec_id.clone();
                    let mut original_encoder_id = stream.manifest.encoder.codec_id.clone();
                    if let Some(existing) = degraded.get(&stream.stream_id) {
                        original_decoder_id = existing.original_decoder_id.clone();
                        original_encoder_id = existing.original_encoder_id.clone();
                    }
                    degraded.insert(
                        stream.stream_id,
                        DegradedStream { alias: stream.manifest.identity.alias.clone(), original_decoder_id, original_encoder_id, stage: ResourceGuardStage::DecoderDisabled, changed_at_ms: now_ms() },
                    );

                    record_runtime_action(ResourceGuardAction {
                        at_ms: now_ms(),
                        kind: ResourceGuardActionKind::DisableDecoder,
                        stream_id: stream.stream_id,
                        alias: stream.manifest.identity.alias.clone(),
                        score: choice.score,
                        reason,
                        mem_available_kb: Some(mem_available_kb),
                    });

                    warn!(
                        stream_id = %stream.stream_id,
                        alias = ?stream.manifest.identity.alias,
                        score = choice.score,
                        "resource guard disabled decoder for stream"
                    );
                    Ok(true)
                }
                Ok(EngineEvent::Nack { reason, .. }) => Err(format!("engine rejected decoder disable for stream {}: {reason}", stream.stream_id)),
                Ok(other) => Err(format!("unexpected response while disabling decoder for stream {}: {other:?}", stream.stream_id)),
                Err(err) => Err(format!("failed disabling decoder for stream {}: {err}", stream.stream_id)),
            }
        }
        ReliefAction::DisableAllCodecs => match handles.engine.set_codecs(stream.stream_id, None, None).await {
            Ok(EngineEvent::Ack { .. }) => {
                let mut original_decoder_id = stream.manifest.decoder.codec_id.clone();
                let mut original_encoder_id = stream.manifest.encoder.codec_id.clone();
                if let Some(existing) = degraded.get(&stream.stream_id) {
                    original_decoder_id = existing.original_decoder_id.clone();
                    original_encoder_id = existing.original_encoder_id.clone();
                }
                degraded.insert(
                    stream.stream_id,
                    DegradedStream { alias: stream.manifest.identity.alias.clone(), original_decoder_id, original_encoder_id, stage: ResourceGuardStage::CodecsDisabled, changed_at_ms: now_ms() },
                );

                record_runtime_action(ResourceGuardAction {
                    at_ms: now_ms(),
                    kind: ResourceGuardActionKind::DisableAllCodecs,
                    stream_id: stream.stream_id,
                    alias: stream.manifest.identity.alias.clone(),
                    score: choice.score,
                    reason,
                    mem_available_kb: Some(mem_available_kb),
                });

                warn!(
                    stream_id = %stream.stream_id,
                    alias = ?stream.manifest.identity.alias,
                    score = choice.score,
                    "resource guard disabled all codecs for stream"
                );
                Ok(true)
            }
            Ok(EngineEvent::Nack { reason, .. }) => Err(format!("engine rejected codec disable for stream {}: {reason}", stream.stream_id)),
            Ok(other) => Err(format!("unexpected response while disabling codecs for stream {}: {other:?}", stream.stream_id)),
            Err(err) => Err(format!("failed disabling codecs for stream {}: {err}", stream.stream_id)),
        },
        ReliefAction::StopStream => match handles.engine.stop_stream_with_timeout(stream.stream_id, Duration::from_millis(cfg.stop_timeout_ms)).await {
            Ok(EngineEvent::Stopped { .. }) => {
                degraded.remove(&stream.stream_id);

                record_runtime_action(ResourceGuardAction {
                    at_ms: now_ms(),
                    kind: ResourceGuardActionKind::StopStream,
                    stream_id: stream.stream_id,
                    alias: stream.manifest.identity.alias.clone(),
                    score: choice.score,
                    reason: format!("MemAvailable {}kB is below low watermark {}kB and no further codec reductions were available", mem_available_kb, cfg.mem_low_kb),
                    mem_available_kb: Some(mem_available_kb),
                });

                warn!(
                    stream_id = %stream.stream_id,
                    alias = ?stream.manifest.identity.alias,
                    score = choice.score,
                    "resource guard stopped stream as last-resort relief action"
                );
                Ok(true)
            }
            Ok(EngineEvent::Nack { reason, .. }) => Err(format!("engine rejected stop for stream {}: {reason}", stream.stream_id)),
            Ok(other) => Err(format!("unexpected response while stopping stream {}: {other:?}", stream.stream_id)),
            Err(err) => Err(format!("failed stopping stream {}: {err}", stream.stream_id)),
        },
    }
}

async fn restore_stream_codecs(
    handles: &Arc<IpcHandles>,
    degraded: &mut HashMap<Uuid, DegradedStream>,
    stream_id: Uuid,
    reason: String,
    mem_available_kb: Option<u64>,
) -> Result<ResourceGuardAction, String> {
    let Some(state) = degraded.get(&stream_id).cloned() else {
        return Err(format!("stream {stream_id} is not marked degraded"));
    };

    match handles.engine.set_codecs(stream_id, state.original_decoder_id.clone(), state.original_encoder_id.clone()).await {
        Ok(EngineEvent::Ack { .. }) => {
            degraded.remove(&stream_id);
            let action = ResourceGuardAction { at_ms: now_ms(), kind: ResourceGuardActionKind::RestoreCodecs, stream_id, alias: state.alias.clone(), score: 0.0, reason, mem_available_kb };
            record_runtime_action(action.clone());
            warn!(stream_id = %stream_id, "resource guard restored stream codecs");
            Ok(action)
        }
        Ok(EngineEvent::Nack { reason, .. }) => Err(format!("engine rejected resource-guard restore for stream {stream_id}: {reason}")),
        Ok(other) => Err(format!("unexpected response while restoring codecs for stream {stream_id}: {other:?}")),
        Err(err) => Err(format!("failed restoring codecs for stream {stream_id}: {err}")),
    }
}

async fn restore_one_stream(handles: &Arc<IpcHandles>, running: &[StreamSummary], degraded: &mut HashMap<Uuid, DegradedStream>, cfg: GuardConfig, mem_available_kb: u64) -> bool {
    let mut candidates: Vec<(Uuid, DegradedStream)> =
        degraded.iter().filter_map(|(stream_id, state)| running.iter().find(|stream| stream.stream_id == *stream_id && !stream.manifest.internal).map(|_| (*stream_id, state.clone()))).collect();

    candidates.sort_by(|a, b| match b.1.stage.cmp(&a.1.stage) {
        Ordering::Equal => a.1.changed_at_ms.cmp(&b.1.changed_at_ms),
        other => other,
    });

    let Some((stream_id, _state)) = candidates.first().cloned() else {
        return false;
    };

    match restore_stream_codecs(handles, degraded, stream_id, format!("MemAvailable {}kB recovered above restore watermark {}kB", mem_available_kb, cfg.mem_recover_kb), Some(mem_available_kb)).await {
        Ok(_) => true,
        Err(err) => {
            warn!(stream_id = %stream_id, error = %err, "resource guard auto-restore failed");
            false
        }
    }
}
