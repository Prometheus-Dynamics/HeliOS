use std::{
    fs,
    os::fd::OwnedFd,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    thread,
};

use daedalus::transport::{Payload, Residency, TypeKey};
use orion::transport::ipc::{DEFAULT_UNIX_FD_FRAME_MAX_FDS, DEFAULT_UNIX_FD_FRAME_MAX_PAYLOAD_BYTES, UnixFdFrame, recv_unix_fd_frame, send_unix_fd_frame};
use serde::{Deserialize, Serialize};
use styx::{
    core::prelude::ColorSpace,
    imports::framelease::{FourCc, FrameBackingExport, FrameFdPlane, FrameLease, FrameLeaseDescriptor, FramePlaneDescriptor, FrameResidency},
};

const FRAMELEASE_TYPE_KEY: &str = "styx:framelease";
const FRAMELEASE_UNIX_PROTOCOL: &str = "styx-frame-lease+unix://";
const SHM_PROTOCOL: &str = "shm://";

#[derive(Debug, thiserror::Error)]
pub enum FrameStreamError {
    #[error("stream resource has no frame lease endpoint")]
    MissingEndpoint,
    #[error("failed to read stream metadata '{path}': {error}")]
    ReadMetadata { path: PathBuf, error: std::io::Error },
    #[error("failed to write stream metadata '{path}': {error}")]
    WriteMetadata { path: PathBuf, error: std::io::Error },
    #[error("failed to parse frame stream JSON: {0}")]
    Metadata(#[from] serde_json::Error),
    #[error("invalid frame stream descriptor: {0}")]
    InvalidDescriptor(String),
    #[error("failed to connect frame stream socket '{path}': {error}")]
    Connect { path: PathBuf, error: std::io::Error },
    #[error("frame stream '{path}' did not return a frame")]
    Empty { path: PathBuf },
    #[error("frame stream transport error: {0}")]
    Transport(String),
    #[error("frame transport descriptor/fd mismatch")]
    DescriptorMismatch,
    #[error("failed to import frame lease: {0}")]
    Import(String),
    #[error("failed to create frame stream directory '{path}': {error}")]
    CreateDirectory { path: PathBuf, error: std::io::Error },
    #[error("failed to remove stale frame stream socket '{path}': {error}")]
    RemoveSocket { path: PathBuf, error: std::io::Error },
    #[error("failed to bind frame stream socket '{path}': {error}")]
    BindSocket { path: PathBuf, error: std::io::Error },
    #[error("failed to export frame lease: {0}")]
    Export(String),
    #[error("failed to clone frame fd: {0}")]
    CloneFd(std::io::Error),
}

#[derive(Debug, Clone, Deserialize)]
struct CaptureFrameLeaseMetadata {
    socket_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrameLeaseTransportMessage {
    pub descriptor: FrameLeaseTransportDescriptor,
    pub backing: FrameLeaseTransportBacking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameLeaseTransportDescriptor {
    pub width: u32,
    pub height: u32,
    pub fourcc: String,
    pub timestamp: u64,
    pub color: String,
    pub planes: Vec<FrameLeaseTransportDescriptorPlane>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FrameLeaseTransportDescriptorPlane {
    pub offset: usize,
    pub len: usize,
    pub stride: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FrameLeaseTransportBacking {
    Memfd { len: usize },
    DmabufPlanes { planes: Vec<FrameLeaseTransportPlane> },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrameLeaseTransportPlane {
    pub offset: usize,
    pub len: usize,
}

#[derive(Debug)]
struct LatestFrameLease {
    descriptor: FrameLeaseDescriptor,
    backing: LatestBacking,
}

#[derive(Debug)]
enum LatestBacking {
    Memfd { fd: OwnedFd, len: usize },
    DmabufPlanes { planes: Vec<FrameFdPlane> },
}

#[derive(Debug)]
pub struct FrameOutputChannel {
    path: PathBuf,
    socket_path: PathBuf,
    latest: Arc<Mutex<Option<LatestFrameLease>>>,
}

static OUTPUT_CHANNELS: OnceLock<Mutex<std::collections::BTreeMap<PathBuf, Arc<FrameOutputChannel>>>> = OnceLock::new();

pub fn register_framelease_type() {
    daedalus::data::typing::register_type::<FrameLease>(daedalus::data::model::TypeExpr::opaque(FRAMELEASE_TYPE_KEY));
}

pub fn framelease_payload(frame: FrameLease) -> Payload {
    register_framelease_type();
    let residency = match frame.residency() {
        FrameResidency::HostOwned | FrameResidency::CompressedPacket => Residency::Cpu,
        FrameResidency::HostExternal | FrameResidency::Dmabuf => Residency::External,
        FrameResidency::GpuTexture => Residency::Gpu,
    };
    let bytes = Some(frame.payload_bytes() as u64);
    Payload::shared_with(TypeKey::new(FRAMELEASE_TYPE_KEY), Arc::new(frame), residency, None, bytes)
}

pub fn import_latest_frame_from_resource_endpoints(endpoints: &[String]) -> Result<FrameLease, FrameStreamError> {
    let socket_path = frame_socket_path_from_endpoints(endpoints)?;
    let stream = UnixStream::connect(&socket_path).map_err(|error| FrameStreamError::Connect { path: socket_path.clone(), error })?;
    let frame = recv_unix_fd_frame(&stream, DEFAULT_UNIX_FD_FRAME_MAX_PAYLOAD_BYTES, DEFAULT_UNIX_FD_FRAME_MAX_FDS)
        .map_err(|error| FrameStreamError::Transport(error.to_string()))?
        .ok_or_else(|| FrameStreamError::Empty { path: socket_path.clone() })?;
    import_frame(frame)
}

pub fn publish_output_frame(stream_dir: &Path, stream_id: &str, frame: &FrameLease) -> Result<(String, Vec<String>), FrameStreamError> {
    let path = stream_dir.join(format!("{stream_id}.stream.json"));
    let channel = output_channel(&path)?;
    channel.publish(frame)?;
    Ok((path.display().to_string(), vec![format!("{FRAMELEASE_UNIX_PROTOCOL}{}", channel.socket_path.display()), format!("{SHM_PROTOCOL}{}", channel.path.display())]))
}

fn frame_socket_path_from_endpoints(endpoints: &[String]) -> Result<PathBuf, FrameStreamError> {
    if let Some(endpoint) = endpoints.iter().find_map(|endpoint| endpoint.strip_prefix(FRAMELEASE_UNIX_PROTOCOL)) {
        return Ok(PathBuf::from(endpoint));
    }
    let Some(metadata_path) = endpoints.iter().find_map(|endpoint| endpoint.strip_prefix(SHM_PROTOCOL)) else {
        return Err(FrameStreamError::MissingEndpoint);
    };
    let metadata_path = PathBuf::from(metadata_path);
    let bytes = fs::read(&metadata_path).map_err(|error| FrameStreamError::ReadMetadata { path: metadata_path.clone(), error })?;
    let metadata: CaptureFrameLeaseMetadata = serde_json::from_slice(&bytes)?;
    Ok(PathBuf::from(metadata.socket_path))
}

fn import_frame(mut frame: UnixFdFrame) -> Result<FrameLease, FrameStreamError> {
    let message: FrameLeaseTransportMessage = serde_json::from_slice(&frame.payload)?;
    let descriptor = message.descriptor.try_into_descriptor()?;
    match message.backing {
        FrameLeaseTransportBacking::Memfd { .. } => {
            if frame.fds.len() != 1 {
                return Err(FrameStreamError::DescriptorMismatch);
            }
            FrameLease::from_memfd_import(descriptor, frame.fds.remove(0)).map_err(|error| FrameStreamError::Import(error.to_string()))
        }
        FrameLeaseTransportBacking::DmabufPlanes { planes } => {
            if planes.len() != frame.fds.len() {
                return Err(FrameStreamError::DescriptorMismatch);
            }
            let imported = planes.into_iter().zip(frame.fds).map(|(plane, fd)| FrameFdPlane { fd, offset: plane.offset, len: plane.len }).collect();
            FrameLease::from_dmabuf_import(descriptor, imported).map_err(|error| FrameStreamError::Import(error.to_string()))
        }
    }
}

fn output_channel(path: &Path) -> Result<Arc<FrameOutputChannel>, FrameStreamError> {
    let channels = OUTPUT_CHANNELS.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()));
    let mut channels = channels.lock().expect("frame output channel registry");
    if let Some(channel) = channels.get(path) {
        return Ok(channel.clone());
    }
    let channel = Arc::new(FrameOutputChannel::create(path)?);
    channels.insert(path.to_path_buf(), channel.clone());
    Ok(channel)
}

impl FrameOutputChannel {
    fn create(path: &Path) -> Result<Self, FrameStreamError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| FrameStreamError::CreateDirectory { path: parent.to_path_buf(), error })?;
        }
        let socket_path = stream_socket_path(path);
        if socket_path.exists() {
            fs::remove_file(&socket_path).map_err(|error| FrameStreamError::RemoveSocket { path: socket_path.clone(), error })?;
        }
        let listener = UnixListener::bind(&socket_path).map_err(|error| FrameStreamError::BindSocket { path: socket_path.clone(), error })?;
        let latest = Arc::new(Mutex::new(None));
        let latest_for_thread = latest.clone();
        thread::spawn(move || run_frame_server(listener, latest_for_thread));
        Ok(Self { path: path.to_path_buf(), socket_path, latest })
    }

