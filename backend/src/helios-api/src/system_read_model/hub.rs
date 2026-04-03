use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex, Weak};

use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

pub(crate) async fn ensure_task<F>(task: &Mutex<Option<JoinHandle<()>>>, spawn: F)
where
    F: FnOnce() -> JoinHandle<()>,
{
    let mut guard = task.lock().await;
    let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
    if needs_spawn {
        *guard = Some(spawn());
    }
}

pub(crate) async fn bind_async_state<T>(state: &RwLock<Option<Weak<T>>>, value: &Arc<T>) {
    let mut guard = state.write().await;
    if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
        *guard = Some(Arc::downgrade(value));
    }
}

pub(crate) fn bind_sync_state<T>(state: &StdMutex<Option<Weak<T>>>, value: &Arc<T>, label: &'static str) {
    let mut guard = state.lock().expect(label);
    if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
        *guard = Some(Arc::downgrade(value));
    }
}

pub(crate) fn clone_sync_latest<T: Clone>(latest: &StdMutex<Option<T>>, label: &'static str) -> Option<T> {
    latest.lock().expect(label).clone()
}

pub(crate) struct TopicRegistry<K, T> {
    inner: Arc<RwLock<BTreeMap<K, Arc<T>>>>,
}

impl<K, T> Clone for TopicRegistry<K, T> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl<K, T> TopicRegistry<K, T>
where
    K: Ord + Clone,
{
    pub(crate) fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(BTreeMap::new())) }
    }

    pub(crate) async fn get_or_insert_with<F>(&self, key: K, create: F) -> Arc<T>
    where
        F: FnOnce() -> T,
    {
        let mut guard = self.inner.write().await;
        guard.entry(key).or_insert_with(|| Arc::new(create())).clone()
    }

    pub(crate) async fn get(&self, key: &K) -> Option<Arc<T>> {
        self.inner.read().await.get(key).cloned()
    }

    pub(crate) async fn remove_if_same(&self, key: &K, topic: &Arc<T>) {
        let mut guard = self.inner.write().await;
        if guard.get(key).is_some_and(|current| Arc::ptr_eq(current, topic)) {
            guard.remove(key);
        }
    }

    pub(crate) async fn values(&self) -> Vec<Arc<T>> {
        self.inner.read().await.values().cloned().collect()
    }
}
