use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::{Duration as StdDuration, Instant as StdInstant};

use lib_runtime_policy::HELIOS_API_SYSTEM_READ_MODEL_POLICY;
use tokio::sync::{broadcast, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::ipc::IpcHandles;
use crate::ws::device::{EngineTelemetry, PowerTelemetry};

use super::super::collector::SystemCollector;
use super::super::config::{devices_updates_stream_poll_interval, spawn_api_sampler_thread};
use super::super::hardware::sample_power_from_peripherals;
use super::models::{DevicesUpdateReason, SharedDevicesUpdate, SharedProcessesSnapshot};
use super::reducers::{broadcast_devices_update, build_shared_process_snapshot, reason_for_update_kind, stream_fingerprint, usb_fingerprint};

pub(super) async fn run_devices_updates_sampler(tx: broadcast::Sender<Arc<SharedDevicesUpdate>>, state: Option<Weak<IpcHandles>>) {
    const MIN_UPDATE_GAP_MS: u64 = 250;

    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let mut updates = state.subscribe_realtime_updates();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));

    let mut last_usb = usb_fingerprint();
    let mut last_streams = stream_fingerprint(&state).await.unwrap_or_default();
    let streams_poll_interval = devices_updates_stream_poll_interval();
    let mut last_streams_poll = Instant::now();
    let mut pending: std::collections::BTreeSet<DevicesUpdateReason> = std::collections::BTreeSet::new();
    let min_gap = Duration::from_millis(MIN_UPDATE_GAP_MS);
    let mut last_sent = Instant::now().checked_sub(min_gap).unwrap_or_else(Instant::now);
    let mut saw_receiver = tx.receiver_count() > 0;

    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        tokio::select! {
            _ = ticker.tick() => {
                let next_usb = usb_fingerprint();
                if next_usb != last_usb {
                    last_usb = next_usb;
                    pending.insert(DevicesUpdateReason::Usb);
                }

                if last_streams_poll.elapsed() >= streams_poll_interval {
                    last_streams_poll = Instant::now();
                    if let Some(next_streams) = stream_fingerprint(&state).await
                        && next_streams != last_streams
                    {
                        last_streams = next_streams;
                        pending.insert(DevicesUpdateReason::Streams);
                    }
                }

                if !pending.is_empty() && last_sent.elapsed() >= min_gap {
                    broadcast_devices_update(&tx, &pending);
                    pending.clear();
                    last_sent = Instant::now();
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        pending.insert(reason_for_update_kind(update.kind.as_str()));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        pending.insert(DevicesUpdateReason::Api);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }

                if !pending.is_empty() && last_sent.elapsed() >= min_gap {
                    broadcast_devices_update(&tx, &pending);
                    pending.clear();
                    last_sent = Instant::now();
                }
            }
        }
    }
}

pub(super) async fn run_telemetry_sampler(tx: broadcast::Sender<Arc<str>>, latest: Arc<StdMutex<Option<Arc<str>>>>, state: Option<Weak<IpcHandles>>, collector: Arc<StdMutex<SystemCollector>>) {
    let sample_interval = Duration::from_millis(HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().telemetry_sample_interval_ms);

    let last_power: Arc<StdMutex<Option<PowerTelemetry>>> = Arc::new(StdMutex::new(None));

    let power_state = state.clone();
    let power_task = {
        let tx = tx.clone();
        let last_power = last_power.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));

            let mut saw_receiver = tx.receiver_count() > 0;
            loop {
                ticker.tick().await;

                let receiver_count = tx.receiver_count();
                saw_receiver |= receiver_count > 0;
                if saw_receiver && receiver_count == 0 {
                    break;
                }

                if let Some(sampled) = sample_power_from_peripherals(power_state.as_ref()).await
                    && let Ok(mut guard) = last_power.lock()
                {
                    *guard = Some(sampled);
                }
            }
        })
    };

    let (sys_done_tx, sys_done_rx) = oneshot::channel();
    let fallback_tx = tx.clone();
    let fallback_latest = latest.clone();
    let fallback_state = state.clone();
    let fallback_last_power = last_power.clone();
    let fallback_collector = collector.clone();
    match spawn_api_sampler_thread("helios-api-telemetry", move || {
        run_telemetry_sys_sampler(tx, latest, state, last_power, collector, sample_interval);
        let _ = sys_done_tx.send(());
    }) {
        Ok(_join) => {
            let _ = sys_done_rx.await;
        }
        Err(err) => {
            warn!(
                error = %err,
                "failed to spawn dedicated telemetry sampler thread; falling back to Tokio blocking pool"
            );
            let _ = tokio::task::spawn_blocking(move || run_telemetry_sys_sampler(fallback_tx, fallback_latest, fallback_state, fallback_last_power, fallback_collector, sample_interval)).await;
        }
    }
    power_task.abort();
}

