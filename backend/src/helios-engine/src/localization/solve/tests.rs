use std::collections::HashMap;
use std::time::{Duration, Instant};

use nalgebra::{Quaternion, UnitQuaternion};

use super::*;
use crate::localization::config::{LocalizationPoseSpace, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSolverRuntimeTuningConfig, LocalizationTemporalStabilizationConfig};
use crate::localization::math::{PoseTransform, compose_transforms};
use crate::localization::types::{LocalizationPose, LocalizationQuaternion, LocalizationRotation, LocalizationSolverOutputs, LocalizationSolverPose, LocalizationSolverResult, LocalizationVector};

fn dummy_pose(x: f64, y: f64, z: f64) -> LocalizationPose {
    LocalizationPose {
        translation: LocalizationVector { x, y, z },
        rotation: LocalizationRotation { roll: 0.0, pitch: 0.0, yaw: 0.0, quaternion: LocalizationQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } },
    }
}

fn clear_temporal_state() {
    if let Some(store) = SOLVER_TEMPORAL_STATE.get() {
        if let Ok(mut map) = store.lock() {
            map.clear();
        }
    }
}

fn quat_from_frontend_xyz_degrees(pitch_deg: f64, yaw_deg: f64, roll_deg: f64) -> UnitQuaternion<f64> {
    let qx = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::x_axis(), pitch_deg.to_radians());
    let qy = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), yaw_deg.to_radians());
    let qz = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::z_axis(), roll_deg.to_radians());
    qx * qy * qz
}

fn heading_yaw_from_forward(rotation: UnitQuaternion<f64>) -> f64 {
    let forward = rotation.transform_vector(&nalgebra::Vector3::new(0.0, 0.0, -1.0));
    (-forward.x).atan2(-forward.z)
}

fn angle_diff_rad(a: f64, b: f64) -> f64 {
    let two_pi = 2.0 * std::f64::consts::PI;
    let mut d = (a - b).rem_euclid(two_pi);
    if d > std::f64::consts::PI {
        d -= two_pi;
    }
    d.abs()
}

fn source_sample_with_tag_ids(tag_ids: &[u32]) -> SourceSample {
    SourceSample {
        source: LocalizationSourceConfig {
            id: "src0".to_string(),
            stream_id: "stream0".to_string(),
            output_key: "tag_poses".to_string(),
            camera_uid: "cam0".to_string(),
            pose_space: None,
            input_key: None,
            enabled: true,
            weight: 1.0,
        },
        detections: tag_ids
            .iter()
            .copied()
            .map(|tag_id| crate::localization::sources::LocalizationDetection {
                source_id: "src0".to_string(),
                camera_uid: "cam0".to_string(),
                tag_id,
                camera_from_tag: PoseTransform { translation: nalgebra::Vector3::zeros(), rotation: UnitQuaternion::identity() },
                tag_size: None,
                code_rotation: None,
                tag_bits: None,
                weight: 1.0,
                quality: 1.0,
            })
            .collect(),
        pose: None,
        poll_ms: 0.0,
        tag_size: None,
        error: None,
    }
}

#[test]
fn tag_filter_applies_allowed_and_excluded_lists() {
    let profile = LocalizationProfile {
        id: "p_filter".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![1, 2, 3],
        excluded_tag_ids: vec![2],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let mut samples = vec![source_sample_with_tag_ids(&[1, 2, 3, 4])];
    apply_profile_tag_filter(&profile, &mut samples);
    let retained = samples[0].detections.iter().map(|d| d.tag_id).collect::<Vec<_>>();
    assert_eq!(retained, vec![1, 3]);
}

#[test]
fn snap_z_to_ground_clamps_field_height() {
    let profile = LocalizationProfile {
        id: "p_snap_z".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: true,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs {
            robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(1.0, -0.5, 2.0), source_ids: vec![] }),
            camera_in_field: Some(vec![crate::localization::types::LocalizationSourcePose {
                source_id: "src".to_string(),
                camera_uid: "cam".to_string(),
                weight: 1.0,
                pose: dummy_pose(0.0, 3.0, 0.0),
            }]),
            ..Default::default()
        },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let robot_y = results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.y;
    let cam_y = results[0].outputs.camera_in_field.as_ref().unwrap()[0].pose.translation.y;
    assert_eq!(robot_y, 0.0);
    assert_eq!(cam_y, 0.0);
}

