use super::*;

pub(super) fn quat_dot(a: Quaternion, b: Quaternion) -> f32 {
    a.w * b.w + a.x * b.x + a.y * b.y + a.z * b.z
}

pub(super) fn quat_slerp(from: Quaternion, to: Quaternion, t: f32) -> Quaternion {
    let t = t.clamp(0.0, 1.0);
    let from = quat_normalize(from).unwrap_or(from);
    let mut to = quat_normalize(to).unwrap_or(to);

    // Take the shortest arc.
    let mut dot = quat_dot(from, to);
    if dot < 0.0 {
        dot = -dot;
        to = Quaternion::new(-to.w, -to.x, -to.y, -to.z);
    }

    // If very close, lerp is fine and avoids division by zero.
    if dot > 0.9995 {
        return quat_normalize(Quaternion::new(lerp(from.w, to.w, t), lerp(from.x, to.x, t), lerp(from.y, to.y, t), lerp(from.z, to.z, t))).unwrap_or(from);
    }

    let theta0 = dot.acos();
    let theta = theta0 * t;
    let sin_theta0 = theta0.sin();
    if !sin_theta0.is_finite() || sin_theta0.abs() <= f32::EPSILON {
        return from;
    }
    let s0 = (theta0 - theta).sin() / sin_theta0;
    let s1 = theta.sin() / sin_theta0;
    quat_normalize(Quaternion::new(from.w * s0 + to.w * s1, from.x * s0 + to.x * s1, from.y * s0 + to.y * s1, from.z * s0 + to.z * s1)).unwrap_or(from)
}

pub(super) fn vec3_normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let n = vec3_norm(v);
    if !n.is_finite() || n <= f32::EPSILON {
        return None;
    }
    Some([v[0] / n, v[1] / n, v[2] / n])
}

pub(super) fn lowpass_scalar(previous: f32, current: f32, dt_seconds: f32, tau_seconds: f32) -> f32 {
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 || !tau_seconds.is_finite() || tau_seconds <= 0.0 {
        return current;
    }
    let alpha = (-dt_seconds / tau_seconds).exp();
    previous * alpha + current * (1.0 - alpha)
}

pub(super) fn stillness_confidence_from_metrics(gyro_dps: f32, accel_mag_error_g: f32, motion_g: f32) -> f32 {
    let gyro_score = inverse_ramp_score(gyro_dps, 0.9, 5.0);
    let accel_mag_score = inverse_ramp_score(accel_mag_error_g, 0.02, 0.12);
    let motion_score = inverse_ramp_score(motion_g, 0.02, 0.13);
    (gyro_score * accel_mag_score * motion_score).clamp(0.0, 1.0)
}

pub(super) fn dr_confidence_from_metrics(stillness_confidence: f32, corrected_accel_world_mps2: f32, gyro_dps: f32) -> f32 {
    let accel_score = inverse_ramp_score(corrected_accel_world_mps2, DR_CONFIDENCE_ACCEL_GOOD_MPS2, DR_CONFIDENCE_ACCEL_BAD_MPS2);
    let gyro_score = inverse_ramp_score(gyro_dps, DR_CONFIDENCE_GYRO_GOOD_DPS, DR_CONFIDENCE_GYRO_BAD_DPS);
    (stillness_confidence.clamp(0.0, 1.0) * 0.55 + accel_score * 0.25 + gyro_score * 0.20).clamp(0.0, 1.0)
}

pub(super) fn rotation_leak_scale_from_gyro(gyro_dps: f32) -> f32 {
    inverse_ramp_score(gyro_dps, DR_ROTATION_LEAK_START_DPS, DR_ROTATION_LEAK_FULL_DPS).clamp(DR_ROTATION_LEAK_MIN_SCALE, 1.0)
}

pub(super) fn rotation_contamination_weight_from_gyro(gyro_dps: f32) -> f32 {
    inverse_ramp_score(gyro_dps, DR_ROTATION_CONTAMINATION_START_DPS, DR_ROTATION_CONTAMINATION_FULL_DPS).clamp(0.0, 1.0)
}

pub(super) fn inverse_ramp_score(value: f32, good_max: f32, bad_min: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    if value <= good_max {
        return 1.0;
    }
    if value >= bad_min || bad_min <= good_max {
        return 0.0;
    }
    1.0 - ((value - good_max) / (bad_min - good_max))
}

