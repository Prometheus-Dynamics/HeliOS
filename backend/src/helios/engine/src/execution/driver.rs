//! Input-driven execution of a resident workload graph.
//!
//! Every workload runs its graph on one thread of its own, serially (no worker pool), and only
//! when input arrives, with Daedalus's drive loops (`HostGraph::drive_blocking` and its async
//! twin `HostGraph::drive`):
//!
//! - **Frame-driven** workloads (a binding to a `styx-frames+unix://` camera service): the graph
//!   thread awaits its camera's Styx `FrameClient` (pollable; Styx `docs/frame-server.md`,
//!   "Without a thread per client") and the graph's inbound wake together, on one
//!   `styx_graph::rt::block_on`. Each frame goes into the graph's latest-only host input,
//!   zero-copy (`styx::core::daedalus::frame_payload`), and wakes the drive loop, which ticks
//!   once; frames that arrived during a tick are drained to the newest, so a graph slower than
//!   the camera never sees a queue of stale ones. A secondary camera's latest frame is pushed in
//!   one atomic batch with the primary frame (`HostGraph::batch`), so both land in the same tick.
//!   Resource inputs are **held** host inputs (`set_held_input`): the engine pushes a resource
//!   when it changes and every tick sees its latest value, without re-pushing and without
//!   ticking on its own. No feeder thread runs per camera.
//! - **Resource-driven** workloads tick when a bound resource changes: the engine pushes the
//!   full set of resource inputs as one batch into latest-only inputs, and the graph thread's
//!   `drive_blocking` ticks once per batch.
//! - Graphs without bindings run once.
//!
//! The graph thread never waits on Orion: it keeps the latest outputs, stats and metrics and
//! signals `outputs_ready`; the engine publishes them, at most every publish interval. Frame
//! leases are released as soon as the tick that consumed them returns; frame outputs are
//! described and dropped at once.

use std::{
    collections::BTreeMap,
    future::{Future, poll_fn},
    pin::pin,
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use daedalus::{
    engine::{EngineError, HostGraphStopHandle, HostGraphTurn},
    runtime::{
        ExecutionTelemetry,
        host_bridge::{HostBridgeHandle, ValueSerializerMap},
    },
    transport::Payload,
};
use styx::imports::framelease::FrameLease;
use tokio::sync::Notify;

use super::{
    ExecutionError,
    bindings::{FrameSourceSpec, ResourceInput},
    frame_source::{FrameReceive, FrameSource, FrameSourceStatus},
    graph::{CompiledWorkloadGraph, ResidentHostGraph},
    outputs::{METRICS_ARTIFACT_KIND, PLAN_ARTIFACT_KIND, TELEMETRY_ARTIFACT_KIND, output_artifact, payload_message, session_artifact, session_id_for},
    stats::{DriverStats, FpsWindow, GraphMetrics},
};
use crate::{
    model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload},
    stream_io,
};

/// A secondary camera's last frame is reused for at most this long (Styx servers reclaim held
/// leases after ~2 s).
const SECONDARY_FRAME_MAX_AGE: Duration = Duration::from_millis(500);
/// Daedalus's frame-overhead report is read from the graph at most this often.
const FRAME_OVERHEAD_INTERVAL: Duration = Duration::from_secs(1);

/// State the graph thread shares with the engine's publishing loop.
struct DriverShared {
    state: Mutex<DriverState>,
    outputs_ready: Arc<Notify>,
}

struct DriverState {
    stats: DriverStats,
    fps: FpsWindow,
    outputs: BTreeMap<String, LatestOutput>,
    metrics: GraphMetrics,
    /// Bumped per tick; outputs carry the tick that produced them.
    sequence: u64,
    /// When the input that the next tick consumes was pushed (tick latency).
    last_input_at: Option<Instant>,
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

/// What makes a workload's graph tick.
enum Trigger {
    /// Camera frames; the first source paces the graph.
    Frames(Vec<Box<dyn FrameSource>>),
    /// Changes of the bound resources.
    Resources,
    /// Nothing: the graph runs once.
    Once,
}

/// A resident workload and its graph thread.
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
    /// The revision of each resource input last pushed.
    revisions: BTreeMap<String, String>,
    thread: Option<JoinHandle<()>>,
}

