//! `/v1/field-layouts`: the field layouts the multi-tag (field) pose solves against
//! (`crate::field_layouts`): list, read, upload a WPILib AprilTag layout JSON or a Limelight
//! `.fmap`, delete, and select the one pipelines without their own use.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use helios_field::{FRC_TAG_SIDE_M, FieldLayout};
use serde::Deserialize;

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    field_layouts::{self, StoredLayout},
    host::now_ms,
};

use super::{check_id, pipelines};

/// `GET /v1/field-layouts`: `{selected, layouts: [summary]}`.
pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<serde_json::Value>> {
    let selected = field_layouts::selected_id(&state).await?;
    let layouts = field_layouts::all_layouts(&state).await?;
    Ok(Json(serde_json::json!({ "selected": selected, "layouts": layouts.iter().map(|layout| layout.summary(&selected)).collect::<Vec<_>>() })))
}

/// `GET /v1/field-layouts/{id}`: the layout with every tag, in the WPILib field frame and tag
/// convention, plus each tag's pose as Eidos solves against it.
pub async fn get_one(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    check_id(&id)?;
    let layout = field_layouts::layout(&state, &id).await.map_err(|_| ApiError::not_found(format!("no field layout {id}")))?;
    let selected = field_layouts::selected_id(&state).await?;
    let mut value = layout.summary(&selected);
    value["layout"] = serde_json::to_value(&layout.layout).map_err(|error| ApiError::internal(error.to_string()))?;
    value["known_tags"] = layout.layout.known_tags_value();
    Ok(Json(value))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    /// The new layout's id (default: from the name).
    pub id: Option<String>,
    pub name: String,
    /// Tag edge length for a WPILib JSON, which has none (default FRC's 0.1651 m).
    pub tag_side_m: Option<f64>,
}

/// `POST /v1/field-layouts?name=..[&id=..][&tag_side_m=..]`, the file as the body: keep a WPILib
/// AprilTag field layout JSON or a Limelight `.fmap`. Replacing an uploaded layout updates the
/// pipelines that use it.
pub async fn upload(State(state): State<SharedState>, Query(query): Query<UploadQuery>, body: String) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let name = query.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::unprocessable("name must be 1 to 120 characters"));
    }
    let id = query.id.clone().unwrap_or_else(|| slug(name));
    check_id(&id)?;
    if field_layouts::is_builtin(&id) {
        return Err(ApiError::conflict(format!("{id} is a built-in layout")));
    }
    let tag_side_m = query.tag_side_m.unwrap_or(FRC_TAG_SIDE_M);
    if !(tag_side_m.is_finite() && tag_side_m > 0.0 && tag_side_m < 10.0) {
        return Err(ApiError::unprocessable("tag_side_m is the tag edge length in metres (above 0, under 10)"));
    }
    let (layout, format) = FieldLayout::parse(&body, tag_side_m).map_err(|error| ApiError::unprocessable(format!("not a field layout: {error}")))?;
    let stored = StoredLayout { id: id.clone(), name: name.to_string(), format, builtin: false, saved_at_ms: now_ms(), layout };
    state.store.put_field_layout(&stored).await?;
    state.events.publish("field_layout", serde_json::json!({ "id": id, "change": "saved" }));
    refresh(&state, Some(&id), false).await;
    let selected = field_layouts::selected_id(&state).await?;
    Ok((StatusCode::CREATED, Json(stored.summary(&selected))))
}

/// `DELETE /v1/field-layouts/{id}`: forget an uploaded layout that no pipeline names and is not
/// selected.
pub async fn remove(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    if field_layouts::is_builtin(&id) {
        return Err(ApiError::conflict(format!("{id} is a built-in layout")));
    }
    if field_layouts::selected_id(&state).await? == id {
        return Err(ApiError::conflict(format!("{id} is the selected layout; select another first")));
    }
    let view = state.orion.view().await?;
    let users = pipelines::pipelines_using_layout(&view, &id);
    if !users.is_empty() {
        return Err(ApiError::conflict(format!("pipelines {} use {id}", users.join(", "))));
    }
    if !state.store.delete_field_layout(&id).await? {
        return Err(ApiError::not_found(format!("no field layout {id}")));
    }
    state.events.publish("field_layout", serde_json::json!({ "id": id, "change": "deleted" }));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
}

/// `GET /v1/field-layouts/selected`.
pub async fn get_selected(State(state): State<SharedState>) -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({ "id": field_layouts::selected_id(&state).await? })))
}

/// `PUT /v1/field-layouts/selected` `{id}`: the layout every pipeline without its own uses; those
/// pipelines are redeployed with it (same revision).
pub async fn put_selected(State(state): State<SharedState>, Json(selection): Json<Selection>) -> ApiResult<Json<serde_json::Value>> {
    check_id(&selection.id)?;
    field_layouts::layout(&state, &selection.id).await?;
    state.store.select_field_layout(&selection.id).await?;
    state.events.publish("field_layout", serde_json::json!({ "id": selection.id, "change": "selected" }));
    let updated = pipelines::refresh_field_layouts(&state, None, true).await?;
    Ok(Json(serde_json::json!({ "id": selection.id, "pipelines": updated })))
}

async fn refresh(state: &SharedState, changed: Option<&str>, selection: bool) {
    match pipelines::refresh_field_layouts(state, changed, selection).await {
        Ok(updated) if !updated.is_empty() => tracing::info!(layout = ?changed, pipelines = ?updated, "field layout handed to its pipelines"),
        Ok(_) => {}
        Err(error) => tracing::warn!(layout = ?changed, error = %error.message, "field layout not handed to its pipelines"),
    }
}

fn slug(name: &str) -> String {
    let mut out: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-');
    let out = if out.is_empty() { "field" } else { out };
    out.chars().take(60).collect()
}
