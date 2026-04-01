use tracing::warn;

use crate::api_observability::ApiCacheMetric;
use crate::http::device::metrics::DeviceMetrics;

use super::collector::collect_device_metrics;
use super::config::{metrics_cache_ttl, metrics_refresh_timeout};
use super::freshness::{ReadModelRefreshOutcome, ReadModelSnapshot};
use super::state::SystemReadModelState;

impl SystemReadModelState {
    pub fn device_metrics_cache_metrics(&self) -> ApiCacheMetric {
        self.metrics_cache.metrics()
    }

    pub async fn load_device_metrics_snapshot(&self) -> ReadModelSnapshot<DeviceMetrics> {
        let collector = self.metrics_collector.clone();
        self.metrics_cache
            .load_snapshot(metrics_cache_ttl(), move || async move {
                match tokio::time::timeout(metrics_refresh_timeout(), tokio::task::spawn_blocking(move || collect_device_metrics(&collector))).await {
                    Ok(Ok(body)) => ReadModelRefreshOutcome::fresh(body),
                    Ok(Err(err)) => {
                        warn!(error = ?err, "device metrics task failed");
                        ReadModelRefreshOutcome::refresh_error()
                    }
                    Err(_) => {
                        warn!(timeout_ms = metrics_refresh_timeout().as_millis(), "device metrics task timed out");
                        ReadModelRefreshOutcome::refresh_timeout()
                    }
                }
            })
            .await
    }
}
