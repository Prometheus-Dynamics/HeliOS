//! Input-driven execution of a resident workload graph.
//!
//! Every workload runs its graph on a thread of its own, serially (no worker pool), and only when
//! input arrives, the way Daedalus's host-driven model and its `external_frame_source` template
//! do it:
//!
//! - **Frame-driven** workloads (a binding to a `styx-frames+unix://` camera service) have a
//!   feeder thread that blocks on the Styx `FrameClient` and pushes each `FrameLease` into the
//!   graph's latest-only host input, zero-copy (`styx::core::daedalus::frame_payload`). The graph
//!   thread waits on the host bridge (`HostGraph::inbound_waiter`) and ticks once per frame; a
//!   graph slower than the camera sees the newest frame, never a queue of stale ones.
//! - **Resource-driven** workloads tick when a bound resource changes: the engine updates the held
//!   inputs and wakes the graph thread.
//! - Graphs without host inputs run once.
//!
//! The non-frame inputs (resource state, a secondary camera's latest frame) are pushed by the
//! graph thread itself right before each tick, so a tick always sees a full set of inputs and the
//! frame push alone decides when the graph runs.
//!
//! The graph thread never waits on Orion: it keeps the latest outputs, stats and metrics and
//! signals `outputs_ready`; the engine publishes them, at most every publish interval. Frame
//! leases are released as soon as the tick that consumed them returns; frame outputs are
//! described and dropped at once.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use daedalus::{
    engine::{HostGraphPayloadInput, HostGraphStopHandle},
    runtime::host_bridge::{HostBridgeHandle, InboundWait, ValueSerializerMap},
    transport::Payload,
};
use styx::imports::framelease::FrameLease;
use tokio::sync::Notify;

use super::{
    ExecutionError,
    bindings::{FrameSourceSpec, ResourceInput},
    frame_source::{FrameReceive, FrameSource},
    graph::CompiledWorkloadGraph,
    outputs::{METRICS_ARTIFACT_KIND, PLAN_ARTIFACT_KIND, TELEMETRY_ARTIFACT_KIND, output_artifact, payload_message, session_artifact, session_id_for},
    stats::{DriverStats, FpsWindow, GraphMetrics},
};
use crate::{
    model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload},
    stream_io,
};

/// How long the feeder blocks for a frame before checking whether it should stop.
const FRAME_RECV_TIMEOUT: Duration = Duration::from_millis(200);
/// A secondary camera's last frame is reused for at most this long (Styx servers reclaim held
/// leases after ~2 s).
const SECONDARY_FRAME_MAX_AGE: Duration = Duration::from_millis(500);

/// State the driver threads share with the engine's publishing loop.
struct DriverShared {
    state: Mutex<DriverState>,
    held: Mutex<HeldInputs>,
    outputs_ready: Arc<Notify>,
}

struct DriverState {
    stats: DriverStats,
    fps: FpsWindow,
    outputs: BTreeMap<String, LatestOutput>,
    metrics: GraphMetrics,
}

/// Inputs the graph thread pushes before every tick.
#[derive(Default)]
struct HeldInputs {
    /// Bumped whenever `resources` changes; resource-driven graphs tick on it.
    generation: u64,
    resources: BTreeMap<String, ResourceInput>,
    /// The latest frame of each secondary camera, with when it arrived.
    secondary: Vec<Option<(Payload, Instant)>>,
}

#[derive(Clone)]
struct LatestOutput {
    sequence: u64,
    updated_at_ms: u64,
    value: OutputValue,
}

#[derive(Clone)]
enum OutputValue {
    /// Serialized when published, not per frame.
    Payload(Payload),
    /// Already rendered (frame descriptors, so the lease is not held).
    Message(String),
}

/// A resident workload: its graph thread, and its frame feeder when it is frame-driven.
pub(crate) struct WorkloadDriver {
    pub workload: ExecutionWorkload,
    pub sources: Vec<FrameSourceSpec>,
    shared: Arc<DriverShared>,
    host: HostBridgeHandle,
    stop: HostGraphStopHandle,
    serializers: ValueSerializerMap,
    plan_message: String,
    planning_ms: f64,
    started_at_ms: u64,
    rendered: BTreeMap<String, (u64, String)>,
    threads: Vec<JoinHandle<()>>,
}

