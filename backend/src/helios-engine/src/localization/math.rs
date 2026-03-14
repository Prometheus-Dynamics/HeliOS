use nalgebra::{Quaternion, UnitQuaternion, Vector3};

use super::types::{LocalizationPose, LocalizationQuaternion, LocalizationRotation, LocalizationVector};
use lib_cv::Translation3;

#[derive(Debug, Clone, Copy)]
pub struct PoseTransform {
    pub translation: Vector3<f64>,
    pub rotation: UnitQuaternion<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct RigPose {
    pub translation: RigTranslation,
    pub rotation: RigRotation,
}

#[derive(Debug, Clone, Copy)]
pub struct RigTranslation {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct RigRotation {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
}

pub fn pose_from_detection(translation: Translation3, quaternion: Option<LocalizationQuaternion>, euler: Option<(f64, f64, f64)>) -> PoseTransform {
    let rotation = if let Some(quat) = quaternion {
        let q = Quaternion::new(quat.w, quat.x, quat.y, quat.z);
        UnitQuaternion::from_quaternion(q)
    } else if let Some((roll, pitch, yaw)) = euler {
        UnitQuaternion::from_euler_angles(roll.to_radians(), pitch.to_radians(), yaw.to_radians())
    } else {
        UnitQuaternion::identity()
    };

    PoseTransform { translation: Vector3::new(translation.x, translation.y, translation.z), rotation }
}

pub fn transform_to_translation(transform: &PoseTransform) -> Translation3 {
    Translation3 { x: transform.translation.x, y: transform.translation.y, z: transform.translation.z }
}

pub fn transform_to_rotation(transform: &PoseTransform) -> lib_cv::Rotation3 {
    let (roll, pitch, yaw) = transform.rotation.euler_angles();
    lib_cv::Rotation3 { roll, pitch, yaw }
}

pub fn transform_to_pose(transform: &PoseTransform) -> LocalizationPose {
    let quat = transform.rotation;
    let (roll, pitch, yaw) = quat.euler_angles();
    LocalizationPose {
        translation: LocalizationVector { x: transform.translation.x, y: transform.translation.y, z: transform.translation.z },
        rotation: LocalizationRotation { roll: roll.to_degrees(), pitch: pitch.to_degrees(), yaw: yaw.to_degrees(), quaternion: LocalizationQuaternion { x: quat.i, y: quat.j, z: quat.k, w: quat.w } },
    }
}

pub fn pose_to_transform(pose: &LocalizationPose) -> PoseTransform {
    let translation = Vector3::new(pose.translation.x, pose.translation.y, pose.translation.z);
    let rotation = {
        let quat = &pose.rotation.quaternion;
        UnitQuaternion::from_quaternion(Quaternion::new(quat.w, quat.x, quat.y, quat.z))
    };
    PoseTransform { translation, rotation }
}

pub fn device_pose_to_transform(pose: &lib_cv::DevicePose) -> PoseTransform {
    let rotation = UnitQuaternion::from_euler_angles(pose.rotation.roll, pose.rotation.pitch, pose.rotation.yaw);
    PoseTransform { translation: Vector3::new(pose.translation.x, pose.translation.y, pose.translation.z), rotation }
}

pub fn compose_transforms(parent_from_child: &PoseTransform, child_from_grandchild: &PoseTransform) -> PoseTransform {
    let rotated = parent_from_child.rotation.transform_vector(&child_from_grandchild.translation);
    PoseTransform { translation: parent_from_child.translation + rotated, rotation: parent_from_child.rotation * child_from_grandchild.rotation }
}

pub fn invert_transform(transform: &PoseTransform) -> PoseTransform {
    let rotation_inv = transform.rotation.inverse();
    let translated = rotation_inv.transform_vector(&transform.translation);
    PoseTransform { translation: -translated, rotation: rotation_inv }
}

pub fn rig_pose_to_viewer_transform(pose: &RigPose) -> PoseTransform {
    let translation = Vector3::new(pose.translation.y, pose.translation.z, pose.translation.x);
    let rotation = rig_rotation_to_viewer_quaternion(&pose.rotation);
    PoseTransform { translation, rotation }
}

pub fn rig_rotation_to_viewer_quaternion(rotation: &RigRotation) -> UnitQuaternion<f64> {
    let pitch = (-rotation.pitch).to_radians();
    let yaw = rotation.yaw.to_radians();
    let roll = rotation.roll.to_radians();

    // Matches frontend eulerDegreesToQuaternionXYZ: qx * qy * qz.
    let qx = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch);
    let qy = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw);
    let qz = UnitQuaternion::from_axis_angle(&Vector3::z_axis(), roll);
    qx * qy * qz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_pose_translation_maps_backend_xyz_into_viewer_axes() {
        let pose = RigPose { translation: RigTranslation { x: 1.0, y: 2.0, z: 3.0 }, rotation: RigRotation { roll: 0.0, pitch: 0.0, yaw: 0.0 } };
        let transformed = rig_pose_to_viewer_transform(&pose);
        assert_eq!(transformed.translation, Vector3::new(2.0, 3.0, 1.0));
    }

    #[test]
    fn rig_rotation_pitch_sign_is_inverted_for_viewer_frame() {
        let quat = rig_rotation_to_viewer_quaternion(&RigRotation { roll: 0.0, pitch: 90.0, yaw: 0.0 });

        let rotated_up = quat.transform_vector(&Vector3::new(0.0, 1.0, 0.0));
        assert!(rotated_up.z < -0.999);
    }
}
