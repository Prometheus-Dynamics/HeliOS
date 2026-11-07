//! The layout and mount as Eidos's own types (feature `eidos`), for code that runs Eidos itself:
//! helios-engine and the layout tests that solve poses with it. helios-api leaves the feature off
//! and has no Eidos.

pub use eidos_calibration::{CameraExtrinsics, KnownTag, KnownTagPoses, Se3Transform};
use eidos_calibration::{RotationMatrix3, Translation3};

use crate::{CameraMount, FIELD_PLACEMENT_SIGMA_M, FIELD_PLACEMENT_SIGMA_RAD, FieldLayout, Pose, Rigid, Vec3, placement_covariance};

impl From<Rigid> for Se3Transform {
    fn from(rigid: Rigid) -> Self {
        let t = rigid.translation;
        Self { rotation: RotationMatrix3 { m: rigid.rotation }, translation: Translation3 { x: t.x, y: t.y, z: t.z } }
    }
}

impl From<Se3Transform> for Rigid {
    fn from(transform: Se3Transform) -> Self {
        let t = transform.translation;
        Self { rotation: transform.rotation.m, translation: Vec3 { x: t.x, y: t.y, z: t.z } }
    }
}

impl Pose {
    pub fn to_se3(&self) -> Se3Transform {
        self.to_rigid().into()
    }

    /// The pose of `transform`, with the quaternion's `w >= 0`.
    pub fn from_se3(transform: &Se3Transform) -> Self {
        Self::from_rigid(&Rigid::from(*transform))
    }
}

impl FieldLayout {
    /// The tags as Eidos known tag poses, reference frame = the field, each with the field's
    /// placement error ([`FIELD_PLACEMENT_SIGMA_M`], [`FIELD_PLACEMENT_SIGMA_RAD`]).
    pub fn known_tags(&self) -> KnownTagPoses {
        KnownTagPoses::new(
            self.tags
                .iter()
                .map(|tag| {
                    let reference_from_tag = tag.field_from_eidos_tag();
                    KnownTag {
                        id: tag.id,
                        side: tag.side_m,
                        reference_from_tag: reference_from_tag.into(),
                        placement: Some(placement_covariance(&reference_from_tag, FIELD_PLACEMENT_SIGMA_M, FIELD_PLACEMENT_SIGMA_RAD)),
                    }
                })
                .collect(),
        )
    }
}

impl CameraMount {
    /// Eidos extrinsics: the camera's optical frame on the robot (`rig_from_camera`), so the
    /// multi-tag pose's `reference_from_rig` is the robot in the field.
    pub fn extrinsics(&self) -> CameraExtrinsics {
        CameraExtrinsics::from_flu_pose(self.robot_from_camera_flu().into())
    }
}
