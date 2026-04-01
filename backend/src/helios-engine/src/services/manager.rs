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

use crate::capture::{BackendKind, CaptureControlInfo, CaptureControlValue, CaptureDescriptor, ControlAssignment};
use crate::error::{Error, Result};
use crate::ipc::{ControlId, JsonWire, RecordingCodec, RecordingContainer, RecordingSource, ResolvedStreamConfig};
use crate::stream::{cleanup_all_stream_files, cleanup_stream_files, EncodedFrame, ShmemWriter, StreamMetrics, StreamRunner, StreamRunnerConfig};
use daedalus::planner::GraphPatch;

use super::worker::{run_stream_worker, CalibrationModeRestore, StreamCommand, StreamContext, StreamExit};

const CALIBRATION_TEMPLATE_ID: &str = "daedalus_aruco";
const UNDISTORT_TEMPLATE_ID: &str = "daedalus_undistort_preview";
const CALIBRATION_MODE_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_00000000c411);
const CALIBRATION_MODE_HOST_BUFFER: usize = 1;
const RAW_STREAM_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);
// Stopping a recording may include MP4 finalize (ffmpeg remux/transcode) which can be slow.
const RECORDING_STOP_TIMEOUT: Duration = Duration::from_secs(180);
const SNAPSHOT_SOURCE_TIMEOUT: Duration = Duration::from_secs(3);
const RECORDING_STOP_GRACE_DEFAULT_MS: u64 = 0;
const RECORDING_STOP_GRACE_MIN_MS: u64 = 0;
const RECORDING_STOP_GRACE_MAX_MS: u64 = 2_000;
const SHADOW_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const SHADOW_WINDOW_DEFAULT_MS: u64 = 120_000;
const SHADOW_WINDOW_MIN_MS: u64 = 5_000;
const SHADOW_WINDOW_MAX_MS: u64 = 600_000;
const SHADOW_SEGMENT_DEFAULT_MS: u64 = 2_000;
const SHADOW_SEGMENT_MIN_MS: u64 = 250;
const SHADOW_SEGMENT_MAX_MS: u64 = 10_000;
const ENV_STREAM_COMMAND_QUEUE_SIZE: &str = "HELIOS_STREAM_COMMAND_QUEUE_SIZE";
const DEFAULT_STREAM_COMMAND_QUEUE_SIZE: usize = 64;
const ENV_RECORDING_FRAME_QUEUE_SIZE: &str = "HELIOS_RECORDING_FRAME_QUEUE_SIZE";
const DEFAULT_RECORDING_FRAME_QUEUE_SIZE: usize = 48;

static SHADOW_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

fn thread_stack_size_bytes(var: &str, default: usize) -> usize {
    const MIN: usize = 256 * 1024;
    const MAX: usize = 8 * 1024 * 1024;
    std::env::var(var).ok().and_then(|raw| raw.trim().parse::<usize>().ok()).unwrap_or(default).clamp(MIN, MAX)
}

fn stream_worker_stack_size_bytes() -> usize {
    thread_stack_size_bytes("HELIOS_ENGINE_STREAM_THREAD_STACK_BYTES", 2 * 1024 * 1024)
}