    fn publish(&self, frame: &FrameLease) -> Result<(), FrameStreamError> {
        let exported = export_latest_frame(frame)?;
        {
            let mut latest = self.latest.lock().expect("frame output channel latest frame");
            *latest = Some(exported);
        }
        let metadata = serde_json::json!({
            "socket_path": self.socket_path.display().to_string(),
            "transport": "styx-frame-lease-v1",
        });
        fs::write(&self.path, metadata.to_string()).map_err(|error| FrameStreamError::WriteMetadata { path: self.path.clone(), error })?;
        Ok(())
    }
}

fn run_frame_server(listener: UnixListener, latest: Arc<Mutex<Option<LatestFrameLease>>>) {
    while let Ok((stream, _addr)) = listener.accept() {
        let message = {
            let latest = latest.lock().expect("frame output channel latest frame");
            latest.as_ref().and_then(|frame| prepare_transport_message(frame).ok())
        };
        if let Some(frame) = message {
            let _ = send_unix_fd_frame(&stream, &frame, DEFAULT_UNIX_FD_FRAME_MAX_PAYLOAD_BYTES, DEFAULT_UNIX_FD_FRAME_MAX_FDS);
        }
    }
}

fn export_latest_frame(frame: &FrameLease) -> Result<LatestFrameLease, FrameStreamError> {
    let (descriptor, backing) = frame.export_or_copy_memfd().map_err(|error| FrameStreamError::Export(error.to_string()))?;
    let backing = match backing {
        FrameBackingExport::Memfd { fd, len } => LatestBacking::Memfd { fd, len },
        FrameBackingExport::DmabufPlanes { planes } => LatestBacking::DmabufPlanes { planes },
    };
    Ok(LatestFrameLease { descriptor, backing })
}

fn prepare_transport_message(frame: &LatestFrameLease) -> Result<UnixFdFrame, FrameStreamError> {
    match &frame.backing {
        LatestBacking::Memfd { fd, len } => {
            let payload = serde_json::to_vec(&FrameLeaseTransportMessage {
                descriptor: FrameLeaseTransportDescriptor::from_descriptor(&frame.descriptor),
                backing: FrameLeaseTransportBacking::Memfd { len: *len },
            })?;
            Ok(UnixFdFrame::new(payload, vec![fd.try_clone().map_err(FrameStreamError::CloneFd)?]))
        }
        LatestBacking::DmabufPlanes { planes } => {
            let payload = serde_json::to_vec(&FrameLeaseTransportMessage {
                descriptor: FrameLeaseTransportDescriptor::from_descriptor(&frame.descriptor),
                backing: FrameLeaseTransportBacking::DmabufPlanes { planes: planes.iter().map(|plane| FrameLeaseTransportPlane { offset: plane.offset, len: plane.len }).collect() },
            })?;
            let fds = planes.iter().map(|plane| plane.fd.try_clone().map_err(FrameStreamError::CloneFd)).collect::<Result<Vec<_>, _>>()?;
            Ok(UnixFdFrame::new(payload, fds))
        }
    }
}

fn stream_socket_path(path: &Path) -> PathBuf {
    let mut socket_path = path.to_path_buf();
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("stream");
    socket_path.set_file_name(format!("{file_name}.sock"));
    socket_path
}

impl FrameLeaseTransportDescriptor {
    fn from_descriptor(descriptor: &FrameLeaseDescriptor) -> Self {
        Self {
            width: descriptor.width,
            height: descriptor.height,
            fourcc: descriptor.fourcc.to_string(),
            timestamp: descriptor.timestamp,
            color: color_space_name(descriptor.color).to_string(),
            planes: descriptor.planes.iter().map(FrameLeaseTransportDescriptorPlane::from_descriptor).collect(),
        }
    }

