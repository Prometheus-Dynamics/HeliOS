use std::{
    fs,
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use nix::unistd::dup;
use orion::transport::ipc::{DEFAULT_UNIX_FD_FRAME_MAX_FDS, DEFAULT_UNIX_FD_FRAME_MAX_PAYLOAD_BYTES, UnixFdFrame, send_unix_fd_frame_async};
use serde::{Deserialize, Serialize};
use styx::codec::prelude::FfmpegEncoderOptions;
use styx::codec::{Codec, CodecRegistry, CodecRegistryHandle};
use styx::core::buffer::{FrameBackingExport, FrameLease, FrameLeaseDescriptor};
use styx::core::prelude::{FourCc, Resolution};
use styx::prelude::{FfmpegMjpegEncoder, TurbojpegEncoder};
use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    sync::watch,
    task::JoinHandle,
};

use crate::model::{ResourceDescriptor, ResourceId};

const FRAME_LEASE_STREAM_MAGIC: &[u8; 8] = b"HFRM0003";
const HEARTBEAT_MAGIC: &[u8; 8] = b"HHBT0001";

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("failed to create stream directory '{path}': {error}")]
    CreateDirectory { path: PathBuf, error: std::io::Error },
    #[error("failed to remove stale stream socket '{path}': {error}")]
    RemoveSocket { path: PathBuf, error: std::io::Error },
    #[error("failed to bind stream socket '{path}': {error}")]
    BindSocket { path: PathBuf, error: std::io::Error },
    #[error("failed to write stream frame '{path}': {error}")]
    WriteFrame { path: PathBuf, error: std::io::Error },
    #[error("failed to encode stream frame: {0}")]
    Encode(String),
    #[error("failed to export frame backing: {0}")]
    Export(String),
    #[error("failed to duplicate frame fd: {0}")]
    DuplicateFd(std::io::Error),
    #[error("failed to initialize preview encoder: {0}")]
    PreviewEncoder(String),
    #[error("failed to encode preview frame: {0}")]
    PreviewEncode(String),
    #[error("frame payload is not exportable on this platform")]
    UnsupportedPlatform,
}

