use std::{
    fs,
    path::{Path, PathBuf},
};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Default locations searched for fan configuration.
pub const DEFAULT_FAN_CONFIG_PATHS: &[&str] = &["/var/lib/helios/fan.toml", "/etc/helios/fan.toml", "configs/presets/common.toml"];

fn default_enabled() -> bool {
    true
}

fn default_pwm_path() -> String {
    "/sys/class/hwmon/hwmon0/pwm1".into()
}

fn default_min_percent() -> u8 {
    20
}

fn default_max_percent() -> u8 {
    100
}

fn default_tacho_path() -> Option<String> {
    Some("auto".into())
}

fn default_poll_interval_ms() -> u64 {
    5_000
}

fn default_invert_pwm() -> bool {
    true
}

fn default_curve() -> Vec<FanCurvePoint> {
    vec![FanCurvePoint { temp_c: 40.0, percent: 70 }, FanCurvePoint { temp_c: 50.0, percent: 85 }, FanCurvePoint { temp_c: 60.0, percent: 100 }]
}

/// Declarative configuration for a PWM fan curve.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, ToSchema)]
pub struct FanConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Sysfs path to the pwmN file exposed by the fan driver (usually under /sys/class/hwmon).
    #[serde(default = "default_pwm_path")]
    pub pwm_path: String,
    /// Optional sysfs path that reports tachometer readings (fan*_input) in RPM.
    #[serde(default = "default_tacho_path")]
    pub tacho_path: Option<String>,
    /// Minimum duty cycle expressed as a percentage (0-100).
    #[serde(default = "default_min_percent")]
    pub min_percent: u8,
    /// Maximum duty cycle expressed as a percentage (0-100).
    #[serde(default = "default_max_percent")]
    pub max_percent: u8,
    /// Optional manual override; when set, the controller drives the fan at this percentage.
    #[serde(default)]
    pub manual_percent: Option<u8>,
    /// Poll interval for recomputing the fan target, in milliseconds.
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Whether PWM polarity should be inverted (100% duty = 0% fan speed).
    #[serde(default = "default_invert_pwm")]
    pub invert_pwm: bool,
    /// Sorted temperature breakpoints and corresponding duty cycles.
    #[serde(default = "default_curve")]
    pub curve: Vec<FanCurvePoint>,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            pwm_path: default_pwm_path(),
            tacho_path: default_tacho_path(),
            min_percent: default_min_percent(),
            max_percent: default_max_percent(),
            manual_percent: None,
            poll_interval_ms: default_poll_interval_ms(),
            invert_pwm: default_invert_pwm(),
            curve: default_curve(),
        }
    }
}

/// A single temperature-to-duty breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, ToSchema)]
pub struct FanCurvePoint {
    pub temp_c: f32,
    pub percent: u8,
}

/// Fan control mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Encode, Decode, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    Disabled,
    Manual,
    #[default]
    Curve,
}

/// Latest applied fan state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode, Default, ToSchema)]
pub struct FanStatus {
    pub mode: FanMode,
    pub target_percent: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_in_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FanDoc {
    #[serde(default)]
    fan: Option<FanConfig>,
}

/// Returns the default fan configuration search paths.
#[must_use]
pub fn default_paths() -> Vec<PathBuf> {
    DEFAULT_FAN_CONFIG_PATHS.iter().map(PathBuf::from).collect()
}

/// Preferred writable location for runtime fan overrides.
#[must_use]
pub fn writable_path() -> PathBuf {
    // Prefer a mutable location over the read-only defaults.
    PathBuf::from("/var/lib/helios/fan.toml")
}

/// Loads fan configuration from the first readable document in the provided paths.
#[must_use]
pub fn load_fan_config<P>(paths: &[P]) -> Option<FanConfig>
where
    P: AsRef<Path>,
{
    for path in paths {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(doc) = toml::from_str::<FanDoc>(&contents)
            && let Some(fan) = doc.fan
        {
            return Some(fan);
        }
    }
    None
}

/// Validates and normalizes a fan configuration.
///
/// Ensures percentages are bounded, curves are sorted/deduplicated, and inserts defaults when needed.
pub fn normalize_fan_config(mut cfg: FanConfig) -> Result<FanConfig, String> {
    if cfg.min_percent > 100 {
        return Err("min_percent must be between 0 and 100".into());
    }
    if cfg.max_percent > 100 {
        return Err("max_percent must be between 0 and 100".into());
    }
    if cfg.min_percent > cfg.max_percent {
        return Err("min_percent must be less than or equal to max_percent".into());
    }
    if cfg.poll_interval_ms == 0 {
        cfg.poll_interval_ms = default_poll_interval_ms();
    }

    if let Some(path) = cfg.tacho_path.take() {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            cfg.tacho_path = Some(trimmed.to_string());
        } else {
            cfg.tacho_path = default_tacho_path();
        }
    }

    if let Some(manual) = cfg.manual_percent.as_mut() {
        if *manual > 100 {
            return Err("manual_percent must be between 0 and 100".into());
        }
        // Manual override can drop below min_percent to allow stopping the fan.
        *manual = (*manual).min(100);
    }

    let mut curve = cfg.curve;
    curve.retain(|point| point.temp_c.is_finite());
    for point in &mut curve {
        if point.percent > 100 {
            return Err("curve percentages must be between 0 and 100".into());
        }
        point.percent = clamp_percent(point.percent, cfg.min_percent, cfg.max_percent);
    }
    curve.sort_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap_or(std::cmp::Ordering::Equal));
    curve.dedup_by(|a, b| a.temp_c == b.temp_c);
    if curve.is_empty() && cfg.enabled && cfg.manual_percent.is_none() {
        curve = default_curve();
    }
    cfg.curve = curve;
    Ok(cfg)
}

fn clamp_percent(value: u8, min: u8, max: u8) -> u8 {
    value.max(min).min(max)
}
