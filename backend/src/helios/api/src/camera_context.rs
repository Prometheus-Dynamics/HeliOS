//! Camera calibration (stored per camera and resolution) and the camera context the API writes
//! into a pipeline's camera bindings for helios-engine: `binding.<input>.context.<field>`, fed to
//! the graph's host inputs `<input>_<field>`, which HeliOS's templates connect to the camera
//! ports of Eidos's pose nodes (`eidos:aruco.pose`, `eidos:aruco.field_pose`): the intrinsics
//! and lens model of the calibration matching the binding's frame size, and the camera mount.
//!
//! Without a matching calibration the context says `fx = fy = 0`, which Eidos reports as
//! `status: "uncalibrated"` with no poses (it never assumes a camera).

use std::collections::BTreeMap;

use orion::control_plane::TypedConfigValue;
use serde::{Deserialize, Serialize};

use crate::{
    error::{ApiError, ApiResult},
    store::CameraMount,
};

/// The lens model of a calibration (Eidos's `lens` port).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LensModel {
    /// Brown-Conrady, OpenCV's 8 coefficients: radial `k1`, `k2`, `k3` over `k4`, `k5`, `k6`,
    /// tangential `p1`, `p2`.
    #[default]
    Pinhole,
    /// Equidistant (`cv::fisheye`): `k1` to `k4`.
    Fisheye,
}

impl LensModel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pinhole => "pinhole",
            Self::Fisheye => "fisheye",
        }
    }
}

/// Distortion coefficients (OpenCV's order and meaning).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Distortion {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub k4: f64,
    pub k5: f64,
    pub k6: f64,
    pub p1: f64,
    pub p2: f64,
}

impl Distortion {
    fn fields(&self) -> [(&'static str, f64); 8] {
        [("k1", self.k1), ("k2", self.k2), ("k3", self.k3), ("k4", self.k4), ("k5", self.k5), ("k6", self.k6), ("p1", self.p1), ("p2", self.p2)]
    }
}

/// One camera calibration: intrinsics in pixels of a `width` x `height` image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CameraCalibration {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub model: LensModel,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    #[serde(default)]
    pub distortion: Distortion,
    /// Reprojection error of the fit, pixels (informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rms_px: Option<f64>,
    /// Where it came from (a tool, a board, a date); informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Set by the API when stored.
    #[serde(default)]
    pub saved_at_ms: u64,
}

impl CameraCalibration {
    pub fn validate(&self) -> ApiResult<()> {
        if !(16..=16384).contains(&self.width) || !(16..=16384).contains(&self.height) {
            return Err(ApiError::unprocessable("width and height are the calibrated image size in pixels (16 to 16384)"));
        }
        if !(self.fx.is_finite() && self.fy.is_finite() && self.fx > 0.0 && self.fy > 0.0) {
            return Err(ApiError::unprocessable("fx and fy are focal lengths in pixels, positive and finite"));
        }
        if !(self.cx.is_finite() && self.cy.is_finite()) {
            return Err(ApiError::unprocessable("cx and cy are the principal point in pixels"));
        }
        if self.distortion.fields().iter().any(|(_, value)| !value.is_finite()) {
            return Err(ApiError::unprocessable("distortion coefficients must be finite"));
        }
        let d = self.distortion;
        if self.model == LensModel::Fisheye && [d.k5, d.k6, d.p1, d.p2].iter().any(|value| *value != 0.0) {
            return Err(ApiError::unprocessable("a fisheye (equidistant) calibration has k1 to k4 only; k5, k6, p1 and p2 must be 0"));
        }
        if self.rms_px.is_some_and(|rms| !rms.is_finite() || rms < 0.0) {
            return Err(ApiError::unprocessable("rms_px must be a non-negative number"));
        }
        Ok(())
    }

    /// The same camera at `width` x `height` (same aspect ratio, a scaled capture): focal
    /// lengths scale, the principal point scales about pixel centres, distortion is unchanged
    /// (it works on normalized coordinates).
    pub fn scaled_to(&self, width: u32, height: u32) -> Self {
        let sx = f64::from(width) / f64::from(self.width);
        let sy = f64::from(height) / f64::from(self.height);
        Self { width, height, fx: self.fx * sx, fy: self.fy * sy, cx: (self.cx + 0.5) * sx - 0.5, cy: (self.cy + 0.5) * sy - 0.5, ..self.clone() }
    }

    fn aspect(&self) -> f64 {
        f64::from(self.width) / f64::from(self.height)
    }
}

/// Which calibration applies to a pipeline's frames, and why.
#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationMatch {
    /// Calibrated at exactly this size.
    Exact(CameraCalibration),
    /// Scaled from a calibration of the same aspect ratio.
    Scaled { calibration: CameraCalibration, from: (u32, u32) },
    /// Nothing usable: the reason.
    None(String),
}

impl CalibrationMatch {
    pub fn calibration(&self) -> Option<&CameraCalibration> {
        match self {
            Self::Exact(calibration) | Self::Scaled { calibration, .. } => Some(calibration),
            Self::None(_) => None,
        }
    }
}

