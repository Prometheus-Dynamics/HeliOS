//! Per-workload statistics and Daedalus graph metrics published with each session.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use daedalus::{
    engine::MetricsLevel,
    runtime::{CustomMetricValue, ExecutionTelemetry, FrameOverheadReport, RuntimePlanExplanation},
};

use super::{frame_source::FrameSourceStatus, pipeline_inputs::PipelineInputStats, pmu::Counts};

/// Window over which the frame rate is measured.
const FPS_WINDOW: Duration = Duration::from_secs(1);
/// With no frame for this long the reported rate drops to zero.
const FPS_STALE_AFTER: Duration = Duration::from_secs(2);
/// Graph metrics are reported over windows of at least this length.
const METRICS_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DriverStats {
    /// Frames the camera service delivered (frame-driven workloads).
    pub frames_received: u64,
    /// Graph ticks that ran (one per frame, or per change of the bound resources).
    pub ticks_processed: u64,
    pub ticks_failed: u64,
    pub last_tick_ms: f64,
    /// From the sensor capturing the last frame to its tick's outputs (`None`: the tick had no
    /// frame, or the frame's clock is not monotonic).
    pub last_capture_latency_ms: Option<f64>,
    /// Ticks per second.
    pub fps: f64,
    pub last_frame_timestamp: Option<u64>,
    pub last_frame_size: Option<(u32, u32)>,
    pub source: FrameSourceStatus,
    pub last_error: Option<String>,
    /// The latest failed tick's error, kept after the ticks recover (`last_error` clears).
    pub last_failure: Option<String>,
    /// The CPU core the graph thread runs on, when it was pinned (`cpu` in the workload config).
    pub cpu: Option<u32>,
    /// Why the graph thread could not be pinned to its core (`cpu` is then unset).
    pub cpu_error: Option<String>,
    /// The graph thread's nice value, when it was set (`nice` in the workload config).
    pub nice: Option<u32>,
    /// Why the graph thread's nice value could not be set (`nice` is then unset).
    pub nice_error: Option<String>,
    /// The state of each pipeline input (other pipelines' outputs), at the latest frame.
    pub pipeline_inputs: Vec<PipelineInputStats>,
    /// The graph thread's hardware counters over the last second (`None` without a PMU).
    pub pmu: Option<PmuStats>,
    /// Wall-clock time of the last change, used as the published records' observed time so
    /// unchanged stats do not churn Orion state.
    pub updated_at_ms: u64,
}

impl DriverStats {
    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "frames_received": self.frames_received,
            "ticks_processed": self.ticks_processed,
            "ticks_failed": self.ticks_failed,
            "last_tick_ms": round2(self.last_tick_ms),
            "capture_latency_ms": self.last_capture_latency_ms.map(round2),
            "fps": round2(self.fps),
            "last_frame_timestamp": self.last_frame_timestamp,
            "last_frame_width": self.last_frame_size.map(|(width, _)| width),
            "last_frame_height": self.last_frame_size.map(|(_, height)| height),
            "source_connected": self.source.connected,
            "source_reconnects": self.source.reconnects,
            "source_plan": self.source.plan,
            "last_error": self.last_error,
            "last_failure": self.last_failure,
            "cpu": self.cpu,
            "cpu_error": self.cpu_error,
            "nice": self.nice,
            "nice_error": self.nice_error,
            "pipeline_inputs": self.pipeline_inputs.iter().map(PipelineInputStats::to_json).collect::<Vec<_>>(),
            "pmu": self.pmu.as_ref().map(PmuStats::to_json),
        })
    }
}

/// The graph thread's work per frame (tick) over a window: what a weaker board needs to know first.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PmuStats {
    pub ipc: f64,
    pub cycles_per_frame: f64,
    pub instructions_per_frame: f64,
    pub cache_misses_per_frame: Option<f64>,
    pub cache_miss_rate: Option<f64>,
    /// Lines read from DRAM (L3 refills x 64 B), per frame and per second.
    pub dram_bytes_per_frame: Option<f64>,
    pub dram_mb_per_s: Option<f64>,
    /// The graph thread's cycle rate (million cycles a second): its load on its core.
    pub mcycles_per_s: f64,
}

