mod catalog;
mod profile;
mod sampling;

pub(crate) use profile::fetch_profile_output;
pub(crate) use sampling::{fetch_peer_output, fetch_stream_output};

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use uuid::Uuid;

use super::super::AppState;
use helios_engine::localization::config::LocalizationSourceConfig;
use helios_engine::localization::fetch::LocalizationSourceFetcher;
use helios_engine::localization::types::{LocalizationPipelineSource, PipelineOutputSample};

pub(super) use helios_engine::contracts::localization::DEVICE_IMU_EXTERNAL_SOURCE_ID as IMU_EXTERNAL_ID;
pub(super) const PROFILE_STREAM_PREFIX: &str = "profile:";
pub(super) const PROFILE_OUTPUT_PREFIX: &str = "solver:";

#[derive(Clone)]
pub struct ApiLocalizationSourceFetcher {
    pub(super) state: AppState,
    pub(super) profile_resolve_stack: ProfileResolveStack,
}

impl ApiLocalizationSourceFetcher {
    pub fn new(state: AppState) -> Self {
        Self { state, profile_resolve_stack: ProfileResolveStack::default() }
    }
}

#[derive(Clone, Default)]
pub(super) struct ProfileResolveStack {
    entries: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ProfileResolveStack {
    pub(super) fn enter(&self, profile_id: &str) -> Result<ProfileResolveGuard, String> {
        let mut stack = self.entries.lock().expect("profile resolve stack mutex poisoned");
        if stack.iter().any(|entry| entry == profile_id) {
            return Err(format!("profile source cycle detected for '{profile_id}'"));
        }
        stack.push(profile_id.to_string());
        Ok(ProfileResolveGuard { entries: Arc::clone(&self.entries), profile_id: profile_id.to_string() })
    }

    #[cfg(test)]
    fn snapshot(&self) -> Vec<String> {
        self.entries.lock().expect("profile resolve stack mutex poisoned").clone()
    }
}

#[derive(Debug)]
pub(super) struct ProfileResolveGuard {
    entries: Arc<std::sync::Mutex<Vec<String>>>,
    profile_id: String,
}

impl Drop for ProfileResolveGuard {
    fn drop(&mut self) {
        let mut stack = self.entries.lock().expect("profile resolve stack mutex poisoned");
        if let Some(index) = stack.iter().rposition(|entry| entry == &self.profile_id) {
            stack.remove(index);
        }
    }
}

impl LocalizationSourceFetcher for ApiLocalizationSourceFetcher {
    fn fetch_source_value<'a>(&'a self, source: &'a LocalizationSourceConfig) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<JsonValue, String>> + Send + 'a>> {
        Box::pin(async move {
            if source.stream_id.starts_with("peer:") {
                fetch_peer_output(&self.state, &source.stream_id, &source.output_key).await
            } else if let Some(source_id) = source.stream_id.strip_prefix("external:") {
                super::external::fetch_external_value(&self.state, source_id, &source.output_key).await
            } else if let Some(profile_id) = source.stream_id.strip_prefix(PROFILE_STREAM_PREFIX) {
                fetch_profile_output(self, profile_id, &source.output_key).await
            } else {
                fetch_stream_output(&self.state, &source.stream_id, &source.output_key).await
            }
        })
    }
}

#[utoipa::path(
    get,
    path = "/localization/sources",
    tag = "Localization",
    responses((status = 200, description = "Available localization pipeline outputs", body = [LocalizationPipelineSource]))
)]
pub async fn list_sources(state: State<AppState>) -> crate::http::error::ApiResult<Json<Vec<LocalizationPipelineSource>>> {
    catalog::list_sources(state).await
}

#[utoipa::path(
    get,
    path = "/localization/streams/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = Uuid, Path, description = "Stream ID"), ("output_key" = String, Path, description = "Graph output port")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn sample_output(state: State<AppState>, path: Path<(Uuid, String)>) -> axum::response::Response {
    sampling::sample_output(state, path).await
}

#[utoipa::path(
    get,
    path = "/localization/peers/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "Peer ID"), ("output_key" = String, Path, description = "Output key (e.g. tag_poses)")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = crate::http::streams::types::EngineErrorBody),
        (status = 400, description = "Unsupported output", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Peer error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn sample_peer_output(state: State<AppState>, path: Path<(String, String)>) -> axum::response::Response {
    sampling::sample_peer_output(state, path).await
}

#[utoipa::path(
    get,
    path = "/localization/profiles/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "Localization profile ID"), ("output_key" = String, Path, description = "Profile solver output key")),
    responses(
        (status = 200, description = "Latest profile output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = crate::http::streams::types::EngineErrorBody),
        (status = 400, description = "Unsupported output", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Solver error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn sample_profile_output(state: State<AppState>, path: Path<(String, String)>) -> axum::response::Response {
    sampling::sample_profile_output(state, path).await
}

#[cfg(test)]
mod tests {
    use super::ProfileResolveStack;

    #[test]
    fn profile_resolve_stack_releases_entries_after_repeated_sampling() {
        let stack = ProfileResolveStack::default();

        for _ in 0..16 {
            let guard = stack.enter("profile-a").expect("first profile entry");
            assert_eq!(stack.snapshot(), vec!["profile-a".to_string()]);
            drop(guard);
            assert!(stack.snapshot().is_empty());
        }
    }

    #[test]
    fn profile_resolve_stack_detects_cycles_without_leaking_entries() {
        let stack = ProfileResolveStack::default();
        let outer = stack.enter("outer").expect("outer entry");
        let inner = stack.enter("inner").expect("inner entry");

        let err = stack.enter("outer").expect_err("cycle to be rejected");
        assert!(err.contains("cycle detected"));
        assert_eq!(stack.snapshot(), vec!["outer".to_string(), "inner".to_string()]);

        drop(inner);
        drop(outer);
        assert!(stack.snapshot().is_empty());

        let guard = stack.enter("outer").expect("stack to be reusable after cleanup");
        drop(guard);
        assert!(stack.snapshot().is_empty());
    }
}
