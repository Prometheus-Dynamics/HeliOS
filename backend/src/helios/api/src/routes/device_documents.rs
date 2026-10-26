//! The device's settings the UI shows on every open tab: the identities of cameras and other
//! objects (`/v1/device/identities`) and first-time setup (`/v1/device/setup`). Each write is
//! announced as a `settings` event, so the other tabs fetch the change.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    SharedState,
    device_settings::{Identity, SetupRecord},
    error::ApiResult,
};

use super::check_id;

/// `GET /v1/device/identities`: every stored identity, by object id.
pub async fn list_identities(State(state): State<SharedState>) -> ApiResult<Json<BTreeMap<String, Identity>>> {
    Ok(Json(state.store.identities().await?))
}

/// `PUT /v1/device/identities/{id}`: keep the identity of an object; returns every identity.
pub async fn put_identity(State(state): State<SharedState>, Path(id): Path<String>, Json(identity): Json<Identity>) -> ApiResult<Json<BTreeMap<String, Identity>>> {
    check_id(&id)?;
    identity.validate()?;
    Ok(Json(state.store.put_identity(&id, identity).await?))
}

/// `DELETE /v1/device/identities/{id}`: forget an object's identity (its defaults apply again).
pub async fn delete_identity(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    state.store.delete_identity(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /v1/device/setup`: whether first-time setup is done.
pub async fn get_setup(State(state): State<SharedState>) -> ApiResult<Json<SetupRecord>> {
    Ok(Json(state.store.setup().await?))
}

/// `PUT /v1/device/setup`.
pub async fn put_setup(State(state): State<SharedState>, Json(record): Json<SetupRecord>) -> ApiResult<Json<SetupRecord>> {
    state.store.put_setup(&record).await?;
    Ok(Json(record))
}
