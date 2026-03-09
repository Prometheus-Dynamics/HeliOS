use crate::http::{AppState, storage};
use crate::ipc::IpcHandles;
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use lib_asyncapi::registry::SchemaRegistry;
use lib_asyncapi::{SchemaProvider, Server, Tag, TypeSchema, WsDoc};
use once_cell::sync::Lazy;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use std::time::Instant;
use sysinfo::{Disks, Networks, System};
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::{debug, warn};

const DEFAULT_INTERVAL_MS: u64 = 1_000;
const MIN_INTERVAL_MS: u64 = 250;
const MAX_INTERVAL_MS: u64 = 10_000;

static NET_SNAPSHOT: Lazy<std::sync::Mutex<Option<NetSnapshot>>> = Lazy::new(|| std::sync::Mutex::new(None));
static TELEMETRY_HUB: Lazy<TelemetryHub> = Lazy::new(TelemetryHub::new);

struct TelemetryHub {
    tx: broadcast::Sender<Arc<EncodedTelemetrySample>>,
    latest: Arc<std::sync::Mutex<Option<Arc<EncodedTelemetrySample>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: std::sync::Mutex<Option<Weak<IpcHandles>>>,
}

impl TelemetryHub {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx, latest: Arc::new(std::sync::Mutex::new(None)), task: Mutex::new(None), state: std::sync::Mutex::new(None) }
    }

    async fn subscribe(&self) -> (broadcast::Receiver<Arc<EncodedTelemetrySample>>, Option<Arc<EncodedTelemetrySample>>) {
        self.ensure_task().await;
        let latest = self.latest.lock().ok().and_then(|guard| guard.clone());
        (self.tx.subscribe(), latest)
    }

    fn set_state(&self, state: &AppState) {
        let mut guard = self.state.lock().unwrap();
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(std::sync::Arc::downgrade(state));
        }
    }

    async fn ensure_task(&self) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let latest = self.latest.clone();
            let state = self.state.lock().unwrap().clone();
            *guard = Some(tokio::spawn(run_telemetry_sampler(tx, latest, state)));
        }
    }
}

#[derive(Clone)]
struct EncodedTelemetrySample {
    payload: Arc<str>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryParams {
    pub interval_ms: Option<u64>,
}

pub async fn telemetry_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>, Query(params): Query<TelemetryParams>) -> impl IntoResponse {
    let interval_ms = params.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS).clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);
    TELEMETRY_HUB.set_state(&state);
    ws.on_upgrade(move |socket| telemetry_loop(socket, Duration::from_millis(interval_ms)))
}