impl PmuStats {
    pub(super) fn from_window(counts: Counts, frames: u64, seconds: f64) -> Option<PmuStats> {
        if frames == 0 || seconds <= 0.0 || counts.cycles == 0 {
            return None;
        }
        let frames = frames as f64;
        let line = 64.0;
        Some(PmuStats {
            ipc: round3(counts.instructions as f64 / counts.cycles as f64),
            cycles_per_frame: (counts.cycles as f64 / frames).round(),
            instructions_per_frame: (counts.instructions as f64 / frames).round(),
            cache_misses_per_frame: counts.cache_misses.map(|misses| (misses as f64 / frames).round()),
            cache_miss_rate: counts.cache_misses.zip(counts.cache_references).filter(|(_, references)| *references > 0).map(|(misses, references)| round3(misses as f64 / references as f64)),
            dram_bytes_per_frame: counts.l3_refills.map(|refills| (refills as f64 * line / frames).round()),
            dram_mb_per_s: counts.l3_refills.map(|refills| round2(refills as f64 * line / seconds / 1e6)),
            mcycles_per_s: round2(counts.cycles as f64 / seconds / 1e6),
        })
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ipc": self.ipc,
            "cycles_per_frame": self.cycles_per_frame,
            "instructions_per_frame": self.instructions_per_frame,
            "cache_misses_per_frame": self.cache_misses_per_frame,
            "cache_miss_rate": self.cache_miss_rate,
            "dram_bytes_per_frame": self.dram_bytes_per_frame,
            "dram_mb_per_s": self.dram_mb_per_s,
            "mcycles_per_s": self.mcycles_per_s,
        })
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn micros(duration: Duration) -> f64 {
    round2(duration.as_secs_f64() * 1e6)
}

/// Frames per second over the last full window.
#[derive(Debug)]
pub(crate) struct FpsWindow {
    window_start: Instant,
    frames_in_window: u32,
    last_frame_at: Option<Instant>,
    fps: f64,
}

impl Default for FpsWindow {
    fn default() -> Self {
        Self { window_start: Instant::now(), frames_in_window: 0, last_frame_at: None, fps: 0.0 }
    }
}

impl FpsWindow {
    pub(super) fn record_frame(&mut self, now: Instant) {
        if self.last_frame_at.is_none_or(|last| now.duration_since(last) >= FPS_STALE_AFTER) {
            self.window_start = now;
            self.frames_in_window = 0;
        }
        self.frames_in_window += 1;
        self.last_frame_at = Some(now);
        let elapsed = now.duration_since(self.window_start);
        if elapsed >= FPS_WINDOW {
            self.fps = f64::from(self.frames_in_window) / elapsed.as_secs_f64();
            self.window_start = now;
            self.frames_in_window = 0;
        }
    }

    /// The current rate, dropping to zero once frames are stale.
    pub(super) fn fps(&self) -> f64 {
        if self.last_frame_at.is_none_or(|last| last.elapsed() >= FPS_STALE_AFTER) { 0.0 } else { self.fps }
    }
}

#[derive(Debug, Clone, Default)]
struct NodeTotals {
    calls: u64,
    handler: Duration,
    total: Duration,
    /// Hardware counters (Daedalus `node_perf_counters`), when recorded.
    pmu: Option<Pmu>,
    /// Bytes into and out of the node (Daedalus transport metrics, the detailed levels).
    in_bytes: u64,
    out_bytes: u64,
    /// Bytes the runtime copied for the node: materialized inputs and conversions.
    copied_bytes: u64,
    /// The node's largest frame scratch (live bytes).
    scratch_peak_bytes: u64,
    /// Counters the node reports itself (`Counter` and `Bytes` custom metrics, summed): an
    /// algorithm's own allocations, bytes moved, cache-line touches.
    custom: BTreeMap<String, u64>,
}

/// Hardware counter totals (user space, the thread that ran the work).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Pmu {
    cycles: u64,
    instructions: u64,
    cache_misses: u64,
    cache_references: u64,
}

impl Pmu {
    fn add(&mut self, other: Pmu) {
        self.cycles += other.cycles;
        self.instructions += other.instructions;
        self.cache_misses += other.cache_misses;
        self.cache_references += other.cache_references;
    }