fn recording_worker_stack_size_bytes() -> usize {
    thread_stack_size_bytes("HELIOS_ENGINE_RECORDING_THREAD_STACK_BYTES", 1 * 1024 * 1024)
}

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
        // Best-effort cleanup of orphaned shared-memory preview buffers from prior crashes/restarts.
        // These live on tmpfs and can permanently inflate RSS if left behind.
        cleanup_all_stream_files();
        // Clean up recording staging dirs that can be left behind if the engine crashes mid-recording.
        cleanup_recording_stage_root_sync();
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            starting: Arc::new(AsyncMutex::new(HashSet::new())),
            recordings: Arc::new(AsyncMutex::new(HashMap::new())),
            shadow_recorders: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    pub async fn start_stream(&self, manifest: ResolvedStreamConfig) -> Result<(Uuid, CaptureDescriptor)> {
        let mut manifest = manifest;
        let stream_id = manifest.identity.id.unwrap_or_else(Uuid::new_v4);
        manifest.identity.id = Some(stream_id);

        if let Some(recording_codec) = manifest.recording_mode.shadow_buffer_codec() {
            if !shadow_recorder_feature_enabled() {
                return Err(Error::InvalidState("shadow-buffer recording mode requires HELIOS_ENABLE_SHADOW_RECORDER"));
            }
            if !manifest.encoder.enabled {
                return Err(Error::InvalidState("shadow-buffer recording mode requires encoder enabled"));
            }
            let encoder_id = manifest.encoder_id().unwrap_or_default().trim();
            if encoder_id.is_empty() {
                return Err(Error::InvalidState("shadow-buffer recording mode requires an encoder_id"));
            }
            if !encoder_matches(recording_codec, encoder_id) {
                return Err(Error::InvalidState("shadow-buffer recording mode codec does not match the selected encoder"));
            }

            // Shadow-based capture/recording needs frequent keyframes so short windows (5s, 30s)
            // remain decodable and ffmpeg can remux without producing empty MP4s.
            let want_fps = infer_recording_fps(&manifest).unwrap_or(30.0).round().clamp(1.0, 240.0) as u32;
            let settings = manifest
                .encoder
                .settings
                .get_or_insert_with(|| {
                    crate::ipc::empty_encoder_settings_for_selector(manifest.encoder.codec_id.as_deref()).unwrap_or(match recording_codec {
                        RecordingCodec::H264 => crate::ipc::EncoderSettings::H264 {
                            bitrate: None,
                            gop: None,
                            framerate: None,
                            thread_count: None,
                            output_resolution: None,
                        },
                        RecordingCodec::H265 => crate::ipc::EncoderSettings::H265 {
                            bitrate: None,
                            gop: None,
                            framerate: None,
                            thread_count: None,
                            output_resolution: None,
                        },
                    })
                });
            if let crate::ipc::EncoderSettings::FfmpegMjpeg { gop, framerate, .. }
            | crate::ipc::EncoderSettings::H264 { gop, framerate, .. }
            | crate::ipc::EncoderSettings::H265 { gop, framerate, .. } = settings
            {
                if framerate.is_none() {
                    *framerate = Some(crate::ipc::FrameRate { numerator: want_fps, denominator: 1 });
                }
                if gop.is_none() {
                    // 1-second GOP by default (in frames).
                    *gop = Some(want_fps as i32);
                }
            }
        }
        {
            let streams = self.streams.read().await;
            if streams.contains_key(&stream_id) {
                return Err(Error::Conflict("stream already exists"));
            }
        }
        if let Some(requested_alias) = normalize_alias(manifest.identity.alias.as_deref()) {
            let entries: Vec<Arc<StreamContext>> = {
                let streams = self.streams.read().await;
                streams.values().cloned().collect()
            };
            for ctx in entries {
                let existing_alias = normalize_alias(ctx.manifest.read().await.identity.alias.as_deref());
                if existing_alias.as_deref() == Some(&requested_alias) {
                    return Err(Error::Conflict("stream alias already exists"));
                }
            }
        }
        {
            let mut starting = self.starting.lock().await;
            if !starting.insert(stream_id) {
                return Err(Error::Conflict("stream already starting"));
            }
        }

        let streams_handle = self.streams.clone();
        let manifest_clone = manifest.clone();
        let start_res = tokio::task::spawn_blocking({
            let manifest = manifest.clone();
            move || {
                let host_buffer = manifest.host_buffer();
                let graph = crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest, None).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))?;
                let shmem = match ShmemWriter::create(stream_id) {
                    Ok(writer) => Some(writer),
                    Err(err) => {
                        tracing::warn!(stream_id = %stream_id, error = %err, "shmem writer init failed; preview endpoints may be unavailable");
                        None
                    }
                };
                let descriptor = crate::capture::descriptor_for_config(&manifest.capture).ok_or(Error::InvalidState("missing capture descriptor"))?;

                let runner = StreamRunner::new(StreamRunnerConfig {
                    capture_config: manifest.capture.clone(),
                    graph: graph.clone(),
                    encoder_id: manifest.encoder.codec_id.clone(),
                    decoder_id: manifest.decoder.codec_id.clone(),
                    encoder_settings: manifest.encoder.settings.clone(),
                    decoder_settings: manifest.decoder.settings.clone(),
                    preview_jpeg_quality: manifest.preview_jpeg_quality,
                    shmem,
                    stream_id: Some(stream_id),
                });
                let encoded_tx = runner.encoded_sender();
                let managed_encoded_consumer_count = runner.managed_encoded_consumer_count_handle();
                let managed_encoded_consumer_last_seen_ms = runner.managed_encoded_consumer_last_seen_handle();
                let raw_tx = runner.raw_sender();
                let (command_tx, command_rx) = sync_channel::<StreamCommand>(stream_command_queue_size());
                let (exit_tx, exit_rx) = watch::channel(StreamExit::Running);
                let join: JoinHandle<()> = std::thread::Builder::new()
                    .name(format!("helios-stream-{stream_id}"))
                    .stack_size(stream_worker_stack_size_bytes())
                    .spawn(move || run_stream_worker(runner, command_rx, exit_tx))
                    .map_err(|err| Error::InvalidStateOwned(format!("stream worker spawn failed: {err}")))?;
                Ok::<_, Error>((descriptor, graph, encoded_tx, managed_encoded_consumer_count, managed_encoded_consumer_last_seen_ms, raw_tx, command_tx, exit_rx, join))
            }
        })
        .await
        .map_err(|_| Error::InvalidState("stream worker start cancelled"));
        let (descriptor, host, encoded_tx, managed_encoded_consumer_count, managed_encoded_consumer_last_seen_ms, raw_tx, command_tx, exit_rx, join) = match start_res {
            Ok(Ok(parts)) => parts,
            Ok(Err(err)) => {
                self.finish_starting(stream_id).await;
                return Err(err);
            }
            Err(err) => {
                self.finish_starting(stream_id).await;
                return Err(err);
            }
        };
        let cleanup_rx = exit_rx.clone();
        let monitor_rx = cleanup_rx.clone();
        let shadow_enabled = manifest_clone.recording_mode.is_shadow_buffer();
        let stream_started_at_ms = current_time_ms();
        let encoded_tx_for_ctx = encoded_tx.clone();
        let raw_tx_for_ctx = raw_tx.clone();
        let mut streams = self.streams.write().await;
        if streams.contains_key(&stream_id) {
            drop(streams);
            abort_unregistered_stream_worker(stream_id, command_tx, join).await;
            self.finish_starting(stream_id).await;
            return Err(Error::Conflict("stream already exists"));
        }
        streams.insert(
            stream_id,
            Arc::new(StreamContext {
                stream_started_at_ms,
                manifest: tokio::sync::RwLock::new(manifest_clone.clone()),
                descriptor: descriptor.clone(),
                host: tokio::sync::RwLock::new(host),
                calibration_mode_restore: tokio::sync::RwLock::new(None),
                encoded_tx: encoded_tx_for_ctx,
                managed_encoded_consumer_count,
                managed_encoded_consumer_last_seen_ms,
                raw_tx: raw_tx_for_ctx,
                command_tx,
                exit_rx,
                cleanup_rx: AsyncMutex::new(Some(cleanup_rx)),
                worker_join: AsyncMutex::new(Some(join)),
            }),
        );
        drop(streams);
        if shadow_enabled {
            if let Err(err) = self.start_shadow_recorder(stream_id, &manifest_clone).await {
                tracing::warn!(stream_id = %stream_id, error = %err, "shadow recorder start failed");
            }
        }
        tracing::info!(stream_id = %stream_id, "stream registered");
        self.finish_starting(stream_id).await;
        tokio::spawn({
            let streams_handle = streams_handle.clone();
            let manager = self.clone();
            let mut exit_rx = monitor_rx;
            async move {
                let _ = exit_rx.changed().await;
                let _ = manager.stop_shadow_recorder(stream_id).await;
                let ctx = {
                    let mut streams = streams_handle.write().await;
                    streams.remove(&stream_id)
                };
                if let Some(ctx) = ctx {
                    let exit_state = ctx.exit_rx.borrow().clone();
                    manager.finalize_stream_teardown(stream_id, ctx).await;
                    if let StreamExit::Stopped(Err(err)) = exit_state {
                        tracing::warn!(stream_id = %stream_id, error = %err, "stream worker exited with error");
                    }
                }
            }
        });
        Ok((stream_id, descriptor))
    }

    pub async fn stop_stream(&self, stream_id: Uuid) -> Result<()> {
        tracing::info!(stream_id = %stream_id, "stream stop requested");
        // Keep the stream registered until the worker has actually stopped.
        // This avoids `/streams == []` while capture/ISP is still active.
        let ctx = {
            let streams = self.streams.read().await;
            streams.get(&stream_id).cloned()
        };
        let Some(ctx) = ctx else {
            return Ok(());
        };
        let _ = self.stop_recording(stream_id).await;
        let _ = self.stop_shadow_recorder(stream_id).await;
        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::Stop { respond_to: tx })?;
        let _ = rx.await;

        let mut exit_rx = ctx.cleanup_rx.lock().await.take().unwrap_or_else(|| ctx.exit_rx.clone());
        let stopped = tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                if matches!(exit_rx.borrow().clone(), StreamExit::Stopped(_)) {
                    break;
                }
                if exit_rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .is_ok();
        if !stopped {
            tracing::warn!(stream_id = %stream_id, "stream stop did not complete within 25s");
            return Err(Error::Timeout);
        }
        // Remove the stream from the manager map (if it hasn't already been removed by the exit monitor),
        // and perform best-effort cleanup.
        let ctx = {
            let mut streams = self.streams.write().await;
            streams.remove(&stream_id)
        };
        if let Some(ctx) = ctx {
            self.finalize_stream_teardown(stream_id, ctx).await;
        }
        tracing::info!(stream_id = %stream_id, "stream stop completed");
        Ok(())
    }

    async fn finalize_stream_teardown(&self, stream_id: Uuid, ctx: Arc<StreamContext>) {
        cleanup_stream_files(stream_id);
        if let Some(join) = ctx.worker_join.lock().await.take() {
            let _ = tokio::task::spawn_blocking(move || join.join()).await;
        }
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

    async fn sample_stream_encoder_fps(&self, stream_id: Uuid) -> Option<f32> {
        // Encoder stats require the encoder worker to have produced samples.
        // Retry briefly so we can remux clips with "actual" fps (avoids time compression when
        // the encoder can't keep up with the capture target_fps).
        for _ in 0..5 {
            if let Ok(metrics) = self.get_metrics(stream_id).await {
                if let Some(fps) = metrics.encoder.as_ref().map(|m| m.fps as f32).filter(|v| v.is_finite() && *v > 0.0) {
                    return Some(fps);
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        None
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
        let StartRecordingParams { source, output_path, container, codec, duration_ms, settings } = params;
        let ctx = self.get_stream(stream_id).await?;
        {
            let recordings = self.recordings.lock().await;
            if recordings.contains_key(&stream_id) {
                return Err(Error::Conflict("recording already active"));
            }
        }

        let manifest_snapshot = ctx.manifest.read().await.clone();
        let resolved_source = resolve_recording_source(&manifest_snapshot, source.clone());

        let record_path = PathBuf::from(output_path);
        let raw_path = match container {
            RecordingContainer::Mp4 => build_raw_path(&record_path, codec),
            RecordingContainer::Raw => record_path.clone(),
        };
        let frame_ts_path = recording_frame_ts_path(&record_path);
        let requested_fps = settings.as_ref().and_then(|s| s.fps).filter(|v| *v > 0.0).or_else(|| infer_recording_fps(&manifest_snapshot));
        let encoder_hint = manifest_snapshot.encoder.codec_id.clone().filter(|value| !value.trim().is_empty());

        let started_at_ms = current_time_ms();
        let (stop_tx, stop_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(RecordingState::Running);
        let session = Arc::new(AsyncMutex::new(RecordingSession { stop_tx: Some(stop_tx), done_rx, started_at_ms }));
        {
            let mut recordings = self.recordings.lock().await;
            if recordings.contains_key(&stream_id) {
                return Err(Error::Conflict("recording already active"));
            }
            recordings.insert(stream_id, session.clone());
        }

        // Record live start/stop sessions from the frame path by default so clip boundaries follow
        // the same preview cadence the operator sees in UI.
        //
        // Encoded passthrough is opt-in for deployments that prioritize lower CPU over boundary
        // fidelity (start can land mid-GOP).
        let passthrough_codec = infer_recording_codec(manifest_snapshot.encoder_id());
        let use_encoded_passthrough = recording_encoded_passthrough_enabled()
            && matches!(&resolved_source, ResolvedRecordingSource::Multiplex)
            && manifest_snapshot.encoder.enabled
            && passthrough_codec.is_some()
            && (matches!(container, RecordingContainer::Mp4) || passthrough_codec == Some(codec));
        if use_encoded_passthrough {
            let manager = self.clone();
            let encoded_consumer = ManagedEncodedConsumer::new(Arc::clone(&ctx.managed_encoded_consumer_count), Arc::clone(&ctx.managed_encoded_consumer_last_seen_ms));
            let encoded_consumer_touch = encoded_consumer.touch_handle();
            let rx = ctx.encoded_tx.subscribe();
            let source_codec = passthrough_codec.unwrap_or(codec);
            let frame_ts_path = frame_ts_path.clone();
            tokio::spawn(async move {
                let _encoded_consumer = encoded_consumer;
                let result = record_encoded_session(
                    rx,
                    stop_rx,
                    &record_path,
                    &raw_path,
                    container,
                    source_codec,
                    codec,
                    duration_ms,
                    requested_fps,
                    settings,
                    Some(frame_ts_path),
                    Some(encoded_consumer_touch),
                )
                .await;
                let _ = done_tx.send(RecordingState::Completed(result.clone()));
                manager.finish_recording(stream_id, result).await;
            });
            return Ok(());
        }

        // Shadow-backed capture for regular start/stop recordings is opt-in. Default behavior keeps
        // clip timing tied to live frame flow.
        let shadow_candidate = recording_shadow_start_stop_enabled()
            && shadow_recorder_feature_enabled()
            && manifest_snapshot.recording_mode.is_shadow_buffer()
            && matches!(source, RecordingSource::Multiplex)
            && manifest_snapshot.recording_mode.shadow_buffer_codec().is_some();
        if shadow_candidate {
            let stream_codec = manifest_snapshot.recording_mode.shadow_buffer_codec().unwrap();
            if codec != stream_codec {
                {
                    let mut recordings = self.recordings.lock().await;
                    recordings.remove(&stream_id);
                }
                return Err(Error::InvalidStateOwned(format!("requested codec {codec:?} does not match stream encoder ({stream_codec:?}); configure encoder_id accordingly")));
            }

            // Ensure shadow recorder is running (it may not have been started if the feature gate
            // was enabled after the stream booted).
            let already_running = {
                let recorders = self.shadow_recorders.lock().await;
                recorders.contains_key(&stream_id)
            };
            if !already_running {
                if let Err(err) = self.start_shadow_recorder(stream_id, &manifest_snapshot).await {
                    {
                        let mut recordings = self.recordings.lock().await;
                        recordings.remove(&stream_id);
                    }
                    return Err(Error::InvalidStateOwned(format!("shadow recorder start failed: {err}")));
                }
            }

            let measured_fps = self.sample_stream_encoder_fps(stream_id).await;
            let fps = settings.as_ref().and_then(|s| s.fps).filter(|v| *v > 0.0).or(measured_fps).or(requested_fps);

            let manager = self.clone();
            let shadow_dir = shadow_dir_for_stream(stream_id);
            tokio::spawn(async move {
                let result = record_shadow_segments_session(stream_id, &shadow_dir, codec, &record_path, &raw_path, container, started_at_ms, duration_ms, fps, settings, stop_rx).await;
                let _ = done_tx.send(RecordingState::Completed(result.clone()));
                manager.finish_recording(stream_id, result).await;
            });
            return Ok(());
        }

        let frame_source = match resolved_source {
            ResolvedRecordingSource::Multiplex => {
                let graph = ctx.host.read().await.clone();
                RecordingFrameSource::Multiplex { rx: graph.subscribe() }
            }
            ResolvedRecordingSource::Raw => RecordingFrameSource::Raw { rx: ctx.raw_tx.subscribe() },
            ResolvedRecordingSource::Pipeline { pipeline_id, output_key } => match build_recording_pipeline_graph(&manifest_snapshot, pipeline_id, output_key.as_deref()) {
                Ok(graph) => RecordingFrameSource::Pipeline { rx: ctx.raw_tx.subscribe(), graph },
                Err(err) => {
                    tracing::warn!(
                        stream_id = %stream_id,
                        pipeline_id = %pipeline_id,
                        error = %err,
                        "recording pipeline invalid; falling back to multiplex"
                    );
                    let graph = ctx.host.read().await.clone();
                    RecordingFrameSource::Multiplex { rx: graph.subscribe() }
                }
            },
        };
        let manager = self.clone();
        tokio::spawn(async move {
            let params =
                RecordingFrameParams { output_path: &record_path, raw_path: &raw_path, container, codec, duration_ms, fps: requested_fps, settings, encoder_hint, frame_ts_path: Some(frame_ts_path) };
            let result = record_frame_stream(frame_source, stop_rx, params).await;
            let _ = done_tx.send(RecordingState::Completed(result.clone()));
            manager.finish_recording(stream_id, result).await;
        });

        Ok(())
    }

    pub async fn stop_recording(&self, stream_id: Uuid) -> Result<()> {
        let session = {
            let recordings = self.recordings.lock().await;
            recordings.get(&stream_id).cloned()
        };
        let Some(session) = session else {
            return Ok(());
        };

        let (stop_tx, mut done_rx) = {
            let mut session = session.lock().await;
            (session.stop_tx.take(), session.done_rx.clone())
        };
        if let Some(stop_tx) = stop_tx {
            let _ = stop_tx.send(());
        }

        if matches!(*done_rx.borrow(), RecordingState::Completed(_)) {
            return recording_state_to_result(done_rx.borrow().clone());
        }

        match tokio::time::timeout(RECORDING_STOP_TIMEOUT, done_rx.changed()).await {
            Ok(Ok(())) => recording_state_to_result(done_rx.borrow().clone()),
            Ok(Err(_)) => Err(Error::InvalidState("recording status channel closed")),
            Err(_) => Err(Error::Timeout),
        }
    }

    pub async fn capture_shadow_recording(&self, stream_id: Uuid, output_path: String, container: RecordingContainer, window_ms: u64) -> Result<()> {
        if !shadow_recorder_feature_enabled() {
            return Err(Error::InvalidState("shadow recorder feature is disabled"));
        }
        let ctx = self.get_stream(stream_id).await?;
        let manifest_snapshot = ctx.manifest.read().await.clone();
        let Some(configured_codec) = manifest_snapshot.recording_mode.shadow_buffer_codec() else {
            return Err(Error::InvalidState("shadow recorder disabled"));
        };
        let (requested_codec, raw_format) = {
            let recorders = self.shadow_recorders.lock().await;
            recorders.get(&stream_id).map(|handle| {
                let raw = RawRecordingFormat::from_u8(handle.raw_format.load(Ordering::Acquire));
                (handle.requested_codec, Some(raw))
            })
        }
        .unwrap_or((configured_codec, None));
        let raw_format = raw_format.unwrap_or_else(|| RawRecordingFormat::from_codec(requested_codec));
        let window_ms = normalize_shadow_window_ms(window_ms);
        if window_ms == 0 {
            return Err(Error::InvalidState("shadow capture window must be > 0"));
        }
        // Pre-roll improves mux validity when the requested window begins mid-GOP (ffmpeg will
        // otherwise drop non-decodable leading frames until an IDR).
        let preroll_ms = shadow_segment_ms().max(1_000);
        let capture_ms = window_ms.saturating_add(preroll_ms).min(shadow_window_ms());

        let shadow_dir = shadow_dir_for_stream(stream_id);
        if fs::metadata(&shadow_dir).await.is_err() {
            return Err(Error::NotFound("shadow recorder data not found"));
        }

        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).await.map_err(|err| Error::InvalidStateOwned(format!("shadow output dir create failed: {err}")))?;
        }

        let raw_path = match container {
            RecordingContainer::Mp4 => build_raw_path(&output_path, requested_codec),
            RecordingContainer::Raw => output_path.clone(),
        };

        capture_shadow_segments(&shadow_dir, &raw_path, requested_codec, capture_ms).await.map_err(Error::InvalidStateOwned)?;

        if matches!(container, RecordingContainer::Mp4) {
            let forced_fps = probe_raw_frames(&raw_path, raw_format).await.ok().map(|frames| {
                let secs = (capture_ms as f32 / 1000.0).max(0.001);
                (frames as f32 / secs).clamp(1.0, 240.0)
            });
            let fps = forced_fps.or(self.sample_stream_encoder_fps(stream_id).await).or_else(|| infer_recording_fps(&manifest_snapshot));

            let pretrim = pretrim_output_path(&output_path);
            let finalize_res = finalize_recording_mp4(raw_path.clone(), pretrim.clone(), raw_format, requested_codec, fps, None).await;
            if let Err(err) = finalize_res {
                let _ = fs::remove_file(&pretrim).await;
                return Err(Error::InvalidStateOwned(err));
            }
            let trim_res = trim_mp4_to_last_window(&pretrim, &output_path, window_ms, requested_codec).await;
            let _ = fs::remove_file(&pretrim).await;
            if let Err(err) = trim_res {
                let _ = fs::remove_file(&output_path).await;
                return Err(Error::InvalidStateOwned(err));
            }
        }

        Ok(())
    }

    async fn start_shadow_recorder(&self, stream_id: Uuid, manifest: &ResolvedStreamConfig) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        if !manifest.encoder.enabled {
            return Err(Error::InvalidState("shadow recorder requires encoder enabled"));
        }
        let preferred_codec = manifest.recording_mode.shadow_buffer_codec().ok_or(Error::InvalidState("shadow recorder requires shadow-buffer recording mode"))?;
        let raw_format = RawRecordingFormat::from_codec(preferred_codec);
        let shadow_dir = shadow_dir_for_stream(stream_id);
        let window_ms = shadow_window_ms();
        let segment_ms = shadow_segment_ms();
        let format_tracker = Arc::new(AtomicU8::new(raw_format.to_u8()));
        let tracker_for_worker = Arc::clone(&format_tracker);
        let (stop_tx, stop_rx) = oneshot::channel();
        let encoded_consumer = ManagedEncodedConsumer::new(Arc::clone(&ctx.managed_encoded_consumer_count), Arc::clone(&ctx.managed_encoded_consumer_last_seen_ms));
        let encoded_consumer_touch = encoded_consumer.touch_handle();
        let mut encoded_rx = ctx.encoded_tx.subscribe();
        let join = tokio::spawn(async move {
            let _encoded_consumer = encoded_consumer;
            let worker = match ShadowRecorderWorker::start(ShadowRecorderConfig { shadow_dir: shadow_dir.clone(), codec: preferred_codec, segment_ms, window_ms, format_tracker: tracker_for_worker }) {
                Ok(worker) => worker,
                Err(err) => {
                    tracing::warn!(stream_id = %stream_id, error = %err, "shadow recorder worker start failed");
                    return;
                }
            };
            if let Err(err) = run_shadow_recorder_stream(&mut encoded_rx, worker, stop_rx, Some(encoded_consumer_touch)).await {
                tracing::warn!(stream_id = %stream_id, error = %err, "shadow recorder stopped with error");
            }
        });

        let mut recorders = self.shadow_recorders.lock().await;
        if recorders.contains_key(&stream_id) {
            join.abort();
            return Err(Error::Conflict("shadow recorder already active"));
        }
        recorders.insert(stream_id, ShadowRecorderHandle { stop_tx: Some(stop_tx), join, requested_codec: preferred_codec, raw_format: format_tracker });
        Ok(())
    }

    async fn stop_shadow_recorder(&self, stream_id: Uuid) -> Result<()> {
        let handle = {
            let mut recorders = self.shadow_recorders.lock().await;
            recorders.remove(&stream_id)
        };
        let Some(mut handle) = handle else {
            return Ok(());
        };
        if let Some(stop_tx) = handle.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        match tokio::time::timeout(SHADOW_STOP_TIMEOUT, handle.join).await {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::Timeout),
        }
    }

    pub async fn set_codecs(&self, stream_id: Uuid, decoder_id: Option<String>, encoder_id: Option<String>) -> Result<()> {
        self.send_command(stream_id, move |respond_to| StreamCommand::SetCodecs { decoder_id, encoder_id, respond_to }).await
    }

    pub async fn set_calibration(&self, stream_id: Uuid, calibration: Option<crate::ipc::StreamCalibration>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        {
            let mut manifest = ctx.manifest.write().await;
            manifest.calibration = calibration.clone();
        }

        // Update graph executor state in-place (no capture restart).
        {
            let host = ctx.host.read().await;
            host.set_calibration(calibration.clone());
        }

        // Also forward to the worker thread in case its graph handle is not sharing the same executor instance.
        self.send_command(stream_id, move |respond_to| StreamCommand::SetCalibration { calibration, respond_to }).await
    }

    pub async fn set_pipeline_inputs(&self, stream_id: Uuid, pipeline_id: Option<Uuid>, inputs: std::collections::BTreeMap<String, Option<serde_json::Value>>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let mut normalized_inputs: std::collections::BTreeMap<String, Option<serde_json::Value>> = std::collections::BTreeMap::new();
        for (raw_key, value) in inputs {
            let key = raw_key.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            normalized_inputs.insert(key, value);
        }
        if normalized_inputs.is_empty() {
            return Ok(());
        }

        // Update graph executor state in-place (no capture restart).
        {
            let host = ctx.host.read().await;
            host.set_pipeline_inputs(pipeline_id, &normalized_inputs);
        }

        // Also forward to the worker thread in case its graph handle is not sharing the same executor instance.
        self.send_command(stream_id, {
            let command_inputs = normalized_inputs.clone();
            move |respond_to| StreamCommand::SetPipelineInputs { pipeline_id, inputs: command_inputs, respond_to }
        })
        .await?;

        {
            let mut manifest = ctx.manifest.write().await;
            for (key, value) in normalized_inputs {
                if let Some(value) = value {
                    manifest.pipeline_host_inputs.insert(key, JsonWire(value));
                } else {
                    manifest.pipeline_host_inputs.remove(&key);
                }
            }
        }

        Ok(())
    }

    pub async fn set_calibration_mode(&self, stream_id: Uuid, enabled: bool, dictionary: Option<String>, mode: Option<String>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        if enabled {
            tracing::info!(
                stream_id = %stream_id,
                enabled,
                mode = mode.as_deref().unwrap_or("calibration"),
                "calibration mode requested"
            );
            {
                let mut restore = ctx.calibration_mode_restore.write().await;
                if restore.is_none() {
                    let mut manifest = ctx.manifest.read().await.clone();
                    reconcile_manifest_pipeline_references(&mut manifest);
                    *restore = Some(CalibrationModeRestore::from_manifest(&manifest));
                }
            }

            let manifest_snapshot = ctx.manifest.read().await.clone();
            let host_buffer = calibration_mode_host_buffer(manifest_snapshot.host_buffer());
            let calibration = manifest_snapshot.calibration.clone();

            let mode = mode.unwrap_or_else(|| "calibration".to_string());
            let template_id = if mode.eq_ignore_ascii_case("undistort") || mode.eq_ignore_ascii_case("undistorted") { UNDISTORT_TEMPLATE_ID } else { CALIBRATION_TEMPLATE_ID };
            let output_port = calibration_mode_output_port(template_id);
            let mut graph_json = if template_id == CALIBRATION_TEMPLATE_ID {
                load_calibration_mode_graph_json().map_err(|err| Error::InvalidStateOwned(format!("calibration template missing: {err}")))?
            } else {
                crate::pipelines::load_template_graph_json(template_id).map_err(|err| Error::InvalidStateOwned(format!("calibration template missing: {err}")))?
            };
            if template_id == CALIBRATION_TEMPLATE_ID {
                let mut selected_dictionary: Option<String> = None;
                if let Some(raw_dictionary) = dictionary.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    let Some(dictionary) = normalize_calibration_dictionary_name(raw_dictionary) else {
                        return Err(Error::InvalidStateOwned(format!("unknown ArUco dictionary '{raw_dictionary}'")));
                    };
                    patch_dictionary_const(&mut graph_json, &dictionary);
                    selected_dictionary = Some(dictionary);
                }
                patch_calibration_mode_detection_strictness(&mut graph_json, selected_dictionary.as_deref());
                ensure_calibration_mode_frame_output(&mut graph_json);
                ensure_calibration_mode_detections_json_output(&mut graph_json);
            }

            tracing::info!(
                stream_id = %stream_id,
                template_id,
                "calibration mode building graph"
            );

            let mut graph_json_for_build = graph_json.clone();
            let stream_alias = manifest_snapshot.identity.alias.as_deref().map(|v| crate::graph::context::sanitize_segment(v, "stream")).unwrap_or_else(|| "stream".to_string());
            crate::graph::context::inject_node_context(&mut graph_json_for_build, &stream_alias, "calibration");

            let mut manifest_for_build = manifest_snapshot.clone();
            manifest_for_build.pipeline_enabled = true;
            manifest_for_build.pipelines = vec![crate::ipc::StreamPipelineBinding {
                pipeline_id: CALIBRATION_MODE_PIPELINE_UUID,
                pipeline_graph: Some(crate::ipc::JsonWire(graph_json_for_build.clone())),
                pipeline_output: Some(output_port.to_string()),
                pipeline_patch: None,
            }];
            manifest_for_build.active_pipeline_id = Some(CALIBRATION_MODE_PIPELINE_UUID);
            manifest_for_build.active_pipeline_output = Some(output_port.to_string());
            manifest_for_build.pipeline_layout = None;
            manifest_for_build.pipeline_wires = Vec::new();

            let graph = tokio::task::spawn_blocking(move || {
                let graph = crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest_for_build, Some(output_port))
                    .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))?;
                graph.set_calibration(calibration);
                Ok::<_, Error>(graph)
            })
            .await
            .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))??;
            tracing::info!(
                stream_id = %stream_id,
                outputs = ?graph.host_output_ports().unwrap_or_default(),
                "calibration mode graph built"
            );

            let (tx, rx) = oneshot::channel();
            enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
            rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;
            *ctx.host.write().await = graph;
            let outputs = {
                let host = ctx.host.read().await;
                host.host_output_ports().unwrap_or_default()
            };
            tracing::info!(stream_id = %stream_id, outputs = ?outputs, "calibration mode graph applied");

            let mut manifest = ctx.manifest.write().await;
            manifest.pipeline_enabled = true;
            manifest.pipelines = vec![crate::ipc::StreamPipelineBinding {
                pipeline_id: CALIBRATION_MODE_PIPELINE_UUID,
                pipeline_graph: Some(crate::ipc::JsonWire(graph_json)),
                pipeline_output: Some(output_port.to_string()),
                pipeline_patch: None,
            }];
            manifest.active_pipeline_id = Some(CALIBRATION_MODE_PIPELINE_UUID);
            manifest.active_pipeline_output = Some(output_port.to_string());
            manifest.pipeline_layout = None;
            manifest.pipeline_wires = Vec::new();
            return Ok(());
        }

        let restore = ctx.calibration_mode_restore.write().await.take();
        let Some(restore) = restore else {
            return Ok(());
        };

        let manifest_snapshot = {
            let mut manifest = ctx.manifest.write().await;
            restore.apply_to_manifest(&mut manifest);
            reconcile_manifest_pipeline_references(&mut manifest);
            manifest.clone()
        };

        let host_buffer = manifest_snapshot.host_buffer();
        let graph = tokio::task::spawn_blocking(move || {
            crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest_snapshot, None).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
        })
        .await
        .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))??;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;
        *ctx.host.write().await = graph;
        Ok(())
    }

    pub async fn subscribe_frames(&self, stream_id: Uuid) -> Result<Receiver<Arc<image::DynamicImage>>> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        Ok(host.subscribe())
    }

    pub async fn subscribe_encoded(&self, stream_id: Uuid) -> Result<Receiver<EncodedFrame>> {
        let ctx = self.get_stream(stream_id).await?;
        Ok(ctx.encoded_tx.subscribe())
    }

    pub async fn set_graph_output(&self, stream_id: Uuid, output: Option<String>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        let host_buffer = manifest_snapshot.host_buffer();
        let active_pipeline_id = manifest_snapshot.active_pipeline_id.or_else(|| manifest_snapshot.pipelines.first().map(|p| p.pipeline_id));
        // In single-view mode, the visible output is controlled by slot (0,0). Respect that target
        // even if `active_pipeline_id` still points at a different (hidden) pipeline instance.
        let view_pipeline_id = single_view_slot_pipeline_id(&manifest_snapshot).or(active_pipeline_id);
        let output_targets_active_pipeline = view_pipeline_id == active_pipeline_id;
        let canonical_output = canonicalize_output_for_pipeline(output.clone(), view_pipeline_id);

        // Special-case: allow users to switch the RAW stream view (`raw` vs `undistorted`)
        // even when pipelines are currently disabled.
        //
        // When pipelines are disabled, the stream normally runs with a passthrough host (no graph).
        // Selecting a non-default output implies we need to enable the built-in RAW graph so the
        // requested port can be produced.
        let wants_output = canonical_output.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let wants_non_default_raw_output = wants_output.is_some_and(|v| !(v.eq_ignore_ascii_case("raw") || v.eq_ignore_ascii_case("frame")));
        // When no pipelines/layout are configured, the stream is implicitly the RAW view.
        // Selecting `undistorted` in that state must enable the RAW graph (otherwise
        // `pipeline_enabled=false` forces passthrough and ignores output selection).
        let implicit_raw_view = view_pipeline_id.is_none() && manifest_snapshot.pipelines.is_empty();
        let wants_raw_graph = (view_pipeline_id == Some(RAW_STREAM_PIPELINE_UUID) || implicit_raw_view) && wants_non_default_raw_output;
        if !manifest_snapshot.pipeline_enabled && wants_raw_graph {
            manifest_snapshot.pipeline_enabled = true;
        }
        if output_targets_active_pipeline {
            manifest_snapshot.active_pipeline_output = canonical_output.clone();
        }
        if let Some(target_id) = view_pipeline_id {
            if let Some(binding) = manifest_snapshot.pipelines.iter_mut().find(|p| p.pipeline_id == target_id) {
                binding.pipeline_output = canonical_output.clone();
            }
        }
        if let Some(layout) = manifest_snapshot.pipeline_layout.as_mut() {
            if layout.rows == 1 && layout.columns == 1 {
                if let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0) {
                    slot.output_key = canonicalize_output_for_pipeline(canonical_output.clone(), slot.pipeline_id);
                }
            }
        }

        let build_graph = |selected_output: Option<String>, host_buffer: usize, manifest: crate::ipc::ResolvedStreamConfig| async move {
            tokio::task::spawn_blocking(move || {
                crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest, selected_output.as_deref()).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
            })
            .await
            .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))?
        };

        let mut selected_output = if output_targets_active_pipeline { canonical_output.clone() } else { manifest_snapshot.active_pipeline_output.clone() };
        let graph = match build_graph(selected_output.clone(), host_buffer, manifest_snapshot.clone()).await {
            Ok(graph) => graph,
            Err(err) => {
                if selected_output.is_some() {
                    tracing::warn!(stream_id = %stream_id, output = ?selected_output, "pipeline output invalid; retrying without output override");
                    selected_output = None;
                    manifest_snapshot.active_pipeline_output = None;
                    if let Some(active_id) = active_pipeline_id {
                        if let Some(binding) = manifest_snapshot.pipelines.iter_mut().find(|p| p.pipeline_id == active_id) {
                            binding.pipeline_output = None;
                        }
                    }
                    if output_targets_active_pipeline {
                        if let Some(layout) = manifest_snapshot.pipeline_layout.as_mut() {
                            if layout.rows == 1 && layout.columns == 1 {
                                if let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0) {
                                    slot.output_key = None;
                                }
                            }
                        }
                    }
                    let host_buffer = manifest_snapshot.host_buffer();
                    build_graph(selected_output.clone(), host_buffer, manifest_snapshot.clone()).await?
                } else {
                    return Err(err);
                }
            }
        };

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;

        {
            let mut manifest = ctx.manifest.write().await;
            if !manifest.pipeline_enabled && wants_raw_graph {
                manifest.pipeline_enabled = true;
            }
            if output_targets_active_pipeline {
                manifest.active_pipeline_output = selected_output.clone();
            }
            if let Some(active_id) = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|p| p.pipeline_id)) {
                manifest.active_pipeline_id = Some(active_id);
                if output_targets_active_pipeline {
                    if let Some(binding) = manifest.pipelines.iter_mut().find(|p| p.pipeline_id == active_id) {
                        binding.pipeline_output = selected_output.clone();
                    }
                }
            }
            let applied_view_output = if output_targets_active_pipeline { selected_output.clone() } else { canonical_output.clone() };
            if let Some(target_id) = view_pipeline_id {
                if let Some(binding) = manifest.pipelines.iter_mut().find(|p| p.pipeline_id == target_id) {
                    binding.pipeline_output = applied_view_output.clone();
                }
            }

            // In 1x1 multiplex mode, the rendered output is selected by the (0,0) layout slot's
            // `output_key` (not `active_pipeline_output`). Keep them in sync so UI "Output"
            // dropdowns immediately affect the preview/encoder output.
            if let Some(layout) = manifest.pipeline_layout.as_mut() {
                if layout.rows == 1 && layout.columns == 1 {
                    if let Some(slot) = layout.slots.iter_mut().find(|slot| slot.row == 0 && slot.column == 0) {
                        slot.output_key = canonicalize_output_for_pipeline(applied_view_output.clone(), slot.pipeline_id);
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn set_graph(&self, stream_id: Uuid, graph_json: serde_json::Value, pipeline_id: Option<Uuid>, output: Option<String>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let graph_wire = crate::ipc::JsonWire(graph_json);

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_enabled = true;

        let target_pipeline_id = match pipeline_id {
            Some(id) => id,
            None => {
                return Err(Error::InvalidState("pipeline_id is required when setting a stream pipeline"));
            }
        };
        let graph_payload = Some(graph_wire.clone());

        // `/pipeline/graph` updates (or inserts) a single pipeline binding.
        // Importantly: do not clear existing multiplex layout state, since users can
        // assign/tune pipelines while a multiplex grid is configured.
        let matching_count = manifest_snapshot.pipelines.iter().filter(|binding| binding.pipeline_id == target_pipeline_id).count();
        let mut updated = false;
        for binding in &mut manifest_snapshot.pipelines {
            if binding.pipeline_id == target_pipeline_id {
                binding.pipeline_graph = graph_payload.clone();
                binding.pipeline_patch = None;
                if output.is_some() && matching_count <= 1 {
                    binding.pipeline_output = output.clone();
                }
                updated = true;
            }
        }
        if !updated {
            manifest_snapshot.pipelines.push(crate::ipc::StreamPipelineBinding {
                pipeline_id: target_pipeline_id,
                pipeline_graph: graph_payload,
                pipeline_output: output.clone(),
                pipeline_patch: None,
            });
        }
        if manifest_snapshot.active_pipeline_id.is_none() {
            manifest_snapshot.active_pipeline_id = Some(target_pipeline_id);
        }
        if output.is_some() && manifest_snapshot.active_pipeline_id == Some(target_pipeline_id) {
            manifest_snapshot.active_pipeline_output = output.clone();
        }

        let host_buffer = manifest_snapshot.host_buffer();
        let selected_output = if manifest_snapshot.active_pipeline_id == Some(target_pipeline_id) { output.clone() } else { None };
        let manifest_for_build = manifest_snapshot.clone();
        let graph = tokio::task::spawn_blocking(move || {
            crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest_for_build, selected_output.as_deref())
                .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
        })
        .await
        .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))??;

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;
        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn apply_graph_patch(&self, stream_id: Uuid, patch_json: serde_json::Value, pipeline_id: Option<Uuid>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let patch: GraphPatch = serde_json::from_value(patch_json.clone()).map_err(|err| Error::InvalidStateOwned(format!("invalid graph patch: {err}")))?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_enabled = true;

        let target_pipeline_id = match pipeline_id {
            Some(id) => id,
            None => {
                return Err(Error::InvalidState("pipeline_id is required when applying a stream patch"));
            }
        };

        let patch_wire = JsonWire(patch_json);
        let mut updated = false;
        for binding in &mut manifest_snapshot.pipelines {
            if binding.pipeline_id == target_pipeline_id {
                binding.pipeline_patch = Some(patch_wire.clone());
                updated = true;
            }
        }
        if !updated {
            manifest_snapshot.pipelines.push(crate::ipc::StreamPipelineBinding {
                pipeline_id: target_pipeline_id,
                pipeline_graph: None,
                pipeline_output: None,
                pipeline_patch: Some(patch_wire.clone()),
            });
        }
        if manifest_snapshot.active_pipeline_id.is_none() {
            manifest_snapshot.active_pipeline_id = Some(target_pipeline_id);
        }

        let applied = {
            let host = ctx.host.read().await;
            host.apply_graph_patch(Some(target_pipeline_id), &patch)
        };

        if applied.is_none() {
            let host_buffer = manifest_snapshot.host_buffer();
            let manifest_for_build = manifest_snapshot.clone();
            let graph = tokio::task::spawn_blocking(move || {
                crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest_for_build, None).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
            })
            .await
            .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))??;

            let (tx, rx) = oneshot::channel();
            enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
            rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;
            *ctx.host.write().await = graph;
        }

        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn set_pipeline_layout(&self, stream_id: Uuid, layout: Option<crate::ipc::StreamPipelineLayout>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_layout = layout.clone();
        if layout.is_some() {
            manifest_snapshot.pipeline_enabled = true;
        }
        if let Some(layout) = manifest_snapshot.pipeline_layout.as_mut() {
            let previous_active_pipeline_id = manifest_snapshot.active_pipeline_id;
            let mut layout_ids = std::collections::BTreeSet::new();
            let mut layout_selected_ids = std::collections::BTreeSet::new();
            for slot in &mut layout.slots {
                if slot.pipeline_id == Some(RAW_STREAM_PIPELINE_UUID) {
                    slot.output_key = canonicalize_output_for_pipeline(slot.output_key.clone(), Some(RAW_STREAM_PIPELINE_UUID));
                }
                if let Some(id) = slot.pipeline_id {
                    layout_selected_ids.insert(id);
                    if id == RAW_STREAM_PIPELINE_UUID {
                        continue;
                    }
                    layout_ids.insert(id);
                }
            }
            if !layout_ids.is_empty() {
                let existing: std::collections::BTreeSet<Uuid> = manifest_snapshot.pipelines.iter().map(|binding| binding.pipeline_id).collect();
                for pipeline_id in layout_ids {
                    if existing.contains(&pipeline_id) {
                        continue;
                    }
                    manifest_snapshot.pipelines.push(crate::ipc::StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: None, pipeline_patch: None });
                }
            }
            let single_slot_layout = layout.rows == 1 && layout.columns == 1;
            let active_in_layout = manifest_snapshot.active_pipeline_id.is_some_and(|id| layout_selected_ids.contains(&id));
            let should_retarget_active = single_slot_layout || !active_in_layout;
            let selected_slot = layout
                .slots
                .iter()
                .find(|slot| slot.row == 0 && slot.column == 0 && slot.pipeline_id.is_some())
                .or_else(|| layout.slots.iter().find(|slot| slot.pipeline_id.is_some()))
                .and_then(|slot| slot.pipeline_id.map(|pipeline_id| (pipeline_id, slot.output_key.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()))));
            if should_retarget_active {
                if let Some((active_id, slot_output)) = selected_slot {
                    manifest_snapshot.active_pipeline_id = Some(active_id);
                    let active_changed = previous_active_pipeline_id != Some(active_id);
                    if active_id == RAW_STREAM_PIPELINE_UUID {
                        // Keep 1x1/raw output selection in sync with the selected slot.
                        if let Some(slot_output) = slot_output {
                            manifest_snapshot.active_pipeline_output = canonicalize_output_for_pipeline(Some(slot_output), Some(RAW_STREAM_PIPELINE_UUID));
                        } else if active_changed {
                            manifest_snapshot.active_pipeline_output = manifest_snapshot.pipelines.iter().find(|p| p.pipeline_id == active_id).and_then(|binding| binding.pipeline_output.clone());
                        }
                    } else if active_changed || manifest_snapshot.active_pipeline_output.is_none() {
                        manifest_snapshot.active_pipeline_output = manifest_snapshot.pipelines.iter().find(|p| p.pipeline_id == active_id).and_then(|binding| binding.pipeline_output.clone());
                    }
                } else {
                    manifest_snapshot.active_pipeline_id = None;
                    manifest_snapshot.active_pipeline_output = None;
                }
            }
        }

        let host_buffer = manifest_snapshot.host_buffer();
        manifest_snapshot.active_pipeline_output = canonicalize_output_for_pipeline(manifest_snapshot.active_pipeline_output.clone(), manifest_snapshot.active_pipeline_id);
        let mut selected_output = manifest_snapshot.active_pipeline_output.clone();
        let manifest_for_build = manifest_snapshot.clone();

        let build_graph = |output: Option<String>, host_buffer: usize, manifest: crate::ipc::ResolvedStreamConfig| async move {
            tokio::task::spawn_blocking(move || {
                crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest, output.as_deref()).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
            })
            .await
            .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))?
        };

        let graph = match build_graph(selected_output.clone(), host_buffer, manifest_for_build).await {
            Ok(graph) => graph,
            Err(err) => {
                if selected_output.is_some() {
                    tracing::warn!(stream_id = %stream_id, output = ?selected_output, "pipeline layout output invalid; retrying without output override");
                    selected_output = None;
                    manifest_snapshot.active_pipeline_output = None;
                    if let Some(active_id) = manifest_snapshot.active_pipeline_id {
                        if let Some(binding) = manifest_snapshot.pipelines.iter_mut().find(|p| p.pipeline_id == active_id) {
                            binding.pipeline_output = None;
                        }
                    }
                    let host_buffer = manifest_snapshot.host_buffer();
                    let manifest_for_retry = manifest_snapshot.clone();
                    build_graph(selected_output.clone(), host_buffer, manifest_for_retry).await?
                } else {
                    return Err(err);
                }
            }
        };

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;
        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn set_pipeline_wires(&self, stream_id: Uuid, wires: Vec<crate::ipc::StreamPipelineWire>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;

        let mut manifest_snapshot = ctx.manifest.read().await.clone();
        manifest_snapshot.pipeline_wires = wires;
        if !manifest_snapshot.pipeline_wires.is_empty() {
            manifest_snapshot.pipeline_enabled = true;
        }

        const RAW_STREAM_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);
        let mut referenced: std::collections::BTreeSet<Uuid> = std::collections::BTreeSet::new();
        for wire in &manifest_snapshot.pipeline_wires {
            if wire.from.pipeline_id != RAW_STREAM_PIPELINE_UUID {
                referenced.insert(wire.from.pipeline_id);
            }
            if wire.to.pipeline_id != RAW_STREAM_PIPELINE_UUID {
                referenced.insert(wire.to.pipeline_id);
            }
        }
        if !referenced.is_empty() {
            let existing: std::collections::BTreeSet<Uuid> = manifest_snapshot.pipelines.iter().map(|binding| binding.pipeline_id).collect();
            for id in referenced {
                if !existing.contains(&id) {
                    return Err(Error::InvalidStateOwned(format!("pipeline {id} referenced by wiring is missing from stream manifest")));
                }
            }
        }

        let host_buffer = manifest_snapshot.host_buffer();
        let mut selected_output = manifest_snapshot.active_pipeline_output.clone();
        let mut manifest_for_build = manifest_snapshot.clone();

        let build_graph = |output: Option<String>, host_buffer: usize, manifest: crate::ipc::ResolvedStreamConfig| async move {
            tokio::task::spawn_blocking(move || {
                crate::graph::build_graph_handle_for_manifest(host_buffer, &manifest, output.as_deref()).map_err(|err| Error::InvalidStateOwned(format!("pipeline graph invalid: {err}")))
            })
            .await
            .map_err(|err| Error::InvalidStateOwned(format!("pipeline graph build failed: {err}")))?
        };

        let graph = match build_graph(selected_output.clone(), host_buffer, manifest_for_build.clone()).await {
            Ok(graph) => graph,
            Err(err) => {
                if selected_output.is_some() {
                    tracing::warn!(stream_id = %stream_id, output = ?selected_output, "pipeline wires output invalid; retrying without output override");
                    selected_output = None;
                    manifest_snapshot.active_pipeline_output = None;
                    if let Some(active_id) = manifest_snapshot.active_pipeline_id {
                        if let Some(binding) = manifest_snapshot.pipelines.iter_mut().find(|p| p.pipeline_id == active_id) {
                            binding.pipeline_output = None;
                        }
                    }
                    manifest_for_build = manifest_snapshot.clone();
                    let host_buffer = manifest_snapshot.host_buffer();
                    build_graph(selected_output.clone(), host_buffer, manifest_for_build).await?
                } else {
                    return Err(err);
                }
            }
        };

        let (tx, rx) = oneshot::channel();
        enqueue_stream_command(&ctx.command_tx, StreamCommand::SetGraph { graph: graph.clone(), respond_to: tx })?;
        rx.await.map_err(|_| Error::InvalidState("stream worker dropped response"))??;

        *ctx.host.write().await = graph;
        *ctx.manifest.write().await = manifest_snapshot;

        Ok(())
    }

    pub async fn list_graph_outputs(&self, stream_id: Uuid) -> Result<Vec<crate::ipc::GraphOutputPortDescriptor>> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        Ok(host.host_output_port_descriptors())
    }

    pub async fn get_graph_output_sample(&self, stream_id: Uuid, port: String, fresh: bool) -> Result<serde_json::Value> {
        let ctx = self.get_stream(stream_id).await?;
        if !fresh {
            let host = ctx.host.read().await;
            return host.read_json_output(&port, false).ok_or(Error::NotFound("graph output sample unavailable"));
        }
        {
            let host = ctx.host.read().await;
            host.request_output_sample(&port);
            if let Some(value) = host.read_json_output(&port, false) {
                return Ok(value);
            }
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
        loop {
            {
                let host = ctx.host.read().await;
                if let Some(value) = host.read_json_output(&port, false) {
                    return Ok(value);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(Error::NotFound("graph output sample unavailable"))
    }

    pub async fn set_graph_perf(&self, stream_id: Uuid, pipeline_id: Option<Uuid>, enabled: bool) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        host.set_perf_enabled(pipeline_id, enabled);
        Ok(())
    }

    pub async fn reset_graph_metrics(&self, stream_id: Uuid, pipeline_id: Option<Uuid>) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        host.reset_pipeline_metrics(pipeline_id);
        Ok(())
    }

    pub async fn capture_graph_flamegraph(&self, stream_id: Uuid, pipeline_id: Option<Uuid>, duration_ms: u64) -> Result<()> {
        let ctx = self.get_stream(stream_id).await?;
        let host = ctx.host.read().await;
        host.capture_flamegraph(pipeline_id, duration_ms).map_err(Error::InvalidStateOwned)?;
        Ok(())
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
            let mut status = if graph_state.disabled {
                crate::ipc::StreamStatus {
                    state: crate::ipc::StreamState::Disabled,
                    started_at_ms: Some(ctx.stream_started_at_ms),
                    disabled_since_ms: graph_state.disabled_since_ms,
                    disabled_reason: graph_state.disabled_reason,
                    recording_active: false,
                    recording_since_ms: None,
                }
            } else {
                crate::ipc::StreamStatus::default()
            };
            if status.started_at_ms.is_none() {
                status.started_at_ms = Some(ctx.stream_started_at_ms);
            }
            if let Some(started_at) = recording_started.get(&id).copied() {
                status.recording_active = true;
                status.recording_since_ms = Some(started_at);
            }
            out.push(crate::ipc::StreamSummary { stream_id: id, descriptor: ctx.descriptor.clone(), manifest, status });
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

fn stream_command_queue_size() -> usize {
    std::env::var(ENV_STREAM_COMMAND_QUEUE_SIZE).ok().and_then(|raw| raw.parse::<usize>().ok()).filter(|size| *size > 0).unwrap_or(DEFAULT_STREAM_COMMAND_QUEUE_SIZE).clamp(8, 512)
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

fn recording_state_to_result(state: RecordingState) -> Result<()> {
    match state {
        RecordingState::Completed(Ok(_)) => Ok(()),
        RecordingState::Completed(Err(reason)) => Err(Error::InvalidStateOwned(reason)),
        RecordingState::Running => Err(Error::InvalidState("recording still running")),
    }
}

fn resolve_recording_source(manifest: &ResolvedStreamConfig, source: RecordingSource) -> ResolvedRecordingSource {
    match source {
        RecordingSource::Multiplex => ResolvedRecordingSource::Multiplex,
        RecordingSource::Raw => ResolvedRecordingSource::Raw,
        RecordingSource::Pipeline { pipeline_id, output_key } => {
            // Allow the built-in RAW pipeline id even when `pipeline_enabled=false` cleared
            // pipelines from the manifest; this enables explicit RAW outputs like "undistorted".
            let matches_manifest = pipeline_id.filter(|id| *id == RAW_STREAM_PIPELINE_UUID || manifest_contains_pipeline(manifest, *id));
            let pipeline_id = matches_manifest.or(manifest.active_pipeline_id).or_else(|| manifest.pipelines.first().map(|p| p.pipeline_id));
            match pipeline_id {
                Some(pipeline_id) => ResolvedRecordingSource::Pipeline { pipeline_id, output_key },
                None => {
                    tracing::warn!("recording pipeline missing; falling back to multiplex");
                    ResolvedRecordingSource::Multiplex
                }
            }
        }
    }
}

fn manifest_contains_pipeline(manifest: &ResolvedStreamConfig, pipeline_id: Uuid) -> bool {
    if manifest.active_pipeline_id == Some(pipeline_id) {
        return true;
    }
    manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id)
}

fn build_recording_pipeline_graph(manifest: &ResolvedStreamConfig, pipeline_id: Uuid, output_key: Option<&str>) -> Result<crate::graph::GraphHandle> {
    crate::graph::build_graph_handle_for_pipeline_output(manifest.host_buffer(), manifest, pipeline_id, output_key)
        .map_err(|err| Error::InvalidStateOwned(format!("recording pipeline build failed: {err}")))
}

fn recording_codec_ext(codec: RecordingCodec) -> &'static str {
    match codec {
        RecordingCodec::H264 => "h264",
        RecordingCodec::H265 => "h265",
    }
}

fn build_raw_path(output_path: &Path, codec: RecordingCodec) -> PathBuf {
    let ext = recording_codec_ext(codec);
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("recording");
    let suffix = Uuid::new_v4().simple().to_string();
    parent.join(format!("{stem}.raw-{suffix}.{ext}"))
}

fn recording_frame_ts_path(output_path: &Path) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path.file_name().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("recording");
    parent.join(format!("{file_name}.frame_ts.txt"))
}

fn temp_output_path(output_path: &Path) -> PathBuf {
    // Keep the real extension as the final suffix so tools like ffmpeg can infer the container.
    // E.g. `video.mp4` -> `video.part.mp4` (NOT `video.mp4.part`).
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty());
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("output");
    match ext {
        Some(ext) => parent.join(format!("{stem}.part.{ext}")),
        None => parent.join(format!("{stem}.part")),
    }
}