/// The calibration for frames of `size` (the binding's output size; `None`: the camera's own
/// size, taken to be the largest calibrated one): the one calibrated at that size, else the
/// largest one with the same aspect ratio (within 0.5 %), scaled.
pub fn match_calibration(calibrations: &[CameraCalibration], size: Option<(u32, u32)>) -> CalibrationMatch {
    let Some(largest) = calibrations.iter().max_by_key(|c| u64::from(c.width) * u64::from(c.height)) else {
        return CalibrationMatch::None("the camera has no calibration".into());
    };
    let Some((width, height)) = size else {
        return CalibrationMatch::Exact(largest.clone());
    };
    if let Some(exact) = calibrations.iter().find(|c| c.width == width && c.height == height) {
        return CalibrationMatch::Exact(exact.clone());
    }
    let aspect = f64::from(width) / f64::from(height);
    match calibrations.iter().filter(|c| (c.aspect() / aspect - 1.0).abs() < 0.005).max_by_key(|c| u64::from(c.width) * u64::from(c.height)) {
        Some(base) => CalibrationMatch::Scaled { calibration: base.scaled_to(width, height), from: (base.width, base.height) },
        None => CalibrationMatch::None(format!("no calibration at {width}x{height} or with its aspect ratio")),
    }
}

/// The context fields for a camera binding: Eidos's camera port names (`fx` ... `p2`, `lens`) and
/// mount ports (`mount`, `mount_x` ... `mount_yaw`). Numbers go as decimal strings (Orion config
/// values have no floats); the engine pushes them as held `f64` values. The API's mount (metres
/// and degrees, x forward, y left, z up) is the camera body frame's pose on the robot: Eidos's
/// `mount = body`, angles in radians.
pub fn context_fields(calibration: Option<&CameraCalibration>, mount: Option<&CameraMount>) -> BTreeMap<String, TypedConfigValue> {
    let number = |value: f64| TypedConfigValue::String(format!("{value}"));
    let mut fields = BTreeMap::new();
    let (lens, intrinsics, distortion) = match calibration {
        Some(c) => (c.model, [c.fx, c.fy, c.cx, c.cy], c.distortion),
        None => (LensModel::Pinhole, [0.0; 4], Distortion::default()),
    };
    fields.insert("lens".to_string(), TypedConfigValue::String(lens.name().into()));
    for (name, value) in ["fx", "fy", "cx", "cy"].into_iter().zip(intrinsics) {
        fields.insert(name.to_string(), number(value));
    }
    for (name, value) in distortion.fields() {
        fields.insert(name.to_string(), number(value));
    }
    let (kind, pose) = match mount {
        Some(m) => ("body", [m.x, m.y, m.z, m.roll.to_radians(), m.pitch.to_radians(), m.yaw.to_radians()]),
        None => ("none", [0.0; 6]),
    };
    fields.insert("mount".to_string(), TypedConfigValue::String(kind.into()));
    for (name, value) in ["mount_x", "mount_y", "mount_z", "mount_roll", "mount_pitch", "mount_yaw"].into_iter().zip(pose) {
        fields.insert(name.to_string(), number(value));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibration(width: u32, height: u32) -> CameraCalibration {
        CameraCalibration {
            width,
            height,
            model: LensModel::Fisheye,
            fx: 560.0,
            fy: 560.0,
            cx: 639.5,
            cy: 399.5,
            distortion: Distortion { k1: 0.1, ..Distortion::default() },
            rms_px: Some(0.3),
            source: None,
            saved_at_ms: 0,
        }
    }

    #[test]
    fn calibrations_validate() {
        calibration(1280, 800).validate().expect("valid");
        assert!(CameraCalibration { fx: 0.0, ..calibration(1280, 800) }.validate().is_err());
        assert!(CameraCalibration { distortion: Distortion { p1: 0.01, ..Distortion::default() }, ..calibration(1280, 800) }.validate().is_err(), "fisheye has no tangential terms");
        assert!(CameraCalibration { model: LensModel::Pinhole, distortion: Distortion { p1: 0.01, ..Distortion::default() }, ..calibration(1280, 800) }.validate().is_ok());
    }

    #[test]
    fn calibrations_match_by_size_and_scale_by_aspect() {
        let stored = [calibration(1280, 800)];
        assert_eq!(match_calibration(&stored, None), CalibrationMatch::Exact(stored[0].clone()));
        assert_eq!(match_calibration(&stored, Some((1280, 800))), CalibrationMatch::Exact(stored[0].clone()));
        let CalibrationMatch::Scaled { calibration: half, from } = match_calibration(&stored, Some((640, 400))) else { panic!("scaled") };
        assert_eq!(from, (1280, 800));
        assert_eq!((half.fx, half.fy, half.cx, half.cy), (280.0, 280.0, 319.5, 199.5));
        assert_eq!(half.distortion, stored[0].distortion);
        assert!(matches!(match_calibration(&stored, Some((640, 480))), CalibrationMatch::None(_)));
        assert!(matches!(match_calibration(&[], None), CalibrationMatch::None(_)));
    }

    #[test]
    fn context_fields_are_eidos_ports() {
        let mount = CameraMount { x: 0.3, y: 0.0, z: 0.25, roll: 0.0, pitch: -15.0, yaw: 180.0 };
        let fields = context_fields(Some(&calibration(1280, 800)), Some(&mount));
        assert_eq!(fields["lens"], TypedConfigValue::String("fisheye".into()));
        assert_eq!(fields["fx"], TypedConfigValue::String("560".into()));
        assert_eq!(fields["k1"], TypedConfigValue::String("0.1".into()));
        assert_eq!(fields["mount"], TypedConfigValue::String("body".into()));
        assert_eq!(fields["mount_yaw"], TypedConfigValue::String(format!("{}", std::f64::consts::PI)));
        let uncalibrated = context_fields(None, None);
        assert_eq!(uncalibrated["fx"], TypedConfigValue::String("0".into()));
        assert_eq!(uncalibrated["mount"], TypedConfigValue::String("none".into()));
        assert_eq!(uncalibrated.len(), 20);
    }
}
