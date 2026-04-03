use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver as StdReceiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use styx::codec::{Codec, CodecKind, CodecPolicy, CodecRegistry};
use styx::prelude::{BufferPool, ColorSpace, FourCc, FrameLease, FrameMeta, MediaFormat, Resolution};
use tokio::fs::{self, File};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;
use tokio::time::{sleep_until, timeout, Instant};
use uuid::Uuid;

use crate::capture::{descriptor_for_config_retrying, BackendKind, CaptureControlInfo, CaptureControlValue, CaptureDescriptor, ControlAssignment};
use crate::contracts::stream_ids::{CALIBRATION_MODE_PIPELINE_UUID, RAW_PIPELINE_UUID as RAW_STREAM_PIPELINE_UUID};
use crate::error::{Error, Result};
use crate::ipc::{normalize_pipeline_output_selection, ControlId, JsonWire, RecordingCodec, RecordingContainer, RecordingSource, ResolvedStreamConfig};
use crate::stream::{cleanup_all_stream_files, cleanup_stream_files, EncodedFrame, ShmemWriter, StreamMetrics, StreamRunner, StreamRunnerConfig};
use daedalus::planner::GraphPatch;

use super::worker::{run_stream_worker, CalibrationModeRestore, StreamCommand, StreamContext, StreamExit};

const CALIBRATION_TEMPLATE_ID: &str = "daedalus_aruco";
const UNDISTORT_TEMPLATE_ID: &str = "daedalus_undistort_preview";
const CALIBRATION_MODE_HOST_BUFFER: usize = 1;
// Stopping a recording may include MP4 finalize (ffmpeg remux/transcode) which can be slow.
const RECORDING_STOP_TIMEOUT: Duration = Duration::from_secs(180);
const SNAPSHOT_SOURCE_TIMEOUT: Duration = Duration::from_secs(3);
const SHADOW_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_RUNTIME_QUERY_TIMEOUT: Duration = Duration::from_millis(250);

static SHADOW_DATA_ROOT: OnceLock<std::result::Result<PathBuf, String>> = OnceLock::new();

#[path = "manager/calibration.rs"]
mod calibration;
#[path = "manager/file_video.rs"]
mod file_video;
#[path = "manager/graph_api.rs"]
mod graph_api;
#[path = "manager/policy.rs"]
mod policy;
#[path = "manager/recording.rs"]
mod recording;
#[path = "manager/recording_impl.rs"]
mod recording_impl;
#[path = "manager/runtime.rs"]
mod runtime;
#[cfg(test)]
#[path = "manager/tests.rs"]
mod tests;

use self::calibration::*;
use self::file_video::*;
use self::recording_impl::*;

struct ManagedEncodedConsumer {
    count: Arc<AtomicU64>,
    last_seen_ms: Arc<AtomicU64>,
}

impl ManagedEncodedConsumer {
    fn new(count: Arc<AtomicU64>, last_seen_ms: Arc<AtomicU64>) -> Self {
        count.fetch_add(1, Ordering::Relaxed);
        let consumer = Self { count, last_seen_ms };
        consumer.touch();
        consumer
    }

    fn touch(&self) {
        self.last_seen_ms.store(current_time_ms(), Ordering::Relaxed);
    }

    fn touch_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.last_seen_ms)
    }
}

impl Drop for ManagedEncodedConsumer {
    fn drop(&mut self) {
        let _ = self.count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| Some(value.saturating_sub(1)));
    }
}

#[derive(Clone, Debug, Default)]
struct RecordingStats {
    frames: u64,
    bytes: u64,
    first_ts_ms: Option<u64>,
    last_ts_ms: Option<u64>,
    raw_format: Option<RawRecordingFormat>,
}

impl RecordingStats {
    fn record_ts(&mut self, ts_ms: u64) {
        if self.frames == 0 {
            self.first_ts_ms = Some(ts_ms);
        }
        self.last_ts_ms = Some(ts_ms);
    }

    fn derived_fps(&self) -> Option<f32> {
        if self.frames <= 1 {
            return None;
        }
        let (Some(first), Some(last)) = (self.first_ts_ms, self.last_ts_ms) else {
            return None;
        };
        if last <= first {
            return None;
        }
        let span_ms = (last - first) as f32;
        let frames = (self.frames - 1) as f32;
        if span_ms <= f32::EPSILON {
            return None;
        }
        Some((frames * 1000.0) / span_ms)
    }
}

struct RecordingTimestampWriter {
    writer: std::io::BufWriter<std::fs::File>,
    last_ts_ms: Option<u64>,
}

