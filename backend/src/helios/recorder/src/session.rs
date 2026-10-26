//! The recording thread: reads the camera's frames and the host messages, writes them into the
//! current part, rotates parts at their caps, and ends on request, at the storage reserve or when
//! the camera is lost. A part being completed is flushed on a finisher thread while the camera
//! client keeps being read (and discarded), so the camera never waits on the disk and a stop
//! returns at once with the state `finishing`.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Receiver,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use styx::replay::{RecorderOptions, RecordingHeader};

use crate::{
    catalog,
    error::RecorderError,
    frames::FrameSource,
    host::HostMessage,
    info::RecordingInfo,
    lemnos::{self, LemnosSource},
    pace::Pacer,
    settings::RecorderSettings,
    status::{PartSummary, RecordingState, RecordingStatus},
    storage,
    writer::PartWriter,
};

/// How long the loop waits for a frame before looking at the host messages and the clock.
const POLL: Duration = Duration::from_millis(20);
/// How often the disk, the reserve and the caps are checked.
const TICK: Duration = Duration::from_secs(1);
/// Host messages written per pass, so frames are not held up by a burst of them.
const HOST_PER_PASS: usize = 1000;

/// Shared between the recording thread and its callers.
pub struct Shared {
    pub status: Mutex<RecordingStatus>,
    pub stop: AtomicBool,
    /// The recordings folder's budget, read before each new part (see `Recorder::set_max_total_bytes`).
    pub max_total_bytes: AtomicU64,
    /// Host messages the senders dropped (channel full).
    pub host_dropped: Arc<AtomicU64>,
}

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, RecordingStatus> {
        self.status.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

/// Everything the thread needs, opened by `Recorder::start`.
pub struct SessionRun {
    pub shared: Arc<Shared>,
    pub source: FrameSource,
    pub host_rx: Receiver<HostMessage>,
    pub first: PartWriter,
    pub dir: PathBuf,
    pub session: String,
    /// Keeps every part of the session from being pruned while the recording runs.
    pub claim: catalog::Claim,
    pub camera: String,
    pub header: RecordingHeader,
    pub options: RecorderOptions,
    pub info: RecordingInfo,
    pub settings: RecorderSettings,
    pub lemnos: Option<(LemnosSource, crate::host::HostSender)>,
    pub started: Instant,
}

/// A part being completed on its own thread.
struct Finisher {
    handle: JoinHandle<Result<PartSummary, RecorderError>>,
}

/// Starts the recording thread (and the lemnosd reader, when there is one).
pub fn spawn(run: SessionRun) -> JoinHandle<()> {
    std::thread::Builder::new().name("helios-recorder".into()).spawn(move || run_loop(run)).expect("spawn recorder thread")
}

struct Loop {
    shared: Arc<Shared>,
    source: FrameSource,
    host_rx: Receiver<HostMessage>,
    dir: PathBuf,
    session: String,
    /// Held until the recording ends: the session is not pruned while it runs.
    _claim: catalog::Claim,
    camera: String,
    header: RecordingHeader,
    options: RecorderOptions,
    info: RecordingInfo,
    settings: RecorderSettings,
    started: Instant,
    current: Option<PartWriter>,
    parts: Vec<PartSummary>,
    pending: Vec<Finisher>,
    /// Host messages dropped before this part's count was taken.
    host_dropped_reported: u64,
    free: Option<u64>,
    error: Option<String>,
    pacer: Pacer,
}

fn run_loop(mut run: SessionRun) {
    // The lemnosd reader lives as long as this recording: it stops with the same flag.
    let lemnos_stop = Arc::new(AtomicBool::new(false));
    let lemnos_thread = run.lemnos.take().map(|(source, sender)| lemnos::spawn(source, sender, lemnos_stop.clone()));
    let pacer = Pacer::new(run.settings.max_fps);
    let mut state = Loop {
        shared: run.shared,
        source: run.source,
        host_rx: run.host_rx,
        dir: run.dir,
        session: run.session,
        _claim: run.claim,
        camera: run.camera,
        header: run.header,
        options: run.options,
        info: run.info,
        settings: run.settings,
        started: run.started,
        current: Some(run.first),
        parts: Vec::new(),
        pending: Vec::new(),
        host_dropped_reported: 0,
        free: None,
        error: None,
        pacer,
    };
    let reason = state.drive();
    state.finish(reason);
    lemnos_stop.store(true, Ordering::Relaxed);
    if let Some(handle) = lemnos_thread {
        let _ = handle.join();
    }
}

impl Loop {
    /// Runs until the recording ends; returns why (`requested`, `storage_reserve`, `camera_lost`,
    /// or `error`).
    fn drive(&mut self) -> &'static str {
        let mut last_tick = Instant::now();
        let mut ending: Option<&'static str> = None;
        loop {
            if ending.is_none() && self.shared.stop.load(Ordering::Relaxed) {
                ending = Some("requested");
            }
            if ending.is_some() {
                self.close_current(ending);
            }
            if let Some(writer) = self.current.as_mut() {
                match self.source.next(POLL) {
                    Ok(Some(frame)) => {
                        if self.pacer.admit(Instant::now())
                            && let Err(error) = writer.record(&frame)
                        {
                            self.error = Some(error.to_string());
                            ending = Some("error");
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.error = Some(error.to_string());
                        ending = Some("camera_lost");
                    }
                }
                if let Err(error) = write_host(writer, &self.host_rx) {
                    self.error = Some(error.to_string());
                    ending = Some("error");
                }
            } else {
                // Finishing: the camera is read and the frames dropped, so it never waits.
                let _ = self.source.next(POLL);
                while self.host_rx.try_recv().is_ok() {}
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                self.collect_finished();
                if ending.is_none() {
                    ending = self.tick();
                }
                self.publish();
            }
            self.collect_finished();
            if ending.is_some() && self.current.is_none() && self.pending.is_empty() {
                return ending.unwrap_or("requested");
            }
            if self.error.is_some() && ending.is_none() {
                ending = Some("error");
            }
        }
    }

    /// Once a second: the reserve, the caps (rotating to a new part) and the status.
    fn tick(&mut self) -> Option<&'static str> {
        match storage::check_reserve(&self.dir, self.settings.reserve_bytes) {
            Ok(free) => self.free = Some(free),
            Err(storage::StorageError::ReserveNotMet { free, .. }) => {
                self.free = Some(free);
                return Some("storage_reserve");
            }
            Err(error) => {
                self.error = Some(error.to_string());
                return Some("error");
            }
        }
        let writer = self.current.as_ref()?;
        let capped = writer.bytes_on_disk() >= self.settings.max_file_bytes || writer.elapsed() >= Duration::from_secs(self.settings.max_file_seconds);
        if capped {
            self.rotate();
        }
        None
    }

    /// Completes the current part on a finisher thread and opens the next one, after the oldest
    /// finished recordings have made room for it.
    fn rotate(&mut self) {
        let index = self.current.as_ref().map(PartWriter::index).unwrap_or(0);
        self.close_current(Some("cap"));
        if let Err(error) = self.make_room() {
            self.error = Some(format!("pruning the recordings folder: {error}"));
            return;
        }
        match PartWriter::open(&self.dir, &self.session, &self.camera, index + 1, &self.header, self.options.clone(), &self.info) {
            Ok(writer) => self.current = Some(writer),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Deletes the oldest finished recordings while the folder is over its budget. This recording
    /// is kept whole.
    fn make_room(&self) -> std::io::Result<()> {
        let budget = self.shared.max_total_bytes.load(Ordering::Relaxed);
        catalog::prune(&self.dir, budget, Some(&self.session)).map(|_| ())
    }

    /// Hands the current part to a finisher thread (no-op without one).
    fn close_current(&mut self, reason: Option<&'static str>) {
        let Some(writer) = self.current.take() else { return };
        let now = self.shared.host_dropped.load(Ordering::Relaxed);
        let host_dropped = now.saturating_sub(self.host_dropped_reported);
        self.host_dropped_reported = now;
        let reason = reason.map(str::to_string);
        let handle = std::thread::Builder::new().name("helios-recorder-finish".into()).spawn(move || writer.finish(reason, host_dropped)).expect("spawn finisher");
        self.pending.push(Finisher { handle });
    }

    fn collect_finished(&mut self) {
        let (done, waiting): (Vec<Finisher>, Vec<Finisher>) = std::mem::take(&mut self.pending).into_iter().partition(|finisher| finisher.handle.is_finished());
        self.pending = waiting;
        for finisher in done {
            match finisher.handle.join() {
                Ok(Ok(summary)) => self.parts.push(summary),
                Ok(Err(error)) => self.error = Some(error.to_string()),
                Err(_) => self.error = Some("the part writer panicked".into()),
            }
        }
    }

    /// Writes the totals and the current part into the shared status.
    fn publish(&self) {
        let mut status = self.shared.lock();
        let (frames, frames_dropped, messages, messages_dropped, bytes, file) = match &self.current {
            Some(writer) => {
                let counts = writer.counts();
                (counts.frames_written, counts.frames_dropped, counts.messages_written, counts.messages_dropped, writer.bytes_on_disk(), Some(writer.file().to_string()))
            }
            None => (0, 0, 0, 0, 0, None),
        };
        let finished = self.parts.iter().fold((0, 0, 0, 0, 0), |acc, p| (acc.0 + p.frames_written, acc.1 + p.frames_dropped, acc.2 + p.messages_written, acc.3 + p.messages_dropped, acc.4 + p.bytes));
        let unreported = self.shared.host_dropped.load(Ordering::Relaxed).saturating_sub(self.host_dropped_reported);
        status.file = file;
        if status.state == RecordingState::Starting && frames > 0 {
            status.state = RecordingState::Recording;
        }
        status.parts = self.parts.clone();
        status.frames_written = finished.0 + frames;
        status.frames_dropped = finished.1 + frames_dropped;
        status.messages_written = finished.2 + messages;
        status.messages_dropped = finished.3 + messages_dropped + unreported;
        status.bytes = finished.4 + bytes;
        status.free_bytes = self.free.or(status.free_bytes);
        status.max_total_bytes = self.shared.max_total_bytes.load(Ordering::Relaxed);
        status.duration_ms = self.started.elapsed().as_millis() as u64;
    }

    /// Ends the recording: the last part is completed (waiting for the finisher), the state is
    /// set, and the thread is done.
    fn finish(&mut self, reason: &'static str) {
        // Anything left in flight is awaited: the parts must all be complete before the state says so.
        while !self.pending.is_empty() {
            self.collect_finished();
            if !self.pending.is_empty() {
                std::thread::sleep(POLL);
            }
        }
        self.publish();
        let mut status = self.shared.lock();
        status.state = if self.error.is_some() { RecordingState::Failed } else { RecordingState::Stopped };
        status.stop_reason = Some(reason.to_string());
        status.error = self.error.clone();
        status.file = None;
        status.parts = self.parts.clone();
        status.duration_ms = self.started.elapsed().as_millis() as u64;
        status.bytes = self.parts.iter().map(|p| p.bytes).sum();
    }
}

/// Writes the host messages waiting now (a bounded number, so frames are not held up).
fn write_host(writer: &mut PartWriter, rx: &Receiver<HostMessage>) -> Result<(), RecorderError> {
    for message in rx.try_iter().take(HOST_PER_PASS) {
        writer.write(&message)?;
    }
    Ok(())
}
