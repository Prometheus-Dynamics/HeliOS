use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, Instant};
use utoipa::ToSchema;

use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadModelFreshnessState {
    Live,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadModelFreshnessReason {
    Live,
    RefreshTimeout,
    RefreshError,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ToSchema)]
pub struct ReadModelFreshness {
    pub state: ReadModelFreshnessState,
    pub reason: ReadModelFreshnessReason,
    pub observed_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReadModelSnapshot<T> {
    pub payload: Option<T>,
    pub freshness: ReadModelFreshness,
    pub revision: u64,
}

pub(super) enum ReadModelRefreshOutcome<T> {
    Fresh(T),
    Failed { reason: ReadModelFreshnessReason, fallback: Option<T> },
}

impl<T> ReadModelRefreshOutcome<T> {
    pub(super) fn fresh(payload: T) -> Self {
        Self::Fresh(payload)
    }

    pub(super) fn refresh_timeout() -> Self {
        Self::Failed { reason: ReadModelFreshnessReason::RefreshTimeout, fallback: None }
    }

    pub(super) fn refresh_error() -> Self {
        Self::Failed { reason: ReadModelFreshnessReason::RefreshError, fallback: None }
    }

    pub(super) fn stale_fallback(payload: T, reason: ReadModelFreshnessReason) -> Self {
        debug_assert_ne!(reason, ReadModelFreshnessReason::Live);
        Self::Failed { reason, fallback: Some(payload) }
    }
}

#[derive(Clone)]
struct ReadModelCacheEntry<T> {
    fetched_at: Instant,
    fetched_at_ms: u64,
    success_revision: u64,
    payload: T,
}

#[derive(Clone)]
struct PublishedSnapshot<T> {
    identity: PublishedSnapshotIdentity,
    snapshot: ReadModelSnapshot<T>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublishedSnapshotIdentity {
    Live { success_revision: u64 },
    StaleCached { success_revision: u64, reason: ReadModelFreshnessReason },
    Unavailable { reason: ReadModelFreshnessReason },
}

pub(super) struct ReadModelCache<T> {
    live_cache: RwLock<Option<ReadModelCacheEntry<T>>>,
    refresh_lock: Mutex<()>,
    published: RwLock<Option<PublishedSnapshot<T>>>,
    stats: CacheMetricCounters,
    success_revision: AtomicU64,
    public_revision: AtomicU64,
}

impl<T> Default for ReadModelCache<T> {
    fn default() -> Self {
        Self {
            live_cache: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            published: RwLock::new(None),
            stats: CacheMetricCounters::default(),
            success_revision: AtomicU64::new(0),
            public_revision: AtomicU64::new(0),
        }
    }
}

impl<T: Clone> ReadModelCache<T> {
    pub(super) fn metrics(&self) -> ApiCacheMetric {
        self.stats.snapshot()
    }

    pub(super) async fn load_snapshot<F, Fut>(&self, ttl: Duration, refresh: F) -> ReadModelSnapshot<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ReadModelRefreshOutcome<T>>,
    {
        if let Some(entry) = self.live_entry(ttl).await {
            self.stats.record_hit();
            return self.publish_live(entry).await;
        }

        self.stats.record_miss();
        let _refresh_guard = self.refresh_lock.lock().await;
        if let Some(entry) = self.live_entry(ttl).await {
            self.stats.record_hit();
            return self.publish_live(entry).await;
        }

        let stale = self.live_cache.read().await.clone();
        match refresh().await {
            ReadModelRefreshOutcome::Fresh(payload) => self.store_fresh(payload).await,
            ReadModelRefreshOutcome::Failed { reason, fallback } => {
                if let Some(entry) = stale {
                    self.stats.record_stale_fallback();
                    return self.publish_stale_cached(entry, reason).await;
                }
                if let Some(payload) = fallback {
                    return self.publish_stale_fallback(payload, reason).await;
                }
                self.publish_unavailable(reason).await
            }
        }
    }

    async fn live_entry(&self, ttl: Duration) -> Option<ReadModelCacheEntry<T>> {
        if ttl == Duration::from_millis(0) {
            return None;
        }
        let entry = self.live_cache.read().await.clone()?;
        if entry.fetched_at.elapsed() < ttl { Some(entry) } else { None }
    }

    async fn store_fresh(&self, payload: T) -> ReadModelSnapshot<T> {
        let fetched_at = Instant::now();
        let fetched_at_ms = now_ms();
        let success_revision = self.success_revision.fetch_add(1, Ordering::Relaxed) + 1;
        let entry = ReadModelCacheEntry { fetched_at, fetched_at_ms, success_revision, payload: payload.clone() };
        let _ = self.stats.record_refresh();
        *self.live_cache.write().await = Some(entry.clone());
        self.publish_live(entry).await
    }

    async fn publish_live(&self, entry: ReadModelCacheEntry<T>) -> ReadModelSnapshot<T> {
        let identity = PublishedSnapshotIdentity::Live { success_revision: entry.success_revision };
        if let Some(snapshot) = self.reuse_published(identity).await {
            return snapshot;
        }

        self.publish_snapshot(
            Some(entry.payload),
            ReadModelFreshness { state: ReadModelFreshnessState::Live, reason: ReadModelFreshnessReason::Live, observed_at_ms: entry.fetched_at_ms, last_success_at_ms: Some(entry.fetched_at_ms) },
            Some(identity),
        )
        .await
    }

