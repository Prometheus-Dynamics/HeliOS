use super::*;

pub(super) const MADGWICK_BETA: f32 = 0.06;
pub(super) const I2C_INIT_RETRIES: usize = 6;
pub(super) const I2C_INIT_RETRY_SLEEP_MS: u64 = 30;
pub(super) const FUSION_ACCEL_LP_TAU_SECONDS: f32 = 0.35;
pub(super) const FUSION_ACCEL_LP_TAU_FAST_SECONDS: f32 = 0.18;
pub(super) const FUSION_MAG_LP_TAU_SECONDS: f32 = 0.60;
pub(super) const FUSION_LINEAR_ACCEL_LP_TAU_SECONDS: f32 = 0.12;
pub(super) const STILL_GYRO_LP_MAX_DPS: f32 = 2.5;
// Allow bias estimation to converge even if there's a few dps of constant gyro offset.
pub(super) const STILL_GYRO_LP_MAX_DPS_FOR_BIAS: f32 = 8.0;
// Require a tighter gyro threshold before we "re-level" against gravity.
pub(super) const STILL_GYRO_LP_MAX_DPS_FOR_RELEVEL: f32 = 1.2;
pub(super) const STILL_ACCEL_MAG_ERROR_G: f32 = 0.05;
pub(super) const STILL_LINEAR_ACCEL_LAX_G: f32 = 0.06;
pub(super) const RELEVEL_ACCEL_MAG_ERROR_G: f32 = 0.08;
pub(super) const RELEVEL_LINEAR_ACCEL_LAX_G: f32 = 0.12;
pub(super) const RELEVEL_GYRO_LP_MAX_DPS: f32 = 2.0;
pub(super) const MOTION_FAST_NOISE_TAU_SECONDS: f32 = 1.6;
pub(super) const MOTION_FAST_FLOOR_MIN_G: f32 = 0.004;
pub(super) const MOTION_FAST_FLOOR_MAX_G: f32 = 0.08;
pub(super) const MOTION_FAST_ENTER_SCALE: f32 = 1.35;
pub(super) const MOTION_FAST_ENTER_BIAS_G: f32 = 0.004;
pub(super) const MOTION_FAST_EXIT_SCALE: f32 = 1.10;
pub(super) const MOTION_FAST_EXIT_BIAS_G: f32 = 0.002;
pub(super) const MOTION_FAST_THRESHOLD_MIN_G: f32 = 0.007;
pub(super) const MOTION_FAST_THRESHOLD_MAX_G: f32 = 0.08;
pub(super) const MOTION_ROTATION_THRESHOLD_BOOST_START_DPS: f32 = 2.5;
pub(super) const MOTION_ROTATION_THRESHOLD_BOOST_FULL_DPS: f32 = 20.0;
pub(super) const MOTION_ROTATION_THRESHOLD_BOOST_SCALE: f32 = 1.40;
pub(super) const MOTION_ROTATION_THRESHOLD_BOOST_BIAS_G: f32 = 0.006;
pub(super) const MOTION_ROTATION_RELEASE_BOOST_SCALE: f32 = 1.25;
pub(super) const MOTION_ROTATION_RELEASE_BOOST_BIAS_G: f32 = 0.003;
pub(super) const GYRO_BIAS_TAU_SECONDS: f32 = 3.0;
pub(super) const GYRO_BIAS_TAU_FAST_SECONDS: f32 = 0.6;
pub(super) const GYRO_BIAS_FAST_WINDOW_SECONDS: f32 = 0.5;
pub(super) const ACCEL_BIAS_TAU_SECONDS: f32 = 1.60;
pub(super) const ACCEL_BIAS_MAX_G: f32 = 0.08;
// Note: do not gate bias estimation on the *current* bias-corrected gyro magnitude.
// Bias estimation must be able to start from a zero bias estimate and converge even if the
// device has a few dps of constant gyro offset (see `STILL_GYRO_LP_MAX_DPS_FOR_BIAS`).
// After detecting motion, hold off bias updates briefly to avoid learning the deceleration tail.
pub(super) const POST_MOTION_BIAS_HOLD_SECONDS: f32 = 0.20;
pub(super) const GYRO_STILL_LP_TAU_SECONDS: f32 = 0.45;
pub(super) const FAN_REJECT_ACCEL_LP_TAU_SECONDS: f32 = 0.75;
pub(super) const STILL_LINEAR_ACCEL_LP_TAU_SECONDS: f32 = 0.28;
pub(super) const RELEVEL_MIN_STILL_SECONDS: f32 = 0.18;
pub(super) const RELEVEL_TAU_SECONDS: f32 = 0.18;
pub(super) const STANDARD_GRAVITY_MPS2: f32 = 9.806_65;
pub(super) const DR_VELOCITY_DAMP_TAU_SECONDS: f32 = 0.55;
pub(super) const DR_VELOCITY_DAMP_TAU_MAX_EFFECTIVE: f32 = 1.20;
pub(super) const DR_STILL_VELOCITY_ZERO_TAU_SECONDS: f32 = 0.10;
pub(super) const DR_MAX_ACCEL_WORLD_MPS2: f32 = 6.0;
pub(super) const DR_MAX_SPEED_MPS: f32 = 4.0;
pub(super) const DR_MAX_POSITION_M: f32 = 2.0;
pub(super) const DR_LOCK_POSITION_DEFAULT: bool = true;
pub(super) const DR_WORLD_ACCEL_LP_TAU_SECONDS: f32 = 0.025;
pub(super) const DR_WORLD_ACCEL_BIAS_TAU_SECONDS: f32 = 1.20;
pub(super) const DR_WORLD_ACCEL_BIAS_XY_DECAY_TAU_SECONDS: f32 = 0.45;
pub(super) const DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2: f32 = 0.020;
pub(super) const DR_WORLD_ACCEL_BIAS_Z_MAX_MPS2: f32 = 0.140;
pub(super) const DR_WORLD_ACCEL_BIAS_XY_APPLY_WHILE_MOVING_SCALE: f32 = 0.30;
pub(super) const DR_WORLD_ACCEL_DEADBAND_MPS2: f32 = 0.028;
pub(super) const DR_ZUPT_RELEASE_ACCEL_MPS2: f32 = 0.075;
pub(super) const DR_COAST_GYRO_MAX_DPS: f32 = 4.5;
pub(super) const DR_STOP_SNAP_HOLD_SECONDS: f32 = 0.08;
pub(super) const DR_STOP_SNAP_SPEED_MPS: f32 = 0.20;
pub(super) const DR_ROTATION_LEAK_START_DPS: f32 = 70.0;
pub(super) const DR_ROTATION_LEAK_FULL_DPS: f32 = 220.0;
pub(super) const DR_ROTATION_LEAK_MIN_SCALE: f32 = 0.35;
pub(super) const DR_ROTATION_CONTAMINATION_START_DPS: f32 = 20.0;
pub(super) const DR_ROTATION_CONTAMINATION_FULL_DPS: f32 = 60.0;
pub(super) const DR_ROTATION_TRANSLATION_DEADBAND_MPS2: f32 = 0.045;
pub(super) const DR_ROTATION_TRANSLATION_SPEED_CAP_MPS: f32 = 0.85;
pub(super) const DR_ROTATION_RECOVERY_HOLD_SECONDS: f32 = 0.20;
pub(super) const DR_ROTATION_ACCEL_MIN_SCALE: f32 = 0.25;
pub(super) const DR_DIRECTION_CHANGE_MIN_SPEED_MPS: f32 = 0.08;
pub(super) const DR_DIRECTION_CHANGE_OPPOSING_ACCEL_MPS2: f32 = 0.045;
pub(super) const DR_DIRECTION_CHANGE_HOLD_SECONDS: f32 = 0.10;
pub(super) const DR_DIRECTION_CHANGE_DEADBAND_SCALE: f32 = 0.40;
pub(super) const DR_MOVING_EVIDENCE_ACCEL_MPS2: f32 = 0.040;
pub(super) const DR_DIRECTION_CHANGE_VELOCITY_BRAKE_SCALE: f32 = 0.75;
pub(super) const DR_GRAVITY_AXIS_LEAK_DOMINANCE_RATIO: f32 = 1.6;
pub(super) const DR_GRAVITY_AXIS_LEAK_ACCEL_MAX_MPS2: f32 = 0.35;
pub(super) const DR_GRAVITY_AXIS_LEAK_SUPPRESS_SCALE: f32 = 0.20;
pub(super) const DR_GRAVITY_AXIS_BIAS_RECOVERY_TAU_SECONDS: f32 = 0.30;
pub(super) const DR_GRAVITY_AXIS_LEAK_MOTION_FAST_MAX_G: f32 = 0.070;
pub(super) const DR_GRAVITY_AXIS_Z_VELOCITY_DAMP_TAU_SECONDS: f32 = 0.05;
pub(super) const DR_CONFIDENCE_ACCEL_GOOD_MPS2: f32 = 0.04;
pub(super) const DR_CONFIDENCE_ACCEL_BAD_MPS2: f32 = 0.30;
pub(super) const DR_CONFIDENCE_GYRO_GOOD_DPS: f32 = 0.8;
pub(super) const DR_CONFIDENCE_GYRO_BAD_DPS: f32 = 6.0;
pub(super) const STILLNESS_CONFIDENCE_LP_TAU_SECONDS: f32 = 0.10;
pub(super) const STILLNESS_CONFIDENCE_ZUPT_MIN: f32 = 0.88;
pub(super) const STILLNESS_CONFIDENCE_BIAS_MIN: f32 = 0.72;
pub(super) const STILLNESS_CONFIDENCE_IS_STILL_MIN: f32 = 0.78;
pub(super) const STILLNESS_CONFIDENCE_RELEVEL_MIN: f32 = 0.62;
pub(super) const STILLNESS_CONFIDENCE_ZUPT_HOLD_SECONDS: f32 = 0.12;

// Frame alignment correction for the BMI088/BMM150 IMU stack.
//
// Backend expects the IMU frame to be +X forward, +Y right, +Z up (right-handed).
// The physical module orientation differs from that convention; this quaternion is a
// constant rotation that brings sensor axes into the backend convention so gravity
// alignment and yaw/pitch/roll match real-world expectations.
//
// Quaternion is (w, x, y, z) and must be a proper rotation (det=+1).
pub(super) const IMU_FRAME_CORRECTION: Quaternion = Quaternion { w: 0.004_166_289, x: -0.971_540_06, y: 0.236_728_45, z: -0.007_224_368 };

/// Default polling intervals suggested for IMU updates.
pub const IMU_INTERVAL_PRESETS_MS: &[u64] = &[20, 40, 60, 80, 100, 150, 200, 250, 500, 1000];
