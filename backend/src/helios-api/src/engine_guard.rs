use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lib_runtime_policy::HELIOS_ENGINE_CRASH_GUARD_POLICY;
use tokio::time::sleep;
use tracing::warn;

use crate::ipc::IpcHandles;

#[derive(Debug, Clone, Copy)]
struct GuardConfig {
    window_ms: u64,
    threshold: usize,
    suppress_ms: u64,
    min_downtime_ms: u64,
    poll_ms: u64,
}

impl GuardConfig {
    fn from_policy() -> Self {
        let resolved = HELIOS_ENGINE_CRASH_GUARD_POLICY.resolve();
        Self { window_ms: resolved.window_ms, threshold: resolved.threshold, suppress_ms: resolved.suppress_ms, min_downtime_ms: resolved.min_downtime_ms, poll_ms: resolved.poll_ms }
    }
}

#[derive(Debug, Default)]
struct GuardState {
    recent: VecDeque<u64>,
    safe_mode_until_ms: u64,
}

#[derive(Debug)]
pub(crate) struct EngineCrashGuardService {
    config: GuardConfig,
    state: Mutex<GuardState>,
}

impl Default for EngineCrashGuardService {
    fn default() -> Self {
        Self { config: GuardConfig::from_policy(), state: Mutex::new(GuardState::default()) }
    }
}

impl EngineCrashGuardService {
    pub fn spawn_task(self: Arc<Self>, handles: Arc<IpcHandles>) {
        let poll_ms = self.config.poll_ms;
        let min_downtime_ms = self.config.min_downtime_ms;
        tokio::spawn(async move {
            let mut last_seen = 0u64;
            let mut pending_disconnect: Option<u64> = None;
            let mut ever_connected = handles.engine.is_connected();
            let mut connect_events = handles.engine.subscribe_connect_events();
            loop {
                tokio::select! {
                    _ = sleep(Duration::from_millis(poll_ms)) => {}
                    _ = connect_events.recv() => {
                        ever_connected = true;
                        if let Some(ts) = pending_disconnect.take() {
                            let now = now_ms();
                            let downtime = now.saturating_sub(ts);
                            if downtime >= min_downtime_ms {
                                self.record_disconnect(ts);
                            }
                        }
                    }
                }

                if !ever_connected && handles.engine.is_connected() {
                    ever_connected = true;
                }
                let Some(ts) = handles.engine.last_disconnect_ms() else {
                    continue;
                };
                if ts <= last_seen {
                    continue;
                }
                last_seen = ts;
                if !ever_connected {
                    continue;
                }
                pending_disconnect = Some(ts);
            }
        });
    }

    pub fn safe_mode_active(&self) -> bool {
        let now = now_ms();
        let guard = self.state.lock().expect("engine crash guard lock");
        guard.safe_mode_until_ms > now
    }

    fn record_disconnect(&self, ts_ms: u64) {
        let now = now_ms();
        let mut guard = self.state.lock().expect("engine crash guard lock");
        let window_start = ts_ms.saturating_sub(self.config.window_ms);
        while guard.recent.front().copied().is_some_and(|value| value < window_start) {
            guard.recent.pop_front();
        }
        guard.recent.push_back(ts_ms);

        if guard.recent.len() >= self.config.threshold && guard.safe_mode_until_ms <= now {
            guard.safe_mode_until_ms = ts_ms.saturating_add(self.config.suppress_ms);
            warn!(
                disconnects = guard.recent.len(),
                window_ms = self.config.window_ms,
                suppress_ms = self.config.suppress_ms,
                "engine crash guard active: suppressing auto-restore to keep UI responsive"
            );
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
