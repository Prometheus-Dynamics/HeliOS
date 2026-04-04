use std::{io, path::PathBuf, sync::OnceLock};

use axum::{Json, extract::Path, response::IntoResponse};
use lib_runtime_policy::HELIOS_API_LIGHTING_TEMPLATES_POLICY;
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use tokio::fs;

use crate::http::error::{ApiError, ApiResult, ErrorBody};

use super::{LightingAnimationPayload, LightingAnimationTemplateDocument, LightingAnimationTemplateSummary, LightingColorPayload, LightingFramePayload, LightingTimelinePayload};

const CURRENT_LIGHTING_TEMPLATE_DOCUMENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct LightingAnimationTemplateDocumentRaw {
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) tags: Vec<String>,
    #[serde(default)]
    pub(super) frame: Option<Vec<LightingColorPayload>>,
    #[serde(default)]
    pub(super) frames: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    pub(super) sequence: Option<Vec<LightingFramePayload>>,
    #[serde(default)]
    pub(super) timeline: Option<LightingTimelinePayload>,
    #[serde(default)]
    pub(super) brightness: Option<u8>,
    #[serde(default)]
    pub(super) animation: Option<LightingAnimationPayload>,
    #[serde(default)]
    pub(super) duration_ms: Option<u32>,
}

use serde::Deserialize;

impl LightingAnimationTemplateDocumentRaw {
    pub(super) fn decode_str(raw: &str) -> Result<Self, String> {
        let mut value = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| format!("failed to decode lighting template: {err}"))?;
        // Built-in templates shipped in Gaia predate explicit schema tagging.
        // Treat them as current-version documents when the schema key is absent.
        if let Some(object) = value.as_object_mut()
            && !object.contains_key("schema_version")
            && !object.contains_key("schemaVersion")
        {
            object.insert("schema_version".to_string(), serde_json::Value::from(CURRENT_LIGHTING_TEMPLATE_DOCUMENT_SCHEMA_VERSION));
        }
        let migrated = normalize_to_current(value, &LIGHTING_TEMPLATE_DOCUMENT_SCHEMA_PLAN)?;
        let mut parsed: Self = serde_json::from_value(migrated).map_err(|err| format!("failed to parse lighting template: {err}"))?;
        parsed.schema_version = CURRENT_LIGHTING_TEMPLATE_DOCUMENT_SCHEMA_VERSION;
        Ok(parsed)
    }
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
        let Ok(raw) = LightingAnimationTemplateDocumentRaw::decode_str(&data) else {
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
    let raw = LightingAnimationTemplateDocumentRaw::decode_str(&data).map_err(|err| map_template_io_error(io::Error::new(io::ErrorKind::InvalidData, err), "failed to decode lighting template"))?;
    let doc = template_raw_into_document(template_id, raw).map_err(|err| map_template_io_error(io::Error::new(io::ErrorKind::InvalidData, err), "invalid lighting template"))?;
    Ok(Json(doc))
}

fn lighting_template_dir() -> PathBuf {
    static CACHED: OnceLock<PathBuf> = OnceLock::new();
    CACHED.get_or_init(|| HELIOS_API_LIGHTING_TEMPLATES_POLICY.resolve(std::env::current_dir().ok().as_deref()).template_dir).clone()
}

pub(super) fn normalize_template_id(raw: &str) -> Option<String> {
    let sanitized = crate::http::storage::sanitize_name(raw)?;
    let trimmed = sanitized.trim().trim_end_matches(".json").trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

pub(super) fn template_raw_into_document(template_id: String, raw: LightingAnimationTemplateDocumentRaw) -> Result<LightingAnimationTemplateDocument, String> {
    let frame = raw.frame;
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
        let sequence = lib_led_animations::timeline_to_sequence(&timeline_ref.clone().into());
        if !sequence.is_empty() {
            frames = Some(sequence.into_iter().map(|item| LightingFramePayload { frame: item.frame.into_iter().map(Into::into).collect(), duration_ms: item.duration_ms }).collect());
        }
    }
    let brightness = raw.brightness;
    let animation = raw.animation;
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

const LIGHTING_TEMPLATE_DOCUMENT_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("lighting template document", CURRENT_LIGHTING_TEMPLATE_DOCUMENT_SCHEMA_VERSION);