fn pretrim_output_path(output_path: &Path) -> PathBuf {
    // Unique temp file next to the final output (so rename is atomic within filesystem).
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty());
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("output");
    let suffix = Uuid::new_v4().simple().to_string();
    match ext {
        Some(ext) => parent.join(format!("{stem}.pretrim-{suffix}.{ext}")),
        None => parent.join(format!("{stem}.pretrim-{suffix}")),
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_millis() as u64).unwrap_or(0)
}

fn infer_recording_fps(manifest: &ResolvedStreamConfig) -> Option<f32> {
    if let Some(settings) = manifest.encoder_settings() {
        if let Some(rate) = settings.framerate() {
            if rate.denominator > 0 {
                return Some(rate.numerator as f32 / rate.denominator as f32);
            }
        }
    }
    if let Some(fps) = manifest.capture.target_fps {
        return Some(fps as f32);
    }
    if let Some(interval) = manifest.capture.interval {
        return Some(interval.fps());
    }
    manifest.capture.mode.interval.map(|interval| interval.fps())
}

fn infer_recording_codec(encoder_id: Option<&str>) -> Option<RecordingCodec> {
    let encoder_id = encoder_id?.trim();
    if encoder_id.is_empty() {
        return None;
    }
    let h264 = encoder_matches(RecordingCodec::H264, encoder_id);
    let h265 = encoder_matches(RecordingCodec::H265, encoder_id);
    match (h264, h265) {
        (true, false) => Some(RecordingCodec::H264),
        (false, true) => Some(RecordingCodec::H265),
        // Ambiguous selector (e.g. impl names like "ffmpeg" that can produce multiple codecs).
        _ => None,
    }
}

