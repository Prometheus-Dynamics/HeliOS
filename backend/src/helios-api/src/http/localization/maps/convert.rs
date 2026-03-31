use serde::Deserialize;

use super::super::super::error::ApiError;
use super::super::super::storage;
use helios_engine::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapOverlay, FieldMapSource, FieldQuaternion, aruco_bits_for_family};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct LimelightFmap {
    pub(super) fieldlength: f64,
    pub(super) fieldwidth: f64,
    #[serde(default)]
    pub(super) fiducials: Vec<LimelightFiducial>,
    #[serde(default)]
    pub(super) r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct LimelightFiducial {
    family: String,
    id: u32,
    size: f64,
    transform: Vec<f64>,
    #[serde(default)]
    unique: bool,
}

pub(super) fn convert_limelight_fmap(id: &str, name: &str, filename: &str, fmap: LimelightFmap, overlay: Option<FieldMapOverlay>) -> Result<FieldMapDocument, Box<ApiError>> {
    use nalgebra::{Matrix3, Matrix4, Rotation3, UnitQuaternion, Vector3};

    // Limelight .fmap uses FRC/WPILib-style field coordinates:
    // +X forward (field length), +Y left (field width), +Z up.
    //
    // The localization viewer uses a Three.js-friendly basis:
    // +X left, +Y up, +Z forward.
    //
    // So: viewer = [wpilib_y, wpilib_z, wpilib_x]
    let width_m = fmap.fieldwidth;
    let depth_m = fmap.fieldlength;
    if !(width_m.is_finite() && width_m > 0.0 && depth_m.is_finite() && depth_m > 0.0) {
        return Err(Box::new(ApiError::bad_request("invalid field dimensions")));
    }

    // Basis change matrix from WPILib -> viewer.
    let basis = Matrix4::new(0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
    let basis_inv = basis.transpose();

    let mut markers = Vec::new();
    for fiducial in fmap.fiducials {
        if fiducial.transform.len() < 16 {
            continue;
        }
        let mut field_from_tag = Matrix4::<f64>::zeros();
        for i in 0..16 {
            field_from_tag[(i / 4, i % 4)] = fiducial.transform[i];
        }
        let viewer_from_tag = basis * field_from_tag * basis_inv;

        let position = [viewer_from_tag[(0, 3)], viewer_from_tag[(1, 3)], viewer_from_tag[(2, 3)]];

        let rot = Matrix3::from_row_slice(&[
            viewer_from_tag[(0, 0)],
            viewer_from_tag[(0, 1)],
            viewer_from_tag[(0, 2)],
            viewer_from_tag[(1, 0)],
            viewer_from_tag[(1, 1)],
            viewer_from_tag[(1, 2)],
            viewer_from_tag[(2, 0)],
            viewer_from_tag[(2, 1)],
            viewer_from_tag[(2, 2)],
        ]);
        let rot = Rotation3::from_matrix_unchecked(rot);
        let quat = UnitQuaternion::from_rotation_matrix(&rot);
        let q = quat.quaternion();

        let normal = quat.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
        let heading_deg = normal.x.atan2(normal.z).to_degrees();

        let size_m = fiducial.size / 1000.0;
        let tag_bits = aruco_bits_for_family(&normalize_family_label_for_bits(&fiducial.family), fiducial.id);

        markers.push(FieldMapMarker {
            id: fiducial.id,
            family: fiducial.family,
            size_m,
            position,
            quaternion: FieldQuaternion { x: q.i, y: q.j, z: q.k, w: q.w },
            heading_deg,
            tag_bits,
            unique: fiducial.unique,
        });
    }

    markers.sort_by_key(|marker| marker.id);

    Ok(FieldMapDocument {
        schema_version: 1,
        id: id.to_string(),
        name: name.to_string(),
        width_m,
        depth_m,
        markers,
        source: FieldMapSource::LimelightFmap { original_file_name: storage::sanitize_name(filename), map_type: fmap.r#type },
        overlay,
    })
}

fn normalize_family_label_for_bits(raw: &str) -> String {
    let value = raw.trim().to_ascii_lowercase();
    for label in ["36h11", "36h10", "25h9", "16h5"] {
        if value == label || value.contains(label) {
            return label.to_string();
        }
    }
    raw.trim().to_string()
}