async fn telemetry_loop(socket: WebSocket, interval: Duration) {
    let (mut tx, mut rx) = socket.split();
    let (mut sampler, initial) = TELEMETRY_HUB.subscribe().await;
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut latest = initial;
    let mut last_sent_at = Instant::now().checked_sub(interval).unwrap_or_else(Instant::now);

    if let Some(sample) = latest.as_ref() {
        if tx.send(Message::Text(sample.payload.as_ref().to_owned().into())).await.is_err() {
            return;
        }
        last_sent_at = Instant::now();
        latest = None;
    }

    loop {
        tokio::select! {
            recv = sampler.recv() => {
                match recv {
                    Ok(sample) => {
                        latest = Some(sample);
                        if last_sent_at.elapsed() >= interval
                            && let Some(sample) = latest.take()
                        {
                            if tx.send(Message::Text(sample.payload.as_ref().to_owned().into())).await.is_err() {
                                break;
                            }
                            last_sent_at = Instant::now();
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = ticker.tick() => {
                if let Some(sample) = latest.take() {
                    if tx.send(Message::Text(sample.payload.as_ref().to_owned().into())).await.is_err() {
                        break;
                    }
                    last_sent_at = Instant::now();
                }
            }
            Some(msg) = rx.next() => {
                if matches!(msg, Err(_) | Ok(Message::Close(_))) {
                    break;
                }
            }
            else => break,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CpuCoreSample {
    pub id: usize,
    pub usage_percent: f32,
    pub frequency_mhz: Option<u64>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GpuMemorySample {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NetworkInterfaceSample {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub mac: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NetworkSample {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    pub interfaces: Vec<NetworkInterfaceSample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TelemetrySample {
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineTelemetry>,
    pub cpu: CpuTelemetry,
    pub memory: MemoryTelemetry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<DiskTelemetry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<DiskPartitionTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<PowerTelemetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkSample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EngineTelemetry {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_disconnect_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CpuTelemetry {
    pub usage_percent: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<CpuThrottleStatus>,
    pub cores: Vec<CpuCoreSample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CpuThrottleStatus {
    pub raw: u32,
    pub undervoltage: bool,
    pub frequency_capped: bool,
    pub throttled: bool,
    pub soft_temp_limit: bool,
    pub undervoltage_since_boot: bool,
    pub frequency_capped_since_boot: bool,
    pub throttled_since_boot: bool,
    pub soft_temp_limit_since_boot: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MemoryTelemetry {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GpuTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_percent: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_mhz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<GpuMemorySample>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DiskTelemetry {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DiskPartitionTelemetry {
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PowerTelemetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watts: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volts: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amps: Option<f32>,
}

impl TelemetrySample {
    fn from_sys(sys: &System, networks: &Networks, disks: &Disks) -> Self {
        let cpu_usage = sys.global_cpu_usage();
        let cores =
            sys.cpus().iter().enumerate().map(|(idx, c)| CpuCoreSample { id: idx, usage_percent: c.cpu_usage(), frequency_mhz: Some(c.frequency()), label: Some(c.name().to_string()) }).collect();
        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();
        let partitions = collect_disk_partitions(disks);
        let data_root = storage::data_root_path();
        let (disk_total, disk_free) = select_disk_for_path(disks, &data_root).map(|disk| (disk.total_space(), disk.available_space())).unwrap_or_else(|| {
            let total = partitions.iter().map(|d| d.total_bytes).sum();
            let free = partitions.iter().map(|d| d.free_bytes).sum();
            (total, free)
        });
        let network = build_network_sample(networks);
        let gpu = sample_gpu();
        let cpu_temp_c = read_cpu_temperature_c();

        let throttle = read_cpu_throttle_status();

        TelemetrySample {
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            engine: None,
            cpu: CpuTelemetry { usage_percent: cpu_usage, temperature_c: cpu_temp_c, throttle, cores },
            memory: MemoryTelemetry { total_bytes: total_mem, used_bytes: used_mem, free_bytes: total_mem.saturating_sub(used_mem) },
            gpu,
            disk: Some(DiskTelemetry { total_bytes: disk_total, used_bytes: disk_total.saturating_sub(disk_free), free_bytes: disk_free }),
            disks: partitions,
            power: None,
            network,
        }
    }
}

fn sample_gpu() -> Option<GpuTelemetry> {
    let usage_percent = read_gpu_busy_percent();
    let temperature_c = read_gpu_temperature();
    let frequency_mhz = read_gpu_frequency_mhz();
    let memory = read_gpu_memory();

    if usage_percent.is_none() && temperature_c.is_none() && frequency_mhz.is_none() && memory.is_none() {
        return None;
    }

    Some(GpuTelemetry { usage_percent, temperature_c, frequency_mhz, memory })
}

fn collect_disk_partitions(disks: &Disks) -> Vec<DiskPartitionTelemetry> {
    disks
        .iter()
        .map(|disk| {
            let total = disk.total_space();
            let free = disk.available_space();
            DiskPartitionTelemetry { mount: disk.mount_point().to_string_lossy().to_string(), total_bytes: total, used_bytes: total.saturating_sub(free), free_bytes: free }
        })
        .collect()
}

fn select_disk_for_path<'a>(disks: &'a Disks, path: &Path) -> Option<&'a sysinfo::Disk> {
    let mut best: Option<&sysinfo::Disk> = None;
    let mut best_len = 0usize;
    for disk in disks.iter() {
        let mount = disk.mount_point();
        if path.starts_with(mount) {
            let len = mount.as_os_str().len();
            if len >= best_len {
                best = Some(disk);
                best_len = len;
            }
        }
    }
    best
}

struct TelemetrySysSampler {
    sys: System,
    networks: Networks,
    disks: Disks,
}

impl TelemetrySysSampler {
    fn new() -> Self {
        let mut sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();

        networks.refresh(false);
        update_net_snapshot(&networks);

        // Prime CPU stats so per-core usage isn't stuck at 0.
        sys.refresh_cpu_all();

        Self { sys, networks, disks }
    }

    fn sample(&mut self) -> TelemetrySample {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        self.disks.refresh(true);
        TelemetrySample::from_sys(&self.sys, &self.networks, &self.disks)
    }
}

async fn run_telemetry_sampler(tx: broadcast::Sender<Arc<EncodedTelemetrySample>>, latest: Arc<std::sync::Mutex<Option<Arc<EncodedTelemetrySample>>>>, state: Option<Weak<IpcHandles>>) {
    let sample_interval = std::env::var("HELIOS_TELEMETRY_SAMPLE_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .map(|d| d.clamp(Duration::from_millis(MIN_INTERVAL_MS), Duration::from_millis(MAX_INTERVAL_MS)))
        .unwrap_or_else(|| Duration::from_millis(DEFAULT_INTERVAL_MS));

    let last_power: std::sync::Arc<std::sync::Mutex<Option<PowerTelemetry>>> = std::sync::Arc::new(std::sync::Mutex::new(None));

    let power_state = state.clone();
    let power_task = {
        let tx = tx.clone();
        let last_power = last_power.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            let mut saw_receiver = tx.receiver_count() > 0;
            loop {
                ticker.tick().await;

                let receiver_count = tx.receiver_count();
                saw_receiver |= receiver_count > 0;
                if saw_receiver && receiver_count == 0 {
                    break;
                }

                if let Some(sampled) = sample_power_from_peripherals(power_state.as_ref()).await
                    && let Ok(mut guard) = last_power.lock()
                {
                    *guard = Some(sampled);
                }
            }
        })
    };

    let sys_task = {
        let tx = tx.clone();
        let last_power = last_power.clone();
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let mut sampler = TelemetrySysSampler::new();

            let soft_budget = (sample_interval / 5).max(Duration::from_millis(50));
            let hard_budget = sample_interval + Duration::from_millis(50);

            let mut last_warn: Option<Instant> = None;
            let mut suppressed_warns: u64 = 0;

            let mut saw_receiver = tx.receiver_count() > 0;
            let mut next_tick = Instant::now();

            loop {
                let receiver_count = tx.receiver_count();
                saw_receiver |= receiver_count > 0;
                if saw_receiver && receiver_count == 0 {
                    break;
                }

                let now = Instant::now();
                if now < next_tick {
                    std::thread::sleep(next_tick - now);
                }
                next_tick = Instant::now() + sample_interval;

                let sampling_started = Instant::now();
                let mut sample = sampler.sample();
                let sampling_elapsed = sampling_started.elapsed();

                if let Some(state) = state.as_ref().and_then(|weak| weak.upgrade()) {
                    sample.engine = Some(EngineTelemetry { connected: state.engine.is_connected(), last_disconnect_ms: state.engine.last_disconnect_ms() });
                }

                if let Ok(guard) = last_power.lock() {
                    sample.power = guard.clone();
                }

                if sampling_elapsed >= soft_budget {
                    let now = Instant::now();
                    let should_warn = last_warn.map(|t| now.duration_since(t) >= Duration::from_secs(10)).unwrap_or(true);
                    if should_warn {
                        if sampling_elapsed >= hard_budget {
                            warn!(
                                elapsed_ms = sampling_elapsed.as_millis(),
                                soft_budget_ms = soft_budget.as_millis(),
                                interval_ms = sample_interval.as_millis(),
                                suppressed = suppressed_warns,
                                "telemetry sampling exceeded budget"
                            );
                        } else {
                            debug!(
                                elapsed_ms = sampling_elapsed.as_millis(),
                                soft_budget_ms = soft_budget.as_millis(),
                                interval_ms = sample_interval.as_millis(),
                                suppressed = suppressed_warns,
                                "telemetry sampling exceeded soft budget"
                            );
                        }
                        last_warn = Some(now);
                        suppressed_warns = 0;
                    } else {
                        suppressed_warns = suppressed_warns.saturating_add(1);
                    }
                }

                let payload = Arc::<str>::from(serde_json::to_string(&sample).unwrap_or_default());
                let encoded = Arc::new(EncodedTelemetrySample { payload });
                if let Ok(mut guard) = latest.lock() {
                    *guard = Some(encoded.clone());
                }
                let _ = tx.send(encoded);
            }
        })
    };

    let _ = sys_task.await;
    power_task.abort();
}

fn read_gpu_busy_percent() -> Option<f32> {
    let drm_dir = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("card")).unwrap_or(false) {
            continue;
        }
        let device_dir = path.join("device");
        let candidates = vec![device_dir.join("gpu_busy_percent")];
        for candidate in candidates {
            if let Some(val) = read_percent_file(&candidate) {
                return Some(val);
            }
        }
        // Fallback: check engine busy files.
        if let Ok(engine_dirs) = fs::read_dir(device_dir.join("engine")) {
            for eng in engine_dirs.flatten() {
                let busy_path = eng.path().join("busy_percent");
                if let Some(val) = read_percent_file(&busy_path) {
                    return Some(val);
                }
            }
        }
    }
    read_gpu_busy_percent_from_stats()
}

#[derive(Debug, Clone, Copy)]
struct GpuStatsSnapshot {
    timestamp: u64,
    jobs: u64,
}

#[derive(Debug, Default, Clone, Copy)]
struct GpuBusyState {
    last: Option<GpuStatsSnapshot>,
}

fn read_gpu_busy_percent_from_stats() -> Option<f32> {
    static STATE: OnceLock<std::sync::Mutex<GpuBusyState>> = OnceLock::new();
    let snapshot = read_gpu_stats_snapshot()?;
    let state = STATE.get_or_init(|| std::sync::Mutex::new(GpuBusyState::default()));
    let mut guard = state.lock().ok()?;

    let percent = match guard.last {
        None => 0.0,
        Some(prev) => {
            let dt = snapshot.timestamp.saturating_sub(prev.timestamp);
            let dj = snapshot.jobs.saturating_sub(prev.jobs);
            // On CM5 (vc4/v3d), `gpu_stats` queue `runtime` appears to advance at wall-clock rate
            // even when idle (dt == dr), which would incorrectly report 100% busy.
            // Use job deltas as a pragmatic "busy vs idle" heuristic.
            if dt == 0 || dj == 0 { 0.0 } else { 100.0 }
        }
    };

    guard.last = Some(snapshot);
    Some(percent)
}

fn read_gpu_stats_snapshot() -> Option<GpuStatsSnapshot> {
    // Raspberry Pi (CM5) exposes per-queue runtime counters via `gpu_stats` on the V3D DRM node.
    // Compute utilization by sampling runtime deltas over timestamp deltas.
    let drm_dir = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let card_dir = entry.path();
        let file_name = card_dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !file_name.starts_with("card") {
            continue;
        }
        let stats_path = card_dir.join("device/gpu_stats");
        if let Some(snapshot) = parse_gpu_stats_file(&stats_path) {
            return Some(snapshot);
        }
    }
    None
}

fn parse_gpu_stats_file(path: &Path) -> Option<GpuStatsSnapshot> {
    let raw = fs::read_to_string(path).ok()?;
    let mut timestamp: Option<u64> = None;
    let mut jobs_sum: u64 = 0;
    for (idx, line) in raw.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let mut parts = line.split_whitespace();
        let queue = parts.next()?;
        let ts = parts.next()?.parse::<u64>().ok()?;
        let jobs = parts.next()?.parse::<u64>().ok()?;
        let _runtime = parts.next()?.parse::<u64>().ok()?;

        // These are V3D hardware queues; summing and clamping provides a practical overall busy %.
        if matches!(queue, "bin" | "render" | "tfu" | "csd") {
            jobs_sum = jobs_sum.saturating_add(jobs);
        }
        timestamp = Some(timestamp.map_or(ts, |prev| prev.max(ts)));
    }
    Some(GpuStatsSnapshot { timestamp: timestamp?, jobs: jobs_sum })
}

fn read_gpu_temperature() -> Option<f32> {
    let drm_dir = Path::new("/sys/class/drm");
    for entry in fs::read_dir(drm_dir).ok()?.flatten() {
        let card_dir = entry.path();
        if !card_dir.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("card")).unwrap_or(false) {
            continue;
        }
        let hwmon_glob = card_dir.join("device/hwmon");
        if let Ok(hwmon_dirs) = fs::read_dir(hwmon_glob) {
            for hw in hwmon_dirs.flatten() {
                let temp_path = hw.path().join("temp1_input");
                if let Some(raw) = read_integer_file(&temp_path) {
                    return Some(raw as f32 / 1000.0);
                }
            }
        }
    }
    None
}

fn read_gpu_frequency_mhz() -> Option<u64> {
    read_gpu_frequency_debugfs_mhz().or_else(read_gpu_frequency_sysfs_mhz)
}

fn read_gpu_frequency_sysfs_mhz() -> Option<u64> {
    let devfreq_root = Path::new("/sys/class/devfreq");
    let entries = fs::read_dir(devfreq_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let raw = fs::read_to_string(path.join("cur_freq")).ok()?;
        let hz = raw.trim().parse::<u64>().ok()?;
        if hz > 0 {
            return Some(hz / 1_000_000);
        }
    }
    None
}

fn read_gpu_frequency_debugfs_mhz() -> Option<u64> {
    let root = Path::new("/sys/kernel/debug/dri");
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("measure_clock");
        if !path.exists() {
            continue;
        }
        if let Some(mhz) = parse_debugfs_measure_clock_mhz(&path) {
            return Some(mhz);
        }
    }
    None
}

fn parse_debugfs_measure_clock_mhz(path: &Path) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let lower = raw.to_ascii_lowercase();
    let start = lower.find('(')?;
    let mhz_pos = lower[start..].find("mhz")? + start;
    let inner = raw.get(start + 1..mhz_pos)?.trim();
    let number = inner.split_whitespace().next()?;
    let parsed = number.parse::<f64>().ok()?;
    Some(parsed.round() as u64)
}

fn read_gpu_memory() -> Option<GpuMemorySample> {
    let drm_dir = Path::new("/sys/class/drm");
    for entry in fs::read_dir(drm_dir).ok()?.flatten() {
        let device_dir = entry.path().join("device");
        if !device_dir.exists() {
            continue;
        }
        let used_path = device_dir.join("mem_info_vram_used");
        let total_path = device_dir.join("mem_info_vram_total");
        let used = read_integer_file(&used_path);
        let total = read_integer_file(&total_path);
        if used.is_none() && total.is_none() {
            continue;
        }
        let total_bytes = total.unwrap_or(0);
        let used_bytes = used.unwrap_or(0);
        let free_bytes = total_bytes.saturating_sub(used_bytes);
        return Some(GpuMemorySample { total_bytes, used_bytes, free_bytes });
    }
    read_gpu_memory_from_cma()
}

fn read_gpu_memory_from_cma() -> Option<GpuMemorySample> {
    // On Raspberry Pi / CM5 the V3D driver typically allocates from CMA; expose it as "GPU memory".
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let mut cma_total_kb: Option<u64> = None;
    let mut cma_free_kb: Option<u64> = None;
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("CmaTotal:") {
            cma_total_kb = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("CmaFree:") {
            cma_free_kb = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
        }
        if cma_total_kb.is_some() && cma_free_kb.is_some() {
            break;
        }
    }
    let total_kb = cma_total_kb?;
    let free_kb = cma_free_kb?;
    let total_bytes = total_kb.saturating_mul(1024);
    let free_bytes = free_kb.saturating_mul(1024);
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    Some(GpuMemorySample { total_bytes, used_bytes, free_bytes })
}

fn read_cpu_temperature_c() -> Option<f32> {
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("thermal_zone")).unwrap_or(false) {
                continue;
            }
            let zone_type = fs::read_to_string(path.join("type")).ok().unwrap_or_default().to_ascii_lowercase();
            if !(zone_type.contains("cpu") || zone_type.contains("soc")) {
                continue;
            }
            let raw = fs::read_to_string(path.join("temp")).ok()?;
            let milli = raw.trim().parse::<i64>().ok()?;
            if milli <= 0 {
                continue;
            }
            return Some(milli as f32 / 1000.0);
        }
    }
    None
}

fn read_cpu_throttle_status() -> Option<CpuThrottleStatus> {
    let raw = fs::read_to_string("/sys/devices/platform/soc/firmware/get_throttled").ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let value = u32::from_str_radix(trimmed, 16).ok()?;
    Some(CpuThrottleStatus {
        raw: value,
        undervoltage: (value & 0x1) != 0,
        frequency_capped: (value & (1 << 1)) != 0,
        throttled: (value & (1 << 2)) != 0,
        soft_temp_limit: (value & (1 << 3)) != 0,
        undervoltage_since_boot: (value & (1 << 16)) != 0,
        frequency_capped_since_boot: (value & (1 << 17)) != 0,
        throttled_since_boot: (value & (1 << 18)) != 0,
        soft_temp_limit_since_boot: (value & (1 << 19)) != 0,
    })
}

async fn sample_power_from_peripherals(state: Option<&Weak<IpcHandles>>) -> Option<PowerTelemetry> {
    use crate::http::device::power::power_status_from_snapshot;
    use helios_peripherals::dto::SensorScope;

    let handles = state?.upgrade()?;
    let sensors = handles.ensure_sensors().await?;
    let response = tokio::time::timeout(Duration::from_millis(250), sensors.sensor_snapshot(SensorScope::Device)).await.ok()?;
    let response = response.ok()?;
    let snapshot = response.ok()?;
    let status = power_status_from_snapshot(&snapshot);

    Some(PowerTelemetry { watts: status.watts.map(|v| v as f32), volts: status.volts.map(|v| v as f32), amps: status.amps.map(|v| v as f32) })
}

fn read_percent_file(path: &PathBuf) -> Option<f32> {
    read_integer_file(path).map(|v| v as f32)
}

fn read_integer_file(path: &PathBuf) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

fn schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schema_for!(T)).expect("schema")
}