impl RecordingTimestampWriter {
    fn open(path: &Path) -> std::result::Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("recording timestamp dir create failed: {err}"))?;
        }
        let file = std::fs::File::create(path).map_err(|err| format!("recording timestamp file open failed: {err}"))?;
        Ok(Self { writer: std::io::BufWriter::new(file), last_ts_ms: None })
    }

    fn record(&mut self, ts_ms: u64) -> std::result::Result<(), String> {
        let ts = self.last_ts_ms.map(|last| ts_ms.max(last)).unwrap_or(ts_ms);
        writeln!(self.writer, "{ts}").map_err(|err| format!("recording timestamp write failed: {err}"))?;
        self.last_ts_ms = Some(ts);
        self.writer.flush().map_err(|err| format!("recording timestamp flush failed: {err}"))?;
        Ok(())
    }

    fn flush(&mut self) -> std::result::Result<(), String> {
        self.writer.flush().map_err(|err| format!("recording timestamp flush failed: {err}"))
    }
}

#[derive(Clone, Debug)]
enum RecordingState {
    Running,
    Completed(std::result::Result<RecordingStats, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingBitstream {
    Unknown,
    AnnexB,
    LengthPrefixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawRecordingFormat {
    H264,
    H265,
    Mjpeg,
}

impl RawRecordingFormat {
    fn to_u8(self) -> u8 {
        match self {
            Self::H264 => 0,
            Self::H265 => 1,
            Self::Mjpeg => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::H264,
            1 => Self::H265,
            2 => Self::Mjpeg,
            _ => Self::H265,
        }
    }

    fn from_codec(codec: RecordingCodec) -> Self {
        match codec {
            RecordingCodec::H264 => Self::H264,
            RecordingCodec::H265 => Self::H265,
        }
    }

    fn as_codec(self) -> Option<RecordingCodec> {
        match self {
            Self::H264 => Some(RecordingCodec::H264),
            Self::H265 => Some(RecordingCodec::H265),
            Self::Mjpeg => None,
        }
    }

    fn ffmpeg_demux(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "hevc",
            Self::Mjpeg => "mjpeg",
        }
    }
}
#[derive(Debug, Default, Clone)]
struct RecordingConfigCache {
    vps: Option<Vec<u8>>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl RecordingConfigCache {
    fn ready(&self, codec: RecordingCodec) -> bool {
        match codec {
            RecordingCodec::H264 => self.sps.is_some() && self.pps.is_some(),
            RecordingCodec::H265 => self.vps.is_some() && self.sps.is_some() && self.pps.is_some(),
        }
    }

    fn prefix(&self, codec: RecordingCodec) -> Option<Vec<u8>> {
        if !self.ready(codec) {
            return None;
        }
        let mut out = Vec::new();
        match codec {
            RecordingCodec::H264 => {
                out.extend_from_slice(self.sps.as_ref()?);
                out.extend_from_slice(self.pps.as_ref()?);
            }
            RecordingCodec::H265 => {
                out.extend_from_slice(self.vps.as_ref()?);
                out.extend_from_slice(self.sps.as_ref()?);
                out.extend_from_slice(self.pps.as_ref()?);
            }
        }
        Some(out)
    }

    fn update_from_annexb(&mut self, codec: RecordingCodec, bytes: &[u8]) {
        let mut pos = 0;
        while let Some((start, nal_start)) = find_annexb_start(bytes, pos) {
            let next = find_annexb_start(bytes, nal_start).map(|(next_start, _)| next_start).unwrap_or(bytes.len());
            if nal_start < next {
                let nal = &bytes[nal_start..next];
                if !nal.is_empty() {
                    let store = |dst: &mut Option<Vec<u8>>| {
                        let mut v = Vec::with_capacity(4 + nal.len());
                        v.extend_from_slice(&[0, 0, 0, 1]);
                        v.extend_from_slice(nal);
                        *dst = Some(v);
                    };
                    match codec {
                        RecordingCodec::H264 => {
                            let nal_type = nal[0] & 0x1f;
                            if nal_type == 7 {
                                store(&mut self.sps);
                            } else if nal_type == 8 {
                                store(&mut self.pps);
                            }
                        }
                        RecordingCodec::H265 => {
                            let nal_type = (nal[0] >> 1) & 0x3f;
                            if nal_type == 32 {
                                store(&mut self.vps);
                            } else if nal_type == 33 {
                                store(&mut self.sps);
                            } else if nal_type == 34 {
                                store(&mut self.pps);
                            }
                        }
                    }
                }
            }
            pos = next.max(start + 1);
        }
    }
}

#[derive(Debug)]
struct RecordingSession {
    stop_tx: Option<oneshot::Sender<()>>,
    done_rx: watch::Receiver<RecordingState>,
    started_at_ms: u64,
}

#[derive(Debug)]
enum RecordingFrameSource {
    Multiplex { rx: Receiver<Arc<image::DynamicImage>> },
    Raw { rx: Receiver<Arc<image::DynamicImage>> },
    Pipeline { rx: Receiver<Arc<image::DynamicImage>>, graph: crate::graph::GraphHandle },
}

#[derive(Debug, Clone)]
pub struct StartRecordingParams {
    pub source: RecordingSource,
    pub output_path: String,
    pub container: RecordingContainer,
    pub codec: RecordingCodec,
    pub duration_ms: Option<u64>,
    pub settings: Option<crate::ipc::RecordingSettings>,
}

#[derive(Debug)]
struct RecordingEncoderConfig {
    output_path: PathBuf,
    codec: RecordingCodec,
    pipeline_graph: Option<crate::graph::GraphHandle>,
    encoder_hint: Option<String>,
    timestamps_path: Option<PathBuf>,
}

struct LatestFrameMailbox {
    state: Mutex<LatestFrameState>,
    cv: Condvar,
}

struct LatestFrameState {
    queue: VecDeque<(Arc<image::DynamicImage>, u64)>,
    max_frames: usize,
    stop: bool,
}

impl LatestFrameMailbox {
    fn new(max_frames: usize) -> Self {
        Self { state: Mutex::new(LatestFrameState { queue: VecDeque::new(), max_frames: max_frames.max(1), stop: false }), cv: Condvar::new() }
    }

    fn push_frame(&self, image: Arc<image::DynamicImage>, ts_ms: u64) {
        if let Ok(mut state) = self.state.lock() {
            if state.queue.len() >= state.max_frames {
                let _ = state.queue.pop_front();
            }
            state.queue.push_back((image, ts_ms));
            self.cv.notify_one();
        }
    }

    fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stop = true;
            self.cv.notify_all();
        }
    }

    fn next(&self) -> Option<(Arc<image::DynamicImage>, u64)> {
        let mut guard = self.state.lock().ok()?;
        loop {
            if let Some(frame) = guard.queue.pop_front() {
                return Some(frame);
            }
            if guard.stop {
                return None;
            }
            guard = self.cv.wait(guard).ok()?;
        }
    }
}

struct RecordingEncoderWorker {
    mailbox: Arc<LatestFrameMailbox>,
    join: Option<JoinHandle<std::result::Result<RecordingStats, String>>>,
}

struct ShadowRecorderConfig {
    shadow_dir: PathBuf,
    codec: RecordingCodec,
    segment_ms: u64,
    window_ms: u64,
    format_tracker: Arc<AtomicU8>,
}

enum ShadowRecorderJob {
    Chunk { data: Arc<[u8]>, ts_ms: u64 },
    Stop,
}

struct ShadowRecorderWorker {
    tx: SyncSender<ShadowRecorderJob>,
    join: Option<JoinHandle<std::result::Result<(), String>>>,
}

struct RecordingEncoderRuntime {
    writer: std::io::BufWriter<std::fs::File>,
    ts_writer: Option<RecordingTimestampWriter>,
    encoder: Arc<dyn Codec>,
    codec: RecordingCodec,
    raw_format: RawRecordingFormat,
    format_tracker: Option<Arc<AtomicU8>>,
    pipeline_graph: Option<crate::graph::GraphHandle>,
    pool: BufferPool,
    pool_capacity: usize,
    bitstream: RecordingBitstream,
    // Scratch buffer for length-prefixed -> AnnexB conversion to avoid per-frame allocations.
    convert_buf: Vec<u8>,
    config_cache: RecordingConfigCache,
    wrote_prefix: bool,
    pending_before_config: std::collections::VecDeque<Vec<u8>>,
    pending_bytes: usize,
    stats: RecordingStats,
    last_frame_meta: Option<FrameMeta>,
}

#[derive(Debug)]
enum ResolvedRecordingSource {
    Multiplex,
    Raw,
    Pipeline { pipeline_id: Uuid, output_key: Option<String> },
}

#[derive(Debug)]
struct ShadowRecorderHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<()>,
    requested_codec: RecordingCodec,
    raw_format: Arc<AtomicU8>,
}

