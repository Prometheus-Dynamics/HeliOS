use crate::error::{Error, HeartbeatTimeout, Result};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct HeartbeatConfig {
    interval: Duration,
    timeout: Duration,
}

impl HeartbeatConfig {
    pub const fn new(interval: Duration, timeout: Duration) -> Self {
        assert!(interval.as_nanos() > 0, "heartbeat interval must be non-zero");
        assert!(timeout.as_nanos() > 0, "heartbeat timeout must be non-zero");
        // Timeout should be at least the interval.
        assert!(timeout.as_nanos() >= interval.as_nanos(), "heartbeat timeout must be >= interval");
        Self { interval, timeout }
    }

    pub const fn interval(&self) -> Duration {
        self.interval
    }

    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        // Send a ping every 5 seconds and consider the transport dead after 15 seconds.
        Self { interval: Duration::from_secs(5), timeout: Duration::from_secs(15) }
    }
}

#[derive(Debug, Clone)]
pub struct HeartbeatGuard {
    cfg: HeartbeatConfig,
    last_seen: Instant,
}

impl HeartbeatGuard {
    pub fn new(cfg: HeartbeatConfig) -> Self {
        Self { cfg, last_seen: Instant::now() }
    }

    /// Mark the heartbeat as observed (e.g. after receiving a pong or successful publish).
    pub fn mark(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Returns `Ok(())` if the heartbeat is still within the timeout window, otherwise returns an error.
    pub fn ensure_alive(&self) -> Result<()> {
        let elapsed = self.last_seen.elapsed();
        if elapsed > self.cfg.timeout {
            Err(Error::HeartbeatTimeout(HeartbeatTimeout::new(elapsed, self.cfg.timeout)))
        } else {
            Ok(())
        }
    }

    pub fn config(&self) -> HeartbeatConfig {
        self.cfg
    }
}