fn encoder_matches(codec: RecordingCodec, encoder_id: &str) -> bool {
    let targets: &[&str] = match codec {
        RecordingCodec::H264 => &["h264", "avc"],
        RecordingCodec::H265 => &["h265", "hevc"],
    };
    let encoder_id = encoder_id.trim();
    if encoder_id.is_empty() {
        return false;
    }
    if targets.iter().any(|t| encoder_id.eq_ignore_ascii_case(t)) {
        return true;
    }
    let lowered = encoder_id.to_ascii_lowercase();
    if targets.iter().any(|t| lowered.contains(t)) {
        return true;
    }
    let entries = CodecRegistry::list_enabled_encoders().ok();
    if let Some(entries) = entries {
        let mut matched_names = std::collections::BTreeSet::new();
        for (_, list) in entries {
            for desc in list {
                if desc.kind != CodecKind::Encoder {
                    continue;
                }
                if desc.impl_name.eq_ignore_ascii_case(encoder_id) {
                    matched_names.insert(desc.name.to_ascii_lowercase());
                }
            }
        }
        if matched_names.len() == 1 && targets.iter().any(|t| matched_names.contains(*t)) {
            return true;
        }
    }
    false
}

fn shadow_window_ms() -> u64 {
    let requested = std::env::var("HELIOS_SHADOW_WINDOW_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(SHADOW_WINDOW_DEFAULT_MS);
    requested.clamp(SHADOW_WINDOW_MIN_MS, SHADOW_WINDOW_MAX_MS)
}

fn shadow_segment_ms() -> u64 {
    let requested = std::env::var("HELIOS_SHADOW_SEGMENT_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(SHADOW_SEGMENT_DEFAULT_MS);
    let clamped = requested.clamp(SHADOW_SEGMENT_MIN_MS, SHADOW_SEGMENT_MAX_MS);
    clamped.min(shadow_window_ms())
}

fn shadow_flush_interval_ms() -> u64 {
    std::env::var("HELIOS_SHADOW_FLUSH_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1_000).clamp(100, 5_000)
}

fn shadow_writer_buffer_bytes() -> usize {
    std::env::var("HELIOS_SHADOW_WRITER_BYTES").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1 << 20).clamp(64 << 10, 8 << 20)
}

fn shadow_config_scan_interval_ms() -> u64 {
    std::env::var("HELIOS_SHADOW_CONFIG_SCAN_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1_000).clamp(100, 10_000)
}

fn recording_stop_grace_ms() -> u64 {
    std::env::var("HELIOS_RECORDING_STOP_GRACE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(RECORDING_STOP_GRACE_DEFAULT_MS)
        .clamp(RECORDING_STOP_GRACE_MIN_MS, RECORDING_STOP_GRACE_MAX_MS)
}

fn recording_frame_queue_size() -> usize {
    std::env::var(ENV_RECORDING_FRAME_QUEUE_SIZE).ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(DEFAULT_RECORDING_FRAME_QUEUE_SIZE).clamp(1, 256)
}

fn keep_raw_on_record_fail() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        let raw = std::env::var("HELIOS_KEEP_RAW_ON_RECORD_FAIL").ok().unwrap_or_default();
        let v = raw.trim().to_ascii_lowercase();
        matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
    })
}

fn normalize_shadow_window_ms(requested: u64) -> u64 {
    let base = shadow_window_ms();
    if requested == 0 {
        return base;
    }
    requested.clamp(SHADOW_WINDOW_MIN_MS, base)
}

fn shadow_data_root() -> PathBuf {
    SHADOW_DATA_ROOT
        .get_or_init(|| {
            if let Ok(dir) = std::env::var("HELIOS_SHADOW_RECORD_DIR") {
                return PathBuf::from(dir);
            }
            if let Ok(dir) = std::env::var("HELIOS_API_DATA_DIR") {
                return PathBuf::from(dir);
            }
            let candidates = [PathBuf::from("/data/helios/api"), PathBuf::from("/var/lib/helios/api"), std::env::temp_dir().join("helios-api")];
            for candidate in candidates {
                if candidate.is_dir() || std::fs::create_dir_all(&candidate).is_ok() {
                    return candidate;
                }
            }
            std::env::temp_dir().join("helios-api")
        })
        .clone()
}

fn env_flag_enabled(var: &str, default_value: bool) -> bool {
    let raw = match std::env::var(var) {
        Ok(value) => value,
        Err(_) => return default_value,
    };
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return default_value;
    }
    matches!(value.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
}

fn shadow_recorder_feature_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_ENABLE_SHADOW_RECORDER", true))
}

fn recording_encoded_passthrough_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_RECORDING_USE_ENCODED_PASSTHROUGH", false))
}

fn recording_shadow_start_stop_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_RECORDING_USE_SHADOW_START_STOP", false))
}

fn shadow_dir_for_stream(stream_id: Uuid) -> PathBuf {
    shadow_data_root().join("media").join(".shadow").join(stream_id.to_string())
}

fn recording_stage_root() -> PathBuf {
    shadow_data_root().join("media").join(".recordings")
}

