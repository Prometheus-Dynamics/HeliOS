//! Camera calibration (stored per camera and resolution) and the camera context the API writes
//! into a pipeline's camera bindings for helios-engine: `binding.<input>.context.<field>`, which
//! the engine turns into the structured values of the graph's held `camera`
//! (`eidos:camera_calibration`) and `extrinsics` (`eidos:camera_extrinsics`) inputs: the
//! calibration matching the binding's frame size (`camera.*`) and the camera's mount on the robot
//! (`mount.*`). Numbers are Orion `TypedConfigValue::F64`, with the unit in the key.
//!
//! Without a matching calibration the context says `fx = fy = 0`, which Eidos reports as
//! `status: "uncalibrated"` with no poses (it never assumes a camera), and the lens is the board's
//! camera module lens ([`board_lens_hint`]) or `unknown`: the live mapper's self-calibration then
//! fits that model, or chooses one from the tag views. Without a mount there are no `mount.*`
//! fields, and the multi-tag pose has no robot pose.

use std::{collections::BTreeMap, path::PathBuf, sync::OnceLock};

use orion::control_plane::TypedConfigValue;
use serde::{Deserialize, Serialize};

use crate::{
    camera_uncertainty::CalibrationUncertainty,
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

/// How the sensor reads out its image: mirrored left-right (`hflip`), top-bottom (`vflip`), or
/// both (turned 180°). The camera's `hflip`, `vflip` and `rotation` controls (Styx's service
/// controls) set it; a calibration keeps the one it was taken in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Orientation {
    pub hflip: bool,
    pub vflip: bool,
}

impl Orientation {
    /// The orientation a camera's stored settings ask for: `rotation: 180` turns it, and `hflip`
    /// and `vflip` mirror it on top of that.
    pub fn from_settings(settings: &BTreeMap<String, serde_json::Value>) -> Self {
        let on = |key: &str| settings.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
        let turned = settings.get("rotation").and_then(serde_json::Value::as_i64) == Some(180);
        Self { hflip: on("hflip") != turned, vflip: on("vflip") != turned }
    }

    /// Turned 180° (both mirrors): a rotation about the optical axis, unlike one mirror.
    pub fn turned(self) -> bool {
        self.hflip && self.vflip
    }
}

