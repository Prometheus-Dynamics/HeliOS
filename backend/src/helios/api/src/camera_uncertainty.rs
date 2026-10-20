//! The uncertainty of a camera calibration, as Eidos's `eidos:camera_calibration.uncertainty`
//! takes it: how well the intrinsics are known. The pose nodes turn it into their covariances, so
//! a solved calibration gives tighter (still honest) pose covariances than the conservative
//! `estimated` default.
//!
//! `sigma` and `covariance` use Eidos's order: `fx`, `fy`, `cx`, `cy`, then the distortion terms
//! as the lens's fit gives them (pinhole `k1`, `k2`, `p1`, `p2`, `k3`; fisheye `k1` to `k4`).
//! `covariance` is square and row-major, `4 + distortion.len()` on a side, and empty when not
//! given; Eidos then uses the sigmas, or its conservative mapping of `rms_px` and `views`.

use serde::{Deserialize, Deserializer, Serialize, de::Error};

use crate::camera_context::LensModel;

/// The number of distortion terms a lens's fit has (Eidos's `distortion_order`).
pub fn distortion_len(model: LensModel) -> usize {
    match model {
        LensModel::Pinhole => 5,
        LensModel::Fisheye => 4,
    }
}

/// Where the pinhole tangential terms sit among the distortion terms (`k1`, `k2`, `p1`, `p2`,
/// `k3`). A fisheye has none.
const PINHOLE_P1: usize = 2;
const PINHOLE_P2: usize = 3;

/// How the intrinsics are known (`uncertainty.kind`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UncertaintyKind {
    /// The library's conservative defaults; also what an absent uncertainty means.
    #[default]
    Estimated,
    /// The calibration is exact.
    Known,
    /// Solved from views: the sigmas and covariance say how well.
    Solved,
}

impl UncertaintyKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Estimated => "estimated",
            Self::Known => "known",
            Self::Solved => "solved",
        }
    }
}

/// One standard deviation per intrinsic (pixels; distortion in its own units).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UncertaintySigma {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub distortion: Vec<f64>,
}

/// The calibration's uncertainty. Stored calibrations without one load as they did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CalibrationUncertainty {
    pub kind: UncertaintyKind,
    /// The fit's RMS reprojection error, pixels.
    pub rms_px: f64,
    /// The number of views the calibration was solved from.
    pub views: u32,
    pub sigma: UncertaintySigma,
    /// Row-major covariance of `fx`, `fy`, `cx`, `cy` and the distortion terms. Given as an array
    /// or as a JSON string of one, as Eidos takes it.
    #[serde(deserialize_with = "list_or_json_string")]
    pub covariance: Vec<f64>,
}

/// A list of numbers, or a JSON string holding one (the flat context fields carry it that way).
fn list_or_json_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrText {
        List(Vec<f64>),
        Text(String),
    }
    match ListOrText::deserialize(deserializer)? {
        ListOrText::List(values) => Ok(values),
        ListOrText::Text(text) if text.trim().is_empty() => Ok(Vec::new()),
        ListOrText::Text(text) => serde_json::from_str::<Vec<f64>>(&text).map_err(D::Error::custom),
    }
}

impl CalibrationUncertainty {
    /// The uncertainty a solved fit gives: its covariance (`covariance`, `4 + n` square in
    /// Eidos's order, empty when the fit formed none) and the sigmas on its diagonal. Without a
    /// covariance the sigmas are zero, so Eidos maps `rms_px` and `views` to a conservative one.
    pub fn solved(covariance: &[f64], rms_px: f64, views: usize, model: LensModel) -> Self {
        let n = 4 + distortion_len(model);
        let covariance = if covariance.len() == n * n { covariance.to_vec() } else { Vec::new() };
        let sigma_at = |i: usize| if covariance.is_empty() { 0.0 } else { covariance[i * n + i].max(0.0).sqrt() };
        Self {
            kind: UncertaintyKind::Solved,
            rms_px,
            views: u32::try_from(views).unwrap_or(u32::MAX),
            sigma: UncertaintySigma { fx: sigma_at(0), fy: sigma_at(1), cx: sigma_at(2), cy: sigma_at(3), distortion: (4..n).map(sigma_at).collect() },
            covariance,
        }
    }

