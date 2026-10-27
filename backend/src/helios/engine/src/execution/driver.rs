//! Input-driven execution of a resident workload graph.
//!
//! Every workload runs its graph on one thread of its own, serially (no worker pool), and only
//! when input arrives. The thread is a plain blocking loop around one `poll(2)` over its
//! cameras' Styx `FrameClient` descriptors and the graph's Daedalus inbound fd
//! (`HostGraph::inbound_fd`; Daedalus `docs/node-authoring.md`, "Waiting With poll(2) / epoll";
//! Styx `docs/frame-server.md`, "Without a thread per client"). No async runtime, no feeder
//! thread per camera:
//!
//! - **Frame-driven** workloads (a binding to a `styx-frames+unix://` camera service): each
//!   camera is a reconnecting, non-blocking `FrameClient` (`request_nonblocking`), so a camera
//!   that is not up never blocks the thread. When a camera's descriptor is readable its frames
//!   are drained with `try_next()` to the newest; the primary camera's frame goes into the
//!   graph's latest-only host input, zero-copy (`styx::core::daedalus::frame_payload`), with
//!   each secondary camera's latest frame in the same atomic batch (`HostGraph::batch`), so
//!   they land in the same tick. A graph slower than the camera never sees a queue of stale
//!   frames. Resource inputs are **held** host inputs (declared in the document before
//!   planning): the engine pushes a resource when it changes and every tick sees its latest
//!   value, without re-pushing and without ticking on its own.
//! - **Resource-driven** workloads tick when a bound resource changes: the engine pushes the
//!   full set of resource inputs as one batch into latest-only inputs, which makes the inbound
//!   fd readable, and the graph thread ticks once per batch.
//! - Graphs without bindings run once.
//!
//! When the inbound fd is readable the thread calls `HostGraph::tick_ready()` (which clears and
//! re-arms the fd; the thread never reads it). `HostGraphStopHandle::stop()` makes it readable,
//! so a stop ends the loop at once.
//!
//! The graph thread never waits on Orion: it keeps the latest outputs, stats and metrics and
//! signals `outputs_ready`; the engine publishes them, at most every publish interval. Frame
//! leases are released as soon as the tick that consumed them returns; frame outputs are
//! described and dropped at once.

use std::{
    collections::BTreeMap,
    os::fd::AsFd,
    sync::{Arc, Mutex, MutexGuard},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use daedalus::{
    engine::HostGraphStopHandle,
    runtime::{
        ExecutionTelemetry,
        host_bridge::{HostBridgeHandle, InboundFd, ValueSerializerMap},
    },
    transport::{FeedOutcome, Payload},
};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, PollTimeout, poll},
};
use styx::imports::framelease::FrameLease;
use tokio::sync::Notify;

use super::{
    ExecutionError,
    bindings::{FrameSourceSpec, ResourceInput},
    camera_context,
    frame_source::{FrameReceive, FrameSource, FrameSourceStatus},
    graph::{CompiledWorkloadGraph, ResidentHostGraph},
    map_io::{self, MAP_PORT, SAVE_PORT, SAVED_MAP_PORT, SavePulse},
    outputs::{METRICS_ARTIFACT_KIND, PLAN_ARTIFACT_KIND, TELEMETRY_ARTIFACT_KIND, output_artifact, payload_message, session_artifact, session_id_for},
    pipeline_bus::{PipelineBus, next_run_id},
    pipeline_inputs::PipelineFeeds,
    stats::{DriverStats, FpsWindow, GraphMetrics},
};
use crate::{
    model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload},
    nt4::{Nt4Sink, Nt4Tick},
    stream_io,
};

/// A secondary camera's last frame is reused for at most this long (Styx servers reclaim held
/// leases after ~2 s).
const SECONDARY_FRAME_MAX_AGE: Duration = Duration::from_millis(500);
/// Daedalus's frame-overhead report is read from the graph at most this often.
const FRAME_OVERHEAD_INTERVAL: Duration = Duration::from_secs(1);
/// The metrics artifact is rendered at most this often.
const METRICS_RENDER_INTERVAL: Duration = Duration::from_secs(1);
/// While a frame waits for pipeline inputs, the poll wakes this often to look at the bus again (a
/// producer's publication does not wake the consumer's descriptors).
const PIPELINE_WAIT_POLL_MS: u16 = 1;

