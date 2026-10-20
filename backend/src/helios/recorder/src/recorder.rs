//! The recorder: one recording at a time per device. `start` checks the settings, the storage and
//! the camera, opens the first part and starts the recording thread; `stop` asks it to end and
//! returns at once (the state is `finishing` until the last part is complete).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use chrono::Utc;

use crate::{
    capture::{self, ClipRequest},
    error::RecorderError,
    frames::{self, FrameSource},
    host::HostSender,
    info::RecordingInfo,
    lemnos::LemnosSource,
    naming,
    ring::{self, RingHandle, RingStatus},
    session::{self, SessionRun, Shared},
    settings::RecorderSettings,
    status::{PartSummary, RecordingState, RecordingStatus},
    storage,
    writer::{self, PartWriter},
};

/// Everything a recording is made from.
pub struct RecordingRequest {
    pub camera_id: String,
    /// The camera service's frames socket (`styx-frames+unix://...`, its path).
    pub frames_socket: PathBuf,
    /// The board's name, for the file name (its hostname).
    pub board: String,
    /// The camera's short label, for the file name (`ov9782`); the full id is in the metadata.
    pub camera_label: String,
    /// The HeliOS commit the image was built from, for the file name.
    pub commit: String,
    pub settings: RecorderSettings,
    pub info: RecordingInfo,
    /// The lemnosd samples to record as `/helios/imu`, `/helios/mag` and `/helios/power`.
    pub lemnos: Option<LemnosSource>,
}

struct Active {
    id: String,
    shared: Arc<Shared>,
    host: HostSender,
}

/// A camera whose capture-last ring should run, and the settings it runs with.
pub struct RingRequest {
    pub camera_id: String,
    /// The camera service's frames socket.
    pub frames_socket: PathBuf,
    pub settings: RecorderSettings,
}

/// Runs at most one recording at a time, and a capture-last ring per camera that asked for one.
#[derive(Default)]
pub struct Recorder {
    active: Mutex<Option<Active>>,
    rings: Mutex<BTreeMap<String, RingHandle>>,
}

impl Recorder {
    /// Starts a recording; returns its status once the camera and the first part are open.
    pub fn start(&self, request: RecordingRequest) -> Result<RecordingStatus, RecorderError> {
        request.settings.validate().map_err(RecorderError::Settings)?;
        let mut active = self.active.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(current) = active.as_ref()
            && current.shared.lock().state.is_active()
        {
            return Err(RecorderError::Busy(current.id.clone()));
        }
        *active = None;

        let settings = request.settings;
        let dir = storage::resolve_dir(&settings)?;
        let free = storage::check_reserve(&dir, settings.reserve_bytes)?;
        let session = naming::unused_session_stem(&dir, &naming::session_stem(&request.board, &request.camera_label, Utc::now(), &request.commit));
        let source = FrameSource::open(&request.frames_socket, &frames::request(settings.frames))?;
        let delivered = source.delivered().ok_or_else(|| RecorderError::Camera("the camera service did not say what frames it delivers".into()))?;
        let format = format!("{} {}x{}", delivered.format, delivered.size.0, delivered.size.1);
        let header = writer::header_for(&request.camera_id, delivered.format, delivered.size, delivered.fps)?;
        let options = writer::options_for(settings.compression, settings.queue_bytes);
        let first = PartWriter::open(&dir, &session, &request.camera_id, 1, &header, options.clone(), &request.info)?;

        let (host, host_rx, host_dropped) = HostSender::channel();
        let shared = Arc::new(Shared {
            status: Mutex::new(RecordingStatus {
                id: session.clone(),
                camera: request.camera_id.clone(),
                state: RecordingState::Starting,
                started_at_ms: writer::unix_ms(),
                duration_ms: 0,
                dir: dir.display().to_string(),
                file: Some(first.file().to_string()),
                parts: Vec::new(),
                bytes: 0,
                frames_written: 0,
                frames_dropped: 0,
                messages_written: 0,
                messages_dropped: 0,
                free_bytes: Some(free),
                reserve_bytes: settings.reserve_bytes,
                max_fps: settings.max_fps,
                frames: settings.frames,
                format,
                compression: settings.compression,
                stop_reason: None,
                error: None,
            }),
            stop: AtomicBool::new(false),
            host_dropped,
        });
        let status = shared.lock().clone();
        let run = SessionRun {
            shared: shared.clone(),
            source,
            host_rx,
            first,
            dir,
            session: session.clone(),
            camera: request.camera_id,
            header,
            options,
            info: request.info,
            settings,
            lemnos: request.lemnos.map(|lemnos| (lemnos, host.clone())),
            started: Instant::now(),
        };
        drop(session::spawn(run));
        *active = Some(Active { id: session, shared, host });
        Ok(status)
    }

