use super::limelight_control;
use super::limelight_types::{LimelightAdapterId, LimelightControlState, LimelightPublishCache, LimelightReadSnapshot};
use crate::http::device::imu::{imu_status_from_snapshot, imu_status_from_snapshot_typed};
use crate::http::device::nt4 as device_nt4;
use crate::ipc::IpcHandles;
use helios_engine::ipc::{EngineEvent, StreamState, StreamSummary};
use helios_engine::stream::{StreamMetrics, read_latest_header};
use helios_peripherals::dto::SensorScope;
use lib_sensors::dto::{ImuAxesPayload, ImuStatusPayload};
use nt_client::data::DataType;
use nt_client::topic::Properties;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::Ipv4Addr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use sysinfo::{Components, System};
use tokio::sync::RwLock;
use tracing::warn;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightAdapterStatus {
    pub table_name: String,
    pub stream_id: uuid::Uuid,
    #[serde(default)]
    pub stream_alias: Option<String>,
    pub stream_state: String,
    pub recording_active: bool,
    pub publish_phase: String,
    pub publish_ready: bool,
    pub cached_topic_count: usize,
    pub updated_at_ms: u64,
    pub read_snapshot: LimelightReadSnapshot,
    pub control_state: LimelightControlState,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightAdapterRegistryStatus {
    pub emulate_enabled: bool,
    pub publish_phase: String,
    pub publish_ready: bool,
    pub adapter_count: usize,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub adapters: Vec<LimelightAdapterStatus>,
}

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
struct LimelightAdapterRuntime {
    id: LimelightAdapterId,
    stream_alias: Option<String>,
    stream_state: String,
    recording_active: bool,
    updated_at_ms: u64,
    read_snapshot: LimelightReadSnapshot,
    control_state: LimelightControlState,
    publish_cache: LimelightPublishCache,
}

#[derive(Debug, Clone)]
struct AdapterSeed {
    table_name: String,
    stream_id: uuid::Uuid,
    stream_alias: Option<String>,
    stream_state: String,
    recording_active: bool,
}

#[derive(Debug, Default)]
struct LimelightPublishRuntime {
    publishers: HashMap<String, nt_client::publish::GenericPublisher>,
    last_target: Option<(String, u16, String)>,
    last_entry_id: Option<u64>,
}

impl LimelightPublishRuntime {
    fn clear(&mut self) {
        self.publishers.clear();
        self.last_target = None;
        self.last_entry_id = None;
    }
}

#[derive(Debug, Clone, Copy)]
struct HwBaseMetrics {
    cpu_temp_c: f64,
    cpu_usage_pct: f64,
    ram_usage_pct: f64,
}

#[derive(Debug, Clone)]
struct PublishOutcome {
    phase: String,
    ready: bool,
    last_error: Option<String>,
}

static LIMELIGHT_STATE: Lazy<RwLock<LimelightRuntimeState>> = Lazy::new(|| {
    RwLock::new(LimelightRuntimeState { emulate_enabled: false, publish_phase: "staged_pre_publish".to_string(), publish_ready: false, adapters: BTreeMap::new(), updated_at_ms: 0, last_error: None })
});

static LIMELIGHT_TASK_STARTED: AtomicBool = AtomicBool::new(false);

pub fn init(handles: Arc<IpcHandles>) {
    if LIMELIGHT_TASK_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    tokio::spawn(async move {
        let mut publish_runtime = LimelightPublishRuntime::default();
        let mut tick = tokio::time::interval(reconcile_period());
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if let Err(err) = reconcile_once(&handles, &mut publish_runtime).await {
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

pub async fn registry_status() -> LimelightAdapterRegistryStatus {
    let state = LIMELIGHT_STATE.read().await;
    let mut adapters = Vec::with_capacity(state.adapters.len());
    for adapter in state.adapters.values() {
        adapters.push(as_status(adapter, &state.publish_phase, state.publish_ready));
    }
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

pub async fn adapter_status(table: &str) -> Option<LimelightAdapterStatus> {
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

async fn reconcile_once(handles: &Arc<IpcHandles>, publish_runtime: &mut LimelightPublishRuntime) -> Result<(), String> {
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
        let previous = by_stream_id.remove(&seed.stream_id);
        let mut control_state = previous.as_ref().map(|adapter| adapter.control_state.clone()).unwrap_or_default();
        limelight_control::stage_control_state(&mut control_state);

        let previous_snapshot = previous.as_ref().map(|adapter| adapter.read_snapshot.clone()).unwrap_or_default();
        let publish_cache = previous.map(|adapter| adapter.publish_cache).unwrap_or_default();
        let stream_summary = stream_by_id.get(&seed.stream_id);
        let read_snapshot = build_read_snapshot(handles, seed.stream_id, stream_summary, &previous_snapshot, hw_base, &imu).await;

        let runtime = LimelightAdapterRuntime {
            id: LimelightAdapterId { stream_id: seed.stream_id, table_name: seed.table_name.clone() },
            stream_alias: seed.stream_alias,
            stream_state: seed.stream_state,
            recording_active: seed.recording_active,
            updated_at_ms: now,
            read_snapshot,
            control_state,
            publish_cache,
        };
        adapters.insert(seed.table_name, runtime);
    }

    let publish_outcome = publish_read_topics(&settings, publish_runtime, &mut adapters).await;

    let mut state = LIMELIGHT_STATE.write().await;
    state.emulate_enabled = true;
    state.publish_ready = publish_outcome.ready;
    state.publish_phase = publish_outcome.phase;
    state.adapters = adapters;
    state.updated_at_ms = now;
    state.last_error = publish_outcome.last_error;
    Ok(())
}

async fn build_read_snapshot(
    handles: &Arc<IpcHandles>,
    stream_id: uuid::Uuid,
    stream_summary: Option<&StreamSummary>,
    previous: &LimelightReadSnapshot,
    hw_base: HwBaseMetrics,
    imu: &[f64],
) -> LimelightReadSnapshot {
    let mut snapshot = previous.clone();

    let metrics = fetch_stream_metrics(handles, stream_id).await;
    let detections_sample = fetch_graph_output_sample(handles, stream_id, "detections").await;
    let target_count = detections_sample.as_ref().map(extract_target_count).unwrap_or(0);
    let tid = detections_sample.as_ref().and_then(extract_primary_tid).unwrap_or(-1.0);

    snapshot.tv = if target_count > 0 { 1.0 } else { 0.0 };
    snapshot.tid = tid;
    snapshot.tx = 0.0;
    snapshot.ty = 0.0;

    snapshot.cl = metrics.as_ref().map_or(0.0, |value| sanitize_non_negative(value.capture.average_time_ms));
    snapshot.tl = metrics.as_ref().map_or(0.0, pipeline_latency_ms);

    if let Ok(header) = read_latest_header(stream_id) {
        snapshot.hb = header.seq as f64;
    }

    let capture_fps = metrics.as_ref().map_or(0.0, |value| sanitize_non_negative(value.capture.fps));
    snapshot.hw = vec![hw_base.cpu_temp_c, hw_base.cpu_usage_pct, hw_base.ram_usage_pct, capture_fps];
    snapshot.imu = imu.to_vec();
    snapshot.t2d = build_t2d(snapshot.tv, target_count, snapshot.tl, snapshot.cl, snapshot.tid);
    snapshot.json = build_json_snapshot(&snapshot, target_count, stream_summary);
    snapshot
}

fn build_t2d(tv: f64, target_count: usize, tl: f64, cl: f64, tid: f64) -> Vec<f64> {
    vec![tv, target_count as f64, tl, cl, 0.0, 0.0, 0.0, 0.0, 0.0, tid, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
}

fn build_json_snapshot(snapshot: &LimelightReadSnapshot, target_count: usize, stream_summary: Option<&StreamSummary>) -> String {
    let table = stream_summary.map(|stream| stream.stream_id.to_string()).unwrap_or_default();
    let payload = serde_json::json!({
        "Results": {
            "tv": snapshot.tv,
            "tx": snapshot.tx,
            "ty": snapshot.ty,
            "tid": snapshot.tid,
            "ta": 0.0,
            "tl": snapshot.tl,
            "cl": snapshot.cl,
            "hb": snapshot.hb,
            "targets": target_count,
            "t2d": snapshot.t2d,
            "hw": snapshot.hw,
            "imu": snapshot.imu
        },
        "table": table,
        "timestamp_ms": now_ms()
    });
    serde_json::to_string(&payload).unwrap_or_else(|_| "{\"Results\":{}}".to_string())
}

fn sample_hw_base_metrics() -> HwBaseMetrics {
    let mut sys = System::new_all();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_usage_pct = sanitize_non_negative(sys.global_cpu_usage() as f64);
    let total_memory = sys.total_memory() as f64;
    let used_memory = sys.used_memory() as f64;
    let ram_usage_pct = if total_memory > 0.0 { sanitize_non_negative((used_memory / total_memory) * 100.0) } else { 0.0 };

    let mut components = Components::new_with_refreshed_list();
    components.refresh(false);
    let cpu_temp_c = components.iter().filter_map(|component| component.temperature().map(|value| value as f64)).fold(0.0f64, f64::max);

    HwBaseMetrics { cpu_temp_c, cpu_usage_pct, ram_usage_pct }
}

async fn sample_imu_snapshot(handles: &Arc<IpcHandles>) -> Vec<f64> {
    let Some(sensors) = handles.ensure_sensors().await else {
        return default_imu_snapshot();
    };

    if let Ok(Ok(snapshot)) = sensors.sensor_snapshot_typed(SensorScope::Device).await {
        return limelight_imu_from_status(&imu_status_from_snapshot_typed(&snapshot));
    }

    match sensors.sensor_snapshot(SensorScope::Device).await {
        Ok(Ok(snapshot)) => limelight_imu_from_status(&imu_status_from_snapshot(&snapshot)),
        Err(_) => {
            handles.invalidate_sensors().await;
            default_imu_snapshot()
        }
        Ok(Err(_)) => default_imu_snapshot(),
    }
}

fn limelight_imu_from_status(status: &ImuStatusPayload) -> Vec<f64> {
    let (roll, pitch, yaw) =
        status.orientation.as_ref().map_or((0.0, 0.0, 0.0), |orientation| (sanitize_finite(orientation.roll), sanitize_finite(orientation.pitch), sanitize_finite(orientation.yaw)));
    let (gyro_x, gyro_y, gyro_z) = axes_or_default(status.gyro.as_ref().or(status.angular_velocity_dps.as_ref()));
    let (accel_x, accel_y, accel_z) = axes_or_default(status.accel.as_ref().or(status.linear_accel.as_ref()).or(status.corrected_world_accel_mps2.as_ref()));

    vec![yaw, roll, pitch, yaw, gyro_x, gyro_y, gyro_z, accel_x, accel_y, accel_z]
}

fn axes_or_default(axes: Option<&ImuAxesPayload>) -> (f64, f64, f64) {
    axes.map_or((0.0, 0.0, 0.0), |value| (sanitize_finite(value.x), sanitize_finite(value.y), sanitize_finite(value.z)))
}

fn default_imu_snapshot() -> Vec<f64> {
    vec![0.0; 10]
}

async fn fetch_stream_metrics(handles: &Arc<IpcHandles>, stream_id: uuid::Uuid) -> Option<StreamMetrics> {
    match handles.engine.get_metrics(stream_id).await {
        Ok(EngineEvent::Metrics { metrics, .. }) => Some(metrics),
        _ => None,
    }
}

async fn fetch_graph_output_sample(handles: &Arc<IpcHandles>, stream_id: uuid::Uuid, port: &str) -> Option<serde_json::Value> {
    match handles.engine.get_graph_output_sample_event(stream_id, port.to_string()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => Some(value.0),
        _ => None,
    }
}

fn extract_target_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => items.len(),
        serde_json::Value::Object(map) => {
            if let Some(items) = map.get("detections").and_then(|value| value.as_array()) {
                return items.len();
            }
            if let Some(items) = map.get("Results").and_then(|results| results.get("Fiducial")).and_then(|value| value.as_array()) {
                return items.len();
            }
            0
        }
        serde_json::Value::String(text) => serde_json::from_str::<serde_json::Value>(text).ok().map_or(0, |decoded| extract_target_count(&decoded)),
        _ => 0,
    }
}

fn extract_primary_tid(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Array(items) => items.iter().find_map(extract_tid_from_entry),
        serde_json::Value::Object(map) => {
            if let Some(items) = map.get("detections").and_then(|value| value.as_array()) {
                return items.iter().find_map(extract_tid_from_entry);
            }
            if let Some(items) = map.get("Results").and_then(|results| results.get("Fiducial")).and_then(|value| value.as_array()) {
                return items.iter().find_map(extract_tid_from_entry);
            }
            None
        }
        serde_json::Value::String(text) => serde_json::from_str::<serde_json::Value>(text).ok().and_then(|decoded| extract_primary_tid(&decoded)),
        _ => None,
    }
}

fn extract_tid_from_entry(entry: &serde_json::Value) -> Option<f64> {
    let object = entry.as_object()?;
    object
        .get("id")
        .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|id| id as f64)))
        .or_else(|| object.get("fid").and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|id| id as f64))))
}