    fn try_into_descriptor(self) -> Result<FrameLeaseDescriptor, FrameStreamError> {
        let fourcc = FourCc::from_str(&self.fourcc).map_err(FrameStreamError::InvalidDescriptor)?;
        Ok(FrameLeaseDescriptor {
            width: self.width,
            height: self.height,
            fourcc,
            timestamp: self.timestamp,
            color: parse_color_space(&self.color),
            planes: self.planes.into_iter().map(FrameLeaseTransportDescriptorPlane::into_descriptor).collect(),
        })
    }
}

impl FrameLeaseTransportDescriptorPlane {
    fn from_descriptor(descriptor: &FramePlaneDescriptor) -> Self {
        Self { offset: descriptor.offset, len: descriptor.len, stride: descriptor.stride }
    }

    fn into_descriptor(self) -> FramePlaneDescriptor {
        FramePlaneDescriptor { offset: self.offset, len: self.len, stride: self.stride }
    }
}

fn color_space_name(color: ColorSpace) -> &'static str {
    match color {
        ColorSpace::Srgb => "Srgb",
        ColorSpace::Bt709 => "Bt709",
        ColorSpace::Bt2020 => "Bt2020",
        ColorSpace::Unknown => "Unknown",
    }
}

fn parse_color_space(value: &str) -> ColorSpace {
    match value {
        "Srgb" | "srgb" => ColorSpace::Srgb,
        "Bt709" | "bt709" => ColorSpace::Bt709,
        "Bt2020" | "bt2020" => ColorSpace::Bt2020,
        _ => ColorSpace::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use styx::{
        core::prelude::{BufferPool, MediaFormat, Resolution, plane_layout_from_dims},
        imports::framelease::FrameMeta,
    };

    use super::*;

    fn test_frame(timestamp: u64) -> FrameLease {
        let format = MediaFormat::new(FourCc::from_str("RG24").expect("fourcc"), Resolution::new(2, 2).expect("resolution"), ColorSpace::Srgb);
        let layout = plane_layout_from_dims(NonZeroU32::new(2).expect("width"), NonZeroU32::new(2).expect("height"), 3);
        let pool = BufferPool::lazy(layout.len, 1);
        FrameLease::single_plane(FrameMeta::new(format, timestamp), pool.lease(), layout.len, layout.stride)
    }

    #[test]
    fn output_channel_round_trips_framelease_without_inline_bytes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let frame = test_frame(42);

        let (_path, endpoints) = publish_output_frame(temp.path(), "frame-out", &frame).expect("publish");
        let imported = import_latest_frame_from_resource_endpoints(&endpoints).expect("import");

        assert_eq!(imported.meta().timestamp, 42);
        assert_eq!(imported.meta().format.resolution.width.get(), 2);
        assert_eq!(imported.meta().format.resolution.height.get(), 2);
        assert!(endpoints.iter().any(|endpoint| endpoint.starts_with(FRAMELEASE_UNIX_PROTOCOL)));
    }
}
