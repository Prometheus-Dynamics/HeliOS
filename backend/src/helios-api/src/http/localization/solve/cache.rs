use helios_engine::localization::types::LocalizationSolveResponse;
use lib_runtime_policy::HELIOS_API_LOCALIZATION_POLICY;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

#[derive(Clone)]
struct CachedLocalizationSolveResponse {
    signature: Vec<u8>,
    response: LocalizationSolveResponse,
    last_access_tick: u64,
}

#[derive(Default)]
pub(crate) struct LocalizationSolveCacheState {
    entries: RwLock<HashMap<String, CachedLocalizationSolveResponse>>,
    hits: AtomicU64,
    misses: AtomicU64,
    inserts: AtomicU64,
    evictions: AtomicU64,
    access_clock: AtomicU64,
}

impl LocalizationSolveCacheState {
    pub(crate) async fn get_matching(&self, key: &str, signature: &[u8]) -> Option<LocalizationSolveResponse> {
        let access_tick = self.next_access_tick();
        let response = self.entries.write().await.get_mut(key).filter(|entry| entry.signature == signature).map(|entry| {
            entry.last_access_tick = access_tick;
            entry.response.clone()
        });
        if response.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        response
    }

    pub(crate) async fn insert(&self, key: String, signature: Vec<u8>, response: LocalizationSolveResponse) {
        let access_tick = self.next_access_tick();
        let mut entries = self.entries.write().await;
        if !entries.contains_key(&key)
            && entries.len() >= localization_solve_cache_entry_limit()
            && let Some(evict_key) = entries.iter().min_by_key(|(_, entry)| entry.last_access_tick).map(|(key, _)| key.clone())
        {
            entries.remove(&evict_key);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
        entries.insert(key, CachedLocalizationSolveResponse { signature, response, last_access_tick: access_tick });
        self.inserts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) async fn snapshot(&self) -> crate::api_observability::LocalizationSolveCacheSnapshot {
        crate::api_observability::LocalizationSolveCacheSnapshot {
            entries: self.entries.read().await.len() as u64,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            inserts: self.inserts.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }

    fn next_access_tick(&self) -> u64 {
        self.access_clock.fetch_add(1, Ordering::Relaxed) + 1
    }
}

pub(super) fn localization_solve_cache_entry_limit() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| HELIOS_API_LOCALIZATION_POLICY.resolve().solve_cache_entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> LocalizationSolveResponse {
        LocalizationSolveResponse { profile_id: "profile".into(), solvers: Vec::new(), sources: Vec::new(), timings: Default::default() }
    }

    #[tokio::test]
    async fn localization_solve_cache_evicts_oldest_entry_when_capacity_is_exceeded() {
        let cache = LocalizationSolveCacheState::default();
        for idx in 0..(localization_solve_cache_entry_limit() + 1) {
            cache.insert(format!("profile-{idx}"), vec![idx as u8], response()).await;
        }

        let snapshot = cache.snapshot().await;
        assert_eq!(snapshot.entries as usize, localization_solve_cache_entry_limit());
        assert_eq!(snapshot.evictions, 1);
        assert!(cache.get_matching("profile-0", &[0]).await.is_none());
    }
}
