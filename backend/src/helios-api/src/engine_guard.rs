use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::time::sleep;
use tracing::warn;

use crate::ipc::IpcHandles;

struct GuardConfig {
    window_ms: u64,
    threshold: usize,
    suppress_ms: u64,
    min_downtime_ms: u64,
    poll_ms: u64,
}

impl GuardConfig {
    fn from_env() -> Self {
        Self {
            window_ms: env_u64("HELIOS_ENGINE_CRASH_GUARD_WINDOW_MS", 60_000).max(5_000),
            // Default threshold is intentionally >2 so a normal deploy/restart sequence does not
            // accidentally suppress stream auto-restore for minutes.
            threshold: env_usize("HELIOS_ENGINE_CRASH_GUARD_THRESHOLD", 3).clamp(1, 10),
            suppress_ms: env_u64("HELIOS_ENGINE_CRASH_GUARD_SUPPRESS_MS", 300_000).max(10_000),
            // Treat very brief disconnect/reconnect cycles (e.g. systemd restarts during deploy)
            // as non-crashes. This is measured from the engine disconnect timestamp until we
            // observe a successful reconnect.
            min_downtime_ms: env_u64("HELIOS_ENGINE_CRASH_GUARD_MIN_DOWNTIME_MS", 2_000),
            poll_ms: env_u64("HELIOS_ENGINE_CRASH_GUARD_POLL_MS", 1000).max(200),
        }
    }
}

struct GuardState {
    recent: VecDeque<u64>,
    safe_mode_until_ms: u64,
}

struct EngineCrashGuardRuntime {
    config: GuardConfig,
    state: Mutex<GuardState>,
}

fn runtime() -> &'static EngineCrashGuardRuntime {
    static RUNTIME: OnceLock<EngineCrashGuardRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| EngineCrashGuardRuntime { config: GuardConfig::from_env(), state: Mutex::new(GuardState { recent: VecDeque::new(), safe_mode_until_ms: 0 }) })
}

pub fn spawn_engine_crash_guard_task(handles: std::sync::Arc<IpcHandles>) {
    let poll_ms = runtime().config.poll_ms;
    let min_downtime_ms = runtime().config.min_downtime_ms;
    tokio::spawn(async move {
        let mut last_seen = 0u64;
        let mut pending_disconnect: Option<u64> = None;
        // Don't treat "engine not up yet" as a crash. Only start counting disconnects
        // after we've observed at least one successful connection.
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
                            record_disconnect(ts);
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
            // Don't record immediately: only count it as a "crash" if it stays down for at least
            // `min_downtime_ms` before reconnecting.
            pending_disconnect = Some(ts);
        }
    });
}

pub fn safe_mode_active() -> bool {
    let now = now_ms();
    let guard = runtime().state.lock().expect("engine crash guard lock");
    guard.safe_mode_until_ms > now
}

fn record_disconnect(ts_ms: u64) {
    let now = now_ms();
    let runtime = runtime();
    let mut guard = runtime.state.lock().expect("engine crash guard lock");
    let window_start = ts_ms.saturating_sub(runtime.config.window_ms);
    // Only trim while we actually have elements; otherwise we'd spin forever on an empty deque.
    while guard.recent.front().copied().is_some_and(|value| value < window_start) {
        guard.recent.pop_front();
    }
    guard.recent.push_back(ts_ms);

    if guard.recent.len() >= runtime.config.threshold && guard.safe_mode_until_ms <= now {
        guard.safe_mode_until_ms = ts_ms.saturating_add(runtime.config.suppress_ms);
        warn!(
            disconnects = guard.recent.len(),
            window_ms = runtime.config.window_ms,
            suppress_ms = runtime.config.suppress_ms,
            "engine crash guard active: suppressing auto-restore to keep UI responsive"
        );
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(default)
}