#[test]
fn snap_z_to_ground_disabled_is_noop() {
    let profile = LocalizationProfile {
        id: "p_no_snap_z".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(1.0, -0.5, 2.0), source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);
    assert_eq!(results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.y, -0.5);
}

#[test]
fn field_origin_blue_offsets_field_space_outputs() {
    use crate::localization::config::LocalizationFieldOriginConfig;
    use crate::localization::maps::{FieldMapDocument, FieldMapSource};

    let profile = LocalizationProfile {
        id: "p_origin_blue".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: Some("map".to_string()),
        field_origin: LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig { enabled: false, ..Default::default() },
        sources: vec![],
        solvers: vec![],
    };

    let field_map = FieldMapDocument {
        schema_version: 1,
        id: "map".to_string(),
        name: "map".to_string(),
        width_m: 8.046,
        depth_m: 16.520,
        markers: vec![],
        source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
        overlay: None,
    };

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs {
            robot_in_field: Some(LocalizationSolverPose {
                pose: LocalizationPose {
                    translation: LocalizationVector { x: 0.377, y: 0.0, z: 2.684 },
                    rotation: LocalizationRotation { roll: 0.0, pitch: 0.0, yaw: 7.9, quaternion: LocalizationQuaternion { x: 0.0, y: 0.068843223, z: 0.0, w: 0.997627343 } },
                },
                source_ids: vec![],
            }),
            ..Default::default()
        },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, Some(&field_map), true);

    let pose = &results[0].outputs.robot_in_field.as_ref().unwrap().pose;
    assert!((pose.translation.x - 4.4).abs() < 1e-3);
    assert!((pose.translation.z - 10.944).abs() < 1e-3);
}

#[test]
fn marker_map_from_field_map_converts_heading_to_viewer_yaw() {
    use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

    let doc = FieldMapDocument {
        schema_version: 1,
        id: "map".to_string(),
        name: "map".to_string(),
        width_m: 8.0,
        depth_m: 16.0,
        markers: vec![FieldMapMarker {
            id: 15,
            family: "36h11".to_string(),
            size_m: 0.165,
            position: [0.0, 0.0, 0.0],
            quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
            heading_deg: 90.0,
            tag_bits: None,
            unique: true,
        }],
        source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
        overlay: None,
    };

    let map = marker_map_from_field_map(&doc);
    let rotation = map.markers[0].rotation.expect("rotation");
    let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
    let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
    let heading = normal.x.atan2(normal.z).to_degrees();
    assert!((heading - 90.0).abs() < 1e-6);
}

#[test]
fn marker_map_from_field_map_prefers_heading_for_limelight_maps() {
    use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

    let doc = FieldMapDocument {
        schema_version: 1,
        id: "map".to_string(),
        name: "map".to_string(),
        width_m: 8.0,
        depth_m: 16.0,
        markers: vec![
            FieldMapMarker {
                id: 1,
                family: "36h11".to_string(),
                size_m: 0.165,
                position: [0.0, 0.0, 0.0],
                quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
                heading_deg: 0.0,
                tag_bits: None,
                unique: true,
            },
            FieldMapMarker {
                id: 2,
                family: "36h11".to_string(),
                size_m: 0.165,
                position: [1.0, 0.0, 0.0],
                quaternion: FieldQuaternion { x: 0.0, y: std::f64::consts::FRAC_1_SQRT_2, z: 0.0, w: std::f64::consts::FRAC_1_SQRT_2 },
                heading_deg: 180.0,
                tag_bits: None,
                unique: true,
            },
        ],
        source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
        overlay: None,
    };

    let map = marker_map_from_field_map(&doc);
    let rotation = map.markers[1].rotation.expect("rotation");
    let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
    let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
    let heading = normal.x.atan2(normal.z).to_degrees();
    assert!((heading - 180.0).abs() < 1e-6, "limelight maps should use heading orientation");
}

