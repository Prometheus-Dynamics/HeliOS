use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;

use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, Default)]
pub struct EngineConnectionObservabilitySnapshot {
    pub connected: bool,
    pub last_disconnect_ms: Option<u64>,
    pub request_queue_capacity: usize,
    pub request_queue_depth: usize,
    pub request_queue_high_water: usize,
    pub pending_requests: usize,
    pub pending_requests_high_water: usize,
    pub connect_count: u64,
    pub disconnect_count: u64,
    pub request_send_timeouts: u64,
    pub request_send_failures: u64,
    pub request_timeouts: u64,
    pub disconnected_pending_requests: u64,
    pub completed_roundtrips: u64,
    pub roundtrip_total_ms: u64,
    pub roundtrip_max_ms: u64,
    pub unsolicited_events: u64,
    pub stale_response_drops: u64,
    pub no_subscriber_event_drops: u64,
    pub event_subscribers: u64,
    pub connect_event_subscribers: u64,
    pub active_streams: usize,
    pub timeout_scale_ppm: u64,
}

#[derive(Debug)]
pub(crate) struct EngineConnectionMetrics {
    pub(crate) request_queue_capacity: usize,
    pub(crate) request_queue_depth: AtomicUsize,
    pub(crate) request_queue_high_water: AtomicUsize,
    pub(crate) pending_requests: AtomicUsize,
    pub(crate) pending_requests_high_water: AtomicUsize,
    pub(crate) connect_count: AtomicU64,
    pub(crate) disconnect_count: AtomicU64,
    pub(crate) request_send_timeouts: AtomicU64,
    pub(crate) request_send_failures: AtomicU64,
    pub(crate) request_timeouts: AtomicU64,
    pub(crate) disconnected_pending_requests: AtomicU64,
    pub(crate) completed_roundtrips: AtomicU64,
    pub(crate) roundtrip_total_ms: AtomicU64,
    pub(crate) roundtrip_max_ms: AtomicU64,
    pub(crate) unsolicited_events: AtomicU64,
    pub(crate) stale_response_drops: AtomicU64,
    pub(crate) no_subscriber_event_drops: AtomicU64,
}

impl EngineConnectionMetrics {
    pub(crate) fn new(request_queue_capacity: usize) -> Self {
        Self {
            request_queue_capacity,
            request_queue_depth: AtomicUsize::new(0),
            request_queue_high_water: AtomicUsize::new(0),
            pending_requests: AtomicUsize::new(0),
            pending_requests_high_water: AtomicUsize::new(0),
            connect_count: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            request_send_timeouts: AtomicU64::new(0),
            request_send_failures: AtomicU64::new(0),
            request_timeouts: AtomicU64::new(0),
            disconnected_pending_requests: AtomicU64::new(0),
            completed_roundtrips: AtomicU64::new(0),
            roundtrip_total_ms: AtomicU64::new(0),
            roundtrip_max_ms: AtomicU64::new(0),
            unsolicited_events: AtomicU64::new(0),
            stale_response_drops: AtomicU64::new(0),
            no_subscriber_event_drops: AtomicU64::new(0),
        }
    }

    pub(crate) fn record_queue_enqueue(&self) {
        let depth = self.request_queue_depth.fetch_add(1, Ordering::Relaxed) + 1;
        update_high_water(&self.request_queue_high_water, depth);
    }

    pub(crate) fn record_queue_dequeue(&self) {
        let _ = self.request_queue_depth.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| Some(current.saturating_sub(1)));
    }

    pub(crate) fn record_queue_timeout(&self) {
        self.request_send_timeouts.fetch_add(1, Ordering::Relaxed);
        update_high_water(&self.request_queue_high_water, self.request_queue_capacity);
    }

    pub(crate) fn record_queue_send_failure(&self) {
        self.request_send_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_pending_len(&self, len: usize) {
        self.pending_requests.store(len, Ordering::Relaxed);
        update_high_water(&self.pending_requests_high_water, len);
    }

    pub(crate) fn record_connect(&self) {
        self.connect_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_disconnect(&self, dropped_pending: usize) {
        self.disconnect_count.fetch_add(1, Ordering::Relaxed);
        self.disconnected_pending_requests.fetch_add(dropped_pending as u64, Ordering::Relaxed);
        self.set_pending_len(0);
    }

    pub(crate) fn record_request_timeout(&self, count: usize) {
        self.request_timeouts.fetch_add(count as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_roundtrip(&self, elapsed: Duration) {
        let elapsed_ms = elapsed.as_millis().min(u64::MAX as u128) as u64;
        self.completed_roundtrips.fetch_add(1, Ordering::Relaxed);
        self.roundtrip_total_ms.fetch_add(elapsed_ms, Ordering::Relaxed);
        let mut current = self.roundtrip_max_ms.load(Ordering::Relaxed);
        while elapsed_ms > current {
            match self.roundtrip_max_ms.compare_exchange(current, elapsed_ms, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn record_unsolicited_event(&self) {
        self.unsolicited_events.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_stale_response_drop(&self) {
        self.stale_response_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_no_subscriber_event_drop(&self) {
        self.no_subscriber_event_drops.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn snapshot(
    connected: bool,
    last_disconnect_ms: u64,
    request_queue_capacity: usize,
    events: &broadcast::Sender<helios_engine::ipc::EngineEvent>,
    connect_events: &broadcast::Sender<()>,
    active_streams: usize,
    timeout_scale_ppm: u64,
    metrics: &Arc<EngineConnectionMetrics>,
) -> EngineConnectionObservabilitySnapshot {
    EngineConnectionObservabilitySnapshot {
        connected,
        last_disconnect_ms: (last_disconnect_ms != 0).then_some(last_disconnect_ms),
        request_queue_capacity,
        request_queue_depth: metrics.request_queue_depth.load(Ordering::Relaxed),
        request_queue_high_water: metrics.request_queue_high_water.load(Ordering::Relaxed),
        pending_requests: metrics.pending_requests.load(Ordering::Relaxed),
        pending_requests_high_water: metrics.pending_requests_high_water.load(Ordering::Relaxed),
        connect_count: metrics.connect_count.load(Ordering::Relaxed),
        disconnect_count: metrics.disconnect_count.load(Ordering::Relaxed),
        request_send_timeouts: metrics.request_send_timeouts.load(Ordering::Relaxed),
        request_send_failures: metrics.request_send_failures.load(Ordering::Relaxed),
        request_timeouts: metrics.request_timeouts.load(Ordering::Relaxed),
        disconnected_pending_requests: metrics.disconnected_pending_requests.load(Ordering::Relaxed),
        completed_roundtrips: metrics.completed_roundtrips.load(Ordering::Relaxed),
        roundtrip_total_ms: metrics.roundtrip_total_ms.load(Ordering::Relaxed),
        roundtrip_max_ms: metrics.roundtrip_max_ms.load(Ordering::Relaxed),
        unsolicited_events: metrics.unsolicited_events.load(Ordering::Relaxed),
        stale_response_drops: metrics.stale_response_drops.load(Ordering::Relaxed),
        no_subscriber_event_drops: metrics.no_subscriber_event_drops.load(Ordering::Relaxed),
        event_subscribers: events.receiver_count() as u64,
        connect_event_subscribers: connect_events.receiver_count() as u64,
        active_streams,
        timeout_scale_ppm,
    }
}

fn update_high_water(counter: &AtomicUsize, value: usize) {
    let mut current = counter.load(Ordering::Relaxed);
    while value > current {
        match counter.compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}