impl SchemaProvider for TelemetrySample {
    const NAME: &'static str = "TelemetrySample";
    fn schema() -> serde_json::Value {
        schema::<TelemetrySample>()
    }
    fn register_schemas(map: &mut BTreeMap<String, serde_json::Value>) {
        map.entry(Self::NAME.to_string()).or_insert_with(Self::schema);
    }
}

pub fn register_docs(host: Option<String>, registry: &mut SchemaRegistry, servers: &mut BTreeMap<String, Server>, tags: &mut Vec<Tag>, docs: &mut Vec<WsDoc>) {
    registry.track::<TelemetrySample, _>(TelemetrySample::register_schemas);

    let doc = WsDoc {
        path: "device.telemetry",
        summary: "Device telemetry",
        description: "Realtime CPU/memory/disk telemetry for the device.",
        tags: vec!["device".into(), "telemetry".into()],
        payload: None,
        responses: vec![TypeSchema { name: TelemetrySample::NAME, schema: TelemetrySample::schema() }],
        params: vec![],
    };

    let server_host = host.unwrap_or_else(|| "localhost:5800/v1/ws".to_string());
    servers.entry("primary".into()).or_insert(Server { host: server_host, protocol: "ws".into(), protocol_version: None, description: Some("Primary WebSocket entrypoint".into()) });
    docs.push(doc);
    tags.push(Tag { name: "device".into(), description: Some("Device telemetry".to_string()), external_docs: None });
}