#[test]
fn marker_map_from_field_map_preserves_tilted_quaternion_for_limelight_maps() {
    use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

    let doc = FieldMapDocument {
        schema_version: 1,
        id: "map".to_string(),
        name: "map".to_string(),
        width_m: 8.0,
        depth_m: 16.0,
        markers: vec![FieldMapMarker {
            id: 3,
            family: "36h11".to_string(),
            size_m: 0.165,
            position: [0.0, 0.0, 0.0],
            quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: std::f64::consts::FRAC_1_SQRT_2, w: std::f64::consts::FRAC_1_SQRT_2 },
            heading_deg: 0.0,
            tag_bits: None,
            unique: true,
        }],
        source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
        overlay: None,
    };

    let map = marker_map_from_field_map(&doc);
    let rotation = map.markers[0].rotation.expect("rotation");
    let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
    let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
    let horizontal_norm = (normal.x * normal.x + normal.z * normal.z).sqrt();
    assert!(horizontal_norm < 0.20, "tilted Limelight tags should keep quaternion tilt instead of yaw-only heading");
}

#[test]
fn marker_map_from_field_map_uses_heading_for_identity_quat_in_mixed_maps() {
    use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

    let doc = FieldMapDocument {
        schema_version: 1,
        id: "map".to_string(),
        name: "map".to_string(),
        width_m: 8.0,
        depth_m: 16.0,
        markers: vec![
            FieldMapMarker {
                id: 9,
                family: "36h11".to_string(),
                size_m: 0.165,
                position: [0.0, 0.0, 0.0],
                quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
                heading_deg: 90.0,
                tag_bits: None,
                unique: true,
            },
            FieldMapMarker {
                id: 10,
                family: "36h11".to_string(),
                size_m: 0.165,
                position: [1.0, 0.0, 0.0],
                quaternion: FieldQuaternion { x: 0.0, y: std::f64::consts::FRAC_1_SQRT_2, z: 0.0, w: std::f64::consts::FRAC_1_SQRT_2 },
                heading_deg: 180.0,
                tag_bits: None,
                unique: true,
            },
        ],
        source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
        overlay: None,
    };

    let map = marker_map_from_field_map(&doc);
    let rotation = map.markers[0].rotation.expect("rotation");
    let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
    let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
    let heading = normal.x.atan2(normal.z).to_degrees();
    assert!((heading - 90.0).abs() < 1e-6, "identity marker in mixed map should follow heading");
}

#[test]
fn snap_roll_and_pitch_to_ground_levels_field_rotation() {
    let profile = LocalizationProfile {
        id: "p_snap_rp".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: true,
        snap_pitch_to_ground: true,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let input_rotation = quat_from_frontend_xyz_degrees(-7.0, 31.0, 15.0);
    let mut pose = dummy_pose(0.0, 0.0, 0.0);
    pose.rotation.roll = 0.0;
    pose.rotation.pitch = 0.0;
    pose.rotation.yaw = 0.0;
    pose.rotation.quaternion.x = input_rotation.i;
    pose.rotation.quaternion.y = input_rotation.j;
    pose.rotation.quaternion.z = input_rotation.k;
    pose.rotation.quaternion.w = input_rotation.w;

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let rotation = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation;
    let actual = &rotation.quaternion;
    let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
    let input_heading = heading_yaw_from_forward(input_rotation);
    let output_heading = heading_yaw_from_forward(actual_q);
    assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);
    let up = actual_q.transform_vector(&nalgebra::Vector3::new(0.0, 1.0, 0.0));
    assert!(up.y > 0.999_999);
}

#[test]
fn snap_pitch_only_preserves_yaw_and_roll() {
    let profile = LocalizationProfile {
        id: "p_snap_pitch".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: true,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let input_rotation = quat_from_frontend_xyz_degrees(12.0, 170.0, -18.0);
    let mut pose = dummy_pose(0.0, 0.0, 0.0);
    pose.rotation.quaternion.x = input_rotation.i;
    pose.rotation.quaternion.y = input_rotation.j;
    pose.rotation.quaternion.z = input_rotation.k;
    pose.rotation.quaternion.w = input_rotation.w;

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let actual = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation.quaternion;
    let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
    let input_heading = heading_yaw_from_forward(input_rotation);
    let output_heading = heading_yaw_from_forward(actual_q);
    assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);

    let yaw_rotation = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), output_heading);
    let residual_in = yaw_rotation.inverse() * input_rotation;
    let residual_out = yaw_rotation.inverse() * actual_q;
    let (_in_pitch_x, _in_yaw, in_roll_z) = residual_in.euler_angles();
    let (out_pitch_x, _out_yaw, out_roll_z) = residual_out.euler_angles();
    assert!(out_pitch_x.abs() < 1e-9);
    assert!(angle_diff_rad(out_roll_z, in_roll_z) < 1e-9);
}