    /// Checks the sizes against `model` and that every number is finite and sensible. A missing
    /// sigma list is allowed only when nothing else is given either (Eidos then reads it as
    /// rms-and-views only).
    pub fn validate(&self, model: LensModel) -> Result<(), String> {
        let d = distortion_len(model);
        let no_sigmas = self.sigma.distortion.is_empty() && self.covariance.is_empty() && self.sigma.fx == 0.0 && self.sigma.fy == 0.0 && self.sigma.cx == 0.0 && self.sigma.cy == 0.0;
        if self.sigma.distortion.len() != d && !no_sigmas {
            return Err(format!("uncertainty.sigma.distortion has {} entries; a {} calibration has {d}", self.sigma.distortion.len(), model.name()));
        }
        let n = 4 + d;
        if !self.covariance.is_empty() && self.covariance.len() != n * n {
            return Err(format!("uncertainty.covariance has {} entries; expected {n} x {n}", self.covariance.len()));
        }
        let sigmas = [self.sigma.fx, self.sigma.fy, self.sigma.cx, self.sigma.cy].into_iter().chain(self.sigma.distortion.iter().copied());
        if !self.rms_px.is_finite() || self.rms_px < 0.0 || sigmas.clone().any(|value| !value.is_finite() || value < 0.0) {
            return Err("uncertainty rms_px and sigmas must be finite and non-negative".into());
        }
        if self.covariance.iter().any(|value| !value.is_finite()) {
            return Err("uncertainty covariance must be finite".into());
        }
        Ok(())
    }

    /// The same camera at `sx` and `sy` times the size: `fx` and `cx` scale with `sx`, `fy` and
    /// `cy` with `sy`, the distortion terms are unchanged.
    pub fn scaled(&self, sx: f64, sy: f64) -> Self {
        let mut factors = vec![sx, sy, sx, sy];
        factors.resize(4 + self.sigma.distortion.len(), 1.0);
        self.transformed(&factors)
    }

    /// The same camera read out mirrored: `h` flips `cx` and `p2`, `v` flips `cy` and `p1`
    /// (pinhole only; a fisheye has no tangential terms). Each flipped parameter turns the sign
    /// of every covariance term it shares with another, so `cov(cx, p2)` (both flipped) keeps its
    /// sign; variances never change.
    pub fn mirrored(&self, model: LensModel, h: bool, v: bool) -> Self {
        let mut factors = vec![1.0; 4 + self.sigma.distortion.len()];
        if h {
            factors[2] = -1.0;
        }
        if v {
            factors[3] = -1.0;
        }
        if model == LensModel::Pinhole {
            if let (true, Some(p2)) = (h, factors.get_mut(4 + PINHOLE_P2)) {
                *p2 = -1.0;
            }
            if let (true, Some(p1)) = (v, factors.get_mut(4 + PINHOLE_P1)) {
                *p1 = -1.0;
            }
        }
        self.transformed(&factors)
    }