fn cleanup_recording_stage_root_sync() {
    let root = recording_stage_root();
    if std::fs::metadata(&root).is_err() {
        return;
    }
    let entries = match std::fs::read_dir(&root) {
        Ok(v) => v,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn parse_shadow_segment_timestamp(name: &str, ext: &str) -> Option<u64> {
    let suffix = format!(".{ext}");
    let trimmed = name.strip_suffix(&suffix)?;
    let ts = trimmed.strip_prefix("segment_")?;
    ts.parse::<u64>().ok()
}

async fn list_shadow_segments(dir: &Path, codec: RecordingCodec) -> std::result::Result<Vec<(u64, PathBuf)>, String> {
    let ext = recording_codec_ext(codec);
    let mut entries = fs::read_dir(dir).await.map_err(|err| format!("shadow dir read failed: {err}"))?;
    let mut segments = Vec::new();
    loop {
        let entry = entries.next_entry().await.map_err(|err| format!("shadow dir entry failed: {err}"))?;
        let Some(entry) = entry else { break };
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let Some(ts) = parse_shadow_segment_timestamp(&name, ext) else { continue };
        let meta = entry.metadata().await.map_err(|err| format!("shadow segment stat failed: {err}"))?;
        if meta.is_file() && meta.len() > 0 {
            segments.push((ts, entry.path()));
        }
    }
    segments.sort_by_key(|(ts, _)| *ts);
    Ok(segments)
}

fn cleanup_shadow_segments_sync(dir: &Path, cutoff_ms: u64, codec: RecordingCodec) -> std::result::Result<(), String> {
    let ext = recording_codec_ext(codec);
    let entries = std::fs::read_dir(dir).map_err(|err| format!("shadow dir read failed: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("shadow dir entry failed: {err}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(ts) = parse_shadow_segment_timestamp(&name, ext) else { continue };
        if ts < cutoff_ms {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn cleanup_shadow_dir_sync(dir: &Path) -> std::result::Result<(), String> {
    if std::fs::metadata(dir).is_err() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|err| format!("shadow dir read failed: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("shadow dir entry failed: {err}"))?;
        let meta = entry.metadata().map_err(|err| format!("shadow segment stat failed: {err}"))?;
        if meta.is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn open_shadow_segment_sync(dir: &Path, start_ms: u64, codec: RecordingCodec, writer_capacity: usize) -> std::result::Result<std::io::BufWriter<std::fs::File>, String> {
    let ext = recording_codec_ext(codec);
    let path = dir.join(format!("segment_{start_ms}.{ext}"));
    let file = std::fs::File::create(&path).map_err(|err| format!("shadow segment open failed: {err}"))?;
    Ok(std::io::BufWriter::with_capacity(writer_capacity, file))
}

fn is_annexb_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0, 0, 0, 1]) || bytes.starts_with(&[0, 0, 1])
}

fn is_mjpeg_payload(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

fn read_be_len(bytes: &[u8], len_size: usize) -> Option<usize> {
    match len_size {
        1 => Some(*bytes.first()? as usize),
        2 => Some(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]) as usize),
        3 => Some(((*bytes.first()? as usize) << 16) | ((*bytes.get(1)? as usize) << 8) | (*bytes.get(2)? as usize)),
        4 => Some(u32::from_be_bytes([*bytes.first()?, *bytes.get(1)?, *bytes.get(2)?, *bytes.get(3)?]) as usize),
        _ => None,
    }
}

fn length_prefixed_to_annexb_with_len(bytes: &[u8], len_size: usize, out: &mut Vec<u8>) -> bool {
    if len_size == 0 || bytes.len() < len_size {
        return false;
    }
    out.clear();
    // Approx reserve: add 4 bytes start-code per NAL; worst-case NAL count is bytes/len_size.
    out.reserve(bytes.len().saturating_add((bytes.len() / len_size).saturating_mul(4)));

    let mut cursor = bytes;
    while cursor.len() >= len_size {
        let Some(len) = read_be_len(cursor, len_size) else {
            out.clear();
            return false;
        };
        cursor = &cursor[len_size..];
        if len == 0 || len > cursor.len() {
            out.clear();
            return false;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&cursor[..len]);
        cursor = &cursor[len..];
    }

    if !cursor.is_empty() || out.is_empty() {
        out.clear();
        return false;
    }
    true
}

fn length_prefixed_to_annexb_best_effort(bytes: &[u8], out: &mut Vec<u8>) -> bool {
    // Most common is 4. Some streams (notably some hardware paths) use 2.
    for len_size in [4usize, 3, 2, 1] {
        if length_prefixed_to_annexb_with_len(bytes, len_size, out) {
            return true;
        }
    }
    false
}

fn find_annexb_start(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    if bytes.len() < 3 {
        return None;
    }
    let mut i = from;
    while i + 3 <= bytes.len() {
        if bytes[i] == 0 && bytes[i + 1] == 0 {
            if bytes[i + 2] == 1 {
                return Some((i, i + 3));
            }
            if i + 3 < bytes.len() && bytes[i + 2] == 0 && bytes[i + 3] == 1 {
                return Some((i, i + 4));
            }
        }
        i += 1;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn write_payload_to_raw(
    writer: &mut dyn Write,
    codec: RecordingCodec,
    raw_format: &mut RawRecordingFormat,
    format_tracker: Option<&Arc<AtomicU8>>,
    bitstream: &mut RecordingBitstream,
    convert_buf: &mut Vec<u8>,
    config_cache: &mut RecordingConfigCache,
    wrote_prefix: &mut bool,
    pending_before_config: &mut std::collections::VecDeque<Vec<u8>>,
    pending_bytes: &mut usize,
    pending_max_bytes: usize,
    stats: &mut RecordingStats,
    ts_writer: &mut Option<RecordingTimestampWriter>,
    payload: &[u8],
    ts_ms: u64,
    allow_mjpeg_switch: bool,
) -> std::result::Result<(), String> {
    if *raw_format != RawRecordingFormat::Mjpeg && is_mjpeg_payload(payload) {
        if !allow_mjpeg_switch {
            return Err("encoded payload was MJPEG but H264/H265 was expected".to_string());
        }
        tracing::warn!(codec = ?codec, "recording encoder output appears to be MJPEG; treating as MJPEG");
        *raw_format = RawRecordingFormat::Mjpeg;
        stats.raw_format = Some(RawRecordingFormat::Mjpeg);
        if let Some(tracker) = format_tracker {
            tracker.store(RawRecordingFormat::Mjpeg.to_u8(), Ordering::Release);
        }
    }

    if matches!(*raw_format, RawRecordingFormat::Mjpeg) {
        writer.write_all(payload).map_err(|err| format!("recording write failed: {err}"))?;
        stats.record_ts(ts_ms);
        stats.frames = stats.frames.saturating_add(1);
        stats.bytes = stats.bytes.saturating_add(payload.len() as u64);
        if let Some(writer) = ts_writer.as_mut() {
            writer.record(ts_ms)?;
        }
        return Ok(());
    }

    let mut out_payload = payload;
    if *bitstream == RecordingBitstream::Unknown {
        *bitstream = if is_annexb_prefix(out_payload) { RecordingBitstream::AnnexB } else { RecordingBitstream::LengthPrefixed };
    }
    if *bitstream == RecordingBitstream::LengthPrefixed {
        if length_prefixed_to_annexb_best_effort(out_payload, convert_buf) {
            out_payload = convert_buf.as_slice();
        } else if is_annexb_prefix(out_payload) {
            *bitstream = RecordingBitstream::AnnexB;
        } else if let Some((start, _)) = find_annexb_start(out_payload, 0) {
            // Some sources prepend non-startcode bytes; best-effort salvage if AnnexB is present.
            *bitstream = RecordingBitstream::AnnexB;
            out_payload = &out_payload[start..];
        } else {
            // Drop malformed payload.
            return Ok(());
        }
    }

    config_cache.update_from_annexb(codec, out_payload);
    if !*wrote_prefix {
        if let Some(prefix) = config_cache.prefix(codec) {
            writer.write_all(&prefix).map_err(|err| format!("recording write failed: {err}"))?;
            *wrote_prefix = true;
            while let Some(buf) = pending_before_config.pop_front() {
                *pending_bytes = pending_bytes.saturating_sub(buf.len());
                writer.write_all(&buf).map_err(|err| format!("recording write failed: {err}"))?;
                stats.bytes = stats.bytes.saturating_add(buf.len() as u64);
            }
        } else {
            let mut v = Vec::with_capacity(out_payload.len());
            v.extend_from_slice(out_payload);
            *pending_bytes = pending_bytes.saturating_add(v.len());
            pending_before_config.push_back(v);
            while *pending_bytes > pending_max_bytes {
                if let Some(dropped) = pending_before_config.pop_front() {
                    *pending_bytes = pending_bytes.saturating_sub(dropped.len());
                } else {
                    *pending_bytes = 0;
                    break;
                }
            }
            return Ok(());
        }
    }

    writer.write_all(out_payload).map_err(|err| format!("recording write failed: {err}"))?;
    stats.record_ts(ts_ms);
    stats.frames = stats.frames.saturating_add(1);
    stats.bytes = stats.bytes.saturating_add(out_payload.len() as u64);
    if let Some(writer) = ts_writer.as_mut() {
        writer.record(ts_ms)?;
    }
    Ok(())
}

async fn capture_shadow_segments(shadow_dir: &Path, output_path: &Path, codec: RecordingCodec, window_ms: u64) -> std::result::Result<u64, String> {
    let segments = list_shadow_segments(shadow_dir, codec).await?;
    if segments.is_empty() {
        return Err("shadow recorder has no segments".to_string());
    }
    let now_ms = current_time_ms();
    let cutoff = now_ms.saturating_sub(window_ms);
    // Select segments that plausibly cover the requested window ending "now".
    //
    // Important: segments are *usually* rotated at fixed `segment_ms`, but rotation only occurs
    // when new encoded chunks arrive. Under encoder backpressure / jitter, a segment can be longer
    // than `segment_ms`, so "cutoff - segment_ms" is not a reliable way to pick the overlapping
    // segment. Instead, include the most recent segment at/before the cutoff plus everything after.
    let idx = segments.iter().position(|(ts, _)| *ts >= cutoff).unwrap_or(segments.len());
    let start_index = idx.saturating_sub(1);
    let selection = &segments[start_index..];
    if selection.is_empty() {
        return Err("shadow recorder has no segments in window".to_string());
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| format!("shadow output dir create failed: {err}"))?;
    }
    let file = File::create(output_path).await.map_err(|err| format!("shadow output open failed: {err}"))?;
    let mut writer = BufWriter::new(file);
    let mut total_bytes = 0u64;

    // Ensure the concatenated bitstream begins with codec config so ffmpeg can parse it even if
    // the first selected segment starts mid-stream.
    if let Some(prefix) = probe_prefix_from_segments(selection, codec).await {
        writer.write_all(&prefix).await.map_err(|err| format!("shadow output write failed: {err}"))?;
        total_bytes = total_bytes.saturating_add(prefix.len() as u64);
    }
    for (_, path) in selection {
        let mut reader = File::open(path).await.map_err(|err| format!("shadow segment open failed: {err}"))?;
        let written = io::copy(&mut reader, &mut writer).await.map_err(|err| format!("shadow segment copy failed: {err}"))?;
        total_bytes = total_bytes.saturating_add(written);
    }
    writer.flush().await.map_err(|err| format!("shadow output flush failed: {err}"))?;
    if total_bytes == 0 {
        return Err("shadow output empty".to_string());
    }
    Ok(total_bytes)
}

async fn run_shadow_recorder_stream(
    rx: &mut Receiver<EncodedFrame>,
    worker: ShadowRecorderWorker,
    mut stop_rx: oneshot::Receiver<()>,
    consumer_touch: Option<Arc<AtomicU64>>,
) -> std::result::Result<(), String> {
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            recv = rx.recv() => match recv {
                Ok(chunk) => {
                    if let Some(touch) = consumer_touch.as_ref() {
                        touch.store(current_time_ms(), Ordering::Relaxed);
                    }
                    let _ = worker.try_send_chunk(chunk.data, chunk.ts_ms);
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    }
    worker.stop()
}

impl RecordingEncoderWorker {
    fn start(config: RecordingEncoderConfig) -> std::result::Result<Self, String> {
        let mailbox = Arc::new(LatestFrameMailbox::new(recording_frame_queue_size()));
        let rx = Arc::clone(&mailbox);
        let join = std::thread::Builder::new()
            .name("helios-recording".into())
            .stack_size(recording_worker_stack_size_bytes())
            .spawn(move || run_recording_encoder(rx, config))
            .map_err(|err| format!("recording worker spawn failed: {err}"))?;
        Ok(Self { mailbox, join: Some(join) })
    }

    fn send_image(&self, image: Arc<image::DynamicImage>, ts_ms: u64) {
        self.mailbox.push_frame(image, ts_ms);
    }

    fn stop(mut self) -> std::result::Result<RecordingStats, String> {
        self.mailbox.stop();
        let join = self.join.take().ok_or_else(|| "recording worker join missing".to_string())?;
        join.join().map_err(|_| "recording worker join failed".to_string())?
    }
}

impl ShadowRecorderWorker {
    fn start(config: ShadowRecorderConfig) -> std::result::Result<Self, String> {
        // Small bounded queue. Shadow recorder must not stall the stream loop, but also should not
        // drop nearly all chunks under momentary IO/cpu hiccups.
        let (tx, rx) = sync_channel::<ShadowRecorderJob>(32);
        let join = std::thread::Builder::new()
            .name("helios-shadow-record".into())
            .stack_size(recording_worker_stack_size_bytes())
            .spawn(move || run_shadow_recorder_worker(rx, config))
            .map_err(|err| format!("shadow recorder spawn failed: {err}"))?;
        Ok(Self { tx, join: Some(join) })
    }

    fn try_send_chunk(&self, data: Arc<[u8]>, ts_ms: u64) -> bool {
        match self.tx.try_send(ShadowRecorderJob::Chunk { data, ts_ms }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    fn stop(mut self) -> std::result::Result<(), String> {
        let _ = self.tx.send(ShadowRecorderJob::Stop);
        let join = self.join.take().ok_or_else(|| "shadow recorder join missing".to_string())?;
        join.join().map_err(|_| "shadow recorder join failed".to_string())?
    }
}

fn run_recording_encoder(rx: Arc<LatestFrameMailbox>, config: RecordingEncoderConfig) -> std::result::Result<RecordingStats, String> {
    let requested_codec = config.codec;
    let (raw_format, probe_codec, encoder) = select_recording_encoder(requested_codec, config.encoder_hint.as_deref())?;
    let desc = encoder.descriptor();
    let impl_name = desc.impl_name;
    if impl_name.eq_ignore_ascii_case("ffmpeg") {
        tracing::warn!(encoder = %impl_name, codec = ?requested_codec, "recording encoder resolved to software implementation");
    } else {
        tracing::info!(encoder = %impl_name, codec = ?requested_codec, "recording encoder initialized");
    }
    if raw_format == RawRecordingFormat::Mjpeg {
        tracing::warn!(codec = ?requested_codec, "recording encoder output appears to be MJPEG; recording will transcode");
    } else if probe_codec != requested_codec {
        tracing::warn!(requested = ?requested_codec, actual = ?probe_codec, "recording encoder fell back to alternate codec");
    }
    if let Some(parent) = config.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("recording dir create failed: {err}"))?;
    }
    let file = std::fs::File::create(&config.output_path).map_err(|err| format!("recording file open failed: {err}"))?;
    let mut runtime = RecordingEncoderRuntime {
        writer: std::io::BufWriter::new(file),
        ts_writer: config.timestamps_path.as_deref().map(RecordingTimestampWriter::open).transpose()?,
        encoder,
        codec: probe_codec,
        raw_format,
        format_tracker: None,
        pipeline_graph: config.pipeline_graph,
        pool: BufferPool::with_capacity(1, 1),
        pool_capacity: 1,
        bitstream: RecordingBitstream::Unknown,
        convert_buf: Vec::new(),
        config_cache: RecordingConfigCache::default(),
        wrote_prefix: false,
        pending_before_config: std::collections::VecDeque::new(),
        pending_bytes: 0,
        stats: RecordingStats::default(),
        last_frame_meta: None,
    };
    runtime.set_raw_format(raw_format);

    while let Some((image, ts_ms)) = rx.next() {
        runtime.encode_image(image, ts_ms)?;
    }
    runtime.flush_delayed_packets()?;

    runtime.writer.flush().map_err(|err| format!("recording flush failed: {err}"))?;
    if let Some(writer) = runtime.ts_writer.as_mut() {
        writer.flush()?;
    }
    if runtime.stats.frames == 0 {
        return Err("no encoded frames recorded".to_string());
    }
    Ok(runtime.stats)
}

fn run_shadow_recorder_worker(rx: StdReceiver<ShadowRecorderJob>, config: ShadowRecorderConfig) -> std::result::Result<(), String> {
    let ShadowRecorderConfig { shadow_dir, codec, segment_ms, window_ms, format_tracker } = config;
    std::fs::create_dir_all(&shadow_dir).map_err(|err| format!("shadow dir create failed: {err}"))?;
    cleanup_shadow_dir_sync(&shadow_dir)?;

    let mut segment_start_ms = current_time_ms();
    // Keep segment files readable for capture. Tune this interval to balance write pressure
    // (lower system load) against capture freshness.
    let mut last_flush_ms = segment_start_ms;
    let flush_interval_ms = shadow_flush_interval_ms();
    let config_scan_interval_ms = shadow_config_scan_interval_ms();
    let writer_capacity = shadow_writer_buffer_bytes();
    let mut last_config_scan_ms = segment_start_ms.saturating_sub(config_scan_interval_ms);
    let raw_format = RawRecordingFormat::from_codec(codec);
    format_tracker.store(raw_format.to_u8(), Ordering::Release);
    let mut writer = open_shadow_segment_sync(&shadow_dir, segment_start_ms, codec, writer_capacity)?;
    let mut bitstream = RecordingBitstream::Unknown;
    let mut config_cache = RecordingConfigCache::default();
    let mut wrote_prefix = false;
    let mut convert_buf: Vec<u8> = Vec::new();
    let mut pending_before_config: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
    let mut pending_bytes: usize = 0;
    let pending_max_bytes: usize = 4 * 1024 * 1024; // 4MiB safety cap
    let mut stats = RecordingStats { raw_format: Some(raw_format), ..RecordingStats::default() };
    stats.raw_format = Some(raw_format);

    if let Some(prefix) = config_cache.prefix(codec) {
        writer.write_all(&prefix).map_err(|err| format!("shadow segment write failed: {err}"))?;
        wrote_prefix = true;
    }

    while let Ok(job) = rx.recv() {
        match job {
            ShadowRecorderJob::Stop => break,
            ShadowRecorderJob::Chunk { data, ts_ms } => {
                // Producer timestamps each chunk at ingress; reuse it to avoid extra syscalls.
                let now_ms = ts_ms.max(last_flush_ms);
                let clock_ms = now_ms;
                if clock_ms.saturating_sub(segment_start_ms) >= segment_ms {
                    writer.flush().map_err(|err| format!("shadow segment flush failed: {err}"))?;
                    let cutoff = now_ms.saturating_sub(window_ms);
                    let _ = cleanup_shadow_segments_sync(&shadow_dir, cutoff, codec);
                    segment_start_ms = clock_ms;
                    last_flush_ms = now_ms;
                    writer = open_shadow_segment_sync(&shadow_dir, segment_start_ms, codec, writer_capacity)?;
                    bitstream = RecordingBitstream::Unknown;
                    wrote_prefix = false;
                    if let Some(prefix) = config_cache.prefix(codec) {
                        writer.write_all(&prefix).map_err(|err| format!("shadow segment write failed: {err}"))?;
                        wrote_prefix = true;
                    }
                }

                // If the upstream encoder is misconfigured and produces MJPEG, fail fast so we
                // don't silently write garbage into a .h264/.h265 segment file.
                if is_mjpeg_payload(data.as_ref()) {
                    return Err("shadow recorder received MJPEG; configure encoder_id to h264/h265".to_string());
                }

                // Treat the stream as "live encoded" and just persist the bytestream.
                // Many sources deliver a raw AnnexB bytestream in arbitrary chunk boundaries,
                // so do not require each chunk to start with a startcode.
                //
                // If we detect length-prefixed NAL units, convert to AnnexB so segments are
                // concat-friendly. If conversion fails, fall back to writing bytes as-is rather
                // than producing empty segments.
                let mut out_payload: &[u8] = data.as_ref();
                if bitstream == RecordingBitstream::Unknown {
                    if is_annexb_prefix(out_payload) || find_annexb_start(out_payload, 0).is_some() {
                        bitstream = RecordingBitstream::AnnexB;
                    } else {
                        bitstream = RecordingBitstream::LengthPrefixed;
                    }
                }
                if bitstream == RecordingBitstream::LengthPrefixed {
                    if length_prefixed_to_annexb_best_effort(out_payload, &mut convert_buf) {
                        out_payload = convert_buf.as_slice();
                    } else {
                        // Likely AnnexB bytestream split across chunks; accept raw bytes.
                        bitstream = RecordingBitstream::AnnexB;
                    }
                }

                // Cache VPS/SPS/PPS so new segments can start decodable without requiring the
                // encoder to repeat headers each segment. Once ready, scan less frequently.
                let should_scan_config = !config_cache.ready(codec) || !wrote_prefix || now_ms.saturating_sub(last_config_scan_ms) >= config_scan_interval_ms;
                if should_scan_config {
                    config_cache.update_from_annexb(codec, out_payload);
                    last_config_scan_ms = now_ms;
                }

                // If we don't yet have VPS/SPS/PPS, buffer early payloads so the very first
                // segment becomes decodable once config arrives, rather than producing an empty
                // or parameter-less stream that ffmpeg cannot remux.
                if !config_cache.ready(codec) && !wrote_prefix {
                    let mut v = Vec::with_capacity(out_payload.len());
                    v.extend_from_slice(out_payload);
                    pending_bytes = pending_bytes.saturating_add(v.len());
                    pending_before_config.push_back(v);
                    while pending_bytes > pending_max_bytes {
                        if let Some(dropped) = pending_before_config.pop_front() {
                            pending_bytes = pending_bytes.saturating_sub(dropped.len());
                        } else {
                            pending_bytes = 0;
                            break;
                        }
                    }
                    continue;
                }

                if !wrote_prefix {
                    if let Some(prefix) = config_cache.prefix(codec) {
                        writer.write_all(&prefix).map_err(|err| format!("shadow segment write failed: {err}"))?;
                        wrote_prefix = true;
                    }
                }

                if wrote_prefix && !pending_before_config.is_empty() {
                    while let Some(buf) = pending_before_config.pop_front() {
                        pending_bytes = pending_bytes.saturating_sub(buf.len());
                        writer.write_all(&buf).map_err(|err| format!("shadow segment write failed: {err}"))?;
                        stats.bytes = stats.bytes.saturating_add(buf.len() as u64);
                    }
                }

                writer.write_all(out_payload).map_err(|err| format!("shadow segment write failed: {err}"))?;
                stats.record_ts(ts_ms);
                stats.frames = stats.frames.saturating_add(1);
                stats.bytes = stats.bytes.saturating_add(out_payload.len() as u64);

                // Periodic flush so captures see up-to-date bytes even mid-segment.
                if now_ms.saturating_sub(last_flush_ms) >= flush_interval_ms {
                    writer.flush().map_err(|err| format!("shadow segment flush failed: {err}"))?;
                    last_flush_ms = now_ms;
                }
            }
        }
    }

    writer.flush().map_err(|err| format!("shadow recorder flush failed: {err}"))?;
    Ok(())
}

impl RecordingEncoderRuntime {
    fn set_raw_format(&mut self, raw_format: RawRecordingFormat) {
        self.raw_format = raw_format;
        self.stats.raw_format = Some(raw_format);
        if let Some(tracker) = &self.format_tracker {
            tracker.store(raw_format.to_u8(), Ordering::Release);
        }
    }

    fn ensure_rgb24_pool(&mut self, bytes: usize) -> &BufferPool {
        let need = bytes.max(1);
        if self.pool_capacity < need {
            self.pool = BufferPool::with_capacity(1, need);
            self.pool_capacity = need;
        }
        &self.pool
    }

    fn dynamic_image_to_rgb24_frame(&mut self, image: &image::DynamicImage, ts_ms: u64) -> Option<FrameLease> {
        let width = image.width().max(1);
        let height = image.height().max(1);
        let resolution = Resolution::new(width, height)?;
        let bytes = width as usize * height as usize * 3;
        let pool = self.ensure_rgb24_pool(bytes);
        let mut lease = pool.lease();
        lease.resize(bytes);

        let out = lease.as_mut_slice();
        if !write_rgb24(image, out) {
            return None;
        }

        let stride = (width as usize * 3).max(1);
        let meta = FrameMeta::new(MediaFormat::new(FourCc::new(*b"RG24"), resolution, ColorSpace::Unknown), ts_ms);
        Some(FrameLease::single_plane(meta, lease, bytes, stride))
    }

    fn write_encoded_payload(&mut self, payload: &[u8], ts_ms: u64) -> std::result::Result<(), String> {
        let mut raw_format = self.raw_format;
        write_payload_to_raw(
            &mut self.writer,
            self.codec,
            &mut raw_format,
            self.format_tracker.as_ref(),
            &mut self.bitstream,
            &mut self.convert_buf,
            &mut self.config_cache,
            &mut self.wrote_prefix,
            &mut self.pending_before_config,
            &mut self.pending_bytes,
            4 * 1024 * 1024,
            &mut self.stats,
            &mut self.ts_writer,
            payload,
            ts_ms,
            true,
        )?;
        if raw_format != self.raw_format {
            self.set_raw_format(raw_format);
        }
        Ok(())
    }

    fn estimated_frame_gap_ms(&self) -> u64 {
        match (self.stats.frames, self.stats.first_ts_ms, self.stats.last_ts_ms) {
            (frames, Some(first), Some(last)) if frames > 1 && last > first => (last.saturating_sub(first) / (frames - 1)).max(1),
            _ => 1,
        }
    }

    fn flush_delayed_packets(&mut self) -> std::result::Result<(), String> {
        let Some(meta) = self.last_frame_meta.clone() else {
            return Ok(());
        };
        let any = self.encoder.as_ref() as &dyn std::any::Any;
        let delayed_packets = if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegH264Encoder>() {
            encoder.0.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegH265Encoder>() {
            encoder.0.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegMjpegEncoder>() {
            encoder.0.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegVideoEncoder>() {
            encoder.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else {
            return Ok(());
        };

        if delayed_packets.is_empty() {
            return Ok(());
        }

        let step_ms = self.estimated_frame_gap_ms();
        let mut ts_cursor = self.stats.last_ts_ms.unwrap_or(meta.timestamp);
        let delayed_frames = delayed_packets.len();
        for frame in delayed_packets {
            let planes = frame.planes();
            let Some(plane) = planes.first() else {
                continue;
            };
            ts_cursor = ts_cursor.saturating_add(step_ms);
            self.write_encoded_payload(plane.data(), ts_cursor)?;
        }
        tracing::debug!(delayed_frames, step_ms, "flushed delayed encoder packets on recording stop");
        Ok(())
    }

    fn encode_image(&mut self, image: Arc<image::DynamicImage>, ts_ms: u64) -> std::result::Result<(), String> {
        let image = if let Some(graph) = self.pipeline_graph.as_ref() {
            match graph.process((*image).clone()) {
                Some(img) => img,
                None => return Ok(()),
            }
        } else {
            (*image).clone()
        };
        let Some(frame) = self.dynamic_image_to_rgb24_frame(&image, ts_ms) else {
            return Err("recording rgb24 conversion failed".to_string());
        };
        self.last_frame_meta = Some(frame.meta().clone());
        let encoded_frame = match self.encoder.process(frame) {
            Ok(frame) => frame,
            Err(styx::codec::CodecError::Backpressure) => return Ok(()),
            Err(err) => return Err(format!("recording encode failed: {err}")),
        };
        let planes = encoded_frame.planes();
        let Some(plane) = planes.first() else {
            return Err("recording encode produced no planes".to_string());
        };
        self.write_encoded_payload(plane.data(), ts_ms)
    }
}

fn encoder_output_matches(codec: RecordingCodec, desc: &styx::codec::CodecDescriptor) -> bool {
    let name = desc.name.to_ascii_lowercase();
    let impl_name = desc.impl_name.to_ascii_lowercase();
    let output = String::from_utf8_lossy(&desc.output.to_u32().to_le_bytes()).to_ascii_lowercase();
    match codec {
        RecordingCodec::H264 => ["h264", "avc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)),
        RecordingCodec::H265 => ["h265", "hevc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)),
    }
}

fn raw_format_from_desc(desc: &styx::codec::CodecDescriptor) -> Option<RawRecordingFormat> {
    let name = desc.name.to_ascii_lowercase();
    let impl_name = desc.impl_name.to_ascii_lowercase();
    let output = String::from_utf8_lossy(&desc.output.to_u32().to_le_bytes()).to_ascii_lowercase();
    if ["mjpeg", "jpeg", "mjpg"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)) {
        return Some(RawRecordingFormat::Mjpeg);
    }
    if ["h264", "avc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)) {
        return Some(RawRecordingFormat::H264);
    }
    if ["h265", "hevc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)) {
        return Some(RawRecordingFormat::H265);
    }
    None
}

fn select_encoder_for_codec(codec: RecordingCodec, encoder_hint: Option<&str>, allow_mjpeg: bool) -> std::result::Result<(RawRecordingFormat, Arc<dyn Codec>), String> {
    let registry = CodecRegistry::with_enabled_codecs().map_err(|err| format!("codec registry init failed: {err}"))?;
    let handle = registry.handle();
    let input = FourCc::new(*b"RG24");
    let targets: &[&str] = match codec {
        RecordingCodec::H264 => &["h264", "avc"],
        RecordingCodec::H265 => &["h265", "hevc"],
    };
    handle.set_policy(CodecPolicy::builder(input).prefer_hardware(true).build());

    let mut last_mismatch: Option<String> = None;
    let check_encoder = |encoder: Arc<dyn Codec>, label: &str| -> std::result::Result<(RawRecordingFormat, Arc<dyn Codec>), String> {
        let desc = encoder.descriptor();
        if let Some(raw_format) = raw_format_from_desc(desc) {
            if raw_format.as_codec() == Some(codec) {
                return Ok((raw_format, encoder));
            }
            if raw_format == RawRecordingFormat::Mjpeg && allow_mjpeg {
                return Ok((raw_format, encoder));
            }
        }
        if encoder_output_matches(codec, desc) {
            return Ok((RawRecordingFormat::from_codec(codec), encoder));
        }
        Err(format!("encoder '{label}' did not produce {:?}", codec))
    };

    if let Some(hint) = encoder_hint.map(str::trim).filter(|value| !value.is_empty()) {
        if let Ok(encoder) = handle.lookup_named_kind(input, CodecKind::Encoder, hint).or_else(|_| handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, hint)) {
            match check_encoder(encoder, hint) {
                Ok(found) => return Ok(found),
                Err(err) => {
                    tracing::warn!(requested = ?codec, hint = %hint, error = %err, "recording encoder hint did not match requested codec; trying codec family fallbacks");
                    last_mismatch = Some(err);
                }
            }
        }
    }

    for target in targets {
        if let Ok(encoder) = handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, target) {
            match check_encoder(encoder, target) {
                Ok(found) => return Ok(found),
                Err(err) => {
                    last_mismatch = Some(err);
                    continue;
                }
            }
        }
    }

    if allow_mjpeg {
        for target in ["mjpeg", "jpeg", "mjpg"] {
            if let Ok(encoder) = handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, target) {
                return Ok((RawRecordingFormat::Mjpeg, encoder));
            }
        }
    }

    if let Some(err) = last_mismatch {
        Err(err)
    } else {
        Err(format!("encoder not found for {:?}", codec))
    }
}

fn select_recording_encoder(preferred: RecordingCodec, encoder_hint: Option<&str>) -> std::result::Result<(RawRecordingFormat, RecordingCodec, Arc<dyn Codec>), String> {
    if let Ok((raw_format, encoder)) = select_encoder_for_codec(preferred, encoder_hint, false) {
        let probe_codec = raw_format.as_codec().unwrap_or(preferred);
        return Ok((raw_format, probe_codec, encoder));
    }
    let fallback = match preferred {
        RecordingCodec::H264 => RecordingCodec::H265,
        RecordingCodec::H265 => RecordingCodec::H264,
    };
    if let Ok((raw_format, encoder)) = select_encoder_for_codec(fallback, encoder_hint, false) {
        let probe_codec = raw_format.as_codec().unwrap_or(fallback);
        return Ok((raw_format, probe_codec, encoder));
    }
    let (raw_format, encoder) = select_encoder_for_codec(preferred, encoder_hint, true)?;
    let probe_codec = raw_format.as_codec().unwrap_or(preferred);
    Ok((raw_format, probe_codec, encoder))
}

struct RecordingFrameParams<'a> {
    output_path: &'a Path,
    raw_path: &'a Path,
    container: RecordingContainer,
    codec: RecordingCodec,
    duration_ms: Option<u64>,
    fps: Option<f32>,
    settings: Option<crate::ipc::RecordingSettings>,
    encoder_hint: Option<String>,
    frame_ts_path: Option<PathBuf>,
}

async fn record_frame_stream(mut source: RecordingFrameSource, mut stop_rx: oneshot::Receiver<()>, params: RecordingFrameParams<'_>) -> std::result::Result<RecordingStats, String> {
    let RecordingFrameParams { output_path, raw_path, container, codec, duration_ms, fps, settings, encoder_hint, frame_ts_path } = params;
    let record_path = if matches!(container, RecordingContainer::Mp4) { raw_path } else { output_path };
    if let Some(parent) = record_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| format!("recording dir create failed: {err}"))?;
    }

    let rewrite_timestamps_path = frame_ts_path.clone();
    let worker = RecordingEncoderWorker::start(RecordingEncoderConfig {
        output_path: record_path.to_path_buf(),
        codec,
        pipeline_graph: match &source {
            RecordingFrameSource::Pipeline { graph, .. } => Some(graph.clone()),
            _ => None,
        },
        encoder_hint: encoder_hint.clone(),
        timestamps_path: frame_ts_path,
    })?;

    // Capture wall span is kept for diagnostics/fallback, but MP4 remux timing should prefer
    // observed frame cadence so playback speed matches what was actually encoded.
    let wall_start_ms = current_time_ms();
    let fps_drop = settings.as_ref().and_then(|s| s.fps).filter(|v| *v > 0.0);
    let min_gap = fps_drop.map(|value| Duration::from_secs_f64(1.0 / f64::from(value.max(f32::EPSILON))));
    let mut last_write: Option<Instant> = None;
    let deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));
    let stop_grace = Duration::from_millis(recording_stop_grace_ms());
    let mut stop_deadline: Option<Instant> = None;

    loop {
        let recv_fut = match &mut source {
            RecordingFrameSource::Multiplex { rx } => rx.recv(),
            RecordingFrameSource::Raw { rx } => rx.recv(),
            RecordingFrameSource::Pipeline { rx, .. } => rx.recv(),
        };

        if let Some(stop_until) = stop_deadline {
            if let Some(duration_until) = deadline {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    _ = sleep_until(duration_until) => break,
                    recv = recv_fut => match recv {
                        Ok(image) => {
                            if let Some(min_gap) = min_gap {
                                if let Some(last) = last_write {
                                    if last.elapsed() < min_gap {
                                        continue;
                                    }
                                }
                                last_write = Some(Instant::now());
                            }
                            let ts_ms = current_time_ms();
                            worker.send_image(image, ts_ms);
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            } else {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    recv = recv_fut => match recv {
                        Ok(image) => {
                            if let Some(min_gap) = min_gap {
                                if let Some(last) = last_write {
                                    if last.elapsed() < min_gap {
                                        continue;
                                    }
                                }
                                last_write = Some(Instant::now());
                            }
                            let ts_ms = current_time_ms();
                            worker.send_image(image, ts_ms);
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        } else if let Some(duration_until) = deadline {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                _ = sleep_until(duration_until) => break,
                recv = recv_fut => match recv {
                    Ok(image) => {
                        if let Some(min_gap) = min_gap {
                            if let Some(last) = last_write {
                                if last.elapsed() < min_gap {
                                    continue;
                                }
                            }
                            last_write = Some(Instant::now());
                        }
                        let ts_ms = current_time_ms();
                        worker.send_image(image, ts_ms);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        } else {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                recv = recv_fut => match recv {
                    Ok(image) => {
                        if let Some(min_gap) = min_gap {
                            if let Some(last) = last_write {
                                if last.elapsed() < min_gap {
                                    continue;
                                }
                            }
                            last_write = Some(Instant::now());
                        }
                        let ts_ms = current_time_ms();
                        worker.send_image(image, ts_ms);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    let wall_end_ms = current_time_ms();
    let stats = tokio::task::spawn_blocking(move || worker.stop()).await.map_err(|_| "recording worker join failed".to_string())??;
    if let Some(path) = rewrite_timestamps_path.as_deref() {
        maybe_rewrite_encoded_frame_timestamps(path, &stats, wall_start_ms, wall_end_ms).await?;
    }

    if matches!(container, RecordingContainer::Mp4) {
        let wall_span_ms = wall_end_ms.saturating_sub(wall_start_ms);
        // Fall back to wall span only when we don't have a reliable per-frame timestamp span.
        let wall_derived_fps = if stats.frames > 0 && wall_span_ms > 0 { Some((stats.frames as f32 * 1000.0) / (wall_span_ms as f32)) } else { None };
        let derived_fps = stats.derived_fps().or(wall_derived_fps);
        let remux_fps = derived_fps.or(fps);
        let transcode = settings.as_ref().is_some_and(|s| s.bitrate_bps.is_some() || s.gop.is_some() || s.quality.is_some() || s.max_width.is_some() || s.max_height.is_some());
        let raw_format = stats.raw_format.unwrap_or_else(|| RawRecordingFormat::from_codec(codec));
        let needs_transcode = transcode || raw_format.as_codec() != Some(codec);
        let raw_path = raw_path.to_path_buf();
        let output_path = output_path.to_path_buf();
        let settings = if needs_transcode { settings } else { None };
        if needs_transcode {
            finalize_recording_mp4(raw_path, output_path, raw_format, codec, remux_fps, settings).await?;
        } else {
            finalize_recording_mp4(raw_path, output_path, raw_format, codec, remux_fps, None).await?;
        }
    }

    Ok(stats)
}

fn write_encoded_chunks_to_raw(path: PathBuf, codec: RecordingCodec, timestamps_path: Option<PathBuf>, rx: std::sync::mpsc::Receiver<(Arc<[u8]>, u64)>) -> std::result::Result<RecordingStats, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("recording dir create failed: {err}"))?;
    }
    let file = std::fs::File::create(&path).map_err(|err| format!("recording file open failed: {err}"))?;
    let mut writer = std::io::BufWriter::new(file);

    let mut raw_format = RawRecordingFormat::from_codec(codec);
    let mut bitstream = RecordingBitstream::Unknown;
    let mut convert_buf = Vec::new();
    let mut config_cache = RecordingConfigCache::default();
    let mut wrote_prefix = false;
    let mut pending_before_config: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
    let mut pending_bytes: usize = 0;
    let pending_max_bytes: usize = 4 * 1024 * 1024;
    let mut ts_writer = timestamps_path.as_deref().map(RecordingTimestampWriter::open).transpose()?;
    let mut stats = RecordingStats { raw_format: Some(raw_format), ..RecordingStats::default() };

    while let Ok((payload, ts_ms)) = rx.recv() {
        write_payload_to_raw(
            &mut writer,
            codec,
            &mut raw_format,
            None,
            &mut bitstream,
            &mut convert_buf,
            &mut config_cache,
            &mut wrote_prefix,
            &mut pending_before_config,
            &mut pending_bytes,
            pending_max_bytes,
            &mut stats,
            &mut ts_writer,
            payload.as_ref(),
            ts_ms,
            true,
        )?;
    }

    writer.flush().map_err(|err| format!("recording flush failed: {err}"))?;
    if let Some(writer) = ts_writer.as_mut() {
        writer.flush()?;
    }
    if stats.frames == 0 {
        return Err("no encoded frames recorded".to_string());
    }
    Ok(stats)
}

async fn record_encoded_stream(
    mut rx: Receiver<EncodedFrame>,
    mut stop_rx: oneshot::Receiver<()>,
    output_path: &Path,
    codec: RecordingCodec,
    duration_ms: Option<u64>,
    timestamps_path: Option<PathBuf>,
    consumer_touch: Option<Arc<AtomicU64>>,
) -> std::result::Result<RecordingStats, String> {
    let (tx, write_rx) = std::sync::mpsc::channel::<(Arc<[u8]>, u64)>();
    let output_path = output_path.to_path_buf();
    let write_task = tokio::task::spawn_blocking(move || write_encoded_chunks_to_raw(output_path, codec, timestamps_path, write_rx));
    // Encoded stream chunk timestamps are source-dependent and may not be milliseconds.
    // Stamp chunks with local wall clock so recording sidecars are in real-time ms.
    let mut last_wall_ts_ms = current_time_ms();

    let stop_grace = Duration::from_millis(recording_stop_grace_ms());
    let deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));
    let mut stop_deadline: Option<Instant> = None;

    loop {
        if let Some(stop_until) = stop_deadline {
            if let Some(duration_until) = deadline {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    _ = sleep_until(duration_until) => break,
                    recv = rx.recv() => match recv {
                        Ok(chunk) => {
                            if let Some(touch) = consumer_touch.as_ref() {
                                touch.store(current_time_ms(), Ordering::Relaxed);
                            }
                            let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                            last_wall_ts_ms = wall_ts_ms;
                            if tx.send((chunk.data, wall_ts_ms)).is_err() {
                                break;
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            } else {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    recv = rx.recv() => match recv {
                        Ok(chunk) => {
                            if let Some(touch) = consumer_touch.as_ref() {
                                touch.store(current_time_ms(), Ordering::Relaxed);
                            }
                            let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                            last_wall_ts_ms = wall_ts_ms;
                            if tx.send((chunk.data, wall_ts_ms)).is_err() {
                                break;
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        } else if let Some(duration_until) = deadline {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                _ = sleep_until(duration_until) => break,
                recv = rx.recv() => match recv {
                    Ok(chunk) => {
                        if let Some(touch) = consumer_touch.as_ref() {
                            touch.store(current_time_ms(), Ordering::Relaxed);
                        }
                        let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                        last_wall_ts_ms = wall_ts_ms;
                        if tx.send((chunk.data, wall_ts_ms)).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        } else {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                recv = rx.recv() => match recv {
                    Ok(chunk) => {
                        let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                        last_wall_ts_ms = wall_ts_ms;
                        if tx.send((chunk.data, wall_ts_ms)).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    drop(tx);
    write_task.await.map_err(|_| "recording writer join failed".to_string())?
}

#[allow(clippy::too_many_arguments)]
async fn record_encoded_session(
    rx: Receiver<EncodedFrame>,
    stop_rx: oneshot::Receiver<()>,
    output_path: &Path,
    raw_path: &Path,
    container: RecordingContainer,
    source_codec: RecordingCodec,
    target_codec: RecordingCodec,
    duration_ms: Option<u64>,
    fps: Option<f32>,
    settings: Option<crate::ipc::RecordingSettings>,
    timestamps_path: Option<PathBuf>,
    consumer_touch: Option<Arc<AtomicU64>>,
) -> std::result::Result<RecordingStats, String> {
    let record_path = if matches!(container, RecordingContainer::Mp4) { raw_path } else { output_path };
    let wall_start_ms = current_time_ms();
    let rewrite_timestamps_path = timestamps_path.clone();
    let stats = record_encoded_stream(rx, stop_rx, record_path, source_codec, duration_ms, timestamps_path, consumer_touch).await?;
    let wall_end_ms = current_time_ms();
    if let Some(path) = rewrite_timestamps_path.as_deref() {
        maybe_rewrite_encoded_frame_timestamps(path, &stats, wall_start_ms, wall_end_ms).await?;
    }

    if matches!(container, RecordingContainer::Mp4) {
        let wall_span_ms = wall_end_ms.saturating_sub(wall_start_ms);
        let wall_derived_fps = if stats.frames > 0 && wall_span_ms > 0 { Some((stats.frames as f32 * 1000.0) / (wall_span_ms as f32)) } else { None };
        let derived_fps = stats.derived_fps().or(wall_derived_fps);
        let remux_fps = derived_fps.or(fps);
        let transcode = settings.as_ref().is_some_and(|s| s.bitrate_bps.is_some() || s.gop.is_some() || s.quality.is_some() || s.max_width.is_some() || s.max_height.is_some());
        let raw_format = stats.raw_format.unwrap_or_else(|| RawRecordingFormat::from_codec(source_codec));
        let needs_transcode = transcode || raw_format.as_codec() != Some(target_codec);
        let raw_path = raw_path.to_path_buf();
        let output_path = output_path.to_path_buf();
        let settings = if needs_transcode { settings } else { None };
        if needs_transcode {
            finalize_recording_mp4(raw_path, output_path, raw_format, target_codec, remux_fps, settings).await?;
        } else {
            finalize_recording_mp4(raw_path, output_path, raw_format, target_codec, remux_fps, None).await?;
        }
    }

    Ok(stats)
}

async fn maybe_rewrite_encoded_frame_timestamps(path: &Path, stats: &RecordingStats, wall_start_ms: u64, wall_end_ms: u64) -> std::result::Result<(), String> {
    if !rewrite_encoded_frame_timestamps_to_wall_enabled() {
        return Ok(());
    }
    if stats.frames == 0 {
        return Ok(());
    }
    let wall_span_ms = wall_end_ms.saturating_sub(wall_start_ms);
    if wall_span_ms == 0 {
        return Ok(());
    }
    let recorded_span_ms = match (stats.first_ts_ms, stats.last_ts_ms) {
        (Some(first), Some(last)) if last > first => Some(last.saturating_sub(first)),
        _ => None,
    };
    // Optional legacy behavior: normalize encoded timestamp sidecars to wall-clock capture span.
    //
    // Disabled by default because stretching timestamps to wall span can make playback cadence
    // diverge from the actual encoded frame cadence.
    tracing::info!(
        path = %path.display(),
        frames = stats.frames,
        wall_span_ms,
        recorded_span_ms = ?recorded_span_ms,
        "rewriting encoded frame timestamps to wall-clock span"
    );
    rewrite_timestamp_file(path, wall_start_ms, wall_end_ms, stats.frames).await
}

fn rewrite_encoded_frame_timestamps_to_wall_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("HELIOS_RECORDING_REWRITE_FRAME_TS_TO_WALL")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false)
    })
}

async fn rewrite_timestamp_file(path: &Path, start_ms: u64, end_ms: u64, frames: u64) -> std::result::Result<(), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("recording timestamp dir create failed: {err}"))?;
        }
        let file = std::fs::File::create(&path).map_err(|err| format!("recording timestamp file rewrite failed: {err}"))?;
        let mut writer = std::io::BufWriter::new(file);
        if frames <= 1 || end_ms <= start_ms {
            writeln!(writer, "{start_ms}").map_err(|err| format!("recording timestamp rewrite failed: {err}"))?;
        } else {
            let span = end_ms.saturating_sub(start_ms) as u128;
            let denom = (frames.saturating_sub(1)) as u128;
            for index in 0..frames {
                let offset = (span.saturating_mul(index as u128)) / denom;
                let ts = start_ms.saturating_add(offset as u64);
                writeln!(writer, "{ts}").map_err(|err| format!("recording timestamp rewrite failed: {err}"))?;
            }
        }
        writer.flush().map_err(|err| format!("recording timestamp rewrite flush failed: {err}"))
    })
    .await
    .map_err(|_| "recording timestamp rewrite task failed".to_string())?
}

#[allow(dead_code)]
async fn probe_prefix_from_segments(selection: &[(u64, PathBuf)], codec: RecordingCodec) -> Option<Vec<u8>> {
    let mut cache = RecordingConfigCache::default();
    let mut scanned = 0usize;
    let max_scan = 1024 * 1024; // 1MiB
    let mut buf = vec![0u8; 256 * 1024];
    for (_, path) in selection {
        if cache.ready(codec) || scanned >= max_scan {
            break;
        }
        let mut file = File::open(path).await.ok()?;
        loop {
            if cache.ready(codec) || scanned >= max_scan {
                break;
            }
            let n = file.read(&mut buf).await.ok()?;
            if n == 0 {
                break;
            }
            scanned = scanned.saturating_add(n);
            cache.update_from_annexb(codec, &buf[..n]);
        }
    }
    cache.prefix(codec)
}

#[allow(clippy::too_many_arguments)]
async fn record_shadow_segments_session(
    _stream_id: Uuid,
    shadow_dir: &Path,
    codec: RecordingCodec,
    output_path: &Path,
    raw_path: &Path,
    container: RecordingContainer,
    started_at_ms: u64,
    duration_ms: Option<u64>,
    fps: Option<f32>,
    settings: Option<crate::ipc::RecordingSettings>,
    mut stop_rx: oneshot::Receiver<()>,
) -> std::result::Result<RecordingStats, String> {
    if fs::metadata(shadow_dir).await.is_err() {
        return Err("shadow recorder data not found".to_string());
    }

    let record_path = if matches!(container, RecordingContainer::Mp4) { raw_path } else { output_path };
    let wall_start_ms = started_at_ms;

    // Wait for the requested window to elapse, then do a single shadow capture ending "now".
    // This avoids racing the shadow writer's buffered IO while it's still actively appending to
    // the live segment file.
    let deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));
    let stop_grace = Duration::from_millis(recording_stop_grace_ms());
    let mut stop_deadline: Option<Instant> = None;

    loop {
        match (stop_deadline, deadline) {
            (Some(stop_until), Some(dl)) => {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    _ = sleep_until(dl) => break,
                }
            }
            (Some(stop_until), None) => {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                }
            }
            (None, Some(dl)) => {
                tokio::select! {
                    _ = &mut stop_rx => {
                        stop_deadline = Some(Instant::now() + stop_grace);
                    }
                    _ = sleep_until(dl) => break,
                }
            }
            (None, None) => {
                tokio::select! {
                    _ = &mut stop_rx => {
                        stop_deadline = Some(Instant::now() + stop_grace);
                    }
                }
            }
        }
    }

    // Give the shadow worker a chance to flush the latest bytes to disk.
    // (Shadow flush interval is 250ms; this keeps regular recordings from "missing the tail".)
    tokio::time::sleep(Duration::from_millis(400)).await;

    let wall_end_ms = current_time_ms();
    let window_ms = if let Some(ms) = duration_ms.filter(|ms| *ms > 0) { ms } else { wall_end_ms.saturating_sub(started_at_ms) };
    if window_ms == 0 {
        return Err("recording duration must be > 0".to_string());
    }
    let max_window = shadow_window_ms();
    if window_ms > max_window {
        return Err(format!("recording duration ({window_ms}ms) exceeds shadow window ({max_window}ms); increase HELIOS_SHADOW_WINDOW_MS"));
    }

    let preroll_ms = shadow_segment_ms().max(1_000);
    let capture_ms = window_ms.saturating_add(preroll_ms).min(max_window);
    let total_bytes = capture_shadow_segments(shadow_dir, record_path, codec, capture_ms).await?;

    if matches!(container, RecordingContainer::Mp4) {
        let raw_format = RawRecordingFormat::from_codec(codec);
        let output = output_path.to_path_buf();
        let raw = raw_path.to_path_buf();
        // Shadow segments can start mid-GOP; pre-roll improves the chance ffmpeg sees an IDR
        // before the requested window. We then trim down to the requested duration.
        let forced_fps = probe_raw_frames(&raw, raw_format).await.ok().map(|frames| {
            let secs = (capture_ms as f32 / 1000.0).max(0.001);
            (frames as f32 / secs).clamp(1.0, 240.0)
        });
        let fps = forced_fps.or(fps);
        let pretrim = pretrim_output_path(&output);
        finalize_recording_mp4(raw, pretrim.clone(), raw_format, codec, fps, settings).await?;
        let trim_res = trim_mp4_to_last_window(&pretrim, &output, window_ms, codec).await;
        let _ = tokio::fs::remove_file(&pretrim).await;
        trim_res?;
    }

    Ok(RecordingStats { frames: 0, bytes: total_bytes, first_ts_ms: Some(wall_start_ms), last_ts_ms: Some(wall_end_ms), raw_format: Some(RawRecordingFormat::from_codec(codec)) })
}

async fn finalize_recording_mp4(
    raw_path: PathBuf,
    output_path: PathBuf,
    raw_format: RawRecordingFormat,
    codec: RecordingCodec,
    fps: Option<f32>,
    settings: Option<crate::ipc::RecordingSettings>,
) -> std::result::Result<(), String> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| format!("recording output dir create failed: {err}"))?;
    }
    let temp_path = temp_output_path(&output_path);
    let transcode =
        settings.as_ref().is_some_and(|s| s.bitrate_bps.is_some() || s.gop.is_some() || s.quality.is_some() || s.max_width.is_some() || s.max_height.is_some()) || raw_format.as_codec() != Some(codec);
    let result = if transcode { transcode_raw_to_mp4(&raw_path, &temp_path, raw_format, codec, fps, settings.as_ref()).await } else { remux_raw_to_mp4(&raw_path, &temp_path, raw_format, fps).await };
    if let Err(err) = result {
        let _ = tokio::fs::remove_file(&temp_path).await;
        // Default to cleanup: raw bitstreams can be large and are typically not useful to keep.
        if !keep_raw_on_record_fail() {
            let _ = tokio::fs::remove_file(&raw_path).await;
        }
        return Err(err);
    }

    // Guardrail: ffmpeg can sometimes produce a "valid" MP4 with zero streams when the input
    // is missing codec parameters (VPS/SPS/PPS) or contains no packets. Treat that as failure.
    let meta = tokio::fs::metadata(&temp_path).await.map_err(|err| format!("recording output stat failed: {err}"))?;
    if meta.len() < 1024 {
        let _ = tokio::fs::remove_file(&temp_path).await;
        if !keep_raw_on_record_fail() {
            let _ = tokio::fs::remove_file(&raw_path).await;
        }
        return Err("ffmpeg produced empty mp4 (no streams)".to_string());
    }

    if let Err(err) = tokio::fs::rename(&temp_path, &output_path).await {
        tracing::warn!(error = %err, "recording output rename failed; retrying");
        let _ = tokio::fs::remove_file(&output_path).await;
        if let Err(err2) = tokio::fs::rename(&temp_path, &output_path).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(format!("recording output rename failed: {err2}"));
        }
    }
    let _ = tokio::fs::remove_file(&raw_path).await;
    Ok(())
}

async fn probe_raw_frames(raw_path: &Path, raw_format: RawRecordingFormat) -> std::result::Result<u64, String> {
    let raw = raw_path.to_path_buf();
    let demux = raw_format.ffmpeg_demux().to_string();
    tokio::task::spawn_blocking(move || {
        fn run(raw: &Path, demux: &str, count_mode: &str, field: &str) -> std::result::Result<u64, String> {
            let mut cmd = Command::new("ffprobe");
            cmd.arg("-v").arg("error");
            cmd.arg("-f").arg(demux);
            cmd.arg(count_mode);
            cmd.arg("-select_streams").arg("v:0");
            cmd.arg("-show_entries").arg(format!("stream={field}"));
            cmd.arg("-of").arg("default=nw=1:nk=1");
            cmd.arg(raw);
            let output_res = cmd.output().map_err(|err| format!("ffprobe launch failed: {err}"))?;
            if !output_res.status.success() {
                let stderr = String::from_utf8_lossy(&output_res.stderr);
                let reason = stderr.trim();
                if reason.is_empty() {
                    return Err(format!("ffprobe failed with status {}", output_res.status));
                }
                return Err(format!("ffprobe failed: {reason}"));
            }
            let stdout = String::from_utf8_lossy(&output_res.stdout);
            let trimmed = stdout.trim();
            trimmed.parse::<u64>().map_err(|_| format!("ffprobe returned non-numeric frame count: {trimmed:?}"))
        }

        // Prefer decoding-backed frame counts. When the stream begins mid-GOP, `nb_read_frames`
        // matches what ffmpeg can actually output, while packets may include unusable slices.
        run(&raw, &demux, "-count_frames", "nb_read_frames").or_else(|_| run(&raw, &demux, "-count_packets", "nb_read_packets"))
    })
    .await
    .map_err(|_| "ffprobe task failed".to_string())?
}

async fn trim_mp4_to_last_window(input_path: &Path, output_path: &Path, window_ms: u64, codec: RecordingCodec) -> std::result::Result<(), String> {
    if window_ms == 0 {
        return Err("trim window must be > 0".to_string());
    }
    let input = input_path.to_path_buf();
    let output = output_path.to_path_buf();
    let temp = temp_output_path(&output);
    let temp_for_cmd = temp.clone();
    let secs = (window_ms as f64 / 1000.0).max(0.001);
    let run_res = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-sseof").arg(format!("-{secs:.03}"));
        cmd.arg("-i").arg(&input);
        cmd.arg("-t").arg(format!("{secs:.03}"));
        cmd.arg("-c:v").arg("copy");
        if matches!(codec, RecordingCodec::H265) {
            cmd.arg("-tag:v").arg("hvc1");
        }
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&temp_for_cmd);
        let output_res = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if !output_res.status.success() {
            let stderr = String::from_utf8_lossy(&output_res.stderr);
            let reason = stderr.trim();
            if reason.is_empty() {
                return Err(format!("ffmpeg failed with status {}", output_res.status));
            }
            return Err(format!("ffmpeg failed: {reason}"));
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "ffmpeg task failed".to_string())?;
    if let Err(err) = run_res {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(err);
    }

    let meta = tokio::fs::metadata(&temp).await.map_err(|err| format!("trim output stat failed: {err}"))?;
    if meta.len() < 1024 {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err("ffmpeg produced empty trimmed mp4 (no streams)".to_string());
    }

    if let Err(err) = tokio::fs::rename(&temp, &output).await {
        tracing::warn!(error = %err, "trim output rename failed; retrying");
        let _ = tokio::fs::remove_file(&output).await;
        if let Err(err2) = tokio::fs::rename(&temp, &output).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(format!("trim output rename failed: {err2}"));
        }
    }
    Ok(())
}

async fn remux_raw_to_mp4(raw_path: &Path, output_path: &Path, raw_format: RawRecordingFormat, fps: Option<f32>) -> std::result::Result<(), String> {
    let raw = raw_path.to_path_buf();
    let output = output_path.to_path_buf();
    let fps_value = fps.filter(|value| value.is_finite() && *value > 0.0).map(|v| v.clamp(1.0, 240.0)).unwrap_or(30.0);
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-fflags").arg("+genpts");
        cmd.arg("-r").arg(format!("{fps_value:.03}"));
        cmd.arg("-f").arg(raw_format.ffmpeg_demux());
        cmd.arg("-i").arg(&raw);
        cmd.arg("-c:v").arg("copy");
        if matches!(raw_format, RawRecordingFormat::H265) {
            cmd.arg("-tag:v").arg("hvc1");
        }
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&output);
        let output_res = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if output_res.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output_res.stderr);
        let reason = stderr.trim();
        if reason.is_empty() {
            Err(format!("ffmpeg failed with status {}", output_res.status))
        } else {
            Err(format!("ffmpeg failed: {reason}"))
        }
    })
    .await
    .map_err(|_| "ffmpeg task failed".to_string())?
}

