//! Camera calibration (stored per camera and resolution) and the camera context the API writes
//! into a pipeline's camera bindings for helios-engine: `binding.<input>.context.<field>`, which
//! the engine turns into the structured values of the graph's held `camera`
//! (`eidos:camera_calibration`) and `extrinsics` (`eidos:camera_extrinsics`) inputs: the
//! calibration matching the binding's frame size (`camera.*`) and the camera's mount on the robot
//! (`mount.*`). Numbers are Orion `TypedConfigValue::F64`, with the unit in the key.
//!
//! Without a matching calibration the context says `fx = fy = 0`, which Eidos reports as
//! `status: "uncalibrated"` with no poses (it never assumes a camera). Without a mount there are
//! no `mount.*` fields, and the multi-tag pose has no robot pose.

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

/// The context fields for a camera binding: `camera.lens` (`pinhole` | `fisheye`),
/// `camera.fx_px`, `camera.fy_px`, `camera.cx_px`, `camera.cy_px`, `camera.k1` ... `camera.k6`,
/// `camera.p1`, `camera.p2`, `camera.width_px`, `camera.height_px` (the calibrated image size;
/// Eidos reports `image_size_mismatch` for frames of another size), and with a mount
/// `mount.x_m`, `mount.y_m`, `mount.z_m`, `mount.roll_rad`, `mount.pitch_rad`, `mount.yaw_rad`:
/// the API's mount (metres and degrees, WPILib's robot frame: x forward, y left, z up;
/// `Rotation3d(roll, pitch, yaw)`) is the pose of the camera's forward-left-up frame on the
/// robot (`helios_field::CameraMount`).
pub fn context_fields(calibration: Option<&CameraCalibration>, mount: Option<&CameraMount>) -> BTreeMap<String, TypedConfigValue> {
    let mut fields = BTreeMap::new();
    let (lens, intrinsics, distortion, size) = match calibration {
        Some(c) => (c.model, [c.fx, c.fy, c.cx, c.cy], c.distortion, (c.width, c.height)),
        None => (LensModel::Pinhole, [0.0; 4], Distortion::default(), (0, 0)),
    };
    fields.insert("camera.lens".to_string(), TypedConfigValue::String(lens.name().into()));
    for (name, value) in ["fx_px", "fy_px", "cx_px", "cy_px"].into_iter().zip(intrinsics) {
        fields.insert(format!("camera.{name}"), TypedConfigValue::F64(value));
    }
    for (name, value) in distortion.fields() {
        fields.insert(format!("camera.{name}"), TypedConfigValue::F64(value));
    }
    fields.insert("camera.width_px".to_string(), TypedConfigValue::Int(i64::from(size.0)));
    fields.insert("camera.height_px".to_string(), TypedConfigValue::Int(i64::from(size.1)));
    if let Some(m) = mount {
        let pose = [("x_m", m.x), ("y_m", m.y), ("z_m", m.z), ("roll_rad", m.roll.to_radians()), ("pitch_rad", m.pitch.to_radians()), ("yaw_rad", m.yaw.to_radians())];
        for (name, value) in pose {
            fields.insert(format!("mount.{name}"), TypedConfigValue::F64(value));
        }
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
    fn context_fields_are_typed_with_units() {
        let mount = CameraMount { x: 0.3, y: 0.0, z: 0.25, roll: 0.0, pitch: -15.0, yaw: 180.0 };
        let fields = context_fields(Some(&calibration(1280, 800)), Some(&mount));
        assert_eq!(fields["camera.lens"], TypedConfigValue::String("fisheye".into()));
        assert_eq!(fields["camera.fx_px"], TypedConfigValue::F64(560.0));
        assert_eq!(fields["camera.k1"], TypedConfigValue::F64(0.1));
        assert_eq!(fields["camera.width_px"], TypedConfigValue::Int(1280));
        assert_eq!(fields["mount.x_m"], TypedConfigValue::F64(0.3));
        assert_eq!(fields["mount.yaw_rad"], TypedConfigValue::F64(std::f64::consts::PI));
        assert_eq!(fields.len(), 21);
        let uncalibrated = context_fields(None, None);
        assert_eq!(uncalibrated["camera.fx_px"], TypedConfigValue::F64(0.0));
        assert_eq!(uncalibrated["camera.width_px"], TypedConfigValue::Int(0));
        assert!(!uncalibrated.keys().any(|key| key.starts_with("mount.")), "no mount, no mount fields");
        assert_eq!(uncalibrated.len(), 15);
    }
}
