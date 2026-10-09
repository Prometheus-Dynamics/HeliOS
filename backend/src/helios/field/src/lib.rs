//! FRC field layouts for HeliOS, converted to what Eidos's multi-tag pose solves against.
//!
//! Everything FRC-specific lives here, in HeliOS: Eidos is a generic CV library that solves a
//! camera pose from tags whose poses are known in *some* reference frame
//! (`eidos:known_tag_poses`, docs/pose.md in Eidos). This crate makes the reference frame the
//! FRC field and the rig the robot:
//!
//! - [`FieldLayout`] reads a WPILib AprilTag field layout (the JSON WPILib's
//!   `AprilTagFieldLayout` loads: `field.length`/`width` and `tags[{ID, pose{translation,
//!   rotation{quaternion{W,X,Y,Z}}}}]`) or a Limelight `.fmap` (`fiducials[{id, size, transform}]`,
//!   `fieldlength`, `fieldwidth`), and gives the tags as Eidos [`KnownTagPoses`] with the field as
//!   the reference frame ([`FieldLayout::known_tags`], [`FieldLayout::known_tags_value`] for a
//!   graph constant).
//! - [`CameraMount`] is the camera's pose on the robot in WPILib's robot frame and gives Eidos
//!   [`CameraExtrinsics`] (`rig_from_camera`), so the multi-tag pose's `reference_from_rig` is the
//!   robot in the field.
//!
//! # Frames
//!
//! Transforms are named `a_from_b` (Eidos's convention): a point of frame `b` is
//! `R p + t` in frame `a`.
//!
//! - **Field** (WPILib, "always blue origin"): origin at the blue alliance wall's right corner
//!   as seen from the blue driver station, `+X` along the field's length towards the red alliance
//!   wall, `+Y` to the left of a blue driver, `+Z` up; metres. A WPILib JSON is already in this
//!   frame. A Limelight `.fmap` is field-centred (origin at the centre of the carpet, same axes),
//!   so its translations are shifted by half its `fieldlength` and `fieldwidth`.
//! - **WPILib tag frame**: origin at the tag's centre, `+X` out of the tag's visible face (the
//!   tag faces `+X`), `+Z` up, `+Y = Z × X` (to the viewer's right when the viewer faces the
//!   tag). The `.fmap` `transform`s use the same tag frame (a 4x4 row-major `field_from_tag`).
//! - **Eidos tag frame** (`square_object_corners`): origin at the centre of the black square,
//!   `+X` along the marker's top edge (corner 0 to 1, the viewer's right), `+Y` down the left
//!   edge (corner 0 to 3), `+Z = X × Y` into the marker: the marker faces `-Z`. Corner 0 is the
//!   marker's top-left as printed.
//! - **Robot** (WPILib): forward-left-up, `+X` forward, `+Y` left, `+Z` up, origin where the
//!   team puts it (usually the robot's centre on the floor).
//! - **Camera**: Eidos's optical frame (`+X` right, `+Y` down, `+Z` forward). A mount is given
//!   as the pose of the camera's own forward-left-up frame on the robot, as WPILib and
//!   PhotonVision give robot-to-camera transforms; [`CameraMount::extrinsics`] composes Eidos's
//!   `flu_from_optical` (`CameraExtrinsics::from_flu_pose`).
//!
//! # The tag-frame rotation
//!
//! An upright tag (WPILib's layouts have no roll about the tag normal) seen from the front: the
//! viewer looks along `-X_wpilib`, up is `+Z_wpilib`, so the viewer's right is
//! `(-X_wpilib) × Z_wpilib = +Y_wpilib`. Eidos's axes in WPILib's tag frame are therefore
//!
//! - `X_eidos` (top edge, to the viewer's right) `= +Y_wpilib`,
//! - `Y_eidos` (down the marker) `= -Z_wpilib`,
//! - `Z_eidos` (into the marker, away from the viewer) `= -X_wpilib`,
//!
//! which is right-handed (`Y_wpilib × -Z_wpilib = -X_wpilib`). As a matrix whose columns are the
//! Eidos axes in WPILib coordinates ([`wpilib_tag_from_eidos_tag`]):
//!
//! ```text
//!                               [ 0  0 -1 ]
//! wpilib_tag_from_eidos_tag  =  [ 1  0  0 ]      (q = (w, x, y, z) = (½, -½, -½, ½): 120 degrees about (-1, -1, 1))
//!                               [ 0 -1  0 ]
//! ```
//!
//! and each tag's `reference_from_tag = field_from_wpilib_tag * wpilib_tag_from_eidos_tag`
//! (no translation: both frames are centred on the tag). The tests check it without trusting
//! the matrix: they build each tag's corners from WPILib's convention alone (centre, face
//! direction, up), name them by where a camera in front of the tag sees them, solve with Eidos
//! and recover the camera and robot poses; and they check the converted 2026 layout's normals
//! (alliance-wall tags face into the field).

