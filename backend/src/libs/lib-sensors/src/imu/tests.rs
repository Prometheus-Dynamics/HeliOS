use super::*;

fn approx(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

fn angle_delta_deg(a: f32, b: f32) -> f32 {
    let mut delta = a - b;
    while delta > 180.0 {
        delta -= 360.0;
    }
    while delta < -180.0 {
        delta += 360.0;
    }
    delta.abs()
}

#[test]
fn initializes_from_gravity_when_z_negative() {
    let mut state = ImuFusionState::default();
    let settings = ImuSettings {
        range: ImuRange::Negative180To180,
        update_interval: Duration::from_millis(20),
        fusion: ImuFusionMethod::Madgwick,
        yaw_offset_deg: 0.0,
        mount_correction: Quaternion::IDENTITY,
        dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
        dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
        dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
        dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
        dr_max_position_m: ImuSettings::default_dr_max_position_m(),
        dr_lock_position: ImuSettings::default_dr_lock_position(),
    };
    let out = fuse_orientation_stateful([0.0, 0.0, -1.0], [0.0, 0.0, 0.0], None, 0.02, &settings, &mut state);

    assert!(approx(out.linear_accel[0], 0.0, 1e-3));
    assert!(approx(out.linear_accel[1], 0.0, 1e-3));
    assert!(approx(out.linear_accel[2], 0.0, 1e-3));

    let roll = out.orientation[0];
    assert!(approx(roll.abs(), 180.0, 2.0));
    assert!(approx(out.orientation[1], 0.0, 2.0));
}

#[test]
fn still_gravity_subtraction_prefers_filtered_accel_over_bad_quat() {
    let mut state = ImuFusionState {
        initialized: true,
        still_time_seconds: 0.6,
        quaternion: euler_deg_to_quat(90.0, 0.0, 0.0),
        accel_lp: Some([0.0, 0.0, 1.0]),
        gyro_lp: Some([0.0, 0.0, 0.0]),
        ..Default::default()
    };

    let settings = ImuSettings {
        range: ImuRange::Negative180To180,
        update_interval: Duration::from_millis(20),
        fusion: ImuFusionMethod::Madgwick,
        yaw_offset_deg: 0.0,
        mount_correction: Quaternion::IDENTITY,
        dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
        dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
        dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
        dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
        dr_max_position_m: ImuSettings::default_dr_max_position_m(),
        dr_lock_position: ImuSettings::default_dr_lock_position(),
    };
    let out = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, 0.02, &settings, &mut state);

    assert!(out.motion_g < 0.02);
    assert!(!out.is_moving);
}

#[test]
fn fast_motion_does_not_cause_long_post_stop_yaw_drift() {
    let mut state = ImuFusionState::default();
    let settings = ImuSettings {
        range: ImuRange::Negative180To180,
        update_interval: Duration::from_millis(20),
        fusion: ImuFusionMethod::Madgwick,
        yaw_offset_deg: 0.0,
        mount_correction: Quaternion::IDENTITY,
        dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
        dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
        dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
        dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
        dr_max_position_m: ImuSettings::default_dr_max_position_m(),
        dr_lock_position: ImuSettings::default_dr_lock_position(),
    };

    let dt = 0.02;
    for _ in 0..10 {
        fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, dt, &settings, &mut state);
    }

    let yaw_rate_dps = 180.0;
    let gyro_bias_dps = 5.0;
    for _ in 0..10 {
        fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, yaw_rate_dps + gyro_bias_dps], None, dt, &settings, &mut state);
    }

    let out_at_stop = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, gyro_bias_dps], None, dt, &settings, &mut state);
    let yaw_at_stop = out_at_stop.orientation[2];

    let mut yaw_after = yaw_at_stop;
    for _ in 0..50 {
        let out = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, gyro_bias_dps], None, dt, &settings, &mut state);
        yaw_after = out.orientation[2];
    }

    assert!(angle_delta_deg(yaw_after, yaw_at_stop) < 4.0);
}

#[test]
fn yaw_offset_is_applied_to_output() {
    let mut state = ImuFusionState::default();
    let settings = ImuSettings {
        range: ImuRange::Negative180To180,
        update_interval: Duration::from_millis(20),
        fusion: ImuFusionMethod::Madgwick,
        yaw_offset_deg: 30.0,
        mount_correction: Quaternion::IDENTITY,
        dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
        dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
        dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
        dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
        dr_max_position_m: ImuSettings::default_dr_max_position_m(),
        dr_lock_position: ImuSettings::default_dr_lock_position(),
    };

    let dt = 0.02;
    for _ in 0..5 {
        fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, dt, &settings, &mut state);
    }

    let out = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, dt, &settings, &mut state);
    assert!(angle_delta_deg(out.orientation[2], 30.0) < 1.0);

    let q = Quaternion::new(out.quaternion[0], out.quaternion[1], out.quaternion[2], out.quaternion[3]);
    let (_, _, yaw) = quat_to_euler_deg(q);
    assert!(angle_delta_deg(yaw, 30.0) < 1.0);
}

#[test]
fn vector_soft_deadband_preserves_multi_axis_signal() {
    let v = [0.050, 0.050, 0.000];
    let out = vec3_soft_deadband_norm(v, 0.060);
    assert!(vec3_norm(out) > 0.0);

    let small = [0.020, 0.010, 0.000];
    let small_out = vec3_soft_deadband_norm(small, 0.060);
    assert!(approx(vec3_norm(small_out), 0.0, 1e-6));
}