/// State the graph thread shares with the engine's publishing loop.
struct DriverShared {
    state: Mutex<DriverState>,
    outputs_ready: Arc<Notify>,
}

struct DriverState {
    /// The `save` input's state: a save request raises it, the saved map lowers it.
    save: SavePulse,
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
    /// The pipeline's id on the bus, and this driver's run there (withdrawn when it stops).
    pipeline: String,
    run: u64,
    bus: PipelineBus,
    shared: Arc<DriverShared>,
    host: HostBridgeHandle,
    stop: HostGraphStopHandle,
    serializers: ValueSerializerMap,
    plan_message: String,
    planning_ms: f64,
    started_at_ms: u64,
    rendered: BTreeMap<String, (u64, String)>,
    /// The metrics artifact as last rendered (when, the JSON, its observed_at_ms).
    metrics_rendered: Option<(Instant, String, u64)>,
    /// The revision of each resource input last pushed.
    revisions: BTreeMap<String, String>,
    /// Host inputs of the graph (for camera context).
    input_ports: Vec<String>,
    /// The structured camera context last pushed, per host input.
    camera_context: BTreeMap<String, serde_json::Value>,
    thread: Option<JoinHandle<()>>,
}

impl WorkloadDriver {
    /// Start driving `graph`: from `frames` (one source per frame binding, the first paces the
    /// graph) when there are any, otherwise from the bound resources.
    pub fn start(
        workload: ExecutionWorkload,
        graph: CompiledWorkloadGraph,
        frames: Vec<(FrameSourceSpec, Box<dyn FrameSource>)>,
        outputs_ready: Arc<Notify>,
        sinks: impl Into<OutputSinks>,
        bus: PipelineBus,
    ) -> Result<Self, ExecutionError> {
        let OutputSinks { nt4, live } = sinks.into();
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
                save: SavePulse::default(),
            }),
            outputs_ready,
        });
        let host = graph.host_graph.host().clone();
        // A saved map is the start of the run: it is in the graph's `map` input before the thread
        // ticks (a new map is a new run, see `map_io`). A map the graph cannot take fails the start.
        if let Some(map) = &workload.map {
            if !graph.input_ports.iter().any(|port| port == MAP_PORT) {
                return Err(ExecutionError::Plan(format!("map {}: the graph has no {MAP_PORT} input", map.id)));
            }
            let bytes = map_io::load_map(&map.path).map_err(ExecutionError::Execute)?;
            if let FeedOutcome::Rejected(reason) = host.push_arc_as(MAP_PORT, map_io::MAP_TYPE_KEY, bytes) {
                return Err(ExecutionError::Plan(format!("map {}: the graph rejected it: {reason:?}", map.id)));
            }
        }
        let stop = graph.host_graph.stop_handle();
        let serializers = graph.host_graph.value_serializers().clone();
        let plan_message = graph.plan.to_string();
        let input_ports = graph.input_ports.clone();
        let planning_ms = graph.planning_ms;
        // Made here, before the thread runs, so a stop at any point makes it readable.
        let inbound = graph.host_graph.inbound_fd().map_err(|error| ExecutionError::Execute(format!("graph inbound fd: {error}")))?;
        let pipeline = pipeline_name(&workload.workload_id);
        let run = next_run_id();
        let publisher = Publisher { bus: bus.clone(), pipeline: pipeline.clone(), run };
        let feeds = PipelineFeeds::new(bus.clone(), &workload.bindings);
        let map_pose_prefix = graph.output_ports.iter().any(|port| port == crate::nt4::robot_pose::MULTI_TAG_POSE_PORT);
        let nt4 = Nt4Output { sink: nt4, live, pipeline: pipeline.clone(), serializers: serializers.clone(), map_pose_prefix };
        let recorder = TickRecorder::new(&graph, shared.clone(), nt4, publisher);
        let graph_loop = GraphLoop { recorder, graph, inbound, frame_inputs, feeds, shared: shared.clone(), stop: stop.clone(), cpu: workload.cpu, nice: workload.nice };
        // Registered before the thread runs: a consumer that starts with this pipeline waits for its
        // first tick (`PipelineFeeds::ready`), instead of taking it as absent.
        bus.register(&pipeline, run);
        let thread = std::thread::Builder::new().name(format!("helios-graph-{}", workload.workload_id)).spawn(move || graph_loop.run(trigger)).map_err(|error| {
            bus.withdraw(&pipeline, run);
            ExecutionError::Execute(format!("failed to start workload driver thread: {error}"))
        })?;
        Ok(Self {
            workload,
            sources,
            pipeline,
            run,
            bus,
            shared,
            host,
            stop,
            serializers,
            plan_message,
            planning_ms,
            started_at_ms,
            rendered: BTreeMap::new(),
            metrics_rendered: None,
            revisions: BTreeMap::new(),
            input_ports,
            camera_context: BTreeMap::new(),
            thread: Some(thread),
        })
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

    /// Push the camera context of `workload`'s frame bindings (calibration, mount) into the
    /// graph's held structured host inputs (`camera`, `extrinsics`, `<input>_camera`, ...; see
    /// `camera_context`) whose value changed. Every tick after a push sees the new value; a push
    /// never ticks the graph. Values the graph has no host input for are ignored.
    /// Raise the graph's `save` input for a new save request (`map.save` changed): the graph saves
    /// its map on the next tick and gives `saved_map`. The request never restarts the graph.
    pub fn update_map(&mut self, workload: &ExecutionWorkload) {
        // Pushed under the state lock: `TickRecorder::record` lowers `save` under the same lock, so
        // a lowering cannot overtake this raise.
        let mut state = lock(&self.shared.state);
        if let Some(value) = state.save.request(workload.map_save)
            && let FeedOutcome::Rejected(reason) = self.host.push(SAVE_PORT, value)
        {
            drop(state);
            self.record_error(format!("save request rejected: {reason:?}"));
        }
    }

    pub fn update_camera_context(&mut self, workload: &ExecutionWorkload) {
        let primary = self.sources.first().map(|source| source.input.clone());
        for binding in workload.bindings.iter().filter(|binding| self.sources.iter().any(|source| source.input == binding.input)) {
            let pushes = match camera_context::context_pushes(binding, primary.as_deref() == Some(binding.input.as_str())) {
                Ok(pushes) => pushes,
                Err(error) => {
                    self.record_error(format!("camera context of {}: {error}", binding.input));
                    continue;
                }
            };
            for push in pushes {
                if !self.input_ports.contains(&push.port) || self.camera_context.get(&push.port) == Some(&push.value) {
                    continue;
                }
                let outcome = match push.feed(&self.host) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        self.record_error(format!("camera context {}: {error}", push.port));
                        continue;
                    }
                };
                if matches!(outcome, daedalus::transport::FeedOutcome::Rejected(_)) {
                    self.record_error(format!("camera context {} rejected: {outcome:?}", push.port));
                    continue;
                }
                self.camera_context.insert(push.port, push.value);
            }
        }
        self.workload.bindings.clone_from(&workload.bindings);
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

    #[cfg(test)]
    pub fn host(&self) -> &HostBridgeHandle {
        &self.host
    }

    /// The session and artifacts to publish now: the latest value of each host output (each
    /// rendered once), the driver's stats, the plan and, when enabled, the graph metrics.
    /// Write the full plan to `dir/<workload>.json` and publish only its summary (counts, host
    /// ports) and the file's path: the plan's text is tens of KB that never changes, and every
    /// publication would carry it.
    pub fn externalize_plan(&mut self, dir: &std::path::Path) {
        let Ok(mut plan) = serde_json::from_str::<serde_json::Value>(&self.plan_message) else { return };
        let path = dir.join(format!("{}.json", crate::execution::outputs::sanitize_id_component(&self.workload.workload_id)));
        let written = std::fs::create_dir_all(dir).and_then(|()| {
            let partial = path.with_extension("json.partial");
            std::fs::write(&partial, &self.plan_message)?;
            std::fs::rename(&partial, &path)
        });
        match written {
            Ok(()) => {
                if let Some(fields) = plan.as_object_mut() {
                    fields.remove("plan");
                    fields.insert("plan_file".into(), serde_json::Value::String(path.display().to_string()));
                }
                self.plan_message = plan.to_string();
            }
            Err(error) => tracing::warn!(path = %path.display(), %error, "could not write the plan file; publishing the plan inline"),
        }
    }

    pub fn snapshot(&mut self) -> (ExecutionSessionState, Vec<ExecutionArtifactRecord>) {
        let (stats, outputs, metrics) = {
            let mut state = lock(&self.shared.state);
            // The rate decays to zero when frames stop, with no timer on the graph thread.
            let fps = state.fps.fps();
            if state.stats.fps != fps {
                state.stats.fps = fps;
                state.stats.updated_at_ms = now_ms();
            }
            // Tens of KB of JSON, rendered under the lock the graph thread takes each tick: at
            // most once per METRICS_RENDER_INTERVAL, the last rendering in between.
            let due = self.metrics_rendered.as_ref().is_none_or(|(at, _, _)| at.elapsed() >= METRICS_RENDER_INTERVAL);
            let metrics = (state.metrics.enabled() && due).then(|| state.metrics.to_json());
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
            self.metrics_rendered = Some((Instant::now(), metrics.to_string(), observed_at_ms));
        }
        if let Some((_, metrics, metrics_at_ms)) = &self.metrics_rendered {
            artifacts.push(session_artifact(&self.workload, "metrics", METRICS_ARTIFACT_KIND, metrics.clone(), *metrics_at_ms));
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
        // After the join: no tick publishes once this pipeline's values are withdrawn.
        self.bus.withdraw(&self.pipeline, self.run);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Records each tick's outputs, stats and metrics for the publishing loop.
struct TickRecorder {
    output_ports: Vec<String>,
    /// The graph's host, to lower `save` after a save.
    host: HostBridgeHandle,
    shared: Arc<DriverShared>,
    nt4: Nt4Output,
    publisher: Publisher,
    /// When to read Daedalus's frame-overhead report next (`None`: not recorded).
    next_overhead: Option<Instant>,
}

impl TickRecorder {
    fn new(graph: &CompiledWorkloadGraph, shared: Arc<DriverShared>, nt4: Nt4Output, publisher: Publisher) -> Self {
        let next_overhead = graph.host_graph.frame_overhead_enabled().then(Instant::now);
        Self { output_ports: graph.output_ports.clone(), host: graph.host_graph.host().clone(), shared, nt4, publisher, next_overhead }
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
        // NetworkTables and the live socket get every tick's outputs (frames are not sent); only
        // while connected (the payloads are shared, not copied).
        // The payload outputs, shared: for NetworkTables, the live socket and the pipeline bus.
        let payload_outputs: Vec<(String, Payload)> = outputs
            .iter()
            .filter_map(|(port, value)| match value {
                OutputValue::Payload(payload) => Some((port.clone(), payload.clone())),
                OutputValue::Message(_) => None,
            })
            .collect();
        let nt4_outputs = (self.nt4.sink.active() || self.nt4.live.active()).then(|| payload_outputs.clone());
        let saved = outputs.iter().any(|(port, _)| port == SAVED_MAP_PORT);
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
            // Every tick is progress, whatever outputs it had: a consumer waits on the tick.
            self.publisher.publish(state.stats.last_frame_timestamp, payload_outputs);
            // The saved map is up to a megabyte: it reaches the live socket and the pipeline bus, not
            // the latest outputs that Orion publishes.
            state.outputs.extend(outputs.into_iter().filter(|(port, _)| port != SAVED_MAP_PORT).map(|(port, value)| (port, LatestOutput { sequence, updated_at_ms, value })));
            // The save's edge was taken by this tick: lower `save`, so the next request is an edge.
            if saved && let Some(value) = state.save.saved() {
                self.host.push(SAVE_PORT, value);
            }
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
                    // Logged once per run of failures (the first after a good tick): every tick
                    // of a failing graph would flood the journal.
                    if state.stats.last_error.is_none() {
                        tracing::warn!(pipeline = %self.nt4.pipeline, failed = state.stats.ticks_failed + 1, %error, "graph tick failed");
                    }
                    state.stats.ticks_failed += 1;
                    let message = format!("graph tick failed: {error}");
                    state.stats.last_failure = Some(message.clone());
                    state.stats.last_error = Some(message);
                }
            }
            if let Some(report) = frame_overhead {
                state.metrics.set_frame_overhead(&report);
            }
            if let Some(outputs) = nt4_outputs {
                self.nt4.offer(outputs, state.stats.last_tick_ms, state.stats.fps, state.stats.last_frame_timestamp);
            }
        }
        self.shared.outputs_ready.notify_one();
    }
}

