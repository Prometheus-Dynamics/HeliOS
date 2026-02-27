use super::super::AppState;
use super::super::error::{ApiError, ApiResult, ErrorBody};
use axum::{Json, Router, extract::Path, response::IntoResponse, routing::get};
use utoipa::ToSchema;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_limelight_adapters)).route("/:table/status", get(limelight_status)).route("/:table/results", get(limelight_results))
}

#[derive(Debug, Clone, serde::Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightResultsStagedResponse {
    pub table_name: String,
    pub publish_phase: String,
    pub publish_ready: bool,
    pub message: String,
}

#[utoipa::path(
    get,
    path = "/limelight",
    tag = "NT4",
    responses((status = 200, description = "Limelight adapter registry status", body = crate::nt4::limelight::LimelightAdapterRegistryStatus))
)]
pub async fn list_limelight_adapters() -> ApiResult<impl IntoResponse> {
    Ok(Json(crate::nt4::limelight::registry_status().await))
}

#[utoipa::path(
    get,
    path = "/limelight/{table}/status",
    tag = "NT4",
    params(("table" = String, Path, description = "Limelight table name (or stream id/alias)")),
    responses(
        (status = 200, description = "Limelight adapter status", body = crate::nt4::limelight::LimelightAdapterStatus),
        (status = 404, description = "Adapter not found", body = ErrorBody)
    )
)]
pub async fn limelight_status(Path(table): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(status) = crate::nt4::limelight::adapter_status(&table).await else {
        return Err(ApiError::not_found(format!("limelight table '{table}' was not found")));
    };
    Ok(Json(status))
}

#[utoipa::path(
    get,
    path = "/limelight/{table}/results",
    tag = "NT4",
    params(("table" = String, Path, description = "Limelight table name (or stream id/alias)")),
    responses(
        (status = 200, description = "Limelight results staging status", body = LimelightResultsStagedResponse),
        (status = 404, description = "Adapter not found", body = ErrorBody)
    )
)]
pub async fn limelight_results(Path(table): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(status) = crate::nt4::limelight::adapter_status(&table).await else {
        return Err(ApiError::not_found(format!("limelight table '{table}' was not found")));
    };

    Ok(Json(LimelightResultsStagedResponse {
        table_name: status.table_name,
        publish_phase: status.publish_phase,
        publish_ready: status.publish_ready,
        message: "Limelight adapter publish wiring is intentionally staged; NT/HTTP value publishing has not started yet.".to_string(),
    }))
}