/// Manages active streams and their lifecycles.
pub struct StreamManager {
    streams: Arc<RwLock<HashMap<Uuid, Arc<StreamContext>>>>,
    starting: Arc<AsyncMutex<HashSet<Uuid>>>,
    recordings: Arc<AsyncMutex<HashMap<Uuid, Arc<AsyncMutex<RecordingSession>>>>>,
    shadow_recorders: Arc<AsyncMutex<HashMap<Uuid, ShadowRecorderHandle>>>,
}

impl StreamManager {
    pub fn new() -> Self {
        runtime::new_manager()
    }

    pub async fn start_stream(&self, manifest: ResolvedStreamConfig) -> Result<(Uuid, CaptureDescriptor)> {
        runtime::start_stream(self, manifest).await
    }

    pub async fn stop_stream(&self, stream_id: Uuid) -> Result<()> {
        runtime::stop_stream(self, stream_id).await
    }

    pub async fn set_control(&self, stream_id: Uuid, control_id: ControlId, value: CaptureControlValue) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let is_file_backend = {
            let manifest = ctx.manifest.read().await;
            manifest.capture.backend == BackendKind::File
        };
        if is_file_backend {
            let (controls, enable_tdn_output) = {
                let manifest = ctx.manifest.read().await;
                let mut controls = with_control_assignment(&manifest.capture.controls, control_id, value.clone());
                sanitize_file_video_frame_controls(&ctx.descriptor, &mut controls);
                (controls, manifest.capture.enable_tdn_output)
            };
            let controls_for_sync = controls.clone();
            self.send_command(stream_id, move |respond_to| StreamCommand::SyncCaptureControls { controls: controls_for_sync, enable_tdn_output, respond_to }).await?;
            {
                let mut manifest = ctx.manifest.write().await;
                manifest.capture.controls = controls;
            }
            return Ok(());
        }