    /// Per `count` (a call or a tick), with IPC and the miss rate.
    fn to_json(self, count: f64) -> serde_json::Value {
        let ratio = |a: u64, b: u64| (b > 0).then(|| round3(a as f64 / b as f64));
        serde_json::json!({
            "cycles": round2(self.cycles as f64 / count),
            "instructions": round2(self.instructions as f64 / count),
            "ipc": ratio(self.instructions, self.cycles),
            "cache_misses": round2(self.cache_misses as f64 / count),
            "cache_miss_rate": ratio(self.cache_misses, self.cache_references),
        })
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[derive(Debug, Clone, Default)]
struct EdgeTotals {
    samples: u64,
    wait: Duration,
    adapter: Duration,
    adapters: u64,
    adapter_errors: u64,
    transport_bytes: u64,
    copied_bytes: u64,
    payload_clones: u64,
    drops: u64,
    gpu_uploads: u64,
    gpu_downloads: u64,
    max_depth: u64,
}

#[derive(Debug, Clone, Default)]
struct MetricsTotals {
    ticks: u64,
    graph: Duration,
    unattributed: Duration,
    nodes: BTreeMap<usize, NodeTotals>,
    edges: BTreeMap<usize, EdgeTotals>,
    /// The graph thread's counters (`pmu`), when it has a PMU.
    thread: Option<Counts>,
}

impl MetricsTotals {
    /// The pipeline's hardware counters: the graph thread's, else its nodes' together (`None`
    /// without counters).
    fn pmu(&self) -> Option<Pmu> {
        if let Some(thread) = self.thread {
            return Some(Pmu { cycles: thread.cycles, instructions: thread.instructions, cache_misses: thread.cache_misses.unwrap_or(0), cache_references: thread.cache_references.unwrap_or(0) });
        }
        self.nodes.values().filter_map(|node| node.pmu).reduce(|mut sum, pmu| {
            sum.add(pmu);
            sum
        })
    }

    /// Bytes the pipeline moved: into its nodes, out of them, and copied for them.
    fn moved_bytes(&self) -> u64 {
        self.nodes.values().map(|node| node.in_bytes + node.out_bytes + node.copied_bytes).sum()
    }
}

impl MetricsTotals {
    fn add(&mut self, telemetry: &ExecutionTelemetry) {
        self.ticks += 1;
        self.graph += telemetry.graph_duration;
        self.unattributed += telemetry.unattributed_runtime_duration;
        for (index, node) in telemetry.node_metrics.iter() {
            let totals = self.nodes.entry(index).or_default();
            totals.calls += node.calls as u64;
            totals.handler += node.handler_duration;
            totals.total += node.total_duration;
            if let Some(perf) = &node.perf {
                let sample = Pmu { cycles: perf.cpu_cycles, instructions: perf.instructions, cache_misses: perf.cache_misses, cache_references: perf.cache_references };
                totals.pmu.get_or_insert_default().add(sample);
            }
            if let Some(transport) = &node.transport {
                totals.in_bytes += transport.in_bytes;
                totals.out_bytes += transport.out_bytes;
            }
            if let Some(resources) = &node.resources {
                totals.copied_bytes += resources.materialization.total_bytes + resources.conversion.total_bytes;
                totals.scratch_peak_bytes = totals.scratch_peak_bytes.max(resources.frame_scratch.peak_live_bytes);
            }
            for (name, value) in &node.custom {
                let count = match value {
                    CustomMetricValue::Counter(count) | CustomMetricValue::Bytes(count) => *count,
                    _ => continue,
                };
                *totals.custom.entry(name.clone()).or_default() += count;
            }
        }
        for (index, edge) in &telemetry.edge_metrics {
            let totals = self.edges.entry(*index).or_default();
            totals.samples += edge.samples as u64;
            totals.wait += edge.total_wait;
            totals.adapter += edge.adapter_duration;
            totals.adapters += edge.adapter_count;
            totals.adapter_errors += edge.adapter_errors;
            totals.transport_bytes += edge.transport_bytes;
            totals.copied_bytes += edge.copied_bytes;
            totals.payload_clones += edge.payload_clone_count;
            totals.drops += edge.drops;
            totals.gpu_uploads += edge.gpu_uploads;
            totals.gpu_downloads += edge.gpu_downloads;
            totals.max_depth = totals.max_depth.max(edge.max_depth);
        }
    }
}

/// Daedalus per-node and per-edge metrics of one workload graph, accumulated from each tick's
/// telemetry and reported per window (the last complete [`METRICS_WINDOW`]) and since start.
#[derive(Debug)]
pub(crate) struct GraphMetrics {
    level: MetricsLevel,
    node_names: BTreeMap<usize, (String, String)>,
    edge_names: BTreeMap<usize, (String, Vec<String>)>,
    window: MetricsTotals,
    window_started: Instant,
    last_window: Option<(MetricsTotals, Duration)>,
    total: MetricsTotals,
    /// Daedalus's latest `FrameOverheadReport`, as JSON.
    /// As JSON text: a fraction of the memory of its `Value` tree, kept between reports.
    frame_overhead: Option<Box<serde_json::value::RawValue>>,
}

impl GraphMetrics {
    pub(super) fn new(level: MetricsLevel, plan: &RuntimePlanExplanation) -> Self {
        let label = |index: usize| plan.nodes.get(index).map(|node| node.label.clone().unwrap_or_else(|| node.id.clone())).unwrap_or_else(|| index.to_string());
        let node_names = plan.nodes.iter().map(|node| (node.index, (label(node.index), node.id.clone()))).collect();
        let edge_names = plan
            .edges
            .iter()
            .map(|edge| {
                (edge.index, (format!("{}.{} -> {}.{}", label(edge.from_node), edge.from_port, label(edge.to_node), edge.to_port), edge.adapter_steps.iter().map(ToString::to_string).collect()))
            })
            .collect();
        Self { level, node_names, edge_names, window: MetricsTotals::default(), window_started: Instant::now(), last_window: None, total: MetricsTotals::default(), frame_overhead: None }
    }

    pub(super) fn enabled(&self) -> bool {
        self.level != MetricsLevel::Off
    }

    pub(super) fn record(&mut self, telemetry: &ExecutionTelemetry, now: Instant) {
        if !self.enabled() {
            return;
        }
        self.window.add(telemetry);
        self.total.add(telemetry);
        let elapsed = now.duration_since(self.window_started);
        if elapsed >= METRICS_WINDOW {
            self.last_window = Some((std::mem::take(&mut self.window), elapsed));
            self.window_started = now;
        }
    }

    /// The graph thread's counters for the time since the last call.
    pub(super) fn add_thread_counts(&mut self, counts: Counts) {
        if !self.enabled() {
            return;
        }
        self.window.thread.get_or_insert_default().add(counts);
        self.total.thread.get_or_insert_default().add(counts);
    }

    /// Keep Daedalus's frame-path overhead report (`HostGraph::frame_overhead`) for the artifact.
    pub(super) fn set_frame_overhead(&mut self, report: &FrameOverheadReport) {
        self.frame_overhead = serde_json::value::to_raw_value(report).ok();
    }

    /// The `metrics` artifact: the last complete window (or the current one before the first
    /// completes) and the totals since the graph started.
    pub(super) fn to_json(&self) -> serde_json::Value {
        let window = match &self.last_window {
            Some((totals, length)) => self.totals_json(totals, Some(*length)),
            None => self.totals_json(&self.window, Some(self.window_started.elapsed())),
        };
        serde_json::json!({
            "metrics_level": format!("{:?}", self.level),
            "window": window,
            "total": self.totals_json(&self.total, None),
            // Daedalus's frame-path overhead over its rolling window: p50/p99/max/mean of host
            // push and take, input collection, adapters, handlers, framing, drain and dispatch,
            // per-tick copies, and per-edge queue and adapter time.
            "frame_overhead": self.frame_overhead,
        })
    }

    fn totals_json(&self, totals: &MetricsTotals, length: Option<Duration>) -> serde_json::Value {
        let ticks = totals.ticks.max(1) as f64;
        let per_tick = |duration: Duration| round2(duration.as_secs_f64() * 1e6 / ticks);
        let nodes = totals
            .nodes
            .iter()
            .map(|(index, node)| {
                let (label, id) = self.node_names.get(index).cloned().unwrap_or_else(|| (index.to_string(), String::new()));
                let calls = node.calls.max(1) as f64;
                let mut json = serde_json::json!({
                    "index": index,
                    "label": label,
                    "id": id,
                    "calls": node.calls,
                    "handler_us_mean": round2(node.handler.as_secs_f64() * 1e6 / calls),
                    "total_us_mean": round2(node.total.as_secs_f64() * 1e6 / calls),
                    "handler_us_per_tick": per_tick(node.handler),
                    // Wall time in the node per tick (the timing level records totals only).
                    "total_us_per_tick": per_tick(node.total),
                });
                // What the levels above timing add, only where recorded (keeps the artifact lean).
                let extra = json.as_object_mut().expect("an object");
                if let Some(pmu) = node.pmu {
                    extra.insert("pmu_per_call".into(), pmu.to_json(calls));
                }
                if node.in_bytes + node.out_bytes + node.copied_bytes > 0 {
                    extra.insert("in_bytes_per_tick".into(), (node.in_bytes as f64 / ticks).round().into());
                    extra.insert("out_bytes_per_tick".into(), (node.out_bytes as f64 / ticks).round().into());
                    extra.insert("copied_bytes_per_tick".into(), (node.copied_bytes as f64 / ticks).round().into());
                }
                if node.scratch_peak_bytes > 0 {
                    extra.insert("scratch_peak_bytes".into(), node.scratch_peak_bytes.into());
                }
                if !node.custom.is_empty() {
                    let custom: serde_json::Map<String, serde_json::Value> = node.custom.iter().map(|(name, count)| (name.clone(), round2(*count as f64 / ticks).into())).collect();
                    extra.insert("custom_per_tick".into(), custom.into());
                }
                json
            })
            .collect::<Vec<_>>();
        let edges = totals
            .edges
            .iter()
            .map(|(index, edge)| {
                let (name, adapter_steps) = self.edge_names.get(index).cloned().unwrap_or_else(|| (index.to_string(), Vec::new()));
                serde_json::json!({
                    "index": index,
                    "edge": name,
                    "adapter_steps": adapter_steps,
                    "samples": edge.samples,
                    "wait_us_per_tick": per_tick(edge.wait),
                    "adapter_us_per_tick": per_tick(edge.adapter),
                    "adapters": edge.adapters,
                    "adapter_errors": edge.adapter_errors,
                    "transport_bytes": edge.transport_bytes,
                    "copied_bytes": edge.copied_bytes,
                    "payload_clones": edge.payload_clones,
                    "drops": edge.drops,
                    "gpu_uploads": edge.gpu_uploads,
                    "gpu_downloads": edge.gpu_downloads,
                    "max_depth": edge.max_depth,
                })
            })
            .collect::<Vec<_>>();
        let moved = totals.moved_bytes();
        let seconds = length.map(|length| length.as_secs_f64()).filter(|seconds| *seconds > 0.0);
        serde_json::json!({
            "seconds": seconds.map(round2),
            "ticks": totals.ticks,
            "graph_us_per_tick": per_tick(totals.graph),
            "unattributed_us_per_tick": per_tick(totals.unattributed),
            "graph_us_total": micros(totals.graph),
            // The pipeline's hardware counters per tick (IPC and cache misses), when recorded.
            "pmu_per_tick": totals.pmu().map(|pmu| pmu.to_json(ticks)),
            // Bytes in, out of and copied for the nodes: per tick, and MB/s over the window.
            "moved_bytes_per_tick": (moved > 0).then(|| (moved as f64 / ticks).round()),
            "moved_mb_per_s": seconds.filter(|_| moved > 0).map(|seconds| round2(moved as f64 / seconds / 1e6)),
            // DRAM reads from the graph thread's L3 refills (64-byte lines), when counted.
            "dram_mb_per_s": totals.thread.and_then(|thread| thread.l3_refills).zip(seconds).map(|(refills, seconds)| round2(refills as f64 * 64.0 / seconds / 1e6)),
            "nodes": nodes,
            "edges": edges,
        })
    }
}