pub(super) fn vec3_norm(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub(super) fn vec3_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn vec3_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn vec3_scale(v: [f32; 3], scalar: f32) -> [f32; 3] {
    [v[0] * scalar, v[1] * scalar, v[2] * scalar]
}

pub(super) fn vec3_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn vec3_clamp_norm(v: [f32; 3], max_norm: f32) -> [f32; 3] {
    let n = vec3_norm(v);
    if !n.is_finite() || n <= f32::EPSILON || !max_norm.is_finite() || max_norm <= 0.0 || n <= max_norm {
        return v;
    }
    vec3_scale(v, max_norm / n)
}

pub(super) fn vec3_soft_deadband_norm(v: [f32; 3], deadband: f32) -> [f32; 3] {
    if !deadband.is_finite() || deadband <= 0.0 {
        return v;
    }
    let n = vec3_norm(v);
    if !n.is_finite() || n <= deadband {
        return [0.0; 3];
    }
    vec3_scale(v, (n - deadband) / n)
}

pub(super) fn lowpass_vec3(previous: [f32; 3], current: [f32; 3], dt_seconds: f32, tau_seconds: f32) -> [f32; 3] {
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 || !tau_seconds.is_finite() || tau_seconds <= 0.0 {
        return current;
    }
    let alpha = (-dt_seconds / tau_seconds).exp();
    [previous[0] * alpha + current[0] * (1.0 - alpha), previous[1] * alpha + current[1] * (1.0 - alpha), previous[2] * alpha + current[2] * (1.0 - alpha)]
}

pub(super) fn rotate_world_to_body(q: Quaternion, v_world: [f32; 3]) -> [f32; 3] {
    let q = quat_normalize(q).unwrap_or(q);
    let q_conj = Quaternion::new(q.w, -q.x, -q.y, -q.z);
    let vq = Quaternion::new(0.0, v_world[0], v_world[1], v_world[2]);
    let rotated = q_conj * vq * q;
    [rotated.x, rotated.y, rotated.z]
}

pub(super) fn rotate_vec3(q: Quaternion, v: [f32; 3]) -> [f32; 3] {
    let q = quat_normalize(q).unwrap_or(q);
    let q_conj = Quaternion::new(q.w, -q.x, -q.y, -q.z);
    let vq = Quaternion::new(0.0, v[0], v[1], v[2]);
    let rotated = q * vq * q_conj;
    [rotated.x, rotated.y, rotated.z]
}

pub(super) fn tilt_from_accel(accel: [f32; 3]) -> (f32, f32) {
    let ax = accel[0] as f64;
    let ay = accel[1] as f64;
    let az = accel[2] as f64;
    let roll = ay.atan2(az).to_degrees() as f32;
    let pitch = (-ax).atan2((ay * ay + az * az).sqrt()).to_degrees() as f32;
    (roll, pitch)
}

pub(super) fn yaw_from_accel_mag(accel: [f32; 3], mag: [f32; 3]) -> Option<f32> {
    if !mag[0].is_finite() || !mag[1].is_finite() || !mag[2].is_finite() {
        return None;
    }

    let (roll_deg, pitch_deg) = tilt_from_accel(accel);
    let (roll, pitch) = (roll_deg.to_radians() as f64, pitch_deg.to_radians() as f64);
    let (sr, cr) = roll.sin_cos();
    let (sp, cp) = pitch.sin_cos();

    let mx = mag[0] as f64;
    let my = mag[1] as f64;
    let mz = mag[2] as f64;

    // Tilt-compensated heading (yaw about +Z) for +X forward / +Y right / +Z up.
    // For a level sensor (roll=pitch=0), this reduces to yaw = atan2(-my, mx).
    let mx2 = mx * cp + mz * sp;
    let my2 = mx * sr * sp + my * cr - mz * sr * cp;
    if !mx2.is_finite() || !my2.is_finite() {
        return None;
    }
    Some((-my2).atan2(mx2).to_degrees() as f32)
}

pub(super) fn normalize_angle(mut angle: f32, range: ImuRange) -> f32 {
    match range {
        ImuRange::ZeroTo360 => {
            while angle < 0.0 {
                angle += 360.0;
            }
            while angle >= 360.0 {
                angle -= 360.0;
            }
        }
        ImuRange::Negative180To180 => {
            while angle <= -180.0 {
                angle += 360.0;
            }
            while angle > 180.0 {
                angle -= 360.0;
            }
        }
    }
    angle
}

pub(super) fn integrate_gyro(previous: Quaternion, gyro_deg_per_sec: [f32; 3], dt_seconds: f32) -> Quaternion {
    let (wx, wy, wz) = (gyro_deg_per_sec[0].to_radians(), gyro_deg_per_sec[1].to_radians(), gyro_deg_per_sec[2].to_radians());
    let omega = Quaternion::new(0.0, wx, wy, wz);
    let q_dot = quat_scale(previous * omega, 0.5);
    quat_normalize(previous + quat_scale(q_dot, dt_seconds)).unwrap_or(previous)
}

pub(super) fn euler_deg_to_quat(roll_deg: f32, pitch_deg: f32, yaw_deg: f32) -> Quaternion {
    let (roll, pitch, yaw) = (roll_deg.to_radians(), pitch_deg.to_radians(), yaw_deg.to_radians());
    let (sr, cr) = (roll * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    let (sy, cy) = (yaw * 0.5).sin_cos();

    // Z (yaw) * Y (pitch) * X (roll)
    Quaternion::new(cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy, cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy)
}

pub(super) fn quat_to_euler_deg(q: Quaternion) -> (f32, f32, f32) {
    let q = quat_normalize(q).unwrap_or(q);
    let (w, x, y, z) = (q.w as f64, q.x as f64, q.y as f64, q.z as f64);

    let sinr_cosp = 2.0 * (w * x + y * z);
    let cosr_cosp = 1.0 - 2.0 * (x * x + y * y);
    let roll = sinr_cosp.atan2(cosr_cosp);

    let sinp = 2.0 * (w * y - z * x);
    let pitch = if sinp.abs() >= 1.0 { sinp.signum() * (std::f64::consts::FRAC_PI_2) } else { sinp.asin() };

    let siny_cosp = 2.0 * (w * z + x * y);
    let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
    let yaw = siny_cosp.atan2(cosy_cosp);

    (roll.to_degrees() as f32, pitch.to_degrees() as f32, yaw.to_degrees() as f32)
}

pub(super) fn quat_scale(q: Quaternion, s: f32) -> Quaternion {
    Quaternion::new(q.w * s, q.x * s, q.y * s, q.z * s)
}

pub(super) fn quat_normalize(q: Quaternion) -> Option<Quaternion> {
    let norm_sq = q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z;
    if !norm_sq.is_finite() || norm_sq <= f32::EPSILON {
        return None;
    }
    let inv = 1.0 / norm_sq.sqrt();
    Some(Quaternion::new(q.w * inv, q.x * inv, q.y * inv, q.z * inv))
}
