mod config;
mod state;
#[cfg(test)]
mod tests;
mod types;

use super::super::error::{ApiError, ApiResult};
use crate::http::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use lib_net::interface::{NetworkInterfaceSettings, get_interfaces, set_interface};

use self::config::persist_networkd_config;

pub(crate) use state::DeviceNetworkState;
pub use types::TeamNumberPayload;

#[utoipa::path(
    get,
    path = "/device/network",
    tag = "Device",
    responses((status = 200, description = "Network config", body = [NetworkInterfaceSettings]), (status = 502, description = "Network unavailable", body = super::super::error::ErrorBody))
)]
pub async fn network() -> ApiResult<impl IntoResponse> {
    let ifaces = get_interfaces().await.map_err(ApiError::from)?;
    Ok(Json(ifaces))
}

#[utoipa::path(
    post,
    path = "/device/network",
    tag = "Device",
    request_body = NetworkInterfaceSettings,
    responses((status = 204, description = "Network config updated"), (status = 400, description = "Invalid request", body = super::super::error::ErrorBody))
)]
pub async fn set_network(Json(payload): Json<NetworkInterfaceSettings>) -> ApiResult<impl IntoResponse> {
    set_interface(&payload).await.map_err(ApiError::from)?;
    persist_networkd_config(&payload).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/device/team",
    tag = "Device",
    responses((status = 200, description = "Team number", body = TeamNumberPayload))
)]
pub async fn team(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    Ok(Json(TeamNumberPayload { team_number: state.services.network.inner().team_value().await? }))
}

#[utoipa::path(
    post,
    path = "/device/team",
    tag = "Device",
    request_body(content = TeamNumberPayload, content_type = "application/json"),
    responses((status = 204, description = "Team updated"), (status = 400, description = "Invalid request", body = super::super::error::ErrorBody))
)]
pub async fn set_team(State(state): State<AppState>, Json(payload): Json<TeamNumberPayload>) -> ApiResult<impl IntoResponse> {
    state.services.network.inner().set_team_value(payload.team_number).await?;
    Ok(StatusCode::NO_CONTENT)
}
