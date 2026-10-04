//! Frame-driven workloads: a dedicated thread per workload blocks on its camera service, feeds
//! each `FrameLease` to the resident graph, ticks it and keeps the latest outputs.
//!
//! The thread never waits on Orion. The engine's execution tick only snapshots the latest
//! outputs and stats (latest wins), so publication is bounded by
//! `HELIOS_ENGINE_EXECUTION_INTERVAL_MS` however fast the camera runs. Leases are released as
//! soon as the tick that consumed them returns; frame outputs are described and dropped at once.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use daedalus::{runtime::host_bridge::ValueSerializerMap, transport::Payload};
use styx::imports::framelease::FrameLease;

use super::{
    ExecutionError,
    bindings::{FrameSourceSpec, ResourceInput},
    frame_source::{FrameReceive, FrameSource, StyxFrameSource},
    graph::CompiledWorkloadGraph,
    outputs::{output_artifact, payload_message, session_id_for, telemetry_artifact},
    stats::{FpsWindow, FrameDriverStats},
};
use crate::{
    model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload},
    stream_io,
};

/// How long the driver blocks for a frame before checking whether it should stop.
const FRAME_RECV_TIMEOUT: Duration = Duration::from_millis(200);
/// A secondary camera's last frame is reused for at most this long (Styx servers reclaim held
/// leases after ~2 s).
const SECONDARY_FRAME_MAX_AGE: Duration = Duration::from_millis(500);

/// State the driver thread shares with the engine's publishing loop.
#[derive(Default)]
struct DriverShared {
    state: Mutex<DriverState>,
    /// Latest JSON of the workload's non-frame bindings, pushed with every frame.
    context: Mutex<BTreeMap<String, String>>,
}

#[derive(Default)]
struct DriverState {
    stats: FrameDriverStats,
    outputs: BTreeMap<String, LatestOutput>,
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

pub(crate) struct FrameDrivenExecution {
    pub workload: ExecutionWorkload,
    pub sources: Vec<FrameSourceSpec>,
    shared: Arc<DriverShared>,
    serializers: ValueSerializerMap,
    planning_ms: f64,
    started_at_ms: u64,
    rendered: BTreeMap<String, (u64, String)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FrameDrivenExecution {
    /// Start driving `graph` from the Styx camera services named by `sources`.
    pub fn start(workload: ExecutionWorkload, graph: CompiledWorkloadGraph, sources: Vec<FrameSourceSpec>) -> Result<Self, ExecutionError> {
        let frame_sources = sources.iter().cloned().map(|spec| Box::new(StyxFrameSource::new(spec)) as Box<dyn FrameSource>).collect();
        Self::spawn(workload, graph, sources, frame_sources)
    }

    /// Start driving `graph` from `frame_sources` (one per spec, in order; the first one paces
    /// the graph).
    pub fn spawn(workload: ExecutionWorkload, graph: CompiledWorkloadGraph, sources: Vec<FrameSourceSpec>, frame_sources: Vec<Box<dyn FrameSource>>) -> Result<Self, ExecutionError> {
        if sources.is_empty() || sources.len() != frame_sources.len() {
            return Err(ExecutionError::Execute("frame-driven workload needs one frame source per frame binding".into()));
        }
        for spec in &sources {
            graph.host_graph.set_latest_input(spec.input.clone()).map_err(|error| ExecutionError::Plan(format!("frame input '{}': {error}", spec.input)))?;
        }
        let started_at_ms = now_ms();
        let shared = Arc::new(DriverShared::default());
        shared.lock_state().stats.updated_at_ms = started_at_ms;
        let stop = Arc::new(AtomicBool::new(false));
        let serializers = graph.host_graph.value_serializers().clone();
        let planning_ms = graph.planning_ms;
        let driver = Driver { graph, inputs: sources.iter().map(|spec| spec.input.clone()).collect(), sources: frame_sources, shared: shared.clone(), stop: stop.clone() };
        let thread = std::thread::Builder::new()
            .name(format!("helios-frames-{}", workload.workload_id))
            .spawn(move || driver.run())
            .map_err(|error| ExecutionError::Execute(format!("failed to start frame driver thread: {error}")))?;
        Ok(Self { workload, sources, shared, serializers, planning_ms, started_at_ms, rendered: BTreeMap::new(), stop, thread: Some(thread) })
    }

    /// Replace the non-frame inputs pushed alongside each frame.
    pub fn update_context(&self, inputs: Vec<ResourceInput>) {
        let context = inputs.into_iter().map(|input| (input.input, input.payload)).collect();
        *self.shared.context.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = context;
    }

    #[cfg(test)]
    pub fn stats(&self) -> FrameDriverStats {
        self.shared.lock_state().stats.clone()
    }

    /// The session and artifacts to publish now: the latest value of each host output (each
    /// rendered once) and the driver's stats.
    pub fn snapshot(&mut self) -> (ExecutionSessionState, Vec<ExecutionArtifactRecord>) {
        let (stats, outputs) = {
            let state = self.shared.lock_state();
            (state.stats.clone(), state.outputs.clone())
        };
        let mut artifacts = Vec::with_capacity(outputs.len() + 1);
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
        let mut report = stats.to_json();
        report["planning_ms"] = serde_json::json!(self.planning_ms);
        report["frame_driven"] = serde_json::json!(true);
        report["sources"] = self.sources.iter().map(|spec| serde_json::json!({ "input": spec.input, "resource_id": spec.resource_id, "socket": spec.socket_path })).collect();
        let observed_at_ms = stats.updated_at_ms.max(self.started_at_ms);
        artifacts.push(telemetry_artifact(&self.workload, report.to_string(), observed_at_ms));
        let status = if stats.frames_processed > 0 { ExecutionSessionStatus::Running } else { ExecutionSessionStatus::Starting };
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

impl Drop for FrameDrivenExecution {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!(workload_id = %self.workload.workload_id, "frame driver thread panicked");
        }
    }
}

impl DriverShared {
    fn lock_state(&self) -> MutexGuard<'_, DriverState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

struct Driver {
    graph: CompiledWorkloadGraph,
    inputs: Vec<String>,
    sources: Vec<Box<dyn FrameSource>>,
    shared: Arc<DriverShared>,
    stop: Arc<AtomicBool>,
}

impl Driver {
    fn run(mut self) {
        let mut fps = FpsWindow::default();
        let mut secondary: Vec<Option<(Payload, Instant)>> = vec![None; self.sources.len() - 1];
        let mut sequence = 0_u64;
        while !self.stop.load(Ordering::Acquire) {
            let received = self.sources[0].recv(FRAME_RECV_TIMEOUT);
            let frame = match received {
                FrameReceive::Frame(frame) => frame,
                FrameReceive::Idle => {
                    self.record_idle(&mut fps, None);
                    continue;
                }
                FrameReceive::Unavailable(reason) => {
                    self.record_idle(&mut fps, Some(reason));
                    continue;
                }
            };
            for (index, source) in self.sources.iter_mut().enumerate().skip(1) {
                while let FrameReceive::Frame(newer) = source.recv(Duration::ZERO) {
                    secondary[index - 1] = Some((stream_io::framelease_payload(newer), Instant::now()));
                }
            }
            sequence += 1;
            self.process(frame, &mut secondary, &mut fps, sequence);
        }
    }

