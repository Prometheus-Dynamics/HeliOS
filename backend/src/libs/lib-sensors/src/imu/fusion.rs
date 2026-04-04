use super::*;
use lib_runtime_policy::{HELIOS_IMU_FUSION_POLICY, ResolvedImuFusionPolicy};

pub(super) struct ImuAxes {
    pub(super) accel: [f32; 3],
    pub(super) gyro: [f32; 3],
    pub(super) mag: Option<[f32; 3]>,
}

pub(super) struct FusedStateOutput {
    pub(super) quaternion: [f32; 4],
    pub(super) orientation: [f32; 3],
    pub(super) linear_accel: [f32; 3],
    pub(super) corrected_world_accel_mps2: [f32; 3],
    pub(super) is_moving: bool,
    pub(super) is_moving_fast: bool,
    pub(super) is_still: bool,
    pub(super) stillness_confidence: f32,
    pub(super) rotation_contaminated: bool,
    pub(super) motion_g: f32,
    pub(super) motion_fast_g: f32,
    pub(super) motion_fast_threshold_g: f32,
    pub(super) motion_noise_floor_g: f32,
    pub(super) velocity_world: [f32; 3],
    pub(super) velocity_delta_world: [f32; 3],
    pub(super) linear_speed_mps: f32,
    pub(super) linear_speed_normalized: f32,
    pub(super) angular_velocity_dps: [f32; 3],
    pub(super) angular_speed_dps: f32,
    pub(super) angular_speed_normalized: f32,
    pub(super) gyro_bias_dps: [f32; 3],
    pub(super) position_world: [f32; 3],
    pub(super) dr_confidence: f32,
}

pub(super) fn remap_sensor_to_backend(v: [f32; 3]) -> [f32; 3] {
    // [x, y, z] -> [z, -y, x]
    [v[2], -v[1], v[0]]
}

pub(super) fn imu_frame_correction() -> Quaternion {
    static CACHED: OnceLock<Quaternion> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let Some(raw) = imu_fusion_policy().frame_correction_wxyz.as_deref() else {
            return IMU_FRAME_CORRECTION;
        };

        match parse_quat_wxyz(raw) {
            Some(q) => q,
            None => {
                warn!(value = %raw, "Invalid HELIOS_IMU_FRAME_CORRECTION_WXYZ; using built-in correction");
                IMU_FRAME_CORRECTION
            }
        }
    })
}

fn imu_fusion_policy() -> &'static ResolvedImuFusionPolicy {
    static CACHED: OnceLock<ResolvedImuFusionPolicy> = OnceLock::new();
    CACHED.get_or_init(|| HELIOS_IMU_FUSION_POLICY.resolve())
}

pub(super) fn parse_quat_wxyz(raw: &str) -> Option<Quaternion> {
    let mut values: Vec<f32> = raw.split(|c: char| c == ',' || c.is_whitespace()).map(str::trim).filter(|part| !part.is_empty()).filter_map(|part| part.parse::<f32>().ok()).collect();
    if values.len() != 4 {
        return None;
    }
    let q = Quaternion::new(values.remove(0), values.remove(0), values.remove(0), values.remove(0));
    quat_normalize(q)
}

pub(super) fn bmi_gyro_from_dps(dps: u16) -> BmiGyroRange {
    match dps {
        0..=125 => BmiGyroRange::Dps125,
        126..=250 => BmiGyroRange::Dps250,
        251..=500 => BmiGyroRange::Dps500,
        501..=1000 => BmiGyroRange::Dps1000,
        _ => BmiGyroRange::Dps2000,
    }
}

pub(super) fn bmi_accel_from_g(g: u16) -> BmiAccelRange {
    match g {
        0..=3 => BmiAccelRange::G3,
        4..=6 => BmiAccelRange::G6,
        7..=12 => BmiAccelRange::G12,
        _ => BmiAccelRange::G24,
    }
}

