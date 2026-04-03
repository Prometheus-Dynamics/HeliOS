use std::sync::Arc;

#[derive(Clone, Default)]
pub struct RuntimeCoordinationService {
    engine_guard: Arc<crate::engine_guard::EngineCrashGuardService>,
    resource_guard: Arc<crate::resource_guard::ResourceGuardRuntime>,
    localization_solve_cache: Arc<crate::http::localization::solve::LocalizationSolveCacheState>,
}

impl RuntimeCoordinationService {
    pub fn spawn_background_tasks(&self, state: &crate::http::AppState) {
        self.engine_guard.clone().spawn_task(state.ipc().clone());
        self.resource_guard.clone().spawn_task(state.ipc().clone());
    }

    pub fn engine_guard(&self) -> &crate::engine_guard::EngineCrashGuardService {
        self.engine_guard.as_ref()
    }

    pub fn resource_guard(&self) -> &crate::resource_guard::ResourceGuardRuntime {
        self.resource_guard.as_ref()
    }

    pub fn localization_solve_cache(&self) -> &crate::http::localization::solve::LocalizationSolveCacheState {
        self.localization_solve_cache.as_ref()
    }
}