fn pipeline_latency_ms(metrics: &StreamMetrics) -> f64 {
    if let Some(pipeline) = metrics.pipeline.as_ref()
        && let Some(graph) = pipeline.nodes.get("graph")
    {
        return sanitize_non_negative(graph.metrics.average_time_ms);
    }
    sanitize_non_negative(metrics.host.average_time_ms)
}

fn sanitize_non_negative(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { 0.0 }
}

fn sanitize_finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

async fn publish_read_topics(settings: &device_nt4::Nt4Settings, runtime: &mut LimelightPublishRuntime, adapters: &mut BTreeMap<String, LimelightAdapterRuntime>) -> PublishOutcome {
    let host_override = settings.server_host.as_ref().map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let host = match host_override {
        Some(host) => Some(host),
        None => default_nt4_server_host_from_team_file().await,
    };
    let Some(host) = host else {
        runtime.clear();
        return PublishOutcome { phase: "waiting_nt_target".to_string(), ready: false, last_error: None };
    };
    let port = settings.server_port.unwrap_or(5810);
    let hostname = lib_net::get_hostname().ok().map(|value| value.to_string_lossy().trim().to_string()).unwrap_or_default();
    let client_name = client_name_from_hostname(&hostname);
    let target = (host.clone(), port, client_name.clone());

    if runtime.last_target.as_ref() != Some(&target) {
        let _ = crate::nt4::pool().disconnect(&host, port).await;
        runtime.last_target = Some(target);
        runtime.last_entry_id = None;
        runtime.publishers.clear();
    }

    let entry = match crate::nt4::pool().get_or_connect(&host, port, &client_name).await {
        Ok(entry) => entry,
        Err(err) => {
            runtime.publishers.clear();
            return PublishOutcome { phase: "nt_connect_failed".to_string(), ready: false, last_error: Some(err) };
        }
    };
    if entry.wait_ready(Duration::from_millis(1500)).await.is_err() {
        let _ = crate::nt4::pool().disconnect(&host, port).await;
        runtime.last_entry_id = None;
        runtime.publishers.clear();
        return PublishOutcome { phase: "nt_unreachable".to_string(), ready: false, last_error: None };
    }
    if runtime.last_entry_id != Some(entry.id()) {
        runtime.last_entry_id = Some(entry.id());
        runtime.publishers.clear();
    }

    let handle = entry.handle().clone();
    for adapter in adapters.values_mut() {
        if let Err(err) = publish_adapter_snapshot(&handle, &mut runtime.publishers, adapter).await {
            return PublishOutcome { phase: "publish_error".to_string(), ready: false, last_error: Some(err) };
        }
    }

    PublishOutcome { phase: "active_publish".to_string(), ready: true, last_error: None }
}