#[derive(Debug, Serialize)]
struct CaptureHeartbeat<'a> {
    channel: &'a str,
    node_id: &'a str,
    resource_id: &'a str,
    sequence: u64,
    produced_at_ms: u64,
    #[serde(with = "serde_bytes")]
    payload: &'a [u8],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CaptureFrameLeaseMetadata {
    channel: String,
    node_id: String,
    resource_id: String,
    sequence: u64,
    produced_at_ms: u64,
    socket_path: String,
    preview_socket_path: String,
    preview_mime: &'static str,
    transport: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameLeaseTransportMessage {
    pub descriptor: FrameLeaseDescriptor,
    pub backing: FrameLeaseTransportBacking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrameLeaseTransportBacking {
    Memfd { len: usize },
    DmabufPlanes { planes: Vec<FrameLeaseTransportPlane> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameLeaseTransportPlane {
    pub offset: usize,
    pub len: usize,
}

pub struct PeripheralStreamWriter {
    path: PathBuf,
    socket_path: PathBuf,
    preview_socket_path: PathBuf,
    channel_id: String,
    node_id: String,
    latest: Arc<Mutex<Option<LatestFrameLease>>>,
    latest_preview: watch::Sender<Option<Arc<LatestMjpegFrame>>>,
    preview_registry: CodecRegistryHandle,
    preview_encoder: Option<PreviewEncoderState>,
    metrics: PublishMetrics,
    _server: JoinHandle<()>,
    _preview_server: JoinHandle<()>,
    sequence: u64,
}

#[derive(Debug)]
struct LatestFrameLease {
    descriptor: FrameLeaseDescriptor,
    backing: LatestBacking,
}

#[derive(Debug)]
enum LatestBacking {
    Memfd { fd: OwnedFd, len: usize },
    DmabufPlanes { planes: Vec<LatestFdPlane> },
}

#[derive(Debug)]
struct LatestFdPlane {
    fd: OwnedFd,
    offset: usize,
    len: usize,
}

#[derive(Debug)]
struct LatestMjpegFrame {
    sequence: u64,
    bytes: Arc<Vec<u8>>,
}

struct PreviewEncoderState {
    backend: PreviewEncoderBackend,
    source_input: FourCc,
    input: FourCc,
    codec: Arc<dyn Codec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewEncoderBackend {
    Ffmpeg,
    Turbojpeg,
}

impl PreviewEncoderBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Turbojpeg => "turbojpeg",
        }
    }
}

#[derive(Debug, Default)]
struct PublishMetrics {
    frame_count: u64,
    last_frame_started_at: Option<Instant>,
    interframe_total: Duration,
    export_total: Duration,
    import_total: Duration,
    decode_total: Duration,
    encode_total: Duration,
    jpeg_copy_total: Duration,
    metadata_write_total: Duration,
    total_publish_total: Duration,
}

impl PeripheralStreamWriter {
    pub fn create_capture_channel(path: &Path, node_id: &str, resource: &ResourceDescriptor) -> Result<Self, StreamError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| StreamError::CreateDirectory { path: parent.to_path_buf(), error })?;
        }
        let socket_path = stream_socket_path(path);
        let preview_socket_path = stream_mjpeg_socket_path(path);
        if socket_path.exists() {
            fs::remove_file(&socket_path).map_err(|error| StreamError::RemoveSocket { path: socket_path.clone(), error })?;
        }
        if preview_socket_path.exists() {
            fs::remove_file(&preview_socket_path).map_err(|error| StreamError::RemoveSocket { path: preview_socket_path.clone(), error })?;
        }
        let listener = UnixListener::bind(&socket_path).map_err(|error| StreamError::BindSocket { path: socket_path.clone(), error })?;
        let preview_listener = UnixListener::bind(&preview_socket_path).map_err(|error| StreamError::BindSocket { path: preview_socket_path.clone(), error })?;
        let latest = Arc::new(Mutex::new(None));
        let latest_for_server = latest.clone();
        let socket_for_server = socket_path.clone();
        let server = tokio::spawn(async move {
            run_frame_lease_server(listener, latest_for_server, socket_for_server).await;
        });
        let (latest_preview, preview_rx) = watch::channel(None::<Arc<LatestMjpegFrame>>);
        let preview_socket_for_server = preview_socket_path.clone();
        let preview_server = tokio::spawn(async move {
            run_mjpeg_server(preview_listener, preview_rx, preview_socket_for_server).await;
        });
        let preview_registry = CodecRegistry::with_enabled_codecs().map_err(|error| StreamError::PreviewEncoder(error.to_string()))?.handle();
        Ok(Self {
            path: path.to_path_buf(),
            socket_path,
            preview_socket_path,
            channel_id: format!("stream.channel.{}.raw", resource.id.as_str()),
            node_id: node_id.to_string(),
            latest,
            latest_preview,
            preview_registry,
            preview_encoder: None,
            metrics: PublishMetrics::default(),
            _server: server,
            _preview_server: preview_server,
            sequence: 0,
        })
    }

    pub fn publish_capture_heartbeat(&mut self, resource: &ResourceDescriptor, sequence: u64, produced_at_ms: u64) -> Result<(), StreamError> {
        let frame = CaptureHeartbeat { channel: &self.channel_id, node_id: &self.node_id, resource_id: resource.id.as_str(), sequence, produced_at_ms, payload: &[] };
        let bytes = encode_capture_heartbeat(&frame);
        write_frame_atomic(&self.path, &bytes)?;
        Ok(())
    }

    pub fn publish_capture_frame(&mut self, frame: &FrameLease) -> Result<(), StreamError> {
        let publish_started_at = Instant::now();
        let export_started_at = Instant::now();
        self.sequence = self.sequence.wrapping_add(1);
        let exported = export_latest_frame(frame)?;
        let export_elapsed = export_started_at.elapsed();
        let preview_result = if preview_encoding_enabled() {
            self.ensure_preview_encoder(frame.meta().format.code)?;
            Some(encode_preview_frame(&self.preview_registry, self.preview_encoder.as_ref().expect("preview encoder initialized"), &exported, self.sequence)?)
        } else {
            None
        };
        let produced_at_ms = now_ms();
        {
            let mut latest = self.latest.lock().expect("frame lease mutex");
            *latest = Some(exported);
        }
        if let Some((preview, _)) = preview_result.as_ref() {
            let _ = self.latest_preview.send(Some(preview.clone()));
        }
        let metadata = CaptureFrameLeaseMetadata {
            channel: self.channel_id.clone(),
            node_id: self.node_id.clone(),
            resource_id: self.channel_id.clone(),
            sequence: self.sequence,
            produced_at_ms,
            socket_path: self.socket_path.display().to_string(),
            preview_socket_path: self.preview_socket_path.display().to_string(),
            preview_mime: "image/jpeg",
            transport: "styx-frame-lease-v1",
        };
        let metadata_write_started_at = Instant::now();
        let bytes = encode_frame_lease_metadata(&metadata)?;
        write_frame_atomic(&self.path, &bytes)?;
        let metadata_write_elapsed = metadata_write_started_at.elapsed();
        self.record_publish_metrics(publish_started_at, export_elapsed, preview_result.map_or_else(PreviewEncodeMetrics::default, |(_, metrics)| metrics), metadata_write_elapsed);
        Ok(())
    }

    pub fn channel_resource_id(resource_id: &ResourceId) -> String {
        format!("stream.channel.{}.raw", resource_id.as_str())
    }
}

