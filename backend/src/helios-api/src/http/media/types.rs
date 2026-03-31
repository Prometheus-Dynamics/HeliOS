use crate::api_tools_protocol::ToolCropRect;
use serde::{Deserialize, Serialize};
use url::form_urlencoded;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct MediaItem {
    pub name: String,
    pub size_bytes: u64,
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub label_attached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_input_resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_tensor_spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imu_data_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imu_data_samples: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct ListMediaParams {
    #[serde(default)]
    pub stream_id: Option<Uuid>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct DownloadMediaArchiveParams {
    #[serde(default)]
    pub name: Vec<String>,
}

pub(super) fn parse_download_media_archive_params(raw_query: Option<&str>) -> DownloadMediaArchiveParams {
    let Some(raw_query) = raw_query else {
        return DownloadMediaArchiveParams::default();
    };

    let name = form_urlencoded::parse(raw_query.as_bytes()).filter_map(|(key, value)| (key == "name").then_some(value.into_owned())).collect();
    DownloadMediaArchiveParams { name }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MediaMetadata {
    #[serde(default)]
    pub(crate) stream_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) captured_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) fps: Option<f32>,
    #[serde(default)]
    pub(crate) video_codec: Option<String>,
    #[serde(default)]
    pub(crate) label_file_name: Option<String>,
    #[serde(default)]
    pub(crate) model_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) model_input_resolution: Option<String>,
    #[serde(default)]
    pub(crate) model_tensor_spec: Option<String>,
    #[serde(default)]
    pub(crate) imu_data_file_name: Option<String>,
    #[serde(default)]
    pub(crate) imu_data_samples: Option<u64>,
    #[serde(default)]
    pub(crate) frame_timestamps_file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct UpdateMetadataRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub model_input_resolution: Option<String>,
    #[serde(default)]
    pub model_tensor_spec: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl From<CropRect> for ToolCropRect {
    fn from(value: CropRect) -> Self {
        Self { x: value.x, y: value.y, width: value.width, height: value.height }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub(crate) struct ImageEditsRequest {
    #[serde(default)]
    pub rotate_degrees: Option<i32>,
    #[serde(default)]
    pub crop: Option<CropRect>,
}

pub(super) fn media_item(name: String, size_bytes: u64, content_type: String, md: MediaMetadata) -> MediaItem {
    MediaItem {
        name,
        size_bytes,
        content_type,
        description: md.description,
        tags: md.tags,
        stream_id: md.stream_id,
        kind: md.kind,
        captured_at_ms: md.captured_at_ms,
        width: md.width,
        height: md.height,
        fps: md.fps,
        video_codec: md.video_codec,
        label_attached: md.label_file_name.is_some(),
        label_file_name: md.label_file_name,
        model_id: md.model_id,
        model_input_resolution: md.model_input_resolution,
        model_tensor_spec: md.model_tensor_spec,
        imu_data_file_name: md.imu_data_file_name,
        imu_data_samples: md.imu_data_samples,
    }
}