async fn publish_adapter_snapshot(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    adapter: &mut LimelightAdapterRuntime,
) -> Result<(), String> {
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "tv", adapter.read_snapshot.tv).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "tid", adapter.read_snapshot.tid).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "tl", adapter.read_snapshot.tl).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "cl", adapter.read_snapshot.cl).await?;
    publish_double_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "hb", adapter.read_snapshot.hb).await?;
    publish_double_array_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "t2d", &adapter.read_snapshot.t2d).await?;
    publish_double_array_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "hw", &adapter.read_snapshot.hw).await?;
    publish_double_array_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "imu", &adapter.read_snapshot.imu).await?;
    publish_string_key(handle, publishers, &mut adapter.publish_cache, &adapter.id.table_name, "json", &adapter.read_snapshot.json).await?;
    Ok(())
}

async fn publish_double_key(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    cache: &mut LimelightPublishCache,
    table: &str,
    key: &str,
    value: f64,
) -> Result<(), String> {
    let topic = format!("/{table}/{key}");
    let cached = serde_json::Value::from(value);
    if !cache_changed(cache, &topic, cached) {
        return Ok(());
    }
    set_double(handle, publishers, &topic, value).await
}

async fn publish_double_array_key(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    cache: &mut LimelightPublishCache,
    table: &str,
    key: &str,
    value: &[f64],
) -> Result<(), String> {
    let topic = format!("/{table}/{key}");
    let cached = serde_json::Value::Array(value.iter().copied().map(serde_json::Value::from).collect());
    if !cache_changed(cache, &topic, cached) {
        return Ok(());
    }
    set_double_array(handle, publishers, &topic, value).await
}