impl WorkloadDriver {
    /// Start driving `graph`: from `frames` (one source per frame binding, the first paces the
    /// graph) when there are any, otherwise from the bound resources.
    pub fn start(workload: ExecutionWorkload, graph: CompiledWorkloadGraph, frames: Vec<(FrameSourceSpec, Box<dyn FrameSource>)>, outputs_ready: Arc<Notify>) -> Result<Self, ExecutionError> {
        let (sources, frame_sources): (Vec<_>, Vec<_>) = frames.into_iter().unzip();
        let frame_inputs = sources.iter().map(|spec| spec.input.clone()).collect::<Vec<_>>();
        let latest = |port: &String| graph.host_graph.set_latest_input(port.clone()).map_err(|error| ExecutionError::Plan(format!("host input '{port}': {error}")));
        frame_inputs.iter().try_for_each(latest)?;
        let mut context_inputs = graph.input_ports.iter().filter(|port| !frame_inputs.contains(port));
        let trigger = if !frame_sources.is_empty() {
            context_inputs.for_each(|port| graph.host_graph.set_held_input(port.clone()));
            Trigger::Frames(frame_sources)
        } else if workload.bindings.is_empty() {
            Trigger::Once
        } else {
            context_inputs.try_for_each(latest)?;
            Trigger::Resources
        };

        let started_at_ms = now_ms();
        let metrics = GraphMetrics::new(graph.metrics_level, &graph.host_graph.explain_plan());
        let shared = Arc::new(DriverShared {
            state: Mutex::new(DriverState {
                stats: DriverStats { updated_at_ms: started_at_ms, ..DriverStats::default() },
                fps: FpsWindow::default(),
                outputs: BTreeMap::new(),
                metrics,
                sequence: 0,
                last_input_at: None,
            }),
            outputs_ready,
        });
        let host = graph.host_graph.host().clone();
        let stop = graph.host_graph.stop_handle();
        let serializers = graph.host_graph.value_serializers().clone();
        let plan_message = graph.plan.to_string();
        let planning_ms = graph.planning_ms;
        let graph_loop = GraphLoop { recorder: TickRecorder::new(&graph, shared.clone()), graph, frame_inputs, shared: shared.clone(), stop: stop.clone() };
        let thread = std::thread::Builder::new()
            .name(format!("helios-graph-{}", workload.workload_id))
            .spawn(move || graph_loop.run(trigger))
            .map_err(|error| ExecutionError::Execute(format!("failed to start workload driver thread: {error}")))?;
        Ok(Self { workload, sources, shared, host, stop, serializers, plan_message, planning_ms, started_at_ms, rendered: BTreeMap::new(), revisions: BTreeMap::new(), thread: Some(thread) })
    }

    /// Hand the bound resources to the graph when one of them changed: a frame-driven graph's
    /// held inputs take the changed values (and its next frame sees them); a resource-driven
    /// graph gets the full set in one batch and ticks once.
    pub fn update_context(&mut self, inputs: Vec<ResourceInput>) {
        let changed = self.revisions.len() != inputs.len() || inputs.iter().any(|input| self.revisions.get(&input.input) != Some(&input.revision));
        if !changed {
            return;
        }
        if self.sources.is_empty() {
            let batch = inputs.iter().fold(self.host.batch(), |batch, input| batch.push(input.input.clone(), input.payload.clone()));
            lock(&self.shared.state).last_input_at = Some(Instant::now());
            if let Err(rejected) = batch.commit() {
                self.record_error(format!("resource inputs rejected: {rejected}"));
            }
        } else {
            for input in inputs.iter().filter(|input| self.revisions.get(&input.input) != Some(&input.revision)) {
                self.host.push(input.input.clone(), input.payload.clone());
            }
        }
        self.revisions = inputs.into_iter().map(|input| (input.input, input.revision)).collect();
    }