    fn process(&mut self, frame: FrameLease, secondary: &mut [Option<(Payload, Instant)>], fps: &mut FpsWindow, sequence: u64) {
        let started_at = Instant::now();
        let frame_timestamp = frame.meta().timestamp;
        let frame_size = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
        let context = self.shared.context.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        for (port, value) in context {
            self.graph.host_graph.push(port, value);
        }
        for (slot, input) in secondary.iter_mut().zip(self.inputs.iter().skip(1)) {
            if slot.as_ref().is_some_and(|(_, received_at)| received_at.elapsed() > SECONDARY_FRAME_MAX_AGE) {
                *slot = None;
            }
            if let Some((payload, _)) = slot {
                self.graph.host_graph.push_payload(input.clone(), payload.clone());
            }
        }
        self.graph.host_graph.push_payload(self.inputs[0].clone(), stream_io::framelease_payload(frame));
        let result = self.graph.host_graph.tick();

        let now = now_ms();
        let mut outputs = Vec::new();
        for port in &self.graph.output_ports {
            if let Some(payload) = self.graph.host_graph.drain_payloads(port).pop() {
                let value = match payload.get_ref::<FrameLease>() {
                    Some(frame) => OutputValue::Message(stream_io::framelease_descriptor_json(frame).to_string()),
                    None => OutputValue::Payload(payload),
                };
                outputs.push((port.clone(), LatestOutput { sequence, updated_at_ms: now, value }));
            }
        }
        let tick_ms = started_at.elapsed().as_secs_f64() * 1000.0;
        fps.record_frame();
        let status = self.sources[0].status();

        let mut state = self.shared.lock_state();
        state.outputs.extend(outputs);
        let stats = &mut state.stats;
        stats.frames_received += 1;
        stats.last_tick_ms = tick_ms;
        stats.last_frame_timestamp = Some(frame_timestamp);
        stats.last_frame_size = Some(frame_size);
        stats.fps = fps.fps();
        stats.source = status;
        stats.updated_at_ms = now;
        match result {
            Ok(_) => {
                stats.frames_processed += 1;
                stats.last_error = None;
            }
            Err(error) => {
                stats.frames_failed += 1;
                stats.last_error = Some(format!("graph tick failed: {error}"));
            }
        }
    }

    fn record_idle(&self, fps: &mut FpsWindow, reason: Option<String>) {
        let status = self.sources[0].status();
        let fps = fps.idle();
        let mut state = self.shared.lock_state();
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
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
