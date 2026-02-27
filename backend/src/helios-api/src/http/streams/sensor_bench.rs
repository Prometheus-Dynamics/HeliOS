use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use helios_engine::{
    capture::{BackendHandle, BackendKind, ControlAssignment},
    ipc::StreamManifest,
    stream::read_latest_header,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use styx::prelude::FourCc;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::{
    sync::{Mutex, watch},
    time::{Instant, sleep},
};
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::{AppState, storage};

use super::bench::BenchCodecStat;
use super::bench_util::{
    StartStreamArgs, codec_impl_cache, discover_devices, find_matching_backend, identity_for_keys, restore_streams_best_effort, start_stream_for_mode, stop_conflicting_streams,
    stop_stream_best_effort,
};

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct StartSensorBenchmarkRequest {
    pub backend: BackendKind,
    pub handle: BackendHandle,
    #[serde(default)]
    pub device_keys: Vec<String>,
    #[serde(default = "default_target_fps")]
    pub target_fps: u32,
    #[serde(default = "default_sample_ms")]
    pub sample_ms: u64,
    /// Benchmark only these mode IDs (defaults to all modes for the backend).
    #[serde(default)]
    pub mode_ids: Vec<styx::capture::ModeId>,
    /// Number of frames to wait per sampling window (per mode + per codec).
    ///
    /// If unset, `sample_ms` determines the sampling window.
    #[serde(default)]
    pub sample_frames: Option<u32>,
    /// Max time to wait per sampling window (per mode + per codec), in milliseconds.
    ///
    /// If unset, `sample_ms` determines the sampling window.
    #[serde(default)]
    pub sample_timeout_ms: Option<u64>,
    /// Stop conflicting streams while benchmarking, then restore them.
    #[serde(default = "default_restore_existing")]
    pub restore_existing: bool,
    /// Capture controls to apply during benchmark runs (e.g., exposure).
    #[serde(default)]
    pub controls: Vec<ControlAssignment>,
}

fn default_target_fps() -> u32 {
    120
}

fn default_sample_ms() -> u64 {
    1500
}

fn default_restore_existing() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorBenchmarkStarted {
    pub benchmark_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SensorBenchmarkProgress {
    pub total_modes: usize,
    pub completed_modes: usize,
    #[serde(default)]
    pub current_format: Option<String>,
    #[serde(default)]
    pub current_resolution: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SensorBenchmarkStatus {
    Running { progress: SensorBenchmarkProgress, started_at: DateTime<Utc> },
    Completed { summary: SensorBenchmarkSummary, result: SensorBenchmarkResult },
    Failed { error: String, started_at: DateTime<Utc> },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SensorBenchmarkSummary {
    pub benchmark_id: Uuid,
    pub backend: BackendKind,
    pub device_keys: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    #[serde(default)]
    pub canceled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct CpuSample {
    /// Average `helios-engine` CPU usage for this run (sysinfo `%`, can exceed 100 on multi-core).
    #[serde(default)]
    pub engine_cpu_avg: Option<f32>,
    /// Average total CPU usage for this run (sysinfo `%`).
    #[serde(default)]
    pub system_cpu_avg: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BenchCodecStatCpu {
    #[serde(flatten)]
    pub stat: BenchCodecStat,
    #[serde(default)]
    pub cpu: CpuSample,
    #[serde(default)]
    pub cpu_delta: CpuSample,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SensorBenchModeResult {
    pub format: String,
    pub resolution: String,
    pub mode_id: styx::capture::ModeId,
    pub capture_avg_fps: f64,
    pub host_avg_fps: f64,
    #[serde(default)]
    pub baseline_cpu: CpuSample,
    #[serde(default)]
    pub decoders: Vec<BenchCodecStatCpu>,
    #[serde(default)]
    pub encoders: Vec<BenchCodecStatCpu>,
    /// For encoder benchmarks we pick a single decoder (if available) to provide RG24.
    #[serde(default)]
    pub encoder_input_decoder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SensorBenchmarkResult {
    pub summary: SensorBenchmarkSummary,
    #[serde(default)]
    pub modes: Vec<SensorBenchModeResult>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone)]
struct JobState {
    started_at: DateTime<Utc>,
    progress: SensorBenchmarkProgress,
    status: SensorBenchmarkInternalStatus,
    cancel: Arc<AtomicBool>,
    active_stream_id: Option<Uuid>,
}

#[derive(Clone)]
enum SensorBenchmarkInternalStatus {
    Running,
    Completed(PathBuf),
    Failed(String),
}

fn jobs() -> &'static Arc<Mutex<HashMap<Uuid, JobState>>> {
    static JOBS: OnceLock<Arc<Mutex<HashMap<Uuid, JobState>>>> = OnceLock::new();
    JOBS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn bench_dir_name() -> &'static str {
    "sensor-benchmarks"
}

async fn bench_file_path(benchmark_id: Uuid) -> Result<PathBuf, std::io::Error> {
    let dir = storage::ensure_subdir_async(bench_dir_name()).await?;
    Ok(dir.join(format!("{benchmark_id}.json")))
}

fn format_resolution_label(mode: &styx::capture::Mode) -> String {
    let w = mode.format.resolution.width.get();
    let h = mode.format.resolution.height.get();
    format!("{w}x{h}")
}

fn find_helios_engine_pid(system: &System) -> Option<Pid> {
    system.processes().iter().find_map(|(pid, proc_)| {
        let name = proc_.name().to_string_lossy();
        if name.contains("helios-engine") {
            return Some(*pid);
        }
        let exe = proc_.exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        if exe.contains("helios-engine") {
            return Some(*pid);
        }
        None
    })
}

struct CpuSampler {
    system: System,
    engine_pid: Option<Pid>,
}

impl CpuSampler {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_processes(ProcessesToUpdate::All, true);
        system.refresh_cpu_all();
        let engine_pid = find_helios_engine_pid(&system);
        if engine_pid.is_none() {
            warn!("unable to find helios-engine pid for cpu benchmark sampling");
        }
        Self { system, engine_pid }
    }

    fn sample_once(&mut self, engine_cpu_sum: &mut f32, system_cpu_sum: &mut f32, samples: &mut u32) {
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        self.system.refresh_cpu_all();
        *system_cpu_sum += self.system.global_cpu_usage();
        if let Some(pid) = self.engine_pid
            && let Some(p) = self.system.process(pid)
        {
            *engine_cpu_sum += p.cpu_usage();
        }
        *samples += 1;
    }

    fn finish(&self, engine_cpu_sum: f32, system_cpu_sum: f32, samples: u32) -> CpuSample {
        if samples == 0 {
            return CpuSample { engine_cpu_avg: None, system_cpu_avg: None };
        }
        CpuSample { engine_cpu_avg: self.engine_pid.map(|_| engine_cpu_sum / samples as f32), system_cpu_avg: Some(system_cpu_sum / samples as f32) }
    }
}

#[derive(Clone, Copy)]
struct SampleWindow {
    frames: u64,
    timeout: Duration,
}

fn sample_window_from_req(req: &StartSensorBenchmarkRequest) -> SampleWindow {
    match (req.sample_frames, req.sample_timeout_ms) {
        (Some(frames), Some(timeout_ms)) => SampleWindow { frames: frames.max(1) as u64, timeout: Duration::from_millis(timeout_ms.max(100)) },
        _ => SampleWindow { frames: 0, timeout: Duration::from_millis(req.sample_ms.max(100)) },
    }
}

async fn touch_preview_periodically(stream_id: Uuid, mut stop: watch::Receiver<bool>) {
    let period = Duration::from_millis(200);
    loop {
        if *stop.borrow() {
            break;
        }
        let _ = tokio::task::spawn_blocking(move || helios_engine::stream::touch_stream_preview(stream_id)).await;
        tokio::select! {
            _ = sleep(period) => {}
            _ = stop.changed() => {}
        }
    }
}

async fn read_shmem_seq(stream_id: Uuid) -> Option<u64> {
    tokio::task::spawn_blocking(move || read_latest_header(stream_id).ok().map(|h| h.seq)).await.ok().flatten()
}

fn cpu_delta(baseline: &CpuSample, current: &CpuSample) -> CpuSample {
    CpuSample {
        engine_cpu_avg: match (baseline.engine_cpu_avg, current.engine_cpu_avg) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        },
        system_cpu_avg: match (baseline.system_cpu_avg, current.system_cpu_avg) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        },
    }
}

async fn sample_metrics_and_cpu(
    state: &AppState,
    stream_id: Uuid,
    window: SampleWindow,
    touch_preview: bool,
    cpu: &mut CpuSampler,
) -> Result<(helios_engine::stream::StreamMetrics, CpuSample), String> {
    let (stop_tx, stop_rx) = watch::channel(false);
    let touch_join = touch_preview.then(|| tokio::spawn(touch_preview_periodically(stream_id, stop_rx)));

    let mut samples: u32 = 0;
    let mut engine_cpu_sum: f32 = 0.0;
    let mut system_cpu_sum: f32 = 0.0;

    // Prime CPU usage so readings aren't stuck at 0 on first sample.
    sleep(Duration::from_millis(200)).await;
    cpu.sample_once(&mut engine_cpu_sum, &mut system_cpu_sum, &mut samples);

    let started = Instant::now();
    let start_seq = if window.frames == 0 { 0 } else { read_shmem_seq(stream_id).await.unwrap_or(0) };
    let mut next_cpu = Instant::now() + Duration::from_millis(200);

    loop {
        if started.elapsed() >= window.timeout {
            break;
        }

        if Instant::now() >= next_cpu {
            cpu.sample_once(&mut engine_cpu_sum, &mut system_cpu_sum, &mut samples);
            next_cpu += Duration::from_millis(200);
        }

        if window.frames != 0 {
            let current_seq = read_shmem_seq(stream_id).await.unwrap_or(start_seq);
            if current_seq.saturating_sub(start_seq) >= window.frames {
                break;
            }
        }

        sleep(Duration::from_millis(10)).await;
    }

    let _ = stop_tx.send(true);
    if let Some(join) = touch_join {
        let _ = join.await;
    }

    let cpu_sample = cpu.finish(engine_cpu_sum, system_cpu_sum, samples);

    match state.engine.get_metrics(stream_id).await {
        Ok(helios_engine::ipc::EngineEvent::Metrics { metrics, .. }) => Ok((metrics, cpu_sample)),
        Ok(other) => {
            warn!(?other, stream_id = %stream_id, "unexpected engine event for get_metrics");
            Err("unexpected metrics response".into())
        }
        Err(err) => Err(err.to_string()),
    }
}

async fn write_benchmark_file(path: &PathBuf, result: &SensorBenchmarkResult) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(result).map_err(|e| e.to_string())?;
    tokio::fs::write(path, bytes).await.map_err(|e| e.to_string())
}

async fn read_benchmark_file(path: &PathBuf) -> Result<SensorBenchmarkResult, String> {
    let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn update_job(id: Uuid, update: impl FnOnce(&mut JobState)) {
    let mut guard = jobs().lock().await;
    if let Some(job) = guard.get_mut(&id) {
        update(job);
    }
}

pub(crate) async fn is_active_benchmark_stream(stream_id: Uuid) -> bool {
    let guard = jobs().lock().await;
    guard.values().any(|job| matches!(job.status, SensorBenchmarkInternalStatus::Running) && job.active_stream_id == Some(stream_id))
}

async fn run_benchmark_job(state: AppState, benchmark_id: Uuid, req: StartSensorBenchmarkRequest) {
    let started_at = Utc::now();
    let mut warnings: Vec<String> = Vec::new();
    let window = sample_window_from_req(&req);
    let mut cpu_sampler = CpuSampler::new();
    let cancel = { jobs().lock().await.get(&benchmark_id).map(|j| j.cancel.clone()) };
    let Some(cancel) = cancel else {
        return;
    };

    let devices = discover_devices().await;
    let Some((device, backend)) = find_matching_backend(&devices, req.backend, &req.handle, &req.device_keys) else {
        update_job(benchmark_id, |job| job.status = SensorBenchmarkInternalStatus::Failed("device/backend not found".into())).await;
        return;
    };
    let keys = if req.device_keys.is_empty() { device.identity.keys.clone() } else { req.device_keys.clone() };
    let identity = identity_for_keys(&keys);

    let descriptor_modes = backend.descriptor.modes.clone();
    let selected_modes: Vec<styx::capture::Mode> =
        if req.mode_ids.is_empty() { descriptor_modes.clone() } else { descriptor_modes.iter().filter(|mode| req.mode_ids.iter().any(|want| want == &mode.id)).cloned().collect() };
    update_job(benchmark_id, |job| job.progress.total_modes = selected_modes.len()).await;

    let mut restored: Vec<StreamManifest> = Vec::new();
    if req.restore_existing {
        restored = stop_conflicting_streams(&state, &keys).await;
    }

    let codec_cache = codec_impl_cache();
    let mut mode_results: Vec<SensorBenchModeResult> = Vec::new();

    for (idx, mode) in selected_modes.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            warnings.push("benchmark canceled".into());
            break;
        }
        let fourcc: FourCc = mode.format.code;
        let format = fourcc.to_string();
        let resolution = format_resolution_label(mode);

        update_job(benchmark_id, |job| {
            job.progress.completed_modes = idx;
            job.progress.current_format = Some(format.clone());
            job.progress.current_resolution = Some(resolution.clone());
        })
        .await;

        let stream_id = match start_stream_for_mode(StartStreamArgs {
            state: &state,
            identity: &identity,
            keys: &keys,
            backend: req.backend,
            handle: req.handle.clone(),
            mode_id: &mode.id,
            target_fps: req.target_fps,
            controls: req.controls.clone(),
        })
        .await
        {
            Ok(id) => id,
            Err(err) => {
                warnings.push(format!("{format} {resolution}: start failed: {err}"));
                continue;
            }
        };
        update_job(benchmark_id, |job| job.active_stream_id = Some(stream_id)).await;

        // Baseline metrics and baseline CPU (capture-only).
        let (baseline_metrics, baseline_cpu) = match sample_metrics_and_cpu(&state, stream_id, window, false, &mut cpu_sampler).await {
            Ok(pair) => pair,
            Err(err) => {
                warnings.push(format!("{format} {resolution}: baseline metrics failed: {err}"));
                stop_stream_best_effort(&state, stream_id).await;
                continue;
            }
        };

        let decoder_impls = codec_cache.decoders_by_input.get(&fourcc).cloned().unwrap_or_default();
        let mut decoder_stats: Vec<BenchCodecStatCpu> = Vec::new();

        for impl_name in decoder_impls.iter() {
            if cancel.load(Ordering::Relaxed) {
                warnings.push("benchmark canceled".into());
                break;
            }
            if let Err(err) = state.engine.set_codecs(stream_id, Some(impl_name.clone()), None).await {
                warnings.push(format!("{format} {resolution}: decoder {impl_name} set failed: {err}"));
                continue;
            }
            match sample_metrics_and_cpu(&state, stream_id, window, true, &mut cpu_sampler).await {
                Ok((metrics, cpu)) => {
                    if let Some(dec) = metrics.decoder.as_ref() {
                        decoder_stats.push(BenchCodecStatCpu {
                            stat: BenchCodecStat { implementation: impl_name.clone(), avg_ms: dec.work_average_time_ms, avg_fps: dec.fps, errors: dec.errors },
                            cpu_delta: cpu_delta(&baseline_cpu, &cpu),
                            cpu,
                        });
                    }
                }
                Err(err) => warnings.push(format!("{format} {resolution}: decoder {impl_name} metrics failed: {err}")),
            };
        }

        decoder_stats.sort_by(|a, b| b.stat.avg_fps.total_cmp(&a.stat.avg_fps));
        let encoder_input_decoder = decoder_stats.first().map(|d| d.stat.implementation.clone());

        // Reset decoder/encoder selection before encoder runs.
        let _ = state.engine.set_codecs(stream_id, None, None).await;

        let mut encoder_stats: Vec<BenchCodecStatCpu> = Vec::new();
        if let Some(decoder_for_encoder) = encoder_input_decoder.clone() {
            if let Err(err) = state.engine.set_codecs(stream_id, Some(decoder_for_encoder.clone()), None).await {
                warnings.push(format!("{format} {resolution}: encoder input decoder {decoder_for_encoder} set failed: {err}"));
            } else {
                for impl_name in &codec_cache.encoders_rg24 {
                    if cancel.load(Ordering::Relaxed) {
                        warnings.push("benchmark canceled".into());
                        break;
                    }
                    if let Err(err) = state.engine.set_codecs(stream_id, Some(decoder_for_encoder.clone()), Some(impl_name.clone())).await {
                        warnings.push(format!("{format} {resolution}: encoder {impl_name} set failed: {err}"));
                        continue;
                    }
                    match sample_metrics_and_cpu(&state, stream_id, window, true, &mut cpu_sampler).await {
                        Ok((metrics, cpu)) => {
                            if let Some(enc) = metrics.encoder.as_ref() {
                                let delta = cpu_delta(&baseline_cpu, &cpu);
                                encoder_stats.push(BenchCodecStatCpu {
                                    stat: BenchCodecStat { implementation: impl_name.clone(), avg_ms: enc.work_average_time_ms, avg_fps: enc.fps, errors: enc.errors },
                                    cpu,
                                    cpu_delta: delta,
                                });
                            }
                        }
                        Err(err) => warnings.push(format!("{format} {resolution}: encoder {impl_name} metrics failed: {err}")),
                    };
                }
            }
        }

        encoder_stats.sort_by(|a, b| b.stat.avg_fps.total_cmp(&a.stat.avg_fps));

        mode_results.push(SensorBenchModeResult {
            format,
            resolution,
            mode_id: mode.id.clone(),
            capture_avg_fps: baseline_metrics.capture.fps,
            host_avg_fps: baseline_metrics.host.fps,
            baseline_cpu,
            decoders: decoder_stats,
            encoders: encoder_stats,
            encoder_input_decoder,
        });

        stop_stream_best_effort(&state, stream_id).await;
        update_job(benchmark_id, |job| job.active_stream_id = None).await;
    }

    update_job(benchmark_id, |job| {
        job.progress.completed_modes = job.progress.total_modes;
        job.progress.current_format = None;
        job.progress.current_resolution = None;
        job.active_stream_id = None;
    })
    .await;

    if req.restore_existing {
        restore_streams_best_effort(&state, restored, &mut warnings).await;
    }

    let completed_at = Utc::now();
    let canceled = cancel.load(Ordering::Relaxed);
    let summary = SensorBenchmarkSummary { benchmark_id, backend: req.backend, device_keys: keys, started_at, completed_at, canceled };
    let result = SensorBenchmarkResult { summary: summary.clone(), modes: mode_results, warnings };

    match bench_file_path(benchmark_id).await {
        Ok(path) => {
            if let Err(err) = write_benchmark_file(&path, &result).await {
                update_job(benchmark_id, |job| job.status = SensorBenchmarkInternalStatus::Failed(err)).await;
                return;
            }
            update_job(benchmark_id, |job| job.status = SensorBenchmarkInternalStatus::Completed(path)).await;
            // Persisted on disk; drop in-memory state to avoid unbounded growth.
            jobs().lock().await.remove(&benchmark_id);
        }
        Err(err) => {
            update_job(benchmark_id, |job| job.status = SensorBenchmarkInternalStatus::Failed(err.to_string())).await;
        }
    };
}

#[utoipa::path(
    post,
    path = "/streams/bench/sensor",
    tag = "EngineStreams",
    request_body = StartSensorBenchmarkRequest,
    responses((status = 202, description = "Benchmark started", body = SensorBenchmarkStarted))
)]
pub async fn start_sensor_benchmark(State(state): State<AppState>, Json(req): Json<StartSensorBenchmarkRequest>) -> Response {
    let benchmark_id = Uuid::new_v4();
    let started_at = Utc::now();

    let job = JobState {
        started_at,
        progress: SensorBenchmarkProgress { total_modes: 0, completed_modes: 0, current_format: None, current_resolution: None },
        status: SensorBenchmarkInternalStatus::Running,
        cancel: Arc::new(AtomicBool::new(false)),
        active_stream_id: None,
    };
    jobs().lock().await.insert(benchmark_id, job);

    tokio::spawn(run_benchmark_job(state, benchmark_id, req));

    (StatusCode::ACCEPTED, Json(SensorBenchmarkStarted { benchmark_id })).into_response()
}