#[test]
fn snap_roll_only_preserves_heading_and_pitch() {
    let profile = LocalizationProfile {
        id: "p_snap_roll".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: true,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let input_rotation = quat_from_frontend_xyz_degrees(14.0, -155.0, 26.0);
    let mut pose = dummy_pose(0.0, 0.0, 0.0);
    pose.rotation.quaternion.x = input_rotation.i;
    pose.rotation.quaternion.y = input_rotation.j;
    pose.rotation.quaternion.z = input_rotation.k;
    pose.rotation.quaternion.w = input_rotation.w;

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let actual = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation.quaternion;
    let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
    let input_heading = heading_yaw_from_forward(input_rotation);
    let output_heading = heading_yaw_from_forward(actual_q);
    assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);

    let yaw_rotation = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), output_heading);
    let residual_in = yaw_rotation.inverse() * input_rotation;
    let residual_out = yaw_rotation.inverse() * actual_q;
    let (in_pitch_x, _in_yaw, _in_roll_z) = residual_in.euler_angles();
    let (out_pitch_x, _out_yaw, out_roll_z) = residual_out.euler_angles();
    assert!(angle_diff_rad(out_pitch_x, in_pitch_x) < 1e-9);
    assert!(out_roll_z.abs() < 1e-9);
}

#[test]
fn snap_roll_and_pitch_near_180_stays_upright() {
    let profile = LocalizationProfile {
        id: "p_snap_180".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: true,
        snap_pitch_to_ground: true,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let input_rotation = quat_from_frontend_xyz_degrees(20.0, 179.0, -10.0);
    let mut pose = dummy_pose(0.0, 0.0, 0.0);
    pose.rotation.quaternion.x = input_rotation.i;
    pose.rotation.quaternion.y = input_rotation.j;
    pose.rotation.quaternion.z = input_rotation.k;
    pose.rotation.quaternion.w = input_rotation.w;

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];

    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let actual = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation.quaternion;
    let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
    let input_heading = heading_yaw_from_forward(input_rotation);
    let output_heading = heading_yaw_from_forward(actual_q);
    assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);
    let up = actual_q.transform_vector(&nalgebra::Vector3::new(0.0, 1.0, 0.0));
    assert!(up.y > 0.999_999);
}

#[test]
fn camera_pose_tracks_robot_pose_via_rig_transform() {
    let profile = LocalizationProfile {
        id: "p_cam_consistency".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
        sources: vec![],
        solvers: vec![],
    };

    let mut robot = dummy_pose(1.0, 0.5, -2.0);
    robot.rotation.yaw = 90.0;
    let q = UnitQuaternion::from_euler_angles(0.0, 0.0, std::f64::consts::FRAC_PI_2);
    robot.rotation.quaternion.x = q.i;
    robot.rotation.quaternion.y = q.j;
    robot.rotation.quaternion.z = q.k;
    robot.rotation.quaternion.w = q.w;

    let camera = dummy_pose(-10.0, -10.0, -10.0);
    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField, LocalizationPoseSpace::CameraInField],
        outputs: LocalizationSolverOutputs {
            robot_in_field: Some(LocalizationSolverPose { pose: robot, source_ids: vec![] }),
            camera_in_field: Some(vec![crate::localization::types::LocalizationSourcePose { source_id: "src".to_string(), camera_uid: "cam".to_string(), weight: 1.0, pose: camera }]),
            ..Default::default()
        },
        errors: vec![],
    }];

    let mut rig_poses: HashMap<String, PoseTransform> = HashMap::new();
    let robot_from_camera = PoseTransform { translation: nalgebra::Vector3::new(0.0, 0.3048, 0.0), rotation: UnitQuaternion::identity() };
    rig_poses.insert("cam".to_string(), robot_from_camera);

    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let camera_pose = &results[0].outputs.camera_in_field.as_ref().unwrap()[0].pose;
    let expected = compose_transforms(
        &PoseTransform { translation: nalgebra::Vector3::new(1.0, 0.5, -2.0), rotation: UnitQuaternion::from_euler_angles(0.0, 0.0, std::f64::consts::FRAC_PI_2) },
        &robot_from_camera,
    );
    assert!((camera_pose.translation.x - expected.translation.x).abs() < 1e-6);
    assert!((camera_pose.translation.y - expected.translation.y).abs() < 1e-6);
    assert!((camera_pose.translation.z - expected.translation.z).abs() < 1e-6);
}

