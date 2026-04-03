mod inputs;
mod request;
mod rig;
#[cfg(test)]
mod tests;

pub(crate) use inputs::{dedupe_enabled_sources, enrich_source_input_keys_from_streams};
pub(crate) use request::{LocalizationSolveCacheState, fetch_localization_source_values, solve_via_engine};
pub(crate) use rig::{inject_imu_leveling_rig_pose, load_rig_poses_from_streams};

use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use utoipa::ToSchema;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::config;
use super::maps;
use super::sources::ApiLocalizationSourceFetcher;
use helios_engine::localization::config::select_profile;
use helios_engine::localization::types::LocalizationSolveResponse;

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct LocalizationSolveQuery {
    #[serde(default, rename = "profile_id", alias = "profileId")]
    profile_id: Option<String>,
    #[serde(default = "default_apply_field_origin", rename = "apply_field_origin", alias = "applyFieldOrigin")]
    apply_field_origin: bool,
    #[serde(default, rename = "field_poses_only", alias = "fieldPosesOnly")]
    field_poses_only: bool,
}

fn default_apply_field_origin() -> bool {
    true
}

#[utoipa::path(
    get,
    path = "/localization/solve",
    tag = "Localization",
    params(
        ("profile_id" = Option<String>, Query, description = "Profile id override"),
        ("apply_field_origin" = Option<bool>, Query, description = "Apply profile fieldOrigin transform to field-space outputs (default true)"),
        ("field_poses_only" = Option<bool>, Query, description = "Return only field-space solve poses, omitting raw detection-space outputs")
    ),
    responses((status = 200, description = "Localization solve outputs", body = LocalizationSolveResponse))
)]
pub async fn solve(State(state): State<AppState>, Query(query): Query<LocalizationSolveQuery>) -> ApiResult<Json<LocalizationSolveResponse>> {
    let config = config::load_config().await?;
    let profile = select_profile(&config, query.profile_id.as_deref()).map_err(ApiError::not_found)?;
    let mut sources = dedupe_enabled_sources(profile.sources.iter().filter(|source| source.enabled).cloned().collect::<Vec<_>>());
    let stream_summaries = state.engine.list_streams().await.unwrap_or_default();
    enrich_source_input_keys_from_streams(&stream_summaries, &mut sources);

    let mut rig_poses = load_rig_poses_from_streams(&sources, &stream_summaries).await;
    inject_imu_leveling_rig_pose(&state, profile, &mut rig_poses).await;
    let field_map = if let Some(map_id) = profile.field_map_id.as_deref() { maps::load_map_document(map_id).await.ok() } else { None };
    let fetcher = ApiLocalizationSourceFetcher::new(state.clone());
    let mut response = solve_via_engine(&state, profile, sources, &rig_poses, field_map.as_ref(), &stream_summaries, &fetcher, query.apply_field_origin).await.map_err(ApiError::bad_gateway)?;
    if query.field_poses_only {
        request::trim_localization_solve_response_to_field_poses(&mut response);
    }

    Ok(Json(response))
}