        let value_clone = value.clone();
        let result = self.send_command(stream_id, move |respond_to| StreamCommand::SetControl { control_id, value, respond_to }).await;
        if result.is_ok() {
            let (controls, enable_tdn_output) = {
                let mut manifest = ctx.manifest.write().await;
                manifest.capture.controls.retain(|ctl| ctl.id != control_id);
                if !matches!(value_clone, CaptureControlValue::None) {
                    manifest.capture.controls.push(crate::capture::ControlAssignment { id: control_id, value: value_clone.clone() });
                }
                (manifest.capture.controls.clone(), manifest.capture.enable_tdn_output)
            };
            self.send_command(stream_id, move |respond_to| StreamCommand::SyncCaptureControls { controls, enable_tdn_output, respond_to }).await?;
        }
        result
    }

    pub async fn get_controls(&self, stream_id: Uuid) -> Result<Vec<CaptureControlInfo>> {
        self.send_command(stream_id, |respond_to| StreamCommand::GetControls { respond_to }).await
    }

    pub async fn get_metrics(&self, stream_id: Uuid) -> Result<StreamMetrics> {
        self.send_command(stream_id, |respond_to| StreamCommand::GetMetrics { respond_to }).await
    }

    pub async fn snapshot_jpeg(&self, stream_id: Uuid, quality: u8, source: Option<RecordingSource>) -> Result<Vec<u8>> {
        let Some(source) = source else {
            return self.send_command(stream_id, move |respond_to| StreamCommand::SnapshotJpeg { quality, respond_to }).await;
        };

        let ctx = self.get_stream(stream_id).await?;
        let manifest_snapshot = ctx.manifest.read().await.clone();
        let resolved_source = resolve_recording_source(&manifest_snapshot, source);
        let image = match resolved_source {
            ResolvedRecordingSource::Multiplex => {
                let host = ctx.host.read().await.clone();
                let mut rx = host.subscribe();
                receive_snapshot_source_frame(&mut rx, None).await?
            }
            ResolvedRecordingSource::Raw => {
                let mut rx = ctx.raw_tx.subscribe();
                receive_snapshot_source_frame(&mut rx, None).await?
            }
            ResolvedRecordingSource::Pipeline { pipeline_id, output_key } => {
                let graph = build_recording_pipeline_graph(&manifest_snapshot, pipeline_id, output_key.as_deref())?;
                let mut rx = ctx.raw_tx.subscribe();
                receive_snapshot_source_frame(&mut rx, Some(&graph)).await?
            }
        };

        encode_snapshot_jpeg(&image, quality)
    }

    pub async fn start_recording(&self, stream_id: Uuid, params: StartRecordingParams) -> Result<()> {
        recording::start_recording(self, stream_id, params).await
    }

    pub async fn stop_recording(&self, stream_id: Uuid) -> Result<()> {
        recording::stop_recording(self, stream_id).await
    }

    pub async fn capture_shadow_recording(&self, stream_id: Uuid, output_path: String, container: RecordingContainer, window_ms: u64) -> Result<()> {
        recording::capture_shadow_recording(self, stream_id, output_path, container, window_ms).await
    }

    async fn start_shadow_recorder(&self, stream_id: Uuid, manifest: &ResolvedStreamConfig) -> Result<()> {
        recording::start_shadow_recorder(self, stream_id, manifest).await
    }

    async fn stop_shadow_recorder(&self, stream_id: Uuid) -> Result<()> {
        recording::stop_shadow_recorder(self, stream_id).await
    }

    pub async fn set_codecs(&self, stream_id: Uuid, decoder_id: Option<String>, encoder_id: Option<String>) -> Result<()> {
        self.send_command(stream_id, move |respond_to| StreamCommand::SetCodecs { decoder_id, encoder_id, respond_to }).await
    }

    pub async fn list_streams(&self) -> Vec<crate::ipc::StreamSummary> {
        let entries: Vec<(Uuid, Arc<StreamContext>)> = {
            let streams = self.streams.read().await;
            streams.iter().map(|(id, ctx)| (*id, Arc::clone(ctx))).collect()
        };
        let recording_started = {
            let sessions: Vec<(Uuid, Arc<AsyncMutex<RecordingSession>>)> = {
                let recordings = self.recordings.lock().await;
                recordings.iter().map(|(id, session)| (*id, Arc::clone(session))).collect()
            };
            let mut started = HashMap::new();
            for (id, session) in sessions {
                let session = session.lock().await;
                started.insert(id, session.started_at_ms);
            }
            started
        };
        let mut out = Vec::with_capacity(entries.len());
        for (id, ctx) in entries {
            let mut manifest = ctx.manifest.read().await.clone();
            for binding in &mut manifest.pipelines {
                binding.pipeline_graph = None;
            }
            let host = ctx.host.read().await;
            let graph_state = host.disabled_state();
            let mut runtime = query_stream_runtime_state(&ctx).await;
            runtime.capture.started_at_ms = Some(ctx.stream_started_at_ms);
            runtime.capture.state = if graph_state.disabled {
                crate::ipc::StreamCaptureState::Disabled
            } else if runtime.capture.state == crate::ipc::StreamCaptureState::Running {
                crate::ipc::StreamCaptureState::Running
            } else {
                crate::ipc::StreamCaptureState::Stopped
            };
            runtime.capture.disabled_since_ms = graph_state.disabled_since_ms;
            runtime.capture.disabled_reason = graph_state.disabled_reason.clone();
            runtime.pipeline.enabled = manifest.pipeline_enabled;
            runtime.pipeline.active_pipeline_id = manifest.active_pipeline_id;
            runtime.pipeline.active_output_key = manifest.active_pipeline_output.clone();
            runtime.pipeline.pipeline_count = manifest.pipelines.len() as u64;
            runtime.pipeline.disabled = graph_state.disabled;
            runtime.pipeline.disabled_since_ms = graph_state.disabled_since_ms;
            runtime.pipeline.disabled_reason = graph_state.disabled_reason;
            if let Some(started_at) = recording_started.get(&id).copied() {
                runtime.recording.state = crate::ipc::StreamRecordingState::Active;
                runtime.recording.started_at_ms = Some(started_at);
            }
            let status = runtime.status();
            out.push(crate::ipc::StreamSummary { stream_id: id, descriptor: ctx.descriptor.clone(), manifest, status, runtime });
        }
        out
    }

    async fn get_stream(&self, stream_id: Uuid) -> Result<Arc<StreamContext>> {
        let streams = self.streams.read().await;
        streams.get(&stream_id).cloned().ok_or(Error::NotFound("stream not found"))
    }

    async fn send_command<T, F>(&self, stream_id: Uuid, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(oneshot::Sender<Result<T>>) -> StreamCommand + Send + 'static,
    {
        let ctx = self.get_stream(stream_id).await?;
        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, f(tx))?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))?
    }

    async fn finish_recording(&self, stream_id: Uuid, result: std::result::Result<RecordingStats, String>) {
        {
            let mut recordings = self.recordings.lock().await;
            recordings.remove(&stream_id);
        }
        if let Err(err) = result {
            tracing::warn!(stream_id = %stream_id, error = %err, "recording finished with error");
        }
    }

    async fn finish_starting(&self, stream_id: Uuid) {
        let mut starting = self.starting.lock().await;
        starting.remove(&stream_id);
    }
}