use std::collections::BTreeSet;

pub use eidos_calibration::{CameraExtrinsics, KnownTag, KnownTagPoses, Se3Transform};
use eidos_calibration::{Quaternion, RotationMatrix3, Translation3, quaternion_from_rotation_matrix, rotation_matrix_from_quaternion, se3_compose};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// The FRC AprilTag edge length (the black square, 6.5 in), metres. WPILib JSON layouts have no
/// tag size; they are read with this one unless the caller gives another.
pub const FRC_TAG_SIDE_M: f64 = 0.1651;

/// The FRC 2026 (REBUILT) AndyMark field, as Limelight's `.fmap`: the built-in default layout.
const FRC_2026_ANDYMARK_FMAP: &str = include_str!("../data/FRC2026_ANDYMARK.fmap");

/// The id of the built-in 2026 layout.
pub const FRC_2026_ANDYMARK_ID: &str = "frc2026-andymark";

/// Longest field side accepted, metres (an FRC field is about 16.5 m).
const MAX_FIELD_SIDE_M: f64 = 100.0;

/// Why a layout could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("not JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("neither a WPILib AprilTag layout (`tags`, `field`) nor a Limelight .fmap (`fiducials`)")]
    UnknownFormat,
    #[error("{0}")]
    Invalid(String),
}

/// The file format a layout was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutFormat {
    /// WPILib's AprilTag field layout JSON.
    Wpilib,
    /// Limelight's `.fmap`.
    Fmap,
}

/// A translation, metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A unit quaternion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Self { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }
    }
}

/// A rigid transform `a_from_b` as Eidos's values show it: `{translation: {x, y, z}, rotation:
/// {w, x, y, z}}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Pose {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl Pose {
    pub fn to_se3(&self) -> Se3Transform {
        let q = self.rotation;
        Se3Transform {
            rotation: rotation_matrix_from_quaternion(Quaternion { w: q.w, x: q.x, y: q.y, z: q.z }),
            translation: Translation3 { x: self.translation.x, y: self.translation.y, z: self.translation.z },
        }
    }

    /// The pose of `transform`, with the quaternion's `w >= 0`.
    pub fn from_se3(transform: &Se3Transform) -> Self {
        let q = quaternion_from_rotation_matrix(transform.rotation);
        let sign = if q.w < 0.0 { -1.0 } else { 1.0 };
        let t = transform.translation;
        Self { translation: Vec3 { x: t.x, y: t.y, z: t.z }, rotation: Quat { w: sign * q.w, x: sign * q.x, y: sign * q.y, z: sign * q.z } }
    }

    fn is_finite(&self) -> bool {
        let (t, q) = (self.translation, self.rotation);
        [t.x, t.y, t.z, q.w, q.x, q.y, q.z].iter().all(|v| v.is_finite())
    }
}

/// One tag of a layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldTag {
    pub id: u32,
    /// Edge length of the black square, metres.
    pub side_m: f64,
    /// The tag in the field, in WPILib's tag frame (the tag faces its `+X`, `+Z` up).
    pub field_from_tag: Pose,
}