pub(super) fn fuse_orientation_stateful(
    accel_g_raw: [f32; 3],
    gyro_deg_per_sec_raw: [f32; 3],
    mag_raw: Option<[f32; 3]>,
    dt_seconds: f32,
    settings: &ImuSettings,
    state: &mut ImuFusionState,
) -> FusedStateOutput {
    let dt_seconds = dt_seconds.clamp(1e-3, 0.25);

    // Keep a low-pass estimate of gravity to reject fan vibration (high-frequency accel noise).
    // When already still, prefer a slower time constant (stronger smoothing).
    // Right after motion stops, settle faster so pitch/roll converge quickly.
    let accel_lp_tau = if state.still_time_seconds >= 0.4 {
        FAN_REJECT_ACCEL_LP_TAU_SECONDS
    } else if state.post_motion_seconds < 0.8 {
        FUSION_ACCEL_LP_TAU_FAST_SECONDS
    } else {
        FUSION_ACCEL_LP_TAU_SECONDS
    };
    let accel_lp = match state.accel_lp {
        Some(prev) => lowpass_vec3(prev, accel_g_raw, dt_seconds, accel_lp_tau),
        None => accel_g_raw,
    };
    state.accel_lp = Some(accel_lp);

    // Low-pass gyro for stillness detection and bias estimation (fan vibration can spike raw gyro).
    let gyro_lp = match state.gyro_lp {
        Some(prev) => lowpass_vec3(prev, gyro_deg_per_sec_raw, dt_seconds, GYRO_STILL_LP_TAU_SECONDS),
        None => gyro_deg_per_sec_raw,
    };
    state.gyro_lp = Some(gyro_lp);

    // Smooth magnetometer for yaw stabilization when available.
    let mag_lp = match (state.mag_lp, mag_raw) {
        (_, None) => None,
        (Some(prev), Some(current)) => Some(lowpass_vec3(prev, current, dt_seconds, FUSION_MAG_LP_TAU_SECONDS)),
        (None, Some(current)) => Some(current),
    };
    state.mag_lp = mag_lp;

    // Stillness detection uses filtered accel magnitude + filtered gyro magnitude.
    let accel_norm = vec3_norm(accel_lp);
    let accel_mag_error = (accel_norm - 1.0).abs();
    let gyro_norm = vec3_norm(gyro_deg_per_sec_raw);
    let gyro_lp_norm = vec3_norm(gyro_lp);
    let gyro_stillness_norm = gyro_norm.min(gyro_lp_norm);

    let mag_for_init = match settings.fusion {
        ImuFusionMethod::MadgwickNoMag => None,
        ImuFusionMethod::Madgwick => select_mag_for_fusion(accel_lp, mag_lp, dt_seconds, state, gyro_stillness_norm, accel_mag_error, 1.0),
    };

    let gyro_corrected = [gyro_deg_per_sec_raw[0] - state.gyro_bias_deg_per_sec[0], gyro_deg_per_sec_raw[1] - state.gyro_bias_deg_per_sec[1], gyro_deg_per_sec_raw[2] - state.gyro_bias_deg_per_sec[2]];
    let gyro_lp_corrected = [gyro_lp[0] - state.gyro_bias_deg_per_sec[0], gyro_lp[1] - state.gyro_bias_deg_per_sec[1], gyro_lp[2] - state.gyro_bias_deg_per_sec[2]];
    let gyro_lp_corrected_norm = vec3_norm(gyro_lp_corrected);

    // Initialize only once we have a reasonable gravity direction (works in any pose / any angle).
    // This avoids locking in a bad orientation when the first sample is taken mid-motion.
    if !state.initialized && accel_norm.is_finite() && accel_norm >= 0.3 {
        let (roll, pitch) = tilt_from_accel(accel_lp);
        let yaw = mag_for_init.and_then(|mag| yaw_from_accel_mag(accel_lp, mag)).unwrap_or(0.0);
        state.quaternion = euler_deg_to_quat(roll, pitch, yaw);
        state.initialized = true;
    }

    // Reduce accelerometer correction during linear acceleration (prevents tilt drift after motion).
    let gravity_prev = rotate_world_to_body(state.quaternion, [0.0, 0.0, 1.0]);
    let linear_est_prev = [accel_lp[0] - gravity_prev[0], accel_lp[1] - gravity_prev[1], accel_lp[2] - gravity_prev[2]];
    let linear_est_norm = vec3_norm(linear_est_prev);
    let mut accel_trust = (1.0 - (linear_est_norm / 0.25)).clamp(0.0, 1.0);
    // After a movement stop, linear-accel estimates can stay elevated briefly due to filtering lag.
    // If gyro indicates we've settled and accel magnitude is reasonable, restore accel trust quickly
    // so roll/pitch snap back without taking ~1s to converge.
    if gyro_stillness_norm <= 2.2 && accel_mag_error <= 0.08 {
        accel_trust = accel_trust.max(if state.post_motion_seconds < 0.8 { 0.98 } else { 0.90 });
    }
    // Boost correction gain briefly right after a stop (still gated by accel_trust).
    let beta_boost = if state.post_motion_seconds < 0.8 && gyro_stillness_norm <= 2.2 && accel_mag_error <= 0.08 { 1.35 } else { 1.0 };
    let beta = settings.beta() * beta_boost * (0.12 + 0.88 * accel_trust);

    // Madgwick 9-axis fusion (uses mag when available).
    let mag_for_fusion = match settings.fusion {
        ImuFusionMethod::MadgwickNoMag => None,
        ImuFusionMethod::Madgwick => select_mag_for_fusion(accel_lp, mag_lp, dt_seconds, state, gyro_stillness_norm, accel_mag_error, accel_trust),
    };

    let mut fused_quat = madgwick_update(state.quaternion, gyro_corrected, accel_lp, mag_for_fusion, dt_seconds, beta);
    state.quaternion = fused_quat;

    // Linear acceleration in device frame: subtract expected gravity.
    // When confidently still, use the filtered accelerometer direction as gravity (it rejects fan noise better than the fused quaternion).
    let gravity_body =
        if state.still_time_seconds >= 0.4 { vec3_normalize(accel_lp).unwrap_or_else(|| rotate_world_to_body(fused_quat, [0.0, 0.0, 1.0])) } else { rotate_world_to_body(fused_quat, [0.0, 0.0, 1.0]) };
    let linear_accel_raw_pre_bias = [accel_g_raw[0] - gravity_body[0], accel_g_raw[1] - gravity_body[1], accel_g_raw[2] - gravity_body[2]];
    let linear_accel_raw = [linear_accel_raw_pre_bias[0] - state.accel_bias_g[0], linear_accel_raw_pre_bias[1] - state.accel_bias_g[1], linear_accel_raw_pre_bias[2] - state.accel_bias_g[2]];
    let linear_tau = if state.still_time_seconds >= 0.4 { STILL_LINEAR_ACCEL_LP_TAU_SECONDS } else { FUSION_LINEAR_ACCEL_LP_TAU_SECONDS };
    state.linear_accel_lp = lowpass_vec3(state.linear_accel_lp, linear_accel_raw, dt_seconds, linear_tau);

    let motion_g = vec3_norm(state.linear_accel_lp);
    let motion_fast_g = vec3_norm(linear_accel_raw);

    // Learn the near-still accel noise floor online and derive adaptive fast-motion thresholds.
    let noise_update_gate = !state.motion_fast_active && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS && accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G;
    if noise_update_gate {
        let floor_target = motion_fast_g.clamp(MOTION_FAST_FLOOR_MIN_G, MOTION_FAST_FLOOR_MAX_G);
        state.motion_noise_floor_g = lowpass_scalar(state.motion_noise_floor_g.clamp(MOTION_FAST_FLOOR_MIN_G, MOTION_FAST_FLOOR_MAX_G), floor_target, dt_seconds, MOTION_FAST_NOISE_TAU_SECONDS);
    } else {
        state.motion_noise_floor_g = state.motion_noise_floor_g.clamp(MOTION_FAST_FLOOR_MIN_G, MOTION_FAST_FLOOR_MAX_G);
    }
    let motion_fast_threshold_g = (state.motion_noise_floor_g * MOTION_FAST_ENTER_SCALE + MOTION_FAST_ENTER_BIAS_G).clamp(MOTION_FAST_THRESHOLD_MIN_G, MOTION_FAST_THRESHOLD_MAX_G);
    let motion_fast_release_threshold_g = (state.motion_noise_floor_g * MOTION_FAST_EXIT_SCALE + MOTION_FAST_EXIT_BIAS_G).clamp(MOTION_FAST_THRESHOLD_MIN_G * 0.6, motion_fast_threshold_g * 0.9);
    let rotation_motion_boost = 1.0 - inverse_ramp_score(gyro_stillness_norm, MOTION_ROTATION_THRESHOLD_BOOST_START_DPS, MOTION_ROTATION_THRESHOLD_BOOST_FULL_DPS);
    let motion_fast_threshold_effective_g = (motion_fast_threshold_g * lerp(1.0, MOTION_ROTATION_THRESHOLD_BOOST_SCALE, rotation_motion_boost)
        + MOTION_ROTATION_THRESHOLD_BOOST_BIAS_G * rotation_motion_boost)
        .clamp(MOTION_FAST_THRESHOLD_MIN_G, MOTION_FAST_THRESHOLD_MAX_G * 2.5);
    let motion_fast_release_threshold_effective_g = (motion_fast_release_threshold_g * lerp(1.0, MOTION_ROTATION_RELEASE_BOOST_SCALE, rotation_motion_boost)
        + MOTION_ROTATION_RELEASE_BOOST_BIAS_G * rotation_motion_boost)
        .clamp(MOTION_FAST_THRESHOLD_MIN_G * 0.5, motion_fast_threshold_effective_g * 0.92);

    // Translation detector: rely on gravity-compensated accel, not gyro alone.
    // Rotating in place should not report linear movement.
    let fast_activate = motion_fast_g >= motion_fast_threshold_effective_g;
    let fast_release = motion_fast_g <= motion_fast_release_threshold_effective_g;
    if fast_activate {
        state.motion_fast_active = true;
    } else if fast_release {
        state.motion_fast_active = false;
    }

    // Bias estimation should only happen when the device is truly still.
    // In particular, do *not* update bias immediately after a quick motion stop; that tends to
    // "learn" the deceleration tail and causes post-stop drift in the direction of motion.
    let moving_candidate = state.motion_fast_active;
    if moving_candidate {
        state.post_motion_seconds = 0.0;
    } else {
        state.post_motion_seconds = (state.post_motion_seconds + dt_seconds).min(10.0);
    }
    if gyro_norm >= DR_ROTATION_CONTAMINATION_START_DPS {
        state.rotation_recovery_seconds = DR_ROTATION_RECOVERY_HOLD_SECONDS;
    } else {
        state.rotation_recovery_seconds = (state.rotation_recovery_seconds - dt_seconds).max(0.0);
    }

    let stillness_confidence_raw = stillness_confidence_from_metrics(gyro_lp_corrected_norm, accel_mag_error, motion_g);
    state.stillness_confidence_lp = lowpass_scalar(state.stillness_confidence_lp, stillness_confidence_raw, dt_seconds, STILLNESS_CONFIDENCE_LP_TAU_SECONDS);
    if state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_ZUPT_MIN {
        state.stillness_confident_time_seconds = (state.stillness_confident_time_seconds + dt_seconds).min(10.0);
    } else {
        // Decay confidence hold faster than rise so ZUPT disengages quickly after motion resumes.
        state.stillness_confident_time_seconds = (state.stillness_confident_time_seconds - dt_seconds * 1.8).max(0.0);
    }
    let confident_still = state.stillness_confident_time_seconds >= STILLNESS_CONFIDENCE_ZUPT_HOLD_SECONDS;
    let bias_hold_satisfied = state.post_motion_seconds >= POST_MOTION_BIAS_HOLD_SECONDS || confident_still;
    let bias_update_allowed = bias_hold_satisfied && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G && motion_g <= STILL_LINEAR_ACCEL_LAX_G;

    // Use the smaller of raw vs low-pass gyro for bias gating. This avoids a long "cooldown"
    // after fast motion where `gyro_lp` stays elevated for ~tau seconds, preventing bias updates.
    let still_for_bias = bias_update_allowed && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS && state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_BIAS_MIN;
    let still_for_relevel = gyro_lp_corrected_norm <= RELEVEL_GYRO_LP_MAX_DPS
        && accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G
        && motion_g <= RELEVEL_LINEAR_ACCEL_LAX_G
        && state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_RELEVEL_MIN;
    if still_for_bias {
        // Estimate bias from the filtered gyro while still; it is less noisy than raw.
        state.gyro_bias_time_seconds = (state.gyro_bias_time_seconds + dt_seconds).min(60.0);
        let bias_tau = if state.gyro_bias_time_seconds < GYRO_BIAS_FAST_WINDOW_SECONDS { GYRO_BIAS_TAU_FAST_SECONDS } else { GYRO_BIAS_TAU_SECONDS };
        let blend = 1.0 - (-dt_seconds / bias_tau).exp();
        // If raw gyro is lower than the low-pass signal (common right after motion stops),
        // prefer raw so we don't "learn" the low-pass tail as bias.
        let bias_sample = if gyro_norm <= gyro_lp_norm { gyro_deg_per_sec_raw } else { gyro_lp };
        state.gyro_bias_deg_per_sec =
            [lerp(state.gyro_bias_deg_per_sec[0], bias_sample[0], blend), lerp(state.gyro_bias_deg_per_sec[1], bias_sample[1], blend), lerp(state.gyro_bias_deg_per_sec[2], bias_sample[2], blend)];
        state.accel_bias_g = lowpass_vec3(state.accel_bias_g, linear_accel_raw_pre_bias, dt_seconds, ACCEL_BIAS_TAU_SECONDS);
        state.accel_bias_g[0] = state.accel_bias_g[0].clamp(-ACCEL_BIAS_MAX_G, ACCEL_BIAS_MAX_G);
        state.accel_bias_g[1] = state.accel_bias_g[1].clamp(-ACCEL_BIAS_MAX_G, ACCEL_BIAS_MAX_G);
        state.accel_bias_g[2] = state.accel_bias_g[2].clamp(-ACCEL_BIAS_MAX_G, ACCEL_BIAS_MAX_G);
        state.still_time_seconds = (state.still_time_seconds + dt_seconds).min(10.0);
    } else {
        state.still_time_seconds = 0.0;
    }

    if still_for_relevel {
        state.relevel_time_seconds = (state.relevel_time_seconds + dt_seconds).min(10.0);
    } else {
        state.relevel_time_seconds = 0.0;
    }

    // Without a magnetometer there is no absolute yaw reference, so post-stop gyro bias
    // cleanup needs to happen aggressively once the normal bias-update gate considers the
    // device still enough. Use a faster Z-axis path here instead of the stricter relevel
    // gate so constant post-stop gyro bias does not just integrate into yaw drift.
    if mag_for_fusion.is_none() && bias_update_allowed && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS {
        state.gyro_bias_deg_per_sec[2] = lowpass_scalar(state.gyro_bias_deg_per_sec[2], gyro_deg_per_sec_raw[2], dt_seconds, NO_MAG_STILL_YAW_BIAS_TAU_SECONDS);
    }

    if mag_for_fusion.is_none()
        && state.rotation_recovery_seconds > 0.0
        && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS
        && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G
        && motion_g <= STILL_LINEAR_ACCEL_LAX_G
    {
        let (_, _, yaw_current) = quat_to_euler_deg(fused_quat);
        let yaw_anchor = state.still_yaw_anchor_deg.get_or_insert(yaw_current);
        let (roll_current, pitch_current, _) = quat_to_euler_deg(fused_quat);
        let target = euler_deg_to_quat(roll_current, pitch_current, *yaw_anchor);
        let alpha = 1.0 - (-dt_seconds / NO_MAG_STILL_YAW_HOLD_TAU_SECONDS).exp();
        fused_quat = quat_slerp(fused_quat, target, alpha);
        state.quaternion = fused_quat;
    } else {
        state.still_yaw_anchor_deg = None;
    }

    // "Still" output should reflect bias-corrected gyro (to avoid reporting motion due to a steady bias).
    let is_still = (gyro_lp_corrected_norm <= STILL_GYRO_LP_MAX_DPS && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G && motion_g <= STILL_LINEAR_ACCEL_LAX_G)
        || (state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_IS_STILL_MIN && confident_still);

    // Stillness-driven gravity calibration: when we've been confidently still for a bit,
    // re-level roll/pitch against gravity while preserving yaw. This works for any pose
    // and does not assume a particular axis alignment or starting orientation.
    if state.relevel_time_seconds >= RELEVEL_MIN_STILL_SECONDS && accel_norm.is_finite() && accel_norm >= 0.3 && gyro_lp_corrected_norm <= STILL_GYRO_LP_MAX_DPS_FOR_RELEVEL {
        let (roll_acc, pitch_acc) = tilt_from_accel(accel_lp);
        let (_, _, yaw_current) = quat_to_euler_deg(fused_quat);
        let target = euler_deg_to_quat(roll_acc, pitch_acc, yaw_current);

        let alpha = 1.0 - (-dt_seconds / RELEVEL_TAU_SECONDS).exp();
        fused_quat = quat_slerp(fused_quat, target, alpha);
        state.quaternion = fused_quat;
    }

    let is_moving_fast = moving_candidate;

    // Best-effort dead reckoning in world frame for short windows.
    //
    // For velocity, prioritize responsiveness over heavy body-frame smoothing to reduce lag.
    // Integrate from near-raw linear acceleration in world frame, then remove a learned world-frame
    // residual bias captured during stillness to compensate gravity/cross-axis leakage.
    let mut linear_world_mps2 = rotate_vec3(fused_quat, linear_accel_raw);
    linear_world_mps2 = [linear_world_mps2[0] * STANDARD_GRAVITY_MPS2, linear_world_mps2[1] * STANDARD_GRAVITY_MPS2, linear_world_mps2[2] * STANDARD_GRAVITY_MPS2];
    linear_world_mps2 = lowpass_vec3(state.linear_world_mps2_lp, linear_world_mps2, dt_seconds, DR_WORLD_ACCEL_LP_TAU_SECONDS);
    state.linear_world_mps2_lp = linear_world_mps2;
    linear_world_mps2 = vec3_clamp_norm(linear_world_mps2, settings.dr_max_accel_world_mps2);

    let bias_reference_still = confident_still && is_still;
    let recovery_bias_update = state.rotation_recovery_seconds > 0.0 && !is_moving_fast && gyro_stillness_norm <= DR_COAST_GYRO_MAX_DPS * 1.8 && accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G;
    if bias_reference_still {
        state.world_accel_bias_mps2 = lowpass_vec3(state.world_accel_bias_mps2, linear_world_mps2, dt_seconds, DR_WORLD_ACCEL_BIAS_TAU_SECONDS);
    } else if recovery_bias_update {
        // During fast-rotation recovery, adapt the gravity-axis bias faster to cancel residual gravity leakage.
        state.world_accel_bias_mps2[2] = lowpass_scalar(state.world_accel_bias_mps2[2], linear_world_mps2[2], dt_seconds, DR_GRAVITY_AXIS_BIAS_RECOVERY_TAU_SECONDS);
    } else {
        // Lateral bias estimates can create one-direction sensitivity; decay them toward zero
        // while moving so dead-reckoning remains directionally symmetric.
        state.world_accel_bias_mps2[0] = lowpass_scalar(state.world_accel_bias_mps2[0], 0.0, dt_seconds, DR_WORLD_ACCEL_BIAS_XY_DECAY_TAU_SECONDS);
        state.world_accel_bias_mps2[1] = lowpass_scalar(state.world_accel_bias_mps2[1], 0.0, dt_seconds, DR_WORLD_ACCEL_BIAS_XY_DECAY_TAU_SECONDS);
    }
    state.world_accel_bias_mps2[0] = state.world_accel_bias_mps2[0].clamp(-DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2, DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2);
    state.world_accel_bias_mps2[1] = state.world_accel_bias_mps2[1].clamp(-DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2, DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2);
    state.world_accel_bias_mps2[2] = state.world_accel_bias_mps2[2].clamp(-DR_WORLD_ACCEL_BIAS_Z_MAX_MPS2, DR_WORLD_ACCEL_BIAS_Z_MAX_MPS2);
    let bias_apply_xy = if is_moving_fast { DR_WORLD_ACCEL_BIAS_XY_APPLY_WHILE_MOVING_SCALE } else { 1.0 };
    let corrected_world_unscaled = [
        linear_world_mps2[0] - state.world_accel_bias_mps2[0] * bias_apply_xy,
        linear_world_mps2[1] - state.world_accel_bias_mps2[1] * bias_apply_xy,
        linear_world_mps2[2] - state.world_accel_bias_mps2[2],
    ];
    let rotation_leak_scale = rotation_leak_scale_from_gyro(gyro_norm);
    let rotation_contamination_weight = rotation_contamination_weight_from_gyro(gyro_norm);
    let mut corrected_world_mps2 = vec3_scale(corrected_world_unscaled, rotation_leak_scale);
    let rotation_accel_scale = lerp(DR_ROTATION_ACCEL_MIN_SCALE, 1.0, rotation_contamination_weight);
    corrected_world_mps2 = vec3_scale(corrected_world_mps2, rotation_accel_scale);
    let rotation_contaminated = rotation_contamination_weight < 0.55;
    let velocity_before = state.velocity_world_mps;
    let speed_before = vec3_norm(velocity_before);
    let velocity_before_unit = if speed_before > 1e-4 { Some(vec3_scale(velocity_before, 1.0 / speed_before)) } else { None };
    let horizontal_norm = (corrected_world_mps2[0] * corrected_world_mps2[0] + corrected_world_mps2[1] * corrected_world_mps2[1]).sqrt();
    let vertical_abs = corrected_world_mps2[2].abs();
    let gravity_axis_leak_suspected = accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G
        && motion_fast_g <= DR_GRAVITY_AXIS_LEAK_MOTION_FAST_MAX_G
        && vertical_abs <= DR_GRAVITY_AXIS_LEAK_ACCEL_MAX_MPS2
        && vertical_abs >= horizontal_norm * DR_GRAVITY_AXIS_LEAK_DOMINANCE_RATIO
        && (state.rotation_recovery_seconds > 0.0 || gyro_stillness_norm <= DR_COAST_GYRO_MAX_DPS * 1.5 || rotation_contaminated);
    if gravity_axis_leak_suspected {
        corrected_world_mps2[2] *= DR_GRAVITY_AXIS_LEAK_SUPPRESS_SCALE;
    }
    let opposing_accel_mps2 = velocity_before_unit.map(|unit| -vec3_dot(corrected_world_mps2, unit)).unwrap_or(0.0);
    let direction_change_detected = speed_before >= DR_DIRECTION_CHANGE_MIN_SPEED_MPS && opposing_accel_mps2 >= DR_DIRECTION_CHANGE_OPPOSING_ACCEL_MPS2;
    if direction_change_detected {
        state.direction_change_hold_seconds = DR_DIRECTION_CHANGE_HOLD_SECONDS;
    } else {
        state.direction_change_hold_seconds = (state.direction_change_hold_seconds - dt_seconds).max(0.0);
    }
    let direction_change_active = direction_change_detected || state.direction_change_hold_seconds > 0.0;
    let deadband_mps2 = if rotation_contaminated { DR_ROTATION_TRANSLATION_DEADBAND_MPS2 } else { DR_WORLD_ACCEL_DEADBAND_MPS2 };
    let deadband_mps2 = if direction_change_active { deadband_mps2 * DR_DIRECTION_CHANGE_DEADBAND_SCALE } else { deadband_mps2 };
    corrected_world_mps2 = vec3_soft_deadband_norm(corrected_world_mps2, deadband_mps2);
    let corrected_world_norm = vec3_norm(corrected_world_mps2);
    let mut dr_confidence = dr_confidence_from_metrics(state.stillness_confidence_lp, corrected_world_norm, gyro_stillness_norm);
    if is_moving_fast {
        dr_confidence *= 0.90;
    }
    if rotation_contaminated {
        dr_confidence *= 0.72;
    }
    if direction_change_active {
        dr_confidence = dr_confidence.max(0.30);
    }
    let still_for_zupt = confident_still && is_still && corrected_world_norm <= DR_ZUPT_RELEASE_ACCEL_MPS2 && !direction_change_active;
    let moving_evidence = is_moving_fast || corrected_world_norm >= DR_MOVING_EVIDENCE_ACCEL_MPS2 || speed_before >= DR_DIRECTION_CHANGE_MIN_SPEED_MPS || direction_change_active;
    state.velocity_world_mps = vec3_add(state.velocity_world_mps, vec3_scale(corrected_world_mps2, dt_seconds));
    if direction_change_detected && let Some(unit) = velocity_before_unit {
        // On reversal, quickly bleed the stale component along the previous direction
        // so velocity flips with the physical motion instead of lagging by seconds.
        let stale_along_prev = vec3_dot(state.velocity_world_mps, unit);
        if stale_along_prev > 0.0 {
            state.velocity_world_mps = vec3_sub(state.velocity_world_mps, vec3_scale(unit, stale_along_prev * DR_DIRECTION_CHANGE_VELOCITY_BRAKE_SCALE));
        }
    }
    if still_for_zupt {
        let alpha_zero = 1.0 - (-dt_seconds / settings.dr_still_velocity_zero_tau_seconds.max(1e-3)).exp();
        state.velocity_world_mps = [lerp(state.velocity_world_mps[0], 0.0, alpha_zero), lerp(state.velocity_world_mps[1], 0.0, alpha_zero), lerp(state.velocity_world_mps[2], 0.0, alpha_zero)];
        if vec3_norm(state.velocity_world_mps) < 0.01 {
            state.velocity_world_mps = [0.0; 3];
        }
    } else {
        let vel_damp_tau_base = settings.dr_velocity_damp_tau_seconds.clamp(0.08, DR_VELOCITY_DAMP_TAU_MAX_EFFECTIVE);
        let settle_tau = (vel_damp_tau_base * 0.55).clamp(0.06, 0.60);
        let moving_tau = if rotation_contaminated { (vel_damp_tau_base * 1.10).clamp(0.12, 1.60) } else { (vel_damp_tau_base * 1.35).clamp(0.16, 1.80) };
        let direction_change_tau = (vel_damp_tau_base * 1.60).clamp(0.18, 2.40);
        let vel_damp_tau = if direction_change_active {
            direction_change_tau
        } else if moving_evidence {
            moving_tau
        } else {
            settle_tau
        };
        let vel_damp = (-dt_seconds / vel_damp_tau.max(1e-3)).exp();
        state.velocity_world_mps = vec3_scale(state.velocity_world_mps, vel_damp);
        if gravity_axis_leak_suspected && !direction_change_active {
            let z_alpha = 1.0 - (-dt_seconds / DR_GRAVITY_AXIS_Z_VELOCITY_DAMP_TAU_SECONDS.max(1e-3)).exp();
            state.velocity_world_mps[2] = lerp(state.velocity_world_mps[2], 0.0, z_alpha);
        }
        if !moving_evidence && state.post_motion_seconds >= DR_STOP_SNAP_HOLD_SECONDS && vec3_norm(state.velocity_world_mps) <= DR_STOP_SNAP_SPEED_MPS * 0.35 {
            state.velocity_world_mps = [0.0; 3];
        }
    }

    state.velocity_world_mps = vec3_clamp_norm(state.velocity_world_mps, settings.dr_max_speed_mps);
    if rotation_contaminated {
        state.velocity_world_mps = vec3_clamp_norm(state.velocity_world_mps, DR_ROTATION_TRANSLATION_SPEED_CAP_MPS.min(settings.dr_max_speed_mps));
    }
    if dr_confidence <= 0.12 && !moving_evidence && !direction_change_active {
        state.velocity_world_mps = [0.0; 3];
    }
    let velocity_delta_world = vec3_sub(state.velocity_world_mps, velocity_before);
    let linear_speed_mps = vec3_norm(state.velocity_world_mps);
    let is_moving = !is_still && (is_moving_fast || linear_speed_mps >= 0.04 || direction_change_active);
    let linear_speed_normalized = if settings.dr_max_speed_mps.is_finite() && settings.dr_max_speed_mps > 0.0 { (linear_speed_mps / settings.dr_max_speed_mps).clamp(0.0, 1.0) } else { 0.0 };
    let angular_speed_dps = vec3_norm(gyro_corrected);
    let angular_speed_normalized = (angular_speed_dps / DR_ROTATION_CONTAMINATION_FULL_DPS.max(1e-3)).clamp(0.0, 1.0);

    if settings.dr_lock_position {
        state.position_world_m = [0.0; 3];
    } else {
        // Position integration follows velocity directly in unlocked mode.
        // Avoid extra confidence scaling to keep pose response aligned with velocity timing.
        let velocity_for_position = state.velocity_world_mps;
        if moving_evidence || vec3_norm(velocity_for_position) >= 0.008 {
            state.position_world_m = vec3_add(state.position_world_m, vec3_scale(velocity_for_position, dt_seconds));
        }
        state.position_world_m = vec3_clamp_norm(state.position_world_m, settings.dr_max_position_m);
    }

    let output_quat = if settings.yaw_offset_deg.is_finite() && settings.yaw_offset_deg.abs() > 1e-6 {
        let yaw_offset = euler_deg_to_quat(0.0, 0.0, settings.yaw_offset_deg);
        quat_normalize(yaw_offset * fused_quat).unwrap_or(fused_quat)
    } else {
        fused_quat
    };

    // Euler angles near ±90° pitch are numerically ill-conditioned (yaw/roll coupling).
    // Prefer roll/pitch derived from gravity when accel is trustworthy so the UI doesn't show
    // a 1s \"settling\" wobble after a yaw twist.
    let (_, _, yaw) = quat_to_euler_deg(output_quat);
    let (roll, pitch) = if accel_trust >= 0.85 && gyro_stillness_norm <= 8.0 && accel_mag_error <= 0.10 {
        tilt_from_accel(accel_lp)
    } else {
        let (r, p, _) = quat_to_euler_deg(output_quat);
        (r, p)
    };
    let orientation = [normalize_angle(roll, settings.range), normalize_angle(pitch, settings.range), normalize_angle(yaw, settings.range)];

    FusedStateOutput {
        quaternion: [output_quat.w, output_quat.x, output_quat.y, output_quat.z],
        orientation,
        linear_accel: state.linear_accel_lp,
        corrected_world_accel_mps2: corrected_world_mps2,
        is_moving,
        is_moving_fast,
        is_still,
        stillness_confidence: state.stillness_confidence_lp,
        rotation_contaminated,
        motion_g,
        motion_fast_g,
        motion_fast_threshold_g: motion_fast_threshold_effective_g,
        motion_noise_floor_g: state.motion_noise_floor_g,
        velocity_world: state.velocity_world_mps,
        velocity_delta_world,
        linear_speed_mps,
        linear_speed_normalized,
        angular_velocity_dps: gyro_corrected,
        angular_speed_dps,
        angular_speed_normalized,
        gyro_bias_dps: state.gyro_bias_deg_per_sec,
        position_world: state.position_world_m,
        dr_confidence,
    }
}

