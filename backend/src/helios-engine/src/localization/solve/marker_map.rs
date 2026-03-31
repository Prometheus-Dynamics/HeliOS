use lib_cv::modules::localization::{MarkerDefinition, MarkerMap};
use lib_cv::{Rotation3, Translation3};
use nalgebra::{Quaternion, UnitQuaternion};

use crate::localization::maps::{FieldMapDocument, FieldMapSource};

pub(super) fn marker_map_from_field_map(doc: &FieldMapDocument) -> MarkerMap {
    let prefer_heading_for_source = matches!(doc.source, FieldMapSource::LimelightFmap { .. });

    let markers = doc
        .markers
        .iter()
        .map(|marker| {
            let quaternion = marker.quaternion;
            let quaternion_is_finite = quaternion.x.is_finite() && quaternion.y.is_finite() && quaternion.z.is_finite() && quaternion.w.is_finite();
            let quaternion_is_identity = quaternion_is_finite && (quaternion.x.abs() + quaternion.y.abs() + quaternion.z.abs() <= 1e-9) && ((quaternion.w - 1.0).abs() <= 1e-9);

            let (rotation_from_quaternion, heading_ambiguous_from_quaternion) = if quaternion_is_finite {
                let norm_sq = quaternion.x * quaternion.x + quaternion.y * quaternion.y + quaternion.z * quaternion.z + quaternion.w * quaternion.w;
                if norm_sq > f64::EPSILON {
                    let q = UnitQuaternion::new_normalize(Quaternion::new(quaternion.w, quaternion.x, quaternion.y, quaternion.z));
                    let (roll, pitch, yaw) = q.euler_angles();

                    // `headingDeg` only captures horizontal heading of the tag normal.
                    // When the normal is close to vertical, heading becomes unstable and can
                    // collapse non-coplanar orientations into yaw-only results.
                    let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
                    let horizontal_norm = (normal.x * normal.x + normal.z * normal.z).sqrt();
                    (Some(Rotation3 { roll, pitch, yaw }), horizontal_norm < 0.20)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };
            let heading_rad = marker.heading_deg.to_radians();
            let rotation_from_heading = if marker.heading_deg.is_finite() {
                // `headingDeg` encodes the horizontal heading of the tag normal, where identity
                // quaternion corresponds to heading=90deg. Convert heading to a Y-up yaw first.
                let yaw_y = heading_rad - std::f64::consts::FRAC_PI_2;
                let q = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), yaw_y);
                let (roll, pitch, yaw) = q.euler_angles();
                Some(Rotation3 { roll, pitch, yaw })
            } else {
                None
            };

            // Limelight maps use heading for every marker, including exact zeros. For non-Limelight
            // maps, treat near-zero heading as "unset" unless quaternion is identity.
            let heading_has_signal = if prefer_heading_for_source { marker.heading_deg.is_finite() } else { marker.heading_deg.is_finite() && marker.heading_deg.abs() > 1e-6 };
            // Some maps contain mixed data: a subset of markers have calibrated quaternions while
            // others are left at identity with a meaningful heading. For identity quaternions, let
            // heading win to avoid 90deg side-of-tag errors on those markers.
            let prefer_heading = if prefer_heading_for_source { heading_has_signal && !heading_ambiguous_from_quaternion } else { heading_has_signal && quaternion_is_identity };
            let rotation = if prefer_heading { rotation_from_heading.or(rotation_from_quaternion) } else { rotation_from_quaternion.or(rotation_from_heading) };
            MarkerDefinition { id: marker.id, translation: Translation3 { x: marker.position[0], y: marker.position[1], z: marker.position[2] }, rotation }
        })
        .collect();
    MarkerMap { markers }
}
