use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use super::super::super::AppState;
use super::super::super::error::{ApiError, ApiResult};
use helios_engine::localization::external::{ExternalLocalizationSample, ExternalLocalizationSampleRequest, ExternalLocalizationSource, ExternalLocalizationSourceUpsert};

#[utoipa::path(
    get,
    path = "/localization/external/sources",
    tag = "Localization",
    responses((status = 200, description = "External localization sources", body = [ExternalLocalizationSource]))
)]
pub(crate) async fn list_external_sources(State(_state): State<AppState>) -> ApiResult<Json<Vec<ExternalLocalizationSource>>> {
    Ok(Json(list_external_sources_snapshot().await))
}

#[utoipa::path(
    put,
    path = "/localization/external/sources/{id}",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id")),
    request_body = ExternalLocalizationSourceUpsert,
    responses((status = 200, description = "Upserted external source", body = ExternalLocalizationSource))
)]
pub(crate) async fn upsert_external_source(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ExternalLocalizationSourceUpsert>,
) -> ApiResult<Json<ExternalLocalizationSource>> {
    let source = helios_engine::localization::external::upsert_source(&id, payload).await.map_err(ApiError::bad_request)?;
    Ok(Json(source))
}

#[utoipa::path(
    delete,
    path = "/localization/external/sources/{id}",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id")),
    responses((status = 204, description = "External source removed"))
)]
pub(crate) async fn delete_external_source(State(_state): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    helios_engine::localization::external::delete_source(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/localization/external/sources/{id}/sample",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id")),
    request_body = ExternalLocalizationSampleRequest,
    responses((status = 200, description = "Updated sample", body = ExternalLocalizationSample))
)]
pub(crate) async fn update_external_sample(
    State(_state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ExternalLocalizationSampleRequest>,
) -> ApiResult<Json<ExternalLocalizationSample>> {
    let sample = helios_engine::localization::external::update_sample(&id, payload).await.map_err(ApiError::bad_request)?;
    Ok(Json(sample))
}

pub(crate) async fn list_external_sources_snapshot() -> Vec<ExternalLocalizationSource> {
    helios_engine::localization::external::list_sources().await
}
