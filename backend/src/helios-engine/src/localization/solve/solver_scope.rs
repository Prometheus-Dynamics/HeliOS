use crate::localization::config::LocalizationPoseSpace;
use crate::localization::sources::SourceSample;

fn solver_scope_uses_multiple_cameras(samples: &[&SourceSample]) -> bool {
    let mut camera_uids = std::collections::HashSet::new();
    for sample in samples {
        let camera_uid = sample.source.camera_uid.trim().to_ascii_lowercase();
        if camera_uid.is_empty() || camera_uid == "imu" {
            continue;
        }
        camera_uids.insert(camera_uid);
        if camera_uids.len() > 1 {
            return true;
        }
    }
    false
}

pub(super) fn filter_solver_output_spaces_for_scope(requested: &[LocalizationPoseSpace], scoped_samples: &[&SourceSample]) -> Vec<LocalizationPoseSpace> {
    if !solver_scope_uses_multiple_cameras(scoped_samples) {
        return requested.to_vec();
    }
    requested.iter().copied().filter(|space| *space != LocalizationPoseSpace::CameraInField).collect()
}
