use std::collections::HashMap;

use nalgebra::{Quaternion, UnitQuaternion};

use crate::localization::config::{LocalizationFieldOriginMode, LocalizationProfile};
use crate::localization::maps::FieldMapDocument;
use crate::localization::math::{compose_transforms, invert_transform, PoseTransform};
use crate::localization::types::{LocalizationPose, LocalizationSolverOutputs, LocalizationSolverResult};

use super::smoothing::{apply_temporal_pose_stabilization, localization_pose_components, update_localization_pose};

pub(super) fn apply_profile_postprocessing(
    profile: &LocalizationProfile,
    solver_results: &mut [LocalizationSolverResult],
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&FieldMapDocument>,
    apply_field_origin: bool,
) {
    apply_temporal_pose_stabilization(profile, solver_results);

    let snap_height = profile.snap_z_to_ground;
    let snap_roll = profile.snap_roll_to_ground;
    let snap_pitch = profile.snap_pitch_to_ground;

    if snap_height || snap_roll || snap_pitch {
        // Snap selected field-space pose components to ground-level orientation/height.
        for solver in solver_results.iter_mut() {
            let mut has_robot_pose = false;
            if let Some(pose) = solver.outputs.robot_in_field.as_mut() {
                apply_field_pose_snaps(&mut pose.pose, snap_height, snap_roll, snap_pitch);
                has_robot_pose = true;
            }

            // When a robot pose exists, camera poses are re-derived from robot+rig below to keep a
            // rigid camera/body relationship. Only snap camera outputs directly for camera-only solves.
            if !has_robot_pose {
                if let Some(list) = solver.outputs.camera_in_field.as_mut() {
                    for entry in list {
                        apply_field_pose_snaps(&mut entry.pose, snap_height, snap_roll, snap_pitch);
                    }
                }
            } else if let Some(list) = solver.outputs.camera_in_field.as_mut() {
                // Preserve legacy behavior for cameras that have no rig pose configured.
                for entry in list {
                    if !rig_poses.contains_key(&entry.camera_uid) {
                        apply_field_pose_snaps(&mut entry.pose, snap_height, snap_roll, snap_pitch);
                    }
                }
            }
        }
    }

    if apply_field_origin {
        apply_profile_field_origin(profile, field_map, solver_results);
    }

    for solver in solver_results {
        enforce_camera_pose_consistency(&mut solver.outputs, rig_poses);
    }
}

fn apply_profile_field_origin(profile: &LocalizationProfile, field_map: Option<&FieldMapDocument>, solver_results: &mut [LocalizationSolverResult]) {
    let origin_from_center = field_origin_from_center_transform(profile, field_map);
    if is_identity_transform(&origin_from_center) {
        return;
    }

    for solver in solver_results.iter_mut() {
        if let Some(robot_pose) = solver.outputs.robot_in_field.as_mut() {
            transform_localization_pose_in_place(&mut robot_pose.pose, &origin_from_center);
        }
        if let Some(camera_poses) = solver.outputs.camera_in_field.as_mut() {
            for entry in camera_poses {
                transform_localization_pose_in_place(&mut entry.pose, &origin_from_center);
            }
        }
    }
}

fn transform_localization_pose_in_place(pose: &mut LocalizationPose, parent_from_child: &PoseTransform) {
    let Some((translation, rotation)) = localization_pose_components(pose) else {
        return;
    };
    let child = PoseTransform { translation, rotation };
    let transformed = compose_transforms(parent_from_child, &child);
    update_localization_pose(pose, transformed.translation, transformed.rotation);
}

fn is_identity_transform(transform: &PoseTransform) -> bool {
    transform.translation.norm_squared() <= 1e-12 && transform.rotation.angle().abs() <= 1e-12
}

fn field_origin_from_center_transform(profile: &LocalizationProfile, field_map: Option<&FieldMapDocument>) -> PoseTransform {
    let (field_width_m, field_depth_m) = field_dimensions_for_origin(field_map);
    let half_width = field_width_m * 0.5;
    let half_depth = field_depth_m * 0.5;

    let sanitized_origin = profile.field_origin.sanitized();
    let (origin_x, origin_z, origin_yaw_deg) = match sanitized_origin.mode {
        LocalizationFieldOriginMode::Center => (0.0, 0.0, 0.0),
        LocalizationFieldOriginMode::Blue => (-half_width, -half_depth, 0.0),
        LocalizationFieldOriginMode::Red => (half_width, half_depth, 180.0),
        LocalizationFieldOriginMode::Custom => {
            if let Some(custom) = sanitized_origin.custom {
                (custom.x, custom.z, custom.yaw_deg)
            } else {
                (-half_width, -half_depth, 0.0)
            }
        }
    };

    let center_from_origin =
        PoseTransform { translation: nalgebra::Vector3::new(origin_x, 0.0, origin_z), rotation: UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), origin_yaw_deg.to_radians()) };
    invert_transform(&center_from_origin)
}