pub(super) fn run_telemetry_sys_sampler(
    tx: broadcast::Sender<Arc<str>>,
    latest: Arc<StdMutex<Option<Arc<str>>>>,
    state: Option<Weak<IpcHandles>>,
    last_power: Arc<StdMutex<Option<PowerTelemetry>>>,
    collector: Arc<StdMutex<SystemCollector>>,
    sample_interval: Duration,
) {
    let soft_budget = (sample_interval / 5).max(Duration::from_millis(50));
    let hard_budget = sample_interval + Duration::from_millis(50);

    let mut last_warn: Option<Instant> = None;
    let mut suppressed_warns: u64 = 0;

    let mut saw_receiver = tx.receiver_count() > 0;
    let mut next_tick = Instant::now();

    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        let now = Instant::now();
        if now < next_tick {
            std::thread::sleep(next_tick - now);
        }
        next_tick = Instant::now() + sample_interval;

        let sampling_started = Instant::now();
        let mut sample = collector.lock().expect("system collector poisoned").collect_telemetry();
        let sampling_elapsed = sampling_started.elapsed();

        if let Some(state) = state.as_ref().and_then(|weak| weak.upgrade()) {
            sample.engine = Some(EngineTelemetry { connected: state.engine.is_connected(), last_disconnect_ms: state.engine.last_disconnect_ms() });
        }

        if let Ok(guard) = last_power.lock() {
            sample.power = guard.clone();
        }

        if sampling_elapsed >= soft_budget {
            let now = Instant::now();
            let should_warn = last_warn.map(|time| now.duration_since(time) >= Duration::from_secs(10)).unwrap_or(true);
            if should_warn {
                if sampling_elapsed >= hard_budget {
                    warn!(
                        elapsed_ms = sampling_elapsed.as_millis(),
                        soft_budget_ms = soft_budget.as_millis(),
                        interval_ms = sample_interval.as_millis(),
                        suppressed = suppressed_warns,
                        "telemetry sampling exceeded budget"
                    );
                } else {
                    debug!(
                        elapsed_ms = sampling_elapsed.as_millis(),
                        soft_budget_ms = soft_budget.as_millis(),
                        interval_ms = sample_interval.as_millis(),
                        suppressed = suppressed_warns,
                        "telemetry sampling exceeded soft budget"
                    );
                }
                last_warn = Some(now);
                suppressed_warns = 0;
            } else {
                suppressed_warns = suppressed_warns.saturating_add(1);
            }
        }

        let payload = Arc::<str>::from(serde_json::to_string(&sample).unwrap_or_default());
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(payload.clone());
        }
        let _ = tx.send(payload);
    }
}

pub(super) fn run_processes_sampler(tx: broadcast::Sender<Arc<SharedProcessesSnapshot>>, latest: Arc<StdMutex<Option<Arc<SharedProcessesSnapshot>>>>) {
    let sample_interval = Duration::from_millis(HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().processes_sample_interval_ms);

    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();

    let mut saw_receiver = tx.receiver_count() > 0;
    let mut next_tick = StdDuration::from_millis(0);
    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        if next_tick > StdDuration::from_millis(0) {
            std::thread::sleep(next_tick);
        }
        let sampling_started = StdInstant::now();

        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        sys.refresh_memory();

        let snapshot = Arc::new(build_shared_process_snapshot(&sys));
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(snapshot.clone());
        }
        let _ = tx.send(snapshot);

        next_tick = sample_interval.saturating_sub(sampling_started.elapsed());
    }
}