fn preview_encoding_enabled() -> bool {
    std::env::var("HELIOS_CAPTURE_PREVIEW_ENCODE").map(|value| !matches!(value.as_str(), "0" | "false" | "FALSE" | "off" | "OFF")).unwrap_or(true)
}

impl PeripheralStreamWriter {
    fn ensure_preview_encoder(&mut self, input: FourCc) -> Result<(), StreamError> {
        let backend = preview_encoder_backend();
        if self.preview_encoder.as_ref().is_some_and(|state| state.source_input == input && state.backend == backend) {
            return Ok(());
        }
        let (encoder_input, codec): (FourCc, Arc<dyn Codec>) = match backend {
            PreviewEncoderBackend::Ffmpeg => match input {
                code if code == FourCc::new(*b"NV12") => {
                    (input, Arc::new(FfmpegMjpegEncoder::with_options_for_input(input, preview_encoder_options()).map_err(|error| StreamError::PreviewEncoder(error.to_string()))?))
                }
                _ => (FourCc::RG24, Arc::new(FfmpegMjpegEncoder::with_options_for_input(FourCc::RG24, preview_encoder_options()).map_err(|error| StreamError::PreviewEncoder(error.to_string()))?)),
            },
            PreviewEncoderBackend::Turbojpeg => (FourCc::RG24, Arc::new(TurbojpegEncoder::new(FourCc::RG24, preview_jpeg_quality()))),
        };
        tracing::warn!(
            channel = %self.channel_id,
            source_input = %input,
            encoder_input = %encoder_input,
            backend = backend.as_str(),
            quality = preview_jpeg_quality(),
            threads = preview_encoder_options().thread_count,
            output_resolution = ?preview_encoder_options().output_resolution,
            "capture preview encoder configured"
        );
        self.preview_encoder = Some(PreviewEncoderState { backend, source_input: input, input: encoder_input, codec });
        Ok(())
    }

    fn record_publish_metrics(&mut self, publish_started_at: Instant, export_elapsed: Duration, preview_metrics: PreviewEncodeMetrics, metadata_write_elapsed: Duration) {
        let interframe = self.metrics.last_frame_started_at.map(|last| publish_started_at.saturating_duration_since(last)).unwrap_or_default();
        self.metrics.last_frame_started_at = Some(publish_started_at);
        self.metrics.frame_count = self.metrics.frame_count.saturating_add(1);
        self.metrics.interframe_total += interframe;
        self.metrics.export_total += export_elapsed;
        self.metrics.import_total += preview_metrics.import_elapsed;
        self.metrics.decode_total += preview_metrics.decode_elapsed;
        self.metrics.encode_total += preview_metrics.encode_elapsed;
        self.metrics.jpeg_copy_total += preview_metrics.copy_elapsed;
        self.metrics.metadata_write_total += metadata_write_elapsed;
        self.metrics.total_publish_total += publish_started_at.elapsed();

        if self.metrics.frame_count % 30 == 0 {
            let frames = self.metrics.frame_count as f64;
            tracing::warn!(
                channel = %self.channel_id,
                frames = self.metrics.frame_count,
                interframe_ms = duration_ms(self.metrics.interframe_total, frames),
                export_ms = duration_ms(self.metrics.export_total, frames),
                import_ms = duration_ms(self.metrics.import_total, frames),
                decode_ms = duration_ms(self.metrics.decode_total, frames),
                encode_ms = duration_ms(self.metrics.encode_total, frames),
                jpeg_copy_ms = duration_ms(self.metrics.jpeg_copy_total, frames),
                metadata_write_ms = duration_ms(self.metrics.metadata_write_total, frames),
                total_publish_ms = duration_ms(self.metrics.total_publish_total, frames),
                "capture preview timing"
            );
        }
    }
}

