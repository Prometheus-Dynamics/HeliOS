use std::io;
use std::path::Path;

use helios_peripherals::dto::{LightingColor, LightingCommand};
use serde::{Deserialize, Serialize};

pub const LED_ANIMATIONS_PATH: &str = "/etc/helios/led-animations.json";
const DEFAULT_TIMELINE_SAMPLE_MS: u32 = 50;
const MIN_FRAME_DURATION_MS: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LedAnimationDoc {
    #[serde(default)]
    pub animations: Vec<LedAnimationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedAnimationEntry {
    pub name: String,
    #[serde(default)]
    pub command: LightingCommand,
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub sequence: Vec<LedAnimationFrame>,
    #[serde(default)]
    pub timeline: Option<LedAnimationTimeline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedAnimationFrame {
    #[serde(default)]
    pub frame: Vec<LightingColor>,
    #[serde(default)]
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LedAnimationTimeline {
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub sample_ms: Option<u32>,
    #[serde(default)]
    pub keyframes: Vec<LedTimelineKeyframe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LedTimelineKeyframe {
    #[serde(default)]
    pub time_ms: u32,
    #[serde(default)]
    pub frame: Vec<LightingColor>,
    #[serde(default)]
    pub easing: LedTimelineEasing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LedTimelineEasing {
    Step,
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl LedTimelineEasing {
    fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Step => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - ((-2.0 * t + 2.0).powi(2) / 2.0)
                }
            }
        }
    }
}

pub fn timeline_to_sequence(timeline: &LedAnimationTimeline) -> Vec<LedAnimationFrame> {
    let mut keyframes = timeline.keyframes.clone();
    keyframes.sort_by_key(|keyframe| keyframe.time_ms);

    if keyframes.is_empty() {
        return Vec::new();
    }

    if keyframes.len() == 1 {
        return vec![LedAnimationFrame { frame: keyframes[0].frame.clone(), duration_ms: timeline.duration_ms.unwrap_or(DEFAULT_TIMELINE_SAMPLE_MS).max(MIN_FRAME_DURATION_MS) }];
    }

    let sample_ms = timeline.sample_ms.unwrap_or(DEFAULT_TIMELINE_SAMPLE_MS).max(MIN_FRAME_DURATION_MS);
    let timeline_end = timeline.duration_ms.unwrap_or_else(|| keyframes.last().map(|keyframe| keyframe.time_ms).unwrap_or(0)).max(keyframes.last().map(|keyframe| keyframe.time_ms).unwrap_or(0));

    let mut sequence = Vec::new();

    for idx in 0..(keyframes.len() - 1) {
        let start = &keyframes[idx];
        let end = &keyframes[idx + 1];
        let segment_duration = end.time_ms.saturating_sub(start.time_ms).max(sample_ms);
        let mut elapsed = 0u32;

        while elapsed < segment_duration {
            let raw_t = (elapsed as f32 / segment_duration as f32).clamp(0.0, 1.0);
            let eased_t = start.easing.apply(raw_t);
            let frame = lerp_frame(&start.frame, &end.frame, eased_t);
            let remaining = segment_duration.saturating_sub(elapsed);
            let frame_duration = remaining.min(sample_ms).max(MIN_FRAME_DURATION_MS);
            sequence.push(LedAnimationFrame { frame, duration_ms: frame_duration });
            elapsed = elapsed.saturating_add(frame_duration);
        }
    }

    if let Some(last) = keyframes.last() {
        let hold_ms = timeline_end.saturating_sub(last.time_ms).max(sample_ms).max(MIN_FRAME_DURATION_MS);
        sequence.push(LedAnimationFrame { frame: last.frame.clone(), duration_ms: hold_ms });
    }

    sequence
}

fn lerp_frame(start: &[LightingColor], end: &[LightingColor], t: f32) -> Vec<LightingColor> {
    let count = start.len().max(end.len());
    (0..count)
        .map(|idx| {
            let a = start.get(idx).cloned().unwrap_or_default();
            let b = end.get(idx).cloned().unwrap_or_default();
            LightingColor { r: lerp_u8(a.r, b.r, t), g: lerp_u8(a.g, b.g, t), b: lerp_u8(a.b, b.b, t), w: lerp_u8(a.w, b.w, t) }
        })
        .collect()
}

fn lerp_u8(start: u8, end: u8, t: f32) -> u8 {
    let start = start as f32;
    let end = end as f32;
    (start + (end - start) * t).round().clamp(0.0, 255.0) as u8
}

fn normalize_doc(mut doc: LedAnimationDoc) -> LedAnimationDoc {
    for entry in &mut doc.animations {
        if entry.sequence.is_empty()
            && let Some(timeline) = entry.timeline.as_ref()
        {
            entry.sequence = timeline_to_sequence(timeline);
        }
    }
    doc
}

pub fn load_led_animations_sync(path: impl AsRef<Path>) -> LedAnimationDoc {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path);
    match contents {
        Ok(raw) => normalize_doc(serde_json::from_str::<LedAnimationDoc>(&raw).unwrap_or_default()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => normalize_doc(LedAnimationDoc::default()),
        Err(_) => normalize_doc(LedAnimationDoc::default()),
    }
}

pub async fn load_led_animations(path: impl AsRef<Path>) -> LedAnimationDoc {
    let path = path.as_ref();
    match tokio::fs::read_to_string(path).await {
        Ok(raw) => normalize_doc(serde_json::from_str::<LedAnimationDoc>(&raw).unwrap_or_default()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => normalize_doc(LedAnimationDoc::default()),
        Err(_) => normalize_doc(LedAnimationDoc::default()),
    }
}

pub async fn persist_led_animations(path: impl AsRef<Path>, doc: &LedAnimationDoc) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let serialized = serde_json::to_string_pretty(doc).unwrap_or_else(|_| "{\"animations\":[]}".to_string());
    tokio::fs::write(path, serialized).await
}