async fn publish_string_key(
    handle: &nt_client::ClientHandle,
    publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>,
    cache: &mut LimelightPublishCache,
    table: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let topic = format!("/{table}/{key}");
    let cached = serde_json::Value::String(value.to_string());
    if !cache_changed(cache, &topic, cached) {
        return Ok(());
    }
    set_string(handle, publishers, &topic, value).await
}

fn cache_changed(cache: &mut LimelightPublishCache, topic: &str, value: serde_json::Value) -> bool {
    if cache.by_topic.get(topic) == Some(&value) {
        return false;
    }
    cache.by_topic.insert(topic.to_string(), value);
    true
}

async fn publisher<'a>(
    handle: &nt_client::ClientHandle,
    publishers: &'a mut HashMap<String, nt_client::publish::GenericPublisher>,
    topic: &str,
    data_type: DataType,
) -> Result<&'a nt_client::publish::GenericPublisher, String> {
    if !publishers.contains_key(topic) {
        let publisher = handle.topic(topic.to_string()).generic_publish(data_type, Properties::default()).await.map_err(|err| err.to_string())?;
        publishers.insert(topic.to_string(), publisher);
    }
    Ok(publishers.get(topic).expect("publisher exists"))
}

async fn set_double(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: f64) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::Double).await?;
    publisher.set(value).await.map_err(|err| err.to_string())
}

