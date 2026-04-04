use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Nt4BridgeObservabilityState {
    active_target: Option<String>,
    last_entry_id: Option<u64>,
    last_publish_success_ms: Option<u64>,
}

#[derive(Default)]
struct Nt4BridgeObservability {
    publish_cycles: AtomicU64,
    publish_failures: AtomicU64,
    reconnects: AtomicU64,
    skipped_ticks: AtomicU64,
}

fn bridge_observability() -> &'static Nt4BridgeObservability {
    static OBSERVABILITY: OnceLock<Nt4BridgeObservability> = OnceLock::new();
    OBSERVABILITY.get_or_init(Nt4BridgeObservability::default)
}

fn bridge_observability_state() -> &'static Mutex<Nt4BridgeObservabilityState> {
    static STATE: OnceLock<Mutex<Nt4BridgeObservabilityState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(Nt4BridgeObservabilityState::default()))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64).unwrap_or(0)
}

pub fn record_bridge_skip() {
    bridge_observability().skipped_ticks.fetch_add(1, Ordering::Relaxed);
    let mut state = bridge_observability_state().lock().expect("nt4 bridge observability mutex poisoned");
    state.active_target = None;
    state.last_entry_id = None;
}

pub fn record_bridge_reconnect(target: Option<String>) {
    bridge_observability().reconnects.fetch_add(1, Ordering::Relaxed);
    let mut state = bridge_observability_state().lock().expect("nt4 bridge observability mutex poisoned");
    state.active_target = target;
    state.last_entry_id = None;
}

pub fn record_bridge_publish_success(target: String, entry_id: u64) {
    bridge_observability().publish_cycles.fetch_add(1, Ordering::Relaxed);
    let mut state = bridge_observability_state().lock().expect("nt4 bridge observability mutex poisoned");
    state.active_target = Some(target);
    state.last_entry_id = Some(entry_id);
    state.last_publish_success_ms = Some(now_ms());
}

pub fn record_bridge_publish_failure(target: String, entry_id: Option<u64>) {
    bridge_observability().publish_failures.fetch_add(1, Ordering::Relaxed);
    let mut state = bridge_observability_state().lock().expect("nt4 bridge observability mutex poisoned");
    state.active_target = Some(target);
    state.last_entry_id = entry_id;
}

pub fn snapshot() -> crate::api_observability::Nt4BridgeObservabilitySnapshot {
    let counters = bridge_observability();
    let state = bridge_observability_state().lock().expect("nt4 bridge observability mutex poisoned");
    crate::api_observability::Nt4BridgeObservabilitySnapshot {
        publish_cycles: counters.publish_cycles.load(Ordering::Relaxed),
        publish_failures: counters.publish_failures.load(Ordering::Relaxed),
        reconnects: counters.reconnects.load(Ordering::Relaxed),
        skipped_ticks: counters.skipped_ticks.load(Ordering::Relaxed),
        active_target: state.active_target.clone(),
        last_entry_id: state.last_entry_id,
        last_publish_success_ms: state.last_publish_success_ms,
    }
}
