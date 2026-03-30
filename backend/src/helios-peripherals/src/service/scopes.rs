use std::collections::BTreeMap;

use crate::dto::SensorScope;
use crate::error::Result;

use super::SensorsService;

impl SensorsService {
    pub(super) async fn ensure_scope_registered(&self, scope: &SensorScope) {
        let mut state = self.state.write().await;
        if state.scopes.insert(scope.clone()) {
            state.readings.insert(scope.clone(), BTreeMap::new());
            state.serialized_readings.insert(scope.clone(), BTreeMap::new());
        }
    }

    pub async fn subscribe_scope(&self, scope: SensorScope) -> Result<bool> {
        let mut state = self.state.write().await;
        if state.scopes.insert(scope.clone()) {
            state.readings.insert(scope.clone(), BTreeMap::new());
            state.serialized_readings.insert(scope.clone(), BTreeMap::new());
        }
        let count = state.scope_subscribers.entry(scope).or_insert(0);
        *count = count.saturating_add(1);
        Ok(*count == 1)
    }

    pub async fn unsubscribe_scope(&self, scope: &SensorScope) -> Result<bool> {
        let mut state = self.state.write().await;
        let Some(count) = state.scope_subscribers.get_mut(scope) else {
            return Ok(false);
        };
        if *count == 0 {
            return Ok(false);
        }
        *count -= 1;
        if *count == 0 {
            state.scope_subscribers.remove(scope);
            if !matches!(scope, SensorScope::Device) {
                state.scopes.remove(scope);
                state.readings.remove(scope);
                state.serialized_readings.remove(scope);
            }
        }
        Ok(true)
    }

    pub(crate) async fn has_scope_subscribers(&self, scope: &SensorScope) -> bool {
        let state = self.state.read().await;
        state.scope_subscribers.get(scope).copied().unwrap_or(0) > 0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio_util::sync::CancellationToken;

    use crate::config::SensorsConfig;
    use crate::dto::SensorScope;

    use super::SensorsService;

    #[tokio::test]
    async fn device_scope_tracks_live_subscriber_count_without_dropping_cache() {
        let service = SensorsService::new(Arc::new(SensorsConfig::default()), CancellationToken::new());
        let scope = SensorScope::Device;

        assert!(!service.has_scope_subscribers(&scope).await);
        assert!(service.subscribe_scope(scope.clone()).await.unwrap());
        assert!(service.has_scope_subscribers(&scope).await);
        assert!(!service.subscribe_scope(scope.clone()).await.unwrap());
        assert!(service.unsubscribe_scope(&scope).await.unwrap());
        assert!(service.has_scope_subscribers(&scope).await);
        assert!(service.unsubscribe_scope(&scope).await.unwrap());
        assert!(!service.has_scope_subscribers(&scope).await);

        let state = service.state.read().await;
        assert!(state.scopes.contains(&scope));
        assert!(state.readings.contains_key(&scope));
        assert!(state.serialized_readings.contains_key(&scope));
    }

    #[tokio::test]
    async fn non_device_scope_is_removed_when_last_subscriber_leaves() {
        let service = SensorsService::new(Arc::new(SensorsConfig::default()), CancellationToken::new());
        let scope = SensorScope::Custom { name: "test".into() };

        assert!(service.subscribe_scope(scope.clone()).await.unwrap());
        assert!(service.unsubscribe_scope(&scope).await.unwrap());
        assert!(!service.has_scope_subscribers(&scope).await);

        let state = service.state.read().await;
        assert!(!state.scopes.contains(&scope));
        assert!(!state.readings.contains_key(&scope));
        assert!(!state.serialized_readings.contains_key(&scope));
    }
}
