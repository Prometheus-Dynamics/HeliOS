use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::RwLock;

use crate::api_observability::CacheMetricCounters;

use super::collector::{MetricsCacheEntry, SystemCollector};
use super::log_sources::LogSourcesCacheEntry;
use super::realtime::{DevicesUpdatesHub, ProcessesHub, TelemetryHub};
use super::streams::{StreamMetricsHub, StreamOutputsHub};

pub struct SystemReadModelState {
    pub(super) processes_hub: ProcessesHub,
    pub(super) devices_updates_hub: DevicesUpdatesHub,
    pub(super) telemetry_hub: TelemetryHub,
    pub(super) stream_metrics_hub: StreamMetricsHub,
    pub(super) stream_outputs_hub: StreamOutputsHub,
    pub(super) metrics_cache: RwLock<Option<MetricsCacheEntry>>,
    pub(super) metrics_refresh_lock: tokio::sync::Mutex<()>,
    pub(super) metrics_stats: CacheMetricCounters,
    pub(super) telemetry_collector: Arc<StdMutex<SystemCollector>>,
    pub(super) metrics_collector: Arc<StdMutex<SystemCollector>>,
    pub(super) log_sources_cache: RwLock<Option<LogSourcesCacheEntry>>,
    pub(super) log_sources_refresh_lock: tokio::sync::Mutex<()>,
    pub(super) log_sources_stats: CacheMetricCounters,
}

impl Default for SystemReadModelState {
    fn default() -> Self {
        Self {
            processes_hub: ProcessesHub::new(),
            devices_updates_hub: DevicesUpdatesHub::new(),
            telemetry_hub: TelemetryHub::new(),
            stream_metrics_hub: StreamMetricsHub::new(),
            stream_outputs_hub: StreamOutputsHub::new(),
            metrics_cache: RwLock::new(None),
            metrics_refresh_lock: tokio::sync::Mutex::new(()),
            metrics_stats: CacheMetricCounters::default(),
            telemetry_collector: Arc::new(StdMutex::new(SystemCollector::new())),
            metrics_collector: Arc::new(StdMutex::new(SystemCollector::new())),
            log_sources_cache: RwLock::new(None),
            log_sources_refresh_lock: tokio::sync::Mutex::new(()),
            log_sources_stats: CacheMetricCounters::default(),
        }
    }
}