/// Publishes a producer's ticks on the pipeline bus (`pipeline_bus`).
struct Publisher {
    bus: PipelineBus,
    pipeline: String,
    run: u64,
}

impl Publisher {
    fn publish(&self, frame_ts_ns: Option<u64>, outputs: Vec<(String, Payload)>) {
        self.bus.publish_tick(&self.pipeline, self.run, frame_ts_ns, outputs);
    }
}

/// Where a workload's ticks go besides Orion: NetworkTables (`crate::nt4`) and the live socket
/// (`crate::live`).
#[derive(Clone, Default)]
pub struct OutputSinks {
    pub nt4: Nt4Sink,
    pub live: crate::live::LiveSink,
}

impl From<Nt4Sink> for OutputSinks {
    fn from(nt4: Nt4Sink) -> Self {
        Self { nt4, live: crate::live::LiveSink::default() }
    }
}

/// A workload's outputs on NetworkTables (`crate::nt4`) and the live socket (`crate::live`).
struct Nt4Output {
    sink: Nt4Sink,
    live: crate::live::LiveSink,
    pipeline: String,
    serializers: ValueSerializerMap,
    /// The graph also outputs `multi_tag_pose` (see [`Nt4Tick::map_pose_prefix`]).
    map_pose_prefix: bool,
}

