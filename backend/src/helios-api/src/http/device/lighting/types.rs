use helios_peripherals::dto::{LightingAnimation, LightingColor, LightingRuntimeState};
use lib_led_animations::{LedAnimationTimeline, LedTimelineEasing, LedTimelineKeyframe};
use lib_sensors::led_config::LedConfig;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct LightingConfigRequest {
    pub lighting: LedConfig,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct LightingFramePayload {
    #[serde(default)]
    pub frame: Vec<LightingColorPayload>,
    #[serde(default)]
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
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