impl FieldTag {
    /// The tag in the field in Eidos's tag frame (it faces its `-Z`, `+Y` down).
    pub fn field_from_eidos_tag(&self) -> Se3Transform {
        let mut transform = self.field_from_tag.to_se3();
        transform.rotation = matmul(&transform.rotation, &wpilib_tag_from_eidos_tag());
        transform
    }

    /// The direction the tag's printed face looks, in the field (WPILib's tag `+X`).
    pub fn normal(&self) -> Vec3 {
        let r = self.field_from_tag.to_se3().rotation.m;
        Vec3 { x: r[0][0], y: r[1][0], z: r[2][0] }
    }
}

/// A field layout: the field's size and every tag's pose in the WPILib field frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldLayout {
    pub length_m: f64,
    pub width_m: f64,
    pub tags: Vec<FieldTag>,
}

/// Eidos's tag frame in WPILib's tag frame: the columns are Eidos's `+X` (`+Y_wpilib`), `+Y`
/// (`-Z_wpilib`) and `+Z` (`-X_wpilib`). See the crate docs.
pub fn wpilib_tag_from_eidos_tag() -> RotationMatrix3 {
    RotationMatrix3 { m: [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]] }
}

fn matmul(a: &RotationMatrix3, b: &RotationMatrix3) -> RotationMatrix3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a.m[i][k] * b.m[k][j]).sum();
        }
    }
    RotationMatrix3 { m }
}

fn normalized(q: Quat) -> Option<Quat> {
    let norm = (q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z).sqrt();
    (norm.is_finite() && norm > 1e-9).then(|| Quat { w: q.w / norm, x: q.x / norm, y: q.y / norm, z: q.z / norm })
}

/// WPILib's AprilTag field layout JSON.
#[derive(Deserialize)]
struct WpilibLayout {
    tags: Vec<WpilibTag>,
    field: WpilibField,
}

#[derive(Deserialize)]
struct WpilibField {
    length: f64,
    width: f64,
}

#[derive(Deserialize)]
struct WpilibTag {
    #[serde(rename = "ID")]
    id: u32,
    pose: WpilibPose,
}

#[derive(Deserialize)]
struct WpilibPose {
    translation: Vec3,
    rotation: WpilibRotation,
}

#[derive(Deserialize)]
struct WpilibRotation {
    quaternion: WpilibQuaternion,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct WpilibQuaternion {
    W: f64,
    X: f64,
    Y: f64,
    Z: f64,
}

/// Limelight's `.fmap`.
#[derive(Deserialize)]
struct Fmap {
    fiducials: Vec<FmapFiducial>,
    #[serde(default)]
    fieldlength: Option<f64>,
    #[serde(default)]
    fieldwidth: Option<f64>,
}

#[derive(Deserialize)]
struct FmapFiducial {
    id: u32,
    /// Edge length, millimetres.
    size: f64,
    /// `field_from_tag`, 4x4 row-major, field-centred, WPILib tag frame.
    transform: Vec<f64>,
}

impl FieldLayout {
    /// Read a WPILib JSON or a `.fmap`, telling them apart by their keys. WPILib layouts get
    /// `tag_side_m` for every tag ([`FRC_TAG_SIDE_M`] for FRC); a `.fmap` has its own sizes.
    pub fn parse(text: &str, tag_side_m: f64) -> Result<(Self, LayoutFormat), LayoutError> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        if value.get("fiducials").is_some() {
            Ok((Self::from_fmap_value(value)?, LayoutFormat::Fmap))
        } else if value.get("tags").is_some() && value.get("field").is_some() {
            Ok((Self::from_wpilib_value(value, tag_side_m)?, LayoutFormat::Wpilib))
        } else {
            Err(LayoutError::UnknownFormat)
        }
    }

    /// A WPILib AprilTag field layout JSON (already in the blue-origin field frame).
    pub fn from_wpilib_json(text: &str, tag_side_m: f64) -> Result<Self, LayoutError> {
        Self::from_wpilib_value(serde_json::from_str(text)?, tag_side_m)
    }