/// Where a calibration came from when it was not given by a user: a live mapper's
/// self-calibration (`method: "self-calibration"`, Eidos's fit of its views). Absent on a
/// calibration a user made, and on any stored before this existed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// How it was made: `self-calibration`.
    pub method: String,
    /// The views the fit used (Eidos's count).
    pub views: u32,
    /// The frames the mapper saw while fitting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// Eidos's revision of the fit (each accepted refit increments it), when it reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// The pipeline whose mapper made the fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
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
    /// How well the intrinsics are known (Eidos's `uncertainty`); absent means estimated, the
    /// library's conservative defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<CalibrationUncertainty>,
    /// Where it came from (a tool, a board, a date); informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The sensor orientation the calibration was taken in (unflipped when not said).
    #[serde(default, skip_serializing_if = "is_default")]
    pub orientation: Orientation,
    /// Set by the API when stored.
    #[serde(default)]
    pub saved_at_ms: u64,
    /// Set when the calibration was fitted by a live mapper rather than given (`None`: a user's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
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
        if let Some(uncertainty) = &self.uncertainty {
            uncertainty.validate(self.model).map_err(ApiError::unprocessable)?;
        }
        Ok(())
    }

    /// The same camera at `width` x `height` (same aspect ratio, a scaled capture): focal
    /// lengths scale, the principal point scales about pixel centres, distortion is unchanged
    /// (it works on normalized coordinates).
    pub fn scaled_to(&self, width: u32, height: u32) -> Self {
        let sx = f64::from(width) / f64::from(self.width);
        let sy = f64::from(height) / f64::from(self.height);
        let uncertainty = self.uncertainty.as_ref().map(|u| u.scaled(sx, sy));
        Self { width, height, fx: self.fx * sx, fy: self.fy * sy, cx: (self.cx + 0.5) * sx - 0.5, cy: (self.cy + 0.5) * sy - 0.5, uncertainty, ..self.clone() }
    }

    /// The same camera read out in `orientation`: a mirror moves the principal point to the
    /// mirrored pixel (`cx' = width - 1 - cx`, `cy' = height - 1 - cy`) and turns the sign of
    /// the tangential term along it (`p2` left-right, `p1` top-bottom); radial terms and focal
    /// lengths are unchanged.
    pub fn oriented(&self, orientation: Orientation) -> Self {
        let (h, v) = (self.orientation.hflip != orientation.hflip, self.orientation.vflip != orientation.vflip);
        let mut out = Self { orientation, uncertainty: self.uncertainty.as_ref().map(|u| u.mirrored(self.model, h, v)), ..self.clone() };
        if h {
            out.cx = f64::from(self.width) - 1.0 - self.cx;
            out.distortion.p2 = -self.distortion.p2;
        }
        if v {
            out.cy = f64::from(self.height) - 1.0 - self.cy;
            out.distortion.p1 = -self.distortion.p1;
        }
        out
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
/// with an uncertainty, `camera.uncertainty.*` (see `uncertainty_fields`);
/// Eidos reports `image_size_mismatch` for frames of another size), and with a mount
/// `mount.x_m`, `mount.y_m`, `mount.z_m`, `mount.roll_rad`, `mount.pitch_rad`, `mount.yaw_rad`:
/// the API's mount (metres and degrees, WPILib's robot frame: x forward, y left, z up;
/// `Rotation3d(roll, pitch, yaw)`) is the pose of the camera's forward-left-up frame on the
/// robot (`helios_field::CameraMount`).
///
/// `orientation` is the camera's now: the calibration is turned into it, and a camera turned
/// 180° adds 180° of roll to the mount (its image's frame is the body's, turned about the optical
/// axis). One mirror has no rotation that matches it; tags do not decode mirrored either.
pub fn context_fields(calibration: Option<&CameraCalibration>, mount: Option<&CameraMount>, orientation: Orientation) -> BTreeMap<String, TypedConfigValue> {
    let mut fields = BTreeMap::new();
    let calibration = calibration.map(|c| c.oriented(orientation));
    let uncertainty = calibration.as_ref().map(|c| uncertainty_fields(c.uncertainty.as_ref())).unwrap_or_default();
    let calibrated = calibration.is_some();
    let (lens, intrinsics, distortion, size) = match calibration {
        Some(c) => (c.model, [c.fx, c.fy, c.cx, c.cy], c.distortion, (c.width, c.height)),
        None => (LensModel::Pinhole, [0.0; 4], Distortion::default(), (0, 0)),
    };
    let lens = if calibrated { lens.name() } else { board_lens_hint().map_or(UNKNOWN_LENS, LensModel::name) };
    fields.insert("camera.lens".to_string(), TypedConfigValue::String(lens.into()));
    if !calibrated && let Some(prior) = board_lens().and_then(|board| board.prior.as_ref()) {
        for (key, value) in prior.oriented(orientation).fields() {
            fields.insert(format!("camera.prior.{key}"), value);
        }
    }
    for (name, value) in ["fx_px", "fy_px", "cx_px", "cy_px"].into_iter().zip(intrinsics) {
        fields.insert(format!("camera.{name}"), TypedConfigValue::F64(value));
    }
    for (name, value) in distortion.fields() {
        fields.insert(format!("camera.{name}"), TypedConfigValue::F64(value));
    }
    fields.extend(uncertainty);
    fields.insert("camera.width_px".to_string(), TypedConfigValue::Int(i64::from(size.0)));
    fields.insert("camera.height_px".to_string(), TypedConfigValue::Int(i64::from(size.1)));
    if let Some(m) = mount {
        let roll = if orientation.turned() { m.roll + 180.0 } else { m.roll };
        let pose = [("x_m", m.x), ("y_m", m.y), ("z_m", m.z), ("roll_rad", roll.to_radians()), ("pitch_rad", m.pitch.to_radians()), ("yaw_rad", m.yaw.to_radians())];
        for (name, value) in pose {
            fields.insert(format!("mount.{name}"), TypedConfigValue::F64(value));
        }
    }
    fields
}

/// The `camera.lens` of an uncalibrated camera whose lens the board doesn't say: Eidos's
/// self-calibration then chooses the model from the tag views.
pub const UNKNOWN_LENS: &str = "unknown";
/// The board's camera description (Atlas's device package; the Raze's names its camera module's
/// lens), unless `HELIOS_BOARD_CAMERA_FILE` names another.
pub const DEFAULT_BOARD_CAMERA_FILE: &str = "/usr/share/board/raze/camera.toml";

/// The board's camera module lens: its model, and one module's fit as a self-calibration prior
/// (`[camera.lens.hint]`), never applied as a calibration.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardLens {
    pub model: LensModel,
    pub prior: Option<LensPrior>,
}