impl Nt4Output {
    fn offer(&self, outputs: Vec<(String, Payload)>, latency_ms: f64, fps: f64, frame_ts_ns: Option<u64>) {
        if self.live.active() {
            self.live.offer(crate::live::LiveTick { pipeline: self.pipeline.clone(), outputs: outputs.clone(), serializers: self.serializers.clone(), frame_ts_ns });
        }
        if self.sink.active() {
            self.sink.offer(Nt4Tick { pipeline: self.pipeline.clone(), outputs, serializers: self.serializers.clone(), latency_ms, fps, map_pose_prefix: self.map_pose_prefix });
        }
    }
}

/// The pipeline a workload is, as helios-api names it (`pipeline.<id>` -> `<id>`).
fn pipeline_name(workload_id: &str) -> String {
    workload_id.strip_prefix("pipeline.").unwrap_or(workload_id).to_string()
}

/// The graph thread.
struct GraphLoop {
    graph: CompiledWorkloadGraph,
    recorder: TickRecorder,
    /// The graph's inbound fd, made before the thread starts, so a stop always reaches it.
    inbound: InboundFd,
    /// Frame inputs, the primary (pacing) one first.
    frame_inputs: Vec<String>,
    /// The graph's inputs from other pipelines, synced before each frame's tick.
    feeds: PipelineFeeds,
    shared: Arc<DriverShared>,
    stop: HostGraphStopHandle,
    /// The core to pin this thread to (the workload's `cpu`), before anything runs on it.
    cpu: Option<u32>,
    /// The thread's nice value (the workload's `nice`), set before anything runs on it.
    nice: Option<u32>,
}

