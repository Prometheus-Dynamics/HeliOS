use std::sync::{Arc, Mutex as StdMutex};

use crate::http::device::metrics::DeviceMetrics;
use crate::logs::LogSource;

use super::collector::SystemCollector;
use super::freshness::ReadModelCache;
use super::realtime::{DevicesUpdatesHub, ProcessesHub, TelemetryHub};
use super::streams::{StreamMetricsHub, StreamOutputsHub};

pub struct SystemReadModelState {
    pub(super) processes_hub: ProcessesHub,
    pub(super) devices_updates_hub: DevicesUpdatesHub,
    pub(super) telemetry_hub: TelemetryHub,
    pub(super) stream_metrics_hub: StreamMetricsHub,
    pub(super) stream_outputs_hub: StreamOutputsHub,
    pub(super) metrics_cache: ReadModelCache<DeviceMetrics>,
    pub(super) telemetry_collector: Arc<StdMutex<SystemCollector>>,
    pub(super) metrics_collector: Arc<StdMutex<SystemCollector>>,
    pub(super) log_sources_cache: ReadModelCache<Vec<LogSource>>,
}

impl Default for SystemReadModelState {
    fn default() -> Self {
        Self {
            processes_hub: ProcessesHub::new(),
            devices_updates_hub: DevicesUpdatesHub::new(),
            telemetry_hub: TelemetryHub::new(),
            stream_metrics_hub: StreamMetricsHub::new(),
            stream_outputs_hub: StreamOutputsHub::new(),
            metrics_cache: ReadModelCache::default(),
            telemetry_collector: Arc::new(StdMutex::new(SystemCollector::new())),
            metrics_collector: Arc::new(StdMutex::new(SystemCollector::new())),
            log_sources_cache: ReadModelCache::default(),
        }
    }
}