    fn record_error(&self, error: String) {
        tracing::warn!(workload_id = %self.workload.workload_id, %error, "workload input");
        let mut state = lock(&self.shared.state);
        state.stats.last_error = Some(error);
        state.stats.updated_at_ms = now_ms();
    }

    #[cfg(test)]
    pub fn stats(&self) -> DriverStats {
        lock(&self.shared.state).stats.clone()
    }

    /// The session and artifacts to publish now: the latest value of each host output (each
    /// rendered once), the driver's stats, the plan and, when enabled, the graph metrics.
    pub fn snapshot(&mut self) -> (ExecutionSessionState, Vec<ExecutionArtifactRecord>) {
        let (stats, outputs, metrics) = {
            let mut state = lock(&self.shared.state);
            // The rate decays to zero when frames stop, with no timer on the graph thread.
            let fps = state.fps.fps();
            if state.stats.fps != fps {
                state.stats.fps = fps;
                state.stats.updated_at_ms = now_ms();
            }
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
        // Sets the flag and wakes the graph thread's drive loop (or its connect backoff).
        self.stop.stop();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!(workload_id = %self.workload.workload_id, "workload driver thread panicked");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Records each tick's outputs, stats and metrics for the publishing loop.
struct TickRecorder {
    output_ports: Vec<String>,
    shared: Arc<DriverShared>,
    /// When to read Daedalus's frame-overhead report next (`None`: not recorded).
    next_overhead: Option<Instant>,
}

impl TickRecorder {
    fn new(graph: &CompiledWorkloadGraph, shared: Arc<DriverShared>) -> Self {
        let next_overhead = graph.host_graph.frame_overhead_enabled().then(Instant::now);
        Self { output_ports: graph.output_ports.clone(), shared, next_overhead }
    }

    fn record(&mut self, graph: &ResidentHostGraph, result: Result<&ExecutionTelemetry, String>) {
        let now = Instant::now();
        let updated_at_ms = now_ms();
        let mut outputs = Vec::new();
        for port in &self.output_ports {
            let mut latest = None;
            while let Some(payload) = graph.take_payload(port) {
                latest = Some(payload);
            }
            if let Some(payload) = latest {
                let value = match payload.get_ref::<FrameLease>() {
                    Some(frame) => OutputValue::Message(stream_io::framelease_descriptor_json(frame).to_string()),
                    None => OutputValue::Payload(payload),
                };
                outputs.push((port.clone(), value));
            }
        }
        let frame_overhead = match self.next_overhead {
            Some(due) if now >= due => {
                self.next_overhead = Some(now + FRAME_OVERHEAD_INTERVAL);
                graph.frame_overhead()
            }
            _ => None,
        };

        {
            let mut state = lock(&self.shared.state);
            let state = &mut *state;
            state.sequence += 1;
            let sequence = state.sequence;
            state.outputs.extend(outputs.into_iter().map(|(port, value)| (port, LatestOutput { sequence, updated_at_ms, value })));
            state.fps.record_frame(now);
            state.stats.fps = state.fps.fps();
            state.stats.last_tick_ms = state.last_input_at.take().map_or(0.0, |pushed| now.duration_since(pushed).as_secs_f64() * 1000.0);
            state.stats.updated_at_ms = updated_at_ms;
            match result {
                Ok(telemetry) => {
                    state.stats.ticks_processed += 1;
                    state.stats.last_error = None;
                    state.metrics.record(telemetry, now);
                }
                Err(error) => {
                    state.stats.ticks_failed += 1;
                    state.stats.last_error = Some(format!("graph tick failed: {error}"));
                }
            }
            if let Some(report) = frame_overhead {
                state.metrics.set_frame_overhead(&report);
            }
        }
        self.shared.outputs_ready.notify_one();
    }

    /// Record a tick that finished (`turn`) from a drive loop.
    fn record_turn(&mut self, graph: &ResidentHostGraph, turn: &HostGraphTurn) -> Result<(), EngineError> {
        if let Some(telemetry) = &turn.telemetry {
            self.record(graph, Ok(telemetry));
        }
        Ok(())
    }
}

/// The graph thread.
struct GraphLoop {
    graph: CompiledWorkloadGraph,
    recorder: TickRecorder,
    /// Frame inputs, the primary (pacing) one first.
    frame_inputs: Vec<String>,
    shared: Arc<DriverShared>,
    stop: HostGraphStopHandle,
}

/// Why a connected frame-driven run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunExit {
    Stopped,
    /// The primary camera's connection is gone; connect again.
    SourceClosed,
}

impl GraphLoop {
    fn run(self, trigger: Trigger) {
        match trigger {
            Trigger::Once => self.run_once(),
            Trigger::Resources => self.drive_resources(),
            Trigger::Frames(sources) => self.drive_frames(sources),
        }
    }

    fn run_once(mut self) {
        lock(&self.shared.state).last_input_at = Some(Instant::now());
        let host_graph = &mut self.graph.host_graph;
        let result = if self.graph.input_ports.is_empty() { host_graph.run_executor_once().map(|run| run.telemetry) } else { host_graph.tick() };
        match result {
            Ok(telemetry) => self.recorder.record(host_graph, Ok(&telemetry)),
            Err(error) => self.recorder.record(host_graph, Err(error.to_string())),
        }
    }

    /// `drive_blocking` until stopped; a failed tick is recorded and the loop resumes.
    fn drive_resources(mut self) {
        loop {
            let recorder = &mut self.recorder;
            match self.graph.host_graph.drive_blocking(&self.stop, |graph, turn| recorder.record_turn(graph, turn)) {
                Ok(_) => return,
                Err(error) => self.recorder.record(&self.graph.host_graph, Err(error.to_string())),
            }
        }
    }

    /// Connect to the primary camera (backing off while it is not there), then await its frames
    /// and the graph's drive loop together on this thread until stopped or disconnected.
    fn drive_frames(mut self, mut sources: Vec<Box<dyn FrameSource>>) {
        let host = self.graph.host_graph.host().clone();
        loop {
            // Created before the stop check, so a stop in between still wakes the backoff wait.
            let waiter = self.graph.host_graph.inbound_waiter();
            if self.stop.is_stopped() {
                return;
            }
            if let Err((reason, retry_in)) = sources[0].connect() {
                record_source_status(&self.shared, sources[0].status(), Some(reason));
                let _ = waiter.wait(Some(retry_in));
                continue;
            }
            drop(waiter);
            record_source_status(&self.shared, sources[0].status(), None);
            let pump = FramePump { sources: &mut sources, host: &host, inputs: &self.frame_inputs, shared: &self.shared, secondary: vec![None; self.frame_inputs.len().saturating_sub(1)] };
            let exit = styx_graph::rt::block_on(run_connected(&mut self.graph.host_graph, &self.stop, pump, &mut self.recorder));
            // A frame pushed right before the connection went away is not left pending.
            self.frame_inputs.iter().for_each(|input| host.clear_input(input));
            if exit == RunExit::Stopped {
                return;
            }
        }
    }
}

/// The graph's async drive loop and the camera pump, polled together (the pump first, so frames
/// that arrived during a tick are in before the drive loop looks). A failed tick is recorded and
/// the drive loop resumes.
async fn run_connected(graph: &mut ResidentHostGraph, stop: &HostGraphStopHandle, pump: FramePump<'_>, recorder: &mut TickRecorder) -> RunExit {
    let mut pump = pin!(pump.run());
    loop {
        let result = {
            let mut drive = pin!(graph.drive(stop, |graph, turn| recorder.record_turn(graph, turn)));
            poll_fn(|cx| {
                if pump.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                drive.as_mut().poll(cx).map(Some)
            })
            .await
        };
        match result {
            None => return RunExit::SourceClosed,
            Some(Ok(_)) => return RunExit::Stopped,
            Some(Err(error)) => recorder.record(graph, Err(error.to_string())),
        }
    }
}

/// Moves camera frames into the graph's host inputs on the graph thread.
struct FramePump<'a> {
    /// The primary source first.
    sources: &'a mut [Box<dyn FrameSource>],
    host: &'a HostBridgeHandle,
    /// The host input of each source.
    inputs: &'a [String],
    shared: &'a DriverShared,
    /// Each secondary camera's latest frame, with when it arrived.
    secondary: Vec<Option<(Payload, Instant)>>,
}

impl FramePump<'_> {
    /// Completes when the primary camera's connection is gone.
    async fn run(mut self) {
        poll_fn(|cx| self.poll(cx)).await;
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let mut newest = None;
        let closed = loop {
            match self.sources[0].poll_frame(cx) {
                // Latest-only: an older frame drained here is released at once.
                Poll::Ready(FrameReceive::Frame(frame)) => newest = Some(frame),
                Poll::Ready(FrameReceive::Closed(reason)) => break Some(reason),
                Poll::Ready(FrameReceive::Idle) => {
                    cx.waker().wake_by_ref();
                    break None;
                }
                Poll::Pending => break None,
            }
        };
        match newest {
            Some(frame) => self.push(frame),
            None => record_source_status(self.shared, self.sources[0].status(), None),
        }
        match closed {
            Some(reason) => {
                record_source_status(self.shared, self.sources[0].status(), Some(reason));
                Poll::Ready(())
            }
            None => Poll::Pending,
        }
    }

