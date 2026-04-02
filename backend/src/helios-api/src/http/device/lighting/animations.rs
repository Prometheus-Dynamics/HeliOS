use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use lib_led_animations::{LED_ANIMATIONS_PATH, LedAnimationEntry, LedAnimationFrame, load_led_animations, persist_led_animations, timeline_to_sequence};
use tracing::debug;

use crate::http::{
    AppState,
    error::{ApiError, ApiResult, ErrorBody},
};

use super::{LightingAnimationEntryResponse, LightingAnimationListResponse, LightingAnimationSaveRequest, LightingFramePayload};

#[utoipa::path(
    get,
    path = "/device/lighting/animations",
    tag = "Device",
    responses((status = 200, description = "Saved lighting animations", body = LightingAnimationListResponse))
)]
pub async fn list_lighting_animations() -> ApiResult<impl IntoResponse> {
    let doc = load_led_animations(LED_ANIMATIONS_PATH).await;
    let animations = doc.animations.into_iter().map(entry_to_response).collect();
    Ok(Json(LightingAnimationListResponse { animations }))
}

#[utoipa::path(
    post,
    path = "/device/lighting/animations",
    tag = "Device",
    request_body = LightingAnimationSaveRequest,
    responses(
        (status = 204, description = "Lighting animation saved"),
        (status = 400, description = "Invalid request", body = ErrorBody)
    )
)]
pub async fn save_lighting_animation(State(state): State<AppState>, Json(payload): Json<LightingAnimationSaveRequest>) -> ApiResult<StatusCode> {
    if let Some(requested_by) = payload.requested_by.as_deref() {
        debug!(requested_by, "lighting animation save requested");
    }

    let name = payload.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("animation name is required"));
    }
    if payload.frame.is_none() && payload.animation.is_none() && payload.frames.is_none() && payload.timeline.is_none() {
        return Err(ApiError::bad_request("animation must include a frame, frames, timeline, or animation payload"));
    }
    if let Some(frames) = payload.frames.as_ref()
        && frames.is_empty()
    {
        return Err(ApiError::bad_request("sequence must include at least one frame"));
    }
    if let Some(timeline) = payload.timeline.as_ref()
        && timeline.keyframes.is_empty()
    {
        return Err(ApiError::bad_request("timeline must include at least one keyframe"));
    }

    let frame = payload.frame.map(|entries| entries.into_iter().map(Into::into).collect::<Vec<_>>());
    let mut frames = payload
        .frames
        .map(|entries| entries.into_iter().map(|entry| LedAnimationFrame { frame: entry.frame.into_iter().map(Into::into).collect(), duration_ms: entry.duration_ms }).collect::<Vec<_>>())
        .unwrap_or_default();
    let timeline = payload.timeline.map(Into::into);
    if frames.is_empty()
        && let Some(timeline_ref) = timeline.as_ref()
    {
        frames = timeline_to_sequence(timeline_ref);
    }
    let animation = payload.animation.map(Into::into);
    let command = helios_peripherals::dto::LightingCommand { frame, brightness: payload.brightness, animation };
    let entry = LedAnimationEntry { name: name.to_string(), command, duration_ms: payload.duration_ms, sequence: frames, timeline };

    let mut doc = load_led_animations(LED_ANIMATIONS_PATH).await;
    doc.animations.retain(|existing| !existing.name.eq_ignore_ascii_case(name));
    doc.animations.push(entry);
    doc.animations.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));

    persist_led_animations(LED_ANIMATIONS_PATH, &doc).await.map_err(|err| ApiError::internal(format!("failed to persist lighting animations: {err}")))?;
    state.services.pipelines.invalidate_registry_cache().await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/device/lighting/animations/{name}",
    tag = "Device",
    responses((status = 204, description = "Lighting animation deleted"))
)]
pub async fn delete_lighting_animation(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<StatusCode> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request("animation name is required"));
    }

    let mut doc = load_led_animations(LED_ANIMATIONS_PATH).await;
    let before = doc.animations.len();
    doc.animations.retain(|entry| !entry.name.eq_ignore_ascii_case(trimmed));
    if doc.animations.len() != before {
        persist_led_animations(LED_ANIMATIONS_PATH, &doc).await.map_err(|err| ApiError::internal(format!("failed to persist lighting animations: {err}")))?;
        state.services.pipelines.invalidate_registry_cache().await;
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn entry_to_response(entry: LedAnimationEntry) -> LightingAnimationEntryResponse {
    let frame = entry.command.frame.map(|colors| colors.into_iter().map(Into::into).collect());
    let animation = entry.command.animation.map(Into::into);
    let frames = if entry.sequence.is_empty() {
        None
    } else {
        Some(entry.sequence.into_iter().map(|item| LightingFramePayload { frame: item.frame.into_iter().map(Into::into).collect(), duration_ms: item.duration_ms }).collect())
    };
    let timeline = entry.timeline.map(Into::into);
    LightingAnimationEntryResponse { name: entry.name, frame, frames, timeline, brightness: entry.command.brightness, animation, duration_ms: entry.duration_ms }
}
