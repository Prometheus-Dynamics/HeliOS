use std::path::{Path, PathBuf};

use chrono::Utc;
use image::image_dimensions;
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::AnyResult;

const MANIFEST_FILENAME: &str = "manifest.json";

#[derive(Debug, Clone)]
pub struct MediaSeedConfig<'a> {
    pub media_root: &'a Path,
    pub image_path: PathBuf,
    pub loop_mode: bool,
    pub rotation: i32,
    pub name: String,
    pub frame_rate: f32,
}

#[derive(Debug, Clone)]
pub struct MediaSeedOutcome {
    pub source: String,
    pub manifest_dir: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub async fn seed_image_sequence(config: &MediaSeedConfig<'_>) -> AnyResult<MediaSeedOutcome> {
    let image_path = if config.image_path.is_absolute() { config.image_path.clone() } else { std::env::current_dir()?.join(&config.image_path) };
    let image_path = image_path.canonicalize()?;
    let (width, height) = image_dimensions(&image_path).map_err(|err| format!("failed to read image dimensions from {}: {err}", image_path.display()))?;
    let metadata = fs::metadata(&image_path).await?;
    let size_bytes = metadata.len();

    let manifest_id = Uuid::new_v4();
    let manifest_dir = config.media_root.join("images").join(manifest_id.to_string());
    let files_dir = manifest_dir.join("files");
    fs::create_dir_all(&files_dir).await?;

    let filename = image_path.file_name().map(|name| name.to_string_lossy().to_string()).ok_or_else(|| "image path missing filename".to_string())?;
    let destination = files_dir.join(&filename);
    fs::copy(&image_path, &destination).await?;

    let format = MediaFormatHint { width: Some(width), height: Some(height), fps: Some(config.frame_rate.max(0.1)) };
    let descriptor = MediaFileDescriptor {
        filename: filename.clone(),
        original_name: Some(filename.clone()),
        size_bytes: Some(size_bytes),
        width: Some(width),
        height: Some(height),
        role: MediaFileRole::Original,
        mime_type: Some("image/jpeg".into()),
    };

    let manifest = MediaManifest {
        id: manifest_id,
        kind: MediaKind::Image,
        name: config.name.clone(),
        description: None,
        tags: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        loop_mode: config.loop_mode,
        rotation: config.rotation,
        files: vec![descriptor],
        format,
        preview_file: None,
        image_crop: None,
        video_clip: None,
        label_file: None,
        model_input_resolution: None,
        model_tensor_spec: None,
        manifest_path: None,
        camera_source: None,
    };

    let manifest_path = manifest_dir.join(MANIFEST_FILENAME);
    let data = serde_json::to_vec_pretty(&manifest)?;
    fs::write(&manifest_path, data).await?;

    let source = format!("media://{}/{manifest_id}", MediaKind::Image.slug());
    Ok(MediaSeedOutcome { source, manifest_dir, width, height })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Video,
    Model,
}

impl MediaKind {
    pub fn slug(&self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
            MediaKind::Model => "model",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaFileRole {
    Original,
    Label,
    Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaFormatHint {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFileDescriptor {
    pub filename: String,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    pub role: MediaFileRole,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaManifest {
    pub id: Uuid,
    pub kind: MediaKind,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub loop_mode: bool,
    pub rotation: i32,
    pub files: Vec<MediaFileDescriptor>,
    pub format: MediaFormatHint,
    #[serde(default)]
    pub preview_file: Option<String>,
    #[serde(default)]
    pub image_crop: Option<ImageCrop>,
    #[serde(default)]
    pub video_clip: Option<VideoClip>,
    #[serde(default)]
    pub label_file: Option<String>,
    #[serde(default)]
    pub model_input_resolution: Option<String>,
    #[serde(default)]
    pub model_tensor_spec: Option<String>,
    #[serde(default)]
    pub manifest_path: Option<PathBuf>,
    #[serde(default)]
    pub camera_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCrop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoClip {
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}