impl WorkloadDriver {
    /// Start driving `graph`: from `frames` (one source per frame binding, the first paces the
    /// graph) when there are any, otherwise from the bound resources.
    pub fn start(workload: ExecutionWorkload, graph: CompiledWorkloadGraph, frames: Vec<(FrameSourceSpec, Box<dyn FrameSource>)>, outputs_ready: Arc<Notify>) -> Result<Self, ExecutionError> {
        let (sources, frame_sources): (Vec<_>, Vec<_>) = frames.into_iter().unzip();
        for spec in &sources {
            graph.host_graph.set_latest_input(spec.input.clone()).map_err(|error| ExecutionError::Plan(format!("frame input '{}': {error}", spec.input)))?;
        }
        let started_at_ms = now_ms();
        let metrics = GraphMetrics::new(graph.metrics_level, &graph.host_graph.explain_plan());
        let held = HeldInputs {
            // A workload with no bindings at all runs its graph once.
            generation: u64::from(workload.bindings.is_empty()),
            resources: BTreeMap::new(),
            secondary: vec![None; sources.len().saturating_sub(1)],
        };
        let shared = Arc::new(DriverShared {
            state: Mutex::new(DriverState { stats: DriverStats { updated_at_ms: started_at_ms, ..DriverStats::default() }, fps: FpsWindow::default(), outputs: BTreeMap::new(), metrics }),
            held: Mutex::new(held),
            outputs_ready,
        });
        let host = graph.host_graph.host().clone();
        let stop = graph.host_graph.stop_handle();
        let serializers = graph.host_graph.value_serializers().clone();
        let plan_message = graph.plan.to_string();
        let planning_ms = graph.planning_ms;
        let frame_inputs = sources.iter().map(|spec| spec.input.clone()).collect::<Vec<_>>();
        let feeder = frame_inputs.first().map(|primary| Feeder { input: graph.host_graph.bind_payload_input(primary.clone()), sources: frame_sources, shared: shared.clone(), stop: stop.clone() });

        let mut threads = Vec::with_capacity(2);
        let graph_loop = GraphLoop { graph, frame_inputs, shared: shared.clone(), stop: stop.clone() };
        threads.push(spawn_thread(format!("helios-graph-{}", workload.workload_id), move || graph_loop.run())?);
        if let Some(feeder) = feeder {
            match spawn_thread(format!("helios-frames-{}", workload.workload_id), move || feeder.run()) {
                Ok(thread) => threads.push(thread),
                Err(error) => {
                    stop.stop();
                    threads.into_iter().for_each(|thread| drop(thread.join()));
                    return Err(error);
                }
            }
        }
        Ok(Self { workload, sources, shared, host, stop, serializers, plan_message, planning_ms, started_at_ms, rendered: BTreeMap::new(), threads })
    }

    /// Replace the resource inputs pushed with each tick. A resource-driven graph ticks when one
    /// of them changed.
    pub fn update_context(&self, inputs: Vec<ResourceInput>) {
        let changed = {
            let mut held = lock(&self.shared.held);
            let changed = held.resources.len() != inputs.len() || inputs.iter().any(|input| held.resources.get(&input.input).is_none_or(|current| current.revision != input.revision));
            if changed {
                held.resources = inputs.into_iter().map(|input| (input.input.clone(), input)).collect();
                held.generation += 1;
            }
            changed
        };
        if changed && self.sources.is_empty() {
            self.host.wake_inbound_waiters();
        }
    }

    #[cfg(test)]
    pub fn stats(&self) -> DriverStats {
        lock(&self.shared.state).stats.clone()
    }