fn preview_encoder_options() -> FfmpegEncoderOptions {
    FfmpegEncoderOptions { thread_count: Some(preview_encoder_threads()), output_resolution: preview_output_resolution(), ..FfmpegEncoderOptions::default() }
}

fn preview_encoder_backend() -> PreviewEncoderBackend {
    match std::env::var("HELIOS_CAPTURE_PREVIEW_ENCODER").ok().as_deref().map(str::trim) {
        Some("turbojpeg" | "turbo-jpeg" | "tj" | "TurboJPEG" | "TURBOJPEG") => PreviewEncoderBackend::Turbojpeg,
        _ => PreviewEncoderBackend::Ffmpeg,
    }
}

fn preview_jpeg_quality() -> i32 {
    std::env::var("HELIOS_CAPTURE_PREVIEW_JPEG_QUALITY").ok().and_then(|value| value.parse::<i32>().ok()).map(|value| value.clamp(1, 100)).unwrap_or(85)
}

fn preview_encoder_threads() -> usize {
    std::env::var("HELIOS_CAPTURE_PREVIEW_ENCODER_THREADS").ok().and_then(|value| value.parse::<usize>().ok()).filter(|threads| *threads > 0).unwrap_or(1)
}

fn preview_output_resolution() -> Option<Resolution> {
    let width = std::env::var("HELIOS_CAPTURE_PREVIEW_OUTPUT_WIDTH").ok().and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0)?;
    let height = std::env::var("HELIOS_CAPTURE_PREVIEW_OUTPUT_HEIGHT").ok().and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0)?;
    Resolution::new(width, height)
}

async fn run_frame_lease_server(listener: UnixListener, latest: Arc<Mutex<Option<LatestFrameLease>>>, socket_path: PathBuf) {
    loop {
        let Ok((stream, _addr)) = listener.accept().await else {
            break;
        };
        let message = {
            let latest = latest.lock().expect("frame lease mutex");
            latest.as_ref().and_then(|frame| prepare_transport_message(frame).ok())
        };
        let Some(frame) = message else {
            continue;
        };
        if let Err(error) = send_unix_fd_frame_async(&stream, &frame, DEFAULT_UNIX_FD_FRAME_MAX_PAYLOAD_BYTES, DEFAULT_UNIX_FD_FRAME_MAX_FDS).await {
            tracing::warn!(socket = %socket_path.display(), error = %error, "failed to send frame lease transport message");
        }
    }
}

async fn run_mjpeg_server(listener: UnixListener, latest: watch::Receiver<Option<Arc<LatestMjpegFrame>>>, socket_path: PathBuf) {
    loop {
        let Ok((stream, _addr)) = listener.accept().await else {
            break;
        };
        let rx = latest.clone();
        let socket_for_task = socket_path.clone();
        tokio::spawn(async move {
            if let Err(error) = stream_mjpeg_client(stream, rx).await {
                tracing::warn!(socket = %socket_for_task.display(), error = %error, "failed to stream mjpeg preview");
            }
        });
    }
}

async fn stream_mjpeg_client(mut stream: UnixStream, mut latest: watch::Receiver<Option<Arc<LatestMjpegFrame>>>) -> Result<(), std::io::Error> {
    let mut last_sequence = None;
    loop {
        let current = latest.borrow().clone();
        if let Some(frame) = current
            && last_sequence != Some(frame.sequence)
        {
            write_mjpeg_chunk(&mut stream, frame.bytes.as_slice()).await?;
            last_sequence = Some(frame.sequence);
            continue;
        }
        if latest.changed().await.is_err() {
            return Ok(());
        }
    }
}

