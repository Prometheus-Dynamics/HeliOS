//! Per-workload frame statistics published with frame-driven sessions.

use std::time::{Duration, Instant};

use super::frame_source::FrameSourceStatus;

/// Window over which the frame rate is measured.
const FPS_WINDOW: Duration = Duration::from_secs(1);
/// With no frame for this long the reported rate drops to zero.
const FPS_STALE_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FrameDriverStats {
    pub frames_received: u64,
    pub frames_processed: u64,
    pub frames_failed: u64,
    pub last_tick_ms: f64,
    pub fps: f64,
    pub last_frame_timestamp: Option<u64>,
    pub last_frame_size: Option<(u32, u32)>,
    pub source: FrameSourceStatus,
    pub last_error: Option<String>,
    /// Wall-clock time of the last change, used as the published records' observed time so
    /// unchanged stats do not churn Orion state.
    pub updated_at_ms: u64,
}

impl FrameDriverStats {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "frames_received": self.frames_received,
            "frames_processed": self.frames_processed,
            "frames_failed": self.frames_failed,
            "last_tick_ms": round2(self.last_tick_ms),
            "fps": round2(self.fps),
            "last_frame_timestamp": self.last_frame_timestamp,
            "last_frame_width": self.last_frame_size.map(|(width, _)| width),
            "last_frame_height": self.last_frame_size.map(|(_, height)| height),
            "source_connected": self.source.connected,
            "source_reconnects": self.source.reconnects,
            "source_plan": self.source.plan,
            "last_error": self.last_error,
        })
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Frames per second over the last full window.
#[derive(Debug)]
pub(crate) struct FpsWindow {
    window_start: Instant,
    frames_in_window: u32,
    last_frame_at: Option<Instant>,
    fps: f64,
}

impl Default for FpsWindow {
    fn default() -> Self {
        Self { window_start: Instant::now(), frames_in_window: 0, last_frame_at: None, fps: 0.0 }
    }
}

impl FpsWindow {
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        if self.last_frame_at.is_none_or(|last| now.duration_since(last) >= FPS_STALE_AFTER) {
            self.window_start = now;
            self.frames_in_window = 0;
        }
        self.frames_in_window += 1;
        self.last_frame_at = Some(now);
        let elapsed = now.duration_since(self.window_start);
        if elapsed >= FPS_WINDOW {
            self.fps = f64::from(self.frames_in_window) / elapsed.as_secs_f64();
            self.window_start = now;
            self.frames_in_window = 0;
        }
    }

    /// The rate while no frame arrives: unchanged until frames are stale, then zero.
    pub fn idle(&mut self) -> f64 {
        if self.last_frame_at.is_none_or(|last| last.elapsed() >= FPS_STALE_AFTER) {
            self.fps = 0.0;
            self.window_start = Instant::now();
            self.frames_in_window = 0;
        }
        self.fps
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }
}
