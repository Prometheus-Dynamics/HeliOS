use std::sync::Arc;

#[derive(Clone, Default)]
pub struct RuntimeCoordinationService {
    engine_guard: Arc<crate::engine_guard::EngineCrashGuardService>,
    resource_guard: Arc<crate::resource_guard::ResourceGuardRuntime>,
    localization_solve_cache: Arc<crate::http::localization::solve::LocalizationSolveCacheState>,
    localization_sample_refresh: Arc<crate::http::localization::sources::LocalizationStreamSampleRefreshRuntime>,
    localization_external_sources: Arc<crate::http::localization::external::LocalizationExternalSourceRegistry>,
    localization_peer_sources: Arc<crate::http::localization::peers::LocalizationPeerSourceRuntime>,
    nt4_pool: Arc<crate::nt4::pool::Nt4ClientPool>,
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

    pub fn localization_sample_refresh(&self) -> &crate::http::localization::sources::LocalizationStreamSampleRefreshRuntime {
        self.localization_sample_refresh.as_ref()
    }

    pub fn localization_external_sources(&self) -> &crate::http::localization::external::LocalizationExternalSourceRegistry {
        self.localization_external_sources.as_ref()
    }

    pub fn localization_peer_sources(&self) -> &crate::http::localization::peers::LocalizationPeerSourceRuntime {
        self.localization_peer_sources.as_ref()
    }

    pub fn nt4_pool(&self) -> &crate::nt4::pool::Nt4ClientPool {
        self.nt4_pool.as_ref()
    }
}
