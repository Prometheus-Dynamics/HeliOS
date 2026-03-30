use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use helios_engine::ipc::{EngineEvent, StreamSummary};
use helios_engine::stream::{CodecMetrics, PipelineGraphMetrics, PipelineNodeRuntimeMetrics, StreamMetrics};
use once_cell::sync::Lazy;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::ipc::IpcHandles;

const MAX_RECENT_ACTIONS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceGuardStage {
    DecoderDisabled = 1,
    CodecsDisabled = 2,
}

#[derive(Debug, Clone)]
struct DegradedStream {
    alias: Option<String>,
    original_decoder_id: Option<String>,
    original_encoder_id: Option<String>,
    stage: ResourceGuardStage,
    changed_at_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceGuardActionKind {
    DisableDecoder,
    DisableAllCodecs,
    StopStream,
    RestoreCodecs,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardAction {
    pub at_ms: u64,
    pub kind: ResourceGuardActionKind,
    pub stream_id: Uuid,
    #[serde(default)]
    pub alias: Option<String>,
    pub score: f64,
    pub reason: String,
    #[serde(default)]
    pub mem_available_kb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardDegradedStream {
    pub stream_id: Uuid,
    #[serde(default)]
    pub alias: Option<String>,
    pub stage: ResourceGuardStage,
    pub changed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ResourceGuardStatus {
    pub enabled: bool,
    pub poll_ms: u64,
    pub cooldown_ms: u64,
    pub mem_low_kb: u64,
    pub mem_recover_kb: u64,
    #[serde(default)]
    pub last_mem_available_kb: Option<u64>,
    pub pressure_active: bool,
    #[serde(default)]
    pub degraded_streams: Vec<ResourceGuardDegradedStream>,
    #[serde(default)]
    pub last_action: Option<ResourceGuardAction>,
    #[serde(default)]
    pub recent_actions: Vec<ResourceGuardAction>,
}

#[derive(Debug, Clone, Copy)]
struct GuardConfig {
    enabled: bool,
    poll_ms: u64,
    mem_low_kb: u64,
    mem_recover_kb: u64,
    cooldown_ms: u64,
    metrics_top_n: usize,
    metrics_timeout_ms: u64,
    allow_stop_fallback: bool,
    stop_timeout_ms: u64,
}

impl GuardConfig {
    fn from_env() -> Self {
        let mem_low_kb = env_u64("HELIOS_RESOURCE_GUARD_MEM_LOW_KB", 700_000).max(64 * 1024);
        let mem_recover_kb = env_u64("HELIOS_RESOURCE_GUARD_MEM_RECOVER_KB", mem_low_kb.saturating_add(300_000)).max(mem_low_kb.saturating_add(64 * 1024));
        Self {
            enabled: env_flag("HELIOS_RESOURCE_GUARD_ENABLED", true),
            poll_ms: env_u64("HELIOS_RESOURCE_GUARD_POLL_MS", 1500).max(250),
            mem_low_kb,
            mem_recover_kb,
            cooldown_ms: env_u64("HELIOS_RESOURCE_GUARD_COOLDOWN_MS", 5000).max(500),
            metrics_top_n: env_usize("HELIOS_RESOURCE_GUARD_METRICS_TOP_N", 6).clamp(1, 24),
            metrics_timeout_ms: env_u64("HELIOS_RESOURCE_GUARD_METRICS_TIMEOUT_MS", 300).max(50),
            allow_stop_fallback: env_flag("HELIOS_RESOURCE_GUARD_ALLOW_STOP_FALLBACK", false),
            stop_timeout_ms: env_u64("HELIOS_RESOURCE_GUARD_STOP_TIMEOUT_MS", 4000).max(500),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ReliefAction {
    DisableDecoder,
    DisableAllCodecs,
    StopStream,
}

#[derive(Debug, Clone)]
struct ReliefCandidate {
    stream_id: Uuid,
    score: f64,
    action: ReliefAction,
}

enum GuardCommand {
    Restore { stream_id: Uuid, respond_to: oneshot::Sender<Result<ResourceGuardAction, String>> },
}

struct GuardRuntimeState {
    enabled: bool,
    poll_ms: u64,
    cooldown_ms: u64,
    mem_low_kb: u64,
    mem_recover_kb: u64,
    last_mem_available_kb: Option<u64>,
    pressure_active: bool,
    degraded_streams: HashMap<Uuid, ResourceGuardDegradedStream>,
    last_action: Option<ResourceGuardAction>,
    recent_actions: VecDeque<ResourceGuardAction>,
}

impl GuardRuntimeState {
    fn disabled() -> Self {
        Self {
            enabled: false,
            poll_ms: 0,
            cooldown_ms: 0,
            mem_low_kb: 0,
            mem_recover_kb: 0,
            last_mem_available_kb: None,
            pressure_active: false,
            degraded_streams: HashMap::new(),
            last_action: None,
            recent_actions: VecDeque::new(),
        }
    }

    fn from_config(cfg: GuardConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            poll_ms: cfg.poll_ms,
            cooldown_ms: cfg.cooldown_ms,
            mem_low_kb: cfg.mem_low_kb,
            mem_recover_kb: cfg.mem_recover_kb,
            last_mem_available_kb: None,
            pressure_active: false,
            degraded_streams: HashMap::new(),
            last_action: None,
            recent_actions: VecDeque::new(),
        }
    }

    fn update_mem(&mut self, mem_available_kb: u64) {
        self.last_mem_available_kb = Some(mem_available_kb);
        self.pressure_active = self.enabled && mem_available_kb <= self.mem_low_kb;
    }

    fn sync_degraded(&mut self, degraded: &HashMap<Uuid, DegradedStream>) {
        self.degraded_streams = degraded
            .iter()
            .map(|(stream_id, state)| (*stream_id, ResourceGuardDegradedStream { stream_id: *stream_id, alias: state.alias.clone(), stage: state.stage, changed_at_ms: state.changed_at_ms }))
            .collect();
    }

    fn push_action(&mut self, action: ResourceGuardAction) {
        self.last_action = Some(action.clone());
        self.recent_actions.push_front(action);
        while self.recent_actions.len() > MAX_RECENT_ACTIONS {
            self.recent_actions.pop_back();
        }
    }

    fn snapshot(&self) -> ResourceGuardStatus {
        let mut degraded_streams: Vec<_> = self.degraded_streams.values().cloned().collect();
        degraded_streams.sort_by(|a, b| b.changed_at_ms.cmp(&a.changed_at_ms));
        ResourceGuardStatus {
            enabled: self.enabled,
            poll_ms: self.poll_ms,
            cooldown_ms: self.cooldown_ms,
            mem_low_kb: self.mem_low_kb,
            mem_recover_kb: self.mem_recover_kb,
            last_mem_available_kb: self.last_mem_available_kb,
            pressure_active: self.pressure_active,
            degraded_streams,
            last_action: self.last_action.clone(),
            recent_actions: self.recent_actions.iter().cloned().collect(),
        }
    }
}

static STATE: Lazy<Mutex<GuardRuntimeState>> = Lazy::new(|| Mutex::new(GuardRuntimeState::disabled()));
static COMMAND_TX: Lazy<Mutex<Option<mpsc::UnboundedSender<GuardCommand>>>> = Lazy::new(|| Mutex::new(None));

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

    // Refine top candidates with live stream metrics to pick the heaviest stream by current load.
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
            let encoder_id = stream.manifest.encoder_id.clone();
            match handles.engine.set_codecs(stream.stream_id, None, encoder_id).await {
                Ok(EngineEvent::Ack { .. }) => {
                    let mut original_decoder_id = stream.manifest.decoder_id.clone();
                    let mut original_encoder_id = stream.manifest.encoder_id.clone();
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
                let mut original_decoder_id = stream.manifest.decoder_id.clone();
                let mut original_encoder_id = stream.manifest.encoder_id.clone();
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

fn relief_action_for_stream(stream: &StreamSummary, degraded: &HashMap<Uuid, DegradedStream>, allow_stop_fallback: bool) -> Option<ReliefAction> {
    match degraded.get(&stream.stream_id).map(|state| state.stage) {
        None => {
            if stream.manifest.decoder_id.is_some() {
                Some(ReliefAction::DisableDecoder)
            } else if stream.manifest.encoder_id.is_some() {
                Some(ReliefAction::DisableAllCodecs)
            } else if allow_stop_fallback {
                Some(ReliefAction::StopStream)
            } else {
                None
            }
        }
        Some(ResourceGuardStage::DecoderDisabled) => {
            if stream.manifest.encoder_id.is_some() {
                Some(ReliefAction::DisableAllCodecs)
            } else if allow_stop_fallback {
                Some(ReliefAction::StopStream)
            } else {
                None
            }
        }
        Some(ResourceGuardStage::CodecsDisabled) => allow_stop_fallback.then_some(ReliefAction::StopStream),
    }
}

fn rough_stream_score(stream: &StreamSummary) -> f64 {
    let mut score = 0.0;
    if stream.manifest.capture.backend == styx::BackendKind::File {
        score += 140.0;
    }
    if stream.manifest.decoder_id.is_some() {
        score += 220.0;
    }
    if stream.manifest.encoder_id.is_some() {
        score += 140.0;
    }
    if stream.manifest.pipeline_enabled != Some(false) {
        score += 180.0;
    }
    score += stream.manifest.pipelines.len() as f64 * 45.0;
    score += stream.manifest.host_buffer as f64 * 8.0;
    if stream.status.recording_active {
        score += 80.0;
    }
    score
}

async fn runtime_stream_score(handles: &Arc<IpcHandles>, stream_id: Uuid, timeout_ms: u64) -> Option<f64> {
    let response = tokio::time::timeout(Duration::from_millis(timeout_ms), handles.engine.get_metrics(stream_id)).await.ok()?;
    let event = response.ok()?;
    match event {
        EngineEvent::Metrics { metrics, .. } => Some(compute_runtime_score(&metrics)),
        EngineEvent::Nack { .. } => None,
        _ => None,
    }
}

fn compute_runtime_score(metrics: &StreamMetrics) -> f64 {
    let decoder = metrics.decoder.as_ref().map(codec_score).unwrap_or(0.0);
    let encoder = metrics.encoder.as_ref().map(codec_score).unwrap_or(0.0);

    let pipeline_primary = metrics.pipeline.as_ref().map(pipeline_score).unwrap_or(0.0);
    let pipeline_instances = if let Some(maps) = metrics.pipeline_instances.as_ref() { maps.values().map(pipeline_score).sum::<f64>() } else { 0.0 };

    let host_work_ms = if metrics.host.work_average_time_ms > 0.0 { metrics.host.work_average_time_ms } else { metrics.host.last_time_ms.max(metrics.host.average_time_ms) };
    let host_load = host_work_ms.max(0.0) * metrics.host.fps.max(0.0) * 0.05;

    decoder + encoder + pipeline_primary + pipeline_instances + host_load
}

fn codec_score(metrics: &CodecMetrics) -> f64 {
    let work_ms = if metrics.work_average_time_ms > 0.0 { metrics.work_average_time_ms } else { metrics.average_time_ms };
    work_ms.max(0.0) * metrics.fps.max(0.0)
}

fn pipeline_score(metrics: &PipelineGraphMetrics) -> f64 {
    let nodes = metrics.nodes.values().map(pipeline_node_score).sum::<f64>();
    let groups = if let Some(groups) = metrics.groups.as_ref() { groups.values().map(pipeline_node_score).sum::<f64>() } else { 0.0 };
    nodes + groups
}

fn pipeline_node_score(node: &PipelineNodeRuntimeMetrics) -> f64 {
    let base = node.metrics.average_time_ms.max(0.0) * node.metrics.average_fps.max(0.0);
    let children = if let Some(children) = node.children.as_ref() { children.values().map(pipeline_node_score).sum::<f64>() } else { 0.0 };
    base + children
}

fn read_mem_available_kb() -> Option<u64> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    parse_mem_available_kb(&contents)
}

fn parse_mem_available_kb(input: &str) -> Option<u64> {
    for line in input.lines() {
        let line = line.trim();
        if !line.starts_with("MemAvailable:") {
            continue;
        }
        let value = line.split_whitespace().nth(1)?;
        return value.parse::<u64>().ok();
    }
    None
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => matches!(raw.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|raw| raw.trim().parse::<u64>().ok()).unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|raw| raw.trim().parse::<usize>().ok()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::parse_mem_available_kb;

    #[test]
    fn parse_meminfo_available() {
        let data = "MemTotal:       7925412 kB\nMemFree:         288120 kB\nMemAvailable:   1330772 kB\n";
        assert_eq!(parse_mem_available_kb(data), Some(1_330_772));
    }

    #[test]
    fn parse_meminfo_missing_available() {
        let data = "MemTotal: 1024 kB\nMemFree: 123 kB\n";
        assert_eq!(parse_mem_available_kb(data), None);
    }
}