fn field_dimensions_for_origin(field_map: Option<&FieldMapDocument>) -> (f64, f64) {
    const DEFAULT_FIELD_WIDTH_M: f64 = 8.2296;
    const DEFAULT_FIELD_DEPTH_M: f64 = 16.4592;

    if let Some(map) = field_map {
        if map.width_m.is_finite() && map.width_m > 0.0 && map.depth_m.is_finite() && map.depth_m > 0.0 {
            return (map.width_m, map.depth_m);
        }
    }

    (DEFAULT_FIELD_WIDTH_M, DEFAULT_FIELD_DEPTH_M)
}

fn enforce_camera_pose_consistency(outputs: &mut LocalizationSolverOutputs, rig_poses: &HashMap<String, PoseTransform>) {
    let Some(robot_pose) = outputs.robot_in_field.as_ref() else {
        return;
    };
    let Some((robot_translation, robot_rotation)) = localization_pose_components(&robot_pose.pose) else {
        return;
    };
    let field_from_robot = PoseTransform { translation: robot_translation, rotation: robot_rotation };
    let Some(camera_outputs) = outputs.camera_in_field.as_mut() else {
        return;
    };

    for entry in camera_outputs {
        let Some(robot_from_camera) = rig_poses.get(&entry.camera_uid) else {
            continue;
        };
        let field_from_camera = compose_transforms(&field_from_robot, robot_from_camera);
        update_localization_pose(&mut entry.pose, field_from_camera.translation, field_from_camera.rotation);
    }
}

fn apply_field_pose_snaps(pose: &mut LocalizationPose, snap_height: bool, snap_roll: bool, snap_pitch: bool) {
    // Viewer frame: +Y is up.
    if snap_height {
        pose.translation.y = 0.0;
    }

    if !(snap_roll || snap_pitch) {
        return;
    }

    let q = &pose.rotation.quaternion;
    let quat_norm_sq = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    let base_rotation = if q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && q.w.is_finite() && quat_norm_sq > f64::EPSILON {
        UnitQuaternion::new_normalize(Quaternion::new(q.w, q.x, q.y, q.z))
    } else {
        // Fallback for malformed quaternion payloads: match UI semantics
        // (pitch=X, yaw=Y, roll=Z).
        UnitQuaternion::from_euler_angles(pose.rotation.pitch.to_radians(), pose.rotation.yaw.to_radians(), pose.rotation.roll.to_radians())
    };

    // Preserve heading from projected forward (-Z), then clear selected tilt axes in yaw-neutral
    // space to avoid Euler branch flips near 180deg heading.
    let forward = base_rotation.transform_vector(&nalgebra::Vector3::new(0.0, 0.0, -1.0));
    let planar_len = (forward.x * forward.x + forward.z * forward.z).sqrt();
    let yaw_y = if planar_len > 1e-9 { (-forward.x).atan2(-forward.z) } else { 0.0 };
    let yaw_rotation = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), yaw_y);
    let rotation = if snap_roll && snap_pitch {
        yaw_rotation
    } else {
        let residual = yaw_rotation.inverse() * base_rotation;
        let (mut pitch_x, _residual_yaw, mut roll_z) = residual.euler_angles();
        if snap_pitch {
            pitch_x = 0.0;
        }
        if snap_roll {
            roll_z = 0.0;
        }
        yaw_rotation * UnitQuaternion::from_euler_angles(pitch_x, 0.0, roll_z)
    };

    let (pitch_x, yaw_y, roll_z) = rotation.euler_angles();
    pose.rotation.pitch = pitch_x.to_degrees();
    pose.rotation.yaw = yaw_y.to_degrees();
    pose.rotation.roll = roll_z.to_degrees();
    pose.rotation.quaternion.x = rotation.i;
    pose.rotation.quaternion.y = rotation.j;
    pose.rotation.quaternion.z = rotation.k;
    pose.rotation.quaternion.w = rotation.w;
}