fn reconcile_manifest_pipeline_references(manifest: &mut ResolvedStreamConfig) {
    let mut known_pipeline_ids: HashSet<Uuid> = manifest.pipelines.iter().map(|binding| binding.pipeline_id).collect();
    // RAW may be referenced in layout/wires without a persisted binding.
    known_pipeline_ids.insert(RAW_STREAM_PIPELINE_UUID);

    if manifest.active_pipeline_id.is_some_and(|id| !known_pipeline_ids.contains(&id)) {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for slot in &mut layout.slots {
            let Some(pipeline_id) = slot.pipeline_id else { continue };
            if known_pipeline_ids.contains(&pipeline_id) {
                continue;
            }
            slot.pipeline_id = None;
            slot.output_key = None;
        }
    }

    manifest.pipeline_wires.retain(|wire| known_pipeline_ids.contains(&wire.from.pipeline_id) && known_pipeline_ids.contains(&wire.to.pipeline_id));

    if manifest.active_pipeline_id.is_none() {
        manifest.active_pipeline_id = manifest.pipelines.first().map(|binding| binding.pipeline_id);
        if manifest.active_pipeline_id.is_none() {
            manifest.active_pipeline_output = None;
        }
    }
}

async fn query_stream_runtime_state(ctx: &StreamContext) -> crate::ipc::StreamRuntimeState {
    let (tx, rx) = oneshot::channel();
    if let Err(err) = enqueue_stream_command(&ctx.command_tx, StreamCommand::GetRuntimeState { respond_to: tx }) {
        tracing::debug!(error = %err, "stream runtime state unavailable");
        return crate::ipc::StreamRuntimeState::default();
    }
    match timeout(STREAM_RUNTIME_QUERY_TIMEOUT, rx).await {
        Ok(Ok(Ok(runtime))) => runtime,
        Ok(Ok(Err(err))) => {
            tracing::debug!(error = %err, "stream runtime state query failed");
            crate::ipc::StreamRuntimeState::default()
        }
        Ok(Err(_)) => {
            tracing::debug!("stream runtime state query channel dropped");
            crate::ipc::StreamRuntimeState::default()
        }
        Err(_) => {
            tracing::debug!("stream runtime state query timed out");
            crate::ipc::StreamRuntimeState::default()
        }
    }
}

