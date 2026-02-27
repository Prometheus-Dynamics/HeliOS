use nalgebra::{Quaternion, UnitQuaternion, Vector4};

use super::math::PoseTransform;

pub(crate) fn merge_robot_estimates(estimates: &[(PoseTransform, f32, String)]) -> Option<(PoseTransform, Vec<String>)> {
    if estimates.is_empty() {
        return None;
    }
    let mut translation_acc = nalgebra::Vector3::zeros();
    let mut weight_sum = 0.0f64;

    let mut quat_acc = Vector4::zeros();
    let reference = estimates[0].0.rotation;
    for (pose, weight, _source_id) in estimates.iter() {
        let w = (*weight).max(0.0) as f64;
        if w == 0.0 {
            continue;
        }
        translation_acc += pose.translation * w;
        weight_sum += w;

        let mut quat = pose.rotation;
        if reference.coords.dot(&quat.coords) < 0.0 {
            quat = UnitQuaternion::from_quaternion(Quaternion::new(-quat.w, -quat.i, -quat.j, -quat.k));
        }
        quat_acc += quat.coords * w;
    }

    if weight_sum <= 0.0 {
        return None;
    }

    let translation = translation_acc / weight_sum;
    let quat = if quat_acc.norm() > 0.0 { UnitQuaternion::new_normalize(Quaternion::new(quat_acc.w, quat_acc.x, quat_acc.y, quat_acc.z)) } else { UnitQuaternion::identity() };
    let source_ids = estimates.iter().map(|(_, _, id)| id.clone()).collect();

    Some((PoseTransform { translation, rotation: quat }, source_ids))
}

pub(crate) fn merge_rotation_estimates(estimates: &[(UnitQuaternion<f64>, f32, String)]) -> Option<(UnitQuaternion<f64>, Vec<String>)> {
    if estimates.is_empty() {
        return None;
    }
    let mut quat_acc = Vector4::zeros();
    let mut weight_sum = 0.0f64;
    let reference = estimates[0].0;
    for (pose, weight, _source_id) in estimates.iter() {
        let w = (*weight).max(0.0) as f64;
        if w == 0.0 {
            continue;
        }
        let mut quat = *pose;
        if reference.coords.dot(&quat.coords) < 0.0 {
            quat = UnitQuaternion::from_quaternion(Quaternion::new(-quat.w, -quat.i, -quat.j, -quat.k));
        }
        quat_acc += quat.coords * w;
        weight_sum += w;
    }

    if weight_sum <= 0.0 || quat_acc.norm() == 0.0 {
        return None;
    }

    let quat = UnitQuaternion::new_normalize(Quaternion::new(quat_acc.w, quat_acc.x, quat_acc.y, quat_acc.z));
    let source_ids = estimates.iter().map(|(_, _, id)| id.clone()).collect();
    Some((quat, source_ids))
}
