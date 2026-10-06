//! Per-workload statistics and Daedalus graph metrics published with each session.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use daedalus::{
    engine::MetricsLevel,
    runtime::{ExecutionTelemetry, FrameOverheadReport, RuntimePlanExplanation},
};

use super::frame_source::FrameSourceStatus;

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
    /// Ticks per second.
    pub fps: f64,
    pub last_frame_timestamp: Option<u64>,
    pub last_frame_size: Option<(u32, u32)>,
    pub source: FrameSourceStatus,
    pub last_error: Option<String>,
    /// Wall-clock time of the last change, used as the published records' observed time so
    /// unchanged stats do not churn Orion state.
    pub updated_at_ms: u64,
}

impl DriverStats {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "frames_received": self.frames_received,
            "ticks_processed": self.ticks_processed,
            "ticks_failed": self.ticks_failed,
            "last_tick_ms": round2(self.last_tick_ms),
            "fps": round2(self.fps),
            "last_frame_timestamp": self.last_frame_timestamp,
            "last_frame_width": self.last_frame_size.map(|(width, _)| width),
            "last_frame_height": self.last_frame_size.map(|(_, height)| height),
            "source_connected": self.source.connected,
            "source_reconnects": self.source.reconnects,
            "source_plan": self.source.plan,
            "last_error": self.last_error,
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
    pub fn record_frame(&mut self, now: Instant) {
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
    pub fn fps(&self) -> f64 {
        if self.last_frame_at.is_none_or(|last| last.elapsed() >= FPS_STALE_AFTER) { 0.0 } else { self.fps }
    }
}

#[derive(Debug, Clone, Default)]
struct NodeTotals {
    calls: u64,
    handler: Duration,
    total: Duration,
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
    frame_overhead: Option<serde_json::Value>,
}

impl GraphMetrics {
    pub fn new(level: MetricsLevel, plan: &RuntimePlanExplanation) -> Self {
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

    pub fn enabled(&self) -> bool {
        self.level != MetricsLevel::Off
    }

    pub fn record(&mut self, telemetry: &ExecutionTelemetry, now: Instant) {
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

    /// Keep Daedalus's frame-path overhead report (`HostGraph::frame_overhead`) for the artifact.
    pub fn set_frame_overhead(&mut self, report: &FrameOverheadReport) {
        self.frame_overhead = serde_json::to_value(report).ok();
    }

    /// The `metrics` artifact: the last complete window (or the current one before the first
    /// completes) and the totals since the graph started.
    pub fn to_json(&self) -> serde_json::Value {
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
                serde_json::json!({
                    "index": index,
                    "label": label,
                    "id": id,
                    "calls": node.calls,
                    "handler_us_mean": round2(node.handler.as_secs_f64() * 1e6 / calls),
                    "total_us_mean": round2(node.total.as_secs_f64() * 1e6 / calls),
                    "handler_us_per_tick": per_tick(node.handler),
                })
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
        serde_json::json!({
            "seconds": length.map(|length| round2(length.as_secs_f64())),
            "ticks": totals.ticks,
            "graph_us_per_tick": per_tick(totals.graph),
            "unattributed_us_per_tick": per_tick(totals.unattributed),
            "graph_us_total": micros(totals.graph),
            "nodes": nodes,
            "edges": edges,
        })
    }
}
