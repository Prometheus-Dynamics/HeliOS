use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct LightingColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    #[serde(default)]
    pub w: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LightingAnimation {
    Off,
    Chase {
        color: LightingColor,
        #[serde(default = "default_speed_hz")]
        speed_hz: f32,
    },
    Pulse {
        color: LightingColor,
        #[serde(default)]
        low: u8,
        #[serde(default = "default_full_intensity")]
        high: u8,
        #[serde(default = "default_period_ms")]
        period_ms: u32,
    },
    Rainbow {
        #[serde(default = "default_speed_hz")]
        speed_hz: f32,
    },
    BreathingRainbow {
        #[serde(default = "default_speed_hz")]
        speed_hz: f32,
        #[serde(default)]
        low: u8,
        #[serde(default = "default_full_intensity")]
        high: u8,
        #[serde(default = "default_period_ms")]
        period_ms: u32,
    },
}

fn default_speed_hz() -> f32 {
    8.0
}

fn default_period_ms() -> u32 {
    1200
}

fn default_full_intensity() -> u8 {
    255
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct LightingCommand {
    #[serde(default)]
    pub frame: Option<Vec<LightingColor>>,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub animation: Option<LightingAnimation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct LightingRuntimeState {
    #[serde(default)]
    pub command: LightingCommand,
    #[serde(default)]
    pub updated_at_ms: u64,
}