    /// Each parameter `i` moves by `factors[i]` (a scale or a sign): its sigma by `|factors[i]|`,
    /// and `cov(i, j)` by `factors[i] * factors[j]`.
    fn transformed(&self, factors: &[f64]) -> Self {
        let n = factors.len();
        let mut out = self.clone();
        out.sigma.fx = self.sigma.fx * factors[0].abs();
        out.sigma.fy = self.sigma.fy * factors[1].abs();
        out.sigma.cx = self.sigma.cx * factors[2].abs();
        out.sigma.cy = self.sigma.cy * factors[3].abs();
        for (i, value) in out.sigma.distortion.iter_mut().enumerate() {
            *value = self.sigma.distortion[i] * factors[4 + i].abs();
        }
        if self.covariance.len() == n * n {
            out.covariance = (0..n * n).map(|k| factors[k / n] * factors[k % n] * self.covariance[k]).collect();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pinhole fit's covariance with distinct, symmetric entries: `(4 + 5)` square.
    fn covariance() -> Vec<f64> {
        let n = 9;
        (0..n * n)
            .map(|k| {
                let (r, c) = (k / n, k % n);
                let (a, b) = (r.min(c) as f64, r.max(c) as f64);
                if r == c { 0.01 * (a + 2.0) } else { 0.001 * (a + 1.0) * (b + 3.0) * if ((a + b) as usize).is_multiple_of(2) { 1.0 } else { -1.0 } }
            })
            .collect()
    }

    fn solved() -> CalibrationUncertainty {
        CalibrationUncertainty::solved(&covariance(), 0.3, 40, LensModel::Pinhole)
    }

    fn cov(u: &CalibrationUncertainty, row: usize, col: usize) -> f64 {
        u.covariance[row * 9 + col]
    }

    #[test]
    fn solved_takes_sigmas_from_the_covariance_diagonal() {
        let u = solved();
        assert_eq!(u.kind, UncertaintyKind::Solved);
        assert_eq!((u.views, u.rms_px), (40, 0.3));
        assert!((u.sigma.fx - cov(&u, 0, 0).sqrt()).abs() < 1e-15);
        assert!((u.sigma.cy - cov(&u, 3, 3).sqrt()).abs() < 1e-15);
        assert_eq!(u.sigma.distortion.len(), 5);
        assert!((u.sigma.distortion[4] - cov(&u, 8, 8).sqrt()).abs() < 1e-15, "k3 is the fifth");
        u.validate(LensModel::Pinhole).expect("valid");
    }

    #[test]
    fn solved_without_a_covariance_is_rms_and_views_only() {
        let u = CalibrationUncertainty::solved(&[], 0.4, 12, LensModel::Fisheye);
        assert_eq!((u.sigma.fx, u.sigma.cx), (0.0, 0.0));
        assert_eq!(u.sigma.distortion, vec![0.0; 4]);
        assert!(u.covariance.is_empty());
        u.validate(LensModel::Fisheye).expect("valid");
    }

    #[test]
    fn round_trips_as_eidos_writes_it_and_takes_a_json_string_covariance() {
        let u = solved();
        let json = serde_json::to_value(&u).expect("serialize");
        assert_eq!(json["kind"], "solved");
        assert_eq!(json["sigma"]["distortion"].as_array().map(Vec::len), Some(5));
        let back: CalibrationUncertainty = serde_json::from_value(json.clone()).expect("round trip");
        assert_eq!(back, u);
        let as_text = serde_json::json!({ "kind": "solved", "covariance": serde_json::to_string(&covariance()).expect("text") });
        let text: CalibrationUncertainty = serde_json::from_value(as_text).expect("string covariance");
        assert_eq!(text.covariance, covariance());
        let empty: CalibrationUncertainty = serde_json::from_value(serde_json::json!({ "kind": "known", "covariance": "[]" })).expect("empty string");
        assert!(empty.covariance.is_empty());
        assert!(serde_json::from_value::<CalibrationUncertainty>(serde_json::json!({ "kind": "precise" })).is_err());
    }

    #[test]
    fn scaling_scales_focal_lengths_and_principal_point_only() {
        let u = solved();
        let s = u.scaled(0.5, 0.25);
        assert!((s.sigma.fx - u.sigma.fx * 0.5).abs() < 1e-15 && (s.sigma.fy - u.sigma.fy * 0.25).abs() < 1e-15);
        assert!((s.sigma.cx - u.sigma.cx * 0.5).abs() < 1e-15 && (s.sigma.cy - u.sigma.cy * 0.25).abs() < 1e-15);
        assert_eq!(s.sigma.distortion, u.sigma.distortion, "distortion is unchanged");
        // cov(fx, k1) scales by sx, cov(cy, p2) by sy, cov(k1, k2) not at all.
        assert!((cov(&s, 0, 4) - cov(&u, 0, 4) * 0.5).abs() < 1e-15);
        assert!((cov(&s, 3, 7) - cov(&u, 3, 7) * 0.25).abs() < 1e-15);
        assert_eq!(cov(&s, 4, 5), cov(&u, 4, 5));
        assert!((cov(&s, 2, 2) - cov(&u, 2, 2) * 0.25).abs() < 1e-15, "cx variance scales with sx squared");
    }

    #[test]
    fn mirroring_horizontally_flips_cx_and_p2_cross_terms_and_keeps_variances() {
        let u = solved();
        let m = u.mirrored(LensModel::Pinhole, true, false);
        for i in 0..9 {
            assert_eq!(cov(&m, i, i), cov(&u, i, i), "variance {i}");
        }
        // cx flips against every other parameter but itself; so does p2 (index 7).
        for j in [0, 1, 3, 4, 5, 6, 8] {
            assert_eq!(cov(&m, 2, j), -cov(&u, 2, j), "cov(cx, {j})");
            assert_eq!(cov(&m, 7, j), if j == 2 { cov(&u, 7, j) } else { -cov(&u, 7, j) }, "cov(p2, {j})");
        }
        assert_eq!(cov(&m, 2, 7), cov(&u, 2, 7), "cx and p2 both flip: their cross-term keeps its sign");
        assert_eq!(cov(&m, 0, 1), cov(&u, 0, 1), "untouched pairs keep their terms");
        assert_eq!(m.sigma.cx, u.sigma.cx);
        assert_eq!(m.sigma.distortion[3], u.sigma.distortion[3], "p2 sigma is a magnitude");
    }

    #[test]
    fn mirroring_vertically_flips_cy_and_p1() {
        let u = solved();
        let m = u.mirrored(LensModel::Pinhole, false, true);
        for i in 0..9 {
            assert_eq!(cov(&m, i, i), cov(&u, i, i), "variance {i}");
        }
        for j in [0, 1, 2, 4, 5, 7, 8] {
            assert_eq!(cov(&m, 3, j), -cov(&u, 3, j), "cov(cy, {j})");
            assert_eq!(cov(&m, 6, j), if j == 3 { cov(&u, 6, j) } else { -cov(&u, 6, j) }, "cov(p1, {j})");
        }
        assert_eq!(cov(&m, 3, 6), cov(&u, 3, 6), "cy and p1 both flip");
        assert_eq!(cov(&m, 2, 3), -cov(&u, 2, 3), "cy flips and cx does not: their cross-term flips");
    }

    #[test]
    fn mirroring_twice_is_the_identity_and_fisheye_has_no_tangential_terms() {
        let u = solved();
        let twice = u.mirrored(LensModel::Pinhole, true, true).mirrored(LensModel::Pinhole, true, true);
        assert_eq!(twice, u);
        let fisheye = CalibrationUncertainty::solved(&[], 0.4, 12, LensModel::Fisheye);
        assert_eq!(fisheye.mirrored(LensModel::Fisheye, true, true).sigma, fisheye.sigma);
    }

    #[test]
    fn validation_rejects_mismatched_sizes_and_bad_numbers() {
        let mut bad = solved();
        bad.sigma.distortion.pop();
        assert!(bad.validate(LensModel::Pinhole).is_err());
        let mut bad = solved();
        bad.covariance.pop();
        assert!(bad.validate(LensModel::Pinhole).is_err());
        let mut bad = solved();
        bad.sigma.fx = f64::NAN;
        assert!(bad.validate(LensModel::Pinhole).is_err());
        let mut bad = solved();
        bad.rms_px = -1.0;
        assert!(bad.validate(LensModel::Pinhole).is_err());
        assert!(CalibrationUncertainty::default().validate(LensModel::Pinhole).is_ok(), "estimated, nothing given");
    }
}
