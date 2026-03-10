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
