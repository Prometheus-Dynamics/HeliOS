use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Clone)]
struct PhotonvisionCacheEntry {
    updated_at: Instant,
    snapshot: Option<crate::nt4::photonvision::PhotonvisionNt4Snapshot>,
}

#[derive(Default)]
pub(crate) struct LocalizationPeerSourceRuntime {
    photonvision_cache: Mutex<HashMap<String, PhotonvisionCacheEntry>>,
}

impl LocalizationPeerSourceRuntime {
    pub(crate) async fn photonvision_snapshot(
        &self,
        pool: &crate::nt4::pool::Nt4ClientPool,
        host: &str,
    ) -> Option<crate::nt4::photonvision::PhotonvisionNt4Snapshot> {
        let now = Instant::now();
        {
            let cache = self.photonvision_cache.lock().await;
            if let Some(entry) = cache.get(host)
                && now.duration_since(entry.updated_at) <= photonvision_cache_ttl()
            {
                return entry.snapshot.clone();
            }
        }

        let snapshot = crate::nt4::photonvision::snapshot(pool, host, 5810, 600).await.ok();
        let mut cache = self.photonvision_cache.lock().await;
        cache.insert(host.to_string(), PhotonvisionCacheEntry { updated_at: now, snapshot: snapshot.clone() });
        snapshot
    }
}

fn photonvision_cache_ttl() -> Duration {
    Duration::from_secs(2)
}
