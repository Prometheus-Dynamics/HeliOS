//! The capture-last ring: the last seconds of a camera's frames, kept compressed in RAM, so a clip
//! can be saved on demand while the camera keeps running. A ring is its own low-priority client of
//! the camera service (like a recording), subsampled to the recording's `max_fps`; it holds copies,
//! never leases, so it never keeps camera buffers out of the capture.

use std::{
    collections::VecDeque,
    fs, io,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::Serialize;
use styx::{
    core::prelude::{FrameLease, FrameMeta},
    prelude::MediaFormat,
};

use crate::{error::RecorderError, frames::FrameSource, pace::Pacer, settings::MIB};

/// How long the ring loop waits for a frame before it checks whether it is to stop.
const POLL: Duration = Duration::from_millis(50);
/// Share of the memory available that a ring may take at most.
const MEMORY_SHARE: u64 = 4;
/// Most a ring may hold, whatever the memory: a ring is a copy of the camera's frames in RAM.
const MAX_RING_BYTES: u64 = 512 * MIB;
/// Compression assumed when sizing a ring (lz4 on 16-bit Bayer measured 2.24:1 on the CM5).
const ASSUMED_RATIO: f64 = 2.0;

/// One frame in a ring: its format, timestamp and metadata, and its visible bytes compressed.
pub struct RingFrame {
    pub format: MediaFormat,
    pub timestamp_ns: u64,
    pub meta: FrameMeta,
    packed: Vec<u8>,
}

impl RingFrame {
    /// Copies and compresses the frame (the lease is released by the caller).
    pub fn from_lease(frame: &FrameLease) -> io::Result<Self> {
        let bytes = frame.to_visible_vec().map_err(io::Error::other)?;
        let packed = lz4::block::compress(&bytes, None, true)?;
        Ok(Self { format: frame.meta().format, timestamp_ns: frame.meta().timestamp, meta: frame.meta().clone(), packed })
    }

    /// The frame again, with its metadata (for writing a clip).
    pub fn to_lease(&self) -> Result<FrameLease, RecorderError> {
        let bytes = lz4::block::decompress(&self.packed, None).map_err(|error| RecorderError::Io(error.to_string()))?;
        let mut lease = FrameLease::from_visible_bytes(self.format, self.timestamp_ns, &bytes).map_err(|error| RecorderError::Mcap(error.to_string()))?;
        *lease.meta_mut() = FrameMeta { format: lease.meta().format, ..self.meta.clone() };
        Ok(lease)
    }

    pub fn packed_bytes(&self) -> usize {
        self.packed.len()
    }
}

/// The frames of one ring, oldest first, within a byte budget and a time window.
pub struct Ring {
    frames: VecDeque<Arc<RingFrame>>,
    bytes: usize,
    budget: usize,
    window_ns: u64,
}

impl Ring {
    pub fn new(budget: usize, window: Duration) -> Self {
        Self { frames: VecDeque::new(), bytes: 0, budget, window_ns: window.as_nanos() as u64 }
    }

    /// Adds `frame`, then drops the oldest frames beyond the budget or older than the window (the
    /// newest frame always stays).
    pub fn push(&mut self, frame: RingFrame) {
        let newest = frame.timestamp_ns;
        self.bytes += frame.packed_bytes();
        self.frames.push_back(Arc::new(frame));
        while self.frames.len() > 1 {
            let Some(oldest) = self.frames.front() else { break };
            let over_budget = self.bytes > self.budget;
            let too_old = newest.saturating_sub(oldest.timestamp_ns) > self.window_ns;
            if !over_budget && !too_old {
                break;
            }
            self.bytes -= oldest.packed_bytes();
            self.frames.pop_front();
        }
    }

    /// The frames of the last `last` (all of them when `None`), oldest first.
    pub fn snapshot(&self, last: Option<Duration>) -> Vec<Arc<RingFrame>> {
        let Some(newest) = self.frames.back().map(|f| f.timestamp_ns) else { return Vec::new() };
        let since = last.map(|d| newest.saturating_sub(d.as_nanos() as u64));
        self.frames.iter().filter(|f| since.is_none_or(|s| f.timestamp_ns >= s)).cloned().collect()
    }

    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Seconds from the oldest frame to the newest.
    pub fn window_seconds(&self) -> f64 {
        match (self.frames.front(), self.frames.back()) {
            (Some(oldest), Some(newest)) => newest.timestamp_ns.saturating_sub(oldest.timestamp_ns) as f64 / 1e9,
            _ => 0.0,
        }
    }
}

/// The bytes a ring may hold for `seconds` of frames at `fps`: their size compressed, but at most a
/// quarter of the memory available now and [`MAX_RING_BYTES`]. Refused when that is not two frames.
pub fn budget_bytes(seconds: u32, fps: f64, frame_bytes: u64, available: Option<u64>) -> Result<usize, RecorderError> {
    let wanted = (f64::from(seconds) * fps * frame_bytes as f64 / ASSUMED_RATIO) as u64;
    // Without a reading of the memory, assume 1 GiB free: the quarter is a conservative 256 MiB.
    let cap = (available.unwrap_or(1 << 30) / MEMORY_SHARE).min(MAX_RING_BYTES);
    let budget = wanted.min(cap);
    if (budget as f64) < 2.0 * frame_bytes as f64 / ASSUMED_RATIO {
        return Err(RecorderError::Settings(format!("not enough free memory for a capture-last ring: {} MiB free", available.unwrap_or(0) >> 20)));
    }
    Ok(budget as usize)
}

/// Memory available to new allocations (`MemAvailable` in `/proc/meminfo`), in bytes.
pub fn memory_available() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|line| line.starts_with("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// A ring's state, as the API shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RingStatus {
    pub camera: String,
    /// `running`, or `failed` with `error` (retried on the next sync).
    pub state: &'static str,
    pub error: Option<String>,
    /// The seconds configured, and the seconds held now.
    pub seconds: u32,
    pub window_seconds: f64,
    pub frames: usize,
    pub bytes: usize,
    pub budget_bytes: usize,
    pub format: Option<String>,
}

/// The shared part of a running ring: its frames, and what the loop saw.
pub struct RingShared {
    pub ring: Mutex<Ring>,
    pub error: Mutex<Option<String>>,
    pub format: Mutex<Option<MediaFormat>>,
}

impl RingShared {
    pub fn lock(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

/// A running ring thread.
pub struct RingHandle {
    pub seconds: u32,
    pub frames: crate::settings::RecordFrames,
    pub max_fps: Option<f64>,
    pub shared: Arc<RingShared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RingHandle {
    /// A ring that could not start (its status says why); the next sync tries again.
    pub fn failed(seconds: u32, frames: crate::settings::RecordFrames, max_fps: Option<f64>, error: String) -> Self {
        let shared = Arc::new(RingShared { ring: Mutex::new(Ring::new(0, Duration::from_secs(1))), error: Mutex::new(Some(error)), format: Mutex::new(None) });
        Self { seconds, frames, max_fps, shared, stop: Arc::new(AtomicBool::new(false)), thread: None }
    }

    /// Whether the ring thread runs without an error.
    pub fn is_running(&self) -> bool {
        self.thread.is_some() && self.shared.error.lock().unwrap_or_else(|poison| poison.into_inner()).is_none()
    }

    /// Whether the ring was started with `settings`' ring and frame values.
    pub fn matches(&self, settings: &crate::settings::RecorderSettings) -> bool {
        self.seconds == settings.capture_last_seconds && self.frames == settings.frames && self.max_fps == settings.max_fps
    }

    /// Starts the ring loop on `source`, which keeps the frames the pacer admits.
    pub fn start(source: FrameSource, seconds: u32, frames: crate::settings::RecordFrames, max_fps: Option<f64>, budget: usize) -> Self {
        let shared = Arc::new(RingShared { ring: Mutex::new(Ring::new(budget, Duration::from_secs(u64::from(seconds)))), error: Mutex::new(None), format: Mutex::new(None) });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop) = (shared.clone(), stop.clone());
            std::thread::Builder::new().name("helios-capture-last".into()).spawn(move || run(source, Pacer::new(max_fps), &shared, &stop)).ok()
        };
        Self { seconds, frames, max_fps, shared, stop, thread }
    }

    pub fn status(&self, camera: &str) -> RingStatus {
        let ring = self.shared.lock();
        let error = self.shared.error.lock().unwrap_or_else(|poison| poison.into_inner()).clone();
        let format = *self.shared.format.lock().unwrap_or_else(|poison| poison.into_inner());
        RingStatus {
            camera: camera.to_string(),
            state: if error.is_some() || self.thread.is_none() { "failed" } else { "running" },
            error: error.or_else(|| self.thread.is_none().then(|| "the ring thread did not start".to_string())),
            seconds: self.seconds,
            window_seconds: ring.window_seconds(),
            frames: ring.frames(),
            bytes: ring.bytes(),
            budget_bytes: ring.budget(),
            format: format.map(|f| format!("{} {}x{}", f.code, f.resolution.width.get(), f.resolution.height.get())),
        }
    }
}

impl Drop for RingHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(source: FrameSource, mut pacer: Pacer, shared: &RingShared, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        match source.next(POLL) {
            Ok(Some(frame)) => {
                if !pacer.admit(Instant::now()) {
                    continue;
                }
                *shared.format.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(frame.meta().format);
                match RingFrame::from_lease(&frame) {
                    Ok(copy) => shared.lock().push(copy),
                    Err(error) => *shared.error.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(error.to_string()),
                }
            }
            Ok(None) => {}
            Err(error) => {
                *shared.error.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(error.to_string());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_the_compressed_window_capped_by_memory() {
        let frame = 2_048_000;
        // 10 s at 30 fps, compressed 2:1: about 307 MB, within a quarter of 4 GiB.
        assert_eq!(budget_bytes(10, 30.0, frame, Some(4 << 30)).unwrap(), 307_200_000);
        // With 800 MiB free the ring is trimmed to a quarter of that.
        assert_eq!(budget_bytes(10, 30.0, frame, Some(800 << 20)).unwrap(), (200 << 20) as usize);
        // However much memory there is, a ring stops at 512 MiB.
        assert_eq!(budget_bytes(60, 120.0, 3_000_000, Some(16 << 30)).unwrap(), (512 << 20) as usize);
        // Not even two frames fit: refused, and the message names the memory.
        let refused = budget_bytes(10, 30.0, frame, Some(4 << 20)).unwrap_err();
        assert!(refused.to_string().contains("free memory"), "{refused}");
    }

    #[test]
    fn a_frame_round_trips_through_the_ring_with_its_metadata() {
        let format = MediaFormat::new(styx::prelude::FourCc::new(*b"BYR2"), styx::prelude::Resolution::new(4, 2).expect("resolution"), styx::prelude::ColorSpace::Srgb);
        // 4x2 samples of 16 bits: 16 bytes.
        let bytes: Vec<u8> = (0..8u8).flat_map(|v| [v, 0]).collect();
        let mut lease = FrameLease::from_visible_bytes(format, 42_000, &bytes).expect("lease");
        lease.meta_mut().timestamp = 42_000;
        let copy = RingFrame::from_lease(&lease).expect("copy");
        assert_eq!((copy.timestamp_ns, copy.format), (42_000, format));
        let back = copy.to_lease().expect("lease again");
        assert_eq!(back.to_visible_vec().expect("bytes"), bytes);
        assert_eq!(back.meta().timestamp, 42_000);
    }

    #[test]
    fn the_ring_trims_to_its_budget_and_its_window() {
        let format = MediaFormat::new(styx::prelude::FourCc::new(*b"BYR2"), styx::prelude::Resolution::new(64, 64).expect("resolution"), styx::prelude::ColorSpace::Srgb);
        let frame_at = |ms: u64| {
            let mut lease = FrameLease::from_visible_bytes(format, ms * 1_000_000, &vec![7u8; 64 * 64 * 2]).expect("lease");
            lease.meta_mut().timestamp = ms * 1_000_000;
            RingFrame::from_lease(&lease).expect("copy")
        };
        // Window of 1 s: frames 2.5 s old are gone; the newest always stays.
        let mut ring = Ring::new(usize::MAX, Duration::from_secs(1));
        for i in 0..=75u64 {
            ring.push(frame_at(i * 33));
        }
        assert!(ring.window_seconds() <= 1.0 + 1e-9, "{}", ring.window_seconds());
        assert_eq!(ring.snapshot(None).last().map(|f| f.timestamp_ns), Some(75 * 33 * 1_000_000));
        // Budget of one frame's bytes: only the newest survives.
        let one = frame_at(0).packed_bytes();
        let mut small = Ring::new(one, Duration::from_secs(60));
        for i in 0..5u64 {
            small.push(frame_at(i));
        }
        assert_eq!(small.frames(), 1);
        // A snapshot of the last 2 ms keeps the frames no older than that.
        let mut timed = Ring::new(usize::MAX, Duration::from_secs(60));
        for i in 0..10u64 {
            timed.push(frame_at(i));
        }
        assert_eq!(timed.snapshot(Some(Duration::from_millis(2))).len(), 3);
    }
}
