use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ApiCacheMetric {
    pub hits: u64,
    pub misses: u64,
    pub refreshes: u64,
    pub stale_fallbacks: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ApiRealtimeMetrics {
    pub telemetry_subscribers: u64,
    pub process_subscribers: u64,
    pub device_update_subscribers: u64,
    pub stream_metrics_topics: u64,
    pub stream_metrics_subscribers: u64,
    pub stream_outputs_topics: u64,
    pub stream_outputs_subscribers: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct RuntimeBroadcastSnapshot {
    pub subscribers: u64,
    pub events_sent: u64,
    pub lagged_event_drops: u64,
    pub no_receiver_drops: u64,
    pub idle_shutdowns: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct RuntimeTopicBroadcastSnapshot {
    pub topics: u64,
    pub subscribers: u64,
    pub events_sent: u64,
    pub lagged_event_drops: u64,
    pub no_receiver_drops: u64,
    pub idle_shutdowns: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ApiRealtimeDiagnostics {
    pub telemetry: RuntimeBroadcastSnapshot,
    pub processes: RuntimeBroadcastSnapshot,
    pub device_updates: RuntimeBroadcastSnapshot,
    pub stream_metrics: RuntimeTopicBroadcastSnapshot,
    pub stream_outputs: RuntimeTopicBroadcastSnapshot,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct LocalizationSolveCacheSnapshot {
    pub entries: u64,
    pub hits: u64,
    pub misses: u64,
    pub inserts: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct LocalizationSampleRefreshSnapshot {
    pub tracked_outputs: u64,
    pub refresh_grants: u64,
    pub throttled_requests: u64,
    pub pruned_entries: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct Nt4PoolObservabilitySnapshot {
    pub client_slots: u64,
    pub connected_clients: u64,
    pub connect_attempts: u64,
    pub connect_reuses: u64,
    pub connect_successes: u64,
    pub connect_failures: u64,
    pub disconnects: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct Nt4BridgeObservabilitySnapshot {
    pub publish_cycles: u64,
    pub publish_failures: u64,
    pub reconnects: u64,
    pub skipped_ticks: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_entry_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_publish_success_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ApiMediaCacheMetrics {
    pub imu_event_entries: u64,
    pub frame_timeline_entries: u64,
    pub playback_duration_entries: u64,
    pub preview_lock_entries: u64,
    pub thumbnail_lock_entries: u64,
    pub imu_cursor_entries: u64,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct ApiRuntimeMetrics {
    pub system_metrics_cache: ApiCacheMetric,
    pub log_sources_cache: ApiCacheMetric,
    pub pipelines_graphs_cache: ApiCacheMetric,
    pub pipelines_registry_cache: ApiCacheMetric,
    pub streams_list_cache: ApiCacheMetric,
    pub peripherals_inventory_cache: ApiCacheMetric,
    pub camera_discovery_cache: ApiCacheMetric,
    pub realtime: ApiRealtimeMetrics,
    pub media: ApiMediaCacheMetrics,
}

#[derive(Debug, Default)]
pub struct CacheMetricCounters {
    hits: AtomicU64,
    misses: AtomicU64,
    refreshes: AtomicU64,
    stale_fallbacks: AtomicU64,
    revision: AtomicU64,
}

#[derive(Debug, Default)]
pub struct RuntimeBroadcastCounters {
    events_sent: AtomicU64,
    lagged_event_drops: AtomicU64,
    no_receiver_drops: AtomicU64,
    idle_shutdowns: AtomicU64,
}

impl CacheMetricCounters {
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_stale_fallback(&self) {
        self.stale_fallbacks.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_refresh(&self) -> u64 {
        self.refreshes.fetch_add(1, Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn snapshot(&self) -> ApiCacheMetric {
        ApiCacheMetric {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            refreshes: self.refreshes.load(Ordering::Relaxed),
            stale_fallbacks: self.stale_fallbacks.load(Ordering::Relaxed),
            revision: self.revision.load(Ordering::Relaxed),
        }
    }
}

impl RuntimeBroadcastCounters {
    pub fn record_sent(&self) {
        self.events_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_lagged(&self, skipped: u64) {
        self.lagged_event_drops.fetch_add(skipped, Ordering::Relaxed);
    }

    pub fn record_no_receiver_drop(&self) {
        self.no_receiver_drops.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_idle_shutdown(&self) {
        self.idle_shutdowns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self, subscribers: u64) -> RuntimeBroadcastSnapshot {
        RuntimeBroadcastSnapshot {
            subscribers,
            events_sent: self.events_sent.load(Ordering::Relaxed),
            lagged_event_drops: self.lagged_event_drops.load(Ordering::Relaxed),
            no_receiver_drops: self.no_receiver_drops.load(Ordering::Relaxed),
            idle_shutdowns: self.idle_shutdowns.load(Ordering::Relaxed),
        }
    }

    pub fn snapshot_topics(&self, topics: u64, subscribers: u64) -> RuntimeTopicBroadcastSnapshot {
        RuntimeTopicBroadcastSnapshot {
            topics,
            subscribers,
            events_sent: self.events_sent.load(Ordering::Relaxed),
            lagged_event_drops: self.lagged_event_drops.load(Ordering::Relaxed),
            no_receiver_drops: self.no_receiver_drops.load(Ordering::Relaxed),
            idle_shutdowns: self.idle_shutdowns.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeBroadcastCounters;

    #[test]
    fn runtime_broadcast_counters_track_drops_and_idle_shutdowns() {
        let counters = RuntimeBroadcastCounters::default();
        counters.record_sent();
        counters.record_lagged(7);
        counters.record_no_receiver_drop();
        counters.record_idle_shutdown();

        let snapshot = counters.snapshot(3);
        assert_eq!(snapshot.subscribers, 3);
        assert_eq!(snapshot.events_sent, 1);
        assert_eq!(snapshot.lagged_event_drops, 7);
        assert_eq!(snapshot.no_receiver_drops, 1);
        assert_eq!(snapshot.idle_shutdowns, 1);
    }
}