    fn from_wpilib_value(value: serde_json::Value, tag_side_m: f64) -> Result<Self, LayoutError> {
        let layout: WpilibLayout = serde_json::from_value(value)?;
        let tags = layout
            .tags
            .into_iter()
            .map(|tag| {
                let q = tag.pose.rotation.quaternion;
                let rotation = normalized(Quat { w: q.W, x: q.X, y: q.Y, z: q.Z }).ok_or_else(|| LayoutError::Invalid(format!("tag {}: rotation quaternion is zero or not finite", tag.id)))?;
                Ok(FieldTag { id: tag.id, side_m: tag_side_m, field_from_tag: Pose { translation: tag.pose.translation, rotation } })
            })
            .collect::<Result<Vec<_>, LayoutError>>()?;
        let layout = Self { length_m: layout.field.length, width_m: layout.field.width, tags };
        layout.validate()?;
        Ok(layout)
    }

    /// A Limelight `.fmap`: field-centred, sizes in millimetres; moved to the blue-origin field
    /// frame by half its `fieldlength` and `fieldwidth`.
    pub fn from_fmap(text: &str) -> Result<Self, LayoutError> {
        Self::from_fmap_value(serde_json::from_str(text)?)
    }

    fn from_fmap_value(value: serde_json::Value) -> Result<Self, LayoutError> {
        let fmap: Fmap = serde_json::from_value(value)?;
        let (Some(length_m), Some(width_m)) = (fmap.fieldlength, fmap.fieldwidth) else {
            return Err(LayoutError::Invalid("the .fmap has no fieldlength/fieldwidth, so its field-centred poses cannot be placed in WPILib's blue-origin field frame".into()));
        };
        let tags = fmap
            .fiducials
            .into_iter()
            .map(|fiducial| {
                let t = &fiducial.transform;
                if t.len() != 16 {
                    return Err(LayoutError::Invalid(format!("tag {}: transform has {} values, not 16", fiducial.id, t.len())));
                }
                let rotation = RotationMatrix3 { m: [[t[0], t[1], t[2]], [t[4], t[5], t[6]], [t[8], t[9], t[10]]] };
                if !is_rotation(&rotation) {
                    return Err(LayoutError::Invalid(format!("tag {}: transform's rotation is not a rotation", fiducial.id)));
                }
                let field_from_tag = Se3Transform { rotation, translation: Translation3 { x: t[3] + length_m / 2.0, y: t[7] + width_m / 2.0, z: t[11] } };
                Ok(FieldTag { id: fiducial.id, side_m: fiducial.size / 1000.0, field_from_tag: Pose::from_se3(&field_from_tag) })
            })
            .collect::<Result<Vec<_>, LayoutError>>()?;
        let layout = Self { length_m, width_m, tags };
        layout.validate()?;
        Ok(layout)
    }

    /// The FRC 2026 AndyMark field (Limelight's `FRC2026_ANDYMARK.fmap`), HeliOS's default.
    pub fn frc_2026_andymark() -> Self {
        Self::from_fmap(FRC_2026_ANDYMARK_FMAP).expect("the built-in 2026 layout is valid")
    }

    /// Sizes positive and finite, poses finite, ids unique.
    pub fn validate(&self) -> Result<(), LayoutError> {
        if !(self.length_m.is_finite() && self.width_m.is_finite() && self.length_m > 0.0 && self.width_m > 0.0 && self.length_m <= MAX_FIELD_SIDE_M && self.width_m <= MAX_FIELD_SIDE_M) {
            return Err(LayoutError::Invalid(format!("field size {} x {} m: each side must be positive and at most {MAX_FIELD_SIDE_M} m", self.length_m, self.width_m)));
        }
        if self.tags.is_empty() {
            return Err(LayoutError::Invalid("the layout has no tags".into()));
        }
        let mut ids = BTreeSet::new();
        for tag in &self.tags {
            if !ids.insert(tag.id) {
                return Err(LayoutError::Invalid(format!("tag {} appears twice", tag.id)));
            }
            if !(tag.side_m.is_finite() && tag.side_m > 0.0 && tag.side_m < 10.0) {
                return Err(LayoutError::Invalid(format!("tag {}: side {} m must be positive and under 10 m", tag.id, tag.side_m)));
            }
            if !tag.field_from_tag.is_finite() {
                return Err(LayoutError::Invalid(format!("tag {}: pose is not finite", tag.id)));
            }
        }
        Ok(())
    }

