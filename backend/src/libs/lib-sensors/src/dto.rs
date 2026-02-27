use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuAxesPayload {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuOrientationPayload {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quaternion: Option<ImuQuaternionPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuQuaternionPayload {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuSourcesPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel_gyro: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magnetometer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuOptionsPayload {
    #[serde(default)]
    pub fusion: Vec<String>,
    #[serde(default)]
    pub range: Vec<String>,
    #[serde(default)]
    pub intervals_ms: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuStatusPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fusion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_interval_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_velocity_damp_tau_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_still_velocity_zero_tau_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_max_accel_world_mps2: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_max_speed_mps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_max_position_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_lock_position: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dt_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub has_sample: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<ImuOrientationPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_accel: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrected_world_accel_mps2: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_world: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity_delta_world: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_speed_mps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_speed_normalized: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angular_velocity_dps: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angular_speed_dps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angular_speed_normalized: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gyro_bias_dps: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_world: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_moving: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_moving_fast: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_still: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stillness_confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_contaminated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_g: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_fast_g: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_fast_threshold_g: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_noise_floor_g: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dr_confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gyro: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mag: Option<ImuAxesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<ImuSourcesPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<ImuOptionsPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ImuUpdateRequest {
    #[serde(default)]
    pub fusion: Option<String>,
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default)]
    pub update_interval_ms: Option<u64>,
    #[serde(default)]
    pub dr_velocity_damp_tau_seconds: Option<f64>,
    #[serde(default)]
    pub dr_still_velocity_zero_tau_seconds: Option<f64>,
    #[serde(default)]
    pub dr_max_accel_world_mps2: Option<f64>,
    #[serde(default)]
    pub dr_max_speed_mps: Option<f64>,
    #[serde(default)]
    pub dr_max_position_m: Option<f64>,
    #[serde(default)]
    pub dr_lock_position: Option<bool>,
    /// Align the current acceleration (gravity) direction to a chassis axis (e.g. "+y", "-z").
    #[serde(default)]
    pub gravity_reference_axis: Option<String>,
    /// Convenience flag: align gravity to the nearest axis.
    #[serde(default)]
    pub snap_gravity: Option<bool>,
    /// Reset integrated world-frame velocity/position used for short-window dead reckoning.
    #[serde(default)]
    pub reset_pose: Option<bool>,
}