#[utoipa::path(
    get,
    path = "/streams/bench/sensor/{id}",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Benchmark ID")),
    responses(
        (status = 200, description = "Benchmark status/result", body = SensorBenchmarkStatus),
        (status = 404, description = "Unknown benchmark")
    )
)]
pub async fn get_sensor_benchmark(State(_state): State<AppState>, axum::extract::Path(id): axum::extract::Path<Uuid>) -> Response {
    let job_opt = { jobs().lock().await.get(&id).cloned() };

    if let Some(job) = job_opt {
        match job.status {
            SensorBenchmarkInternalStatus::Running => {
                return (StatusCode::OK, Json(SensorBenchmarkStatus::Running { progress: job.progress, started_at: job.started_at })).into_response();
            }
            SensorBenchmarkInternalStatus::Failed(error) => {
                return (StatusCode::OK, Json(SensorBenchmarkStatus::Failed { error, started_at: job.started_at })).into_response();
            }
            SensorBenchmarkInternalStatus::Completed(path) => match read_benchmark_file(&path).await {
                Ok(result) => {
                    return (StatusCode::OK, Json(SensorBenchmarkStatus::Completed { summary: result.summary.clone(), result })).into_response();
                }
                Err(err) => {
                    return (StatusCode::OK, Json(SensorBenchmarkStatus::Failed { error: err, started_at: job.started_at })).into_response();
                }
            },
        }
    }

    // No in-memory job; try to load persisted file.
    match bench_file_path(id).await {
        Ok(path) if path.exists() => match read_benchmark_file(&path).await {
            Ok(result) => (StatusCode::OK, Json(SensorBenchmarkStatus::Completed { summary: result.summary.clone(), result })).into_response(),
            Err(err) => (StatusCode::OK, Json(SensorBenchmarkStatus::Failed { error: err, started_at: Utc::now() })).into_response(),
        },
        _ => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "benchmark not found" }))).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/streams/bench/sensor/{id}/cancel",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Benchmark ID")),
    responses(
        (status = 202, description = "Cancellation requested"),
        (status = 404, description = "Unknown benchmark")
    )
)]
pub async fn cancel_sensor_benchmark(State(_state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    let mut guard = jobs().lock().await;
    let Some(job) = guard.get_mut(&id) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "benchmark not found" }))).into_response();
    };
    job.cancel.store(true, Ordering::Relaxed);
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "ok": true }))).into_response()
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorBenchmarkListItem {
    pub summary: SensorBenchmarkSummary,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorBenchmarkListResponse {
    #[serde(default)]
    pub benchmarks: Vec<SensorBenchmarkListItem>,
}

#[utoipa::path(
    get,
    path = "/streams/bench/sensor",
    tag = "EngineStreams",
    responses((status = 200, description = "List persisted sensor benchmarks", body = SensorBenchmarkListResponse))
)]
pub async fn list_sensor_benchmarks(State(_state): State<AppState>) -> Response {
    let dir = match storage::ensure_subdir_async(bench_dir_name()).await {
        Ok(dir) => dir,
        Err(_) => return (StatusCode::OK, Json(SensorBenchmarkListResponse { benchmarks: vec![] })).into_response(),
    };

    let mut benchmarks: Vec<SensorBenchmarkListItem> = Vec::new();
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return (StatusCode::OK, Json(SensorBenchmarkListResponse { benchmarks: vec![] })).into_response(),
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(_) => continue,
        };
        let parsed: SensorBenchmarkResult = match serde_json::from_slice(&bytes) {
            Ok(p) => p,
            Err(_) => continue,
        };
        benchmarks.push(SensorBenchmarkListItem { summary: parsed.summary });
    }

    benchmarks.sort_by(|a, b| b.summary.completed_at.cmp(&a.summary.completed_at));
    (StatusCode::OK, Json(SensorBenchmarkListResponse { benchmarks })).into_response()
}