    /// The session and artifacts to publish now: the latest value of each host output (each
    /// rendered once), the driver's stats, the plan and, when enabled, the graph metrics.
    pub fn snapshot(&mut self) -> (ExecutionSessionState, Vec<ExecutionArtifactRecord>) {
        let (stats, outputs, metrics) = {
            let state = lock(&self.shared.state);
            let metrics = state.metrics.enabled().then(|| state.metrics.to_json());
            (state.stats.clone(), state.outputs.clone(), metrics)
        };
        let mut artifacts = Vec::with_capacity(outputs.len() + 3);
        for (port, latest) in outputs {
            let message = match self.rendered.get(&port) {
                Some((sequence, message)) if *sequence == latest.sequence => message.clone(),
                _ => {
                    let message = match &latest.value {
                        OutputValue::Payload(payload) => payload_message(payload, &self.serializers),
                        OutputValue::Message(message) => message.clone(),
                    };
                    self.rendered.insert(port.clone(), (latest.sequence, message.clone()));
                    message
                }
            };
            artifacts.push(output_artifact(&self.workload, &port, message, latest.updated_at_ms));
        }
        let observed_at_ms = stats.updated_at_ms.max(self.started_at_ms);
        let mut report = stats.to_json();
        report["planning_ms"] = serde_json::json!(self.planning_ms);
        report["frame_driven"] = serde_json::json!(!self.sources.is_empty());
        report["sources"] = self.sources.iter().map(|spec| serde_json::json!({ "input": spec.input, "resource_id": spec.resource_id, "socket": spec.socket_path })).collect();
        artifacts.push(session_artifact(&self.workload, "telemetry", TELEMETRY_ARTIFACT_KIND, report.to_string(), observed_at_ms));
        artifacts.push(session_artifact(&self.workload, "plan", PLAN_ARTIFACT_KIND, self.plan_message.clone(), self.started_at_ms));
        if let Some(metrics) = metrics {
            artifacts.push(session_artifact(&self.workload, "metrics", METRICS_ARTIFACT_KIND, metrics.to_string(), observed_at_ms));
        }
        let status = match (stats.ticks_processed, stats.ticks_failed) {
            (0, 0) => ExecutionSessionStatus::Starting,
            (0, _) => ExecutionSessionStatus::Failed,
            _ => ExecutionSessionStatus::Running,
        };
        report["status"] = serde_json::json!(status.as_str());
        let session = ExecutionSessionState {
            workload_id: self.workload.workload_id.clone(),
            session_id: session_id_for(&self.workload),
            status,
            observed_at_ms,
            graph_ref: self.workload.graph_ref.clone(),
            bindings: self.workload.bindings.clone(),
            plugin_requirements: self.workload.plugin_requirements.clone(),
            message: Some(report.to_string()),
        };
        (session, artifacts)
    }
}

impl Drop for WorkloadDriver {
    fn drop(&mut self) {
        // Sets the flag and wakes the graph thread; the feeder sees it within one receive wait.
        self.stop.stop();
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                tracing::warn!(workload_id = %self.workload.workload_id, "workload driver thread panicked");
            }
        }
    }
}

