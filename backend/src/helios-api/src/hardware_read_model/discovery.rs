use crate::http::AppState;
use helios_engine::capture::DiscoveryResult;
use std::time::{Duration, Instant};

use super::{HardwareReadModelState, camera_discovery_timeout};

pub(super) struct CachedDiscovery {
    pub(super) result: DiscoveryResult,
    pub(super) at: Instant,
    pub(super) revision: u64,
}

impl HardwareReadModelState {
    pub async fn cached_discover_cameras_snapshot(&self, state: &AppState) -> Result<(DiscoveryResult, u64), String> {
        const TTL: Duration = Duration::from_secs(5);

        {
            let guard = self.camera_cache.lock().await;
            if let Some(cached) = guard.as_ref()
                && cached.at.elapsed() < TTL
            {
                self.camera_discovery_stats.record_hit();
                return Ok((cached.result.clone(), cached.revision));
            }
        }

        self.camera_discovery_stats.record_miss();
        let _refresh_guard = self.camera_refresh_lock.lock().await;
        {
            let guard = self.camera_cache.lock().await;
            if let Some(cached) = guard.as_ref()
                && cached.at.elapsed() < TTL
            {
                self.camera_discovery_stats.record_hit();
                return Ok((cached.result.clone(), cached.revision));
            }
        }

        let discovery = match tokio::time::timeout(camera_discovery_timeout(), state.engine.discover_devices()).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(err)) => Err(err.to_string()),
            Err(_) => Err("camera discovery timed out".to_string()),
        };

        match discovery {
            Ok(result) => {
                let revision = self.camera_discovery_stats.record_refresh();
                let mut guard = self.camera_cache.lock().await;
                *guard = Some(CachedDiscovery { result: result.clone(), at: Instant::now(), revision });
                Ok((result, revision))
            }
            Err(err) => {
                let mut fallback = {
                    let guard = self.camera_cache.lock().await;
                    guard.as_ref().map(|entry| entry.result.clone()).unwrap_or_else(|| DiscoveryResult { devices: Vec::new(), errors: Vec::new() })
                };
                if fallback.errors.iter().all(|entry| entry != &err) {
                    fallback.errors.push(err);
                    if fallback.errors.len() > 5 {
                        let drain = fallback.errors.len() - 5;
                        fallback.errors.drain(0..drain);
                    }
                }
                let revision = self.camera_discovery_stats.record_refresh();
                let mut guard = self.camera_cache.lock().await;
                *guard = Some(CachedDiscovery { result: fallback.clone(), at: Instant::now(), revision });
                Ok((fallback, revision))
            }
        }
    }
}