impl GraphLoop {
    fn run(self, trigger: Trigger) {
        self.place_thread();
        match trigger {
            Trigger::Once => self.run_once(),
            Trigger::Resources => self.run_polled(Vec::new()),
            Trigger::Frames(sources) => self.run_polled(sources),
        }
    }

    /// Place this thread before anything runs on it: pin it to its core (`cpu`) and set its nice
    /// value (`nice`), and report what was applied (`cpu`, `cpu_error`, `nice`, `nice_error` in the
    /// stats). A thread that could not be placed runs anyway, where the scheduler puts it.
    fn place_thread(&self) {
        if self.cpu.is_none() && self.nice.is_none() {
            return;
        }
        let pinned = self.cpu.map(|cpu| (cpu, super::affinity::pin_current_thread(cpu)));
        let niced = self.nice.map(|nice| (nice, super::affinity::set_current_thread_nice(nice)));
        if let Some((cpu, Err(error))) = &pinned {
            tracing::warn!(pipeline = %self.recorder.nt4.pipeline, cpu, %error, "graph thread not pinned");
        }
        if let Some((nice, Err(error))) = &niced {
            tracing::warn!(pipeline = %self.recorder.nt4.pipeline, nice, %error, "graph thread priority not set");
        }
        {
            let mut state = lock(&self.shared.state);
            let stats = &mut state.stats;
            if let Some((cpu, outcome)) = pinned {
                (stats.cpu, stats.cpu_error) = match outcome {
                    Ok(()) => (Some(cpu), None),
                    Err(error) => (None, Some(error)),
                };
            }
            if let Some((nice, outcome)) = niced {
                (stats.nice, stats.nice_error) = match outcome {
                    Ok(()) => (Some(nice), None),
                    Err(error) => (None, Some(error)),
                };
            }
            stats.updated_at_ms = now_ms();
        }
        self.shared.outputs_ready.notify_one();
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

    /// One `poll(2)` over every camera's descriptor and the graph's inbound fd, until stopped.
    ///
    /// - A camera's descriptor is readable when it has frames or news (connected, the next
    ///   attempt due): its frames are drained with `try_next()` to the newest (older ones are
    ///   released at once), a secondary camera's into its slot, the primary camera's pushed
    ///   zero-copy with the secondaries' latest in one batch.
    /// - The inbound fd is readable when a push is pending (frames, a resource batch) or the
    ///   graph was stopped: `tick_ready()` runs one tick (Daedalus clears and re-arms the fd;
    ///   this loop never reads it). A failed tick is recorded and the loop goes on.
    ///
    /// Cameras are non-blocking reconnecting clients, so one that is not up never holds the
    /// thread; `HostGraphStopHandle::stop()` makes the inbound fd readable, so a stop ends the
    /// loop at once. Resource-driven graphs run the same loop with no cameras.
    fn run_polled(mut self, mut sources: Vec<Box<dyn FrameSource>>) {
        let mut secondary: Vec<Option<(Payload, Instant)>> = vec![None; sources.len().saturating_sub(1)];
        // The primary camera's newest frame, held while the pipeline inputs are not aligned to it.
        let mut waiting: Option<WaitingFrame> = None;
        // The loop owns the feeds, so a push can sync them while the poll borrows the graph and the inbound fd.
        let mut feeds = std::mem::take(&mut self.feeds);
        if let Some(primary) = sources.first() {
            record_source_status(&self.shared, primary.status(), None);
        }
        loop {
            // `Some(index)`: camera `index` gave up and is reopened before polling again.
            let closed = 'poll: {
                let inbound_index = sources.len();
                let mut fds =
                    sources.iter().map(|source| PollFd::new(source.as_fd(), PollFlags::POLLIN)).chain(std::iter::once(PollFd::new(self.inbound.as_fd(), PollFlags::POLLIN))).collect::<Vec<_>>();
                loop {
                    if self.stop.is_stopped() {
                        return;
                    }
                    let timeout = if waiting.is_some() { PollTimeout::from(PIPELINE_WAIT_POLL_MS) } else { PollTimeout::NONE };
                    match poll(&mut fds, timeout) {
                        Ok(_) | Err(Errno::EINTR) => {}
                        Err(error) => {
                            self.recorder.record(&self.graph.host_graph, Err(format!("poll: {error}")));
                            return;
                        }
                    }
                    if self.stop.is_stopped() {
                        return;
                    }
                    let readable = |index: usize| fds[index].revents().is_some_and(|events| !events.is_empty());
                    // Secondaries first, so a primary frame in the same wake carries their newest.
                    for index in 1..sources.len() {
                        if !readable(index) {
                            continue;
                        }
                        match drain(&*sources[index]) {
                            Ok(Some(frame)) => secondary[index - 1] = Some((stream_io::framelease_payload(frame), Instant::now())),
                            Ok(None) => {}
                            Err(reason) => {
                                tracing::warn!(input = %self.frame_inputs[index], %reason, "secondary camera closed; reopening");
                                secondary[index - 1] = None;
                                break 'poll Some(index);
                            }
                        }
                    }
                    if !sources.is_empty() && readable(0) {
                        match drain(&*sources[0]) {
                            Ok(Some(frame)) => {
                                // A newer frame replaces the waiting one, and keeps its wait.
                                let since = waiting.as_ref().map_or_else(Instant::now, |held| held.since);
                                waiting = Some(WaitingFrame { timestamp: frame.meta().timestamp, frame, since });
                            }
                            Ok(None) => record_source_status(&self.shared, sources[0].status(), None),
                            Err(reason) => {
                                record_source_status(&self.shared, sources[0].status(), Some(reason));
                                break 'poll Some(0);
                            }
                        }
                    }
                    // The waiting frame goes in once the pipeline inputs are aligned to it, or once it has
                    // waited as long as they may take. Without pipeline inputs that is at once.
                    let max_wait = feeds.max_wait();
                    let mut pushed = false;
                    if let Some(held) = waiting.take_if(|held| feeds.ready(held.timestamp, held.since.elapsed() >= max_wait)) {
                        self.push(held.frame, &sources, &mut secondary, &mut feeds);
                        pushed = true;
                    }
                    if pushed || readable(inbound_index) {
                        match self.graph.host_graph.tick_ready() {
                            Ok(Some(telemetry)) => self.recorder.record(&self.graph.host_graph, Ok(&telemetry)),
                            Ok(None) => {}
                            Err(error) => self.recorder.record(&self.graph.host_graph, Err(error.to_string())),
                        }
                    }
                }
            };
            if let Some(index) = closed
                && let Err(reason) = sources[index].reopen()
            {
                // Without descriptors there is nothing to poll for this camera.
                self.recorder.record(&self.graph.host_graph, Err(reason));
                return;
            }
        }
    }