#[test]
fn temporal_stabilization_smooths_robot_pose_jumps() {
    clear_temporal_state();
    let profile = LocalizationProfile {
        id: "p_temporal".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: LocalizationTemporalStabilizationConfig {
            enabled: true,
            single_tag_translation_alpha: 0.2,
            single_tag_rotation_alpha: 0.2,
            multi_tag_translation_alpha: 0.5,
            multi_tag_rotation_alpha: 0.5,
            max_translation_jump_m: 3.0,
            max_rotation_jump_deg: 120.0,
            reanchor_reject_window_ms: 600,
        },
        sources: vec![],
        solvers: vec![LocalizationSolverConfig {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            source_ids: vec![],
            color: None,
            runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
            temporal_stabilization: None,
        }],
    };

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(0.0, 0.0, 0.0), source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];
    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();

    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);
    results[0].outputs.robot_in_field.as_mut().unwrap().pose.translation.x = 1.0;
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let x = results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.x;
    assert!(x > 0.02 && x < 0.9, "expected smoothing to keep x between prior and measurement, got {x}");
    clear_temporal_state();
}

#[test]
fn solver_temporal_override_can_disable_smoothing() {
    clear_temporal_state();
    let profile = LocalizationProfile {
        id: "p_temporal_override".to_string(),
        name: "p".to_string(),
        tag_size_m: None,
        allowed_tag_ids: vec![],
        excluded_tag_ids: vec![],
        field_map_id: None,
        field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
        snap_z_to_ground: false,
        snap_roll_to_ground: false,
        snap_pitch_to_ground: false,
        enabled: true,
        color: None,
        view_enabled: true,
        temporal_stabilization: LocalizationTemporalStabilizationConfig {
            enabled: true,
            single_tag_translation_alpha: 0.1,
            single_tag_rotation_alpha: 0.1,
            multi_tag_translation_alpha: 0.3,
            multi_tag_rotation_alpha: 0.3,
            max_translation_jump_m: 3.0,
            max_rotation_jump_deg: 120.0,
            reanchor_reject_window_ms: 600,
        },
        sources: vec![],
        solvers: vec![LocalizationSolverConfig {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            source_ids: vec![],
            color: None,
            runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
            temporal_stabilization: Some(LocalizationTemporalStabilizationConfig { enabled: false, ..LocalizationTemporalStabilizationConfig::default() }),
        }],
    };

    let mut results = vec![LocalizationSolverResult {
        id: "s".to_string(),
        name: "s".to_string(),
        mode: LocalizationSolverMode::GroupSolve,
        output_spaces: vec![LocalizationPoseSpace::RobotInField],
        outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(0.0, 0.0, 0.0), source_ids: vec![] }), ..Default::default() },
        errors: vec![],
    }];
    let rig_poses: HashMap<String, PoseTransform> = HashMap::new();

    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);
    results[0].outputs.robot_in_field.as_mut().unwrap().pose.translation.x = 1.0;
    apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

    let x = results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.x;
    assert!((x - 1.0).abs() < 1e-9, "solver override disabled smoothing, expected raw measurement, got {x}");
    clear_temporal_state();
}