    pub fn tag(&self, id: u32) -> Option<&FieldTag> {
        self.tags.iter().find(|tag| tag.id == id)
    }

    /// The tags as Eidos known tag poses, reference frame = the field.
    pub fn known_tags(&self) -> KnownTagPoses {
        KnownTagPoses::new(self.tags.iter().map(|tag| KnownTag { id: tag.id, side: tag.side_m, reference_from_tag: tag.field_from_eidos_tag() }).collect())
    }

    /// [`Self::known_tags`] as the value of an `eidos:known_tag_poses` graph constant: a list of
    /// `{id, side, reference_from_tag: {translation: {x, y, z}, rotation: {w, x, y, z}}}`.
    pub fn known_tags_value(&self) -> serde_json::Value {
        let tags = self
            .tags
            .iter()
            .map(|tag| {
                let pose = Pose::from_se3(&tag.field_from_eidos_tag());
                serde_json::json!({ "id": tag.id, "side": tag.side_m, "reference_from_tag": pose })
            })
            .collect();
        serde_json::Value::Array(tags)
    }
}

fn is_rotation(r: &RotationMatrix3) -> bool {
    let m = &r.m;
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let orthonormal = (0..3).all(|i| (0..3).all(|j| ((0..3).map(|k| m[k][i] * m[k][j]).sum::<f64>() - f64::from(u8::from(i == j))).abs() < 1e-6));
    orthonormal && (det - 1.0).abs() < 1e-6
}

/// The camera's pose on the robot, WPILib's robot frame (forward-left-up): the translation of the
/// camera in metres and its rotation as WPILib's `Rotation3d(roll, pitch, yaw)` in radians (about
/// the robot's `X`, then `Y`, then `Z` axes, fixed axes: `R = Rz(yaw) Ry(pitch) Rx(roll)`). A
/// positive pitch tilts the camera down; yaw π faces it backwards.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct CameraMount {
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
    pub roll_rad: f64,
    pub pitch_rad: f64,
    pub yaw_rad: f64,
}

impl CameraMount {
    /// The camera's forward-left-up frame in the robot frame.
    pub fn robot_from_camera_flu(&self) -> Se3Transform {
        let (sr, cr) = self.roll_rad.sin_cos();
        let (sp, cp) = self.pitch_rad.sin_cos();
        let (sy, cy) = self.yaw_rad.sin_cos();
        let rx = RotationMatrix3 { m: [[1.0, 0.0, 0.0], [0.0, cr, -sr], [0.0, sr, cr]] };
        let ry = RotationMatrix3 { m: [[cp, 0.0, sp], [0.0, 1.0, 0.0], [-sp, 0.0, cp]] };
        let rz = RotationMatrix3 { m: [[cy, -sy, 0.0], [sy, cy, 0.0], [0.0, 0.0, 1.0]] };
        Se3Transform { rotation: matmul(&rz, &matmul(&ry, &rx)), translation: Translation3 { x: self.x_m, y: self.y_m, z: self.z_m } }
    }

    /// Eidos extrinsics: the camera's optical frame on the robot (`rig_from_camera`), so the
    /// multi-tag pose's `reference_from_rig` is the robot in the field.
    pub fn extrinsics(&self) -> CameraExtrinsics {
        CameraExtrinsics::from_flu_pose(self.robot_from_camera_flu())
    }

    /// No mount: the robot frame is the camera's own forward-left-up frame.
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

/// `a * b` for rigid transforms (Eidos's `se3_compose`).
pub fn compose(a: Se3Transform, b: Se3Transform) -> Se3Transform {
    se3_compose(a, b)
}