/// One module's fit (Eidos's `prior`): focal lengths, principal point and distortion at
/// `width` x `height`, with how far a unit may be from it (`sigma` relative, for the focal length).
#[derive(Debug, Clone, PartialEq)]
pub struct LensPrior {
    pub width: u32,
    pub height: u32,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    /// `k1` to `k4` (the fisheye's coefficients; a pinhole's first four radial ones).
    pub distortion: [f64; 4],
    pub sigma: Option<f64>,
}

impl LensPrior {
    /// The prior for a camera read out flipped (the module's fit is of the unflipped array):
    /// the principal point mirrors, as a calibration's does. The fisheye's radial coefficients
    /// don't change.
    pub fn oriented(&self, orientation: Orientation) -> Self {
        let mut out = self.clone();
        if orientation.hflip {
            out.cx = f64::from(self.width) - 1.0 - self.cx;
        }
        if orientation.vflip {
            out.cy = f64::from(self.height) - 1.0 - self.cy;
        }
        out
    }

    /// The `camera.prior.*` context fields (without the prefix).
    fn fields(&self) -> Vec<(&'static str, TypedConfigValue)> {
        let mut fields = vec![
            ("fx_px", TypedConfigValue::F64(self.fx)),
            ("fy_px", TypedConfigValue::F64(self.fy)),
            ("cx_px", TypedConfigValue::F64(self.cx)),
            ("cy_px", TypedConfigValue::F64(self.cy)),
            ("width_px", TypedConfigValue::Int(i64::from(self.width))),
            ("height_px", TypedConfigValue::Int(i64::from(self.height))),
        ];
        fields.extend(["k1", "k2", "k3", "k4"].into_iter().zip(self.distortion).map(|(name, value)| (name, TypedConfigValue::F64(value))));
        if let Some(sigma) = self.sigma {
            fields.push(("sigma", TypedConfigValue::F64(sigma)));
        }
        fields
    }
}

/// The board's camera module lens, read once from `HELIOS_BOARD_CAMERA_FILE` or
/// [`DEFAULT_BOARD_CAMERA_FILE`]; `None` without the file or a model this crate knows.
pub fn board_lens() -> Option<&'static BoardLens> {
    static LENS: OnceLock<Option<BoardLens>> = OnceLock::new();
    LENS.get_or_init(|| {
        let path = std::env::var_os("HELIOS_BOARD_CAMERA_FILE").map_or_else(|| PathBuf::from(DEFAULT_BOARD_CAMERA_FILE), PathBuf::from);
        std::fs::read_to_string(path).ok().and_then(|text| lens_of_camera_toml(&text))
    })
    .as_ref()
}

/// The board's camera module lens model, the hint an uncalibrated camera carries.
pub fn board_lens_hint() -> Option<LensModel> {
    board_lens().map(|lens| lens.model)
}