    /// Asks recording `id` to end. Returns at once, with the state `finishing` until the last part
    /// is complete (poll `status`).
    pub fn stop(&self, id: &str) -> Result<RecordingStatus, RecorderError> {
        let active = self.active.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(current) = active.as_ref().filter(|current| current.id == id) else {
            return Err(RecorderError::NotFound(format!("no recording {id} is running")));
        };
        let mut status = current.shared.lock();
        if status.state.is_active() {
            current.shared.stop.store(true, Ordering::Relaxed);
            status.state = RecordingState::Finishing;
        }
        Ok(status.clone())
    }

    /// The running recording, or the last one until the next starts.
    pub fn status(&self) -> Option<RecordingStatus> {
        let active = self.active.lock().unwrap_or_else(|poison| poison.into_inner());
        active.as_ref().map(|current| current.shared.lock().clone())
    }

    /// Where host messages go while a recording is active (`None` otherwise).
    pub fn host(&self) -> Option<HostSender> {
        let active = self.active.lock().unwrap_or_else(|poison| poison.into_inner());
        active.as_ref().filter(|current| current.shared.lock().state.is_active()).map(|current| current.host.clone())
    }
}

impl Recorder {
    fn rings_lock(&self) -> MutexGuard<'_, BTreeMap<String, RingHandle>> {
        self.rings.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Makes the rings match `wanted`: the rings of other cameras stop, missing ones start, those
    /// whose settings changed restart, and failed ones are tried again. Opening a camera waits for
    /// its camera service (a missing one fails at once). Returns the status of each wanted ring.
    pub fn sync_rings(&self, wanted: &[RingRequest]) -> Vec<RingStatus> {
        let mut rings = self.rings_lock();
        rings.retain(|camera, _| wanted.iter().any(|request| &request.camera_id == camera));
        for request in wanted {
            if rings.get(&request.camera_id).is_some_and(|ring| ring.is_running() && ring.matches(&request.settings)) {
                continue;
            }
            rings.remove(&request.camera_id);
            let settings = &request.settings;
            let handle = start_ring(request).unwrap_or_else(|error| RingHandle::failed(settings.capture_last_seconds, settings.frames, settings.max_fps, error.to_string()));
            rings.insert(request.camera_id.clone(), handle);
        }
        wanted.iter().filter_map(|request| rings.get(&request.camera_id).map(|ring| ring.status(&request.camera_id))).collect()
    }

    /// Every ring's status.
    pub fn rings(&self) -> Vec<RingStatus> {
        self.rings_lock().iter().map(|(camera, ring)| ring.status(camera)).collect()
    }

    /// Writes the last `last` of `clip.camera_id`'s ring (all it holds when `None`) to a new
    /// stored recording in `dir`. The ring keeps running.
    pub fn capture_last(&self, last: Option<Duration>, clip: &ClipRequest<'_>, dir: &Path) -> Result<PartSummary, RecorderError> {
        let frames = {
            let rings = self.rings_lock();
            let ring = rings.get(clip.camera_id).ok_or_else(|| RecorderError::NotFound(format!("no capture-last ring runs for {}", clip.camera_id)))?;
            if !ring.is_running() {
                let status = ring.status(clip.camera_id);
                return Err(RecorderError::Camera(status.error.unwrap_or_else(|| "the capture-last ring is not running".into())));
            }
            ring.shared.lock().snapshot(last)
        };
        capture::write_clip(&frames, dir, clip)
    }
}

/// Opens the camera for a ring and starts it, with a budget of RAM from the memory available now.
fn start_ring(request: &RingRequest) -> Result<RingHandle, RecorderError> {
    let settings = &request.settings;
    settings.validate().map_err(RecorderError::Settings)?;
    let source = FrameSource::open(&request.frames_socket, &frames::request(settings.frames))?;
    let delivered = source.delivered().ok_or_else(|| RecorderError::Camera("the camera service did not say what frames it delivers".into()))?;
    let frame_bytes = u64::from(delivered.size.0) * u64::from(delivered.size.1) * capture::bytes_per_pixel(delivered.format);
    let fps = settings.max_fps.or(delivered.fps.map(f64::from)).unwrap_or(30.0);
    let budget = ring::budget_bytes(settings.capture_last_seconds, fps, frame_bytes, ring::memory_available())?;
    Ok(RingHandle::start(source, settings.capture_last_seconds, settings.frames, settings.max_fps, budget))
}