fn enqueue_stream_command(tx: &SyncSender<StreamCommand>, command: StreamCommand) -> Result<()> {
    match tx.try_send(command) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(Error::InvalidState("stream worker command queue full")),
        Err(TrySendError::Disconnected(_)) => Err(Error::InvalidState("stream worker stopped")),
    }
}

async fn abort_unregistered_stream_worker(stream_id: Uuid, command_tx: SyncSender<StreamCommand>, join: JoinHandle<()>) {
    let (respond_to, rx) = oneshot::channel();
    if enqueue_stream_command(&command_tx, StreamCommand::Stop { respond_to }).is_ok() {
        let _ = tokio::time::timeout(Duration::from_secs(2), rx).await;
    }
    let _ = tokio::task::spawn_blocking(move || join.join()).await;
    cleanup_stream_files(stream_id);
}

impl Clone for StreamManager {
    fn clone(&self) -> Self {
        Self { streams: Arc::clone(&self.streams), starting: Arc::clone(&self.starting), recordings: Arc::clone(&self.recordings), shadow_recorders: Arc::clone(&self.shadow_recorders) }
    }
}

impl Default for StreamManager {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_alias(alias: Option<&str>) -> Option<String> {
    alias.map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_ascii_lowercase())
}

fn single_view_slot_pipeline_id(manifest: &ResolvedStreamConfig) -> Option<Uuid> {
    let layout = manifest.pipeline_layout.as_ref()?;
    if layout.rows != 1 || layout.columns != 1 {
        return None;
    }
    layout.slots.iter().find(|slot| slot.row == 0 && slot.column == 0).and_then(|slot| slot.pipeline_id).or_else(|| layout.slots.iter().find_map(|slot| slot.pipeline_id))
}

fn normalize_output_for_pipeline(output: Option<String>, pipeline_id: Option<Uuid>) -> std::result::Result<Option<String>, String> {
    normalize_pipeline_output_selection(output.as_deref(), pipeline_id)
}
