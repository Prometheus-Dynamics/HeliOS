//! Estimate 3D pose information for planar ArUco detections.
//!
//! The helper converts a 2D quad (pixel corners) into a camera-relative translation by
//! decomposing the homography induced by the planar tag. This keeps the dependency surface
//! small (no OpenCV bindings) while producing stable metric translations as long as camera
//! intrinsics and tag size are known.

use nalgebra::{Matrix3, Rotation3, SMatrix, SVector, UnitQuaternion, Vector2, Vector3};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::warn;

use crate::Point;
use crate::Translation3;
use crate::modules::aruco::{
    ArucoDetection2D, DetectionPose, DetectionPoseCalibrationSummary, DetectionPoseFailureCounts, DetectionPoseFailureSample, DetectionPoseOutput, DetectionPoseRotation, DetectionPoseStats,
    DetectionQuat,
};
use crate::modules::calibration::LensModel;

static TAG_POSE_LOG_COUNT: AtomicU64 = AtomicU64::new(0);

mod homography;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagPoseMethod {
    Auto,
    HomographyV1,
    HomographyV2,
    PnpRefine,
}

impl TagPoseMethod {
    pub fn parse(raw: &str) -> Self {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "" | "auto" | "v2" | "homography_v2" | "homography2" => Self::Auto,
            "v1" | "homography_v1" | "homography1" | "legacy" => Self::HomographyV1,
            "pnp" | "pnp_refine" | "pnp_iter" | "pnp_iterative" | "refine" | "refined" => Self::PnpRefine,
            other => {
                warn!(method = other, "aruco:tag_poses unknown solver method; defaulting to homography_v2");
                Self::Auto
            }
        }
    }

    pub fn resolve(self) -> Self {
        match self {
            Self::Auto => Self::HomographyV2,
            other => other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::HomographyV1 => "homography_v1",
            Self::HomographyV2 => "homography_v2",
            Self::PnpRefine => "pnp_refine",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TagPoseCalibration {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub k1: f64,
    pub k2: f64,
    pub p1: f64,
    pub p2: f64,
    pub k3: f64,
    pub undistort_iters: u8,
    pub lens_model: LensModel,
}

#[derive(Default)]
struct PoseScratch {
    fx_samples: Vec<f64>,
    fy_samples: Vec<f64>,
    f_samples: Vec<f64>,
}

thread_local! {
    static POSE_SCRATCH: RefCell<PoseScratch> = RefCell::new(PoseScratch::default());
}

impl TagPoseCalibration {
    pub fn is_usable(&self) -> bool {
        self.fx.is_finite() && self.fy.is_finite() && self.fx.abs() > f64::EPSILON && self.fy.abs() > f64::EPSILON && self.cx.is_finite() && self.cy.is_finite()
    }

    fn undistort_pixel(&self, u: f64, v: f64) -> Option<(f64, f64)> {
        if !self.is_usable() {
            return None;
        }
        if !u.is_finite() || !v.is_finite() {
            return None;
        }

        let xd = (u - self.cx) / self.fx;
        let yd = (v - self.cy) / self.fy;
        let (xu, yu) = homography::undistort_norm(xd, yd, *self)?;
        Some((self.fx * xu + self.cx, self.fy * yu + self.cy))
    }
}

/// Convert a list of ArUco detections into a JSON array suitable for UI consumption.
///
/// The returned translation uses a Three.js-friendly camera basis:
/// - +X right
/// - +Y up
/// - -Z forward (matches the default Three.js camera looking down -Z)
pub fn detections_to_tag_pose_json(detections: &[ArucoDetection2D], calib: TagPoseCalibration, tag_size_m: f64, method: TagPoseMethod) -> serde_json::Value {
    let output = detections_to_tag_pose_output(detections, calib, tag_size_m, method);
    let value = serde_json::to_value(&output).unwrap_or_else(|err| {
        json!({
            "detections": [],
            "stats": {
                "inputDetections": detections.len(),
                "outputDetections": 0,
                "poseMethod": method.as_str(),
                "tagSize": tag_size_m,
                "calibration": { "fx": calib.fx, "fy": calib.fy, "cx": calib.cx, "cy": calib.cy },
                "estimatedIntrinsics": false,
                "failures": { "invalid": detections.len(), "homography": 0, "decompose": 0 },
                "firstFailure": {
                    "reason": "serialize",
                    "error": err.to_string()
                }
            }
        })
    });

    let idx = TAG_POSE_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx < 3 {
        warn!(
            target: "helios_engine::graph",
            detections = detections.len(),
            poses = output.stats.output_detections,
            estimated_intrinsics = output.stats.estimated_intrinsics,
            fx = output.stats.calibration.fx,
            fy = output.stats.calibration.fy,
            cx = output.stats.calibration.cx,
            cy = output.stats.calibration.cy,
            "aruco:tag_poses computed pose sample"
        );
    }

    value
}

pub fn detections_to_tag_pose_output(detections: &[ArucoDetection2D], calib: TagPoseCalibration, tag_size_m: f64, method: TagPoseMethod) -> DetectionPoseOutput {
    let mut failures_homography = 0usize;
    let mut failures_decompose = 0usize;
    let mut failures_invalid = 0usize;
    let mut first_failure: Option<DetectionPoseFailureSample> = None;

    if !tag_size_m.is_finite() || tag_size_m <= 0.0 {
        failures_invalid = detections.len();
        let stats = DetectionPoseStats {
            input_detections: detections.len(),
            output_detections: 0,
            pose_method: method.as_str().to_string(),
            tag_size: tag_size_m,
            calibration: DetectionPoseCalibrationSummary { fx: calib.fx, fy: calib.fy, cx: calib.cx, cy: calib.cy },
            estimated_intrinsics: false,
            failures: DetectionPoseFailureCounts { invalid: failures_invalid, homography: failures_homography, decompose: failures_decompose },
            first_failure,
        };
        return DetectionPoseOutput { detections: Vec::new(), stats };
    }

    let mut estimated_intrinsics = false;
    let calib = if calib.is_usable() {
        calib
    } else {
        match estimate_pinhole_intrinsics_from_detections(detections, calib.cx, calib.cy, tag_size_m) {
            Some((fx, fy)) => {
                estimated_intrinsics = true;
                TagPoseCalibration { fx, fy, ..calib }
            }
            None => {
                failures_invalid = detections.len();
                let stats = DetectionPoseStats {
                    input_detections: detections.len(),
                    output_detections: 0,
                    pose_method: method.as_str().to_string(),
                    tag_size: tag_size_m,
                    calibration: DetectionPoseCalibrationSummary { fx: calib.fx, fy: calib.fy, cx: calib.cx, cy: calib.cy },
                    estimated_intrinsics: false,
                    failures: DetectionPoseFailureCounts { invalid: failures_invalid, homography: failures_homography, decompose: failures_decompose },
                    first_failure,
                };
                return DetectionPoseOutput { detections: Vec::new(), stats };
            }
        }
    };

    let mut out: Vec<DetectionPose> = Vec::with_capacity(detections.len());
    for det in detections {
        match estimate_tag_pose(det.corners, det.rotation, calib, tag_size_m, method) {
            Ok(pose) => out.push(DetectionPose {
                id: det.id,
                translation: Translation3 { x: pose.translation_three.x, y: pose.translation_three.y, z: pose.translation_three.z },
                reprojection_error_px: pose.reprojection_rms_px,
                rotation: DetectionPoseRotation {
                    roll: pose.roll_deg,
                    pitch: pose.pitch_deg,
                    yaw: pose.yaw_deg,
                    quaternion: DetectionQuat { x: pose.quat_x, y: pose.quat_y, z: pose.quat_z, w: pose.quat_w },
                },
                code_rotation: det.rotation,
                bits: det.bits.clone(),
                tag_size: tag_size_m,
                pose_method: method.as_str().to_string(),
            }),
            Err(err) => {
                match err {
                    TagPoseSolveError::InvalidInput => failures_invalid += 1,
                    TagPoseSolveError::Homography => failures_homography += 1,
                    TagPoseSolveError::Decompose => {
                        failures_decompose += 1;
                        if first_failure.is_none() {
                            first_failure = Some(DetectionPoseFailureSample { tag_id: det.id, reason: "decompose".to_string(), corners: det.corners });
                        }
                    }
                }
                if first_failure.is_none() && matches!(err, TagPoseSolveError::Homography) {
                    first_failure = Some(DetectionPoseFailureSample { tag_id: det.id, reason: "homography".to_string(), corners: det.corners });
                }
            }
        }
    }

    let stats = DetectionPoseStats {
        input_detections: detections.len(),
        output_detections: out.len(),
        pose_method: method.as_str().to_string(),
        tag_size: tag_size_m,
        calibration: DetectionPoseCalibrationSummary { fx: calib.fx, fy: calib.fy, cx: calib.cx, cy: calib.cy },
        estimated_intrinsics,
        failures: DetectionPoseFailureCounts { invalid: failures_invalid, homography: failures_homography, decompose: failures_decompose },
        first_failure,
    };
    let value = DetectionPoseOutput { detections: out, stats };

    let idx = TAG_POSE_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if idx < 3 {
        warn!(
            target: "helios_engine::graph",
            detections = detections.len(),
            poses = value.stats.output_detections,
            estimated_intrinsics,
            fx = calib.fx,
            fy = calib.fy,
            cx = calib.cx,
            cy = calib.cy,
            "aruco:tag_poses computed pose sample"
        );
    }

    value
}

struct EstimatedTagPose {
    translation_three: Vector3<f64>,
    quat_x: f64,
    quat_y: f64,
    quat_z: f64,
    quat_w: f64,
    roll_deg: f64,
    pitch_deg: f64,
    yaw_deg: f64,
    reprojection_rms_px: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TagPoseSolveError {
    InvalidInput,
    Homography,
    Decompose,
}

fn estimate_tag_pose(corners_px: [Point; 4], code_rotation: u8, calib: TagPoseCalibration, tag_size_m: f64, method: TagPoseMethod) -> Result<EstimatedTagPose, TagPoseSolveError> {
    if !tag_size_m.is_finite() || tag_size_m <= 0.0 || corners_px.iter().any(|pt| !pt.x.is_finite() || !pt.y.is_finite()) {
        return Err(TagPoseSolveError::InvalidInput);
    }

    // Canonical tag corners in a tag-local XY plane (Z=0), matching the quad order:
    // top-left, top-right, bottom-right, bottom-left.
    //
    // The tag-local +X points right and +Y points up so the resulting rotation can be
    // applied directly to a Three.js plane geometry.
    //
    // We use a unit square here for numerical stability. Translation recovered from the
    // homography decomposition is therefore expressed in "tag widths"; multiply by the
    // physical tag size to convert to meters.
    let object = [(-0.5, 0.5), (0.5, 0.5), (0.5, -0.5), (-0.5, -0.5)];

    // The decode stage reports a code rotation (0..=3) that indicates how many 90-degree
    // rotations were required to align the sampled grid with the family codebook. Reorder
    // the quad corners so corner[0] always maps to the canonical tag top-left.
    let r = (code_rotation & 3) as usize;
    let shift = (4 - r) & 3;
    let ordered_px = [corners_px[shift], corners_px[(shift + 1) & 3], corners_px[(shift + 2) & 3], corners_px[(shift + 3) & 3]];

    let undistorted: [(f64, f64); 4] = ordered_px.map(|pt| calib.undistort_pixel(pt.x, pt.y).unwrap_or((pt.x, pt.y)));

    let resolved_method = method.resolve();
    let homography_method = match resolved_method {
        TagPoseMethod::HomographyV1 => TagPoseMethod::HomographyV1,
        TagPoseMethod::HomographyV2 | TagPoseMethod::PnpRefine => TagPoseMethod::HomographyV2,
        TagPoseMethod::Auto => TagPoseMethod::HomographyV2,
    };

    let homography = match homography_method {
        TagPoseMethod::HomographyV1 => homography::homography_dlt_v1(&object, &undistorted),
        TagPoseMethod::HomographyV2 | TagPoseMethod::PnpRefine => homography::homography_dlt_v2(&object, &undistorted),
        TagPoseMethod::Auto => homography::homography_dlt_v2(&object, &undistorted),
    }
    .ok_or(TagPoseSolveError::Homography)?;

    let (translation_cam, rotation_cam) = match homography_method {
        TagPoseMethod::HomographyV1 => homography::decompose_planar_homography_v1(homography, calib),
        TagPoseMethod::HomographyV2 | TagPoseMethod::PnpRefine => homography::decompose_planar_homography_v2(homography, calib),
        TagPoseMethod::Auto => homography::decompose_planar_homography_v2(homography, calib),
    }
    .ok_or(TagPoseSolveError::Decompose)?;
    let mut translation_cam = translation_cam * tag_size_m;
    let mut rotation_cam = rotation_cam;

    // Always compute a reprojection RMS when intrinsics are available. This is cheap (4 points)
    // and lets downstream systems pick the best interpretation (distorted vs already-undistorted
    // pixels) and debug calibration issues.
    let mut reproj_rms_px = None;
    if calib.is_usable() {
        let half = tag_size_m * 0.5;
        let object_m = [(-half, half), (half, half), (half, -half), (-half, -half)];
        reproj_rms_px = reprojection_rms_px(&object_m, &undistorted, calib, translation_cam, rotation_cam);

        if matches!(method, TagPoseMethod::PnpRefine)
            && let Some((t_refined, r_refined, rms)) = refine_pose_pnp_square(&object_m, &undistorted, calib, translation_cam, rotation_cam)
        {
            translation_cam = t_refined;
            rotation_cam = r_refined;
            reproj_rms_px = Some(rms);
        }
    }

    // Camera frame from homography decomposition follows the classic pinhole convention:
    // +X right, +Y down, +Z forward.
    //
    // Convert to a Three.js-friendly camera basis (+Y up, -Z forward) using a proper rotation
    // (180 degrees about +X).
    let cv_to_three = Matrix3::new(1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0);
    let translation_three = cv_to_three * translation_cam;
    let rotation_three = cv_to_three * rotation_cam;

    let rotation_three = Rotation3::from_matrix_unchecked(rotation_three);
    let quat = UnitQuaternion::from_rotation_matrix(&rotation_three);
    let (roll, pitch, yaw) = quat.euler_angles();

    Ok(EstimatedTagPose {
        translation_three,
        quat_x: quat.coords.x,
        quat_y: quat.coords.y,
        quat_z: quat.coords.z,
        quat_w: quat.coords.w,
        roll_deg: roll.to_degrees(),
        pitch_deg: pitch.to_degrees(),
        yaw_deg: yaw.to_degrees(),
        reprojection_rms_px: reproj_rms_px,
    })
}

fn reprojection_rms_px(object_xy: &[(f64, f64); 4], image_uv: &[(f64, f64); 4], calib: TagPoseCalibration, translation_cam: Vector3<f64>, rotation_cam: Matrix3<f64>) -> Option<f64> {
    if !calib.is_usable() {
        return None;
    }
    if !translation_cam.iter().all(|v| v.is_finite()) || !rotation_cam.iter().all(|v| v.is_finite()) {
        return None;
    }

    let mut sum_sq = 0.0f64;
    for i in 0..4 {
        let obj = Vector3::new(object_xy[i].0, object_xy[i].1, 0.0);
        let p = rotation_cam * obj + translation_cam;
        if !p.iter().all(|v| v.is_finite()) || p.z.abs() < 1e-12 {
            return None;
        }
        let u = calib.fx * (p.x / p.z) + calib.cx;
        let v = calib.fy * (p.y / p.z) + calib.cy;
        let du = image_uv[i].0 - u;
        let dv = image_uv[i].1 - v;
        if !du.is_finite() || !dv.is_finite() {
            return None;
        }
        sum_sq += du * du + dv * dv;
    }

    let mean = sum_sq / 8.0;
    Some(mean.max(0.0).sqrt())
}

fn refine_pose_pnp_square(
    object_xy: &[(f64, f64); 4],
    image_uv: &[(f64, f64); 4],
    calib: TagPoseCalibration,
    translation_cam: Vector3<f64>,
    rotation_cam: Matrix3<f64>,
) -> Option<(Vector3<f64>, Matrix3<f64>, f64)> {
    if !calib.is_usable() {
        return None;
    }
    if !translation_cam.iter().all(|v| v.is_finite()) || !rotation_cam.iter().all(|v| v.is_finite()) {
        return None;
    }

    let initial_rms = reprojection_rms_px(object_xy, image_uv, calib, translation_cam, rotation_cam)?;
    let mut best_rms = initial_rms;
    let mut best_t = translation_cam;
    let mut best_r = rotation_cam;

    let mut t = translation_cam;
    let mut r = rotation_cam;
    let mut lambda = 1e-3f64;

    for _ in 0..6 {
        let mut a = SMatrix::<f64, 6, 6>::zeros();
        let mut b = SVector::<f64, 6>::zeros();

        for i in 0..4 {
            let obj = Vector3::new(object_xy[i].0, object_xy[i].1, 0.0);
            let p = r * obj + t;
            let x = p.x;
            let y = p.y;
            let z = p.z;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() || z <= 1e-6 {
                return None;
            }

            let inv_z = 1.0 / z;
            let inv_z2 = inv_z * inv_z;
            let u_pred = calib.fx * x * inv_z + calib.cx;
            let v_pred = calib.fy * y * inv_z + calib.cy;

            let ru = image_uv[i].0 - u_pred;
            let rv = image_uv[i].1 - v_pred;
            if !ru.is_finite() || !rv.is_finite() {
                return None;
            }

            let du_dx = calib.fx * inv_z;
            let du_dy = 0.0;
            let du_dz = -calib.fx * x * inv_z2;
            let dv_dx = 0.0;
            let dv_dy = calib.fy * inv_z;
            let dv_dz = -calib.fy * y * inv_z2;

            let dp_dwx = Vector3::new(0.0, z, -y);
            let dp_dwy = Vector3::new(-z, 0.0, x);
            let dp_dwz = Vector3::new(y, -x, 0.0);

            let ju = [
                du_dx * dp_dwx.x + du_dy * dp_dwx.y + du_dz * dp_dwx.z,
                du_dx * dp_dwy.x + du_dy * dp_dwy.y + du_dz * dp_dwy.z,
                du_dx * dp_dwz.x + du_dy * dp_dwz.y + du_dz * dp_dwz.z,
                du_dx,
                du_dy,
                du_dz,
            ];
            let jv = [
                dv_dx * dp_dwx.x + dv_dy * dp_dwx.y + dv_dz * dp_dwx.z,
                dv_dx * dp_dwy.x + dv_dy * dp_dwy.y + dv_dz * dp_dwy.z,
                dv_dx * dp_dwz.x + dv_dy * dp_dwz.y + dv_dz * dp_dwz.z,
                dv_dx,
                dv_dy,
                dv_dz,
            ];

            for k in 0..6 {
                b[k] += ju[k] * ru + jv[k] * rv;
                for l in 0..6 {
                    a[(k, l)] += ju[k] * ju[l] + jv[k] * jv[l];
                }
            }
        }

        let mut accepted = false;
        for _ in 0..4 {
            let mut a_damped = a;
            for k in 0..6 {
                a_damped[(k, k)] += lambda;
            }

            let delta = a_damped.lu().solve(&b)?;
            if !delta.iter().all(|v| v.is_finite()) {
                return None;
            }

            let mut omega = Vector3::new(delta[0], delta[1], delta[2]);
            let mut v = Vector3::new(delta[3], delta[4], delta[5]);

            let omega_norm = omega.norm();
            if omega_norm > 0.25 {
                omega *= 0.25 / omega_norm;
            }
            let v_norm = v.norm();
            if v_norm > 0.25 {
                v *= 0.25 / v_norm;
            }

            if omega.norm() < 1e-12 && v.norm() < 1e-12 {
                accepted = true;
                break;
            }

            let delta_r = Rotation3::from_scaled_axis(omega);
            let r_candidate = delta_r.matrix() * r;
            let t_candidate = delta_r.matrix() * t + v;
            if !t_candidate.iter().all(|v| v.is_finite()) || t_candidate.z <= 1e-6 {
                lambda = (lambda * 10.0).min(1e6);
                continue;
            }

            let Some(rms) = reprojection_rms_px(object_xy, image_uv, calib, t_candidate, r_candidate) else {
                lambda = (lambda * 10.0).min(1e6);
                continue;
            };

            if rms < best_rms {
                best_rms = rms;
                best_t = t_candidate;
                best_r = r_candidate;
                t = t_candidate;
                r = r_candidate;
                lambda = (lambda * 0.5).max(1e-6);
                accepted = true;
                break;
            }

            lambda = (lambda * 10.0).min(1e6);
        }

        if !accepted {
            break;
        }
    }

    if best_rms.partial_cmp(&initial_rms) != Some(std::cmp::Ordering::Less) {
        return None;
    }

    // Ensure a proper rotation matrix.
    let svd = best_r.svd(true, true);
    let u = svd.u?;
    let v_t = svd.v_t?;
    let mut r = u * v_t;
    if r.determinant() < 0.0 {
        r.column_mut(2).neg_mut();
    }
    if !best_t.iter().all(|v| v.is_finite()) || best_t.z <= 1e-6 {
        return None;
    }

    let rms = reprojection_rms_px(object_xy, image_uv, calib, best_t, r)?;

    // Guard against planar ambiguity branch flips under heavy skew/noise: if refinement only
    // improves reprojection trivially but requires a large orientation/translation jump from the
    // homography seed, keep the seed pose.
    let initial_rot = Rotation3::from_matrix_unchecked(rotation_cam);
    let refined_rot = Rotation3::from_matrix_unchecked(r);
    let initial_q = UnitQuaternion::from_rotation_matrix(&initial_rot);
    let refined_q = UnitQuaternion::from_rotation_matrix(&refined_rot);
    let rot_delta_deg = initial_q.angle_to(&refined_q).to_degrees().abs();
    let trans_delta = (best_t - translation_cam).norm();
    let rms_gain = (initial_rms - rms).max(0.0);
    let depth_scale = translation_cam.z.abs().max(0.2);
    let trans_delta_ratio = trans_delta / depth_scale;

    if (rot_delta_deg > 35.0 && rms_gain < 0.25) || (trans_delta_ratio > 0.28 && rms_gain < 0.35) {
        return None;
    }
    Some((best_t, r, rms))
}

fn estimate_pinhole_intrinsics_from_detections(detections: &[ArucoDetection2D], cx: f64, cy: f64, tag_size_m: f64) -> Option<(f64, f64)> {
    if detections.is_empty() {
        return None;
    }
    if !cx.is_finite() || !cy.is_finite() {
        return None;
    }
    if !tag_size_m.is_finite() || tag_size_m <= 0.0 {
        return None;
    }

    let half = tag_size_m * 0.5;
    let object = [(-half, -half), (half, -half), (half, half), (-half, half)];

    let (mut fx_samples, mut fy_samples, mut f_samples) = POSE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        (std::mem::take(&mut scratch.fx_samples), std::mem::take(&mut scratch.fy_samples), std::mem::take(&mut scratch.f_samples))
    });
    fx_samples.clear();
    fy_samples.clear();
    f_samples.clear();

    for det in detections {
        let image_uv = det.corners.map(|pt| (pt.x, pt.y));
        let Some(h) = homography::homography_dlt_v2(&object, &image_uv) else { continue };

        let h11 = h[(0, 0)];
        let h21 = h[(1, 0)];
        let h31 = h[(2, 0)];
        let h12 = h[(0, 1)];
        let h22 = h[(1, 1)];
        let h32 = h[(2, 1)];

        let x1 = h11 - cx * h31;
        let y1 = h21 - cy * h31;
        let x2 = h12 - cx * h32;
        let y2 = h22 - cy * h32;

        // Solve for fx/fy assuming zero skew and known principal point:
        //   (x1*x2)/fx^2 + (y1*y2)/fy^2 + h31*h32 = 0
        //   (x1^2 - x2^2)/fx^2 + (y1^2 - y2^2)/fy^2 + (h31^2 - h32^2) = 0
        let a1 = x1 * x2;
        let b1 = y1 * y2;
        let c1 = h31 * h32;
        let a2 = x1 * x1 - x2 * x2;
        let b2 = y1 * y1 - y2 * y2;
        let c2 = h31 * h31 - h32 * h32;

        let det_ab = a1 * b2 - a2 * b1;
        if det_ab.abs() > 1e-12 {
            let inv_fx2 = (-c1 * b2 + c2 * b1) / det_ab;
            let inv_fy2 = (-a1 * c2 + a2 * c1) / det_ab;
            if inv_fx2.is_finite() && inv_fy2.is_finite() && inv_fx2 > 0.0 && inv_fy2 > 0.0 {
                let fx = (1.0 / inv_fx2).sqrt();
                let fy = (1.0 / inv_fy2).sqrt();
                if fx.is_finite() && fy.is_finite() && (50.0..=20_000.0).contains(&fx) && (50.0..=20_000.0).contains(&fy) {
                    fx_samples.push(fx);
                    fy_samples.push(fy);
                }
            }
        }

        // Fallback: assume fx == fy and solve from the orthogonality constraint alone.
        let a = a1 + b1;
        if a.abs() > 1e-12 && c1.is_finite() && c1 != 0.0 {
            let inv_f2 = -c1 / a;
            if inv_f2.is_finite() && inv_f2 > 0.0 {
                let f = (1.0 / inv_f2).sqrt();
                if f.is_finite() && (50.0..=20_000.0).contains(&f) {
                    f_samples.push(f);
                }
            }
        }
    }

    fn median_in_place(v: &mut [f64]) -> Option<f64> {
        if v.is_empty() {
            return None;
        }
        v.sort_by(|a, b| a.total_cmp(b));
        Some(v[v.len() / 2])
    }

    let mut result = None;
    if fx_samples.len() >= 4
        && fy_samples.len() >= 4
        && let (Some(fx), Some(fy)) = (median_in_place(&mut fx_samples), median_in_place(&mut fy_samples))
    {
        result = Some((fx, fy));
    }

    if result.is_none()
        && f_samples.len() >= 4
        && let Some(f) = median_in_place(&mut f_samples)
    {
        result = Some((f, f));
    }

    if result.is_none() {
        match (median_in_place(&mut fx_samples), median_in_place(&mut fy_samples), median_in_place(&mut f_samples)) {
            (Some(fx), Some(fy), _) => result = Some((fx, fy)),
            (_, _, Some(f)) => result = Some((f, f)),
            _ => {}
        }
    }

    POSE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.fx_samples = fx_samples;
        scratch.fy_samples = fy_samples;
        scratch.f_samples = f_samples;
    });

    result
}