/// `[camera.lens]` of a board camera description: `model` (`pinhole` or `fisheye`) and, when
/// complete, the `[camera.lens.hint]` prior (`width`, `height`, `fx_px`, `fy_px`, `cx_px`,
/// `cy_px`, `sigma`, `[camera.lens.hint.distortion] k1`..`k4`).
fn lens_of_camera_toml(text: &str) -> Option<BoardLens> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let lens = value.get("camera")?.get("lens")?;
    let model = match lens.get("model")?.as_str()? {
        "pinhole" => LensModel::Pinhole,
        "fisheye" => LensModel::Fisheye,
        _ => return None,
    };
    let prior = lens.get("hint").and_then(|hint| {
        let number = |table: &toml::Value, key: &str| table.get(key).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
        let size = |key: &str| hint.get(key).and_then(toml::Value::as_integer).and_then(|v| u32::try_from(v).ok());
        let distortion = hint.get("distortion");
        let k = |key: &str| distortion.and_then(|d| number(d, key)).unwrap_or(0.0);
        Some(LensPrior {
            width: size("width")?,
            height: size("height")?,
            fx: number(hint, "fx_px")?,
            fy: number(hint, "fy_px")?,
            cx: number(hint, "cx_px")?,
            cy: number(hint, "cy_px")?,
            distortion: [k("k1"), k("k2"), k("k3"), k("k4")],
            sigma: number(hint, "sigma"),
        })
    });
    Some(BoardLens { model, prior })
}