async fn set_double_array(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &[f64]) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::DoubleArray).await?;
    publisher.set(value.to_vec()).await.map_err(|err| err.to_string())
}

async fn set_string(handle: &nt_client::ClientHandle, publishers: &mut HashMap<String, nt_client::publish::GenericPublisher>, topic: &str, value: &str) -> Result<(), String> {
    let publisher = publisher(handle, publishers, topic, DataType::String).await?;
    publisher.set(value.to_string()).await.map_err(|err| err.to_string())
}

fn build_adapter_seeds(streams: Vec<StreamSummary>) -> Vec<AdapterSeed> {
    let mut visible = streams.into_iter().filter(|stream| !stream.manifest.internal).collect::<Vec<_>>();
    visible.sort_by(|a, b| {
        let a_key = candidate_table_base(a);
        let b_key = candidate_table_base(b);
        a_key.cmp(&b_key).then_with(|| a.stream_id.cmp(&b.stream_id))
    });

    let mut seen_tables = BTreeSet::new();
    let mut out = Vec::with_capacity(visible.len());
    for stream in visible {
        let base = candidate_table_base(&stream);
        let table_name = unique_table_name(&base, &mut seen_tables);
        let stream_state = match stream.status.state {
            StreamState::Running => "running",
            StreamState::Disabled => "disabled",
        }
        .to_string();

        out.push(AdapterSeed {
            table_name,
            stream_id: stream.stream_id,
            stream_alias: stream.manifest.identity.alias.clone().and_then(non_empty_trimmed),
            stream_state,
            recording_active: stream.status.recording_active,
        });
    }
    out
}