async fn transcode_raw_to_mp4(
    raw_path: &Path,
    output_path: &Path,
    raw_format: RawRecordingFormat,
    codec: RecordingCodec,
    fps: Option<f32>,
    settings: Option<&crate::ipc::RecordingSettings>,
) -> std::result::Result<(), String> {
    let raw = raw_path.to_path_buf();
    let output = output_path.to_path_buf();
    let fps_value = fps.filter(|value| value.is_finite() && *value > 0.0).map(|v| v.clamp(1.0, 240.0)).unwrap_or(30.0);
    let cfg = settings.cloned().unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-fflags").arg("+genpts");
        cmd.arg("-r").arg(format!("{fps_value:.03}"));
        cmd.arg("-f").arg(raw_format.ffmpeg_demux());
        cmd.arg("-i").arg(&raw);
        if cfg.max_width.is_some() || cfg.max_height.is_some() {
            let width = cfg.max_width.unwrap_or(0);
            let height = cfg.max_height.unwrap_or(0);
            let scale = if width > 0 && height > 0 {
                format!("scale='min({width},iw)':'min({height},ih)':force_original_aspect_ratio=decrease")
            } else if width > 0 {
                format!("scale='min({width},iw)':-2:force_original_aspect_ratio=decrease")
            } else if height > 0 {
                format!("scale=-2:'min({height},ih)':force_original_aspect_ratio=decrease")
            } else {
                String::new()
            };
            if !scale.is_empty() {
                cmd.arg("-vf").arg(scale);
            }
        }
        match codec {
            RecordingCodec::H264 => {
                cmd.arg("-c:v").arg("libx264");
                // Keep output browser-friendly; some players reject High 4:4:4 profiles.
                cmd.arg("-pix_fmt").arg("yuv420p");
                cmd.arg("-profile:v").arg("high");
            }
            RecordingCodec::H265 => {
                cmd.arg("-c:v").arg("libx265");
            }
        }
        if let Some(bitrate) = cfg.bitrate_bps.filter(|v| *v > 0) {
            cmd.arg("-b:v").arg(bitrate.to_string());
        } else if let Some(crf) = cfg.quality.filter(|v| *v <= 51) {
            cmd.arg("-crf").arg(crf.to_string());
        }
        if let Some(gop) = cfg.gop.filter(|v| *v > 0) {
            cmd.arg("-g").arg(gop.to_string());
        }
        cmd.arg("-preset").arg("veryfast");
        if matches!(codec, RecordingCodec::H265) {
            cmd.arg("-tag:v").arg("hvc1");
        }
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&output);
        let output_res = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if output_res.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output_res.stderr);
        let reason = stderr.trim();
        if reason.is_empty() {
            Err(format!("ffmpeg failed with status {}", output_res.status))
        } else {
            Err(format!("ffmpeg failed: {reason}"))
        }
    })
    .await
    .map_err(|_| "ffmpeg task failed".to_string())?
}