/// The `camera.uncertainty.*` fields of a calibration that has an uncertainty (none otherwise:
/// Eidos reads an absent one as estimated). The sigma list and the covariance are JSON strings,
/// as context carries no lists.
fn uncertainty_fields(uncertainty: Option<&CalibrationUncertainty>) -> BTreeMap<String, TypedConfigValue> {
    let Some(u) = uncertainty else { return BTreeMap::new() };
    let json = |values: &[f64]| serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
    let numbers = [("sigma.fx", u.sigma.fx), ("sigma.fy", u.sigma.fy), ("sigma.cx", u.sigma.cx), ("sigma.cy", u.sigma.cy), ("rms_px", u.rms_px)];
    let mut fields = BTreeMap::new();
    fields.insert("camera.uncertainty.kind".to_string(), TypedConfigValue::String(u.kind.name().into()));
    fields.insert("camera.uncertainty.views".to_string(), TypedConfigValue::Int(i64::from(u.views)));
    for (name, value) in numbers {
        fields.insert(format!("camera.uncertainty.{name}"), TypedConfigValue::F64(value));
    }
    fields.insert("camera.uncertainty.sigma.distortion".to_string(), TypedConfigValue::String(json(&u.sigma.distortion)));
    fields.insert("camera.uncertainty.covariance".to_string(), TypedConfigValue::String(json(&u.covariance)));
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
            uncertainty: None,
            source: None,
            orientation: Orientation::default(),
            saved_at_ms: 0,
            provenance: None,
        }
    }

    #[test]
    fn calibrations_follow_the_orientation() {
        let base = CameraCalibration { model: LensModel::Pinhole, cx: 600.0, cy: 380.0, distortion: Distortion { p1: 0.01, p2: -0.02, ..Distortion::default() }, ..calibration(1280, 800) };
        let turned = base.oriented(Orientation { hflip: true, vflip: true });
        assert_eq!((turned.cx, turned.cy, turned.distortion.p1, turned.distortion.p2), (679.0, 419.0, -0.01, 0.02));
        assert_eq!(turned.oriented(Orientation::default()), base, "and back");
        let mirrored = base.oriented(Orientation { hflip: true, vflip: false });
        assert_eq!((mirrored.cx, mirrored.cy, mirrored.distortion.p1, mirrored.distortion.p2), (679.0, 380.0, 0.01, 0.02));
        let settings = |pairs: &[(&str, serde_json::Value)]| pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>();
        assert_eq!(Orientation::from_settings(&settings(&[("rotation", 180.into())])), Orientation { hflip: true, vflip: true });
        assert_eq!(Orientation::from_settings(&settings(&[("rotation", 180.into()), ("hflip", true.into())])), Orientation { hflip: false, vflip: true });
        assert_eq!(Orientation::from_settings(&settings(&[])), Orientation::default());
        let mount = CameraMount { x: 0.0, y: 0.0, z: 0.0, roll: 10.0, pitch: 0.0, yaw: 0.0 };
        let fields = context_fields(Some(&base), Some(&mount), Orientation { hflip: true, vflip: true });
        assert_eq!(fields["camera.cx_px"], TypedConfigValue::F64(679.0));
        assert_eq!(fields["mount.roll_rad"], TypedConfigValue::F64(190f64.to_radians()));
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
        let fields = context_fields(Some(&calibration(1280, 800)), Some(&mount), Orientation::default());
        assert_eq!(fields["camera.lens"], TypedConfigValue::String("fisheye".into()));
        assert_eq!(fields["camera.fx_px"], TypedConfigValue::F64(560.0));
        assert_eq!(fields["camera.k1"], TypedConfigValue::F64(0.1));
        assert_eq!(fields["camera.width_px"], TypedConfigValue::Int(1280));
        assert_eq!(fields["mount.x_m"], TypedConfigValue::F64(0.3));
        assert_eq!(fields["mount.yaw_rad"], TypedConfigValue::F64(std::f64::consts::PI));
        assert_eq!(fields.len(), 21);
        let uncalibrated = context_fields(None, None, Orientation::default());
        assert_eq!(uncalibrated["camera.fx_px"], TypedConfigValue::F64(0.0));
        let hint = board_lens_hint().map_or(UNKNOWN_LENS, LensModel::name);
        assert_eq!(uncalibrated["camera.lens"], TypedConfigValue::String(hint.into()), "the board's lens, or unknown");
        assert_eq!(uncalibrated["camera.width_px"], TypedConfigValue::Int(0));
        assert!(!uncalibrated.keys().any(|key| key.starts_with("mount.")), "no mount, no mount fields");
        assert_eq!(uncalibrated.len(), 15);
    }

    #[test]
    fn the_board_camera_file_names_the_lens_and_its_prior() {
        let raze = include_str!("../../../../../tools/sim/raze-camera.toml");
        let lens = lens_of_camera_toml(raze).expect("the Raze's camera.toml");
        assert_eq!(lens.model, LensModel::Fisheye);
        let prior = lens.prior.expect("its prior");
        assert_eq!((prior.width, prior.height, prior.fx, prior.cy), (1280, 800, 566.508, 351.807));
        assert_eq!(prior.distortion[0], -0.0156148);
        assert_eq!(prior.sigma, Some(0.05));
        let turned = prior.oriented(Orientation { hflip: true, vflip: true });
        assert_eq!((turned.cx, turned.cy), (1279.0 - 627.29, 799.0 - 351.807), "a turned camera's prior mirrors its principal point");
        let fields: BTreeMap<_, _> = prior.fields().into_iter().collect();
        assert_eq!(fields["width_px"], TypedConfigValue::Int(1280));
        assert_eq!(fields["k4"], TypedConfigValue::F64(0.00147003));
        // A model alone, or an incomplete hint: no prior.
        assert_eq!(lens_of_camera_toml("[camera.lens]\nmodel = \"pinhole\"\n"), Some(BoardLens { model: LensModel::Pinhole, prior: None }));
        assert_eq!(lens_of_camera_toml("[camera.lens]\nmodel = \"fisheye\"\n[camera.lens.hint]\nfx_px = 566.0\n").and_then(|l| l.prior), None);
        assert_eq!(lens_of_camera_toml("[camera.lens]\nmodel = \"catadioptric\"\n"), None);
        assert_eq!(lens_of_camera_toml("[camera]\nsensor = \"ov9782\"\n"), None);
    }

    fn solved_uncertainty() -> CalibrationUncertainty {
        CalibrationUncertainty::solved(&(0..81).map(|k| if k % 10 == 0 { 0.01 } else { 0.0 }).collect::<Vec<_>>(), 0.25, 30, LensModel::Pinhole)
    }

    #[test]
    fn uncertainty_fields_are_written_only_with_an_uncertainty() {
        let plain = context_fields(Some(&calibration(1280, 800)), None, Orientation::default());
        assert!(!plain.keys().any(|key| key.starts_with("camera.uncertainty.")), "estimated: no fields");
        let solved = CameraCalibration { model: LensModel::Pinhole, uncertainty: Some(solved_uncertainty()), ..calibration(1280, 800) };
        let fields = context_fields(Some(&solved), None, Orientation::default());
        assert_eq!(fields["camera.uncertainty.kind"], TypedConfigValue::String("solved".into()));
        assert_eq!(fields["camera.uncertainty.rms_px"], TypedConfigValue::F64(0.25));
        assert_eq!(fields["camera.uncertainty.views"], TypedConfigValue::Int(30));
        assert_eq!(fields["camera.uncertainty.sigma.fx"], TypedConfigValue::F64(0.1));
        assert_eq!(fields["camera.uncertainty.sigma.distortion"], TypedConfigValue::String("[0.1,0.1,0.1,0.1,0.1]".into()));
        let covariance: Vec<f64> = match &fields["camera.uncertainty.covariance"] {
            TypedConfigValue::String(text) => serde_json::from_str(text).expect("json array"),
            other => panic!("covariance is a string: {other:?}"),
        };
        assert_eq!(covariance.len(), 81);
        assert_eq!(fields.len(), 15 + 9, "the calibrated fields plus nine uncertainty ones");
        assert!(!context_fields(None, None, Orientation::default()).keys().any(|key| key.starts_with("camera.uncertainty.")));
    }

    #[test]
    fn uncertainty_follows_scale_and_orientation() {
        let base = CameraCalibration { model: LensModel::Pinhole, uncertainty: Some(solved_uncertainty()), ..calibration(1280, 800) };
        let half = base.scaled_to(640, 400);
        assert_eq!(half.uncertainty.as_ref().map(|u| u.sigma.fx), Some(0.05), "fx scales with sx");
        let turned = base.oriented(Orientation { hflip: true, vflip: true });
        let u = turned.uncertainty.clone().expect("kept");
        assert_eq!(u.sigma.cx, base.uncertainty.as_ref().expect("base").sigma.cx, "a mirror keeps the variance");
        assert_eq!(turned.oriented(Orientation::default()).uncertainty, base.uncertainty, "and back");
    }

    #[test]
    fn stored_calibrations_without_an_uncertainty_load_and_validate_with_one() {
        let stored = serde_json::json!({ "width": 1280, "height": 800, "model": "pinhole", "fx": 600.0, "fy": 600.0, "cx": 639.5, "cy": 399.5, "saved_at_ms": 7 });
        let loaded: CameraCalibration = serde_json::from_value(stored).expect("loads");
        assert_eq!(loaded.uncertainty, None);
        assert!(!serde_json::to_value(&loaded).expect("json").as_object().expect("object").contains_key("uncertainty"));
        let with = CameraCalibration { uncertainty: Some(solved_uncertainty()), ..loaded.clone() };
        let json = serde_json::to_value(&with).expect("json");
        assert_eq!(serde_json::from_value::<CameraCalibration>(json).expect("round trip"), with);
        with.validate().expect("valid");
        let bad = CameraCalibration { uncertainty: Some(CalibrationUncertainty { rms_px: f64::NAN, ..solved_uncertainty() }), ..loaded };
        assert!(bad.validate().is_err());
    }
}
