use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use once_cell::sync::Lazy;
use tokio::sync::RwLock;
use tracing::warn;

use crate::http::device::nt4 as device_nt4;
use crate::ipc::IpcHandles;
use crate::nt4::limelight_control;

use super::super::limelight_types::{LimelightAdapterId, LimelightControlState, LimelightPublishCache, LimelightReadSnapshot};
use super::naming::{AdapterSeed, build_adapter_seeds, sanitize_table_name};
use super::publish::{LimelightPublishRuntime, publish_read_topics};
use super::snapshot::{build_read_snapshot, sample_hw_base_metrics, sample_imu_snapshot};
use super::{LimelightAdapterRegistryStatus, LimelightAdapterStatus};

#[derive(Debug, Default)]
struct LimelightRuntimeState {
    emulate_enabled: bool,
    publish_phase: String,
    publish_ready: bool,
    adapters: BTreeMap<String, LimelightAdapterRuntime>,
    updated_at_ms: u64,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct LimelightAdapterRuntime {
    pub(super) id: LimelightAdapterId,
    pub(super) stream_alias: Option<String>,
    pub(super) stream_state: String,
    pub(super) recording_active: bool,
    pub(super) updated_at_ms: u64,
    pub(super) read_snapshot: LimelightReadSnapshot,
    pub(super) control_state: LimelightControlState,
    pub(super) publish_cache: LimelightPublishCache,
}

static LIMELIGHT_STATE: Lazy<RwLock<LimelightRuntimeState>> = Lazy::new(|| {
    RwLock::new(LimelightRuntimeState { emulate_enabled: false, publish_phase: "staged_pre_publish".to_string(), publish_ready: false, adapters: BTreeMap::new(), updated_at_ms: 0, last_error: None })
});

static LIMELIGHT_TASK_STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn init(handles: Arc<IpcHandles>, pool: crate::nt4::pool::Nt4ClientPool) {
    if LIMELIGHT_TASK_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    tokio::spawn(async move {
        let mut publish_runtime = LimelightPublishRuntime::default();
        let mut tick = tokio::time::interval(reconcile_period());
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(err) = reconcile_once(&handles, &pool, &mut publish_runtime).await {
                warn!(%err, "limelight adapter reconcile failed");
                let mut state = LIMELIGHT_STATE.write().await;
                state.last_error = Some(err);
                state.updated_at_ms = now_ms();
                state.publish_ready = false;
                state.publish_phase = "publish_error".to_string();
            }
        }
    });
}

pub(super) async fn registry_status() -> LimelightAdapterRegistryStatus {
    let state = LIMELIGHT_STATE.read().await;
    let adapters = state.adapters.values().map(|adapter| as_status(adapter, &state.publish_phase, state.publish_ready)).collect::<Vec<_>>();
    LimelightAdapterRegistryStatus {
        emulate_enabled: state.emulate_enabled,
        publish_phase: state.publish_phase.clone(),
        publish_ready: state.publish_ready,
        adapter_count: adapters.len(),
        updated_at_ms: state.updated_at_ms,
        last_error: state.last_error.clone(),
        adapters,
    }
}

pub(super) async fn adapter_status(table: &str) -> Option<LimelightAdapterStatus> {
    let state = LIMELIGHT_STATE.read().await;
    resolve_adapter(&state, table).map(|adapter| as_status(adapter, &state.publish_phase, state.publish_ready))
}

fn as_status(adapter: &LimelightAdapterRuntime, publish_phase: &str, publish_ready: bool) -> LimelightAdapterStatus {
    LimelightAdapterStatus {
        table_name: adapter.id.table_name.clone(),
        stream_id: adapter.id.stream_id,
        stream_alias: adapter.stream_alias.clone(),
        stream_state: adapter.stream_state.clone(),
        recording_active: adapter.recording_active,
        publish_phase: publish_phase.to_string(),
        publish_ready,
        cached_topic_count: adapter.publish_cache.by_topic.len(),
        updated_at_ms: adapter.updated_at_ms,
        read_snapshot: adapter.read_snapshot.clone(),
        control_state: adapter.control_state.clone(),
    }
}