async fn receive_snapshot_source_frame(rx: &mut Receiver<Arc<image::DynamicImage>>, pipeline_graph: Option<&crate::graph::GraphHandle>) -> Result<image::DynamicImage> {
    let deadline = Instant::now() + SNAPSHOT_SOURCE_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        let next = timeout(remaining, rx.recv()).await;
        let frame = match next {
            Ok(Ok(frame)) => frame,
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => return Err(Error::InvalidState("snapshot source stream closed")),
            Err(_) => return Err(Error::Timeout),
        };
        let image = (*frame).clone();
        if let Some(graph) = pipeline_graph {
            if let Some(processed) = graph.process(image) {
                return Ok(processed);
            }
            continue;
        }
        return Ok(image);
    }
}

fn encode_snapshot_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let width = image.width().max(1);
    let height = image.height().max(1);
    let wanted = width as usize * height as usize * 3;
    let mut rgb = vec![0u8; wanted];
    if !write_rgb24(image, &mut rgb) {
        return Err(Error::InvalidState("snapshot rgb24 conversion failed"));
    }
    use image::codecs::jpeg::JpegEncoder;
    use image::ColorType;

    let mut jpeg = Vec::<u8>::new();
    let quality = quality.clamp(1, 100);
    let mut enc = JpegEncoder::new_with_quality(&mut jpeg, quality);
    enc.encode(&rgb, width, height, ColorType::Rgb8.into()).map_err(|_| Error::InvalidState("snapshot jpeg encode failed"))?;
    Ok(jpeg)
}

fn write_rgb24(image: &image::DynamicImage, out: &mut [u8]) -> bool {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let want = width.saturating_mul(height).saturating_mul(3);
    if out.len() < want {
        return false;
    }

    match image {
        image::DynamicImage::ImageRgb8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != want {
                return false;
            }
            out[..want].copy_from_slice(raw);
            true
        }
        image::DynamicImage::ImageRgba8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height).saturating_mul(4) {
                return false;
            }
            for row in 0..height {
                let src_row = row.saturating_mul(width).saturating_mul(4);
                let dst_row = row.saturating_mul(width).saturating_mul(3);
                for col in 0..width {
                    let src = src_row.saturating_add(col.saturating_mul(4));
                    let dst = dst_row.saturating_add(col.saturating_mul(3));
                    if src + 3 < raw.len() && dst + 2 < out.len() {
                        out[dst] = raw[src];
                        out[dst + 1] = raw[src + 1];
                        out[dst + 2] = raw[src + 2];
                    }
                }
            }
            true
        }
        image::DynamicImage::ImageLuma8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height) {
                return false;
            }
            for row in 0..height {
                let src_row = row.saturating_mul(width);
                let dst_row = row.saturating_mul(width).saturating_mul(3);
                for col in 0..width {
                    let src = src_row.saturating_add(col);
                    let dst = dst_row.saturating_add(col.saturating_mul(3));
                    if src < raw.len() && dst + 2 < out.len() {
                        let v = raw[src];
                        out[dst] = v;
                        out[dst + 1] = v;
                        out[dst + 2] = v;
                    }
                }
            }
            true
        }
        _ => false,
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

fn canonicalize_output_for_pipeline(output: Option<String>, pipeline_id: Option<Uuid>) -> Option<String> {
    let normalized = output.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    if pipeline_id == Some(RAW_STREAM_PIPELINE_UUID) {
        return normalized.map(|value| {
            if value.eq_ignore_ascii_case("undistorted") {
                "undistorted".to_string()
            } else {
                // RAW pipeline only supports `raw` and `undistorted`.
                // Coerce legacy/invalid values (e.g. `frame`, `overlay`) to `raw`.
                "raw".to_string()
            }
        });
    }
    normalized
}

fn calibration_mode_output_port(_template_id: &str) -> &'static str {
    "frame"
}

fn calibration_mode_host_buffer(_default_host_buffer: usize) -> usize {
    CALIBRATION_MODE_HOST_BUFFER
}

fn load_calibration_mode_graph_json() -> std::io::Result<serde_json::Value> {
    crate::pipelines::load_template_graph_json(CALIBRATION_TEMPLATE_ID)
}

fn patch_dictionary_const(graph: &mut serde_json::Value, dictionary: &str) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let upsert = |consts: &mut Vec<serde_json::Value>, name: &str, value: &str| {
        for entry in consts.iter_mut() {
            let Some(pair) = entry.as_array_mut() else { continue };
            if pair.len() != 2 {
                continue;
            }
            if pair[0].as_str() == Some(name) {
                pair[1] = serde_json::json!({ "type": "String", "value": value });
                return;
            }
        }
        consts.push(serde_json::json!([name, { "type": "String", "value": value }]));
    };
    let mut patched_nodes = 0usize;
    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if !id.starts_with("cv:aruco:") {
            continue;
        }
        let accepts_dictionary =
            node.get("inputs").and_then(|v| v.as_array()).is_some_and(|inputs| inputs.iter().any(|name| name.as_str().is_some_and(|value| value.eq_ignore_ascii_case("dictionary"))));
        if !accepts_dictionary {
            continue;
        }

        let consts = match node.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
            Some(arr) => arr,
            None => {
                let Some(obj) = node.as_object_mut() else {
                    continue;
                };
                obj.insert("const_inputs".into(), serde_json::Value::Array(Vec::new()));
                match obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) {
                    Some(arr) => arr,
                    None => continue,
                }
            }
        };
        upsert(consts, "dictionary", dictionary);
        patched_nodes += 1;
    }

    if patched_nodes == 0 {
        tracing::warn!("calibration dictionary patch skipped: no aruco nodes with dictionary input found");
    }
}

fn upsert_const_input_bool(node: &mut serde_json::Value, name: &str, value: bool) -> bool {
    let Some(obj) = node.as_object_mut() else {
        return false;
    };
    if !obj.contains_key("const_inputs") {
        obj.insert("const_inputs".into(), serde_json::Value::Array(Vec::new()));
    }
    let Some(consts) = obj.get_mut("const_inputs").and_then(|v| v.as_array_mut()) else {
        return false;
    };
    for entry in consts.iter_mut() {
        let Some(pair) = entry.as_array_mut() else { continue };
        if pair.len() != 2 {
            continue;
        }
        if pair[0].as_str() == Some(name) {
            pair[1] = serde_json::json!({ "type": "Bool", "value": value });
            return true;
        }
    }
    consts.push(serde_json::json!([name, { "type": "Bool", "value": value }]));
    true
}

fn patch_calibration_mode_detection_strictness(graph: &mut serde_json::Value, dictionary: Option<&str>) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let selected_dictionary = dictionary.unwrap_or_default().trim().to_ascii_lowercase();
    let dictionary_max_id = calibration_dictionary_max_id(&selected_dictionary);

    let mut overlay_nodes = 0usize;

    for node in nodes {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if id == "cv:aruco:overlay_detections" {
            // Keep calibration stream clean by default; overlays are for explicit debug only.
            let _ = upsert_const_input_bool(node, "draw_boxes", false);
            let _ = upsert_const_input_bool(node, "draw_corners", false);
            let _ = upsert_const_input_bool(node, "draw_ids", false);
            let _ = upsert_const_input_bool(node, "draw_hud", false);
            overlay_nodes += 1;
        }
    }

    tracing::info!(
        dictionary = selected_dictionary,
        dictionary_max_id = ?dictionary_max_id,
        overlay_nodes,
        "patched calibration mode graph defaults"
    );
}

fn ensure_calibration_mode_frame_output(graph: &mut serde_json::Value) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let mut host_bridge_idx: Option<usize> = None;
    let mut host_output_idx: Option<usize> = None;

    for (idx, node) in nodes.iter_mut().enumerate() {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if id == "io.host_bridge" || id.ends_with(":io.host_bridge") {
            host_bridge_idx = Some(idx);
            let Some(obj) = node.as_object_mut() else {
                continue;
            };
            if !obj.get("metadata").is_some_and(|value| value.is_object()) {
                obj.insert("metadata".into(), serde_json::json!({}));
            }
            let Some(metadata) = obj.get_mut("metadata").and_then(|value| value.as_object_mut()) else {
                continue;
            };
            // `io.host_bridge` can expose multiple outputs (frame + ROI ports). Force the
            // bridge input selection so graph build does not fail with ambiguous input ports.
            metadata.insert("helios.host_input_port".into(), serde_json::json!({ "type": "String", "value": "frame" }));
        } else if id == "io.host_output" || id.ends_with(":io.host_output") {
            host_output_idx = Some(idx);
            let Some(obj) = node.as_object_mut() else {
                continue;
            };
            if !obj.contains_key("inputs") {
                obj.insert("inputs".into(), serde_json::Value::Array(Vec::new()));
            }
            let Some(inputs) = obj.get_mut("inputs").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            let has_frame = inputs.iter().any(|value| value.as_str() == Some("frame"));
            if !has_frame {
                inputs.push(serde_json::Value::String("frame".to_string()));
            }
        }
    }

    let (Some(from_idx), Some(to_idx)) = (host_bridge_idx, host_output_idx) else {
        tracing::warn!("calibration mode frame output patch skipped: missing io.host_bridge or io.host_output");
        return;
    };

    let Some(edges) = graph.get_mut("edges").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let has_frame_edge = edges.iter().any(|edge| {
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        from_node == Some(from_idx as u64) && from_port == Some("frame") && to_node == Some(to_idx as u64) && to_port == Some("frame")
    });
    if !has_frame_edge {
        edges.push(serde_json::json!({
            "from": { "node": from_idx, "port": "frame" },
            "to": { "node": to_idx, "port": "frame" }
        }));
    }
}