pub(super) fn mag_norm_rel_tol() -> f32 {
    imu_fusion_policy().mag_norm_rel_tol
}

pub(super) fn mag_norm_lp_tau_seconds() -> f32 {
    imu_fusion_policy().mag_norm_lp_tau_seconds
}

pub(super) fn mag_min_horizontal_component() -> f32 {
    imu_fusion_policy().mag_min_horizontal
}

pub(super) fn select_mag_for_fusion(
    accel_lp: [f32; 3],
    mag_lp: Option<[f32; 3]>,
    dt_seconds: f32,
    state: &mut ImuFusionState,
    gyro_stillness_norm: f32,
    accel_mag_error: f32,
    accel_trust: f32,
) -> Option<[f32; 3]> {
    let mag = mag_lp?;
    let mag_norm = vec3_norm(mag);
    if !mag_norm.is_finite() || mag_norm <= f32::EPSILON {
        return None;
    }

    // Reject magnetometer readings that cannot provide a stable yaw (field nearly vertical).
    let gravity = vec3_normalize(accel_lp).unwrap_or([0.0, 0.0, 1.0]);
    let mag_unit = [mag[0] / mag_norm, mag[1] / mag_norm, mag[2] / mag_norm];
    let dot = mag_unit[0] * gravity[0] + mag_unit[1] * gravity[1] + mag_unit[2] * gravity[2];
    let horiz = [mag_unit[0] - gravity[0] * dot, mag_unit[1] - gravity[1] * dot, mag_unit[2] - gravity[2] * dot];
    if vec3_norm(horiz) < mag_min_horizontal_component() {
        return None;
    }

    // Learn a baseline field magnitude while confidently still, then reject large excursions
    // (common for hard/soft-iron interference, nearby motors, or moving past metal).
    let update_baseline = accel_trust >= 0.7 && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G;
    if update_baseline {
        let tau = mag_norm_lp_tau_seconds();
        state.mag_norm_lp = Some(match state.mag_norm_lp {
            Some(prev) => lowpass_scalar(prev, mag_norm, dt_seconds, tau),
            None => mag_norm,
        });
    }

    if let Some(baseline) = state.mag_norm_lp {
        let denom = baseline.abs().max(1e-6);
        let rel = ((mag_norm - baseline).abs()) / denom;
        if rel > mag_norm_rel_tol() && accel_trust >= 0.3 {
            return None;
        }
    }

    Some(mag)
}