    /// Push the primary camera's `frame`, with the secondary cameras' latest frames in the same
    /// batch. The pipeline inputs are synced first: they are held, so pushing them does not tick.
    fn push(&self, frame: FrameLease, sources: &[Box<dyn FrameSource>], secondary: &mut [Option<(Payload, Instant)>], feeds: &mut PipelineFeeds) {
        let timestamp = frame.meta().timestamp;
        let size = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
        let status = sources[0].status();
        {
            let mut state = lock(&self.shared.state);
            state.last_input_at = Some(Instant::now());
            let stats = &mut state.stats;
            stats.frames_received += 1;
            stats.last_frame_timestamp = Some(timestamp);
            stats.last_frame_size = Some(size);
            stats.source = status;
        }
        if !feeds.is_empty() {
            for reason in feeds.sync(self.graph.host_graph.host(), timestamp) {
                record_source_status(&self.shared, sources[0].status(), Some(reason));
            }
            lock(&self.shared.state).stats.pipeline_inputs = feeds.stats();
        }
        let host = self.graph.host_graph.host();
        let payload = stream_io::framelease_payload(frame);
        if sources.len() == 1 {
            // Latest-only: a frame the graph has not taken yet is replaced (and released).
            host.feed_payload(self.frame_inputs[0].clone(), payload);
            return;
        }
        let mut batch = host.batch().push_payload(self.frame_inputs[0].clone(), payload);
        for (index, slot) in secondary.iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|(_, received_at)| received_at.elapsed() > SECONDARY_FRAME_MAX_AGE) {
                *slot = None;
            }
            if let Some((payload, _)) = slot {
                batch = batch.push_payload(self.frame_inputs[index + 1].clone(), payload.clone());
            }
        }
        if let Err(rejected) = batch.commit() {
            record_source_status(&self.shared, sources[0].status(), Some(format!("frames rejected: {rejected}")));
        }
    }
}

/// The primary camera's frame waiting for the pipeline inputs (`GraphLoop::run_polled`).
struct WaitingFrame {
    frame: FrameLease,
    timestamp: u64,
    /// When the wait began (kept while a newer frame replaces this one).
    since: Instant,
}

/// Everything `source` has now, down to the newest frame (older ones are released at once).
fn drain(source: &dyn FrameSource) -> Result<Option<FrameLease>, String> {
    let mut newest = None;
    loop {
        match source.try_frame() {
            FrameReceive::Frame(frame) => newest = Some(frame),
            FrameReceive::Idle => return Ok(newest),
            FrameReceive::Closed(reason) => return Err(reason),
        }
    }
}

/// Note the primary camera's connection state (and why it has none: `reason`, else the source's
/// last connection error); publishes when it changed.
fn record_source_status(shared: &DriverShared, status: FrameSourceStatus, reason: Option<String>) {
    let reason = reason.or_else(|| status.error.clone());
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