fn ensure_calibration_mode_detections_json_output(graph: &mut serde_json::Value) {
    let Some(nodes) = graph.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let mut host_output_idx: Option<usize> = None;
    let mut detections_json_idx: Option<usize> = None;

    for (idx, node) in nodes.iter_mut().enumerate() {
        let Some(id) = node.get("id").and_then(|v| v.as_str()) else {
            continue;
        };

        if id == "io.host_output" {
            host_output_idx = Some(idx);
            let Some(obj) = node.as_object_mut() else {
                continue;
            };
            if !obj.contains_key("inputs") {
                obj.insert("inputs".into(), serde_json::Value::Array(Vec::new()));
            }
            let Some(inputs) = obj.get_mut("inputs").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            if !inputs.iter().any(|value| value.as_str() == Some("detections_json")) {
                inputs.push(serde_json::Value::String("detections_json".to_string()));
            }
        } else if id == "cv:aruco:detections_json" {
            detections_json_idx = Some(idx);
        }
    }

    let Some(host_output_idx) = host_output_idx else {
        tracing::warn!("calibration mode detections_json patch skipped: missing io.host_output");
        return;
    };

    if detections_json_idx.is_none() {
        nodes.push(serde_json::json!({
            "bundle": null,
            "compute": "CpuOnly",
            "const_inputs": [
                ["min_id", { "type": "Int", "value": -1 }],
                ["max_id", { "type": "Int", "value": -1 }]
            ],
            "id": "cv:aruco:detections_json",
            "inputs": ["detections", "min_id", "max_id"],
            "label": "Detections JSON",
            "metadata": {},
            "outputs": ["json"]
        }));
        detections_json_idx = Some(nodes.len() - 1);
    }

    let Some(detections_json_idx) = detections_json_idx else {
        return;
    };

    let Some(edges) = graph.get_mut("edges").and_then(|v| v.as_array_mut()) else {
        return;
    };

    let detections_source = edges.iter().find_map(|edge| {
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64())?;
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str())?;
        if to_node != host_output_idx as u64 || to_port != "detections" {
            return None;
        }
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64())?;
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str())?;
        Some((from_node, from_port.to_string()))
    });

    let Some((source_node, source_port)) = detections_source else {
        tracing::warn!("calibration mode detections_json patch skipped: missing detections source edge");
        return;
    };

    let has_source_edge = edges.iter().any(|edge| {
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        from_node == Some(source_node) && from_port == Some(source_port.as_str()) && to_node == Some(detections_json_idx as u64) && to_port == Some("detections")
    });
    if !has_source_edge {
        edges.push(serde_json::json!({
            "from": { "node": source_node, "port": source_port },
            "to": { "node": detections_json_idx, "port": "detections" }
        }));
    }

    let has_output_edge = edges.iter().any(|edge| {
        let from_node = edge.get("from").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let from_port = edge.get("from").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        let to_node = edge.get("to").and_then(|v| v.get("node")).and_then(|v| v.as_u64());
        let to_port = edge.get("to").and_then(|v| v.get("port")).and_then(|v| v.as_str());
        from_node == Some(detections_json_idx as u64) && from_port == Some("json") && to_node == Some(host_output_idx as u64) && to_port == Some("detections_json")
    });
    if !has_output_edge {
        edges.push(serde_json::json!({
            "from": { "node": detections_json_idx, "port": "json" },
            "to": { "node": host_output_idx, "port": "detections_json" }
        }));
    }
}

fn normalize_calibration_dictionary_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    let mut candidates = vec![normalized.clone()];
    if let Some(stripped) = normalized.strip_prefix("dict_") {
        candidates.push(stripped.to_string());
    }
    if let Some(stripped) = normalized.strip_prefix("dict") {
        if stripped.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            candidates.push(stripped.to_string());
        }
    }

    candidates.into_iter().find(|candidate| lib_cv::modules::aruco::ArucoDictionaryKind::from_str(candidate).is_ok())
}

fn calibration_dictionary_max_id(dictionary: &str) -> Option<i64> {
    let normalized = dictionary.trim().to_ascii_lowercase();
    let (_, count_str) = normalized.split_once('_')?;
    if !count_str.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let count = count_str.parse::<i64>().ok()?;
    (count > 0).then_some(count - 1)
}

fn with_control_assignment(existing: &[ControlAssignment], control_id: ControlId, value: CaptureControlValue) -> Vec<ControlAssignment> {
    let mut controls = existing.to_vec();
    controls.retain(|ctl| ctl.id != control_id);
    if !matches!(value, CaptureControlValue::None) {
        controls.push(ControlAssignment { id: control_id, value });
    }
    controls
}

fn sanitize_file_video_frame_controls(descriptor: &CaptureDescriptor, controls: &mut Vec<ControlAssignment>) {
    let values_by_id = controls.iter().filter_map(|ctl| control_value_to_u32(&ctl.value).map(|value| (ctl.id, value))).collect::<HashMap<_, _>>();

    for pair in collect_file_video_range_control_pairs(descriptor) {
        let has_start = values_by_id.contains_key(&pair.start_id);
        let has_stop = values_by_id.contains_key(&pair.stop_id);
        let mut start = values_by_id.get(&pair.start_id).copied().unwrap_or(0);
        let mut stop = values_by_id.get(&pair.stop_id).copied().unwrap_or(0);

        if let Some(max_stop_frame) = pair.max_stop_frame {
            start = start.min(max_stop_frame);
            if stop > 0 {
                stop = stop.min(max_stop_frame);
            }
        }
        if stop > 0 && stop < start {
            stop = start;
        }
        if has_start && has_stop && stop == start {
            if let Some(max_stop_frame) = pair.max_stop_frame {
                if max_stop_frame > 0 {
                    if stop < max_stop_frame {
                        stop = stop.saturating_add(1);
                    } else if start > 0 {
                        start = start.saturating_sub(1);
                    }
                }
            } else if start > 0 {
                start = start.saturating_sub(1);
            }
        }

        if values_by_id.get(&pair.start_id).is_some_and(|existing| *existing != start) {
            upsert_control_assignment(controls, pair.start_id, CaptureControlValue::Uint(start));
        }
        if values_by_id.get(&pair.stop_id).is_some_and(|existing| *existing != stop) {
            upsert_control_assignment(controls, pair.stop_id, CaptureControlValue::Uint(stop));
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FileVideoRangeControlPair {
    start_id: u32,
    stop_id: u32,
    max_stop_frame: Option<u32>,
}

#[derive(Default)]
struct FileVideoRangeControlIds {
    start_id: Option<u32>,
    stop_id: Option<u32>,
    max_stop_frame: Option<u32>,
}

fn collect_file_video_range_control_pairs(descriptor: &CaptureDescriptor) -> Vec<FileVideoRangeControlPair> {
    let mut pairs_by_token: HashMap<String, FileVideoRangeControlIds> = HashMap::new();
    for control in &descriptor.controls {
        if let Some(token) = control.name.strip_prefix("file.video.").and_then(|name| name.strip_suffix(".start_frame")) {
            pairs_by_token.entry(token.to_string()).or_default().start_id = Some(control.id.0);
            continue;
        }
        if let Some(token) = control.name.strip_prefix("file.video.").and_then(|name| name.strip_suffix(".stop_frame")) {
            let entry = pairs_by_token.entry(token.to_string()).or_default();
            entry.stop_id = Some(control.id.0);
            entry.max_stop_frame = match &control.default {
                CaptureControlValue::Uint(value) if *value > 0 => Some(*value),
                CaptureControlValue::Int(value) if *value > 0 => Some(*value as u32),
                _ => None,
            };
        }
    }

    pairs_by_token.into_values().filter_map(|ids| Some(FileVideoRangeControlPair { start_id: ids.start_id?, stop_id: ids.stop_id?, max_stop_frame: ids.max_stop_frame })).collect()
}

fn control_value_to_u32(value: &CaptureControlValue) -> Option<u32> {
    match value {
        CaptureControlValue::Uint(value) => Some(*value),
        CaptureControlValue::Int(value) if *value >= 0 => Some(*value as u32),
        _ => None,
    }
}

fn upsert_control_assignment(controls: &mut Vec<ControlAssignment>, id: u32, value: CaptureControlValue) {
    if let Some(control) = controls.iter_mut().find(|control| control.id == id) {
        control.value = value;
    } else {
        controls.push(ControlAssignment { id, value });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{CURRENT_STREAM_CONFIG_SCHEMA_VERSION, RequestedDecoderConfig, RequestedEncoderConfig, StreamManifest};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use styx::core::controls::{Access, ControlId as StyxControlId, ControlKind, ControlMeta, ControlMetadata, ControlValue};

    fn sample_manifest_for_encoder_defaults(width: u32, height: u32) -> ResolvedStreamConfig {
        let identity = crate::identity::DeviceIdentity { id: None, alias: None, hardware_id: None };
        let format = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(width, height).expect("resolution"), ColorSpace::Srgb);
        let capture = crate::capture::CaptureConfig {
            device_keys: vec![],
            backend: crate::capture::BackendKind::Virtual,
            handle: crate::capture::BackendHandle::Virtual,
            mode: crate::capture::ModeId { format, interval: None },
            target_fps: None,
            interval: None,
            controls: vec![],
            enable_tdn_output: false,
        };
        StreamManifest {
            schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
            identity,
            capture,
            host_buffer: crate::ipc::default_host_buffer(),
            internal: false,
            pipeline_enabled: false,
            pipelines: Vec::new(),
            active_pipeline_id: None,
            active_pipeline_output: None,
            pipeline_layout: None,
            pipeline_wires: Vec::new(),
            pipeline_host_inputs: std::collections::BTreeMap::new(),
            calibration: None,
            pose: None,
            encoder: RequestedEncoderConfig::enabled(Some("h264".to_string()), None),
            decoder: RequestedDecoderConfig::default(),
            preview_jpeg_quality: 30,
            recording_mode: crate::ipc::default_recording_mode(),
            start_on_boot: false,
        }
        .resolve()
    }

    fn file_video_descriptor(start_id: u32, stop_id: u32, default_stop: u32) -> CaptureDescriptor {
        CaptureDescriptor {
            modes: Vec::new(),
            controls: vec![
                ControlMeta {
                    id: StyxControlId(start_id),
                    name: "file.video.sample.start_frame".to_string(),
                    kind: ControlKind::Uint,
                    access: Access::ReadWrite,
                    min: ControlValue::Uint(0),
                    max: ControlValue::Uint(u32::MAX),
                    default: ControlValue::Uint(0),
                    step: Some(ControlValue::Uint(1)),
                    menu: None,
                    metadata: ControlMetadata::default(),
                },
                ControlMeta {
                    id: StyxControlId(stop_id),
                    name: "file.video.sample.stop_frame".to_string(),
                    kind: ControlKind::Uint,
                    access: Access::ReadWrite,
                    min: ControlValue::Uint(0),
                    max: ControlValue::Uint(u32::MAX),
                    default: ControlValue::Uint(default_stop),
                    step: Some(ControlValue::Uint(1)),
                    menu: None,
                    metadata: ControlMetadata::default(),
                },
            ],
        }
    }

    #[test]
    fn calibration_mode_output_port_uses_frame_for_calibration() {
        assert_eq!(calibration_mode_output_port(CALIBRATION_TEMPLATE_ID), "frame");
        assert_eq!(calibration_mode_output_port(UNDISTORT_TEMPLATE_ID), "frame");
    }

    #[test]
    fn canonicalize_output_for_raw_pipeline_coerces_invalid_ports() {
        assert_eq!(canonicalize_output_for_pipeline(Some("frame".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).as_deref(), Some("raw"));
        assert_eq!(canonicalize_output_for_pipeline(Some("raw".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).as_deref(), Some("raw"));
        assert_eq!(canonicalize_output_for_pipeline(Some("undistorted".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).as_deref(), Some("undistorted"));
        assert_eq!(canonicalize_output_for_pipeline(Some("overlay".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).as_deref(), Some("raw"));
    }

    #[test]
    fn calibration_mode_host_buffer_forces_low_latency() {
        assert_eq!(calibration_mode_host_buffer(8), CALIBRATION_MODE_HOST_BUFFER);
        assert_eq!(calibration_mode_host_buffer(0), CALIBRATION_MODE_HOST_BUFFER);
    }

    #[test]
    fn normalize_calibration_dictionary_name_accepts_dict_aliases() {
        assert_eq!(normalize_calibration_dictionary_name("4x4_1000").as_deref(), Some("4x4_1000"));
        assert_eq!(normalize_calibration_dictionary_name("dict4x4_1000").as_deref(), Some("4x4_1000"));
        assert_eq!(normalize_calibration_dictionary_name("DICT_4X4_1000").as_deref(), Some("4x4_1000"));
        assert_eq!(normalize_calibration_dictionary_name("apriltag_36h11").as_deref(), Some("apriltag_36h11"));
        assert_eq!(normalize_calibration_dictionary_name("bogus"), None);
    }

    #[test]
    fn calibration_dictionary_max_id_parses_aruco_sizes() {
        assert_eq!(calibration_dictionary_max_id("4x4_50"), Some(49));
        assert_eq!(calibration_dictionary_max_id("4x4_1000"), Some(999));
        assert_eq!(calibration_dictionary_max_id("apriltag_36h11"), None);
        assert_eq!(calibration_dictionary_max_id(""), None);
    }

    #[test]
    fn default_encoder_settings_use_480p_for_1080p_capture() {
        let manifest = sample_manifest_for_encoder_defaults(1920, 1080);
        let output = manifest
            .encoder
            .settings
            .as_ref()
            .and_then(|settings| settings.output_resolution())
            .expect("output resolution");
        assert_eq!(output.width, 854);
        assert_eq!(output.height, 480);
    }

    #[test]
    fn default_encoder_settings_preserve_aspect_for_16_by_10_capture() {
        let manifest = sample_manifest_for_encoder_defaults(1280, 800);
        let output = manifest
            .encoder
            .settings
            .as_ref()
            .and_then(|settings| settings.output_resolution())
            .expect("output resolution");
        assert_eq!(output.width, 768);
        assert_eq!(output.height, 480);
    }

    #[test]
    fn default_encoder_settings_set_framerate_and_preview_quality() {
        let manifest = sample_manifest_for_encoder_defaults(1920, 1080);
        let settings = manifest.encoder.settings.expect("encoder settings");
        let framerate = settings.framerate().expect("framerate");
        assert_eq!(framerate.numerator, 60);
        assert_eq!(framerate.denominator, 1);
        assert_eq!(manifest.preview_jpeg_quality, 30);
    }

    #[test]
    fn default_encoder_settings_do_not_override_explicit_values() {
        let manifest = StreamManifest {
            encoder: RequestedEncoderConfig::enabled(
                None,
                Some(crate::ipc::EncoderSettings::H264 {
                    bitrate: None,
                    gop: None,
                    framerate: Some(crate::ipc::FrameRate { numerator: 24, denominator: 1 }),
                    thread_count: None,
                    output_resolution: Some(crate::ipc::ResolutionHint { width: 1280, height: 720 }),
                }),
            ),
            preview_jpeg_quality: 80,
            ..sample_manifest_for_encoder_defaults(1920, 1080).to_requested_manifest()
        }
        .resolve();
        let settings = manifest.encoder.settings.expect("encoder settings");
        let output = settings.output_resolution().expect("output resolution");
        assert_eq!(output.width, 1280);
        assert_eq!(output.height, 720);
        let framerate = settings.framerate().expect("framerate");
        assert_eq!(framerate.numerator, 24);
        assert_eq!(framerate.denominator, 1);
        assert_eq!(manifest.preview_jpeg_quality, 80);
    }

    #[test]
    fn patch_dictionary_const_updates_aruco_dictionary_inputs() {
        let mut graph = json!({
            "nodes": [
                { "id": "cv:aruco:decode_quads_hamming", "inputs": ["frame", "quads", "dictionary"] },
                { "id": "cv:aruco:overlay_detections", "inputs": ["frame", "detections"] },
                { "id": "cv:other:example", "inputs": ["dictionary"] }
            ]
        });
        patch_dictionary_const(&mut graph, "4x4_1000");

        let nodes = graph.get("nodes").and_then(|v| v.as_array()).expect("nodes");
        let decode_consts = nodes[0].get("const_inputs").and_then(|v| v.as_array()).expect("decode consts");
        assert!(decode_consts.iter().any(|entry| {
            let pair = entry.as_array().expect("const pair");
            pair[0].as_str() == Some("dictionary") && pair[1].get("value").and_then(|v| v.as_str()) == Some("4x4_1000")
        }));
        assert!(nodes[1].get("const_inputs").is_none());
        assert!(nodes[2].get("const_inputs").is_none());
    }

    #[test]
    fn ensure_calibration_mode_frame_output_sets_host_input_port_metadata() {
        let mut graph = json!({
            "nodes": [
                {
                    "id": "io.host_bridge",
                    "outputs": ["frame", "roi_x", "roi_y"],
                    "metadata": { "host_bridge": { "type": "Bool", "value": true } }
                },
                {
                    "id": "io.host_output",
                    "inputs": ["detections"],
                    "outputs": []
                }
            ],
            "edges": []
        });

        ensure_calibration_mode_frame_output(&mut graph);

        let nodes = graph.get("nodes").and_then(|value| value.as_array()).expect("nodes");
        let bridge_metadata = nodes[0].get("metadata").and_then(|value| value.as_object()).expect("bridge metadata");
        assert_eq!(bridge_metadata.get("helios.host_input_port").and_then(|value| value.get("value")).and_then(|value| value.as_str()), Some("frame"));

        let host_output_inputs = nodes[1].get("inputs").and_then(|value| value.as_array()).expect("host output inputs");
        assert!(host_output_inputs.iter().any(|value| value.as_str() == Some("frame")));
    }

    #[test]
    fn sanitize_file_video_controls_clamps_invalid_stop_to_start() {
        let descriptor = file_video_descriptor(10, 11, 800);
        let mut controls = vec![ControlAssignment { id: 10, value: CaptureControlValue::Uint(200) }, ControlAssignment { id: 11, value: CaptureControlValue::Uint(120) }];

        sanitize_file_video_frame_controls(&descriptor, &mut controls);

        let start = controls.iter().find(|ctl| ctl.id == 10).and_then(|ctl| control_value_to_u32(&ctl.value));
        let stop = controls.iter().find(|ctl| ctl.id == 11).and_then(|ctl| control_value_to_u32(&ctl.value));
        assert_eq!(start, Some(200));
        assert_eq!(stop, Some(200));
    }

    #[test]
    fn sanitize_file_video_controls_clamps_to_known_frame_count() {
        let descriptor = file_video_descriptor(10, 11, 500);
        let mut controls = vec![ControlAssignment { id: 10, value: CaptureControlValue::Uint(800) }, ControlAssignment { id: 11, value: CaptureControlValue::Uint(700) }];

        sanitize_file_video_frame_controls(&descriptor, &mut controls);

        let start = controls.iter().find(|ctl| ctl.id == 10).and_then(|ctl| control_value_to_u32(&ctl.value));
        let stop = controls.iter().find(|ctl| ctl.id == 11).and_then(|ctl| control_value_to_u32(&ctl.value));
        assert_eq!(start, Some(500));
        assert_eq!(stop, Some(500));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn abort_unregistered_stream_worker_stops_and_joins_thread() {
        let (command_tx, command_rx) = sync_channel::<StreamCommand>(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_flag = Arc::clone(&stopped);
        let join = std::thread::spawn(move || {
            match command_rx.recv().expect("stop command") {
                StreamCommand::Stop { respond_to } => {
                    let _ = respond_to.send(());
                }
                _ => panic!("expected stop command"),
            }
            stopped_flag.store(true, Ordering::SeqCst);
        });

        abort_unregistered_stream_worker(Uuid::nil(), command_tx, join).await;

        assert!(stopped.load(Ordering::SeqCst));
    }
}