async fn write_mjpeg_chunk(stream: &mut UnixStream, jpeg: &[u8]) -> Result<(), std::io::Error> {
    stream.write_all(format!("--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", jpeg.len()).as_bytes()).await?;
    stream.write_all(jpeg).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await
}

fn export_latest_frame(frame: &FrameLease) -> Result<LatestFrameLease, StreamError> {
    #[cfg(target_os = "linux")]
    {
        let (descriptor, backing) = frame.export_or_copy_memfd().map_err(|error| StreamError::Export(error.to_string()))?;
        let backing = match backing {
            FrameBackingExport::Memfd { fd, len } => LatestBacking::Memfd { fd, len },
            FrameBackingExport::DmabufPlanes { planes } => {
                LatestBacking::DmabufPlanes { planes: planes.into_iter().map(|plane| LatestFdPlane { fd: plane.fd, offset: plane.offset, len: plane.len }).collect() }
            }
        };
        return Ok(LatestFrameLease { descriptor, backing });
    }
    #[allow(unreachable_code)]
    Err(StreamError::UnsupportedPlatform)
}

fn encode_preview_frame(registry: &CodecRegistryHandle, encoder: &PreviewEncoderState, frame: &LatestFrameLease, sequence: u64) -> Result<(Arc<LatestMjpegFrame>, PreviewEncodeMetrics), StreamError> {
    let import_started_at = Instant::now();
    let imported = import_latest_frame(frame)?;
    let import_elapsed = import_started_at.elapsed();
    let decode_started_at = Instant::now();
    let decoded = if imported.meta().format.code == encoder.input {
        imported
    } else if imported.meta().format.code == FourCc::NV12 && encoder.input == FourCc::RG24 {
        registry.process_preferred(FourCc::NV12, &["nv12-cpu"], false, imported).map_err(|error| StreamError::PreviewEncode(error.to_string()))?
    } else {
        registry.process(imported.meta().format.code, imported).map_err(|error| StreamError::PreviewEncode(error.to_string()))?
    };
    let decode_elapsed = decode_started_at.elapsed();
    let encode_started_at = Instant::now();
    let encoded = encoder.codec.process(decoded).map_err(|error| StreamError::PreviewEncode(error.to_string()))?;
    let encode_elapsed = encode_started_at.elapsed();
    let planes = encoded.planes();
    let plane = planes.first().ok_or_else(|| StreamError::PreviewEncode("encoded mjpeg frame missing plane".into()))?;
    let copy_started_at = Instant::now();
    let bytes = Arc::new(plane.data().to_vec());
    let copy_elapsed = copy_started_at.elapsed();
    Ok((Arc::new(LatestMjpegFrame { sequence, bytes }), PreviewEncodeMetrics { import_elapsed, decode_elapsed, encode_elapsed, copy_elapsed }))
}

#[derive(Debug, Default, Clone, Copy)]
struct PreviewEncodeMetrics {
    import_elapsed: Duration,
    decode_elapsed: Duration,
    encode_elapsed: Duration,
    copy_elapsed: Duration,
}

fn import_latest_frame(frame: &LatestFrameLease) -> Result<FrameLease, StreamError> {
    match &frame.backing {
        LatestBacking::Memfd { fd, .. } => {
            let duplicated = dup(fd).map_err(|error| StreamError::DuplicateFd(std::io::Error::from_raw_os_error(error as i32)))?;
            FrameLease::from_memfd_import(frame.descriptor.clone(), duplicated).map_err(|error| StreamError::PreviewEncode(error.to_string()))
        }
        LatestBacking::DmabufPlanes { planes } => {
            let mut duplicated = Vec::with_capacity(planes.len());
            for plane in planes {
                duplicated.push(styx::core::buffer::FrameFdPlane {
                    fd: dup(&plane.fd).map_err(|error| StreamError::DuplicateFd(std::io::Error::from_raw_os_error(error as i32)))?,
                    offset: plane.offset,
                    len: plane.len,
                });
            }
            FrameLease::from_dmabuf_import(frame.descriptor.clone(), duplicated).map_err(|error| StreamError::PreviewEncode(error.to_string()))
        }
    }
}

