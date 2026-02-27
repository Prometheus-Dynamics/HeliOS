use std::collections::BTreeMap;

use crate::dto::SensorScope;
use crate::error::Result;

use super::SensorsService;

impl SensorsService {
    pub(super) async fn ensure_scope_registered(&self, scope: &SensorScope) {
        let mut state = self.state.write().await;
        if state.scopes.insert(scope.clone()) {
            state.readings.insert(scope.clone(), BTreeMap::new());
        }
    }

    pub async fn subscribe_scope(&self, scope: SensorScope) -> Result<bool> {
        let mut state = self.state.write().await;
        let was_new = state.scopes.insert(scope.clone());
        if was_new {
            state.readings.insert(scope, BTreeMap::new());
        }
        Ok(was_new)
    }

    pub async fn unsubscribe_scope(&self, scope: &SensorScope) -> Result<bool> {
        let mut state = self.state.write().await;
        let removed = state.scopes.remove(scope);
        if removed {
            state.readings.remove(scope);
        }
        Ok(removed)
    }
}
