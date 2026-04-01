use tokio::time::Instant;
use tracing::warn;

use crate::api_observability::ApiCacheMetric;
use crate::http::device::metrics::DeviceMetrics;

use super::collector::collect_device_metrics;
use super::config::{metrics_cache_ttl, metrics_refresh_timeout};
use super::state::SystemReadModelState;

impl SystemReadModelState {
    pub fn device_metrics_cache_metrics(&self) -> ApiCacheMetric {
        self.metrics_stats.snapshot()
    }

    pub async fn load_device_metrics_snapshot(&self) -> Result<(DeviceMetrics, u64), String> {
        let ttl = metrics_cache_ttl();
        if ttl != tokio::time::Duration::from_millis(0)
            && let Some(entry) = self.metrics_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.metrics_stats.record_hit();
            return Ok((entry.body, entry.revision));
        }

        self.metrics_stats.record_miss();
        let _refresh_guard = self.metrics_refresh_lock.lock().await;
        if ttl != tokio::time::Duration::from_millis(0)
            && let Some(entry) = self.metrics_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.metrics_stats.record_hit();
            return Ok((entry.body, entry.revision));
        }

        let stale = self.metrics_cache.read().await.clone();
        let collector = self.metrics_collector.clone();
        let body = match tokio::time::timeout(metrics_refresh_timeout(), tokio::task::spawn_blocking(move || collect_device_metrics(&collector))).await {
            Ok(Ok(body)) => body,
            Ok(Err(err)) => {
                warn!(error = ?err, "device metrics task failed");
                if let Some(entry) = stale {
                    self.metrics_stats.record_stale_fallback();
                    return Ok((entry.body, entry.revision));
                }
                return Err(format!("metrics task failed: {err}"));
            }
            Err(_) => {
                warn!(timeout_ms = metrics_refresh_timeout().as_millis(), "device metrics task timed out");
                if let Some(entry) = stale {
                    self.metrics_stats.record_stale_fallback();
                    return Ok((entry.body, entry.revision));
                }
                return Err("metrics task timed out".into());
            }
        };

        let revision = self.metrics_stats.record_refresh();
        *self.metrics_cache.write().await = Some(super::collector::MetricsCacheEntry { fetched_at: Instant::now(), revision, body: body.clone() });
        Ok((body, revision))
    }
}