fn resolve_adapter<'a>(state: &'a LimelightRuntimeState, table: &str) -> Option<&'a LimelightAdapterRuntime> {
    let trimmed = table.trim();
    if trimmed.is_empty() {
        return None;
    }

    let table_key = sanitize_table_name(trimmed, "");
    if !table_key.is_empty()
        && let Some(adapter) = state.adapters.get(&table_key)
    {
        return Some(adapter);
    }

    if trimmed.eq_ignore_ascii_case("limelight") && state.adapters.len() == 1 {
        return state.adapters.values().next();
    }

    for adapter in state.adapters.values() {
        if adapter.id.stream_id.to_string() == trimmed {
            return Some(adapter);
        }
        if let Some(alias) = adapter.stream_alias.as_deref() {
            if alias.eq_ignore_ascii_case(trimmed) {
                return Some(adapter);
            }
            if sanitize_table_name(alias, "") == table_key {
                return Some(adapter);
            }
        }
    }

    None
}

async fn reconcile_once(handles: &Arc<IpcHandles>, pool: &crate::nt4::pool::Nt4ClientPool, publish_runtime: &mut LimelightPublishRuntime) -> Result<(), String> {
    let settings = device_nt4::load_settings().await;
    if !(settings.enabled && settings.emulate_limelight_api) {
        publish_runtime.clear();
        let mut state = LIMELIGHT_STATE.write().await;
        state.emulate_enabled = false;
        state.publish_ready = false;
        state.publish_phase = "staged_pre_publish".to_string();
        state.adapters.clear();
        state.updated_at_ms = now_ms();
        state.last_error = None;
        return Ok(());
    }

    let streams = handles.engine.list_streams().await.map_err(|err| format!("failed to list streams for limelight adapters: {err}"))?;
    let seeds = build_adapter_seeds(streams.clone());
    let stream_by_id = streams.into_iter().map(|stream| (stream.stream_id, stream)).collect::<HashMap<_, _>>();

    let mut by_stream_id = {
        let mut state = LIMELIGHT_STATE.write().await;
        let mut by_stream_id = HashMap::new();
        for adapter in std::mem::take(&mut state.adapters).into_values() {
            by_stream_id.insert(adapter.id.stream_id, adapter);
        }
        by_stream_id
    };

    let now = now_ms();
    let hw_base = sample_hw_base_metrics();
    let imu = sample_imu_snapshot(handles).await;
    let mut adapters = BTreeMap::new();
    for seed in seeds {
        let runtime = rebuild_adapter_runtime(seed, now, &stream_by_id, &mut by_stream_id, handles, hw_base, &imu).await;
        adapters.insert(runtime.id.table_name.clone(), runtime);
    }

    let publish_outcome = publish_read_topics(pool, &settings, publish_runtime, &mut adapters).await;

    let mut state = LIMELIGHT_STATE.write().await;
    state.emulate_enabled = true;
    state.publish_ready = publish_outcome.ready;
    state.publish_phase = publish_outcome.phase;
    state.adapters = adapters;
    state.updated_at_ms = now;
    state.last_error = publish_outcome.last_error;
    Ok(())
}

async fn rebuild_adapter_runtime(
    seed: AdapterSeed,
    now: u64,
    stream_by_id: &HashMap<uuid::Uuid, helios_engine::ipc::StreamSummary>,
    by_stream_id: &mut HashMap<uuid::Uuid, LimelightAdapterRuntime>,
    handles: &Arc<IpcHandles>,
    hw_base: super::snapshot::HwBaseMetrics,
    imu: &[f64],
) -> LimelightAdapterRuntime {
    let previous = by_stream_id.remove(&seed.stream_id);
    let mut control_state = previous.as_ref().map(|adapter| adapter.control_state.clone()).unwrap_or_default();
    limelight_control::stage_control_state(&mut control_state);

    let previous_snapshot = previous.as_ref().map(|adapter| adapter.read_snapshot.clone()).unwrap_or_default();
    let publish_cache = previous.map(|adapter| adapter.publish_cache).unwrap_or_default();
    let stream_summary = stream_by_id.get(&seed.stream_id);
    let read_snapshot = build_read_snapshot(handles, seed.stream_id, stream_summary, &previous_snapshot, hw_base, imu).await;

    LimelightAdapterRuntime {
        id: LimelightAdapterId { stream_id: seed.stream_id, table_name: seed.table_name },
        stream_alias: seed.stream_alias,
        stream_state: seed.stream_state,
        recording_active: seed.recording_active,
        updated_at_ms: now,
        read_snapshot,
        control_state,
        publish_cache,
    }
}

fn reconcile_period() -> Duration {
    let ms = std::env::var("HELIOS_LIMELIGHT_RECONCILE_MS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(300).clamp(100, 5_000);
    Duration::from_millis(ms)
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