fn candidate_table_base(stream: &StreamSummary) -> String {
    if let Some(alias) = stream.manifest.identity.alias.as_deref()
        && !alias.trim().is_empty()
    {
        return sanitize_table_name(alias, "limelight");
    }
    if let Some(hardware_id) = stream.manifest.identity.hardware_id.as_deref()
        && !hardware_id.trim().is_empty()
    {
        return sanitize_table_name(hardware_id, "limelight");
    }
    sanitize_table_name(&stream.stream_id.to_string(), "limelight")
}

fn unique_table_name(base: &str, seen: &mut BTreeSet<String>) -> String {
    if seen.insert(base.to_string()) {
        return base.to_string();
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{base}-{suffix}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn sanitize_table_name(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in trimmed.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            previous_dash = false;
        } else if (c == '-' || c == '_' || c == ' ' || c == '.' || c == '/') && !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { fallback.to_string() } else { out }
}

fn reconcile_period() -> Duration {
    let ms = std::env::var("HELIOS_LIMELIGHT_RECONCILE_MS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(300).clamp(100, 5_000);
    Duration::from_millis(ms)
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn client_name_from_hostname(hostname: &str) -> String {
    let trimmed = hostname.trim();
    if trimmed.is_empty() {
        return "HeliOS".to_string();
    }
    trimmed.to_string()
}

async fn default_nt4_server_host_from_team_file() -> Option<String> {
    // TEMP_SHIM: nt4-limelight-team-file-etc-fallback
    // Keep the /etc team fallback until every deployed image writes the canonical team file into /var/lib/helios.
    let primary = std::env::var_os("HELIOS_TEAM_FILE").map(std::path::PathBuf::from).unwrap_or_else(|| "/var/lib/helios/team".into());
    let fallback = (primary.as_path() == std::path::Path::new("/var/lib/helios/team")).then_some(std::path::Path::new("/etc/helios/team"));
    let content = match tokio::fs::read_to_string(&primary).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => tokio::fs::read_to_string(fallback?).await.ok()?,
        Err(_) => return None,
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let team: u32 = trimmed.parse().ok()?;
    team_number_to_rio_ip(team).map(|ip| ip.to_string())
}

fn team_number_to_rio_ip(team: u32) -> Option<Ipv4Addr> {
    if team == 0 || team > 25_599 {
        return None;
    }
    let a = (team / 100) as u8;
    let b = (team % 100) as u8;
    Some(Ipv4Addr::new(10, a, b, 2))
}
