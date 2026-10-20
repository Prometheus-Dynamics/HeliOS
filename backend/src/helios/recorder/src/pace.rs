//! The frame rate limit of a recording: which of a camera's frames are kept.

use std::time::{Duration, Instant};

/// Admits frames at most `max_fps` a second. Each admitted frame moves the schedule on by one
/// interval (not to the arrival time), and a frame may come up to a quarter interval early: a
/// 120 fps camera with a 30 fps limit keeps 30 frames a second rather than 24 or 40.
#[derive(Debug, Clone)]
pub struct Pacer {
    interval: Option<Duration>,
    next: Option<Instant>,
}

impl Pacer {
    /// `None`: every frame is admitted.
    pub fn new(max_fps: Option<f64>) -> Self {
        Self { interval: max_fps.map(|fps| Duration::from_secs_f64(1.0 / fps)), next: None }
    }

    /// Whether the frame arriving at `now` is kept.
    pub fn admit(&mut self, now: Instant) -> bool {
        let Some(interval) = self.interval else { return true };
        let Some(next) = self.next else {
            self.next = Some(now + interval);
            return true;
        };
        if now + interval / 4 < next {
            return false;
        }
        let following = next + interval;
        // Behind by more than an interval (a stall): restart the schedule from now.
        self.next = Some(if following <= now { now + interval } else { following });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_is_admitted_without_a_limit() {
        let mut pacer = Pacer::new(None);
        let start = Instant::now();
        assert!((0..100).all(|i| pacer.admit(start + Duration::from_millis(i))));
    }

    #[test]
    fn a_120_fps_camera_keeps_30_frames_a_second_under_a_30_fps_limit() {
        let mut pacer = Pacer::new(Some(30.0));
        let start = Instant::now();
        // 120 frames, 8.33 ms apart: one second of the camera.
        let kept = (0..120).filter(|&i| pacer.admit(start + Duration::from_micros(8_333 * i))).count();
        assert!((29..=31).contains(&kept), "kept {kept}");
    }

    #[test]
    fn a_camera_slower_than_the_limit_is_not_thinned() {
        let mut pacer = Pacer::new(Some(30.0));
        let start = Instant::now();
        // 34 ms frames: about 29 fps, under the limit.
        let kept = (0..30).filter(|&i| pacer.admit(start + Duration::from_millis(34 * i))).count();
        assert_eq!(kept, 30);
    }

    #[test]
    fn after_a_stall_the_schedule_restarts_from_now() {
        let mut pacer = Pacer::new(Some(10.0));
        let start = Instant::now();
        assert!(pacer.admit(start));
        // Two seconds without frames, then a frame: admitted, and the next one 100 ms on.
        let later = start + Duration::from_secs(2);
        assert!(pacer.admit(later));
        assert!(!pacer.admit(later + Duration::from_millis(20)));
        assert!(pacer.admit(later + Duration::from_millis(100)));
    }
}