    async fn publish_stale_cached(&self, entry: ReadModelCacheEntry<T>, reason: ReadModelFreshnessReason) -> ReadModelSnapshot<T> {
        let identity = PublishedSnapshotIdentity::StaleCached { success_revision: entry.success_revision, reason };
        if let Some(snapshot) = self.reuse_published(identity).await {
            return snapshot;
        }

        self.publish_snapshot(
            Some(entry.payload),
            ReadModelFreshness { state: ReadModelFreshnessState::Stale, reason, observed_at_ms: now_ms(), last_success_at_ms: Some(entry.fetched_at_ms) },
            Some(identity),
        )
        .await
    }

    async fn publish_stale_fallback(&self, payload: T, reason: ReadModelFreshnessReason) -> ReadModelSnapshot<T> {
        self.publish_snapshot(Some(payload), ReadModelFreshness { state: ReadModelFreshnessState::Stale, reason, observed_at_ms: now_ms(), last_success_at_ms: None }, None).await
    }

    async fn publish_unavailable(&self, reason: ReadModelFreshnessReason) -> ReadModelSnapshot<T> {
        let identity = PublishedSnapshotIdentity::Unavailable { reason };
        if let Some(snapshot) = self.reuse_published(identity).await {
            return snapshot;
        }

        self.publish_snapshot(None, ReadModelFreshness { state: ReadModelFreshnessState::Unavailable, reason, observed_at_ms: now_ms(), last_success_at_ms: None }, Some(identity)).await
    }

    async fn reuse_published(&self, identity: PublishedSnapshotIdentity) -> Option<ReadModelSnapshot<T>> {
        let published = self.published.read().await;
        match published.as_ref() {
            Some(current) if current.identity == identity => Some(current.snapshot.clone()),
            _ => None,
        }
    }

    async fn publish_snapshot(&self, payload: Option<T>, freshness: ReadModelFreshness, identity: Option<PublishedSnapshotIdentity>) -> ReadModelSnapshot<T> {
        let revision = self.public_revision.fetch_add(1, Ordering::Relaxed) + 1;
        let snapshot = ReadModelSnapshot { payload, freshness, revision };
        *self.published.write().await = identity.map(|identity| PublishedSnapshot { identity, snapshot: snapshot.clone() });
        snapshot
    }
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use tokio::time::Duration;

    use super::{ReadModelCache, ReadModelFreshnessReason, ReadModelFreshnessState, ReadModelRefreshOutcome};

    #[tokio::test]
    async fn live_cache_hits_keep_public_revision_stable() {
        let cache = ReadModelCache::<u64>::default();
        let first = cache.load_snapshot(Duration::from_secs(30), || async { ReadModelRefreshOutcome::fresh(7) }).await;
        let second = cache.load_snapshot(Duration::from_secs(30), || async { panic!("live cache hit should bypass refresh") }).await;

        assert_eq!(first.revision, second.revision);
        assert_eq!(second.payload, Some(7));
        assert_eq!(second.freshness.state, ReadModelFreshnessState::Live);
        assert_eq!(second.freshness.reason, ReadModelFreshnessReason::Live);
    }

    #[tokio::test]
    async fn stale_cached_responses_publish_a_new_state_once() {
        let cache = ReadModelCache::<u64>::default();
        let fresh = cache.load_snapshot(Duration::from_secs(30), || async { ReadModelRefreshOutcome::fresh(9) }).await;
        let stale_first = cache.load_snapshot(Duration::from_millis(0), || async { ReadModelRefreshOutcome::<u64>::refresh_timeout() }).await;
        let stale_second = cache.load_snapshot(Duration::from_millis(0), || async { ReadModelRefreshOutcome::<u64>::refresh_timeout() }).await;

        assert!(stale_first.revision > fresh.revision);
        assert_eq!(stale_first.revision, stale_second.revision);
        assert_eq!(stale_first.payload, Some(9));
        assert_eq!(stale_first.freshness.state, ReadModelFreshnessState::Stale);
        assert_eq!(stale_first.freshness.reason, ReadModelFreshnessReason::RefreshTimeout);
        assert_eq!(stale_first.freshness.last_success_at_ms, fresh.freshness.last_success_at_ms);
    }

    #[tokio::test]
    async fn unavailable_without_cache_remains_explicit() {
        let cache = ReadModelCache::<u64>::default();
        let first = cache.load_snapshot(Duration::from_secs(30), || async { ReadModelRefreshOutcome::<u64>::refresh_error() }).await;
        let second = cache.load_snapshot(Duration::from_secs(30), || async { ReadModelRefreshOutcome::<u64>::refresh_error() }).await;

        assert_eq!(first.revision, second.revision);
        assert_eq!(first.payload, None);
        assert_eq!(first.freshness.state, ReadModelFreshnessState::Unavailable);
        assert_eq!(first.freshness.reason, ReadModelFreshnessReason::RefreshError);
        assert_eq!(first.freshness.last_success_at_ms, None);
    }

    #[tokio::test]
    async fn stale_fallback_payload_is_exposed_without_a_success_cache() {
        let cache = ReadModelCache::<Vec<&'static str>>::default();
        let snapshot = cache.load_snapshot(Duration::from_secs(30), || async { ReadModelRefreshOutcome::stale_fallback(vec!["base"], ReadModelFreshnessReason::RefreshTimeout) }).await;

        assert_eq!(snapshot.payload, Some(vec!["base"]));
        assert_eq!(snapshot.freshness.state, ReadModelFreshnessState::Stale);
        assert_eq!(snapshot.freshness.reason, ReadModelFreshnessReason::RefreshTimeout);
        assert_eq!(snapshot.freshness.last_success_at_ms, None);
    }
}