#[derive(Clone)]
struct NetSnapshot {
    at: Instant,
    totals: BTreeMap<String, (u64, u64)>,
}

fn update_net_snapshot(networks: &Networks) {
    let mut guard = NET_SNAPSHOT.lock().unwrap();
    let mut totals = BTreeMap::new();
    for (name, data) in networks.iter() {
        totals.insert(name.to_string(), (data.total_received(), data.total_transmitted()));
    }
    *guard = Some(NetSnapshot { at: Instant::now(), totals });
}

fn build_network_sample(networks: &Networks) -> Option<NetworkSample> {
    let prev = NET_SNAPSHOT.lock().unwrap().clone();
    let mut totals = BTreeMap::new();
    let mut interfaces = Vec::new();
    let snapshot_time = prev.as_ref().map(|snap| snap.at);

    for (name, data) in networks.iter() {
        let rx = data.total_received();
        let tx = data.total_transmitted();
        totals.insert(name.to_string(), (rx, tx));
        let prev_vals = prev.as_ref().and_then(|snap| snap.totals.get(name)).copied();
        let (delta_rx, delta_tx, per_sec_rx, per_sec_tx) = compute_net_deltas(prev_vals, rx, tx, snapshot_time);
        interfaces.push(NetworkInterfaceSample {
            name: name.to_string(),
            rx_bytes: delta_rx,
            tx_bytes: delta_tx,
            total_rx_bytes: rx,
            total_tx_bytes: tx,
            rx_bytes_per_sec: per_sec_rx,
            tx_bytes_per_sec: per_sec_tx,
            mac: None,
        });
    }

    let total_rx = totals.values().map(|(rx, _)| *rx).sum();
    let total_tx = totals.values().map(|(_, tx)| *tx).sum();
    let prev_totals = prev.as_ref().map(|snap| {
        let rx_sum: u64 = snap.totals.values().map(|(rx, _)| *rx).sum();
        let tx_sum: u64 = snap.totals.values().map(|(_, tx)| *tx).sum();
        (rx_sum, tx_sum)
    });
    let (delta_rx, delta_tx, per_sec_rx, per_sec_tx) = compute_net_deltas(prev_totals, total_rx, total_tx, snapshot_time);

    // Update snapshot for next tick.
    update_net_snapshot(networks);

    if total_rx == 0 && total_tx == 0 && interfaces.is_empty() {
        return None;
    }

    Some(NetworkSample { rx_bytes: delta_rx, tx_bytes: delta_tx, total_rx_bytes: total_rx, total_tx_bytes: total_tx, rx_bytes_per_sec: per_sec_rx, tx_bytes_per_sec: per_sec_tx, interfaces })
}

fn compute_net_deltas(prev: Option<(u64, u64)>, rx: u64, tx: u64, snapshot_time: Option<Instant>) -> (u64, u64, f64, f64) {
    if let (Some((prev_rx, prev_tx)), Some(at)) = (prev, snapshot_time) {
        let elapsed = at.elapsed().as_secs_f64().max(0.001);
        let delta_rx = rx.saturating_sub(prev_rx);
        let delta_tx = tx.saturating_sub(prev_tx);
        let per_sec_rx = delta_rx as f64 / elapsed;
        let per_sec_tx = delta_tx as f64 / elapsed;
        (delta_rx, delta_tx, per_sec_rx, per_sec_tx)
    } else {
        (0, 0, 0.0, 0.0)
    }
}