#[test]
fn temporal_stabilization_damps_single_tag_switch_jump() {
    let mut state_store = HashMap::new();
    let settings = LocalizationTemporalStabilizationConfig {
        enabled: true,
        single_tag_translation_alpha: 0.2,
        single_tag_rotation_alpha: 0.2,
        multi_tag_translation_alpha: 0.5,
        multi_tag_rotation_alpha: 0.5,
        max_translation_jump_m: 1.2,
        max_rotation_jump_deg: 70.0,
        reanchor_reject_window_ms: 450,
    };
    let now = Instant::now();

    let mut first = dummy_pose(0.0, 0.0, 0.0);
    let runtime_tuning = LocalizationSolverRuntimeTuningConfig::default();
    smooth_localization_pose(
        &mut state_store,
        "k",
        &mut first,
        LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[1], solve_confidence: 0.50, now },
    );

    let mut switched = dummy_pose(1.0, 0.0, 0.0);
    smooth_localization_pose(
        &mut state_store,
        "k",
        &mut switched,
        LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[2], solve_confidence: 0.50, now: now + Duration::from_millis(33) },
    );

    assert!(switched.translation.x < 0.2, "single-tag switch should be strongly damped, got {}", switched.translation.x);
}

#[test]
fn temporal_stabilization_does_not_ping_pong_on_alternating_single_tags() {
    let mut state_store = HashMap::new();
    let settings = LocalizationTemporalStabilizationConfig {
        enabled: true,
        single_tag_translation_alpha: 0.2,
        single_tag_rotation_alpha: 0.2,
        multi_tag_translation_alpha: 0.5,
        multi_tag_rotation_alpha: 0.5,
        max_translation_jump_m: 1.2,
        max_rotation_jump_deg: 70.0,
        reanchor_reject_window_ms: 450,
    };
    let now = Instant::now();

    let mut initial = dummy_pose(0.0, 0.0, 0.0);
    let runtime_tuning = LocalizationSolverRuntimeTuningConfig::default();
    smooth_localization_pose(
        &mut state_store,
        "k",
        &mut initial,
        LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[1], solve_confidence: 0.50, now },
    );

    let mut latest = initial;
    for step in 1..=20 {
        let tag = if step % 2 == 0 { 1 } else { 2 };
        let x = if tag == 1 { -1.8 } else { 1.8 };
        latest = dummy_pose(x, 0.0, 0.0);
        smooth_localization_pose(
            &mut state_store,
            "k",
            &mut latest,
            LocalizationPoseSmoothingInput {
                settings: &settings,
                runtime_tuning: &runtime_tuning,
                tag_count: 1,
                tag_ids: &[tag],
                solve_confidence: 0.50,
                now: now + Duration::from_millis((step * 40) as u64),
            },
        );
    }

    assert!(latest.translation.x.abs() < 0.5, "alternating tags should not cause large pose ping-pong, got {}", latest.translation.x);
}

#[test]
fn temporal_stabilization_reanchors_after_persistent_single_tag_switch() {
    let mut state_store = HashMap::new();
    let settings = LocalizationTemporalStabilizationConfig {
        enabled: true,
        single_tag_translation_alpha: 0.2,
        single_tag_rotation_alpha: 0.2,
        multi_tag_translation_alpha: 0.5,
        multi_tag_rotation_alpha: 0.5,
        max_translation_jump_m: 1.2,
        max_rotation_jump_deg: 70.0,
        reanchor_reject_window_ms: 450,
    };
    let now = Instant::now();

    let mut initial = dummy_pose(0.0, 0.0, 0.0);
    let runtime_tuning = LocalizationSolverRuntimeTuningConfig::default();
    smooth_localization_pose(
        &mut state_store,
        "k",
        &mut initial,
        LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[1], solve_confidence: 0.50, now },
    );

    let mut latest = initial;
    for step in 1..=30 {
        latest = dummy_pose(1.6, 0.0, 0.0);
        smooth_localization_pose(
            &mut state_store,
            "k",
            &mut latest,
            LocalizationPoseSmoothingInput {
                settings: &settings,
                runtime_tuning: &runtime_tuning,
                tag_count: 1,
                tag_ids: &[2],
                solve_confidence: 0.50,
                now: now + Duration::from_millis((step * 50) as u64),
            },
        );
    }

    assert!(latest.translation.x > 1.0, "persistent single-tag regime should eventually re-anchor, got {}", latest.translation.x);
}