fn spawn_thread(name: String, run: impl FnOnce() + Send + 'static) -> Result<JoinHandle<()>, ExecutionError> {
    std::thread::Builder::new().name(name).spawn(run).map_err(|error| ExecutionError::Execute(format!("failed to start workload driver thread: {error}")))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The graph thread: waits on the host bridge and ticks when input is there.
struct GraphLoop {
    graph: CompiledWorkloadGraph,
    /// Frame inputs, the primary (pacing) one first.
    frame_inputs: Vec<String>,
    shared: Arc<DriverShared>,
    stop: HostGraphStopHandle,
}

impl GraphLoop {
    fn run(mut self) {
        let frame_driven = !self.frame_inputs.is_empty();
        let has_host_inputs = !self.graph.input_ports.is_empty();
        let mut seen_generation = 0;
        let mut sequence = 0_u64;
        loop {
            // Created before the checks, so input (or a stop) arriving in between still wakes it.
            let waiter = self.graph.host_graph.inbound_waiter();
            if self.stop.is_stopped() {
                break;
            }
            let ready = if frame_driven { self.graph.host_graph.host().has_pending_inbound() } else { lock(&self.shared.held).generation != seen_generation };
            if !ready {
                if waiter.wait(None) == InboundWait::Closed {
                    break;
                }
                continue;
            }
            seen_generation = self.push_held_inputs();
            let started_at = Instant::now();
            let result = if has_host_inputs { self.graph.host_graph.tick() } else { self.graph.host_graph.run_executor_once().map(|run| run.telemetry) };
            sequence += 1;
            self.finish_tick(result, started_at, sequence);
        }
    }

    /// Push the resource inputs and the secondary cameras' latest frames; returns the resource
    /// generation pushed.
    fn push_held_inputs(&mut self) -> u64 {
        let mut held = lock(&self.shared.held);
        for (port, input) in &held.resources {
            self.graph.host_graph.push(port.clone(), input.payload.clone());
        }
        let host_graph = &self.graph.host_graph;
        for (slot, input) in held.secondary.iter_mut().zip(self.frame_inputs.iter().skip(1)) {
            if slot.as_ref().is_some_and(|(_, received_at)| received_at.elapsed() > SECONDARY_FRAME_MAX_AGE) {
                *slot = None;
            }
            if let Some((payload, _)) = slot {
                host_graph.push_payload(input.clone(), payload.clone());
            }
        }
        held.generation
    }

    fn finish_tick(&mut self, result: Result<daedalus::runtime::ExecutionTelemetry, daedalus::engine::EngineError>, started_at: Instant, sequence: u64) {
        let now = Instant::now();
        let tick_ms = now.duration_since(started_at).as_secs_f64() * 1000.0;
        let updated_at_ms = now_ms();
        let mut outputs = Vec::new();
        for port in &self.graph.output_ports {
            let mut latest = None;
            while let Some(payload) = self.graph.host_graph.take_payload(port) {
                latest = Some(payload);
            }
            if let Some(payload) = latest {
                let value = match payload.get_ref::<FrameLease>() {
                    Some(frame) => OutputValue::Message(stream_io::framelease_descriptor_json(frame).to_string()),
                    None => OutputValue::Payload(payload),
                };
                outputs.push((port.clone(), LatestOutput { sequence, updated_at_ms, value }));
            }
        }

        {
            let mut state = lock(&self.shared.state);
            let state = &mut *state;
            state.outputs.extend(outputs);
            state.fps.record_frame(now);
            state.stats.fps = state.fps.fps();
            state.stats.last_tick_ms = tick_ms;
            state.stats.updated_at_ms = updated_at_ms;
            match result {
                Ok(telemetry) => {
                    state.stats.ticks_processed += 1;
                    state.stats.last_error = None;
                    state.metrics.record(&telemetry, now);
                }
                Err(error) => {
                    state.stats.ticks_failed += 1;
                    state.stats.last_error = Some(format!("graph tick failed: {error}"));
                }
            }
        }
        self.shared.outputs_ready.notify_one();
    }
}

/// The frame feeder: blocks on the primary camera and pushes each frame into the graph.
struct Feeder {
    input: HostGraphPayloadInput,
    sources: Vec<Box<dyn FrameSource>>,
    shared: Arc<DriverShared>,
    stop: HostGraphStopHandle,
}

impl Feeder {
    fn run(mut self) {
        while !self.stop.is_stopped() {
            let frame = match self.sources[0].recv(FRAME_RECV_TIMEOUT) {
                FrameReceive::Frame(frame) => frame,
                FrameReceive::Idle => {
                    self.record_idle(None);
                    continue;
                }
                FrameReceive::Unavailable(reason) => {
                    self.record_idle(Some(reason));
                    continue;
                }
            };
            if self.sources.len() > 1 {
                let mut held = lock(&self.shared.held);
                for (index, source) in self.sources.iter_mut().enumerate().skip(1) {
                    while let FrameReceive::Frame(newer) = source.recv(Duration::ZERO) {
                        held.secondary[index - 1] = Some((stream_io::framelease_payload(newer), Instant::now()));
                    }
                }
            }
            let timestamp = frame.meta().timestamp;
            let size = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
            let status = self.sources[0].status();
            {
                let mut state = lock(&self.shared.state);
                let stats = &mut state.stats;
                stats.frames_received += 1;
                stats.last_frame_timestamp = Some(timestamp);
                stats.last_frame_size = Some(size);
                stats.source = status;
            }
            // Latest-only: a frame the graph has not taken yet is replaced (and released).
            self.input.push(stream_io::framelease_payload(frame));
        }
    }

    fn record_idle(&self, reason: Option<String>) {
        let status = self.sources[0].status();
        let changed = {
            let mut state = lock(&self.shared.state);
            let fps = state.fps.fps();
            let stats = &mut state.stats;
            let changed = stats.source != status || stats.fps != fps || (reason.is_some() && stats.last_error != reason);
            if changed {
                stats.source = status;
                stats.fps = fps;
                if reason.is_some() {
                    stats.last_error = reason;
                }
                stats.updated_at_ms = now_ms();
            }
            changed
        };
        if changed {
            self.shared.outputs_ready.notify_one();
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
