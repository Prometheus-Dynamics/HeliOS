use lib_sensors::dto::ImuUpdateRequest;
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use crate::http::error::ApiError;

#[derive(Default)]
pub(super) struct ImuUpdate {
    fusion: Option<String>,
    range: Option<String>,
    update_interval_ms: Option<u64>,
    dr_velocity_damp_tau_seconds: Option<f64>,
    dr_still_velocity_zero_tau_seconds: Option<f64>,
    dr_max_accel_world_mps2: Option<f64>,
    dr_max_speed_mps: Option<f64>,
    dr_max_position_m: Option<f64>,
    dr_lock_position: Option<bool>,
    gravity_reference_axis: Option<String>,
    snap_gravity: Option<bool>,
    reset_pose: Option<bool>,
}

impl ImuUpdate {
    pub(super) fn try_from_request(req: ImuUpdateRequest) -> Result<Self, Box<ApiError>> {
        let fusion = req.fusion.as_ref().and_then(|s| trimmed_string(s));
        let range = req.range.as_ref().and_then(|s| trimmed_string(s));
        let update_interval_ms = match req.update_interval_ms {
            Some(0) => return Err(Box::new(ApiError::bad_request("update_interval_ms must be greater than zero"))),
            Some(ms) => Some(ms),
            None => None,
        };
        let dr_velocity_damp_tau_seconds = parse_positive_f64(req.dr_velocity_damp_tau_seconds, "dr_velocity_damp_tau_seconds")?;
        let dr_still_velocity_zero_tau_seconds = parse_positive_f64(req.dr_still_velocity_zero_tau_seconds, "dr_still_velocity_zero_tau_seconds")?;
        let dr_max_accel_world_mps2 = parse_positive_f64(req.dr_max_accel_world_mps2, "dr_max_accel_world_mps2")?;
        let dr_max_speed_mps = parse_positive_f64(req.dr_max_speed_mps, "dr_max_speed_mps")?;
        let dr_max_position_m = parse_positive_f64(req.dr_max_position_m, "dr_max_position_m")?;
        let dr_lock_position = req.dr_lock_position;
        let gravity_reference_axis = req.gravity_reference_axis.as_ref().and_then(|s| trimmed_string(s));
        if let Some(axis) = gravity_reference_axis.as_deref()
            && !is_valid_axis_reference(axis)
        {
            return Err(Box::new(ApiError::bad_request("gravity_reference_axis must be one of: +x, -x, +y, -y, +z, -z")));
        }
        let snap_gravity = req.snap_gravity.filter(|value| *value);
        let reset_pose = req.reset_pose.filter(|value| *value);

        Ok(Self {
            fusion,
            range,
            update_interval_ms,
            dr_velocity_damp_tau_seconds,
            dr_still_velocity_zero_tau_seconds,
            dr_max_accel_world_mps2,
            dr_max_speed_mps,
            dr_max_position_m,
            dr_lock_position,
            gravity_reference_axis,
            snap_gravity,
            reset_pose,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.fusion.is_none()
            && self.range.is_none()
            && self.update_interval_ms.is_none()
            && self.dr_velocity_damp_tau_seconds.is_none()
            && self.dr_still_velocity_zero_tau_seconds.is_none()
            && self.dr_max_accel_world_mps2.is_none()
            && self.dr_max_speed_mps.is_none()
            && self.dr_max_position_m.is_none()
            && self.dr_lock_position.is_none()
            && self.gravity_reference_axis.is_none()
            && self.snap_gravity.is_none()
            && self.reset_pose.is_none()
    }

    pub(super) fn into_json(self) -> JsonValue {
        let mut map = JsonMap::new();
        if let Some(fusion) = self.fusion {
            map.insert("fusion".into(), json!(fusion));
        }
        if let Some(range) = self.range {
            map.insert("range".into(), json!(range));
        }
        if let Some(ms) = self.update_interval_ms {
            map.insert("update_interval_ms".into(), json!(ms));
        }
        if let Some(value) = self.dr_velocity_damp_tau_seconds {
            map.insert("dr_velocity_damp_tau_seconds".into(), json!(value));
        }
        if let Some(value) = self.dr_still_velocity_zero_tau_seconds {
            map.insert("dr_still_velocity_zero_tau_seconds".into(), json!(value));
        }
        if let Some(value) = self.dr_max_accel_world_mps2 {
            map.insert("dr_max_accel_world_mps2".into(), json!(value));
        }
        if let Some(value) = self.dr_max_speed_mps {
            map.insert("dr_max_speed_mps".into(), json!(value));
        }
        if let Some(value) = self.dr_max_position_m {
            map.insert("dr_max_position_m".into(), json!(value));
        }
        if let Some(value) = self.dr_lock_position {
            map.insert("dr_lock_position".into(), json!(value));
        }
        if let Some(value) = self.gravity_reference_axis {
            map.insert("gravity_reference_axis".into(), json!(value));
        }
        if let Some(value) = self.snap_gravity {
            map.insert("snap_gravity".into(), json!(value));
        }
        if let Some(value) = self.reset_pose {
            map.insert("reset_pose".into(), json!(value));
        }
        JsonValue::Object(map)
    }
}

fn trimmed_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn parse_positive_f64(value: Option<f64>, field: &str) -> Result<Option<f64>, Box<ApiError>> {
    match value {
        Some(v) if !v.is_finite() || v <= 0.0 => Err(Box::new(ApiError::bad_request(format!("{field} must be a finite number greater than zero")))),
        Some(v) => Ok(Some(v)),
        None => Ok(None),
    }
}

pub(super) fn is_valid_axis_reference(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "x" | "+x" | "x+" | "-x" | "x-" | "y" | "+y" | "y+" | "-y" | "y-" | "z" | "+z" | "z+" | "-z" | "z-")
}