fn prepare_transport_message(frame: &LatestFrameLease) -> Result<UnixFdFrame, StreamError> {
    match &frame.backing {
        LatestBacking::Memfd { fd, len } => {
            let duplicated = dup(fd).map_err(|error| StreamError::DuplicateFd(std::io::Error::from_raw_os_error(error as i32)))?;
            let payload = serde_json::to_vec(&FrameLeaseTransportMessage { descriptor: frame.descriptor.clone(), backing: FrameLeaseTransportBacking::Memfd { len: *len } })
                .map_err(|error| StreamError::Encode(error.to_string()))?;
            Ok(UnixFdFrame::new(payload, vec![duplicated]))
        }
        LatestBacking::DmabufPlanes { planes } => {
            let payload = serde_json::to_vec(&FrameLeaseTransportMessage {
                descriptor: frame.descriptor.clone(),
                backing: FrameLeaseTransportBacking::DmabufPlanes { planes: planes.iter().map(|plane| FrameLeaseTransportPlane { offset: plane.offset, len: plane.len }).collect() },
            })
            .map_err(|error| StreamError::Encode(error.to_string()))?;
            let mut duplicated = Vec::with_capacity(planes.len());
            for plane in planes {
                duplicated.push(dup(&plane.fd).map_err(|error| StreamError::DuplicateFd(std::io::Error::from_raw_os_error(error as i32)))?);
            }
            Ok(UnixFdFrame::new(payload, duplicated))
        }
    }
}

fn encode_frame_lease_metadata(metadata: &CaptureFrameLeaseMetadata) -> Result<Vec<u8>, StreamError> {
    let json = serde_json::to_vec(metadata).map_err(|error| StreamError::Encode(error.to_string()))?;
    let mut encoded = Vec::with_capacity(FRAME_LEASE_STREAM_MAGIC.len() + json.len());
    encoded.extend_from_slice(FRAME_LEASE_STREAM_MAGIC);
    encoded.extend_from_slice(&json);
    Ok(encoded)
}

fn encode_capture_heartbeat(frame: &CaptureHeartbeat<'_>) -> Vec<u8> {
    let payload = if frame.payload.is_empty() {
        String::from("[]")
    } else {
        let mut out = String::from("[");
        for (index, byte) in frame.payload.iter().enumerate() {
            if index != 0 {
                out.push(',');
            }
            out.push_str(&byte.to_string());
        }
        out.push(']');
        out
    };
    let json = format!(
        "{{\"channel\":\"{}\",\"node_id\":\"{}\",\"resource_id\":\"{}\",\"sequence\":{},\"produced_at_ms\":{},\"payload\":{}}}",
        json_escape(frame.channel),
        json_escape(frame.node_id),
        json_escape(frame.resource_id),
        frame.sequence,
        frame.produced_at_ms,
        payload
    );
    let mut encoded = Vec::with_capacity(8 + json.len());
    encoded.extend_from_slice(HEARTBEAT_MAGIC);
    encoded.extend_from_slice(json.as_bytes());
    encoded
}

fn write_frame_atomic(path: &Path, bytes: &[u8]) -> Result<(), StreamError> {
    let mut temp_path = path.to_path_buf();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or_default();
    let file_name = format!(".{}.tmp-{nonce}", path.file_name().and_then(|name| name.to_str()).unwrap_or("stream"));
    temp_path.set_file_name(file_name);

    fs::write(&temp_path, bytes).map_err(|error| StreamError::WriteFrame { path: temp_path.clone(), error })?;
    fs::rename(&temp_path, path).map_err(|error| StreamError::WriteFrame { path: path.to_path_buf(), error })?;
    Ok(())
}

pub fn stream_socket_path(path: &Path) -> PathBuf {
    let mut socket_path = path.to_path_buf();
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("stream");
    socket_path.set_file_name(format!("{file_name}.sock"));
    socket_path
}

pub fn stream_mjpeg_socket_path(path: &Path) -> PathBuf {
    let mut socket_path = path.to_path_buf();
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("stream");
    socket_path.set_file_name(format!("{file_name}.mjpeg.sock"));
    socket_path
}

fn json_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => escaped.push_str(&format!("\\u{:04x}", control as u32)),
            other => escaped.push(other),
        }
    }
    escaped
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn duration_ms(duration: Duration, divisor: f64) -> f64 {
    ((duration.as_secs_f64() * 1000.0) / divisor * 1000.0).round() / 1000.0
}
