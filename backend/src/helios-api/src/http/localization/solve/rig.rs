use nalgebra::{UnitQuaternion, Vector3};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

use super::inputs::{source_looks_like_imu, source_looks_like_imu_pose};
use crate::http::{AppState, streams_persist};
use helios_engine::ipc::{RigPose as StreamRigPose, StreamSummary};
use helios_engine::localization::config::LocalizationSourceConfig;
use helios_engine::localization::fetch::imu_vec_to_viewer_frame;
use helios_engine::localization::math::{PoseTransform, RigPose, RigRotation, RigTranslation, rig_pose_to_viewer_transform};
use helios_peripherals::dto::SensorScope;

pub(crate) async fn load_rig_poses_from_streams(sources: &[LocalizationSourceConfig], stream_summaries: &[StreamSummary]) -> HashMap<String, PoseTransform> {
    let mut out: HashMap<String, PoseTransform> = streams_persist::list_pose_map().await.into_iter().map(|(uid, pose)| (uid, rig_pose_to_viewer_transform(&map_rig_pose(&pose)))).collect();

    let running_by_stream: HashMap<Uuid, StreamRigPose> = stream_summaries.iter().filter_map(|stream| stream.manifest.pose.clone().map(|pose| (stream.stream_id, pose))).collect();

    for source in sources {
        let camera_uid = source.camera_uid.trim();
        if camera_uid.is_empty() || out.contains_key(camera_uid) {
            continue;
        }
        let Ok(stream_id) = Uuid::parse_str(source.stream_id.trim()) else {
            continue;
        };

        let stream_pose = if let Some(pose) = running_by_stream.get(&stream_id).cloned() { Some(pose) } else { streams_persist::pose_for_stream_id(stream_id).await };
        if let Some(pose) = stream_pose {
            out.insert(camera_uid.to_string(), rig_pose_to_viewer_transform(&map_rig_pose(&pose)));
        }
    }

    for source in sources {
        let camera_uid = source.camera_uid.trim();
        if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") {
            continue;
        }
        out.entry(camera_uid.to_string()).or_insert_with(|| PoseTransform { translation: Vector3::new(0.0, 0.0, 0.0), rotation: UnitQuaternion::identity() });
    }

    out
}

pub(crate) async fn inject_imu_leveling_rig_pose(state: &AppState, profile: &helios_engine::localization::config::LocalizationProfile, rig_poses: &mut HashMap<String, PoseTransform>) {
    let enabled_sources: Vec<&LocalizationSourceConfig> = profile.sources.iter().filter(|source| source.enabled).collect();
    let has_imu_source = enabled_sources.iter().any(|source| source_looks_like_imu(source));
    if !has_imu_source {
        return;
    }

    let has_explicit_imu_pose_source = enabled_sources.iter().any(|source| source_looks_like_imu_pose(source));
    if has_explicit_imu_pose_source {
        return;
    }

    let mut camera_uid_set = BTreeSet::<String>::new();
    for imu_source in enabled_sources.iter().copied().filter(|source| source_looks_like_imu(source)) {
        let imu_camera_uid = imu_source.camera_uid.trim();
        if !imu_camera_uid.is_empty() && !imu_camera_uid.eq_ignore_ascii_case("imu") && !imu_camera_uid.starts_with("peer:") {
            camera_uid_set.insert(imu_camera_uid.to_string());
            continue;
        }

        let imu_stream_id = imu_source.stream_id.trim();
        if imu_stream_id.is_empty() {
            continue;
        }
        for source in enabled_sources.iter().copied().filter(|source| !source_looks_like_imu(source)) {
            if source.stream_id.trim() != imu_stream_id {
                continue;
            }
            let camera_uid = source.camera_uid.trim();
            if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") || camera_uid.starts_with("peer:") {
                continue;
            }
            camera_uid_set.insert(camera_uid.to_string());
        }
    }

    if camera_uid_set.is_empty() {
        let non_imu_camera_uids = enabled_sources
            .iter()
            .copied()
            .filter(|source| !source_looks_like_imu(source))
            .filter_map(|source| {
                let camera_uid = source.camera_uid.trim();
                if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") || camera_uid.starts_with("peer:") {
                    return None;
                }
                Some(camera_uid.to_string())
            })
            .collect::<BTreeSet<_>>();
        if non_imu_camera_uids.len() == 1 {
            camera_uid_set = non_imu_camera_uids;
        }
    }

    let camera_uids: Vec<String> = camera_uid_set.into_iter().collect();
    if camera_uids.is_empty() {
        return;
    }

    let Some(peripherals) = state.ensure_sensors().await else {
        return;
    };
    let snapshot = match peripherals.sensor_snapshot(SensorScope::Device).await {
        Ok(Ok(values)) => values,
        _ => return,
    };
    let status = crate::http::device::imu::imu_status_from_snapshot(&snapshot);
    let Some(accel) = status.accel else {
        return;
    };

    let g_imu = Vector3::new(accel.x, accel.y, accel.z);
    let norm = g_imu.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return;
    }
    if !(0.3..=2.0).contains(&norm) {
        return;
    }
    let g_imu = g_imu / norm;

    let mut g_view = imu_vec_to_viewer_frame(g_imu);
    let target = Vector3::new(0.0, -1.0, 0.0);
    if g_view.dot(&target) < 0.0 {
        g_view = -g_view;
    }

    let Some(level_rot) = UnitQuaternion::rotation_between(&g_view, &target) else {
        return;
    };

    for uid in camera_uids {
        rig_poses
            .entry(uid)
            .and_modify(|pose| {
                let forward = pose.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
                let mut forward_xz = Vector3::new(forward.x, 0.0, forward.z);
                let yaw_rot = if forward_xz.norm_squared() > 1e-12 {
                    forward_xz = forward_xz.normalize();
                    let yaw = forward_xz.x.atan2(forward_xz.z);
                    UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw)
                } else {
                    UnitQuaternion::identity()
                };

                pose.rotation = yaw_rot * level_rot;
            })
            .or_insert_with(|| PoseTransform { translation: Vector3::new(0.0, 0.0, 0.0), rotation: level_rot });
    }
}

fn map_rig_pose(pose: &StreamRigPose) -> RigPose {
    RigPose {
        translation: RigTranslation { x: pose.translation.x, y: pose.translation.y, z: pose.translation.z },
        rotation: RigRotation { roll: pose.rotation.roll, pitch: pose.rotation.pitch, yaw: pose.rotation.yaw },
    }
}