    /// Push `frame`, with the secondary cameras' latest frames in the same batch.
    fn push(&mut self, frame: FrameLease) {
        let timestamp = frame.meta().timestamp;
        let size = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
        let status = self.sources[0].status();
        {
            let mut state = lock(&self.shared.state);
            state.last_input_at = Some(Instant::now());
            let stats = &mut state.stats;
            stats.frames_received += 1;
            stats.last_frame_timestamp = Some(timestamp);
            stats.last_frame_size = Some(size);
            stats.source = status;
        }
        let payload = stream_io::framelease_payload(frame);
        if self.sources.len() == 1 {
            // Latest-only: a frame the graph has not taken yet is replaced (and released).
            self.host.feed_payload(self.inputs[0].clone(), payload);
            return;
        }
        let mut batch = self.host.batch().push_payload(self.inputs[0].clone(), payload);
        for (index, source) in self.sources.iter_mut().enumerate().skip(1) {
            let slot = &mut self.secondary[index - 1];
            while let FrameReceive::Frame(newer) = source.try_frame() {
                *slot = Some((stream_io::framelease_payload(newer), Instant::now()));
            }
            if slot.as_ref().is_some_and(|(_, received_at)| received_at.elapsed() > SECONDARY_FRAME_MAX_AGE) {
                *slot = None;
            }
            if let Some((payload, _)) = slot {
                batch = batch.push_payload(self.inputs[index].clone(), payload.clone());
            }
        }
        if let Err(rejected) = batch.commit() {
            record_source_status(self.shared, self.sources[0].status(), Some(format!("frames rejected: {rejected}")));
        }
    }
}

/// Note the primary camera's connection state (and why it has none); publishes when it changed.
fn record_source_status(shared: &DriverShared, status: FrameSourceStatus, reason: Option<String>) {
    let changed = {
        let mut state = lock(&shared.state);
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
        shared.outputs_ready.notify_one();
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
