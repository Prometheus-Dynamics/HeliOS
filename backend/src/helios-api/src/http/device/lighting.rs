use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use lib_led_animations::{
    LED_ANIMATIONS_PATH, LEGACY_LED_ANIMATIONS_PATH, LedAnimationEntry, LedAnimationFrame, LedAnimationTimeline, LedTimelineEasing, LedTimelineKeyframe, load_led_animations, persist_led_animations,
    timeline_to_sequence,
};
use lib_sensors::led_config::{self, LedConfig};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
use tokio::fs;
use tracing::debug;
use utoipa::ToSchema;

use helios_peripherals::dto::{LightingAnimation, LightingColor, LightingCommand, LightingRuntimeState};

use super::super::AppState;
use super::super::error::{ApiError, ApiResult, ErrorBody};
use crate::http::persisted_files;

const LEGACY_LED_SETTINGS_PATH: &str = "/etc/helios/leds.toml";
const LIGHTING_TEMPLATE_DIR: &str = "/usr/share/helios/lighting-templates";

#[derive(Debug, Deserialize, ToSchema)]
pub struct LightingConfigRequest {
    pub lighting: LedConfig,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Serialize)]
struct LightingConfigDoc {
    leds: LedConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LightingColorPayload {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    #[serde(default)]
    pub w: u8,
}

impl From<LightingColorPayload> for LightingColor {
    fn from(value: LightingColorPayload) -> Self {
        Self { r: value.r, g: value.g, b: value.b, w: value.w }
    }
}

impl From<LightingColor> for LightingColorPayload {
    fn from(value: LightingColor) -> Self {
        Self { r: value.r, g: value.g, b: value.b, w: value.w }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LightingAnimationPayload {
    Off,
    Chase {
        color: LightingColorPayload,
        #[serde(default)]
        speed_hz: f32,
    },
    Pulse {
        color: LightingColorPayload,
        #[serde(default)]
        low: u8,
        #[serde(default)]
        high: u8,
        #[serde(default)]
        period_ms: u32,
    },
    Rainbow {
        #[serde(default)]
        speed_hz: f32,
    },
    BreathingRainbow {
        #[serde(default)]
        speed_hz: f32,
        #[serde(default)]
        low: u8,
        #[serde(default)]
        high: u8,
        #[serde(default)]
        period_ms: u32,
    },
}

impl From<LightingAnimationPayload> for LightingAnimation {
    fn from(value: LightingAnimationPayload) -> Self {
        match value {
            LightingAnimationPayload::Off => Self::Off,
            LightingAnimationPayload::Chase { color, speed_hz } => Self::Chase { color: color.into(), speed_hz },
            LightingAnimationPayload::Pulse { color, low, high, period_ms } => Self::Pulse { color: color.into(), low, high, period_ms },
            LightingAnimationPayload::Rainbow { speed_hz } => Self::Rainbow { speed_hz },
            LightingAnimationPayload::BreathingRainbow { speed_hz, low, high, period_ms } => Self::BreathingRainbow { speed_hz, low, high, period_ms },
        }
    }
}

impl From<LightingAnimation> for LightingAnimationPayload {
    fn from(value: LightingAnimation) -> Self {
        match value {
            LightingAnimation::Off => Self::Off,
            LightingAnimation::Chase { color, speed_hz } => Self::Chase { color: color.into(), speed_hz },
            LightingAnimation::Pulse { color, low, high, period_ms } => Self::Pulse { color: color.into(), low, high, period_ms },
            LightingAnimation::Rainbow { speed_hz } => Self::Rainbow { speed_hz },
            LightingAnimation::BreathingRainbow { speed_hz, low, high, period_ms } => Self::BreathingRainbow { speed_hz, low, high, period_ms },
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LightingAnimationSaveRequest {
    pub name: String,
    #[serde(default)]
    pub frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    pub frames: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    pub timeline: Option<LightingTimelinePayload>,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LightingAnimationEntryResponse {
    pub name: String,
    #[serde(default)]
    pub frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    pub frames: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    pub timeline: Option<LightingTimelinePayload>,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LightingAnimationListResponse {
    pub animations: Vec<LightingAnimationEntryResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LightingAnimationTemplateSummary {
    pub template_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LightingAnimationTemplateDocument {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    pub frames: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    pub timeline: Option<LightingTimelinePayload>,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LightingTemplateCommandRaw {
    #[serde(default)]
    frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    brightness: Option<u8>,
    #[serde(default)]
    animation: Option<LightingAnimationPayload>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LightingAnimationTemplateDocumentRaw {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    command: Option<LightingTemplateCommandRaw>,
    #[serde(default)]
    frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    frames: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    sequence: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    timeline: Option<LightingTimelinePayload>,
    #[serde(default)]
    brightness: Option<u8>,
    #[serde(default)]
    animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    duration_ms: Option<u32>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LightingCommandRequest {
    #[serde(default)]
    pub frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LightingRuntimeStatePayload {
    #[serde(default)]
    pub frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub animation_running: bool,
}

impl From<LightingRuntimeState> for LightingRuntimeStatePayload {
    fn from(value: LightingRuntimeState) -> Self {
        let animation = value.command.animation.map(Into::into);
        let animation_running = matches!(
            animation,
            Some(LightingAnimationPayload::Chase { .. } | LightingAnimationPayload::Pulse { .. } | LightingAnimationPayload::Rainbow { .. } | LightingAnimationPayload::BreathingRainbow { .. })
        );
        Self {
            frame: value.command.frame.map(|entries| entries.into_iter().map(Into::into).collect()),
            brightness: value.command.brightness,
            animation,
            updated_at_ms: value.updated_at_ms,
            animation_running,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LightingFramePayload {
    #[serde(default)]
    pub frame: Vec<LightingColorPayload>,
    #[serde(default)]
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LightingTimelineEasingPayload {
    Step,
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl From<LightingTimelineEasingPayload> for LedTimelineEasing {
    fn from(value: LightingTimelineEasingPayload) -> Self {
        match value {
            LightingTimelineEasingPayload::Step => Self::Step,
            LightingTimelineEasingPayload::Linear => Self::Linear,
            LightingTimelineEasingPayload::EaseIn => Self::EaseIn,
            LightingTimelineEasingPayload::EaseOut => Self::EaseOut,
            LightingTimelineEasingPayload::EaseInOut => Self::EaseInOut,
        }
    }
}

impl From<LedTimelineEasing> for LightingTimelineEasingPayload {
    fn from(value: LedTimelineEasing) -> Self {
        match value {
            LedTimelineEasing::Step => Self::Step,
            LedTimelineEasing::Linear => Self::Linear,
            LedTimelineEasing::EaseIn => Self::EaseIn,
            LedTimelineEasing::EaseOut => Self::EaseOut,
            LedTimelineEasing::EaseInOut => Self::EaseInOut,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LightingTimelineKeyframePayload {
    #[serde(default)]
    pub time_ms: u32,
    #[serde(default)]
    pub frame: Vec<LightingColorPayload>,
    #[serde(default)]
    pub easing: LightingTimelineEasingPayload,
}

impl From<LightingTimelineKeyframePayload> for LedTimelineKeyframe {
    fn from(value: LightingTimelineKeyframePayload) -> Self {
        Self { time_ms: value.time_ms, frame: value.frame.into_iter().map(Into::into).collect(), easing: value.easing.into() }
    }
}

impl From<LedTimelineKeyframe> for LightingTimelineKeyframePayload {
    fn from(value: LedTimelineKeyframe) -> Self {
        Self { time_ms: value.time_ms, frame: value.frame.into_iter().map(Into::into).collect(), easing: value.easing.into() }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct LightingTimelinePayload {
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub sample_ms: Option<u32>,
    #[serde(default)]
    pub keyframes: Vec<LightingTimelineKeyframePayload>,
}

impl From<LightingTimelinePayload> for LedAnimationTimeline {
    fn from(value: LightingTimelinePayload) -> Self {
        Self { duration_ms: value.duration_ms, sample_ms: value.sample_ms, keyframes: value.keyframes.into_iter().map(Into::into).collect() }
    }
}

impl From<LedAnimationTimeline> for LightingTimelinePayload {
    fn from(value: LedAnimationTimeline) -> Self {
        Self { duration_ms: value.duration_ms, sample_ms: value.sample_ms, keyframes: value.keyframes.into_iter().map(Into::into).collect() }
    }
}

#[utoipa::path(
    get,
    path = "/device/lighting/config",
    tag = "Device",
    responses((status = 200, description = "Lighting configuration", body = LedConfig))
)]
pub async fn lighting_config() -> ApiResult<impl IntoResponse> {
    let config = load_lighting_config().await;
    Ok(Json(config))
}

#[utoipa::path(
    post,
    path = "/device/lighting/config",
    tag = "Device",
    request_body = LightingConfigRequest,
    responses(
        (status = 204, description = "Lighting configuration updated"),
        (status = 400, description = "Invalid request", body = ErrorBody)
    )
)]
pub async fn update_lighting_config(Json(payload): Json<LightingConfigRequest>) -> ApiResult<StatusCode> {
    if let Some(requested_by) = payload.requested_by.as_deref() {
        debug!(requested_by, "lighting config update requested");
    }

    if payload.lighting.count == 0 {
        return Err(ApiError::bad_request("led count must be at least 1"));
    }
    if payload.lighting.color_order.trim().is_empty() {
        return Err(ApiError::bad_request("color order is required"));
    }
    if payload.lighting.protocol.trim().is_empty() {
        return Err(ApiError::bad_request("protocol is required"));
    }

    persist_lighting_config(&payload.lighting).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/device/lighting/config/reset",
    tag = "Device",
    responses((status = 200, description = "Lighting configuration reset", body = LedConfig))
)]
pub async fn reset_lighting_config() -> ApiResult<impl IntoResponse> {
    let config = LedConfig::default();
    persist_lighting_config(&config).await?;
    Ok(Json(config))
}

#[utoipa::path(
    post,
    path = "/device/lighting",
    tag = "Device",
    request_body = LightingCommandRequest,
    responses(
        (status = 204, description = "Lighting command applied"),
        (status = 400, description = "Bad request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody),
        (status = 502, description = "Peripherals error", body = ErrorBody),
    )
)]
pub async fn lighting_command(State(state): State<AppState>, Json(body): Json<LightingCommandRequest>) -> ApiResult<StatusCode> {
    let Some(sensors) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    if let Some(requested_by) = body.requested_by.as_deref() {
        debug!(requested_by, brightness = ?body.brightness, "lighting command requested");
    }

    let frame = body.frame.map(|entries| entries.into_iter().map(Into::into).collect::<Vec<_>>());
    let animation = body.animation.map(Into::into);
    let command = LightingCommand { frame, brightness: body.brightness, animation };

    match sensors.lighting_command(command).await {
        Ok(Ok(())) => Ok(StatusCode::NO_CONTENT),
        Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
        Err(err) => Err(ApiError::bad_gateway(format!("failed to send lighting command: {err}"))),
    }
}

#[utoipa::path(
    get,
    path = "/device/lighting/state",
    tag = "Device",
    responses(
        (status = 200, description = "Current lighting runtime state", body = LightingRuntimeStatePayload),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody),
        (status = 502, description = "Peripherals error", body = ErrorBody),
    )
)]
pub async fn lighting_state(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let Some(sensors) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    match sensors.lighting_state().await {
        Ok(Ok(current)) => Ok(Json(LightingRuntimeStatePayload::from(current))),
        Ok(Err(reason)) => Err(ApiError::bad_gateway(reason)),
        Err(err) => Err(ApiError::bad_gateway(format!("failed to fetch lighting runtime state: {err}"))),
    }
}

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
    get,
    path = "/device/lighting/templates",
    tag = "Device",
    responses((status = 200, description = "List built-in lighting templates", body = [LightingAnimationTemplateSummary]))
)]
pub async fn list_lighting_templates() -> ApiResult<impl IntoResponse> {
    let dir = lighting_template_dir();
    let mut summaries = Vec::new();
    let mut entries = match fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Json(summaries)),
        Err(err) => return Err(map_template_io_error(err, "failed to read lighting template directory")),
    };

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return Err(map_template_io_error(err, "failed to read lighting template entry")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(template_id) = path.file_stem().and_then(|stem| stem.to_str()).and_then(normalize_template_id) else {
            continue;
        };
        let data = match fs::read_to_string(&path).await {
            Ok(data) => data,
            Err(err) => return Err(map_template_io_error(err, "failed to read lighting template file")),
        };
        let Ok(raw) = serde_json::from_str::<LightingAnimationTemplateDocumentRaw>(&data) else {
            continue;
        };
        let name = raw.name.as_deref().map(str::trim).filter(|value| !value.is_empty()).unwrap_or(&template_id).to_string();
        let summary = raw.summary.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        summaries.push(LightingAnimationTemplateSummary { template_id, name, summary, tags: raw.tags });
    }

    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(summaries))
}

#[utoipa::path(
    get,
    path = "/device/lighting/templates/{id}",
    tag = "Device",
    params(("id" = String, Path, description = "Template identifier")),
    responses(
        (status = 200, description = "Lighting template document", body = LightingAnimationTemplateDocument),
        (status = 404, description = "Template not found", body = ErrorBody)
    )
)]
pub async fn fetch_lighting_template(Path(id): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(template_id) = normalize_template_id(&id) else {
        return Err(ApiError::bad_request("invalid template id"));
    };
    let path = lighting_template_dir().join(format!("{template_id}.json"));
    let data = match fs::read_to_string(&path).await {
        Ok(data) => data,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(ApiError::not_found("template not found")),
        Err(err) => return Err(map_template_io_error(err, "failed to read lighting template file")),
    };
    let raw = serde_json::from_str::<LightingAnimationTemplateDocumentRaw>(&data)
        .map_err(|err| map_template_io_error(io::Error::new(io::ErrorKind::InvalidData, err), "failed to decode lighting template"))?;
    let doc = template_raw_into_document(template_id, raw).map_err(|err| map_template_io_error(io::Error::new(io::ErrorKind::InvalidData, err), "invalid lighting template"))?;
    Ok(Json(doc))
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
    let command = LightingCommand { frame, brightness: payload.brightness, animation };
    let entry = LedAnimationEntry { name: name.to_string(), command, duration_ms: payload.duration_ms, sequence: frames, timeline };

    let mut doc = load_led_animations(LED_ANIMATIONS_PATH).await;
    doc.animations.retain(|existing| !existing.name.eq_ignore_ascii_case(name));
    doc.animations.push(entry);
    doc.animations.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));

    persist_led_animations(LED_ANIMATIONS_PATH, &doc).await.map_err(|err| ApiError::internal(format!("failed to persist lighting animations: {err}")))?;
    persist_led_animations(LEGACY_LED_ANIMATIONS_PATH, &doc).await.map_err(|err| ApiError::internal(format!("failed to mirror lighting animations: {err}")))?;
    let _ = state.engine.refresh_node_registry().await;
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
        persist_led_animations(LEGACY_LED_ANIMATIONS_PATH, &doc).await.map_err(|err| ApiError::internal(format!("failed to mirror lighting animations: {err}")))?;
        let _ = state.engine.refresh_node_registry().await;
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn lighting_template_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HELIOS_LIGHTING_TEMPLATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        let dev = cwd.join("configs").join("lighting").join("templates");
        if dev.is_dir() {
            return dev;
        }
    }
    PathBuf::from(LIGHTING_TEMPLATE_DIR)
}

fn normalize_template_id(raw: &str) -> Option<String> {
    let sanitized = super::super::storage::sanitize_name(raw)?;
    let trimmed = sanitized.trim().trim_end_matches(".json").trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn template_raw_into_document(template_id: String, raw: LightingAnimationTemplateDocumentRaw) -> Result<LightingAnimationTemplateDocument, String> {
    let command = raw.command.unwrap_or_default();
    let frame = raw.frame.or(command.frame);
    let mut frames = raw.frames.or(raw.sequence);
    if let Some(existing_frames) = frames.as_ref()
        && existing_frames.is_empty()
    {
        return Err("sequence must include at least one frame".to_string());
    }
    let timeline = raw.timeline;
    if let Some(existing_timeline) = timeline.as_ref()
        && existing_timeline.keyframes.is_empty()
    {
        return Err("timeline must include at least one keyframe".to_string());
    }
    if frames.is_none()
        && let Some(timeline_ref) = timeline.as_ref()
    {
        let sequence = timeline_to_sequence(&timeline_ref.clone().into());
        if !sequence.is_empty() {
            frames = Some(sequence.into_iter().map(|item| LightingFramePayload { frame: item.frame.into_iter().map(Into::into).collect(), duration_ms: item.duration_ms }).collect());
        }
    }
    let brightness = raw.brightness.or(command.brightness);
    let animation = raw.animation.or(command.animation);
    if frame.is_none() && animation.is_none() && frames.is_none() && timeline.is_none() {
        return Err("template must include a frame, frames, timeline, or animation payload".to_string());
    }
    let name = raw.name.as_deref().map(str::trim).filter(|value| !value.is_empty()).unwrap_or(&template_id).to_string();
    let summary = raw.summary.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    let _raw_id = raw.id.as_deref().and_then(normalize_template_id);
    Ok(LightingAnimationTemplateDocument { id: template_id, name, summary, tags: raw.tags, frame, frames, timeline, brightness, animation, duration_ms: raw.duration_ms })
}

fn map_template_io_error(err: io::Error, context: &str) -> ApiError {
    if err.kind() == io::ErrorKind::NotFound { ApiError::not_found(format!("{context}: {err}")) } else { ApiError::internal(format!("{context}: {err}")) }
}

async fn load_lighting_config() -> LedConfig {
    let paths = led_config::default_paths();
    led_config::load_led_config(&paths).unwrap_or_default()
}

async fn persist_lighting_config(config: &LedConfig) -> ApiResult<()> {
    let doc = LightingConfigDoc { leds: config.clone() };
    let serialized = toml::to_string_pretty(&doc).map_err(|err| ApiError::bad_request(format!("failed to serialize lighting config: {err}")))?;
    let persistent_path = led_config::writable_path();
    persisted_files::write_mirrored(&persistent_path, Some(std::path::Path::new(LEGACY_LED_SETTINGS_PATH)), serialized.as_bytes())
        .await
        .map_err(|err| ApiError::internal(format!("failed to write lighting config: {err}")))?;
    Ok(())
}

fn entry_to_response(entry: LedAnimationEntry) -> LightingAnimationEntryResponse {
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