pub(super) fn madgwick_update(previous: Quaternion, gyro_deg_per_sec: [f32; 3], accel_g: [f32; 3], mag: Option<[f32; 3]>, dt_seconds: f32, beta: f32) -> Quaternion {
    let (gx, gy, gz) = (gyro_deg_per_sec[0].to_radians(), gyro_deg_per_sec[1].to_radians(), gyro_deg_per_sec[2].to_radians());
    let (mut q1, mut q2, mut q3, mut q4) = (previous.w, previous.x, previous.y, previous.z);

    let accel_norm = vec3_norm(accel_g);
    if !accel_norm.is_finite() || accel_norm <= f32::EPSILON {
        return integrate_gyro(previous, gyro_deg_per_sec, dt_seconds);
    }
    let (ax, ay, az) = (accel_g[0] / accel_norm, accel_g[1] / accel_norm, accel_g[2] / accel_norm);

    let (mut s1, mut s2, mut s3, mut s4) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

    let mut used_mag = false;
    if let Some(mag) = mag {
        let mag_norm = vec3_norm(mag);
        if mag_norm.is_finite() && mag_norm > f32::EPSILON {
            used_mag = true;
            let (mx, my, mz) = (mag[0] / mag_norm, mag[1] / mag_norm, mag[2] / mag_norm);

            let _2q1 = 2.0 * q1;
            let _2q2 = 2.0 * q2;
            let _2q3 = 2.0 * q3;
            let _2q4 = 2.0 * q4;
            let _2q1q3 = 2.0 * q1 * q3;
            let _2q3q4 = 2.0 * q3 * q4;
            let q1q1 = q1 * q1;
            let q1q2 = q1 * q2;
            let q1q3 = q1 * q3;
            let q1q4 = q1 * q4;
            let q2q2 = q2 * q2;
            let q2q3 = q2 * q3;
            let q2q4 = q2 * q4;
            let q3q3 = q3 * q3;
            let q3q4 = q3 * q4;
            let q4q4 = q4 * q4;

            let _2q1mx = 2.0 * q1 * mx;
            let _2q1my = 2.0 * q1 * my;
            let _2q1mz = 2.0 * q1 * mz;
            let _2q2mx = 2.0 * q2 * mx;

            let hx = mx * q1q1 - _2q1my * q4 + _2q1mz * q3 + mx * q2q2 + _2q2 * my * q3 + _2q2 * mz * q4 - mx * q3q3 - mx * q4q4;
            let hy = _2q1mx * q4 + my * q1q1 - _2q1mz * q2 + _2q2mx * q3 - my * q2q2 + my * q3q3 + _2q3 * mz * q4 - my * q4q4;
            let _2bx = (hx * hx + hy * hy).sqrt();
            let _2bz = -_2q1mx * q3 + _2q1my * q2 + mz * q1q1 + _2q2mx * q4 - mz * q2q2 + _2q3 * my * q4 - mz * q3q3 + mz * q4q4;
            let _4bx = 2.0 * _2bx;
            let _4bz = 2.0 * _2bz;

            s1 = -_2q3 * (2.0 * q2q4 - _2q1q3 - ax) + _2q2 * (2.0 * q1q2 + _2q3q4 - ay) - _2bz * q3 * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (-_2bx * q4 + _2bz * q2) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + _2bx * q3 * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
            s2 = _2q4 * (2.0 * q2q4 - _2q1q3 - ax) + _2q1 * (2.0 * q1q2 + _2q3q4 - ay) - 4.0 * q2 * (1.0 - 2.0 * q2q2 - 2.0 * q3q3 - az)
                + _2bz * q4 * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (_2bx * q3 + _2bz * q1) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + (_2bx * q4 - _4bz * q2) * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
            s3 = -_2q1 * (2.0 * q2q4 - _2q1q3 - ax) + _2q4 * (2.0 * q1q2 + _2q3q4 - ay) - 4.0 * q3 * (1.0 - 2.0 * q2q2 - 2.0 * q3q3 - az)
                + (-_4bx * q3 - _2bz * q1) * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (_2bx * q2 + _2bz * q4) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + (_2bx * q1 - _4bz * q3) * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
            s4 = _2q2 * (2.0 * q2q4 - _2q1q3 - ax)
                + _2q3 * (2.0 * q1q2 + _2q3q4 - ay)
                + (-_4bx * q4 + _2bz * q2) * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (-_2bx * q1 + _2bz * q3) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + _2bx * q2 * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
        }
    }

    if !used_mag {
        let _2q1 = 2.0 * q1;
        let _2q2 = 2.0 * q2;
        let _2q3 = 2.0 * q3;
        let _2q4 = 2.0 * q4;
        let _4q1 = 4.0 * q1;
        let _4q2 = 4.0 * q2;
        let _4q3 = 4.0 * q3;
        let _8q2 = 8.0 * q2;
        let _8q3 = 8.0 * q3;
        let q1q1 = q1 * q1;
        let q2q2 = q2 * q2;
        let q3q3 = q3 * q3;
        let q4q4 = q4 * q4;

        s1 = _4q1 * q3q3 + _2q3 * ax + _4q1 * q2q2 - _2q2 * ay;
        s2 = _4q2 * q4q4 - _2q4 * ax + 4.0 * q1q1 * q2 - _2q1 * ay - _4q2 + _8q2 * q2q2 + _8q2 * q3q3 + _4q2 * az;
        s3 = 4.0 * q1q1 * q3 + _2q1 * ax + _4q3 * q4q4 - _2q4 * ay - _4q3 + _8q3 * q2q2 + _8q3 * q3q3 + _4q3 * az;
        s4 = 4.0 * q2q2 * q4 - _2q2 * ax + 4.0 * q3q3 * q4 - _2q3 * ay;
    }

    let s_norm = (s1 * s1 + s2 * s2 + s3 * s3 + s4 * s4).sqrt();
    if s_norm.is_finite() && s_norm > f32::EPSILON {
        s1 /= s_norm;
        s2 /= s_norm;
        s3 /= s_norm;
        s4 /= s_norm;
    }

    let q_dot1 = 0.5 * (-q2 * gx - q3 * gy - q4 * gz) - beta * s1;
    let q_dot2 = 0.5 * (q1 * gx + q3 * gz - q4 * gy) - beta * s2;
    let q_dot3 = 0.5 * (q1 * gy - q2 * gz + q4 * gx) - beta * s3;
    let q_dot4 = 0.5 * (q1 * gz + q2 * gy - q3 * gx) - beta * s4;

    q1 += q_dot1 * dt_seconds;
    q2 += q_dot2 * dt_seconds;
    q3 += q_dot3 * dt_seconds;
    q4 += q_dot4 * dt_seconds;

    quat_normalize(Quaternion::new(q1, q2, q3, q4)).unwrap_or(previous)
}
