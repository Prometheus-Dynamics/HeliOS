//! Utilities for estimating device pose from a set of known world-space markers.
//!
//! The solvers accept a JSON marker map describing known marker positions and a
//! list of observations captured by the vision stack. Consumers can choose a
//! pose estimation strategy ranging from a simple centroid estimator to a full
//! rigid-body alignment or a Perspective-n-Point solver that operates directly
//! on pixel observations.

use crate::{DevicePose, Rotation3, Translation3};
use nalgebra::{DMatrix, DVector, Matrix3, SMatrix, SVector, UnitQuaternion, Vector3};
use rand::seq::index::sample;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, path::Path};

/// Marker definition expressed in world coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarkerDefinition {
    pub id: u32,
    pub translation: Translation3,
    #[serde(default)]
    pub rotation: Option<Rotation3>,
}

impl MarkerDefinition {
    pub fn new(id: u32, translation: Translation3, rotation: Option<Rotation3>) -> Self {
        Self { id, translation, rotation }
    }
}

/// Marker map persisted to/from disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MarkerMap {
    pub markers: Vec<MarkerDefinition>,
}

impl MarkerMap {
    /// Parse a [`MarkerMap`] from a JSON string.
    pub fn from_json_str(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }

    /// Serialize the marker map to a pretty JSON string.
    pub fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Load a marker map from a JSON file.
    pub fn from_json_file(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(std::io::Error::other)
    }

    /// Persist the marker map into a JSON file.
    pub fn to_json_file(&self, path: &Path) -> std::io::Result<()> {
        let data = self.to_json_string().map_err(std::io::Error::other)?;
        std::fs::write(path, data)
    }

    /// Quickly look up markers by their id.
    fn as_lookup(&self) -> HashMap<u32, &MarkerDefinition> {
        self.markers.iter().map(|marker| (marker.id, marker)).collect()
    }

    /// Estimate the camera/device pose based on a set of marker observations.
    pub fn estimate_pose(&self, observations: &[MarkerObservation], method: PoseEstimationMethod) -> Result<DevicePose, PoseEstimationError> {
        if observations.is_empty() {
            return Err(PoseEstimationError::InsufficientMarkers { required: method.minimum_required(), provided: 0 });
        }
        let lookup = self.as_lookup();
        let mut valid_pairs = Vec::with_capacity(observations.len());
        for obs in observations {
            if obs.weight <= 0.0 {
                continue;
            }
            if let Some(marker) = lookup.get(&obs.id) {
                valid_pairs.push((*marker, obs));
            } else {
                return Err(PoseEstimationError::UnknownMarkerId(obs.id));
            }
        }
        if valid_pairs.len() < method.minimum_required() {
            return Err(PoseEstimationError::InsufficientMarkers { required: method.minimum_required(), provided: valid_pairs.len() });
        }
        match method {
            PoseEstimationMethod::Centroid => centroid_estimate(&valid_pairs),
            PoseEstimationMethod::RigidProcrustes => procrustes_estimate(&valid_pairs),
            PoseEstimationMethod::PerspectiveNPoint(intrinsics) => pnp_estimate(&valid_pairs, &intrinsics),
            PoseEstimationMethod::PerspectiveNPointRefine { intrinsics, refine } => pnp_refine_estimate(&valid_pairs, &intrinsics, &refine),
            PoseEstimationMethod::PerspectiveNPointRansac { intrinsics, config } => pnp_ransac_estimate(&valid_pairs, &intrinsics, &config),
            PoseEstimationMethod::PerspectiveNPointRansacRefine { intrinsics, config, refine } => pnp_ransac_refine_estimate(&valid_pairs, &intrinsics, &config, &refine),
        }
    }
}

/// Intrinsic parameters used by the camera projection model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraIntrinsics {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
}

impl CameraIntrinsics {
    pub fn new(fx: f64, fy: f64, cx: f64, cy: f64) -> Self {
        Self { fx, fy, cx, cy }
    }

    fn normalize(&self, u: f64, v: f64) -> (f64, f64) {
        (((u - self.cx) / self.fx), ((v - self.cy) / self.fy))
    }
}

/// Configuration knobs for the RANSAC PnP solver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RansacConfig {
    pub max_iterations: usize,
    pub reprojection_threshold: f64,
    pub min_inliers: usize,
}

impl RansacConfig {
    pub fn new(max_iterations: usize, reprojection_threshold: f64, min_inliers: usize) -> Self {
        Self { max_iterations, reprojection_threshold, min_inliers }
    }
}

impl Default for RansacConfig {
    fn default() -> Self {
        Self { max_iterations: 64, reprojection_threshold: 2.0, min_inliers: 6 }
    }
}

/// Configuration knobs for the iterative refinement stage applied to a PnP estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PnpRefineConfig {
    pub max_iterations: usize,
    /// Levenberg–Marquardt style diagonal damping (larger values are more stable but slower).
    pub damping: f64,
    /// Finite difference step size used when differentiating translation (meters).
    pub jacobian_eps_translation: f64,
    /// Finite difference step size used when differentiating rotation (radians).
    pub jacobian_eps_rotation: f64,
    /// Maximum translation update per iteration (meters).
    pub max_step_translation: f64,
    /// Maximum rotation update per iteration (radians).
    pub max_step_rotation: f64,
}

impl Default for PnpRefineConfig {
    fn default() -> Self {
        Self { max_iterations: 10, damping: 1e-6, jacobian_eps_translation: 1e-4, jacobian_eps_rotation: 1e-5, max_step_translation: 0.5, max_step_rotation: 0.25 }
    }
}

/// Pixel-space coordinates associated with a marker detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelCoordinate {
    pub u: f64,
    pub v: f64,
}

impl PixelCoordinate {
    pub fn new(u: f64, v: f64) -> Self {
        Self { u, v }
    }
}

/// Observation describing either Euclidean or pixel-space marker data.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerObservation {
    pub id: u32,
    pub translation: Option<Translation3>,
    pub rotation: Option<Rotation3>,
    pub pixel: Option<PixelCoordinate>,
    pub weight: f32,
}

impl MarkerObservation {
    pub fn new(id: u32) -> Self {
        Self { id, translation: None, rotation: None, pixel: None, weight: 1.0 }
    }

    pub fn translation(id: u32, translation: Translation3) -> Self {
        Self::new(id).with_translation(translation)
    }

    pub fn pixel(id: u32, pixel: PixelCoordinate) -> Self {
        Self::new(id).with_pixel(pixel)
    }

    pub fn with_translation(mut self, translation: Translation3) -> Self {
        self.translation = Some(translation);
        self
    }

    pub fn with_rotation(mut self, rotation: Rotation3) -> Self {
        self.rotation = Some(rotation);
        self
    }

    pub fn with_pixel(mut self, pixel: PixelCoordinate) -> Self {
        self.pixel = Some(pixel);
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }
}

/// Available pose estimation strategies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PoseEstimationMethod {
    /// Averages the world minus observation translations, optionally blending any rotation deltas.
    Centroid,
    /// Solves the full rigid-body transformation using the Kabsch algorithm (a.k.a. absolute orientation).
    RigidProcrustes,
    /// Estimates pose from pixel detections using a Perspective-n-Point formulation.
    PerspectiveNPoint(CameraIntrinsics),
    /// Perspective-n-Point with iterative refinement (Gauss-Newton + damping) for lower reprojection error.
    PerspectiveNPointRefine { intrinsics: CameraIntrinsics, refine: PnpRefineConfig },
    /// Robust Perspective-n-Point that rejects outliers via RANSAC before refinement.
    PerspectiveNPointRansac { intrinsics: CameraIntrinsics, config: RansacConfig },
    /// Robust Perspective-n-Point via RANSAC followed by iterative refinement on inliers.
    PerspectiveNPointRansacRefine { intrinsics: CameraIntrinsics, config: RansacConfig, refine: PnpRefineConfig },
}

impl PoseEstimationMethod {
    fn minimum_required(self) -> usize {
        match self {
            PoseEstimationMethod::Centroid => 1,
            PoseEstimationMethod::RigidProcrustes => 3,
            PoseEstimationMethod::PerspectiveNPoint(_) => 4,
            PoseEstimationMethod::PerspectiveNPointRansac { config, .. } => config.min_inliers.max(4),
            PoseEstimationMethod::PerspectiveNPointRefine { .. } => 4,
            PoseEstimationMethod::PerspectiveNPointRansacRefine { config, .. } => config.min_inliers.max(4),
        }
    }
}

/// Errors surfaced while estimating the device pose.
#[derive(Debug, Clone, PartialEq)]
pub enum PoseEstimationError {
    UnknownMarkerId(u32),
    InsufficientMarkers { required: usize, provided: usize },
    MissingObservationData { id: u32, field: &'static str },
    DegenerateConfiguration,
    RansacFailed,
}

impl fmt::Display for PoseEstimationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PoseEstimationError::UnknownMarkerId(id) => write!(f, "observation references unknown marker id {id}"),
            PoseEstimationError::InsufficientMarkers { required, provided } => {
                write!(f, "insufficient marker observations: required {required}, provided {provided}")
            }
            PoseEstimationError::MissingObservationData { id, field } => {
                write!(f, "observation {id} missing required {field} data")
            }
            PoseEstimationError::DegenerateConfiguration => write!(f, "marker arrangement is degenerate for pose estimation"),
            PoseEstimationError::RansacFailed => write!(f, "RANSAC failed to locate a consensus pose"),
        }
    }
}

impl std::error::Error for PoseEstimationError {}

type MarkerPair<'a> = (&'a MarkerDefinition, &'a MarkerObservation);

fn centroid_estimate(pairs: &[MarkerPair<'_>]) -> Result<DevicePose, PoseEstimationError> {
    let mut weight_sum = 0.0f64;
    let mut translation_acc = Vector3::zeros();
    let mut rotation_acc = Rotation3::default();
    let mut rotation_weight = 0.0f64;

    for (marker, obs) in pairs {
        let weight = obs.weight as f64;
        if weight <= 0.0 {
            continue;
        }
        let Some(obs_translation) = obs.translation else {
            return Err(PoseEstimationError::MissingObservationData { id: obs.id, field: "translation" });
        };
        let marker_vec = translation_to_vec(&marker.translation);
        let obs_vec = translation_to_vec(&obs_translation);
        translation_acc += (marker_vec - obs_vec) * weight;
        weight_sum += weight;

        if let (Some(map_rot), Some(obs_rot)) = (marker.rotation, obs.rotation) {
            rotation_acc += (map_rot - obs_rot) * weight;
            rotation_weight += weight;
        }
    }

    if weight_sum == 0.0 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 1, provided: 0 });
    }

    let translation = translation_acc / weight_sum;
    let rotation = if rotation_weight > 0.0 { rotation_acc / rotation_weight } else { Rotation3::default() };
    Ok(DevicePose { translation: vec_to_translation(translation), rotation })
}

fn procrustes_estimate(pairs: &[MarkerPair<'_>]) -> Result<DevicePose, PoseEstimationError> {
    let mut weight_sum = 0.0f64;
    let mut centroid_obs = Vector3::zeros();
    let mut centroid_world = Vector3::zeros();

    for (marker, obs) in pairs {
        let weight = obs.weight as f64;
        if weight <= 0.0 {
            continue;
        }
        let Some(obs_translation) = obs.translation else {
            return Err(PoseEstimationError::MissingObservationData { id: obs.id, field: "translation" });
        };
        let marker_vec = translation_to_vec(&marker.translation);
        let obs_vec = translation_to_vec(&obs_translation);
        centroid_world += marker_vec * weight;
        centroid_obs += obs_vec * weight;
        weight_sum += weight;
    }

    if weight_sum == 0.0 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 3, provided: 0 });
    }

    centroid_world /= weight_sum;
    centroid_obs /= weight_sum;

    let mut covariance = Matrix3::zeros();
    for (marker, obs) in pairs {
        let weight = obs.weight as f64;
        if weight <= 0.0 {
            continue;
        }
        let Some(obs_translation) = obs.translation else {
            return Err(PoseEstimationError::MissingObservationData { id: obs.id, field: "translation" });
        };
        let marker_vec = translation_to_vec(&marker.translation) - centroid_world;
        let obs_vec = translation_to_vec(&obs_translation) - centroid_obs;
        covariance += weight * obs_vec * marker_vec.transpose();
    }

    let svd = covariance.svd(true, true);
    let Some(u) = svd.u else {
        return Err(PoseEstimationError::DegenerateConfiguration);
    };
    let Some(v_t) = svd.v_t else {
        return Err(PoseEstimationError::DegenerateConfiguration);
    };

    let v = v_t.transpose();
    let u_t = u.transpose();
    let mut rotation_matrix = v * u_t;
    if rotation_matrix.determinant() < 0.0 {
        let mut fix = Matrix3::identity();
        fix[(2, 2)] = -1.0;
        rotation_matrix = v * fix * u_t;
    }

    let translation = centroid_world - rotation_matrix * centroid_obs;
    let rotation = matrix_to_euler(rotation_matrix)?;

    Ok(DevicePose { translation: vec_to_translation(translation), rotation })
}

fn pnp_estimate(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics) -> Result<DevicePose, PoseEstimationError> {
    let indices: Vec<usize> = pairs.iter().enumerate().filter(|(_, (_, obs))| obs.pixel.is_some() && obs.weight > 0.0).map(|(idx, _)| idx).collect();
    if indices.len() < 4 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 4, provided: indices.len() });
    }
    pnp_estimate_with_indices(pairs, intrinsics, &indices)
}

fn pnp_refine_estimate(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics, refine: &PnpRefineConfig) -> Result<DevicePose, PoseEstimationError> {
    let indices: Vec<usize> = pairs.iter().enumerate().filter(|(_, (_, obs))| obs.pixel.is_some() && obs.weight > 0.0).map(|(idx, _)| idx).collect();
    if indices.len() < 4 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 4, provided: indices.len() });
    }
    let initial = pnp_estimate_with_indices(pairs, intrinsics, &indices)?;
    Ok(pnp_refine_with_indices(pairs, intrinsics, &indices, initial, refine))
}

fn pnp_estimate_with_indices(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics, indices: &[usize]) -> Result<DevicePose, PoseEstimationError> {
    if indices.len() < 4 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 4, provided: indices.len() });
    }

    #[derive(Clone, Copy)]
    struct PnpPoint {
        world: Vector3<f64>,
        x: f64,
        y: f64,
        weight: f64,
    }

    let mut pts: Vec<PnpPoint> = Vec::with_capacity(indices.len());
    for &idx in indices {
        let (marker, obs) = pairs[idx];
        let Some(pixel) = obs.pixel else {
            return Err(PoseEstimationError::MissingObservationData { id: obs.id, field: "pixel" });
        };
        let weight = obs.weight.max(0.0) as f64;
        if weight == 0.0 {
            continue;
        }
        let (x, y) = intrinsics.normalize(pixel.u, pixel.v);
        pts.push(PnpPoint { world: translation_to_vec(&marker.translation), x, y, weight });
    }

    if pts.len() < 4 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 4, provided: pts.len() });
    }

    let solve_t_for_rotation_wc = |rotation_wc: &Matrix3<f64>| -> Option<Vector3<f64>> {
        let mut a = DMatrix::<f64>::zeros(pts.len() * 2, 3);
        let mut b = DVector::<f64>::zeros(pts.len() * 2);
        for (i, pt) in pts.iter().enumerate() {
            let p = rotation_wc * pt.world;
            let scale = pt.weight.sqrt();
            let row = i * 2;
            a[(row, 0)] = scale;
            a[(row, 2)] = -pt.x * scale;
            b[row] = (pt.x * p.z - p.x) * scale;
            a[(row + 1, 1)] = scale;
            a[(row + 1, 2)] = -pt.y * scale;
            b[row + 1] = (pt.y * p.z - p.y) * scale;
        }
        let svd = a.svd(true, true);
        let t = svd.solve(&b, 1e-12).ok()?;
        if t.nrows() != 3 {
            return None;
        }
        Some(Vector3::new(t[0], t[1], t[2]))
    };

    let eval_error = |pose: &DevicePose| -> f64 {
        let mut sum = 0.0;
        let mut count = 0usize;
        for &idx in indices {
            let (marker, obs) = pairs[idx];
            let Some(pixel) = obs.pixel else {
                continue;
            };
            if let Some(err) = reprojection_error(&marker.translation, pixel, pose, intrinsics) {
                sum += err;
                count += 1;
            } else {
                sum += 1e9;
                count += 1;
            }
        }
        if count == 0 { 1e9 } else { sum / (count as f64) }
    };

    let yaw_candidates: &[f64] = if pts.len() <= 4 {
        &[0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, -std::f64::consts::FRAC_PI_2]
    } else {
        &[-std::f64::consts::PI, -std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_4, 0.0, std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
    };
    let (roll_candidates, pitch_candidates): (&[f64], &[f64]) = if pts.len() <= 4 {
        // RANSAC inner-loop: keep this small but not "too small" or we never converge on tilted cameras.
        (&[-0.2, 0.0, 0.2], &[-0.2, 0.0, 0.2])
    } else {
        (&[-0.15, 0.0, 0.15], &[-0.15, 0.0, 0.15])
    };

    let refine_cfg = PnpRefineConfig { max_iterations: 12, ..PnpRefineConfig::default() };
    let mut best: Option<(DevicePose, f64)> = None;

    for &roll in roll_candidates {
        for &pitch in pitch_candidates {
            for &yaw in yaw_candidates {
                let rot_cw = euler_to_matrix(&Rotation3 { roll, pitch, yaw });
                let rot_wc = rot_cw.transpose();
                let Some(t) = solve_t_for_rotation_wc(&rot_wc) else {
                    continue;
                };
                let cam_pos = -(rot_wc.transpose() * t);
                let Ok(rot) = matrix_to_euler(rot_cw) else {
                    continue;
                };
                let initial = DevicePose { translation: vec_to_translation(cam_pos), rotation: rot };
                let refined = pnp_refine_with_indices(pairs, intrinsics, indices, initial, &refine_cfg);
                let err = eval_error(&refined);
                let update = match best {
                    None => true,
                    Some((_, best_err)) => err < best_err,
                };
                if update {
                    best = Some((refined, err));
                }
            }
        }
    }

    best.map(|(pose, _)| pose).ok_or(PoseEstimationError::DegenerateConfiguration)
}

fn pnp_ransac_estimate(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics, config: &RansacConfig) -> Result<DevicePose, PoseEstimationError> {
    let inliers = pnp_ransac_inliers(pairs, intrinsics, config)?;
    pnp_estimate_with_indices(pairs, intrinsics, &inliers)
}

fn pnp_ransac_refine_estimate(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics, config: &RansacConfig, refine: &PnpRefineConfig) -> Result<DevicePose, PoseEstimationError> {
    let inliers = pnp_ransac_inliers(pairs, intrinsics, config)?;
    let initial = pnp_estimate_with_indices(pairs, intrinsics, &inliers)?;
    Ok(pnp_refine_with_indices(pairs, intrinsics, &inliers, initial, refine))
}

fn pnp_ransac_inliers(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics, config: &RansacConfig) -> Result<Vec<usize>, PoseEstimationError> {
    let pixel_indices: Vec<usize> = pairs.iter().enumerate().filter(|(_, (_, obs))| obs.pixel.is_some() && obs.weight > 0.0).map(|(idx, _)| idx).collect();

    if pixel_indices.len() < 4 {
        return Err(PoseEstimationError::InsufficientMarkers { required: 4, provided: pixel_indices.len() });
    }

    let max_iterations = config.max_iterations.max(1);
    let mut rng = rand::rng();
    let mut best_inliers: Vec<usize> = Vec::new();
    let mut best_error = f64::INFINITY;

    for _ in 0..max_iterations {
        let sampled = sample(&mut rng, pixel_indices.len(), 4);
        let sample_indices: Vec<usize> = sampled.iter().map(|i| pixel_indices[i]).collect();

        let Ok(candidate_pose) = pnp_estimate_with_indices(pairs, intrinsics, &sample_indices) else {
            continue;
        };

        let mut inliers = Vec::new();
        let mut error_sum = 0.0;
        for &idx in &pixel_indices {
            let (marker, obs) = pairs[idx];
            let pixel = obs.pixel.expect("pixel indices guarantee presence");
            if let Some(err) = reprojection_error(&marker.translation, pixel, &candidate_pose, intrinsics)
                && err <= config.reprojection_threshold
            {
                inliers.push(idx);
                error_sum += err;
            }
        }

        if inliers.len() >= config.min_inliers {
            let should_update = best_inliers.is_empty() || inliers.len() > best_inliers.len() || (inliers.len() == best_inliers.len() && error_sum < best_error);
            if should_update {
                best_error = error_sum;
                best_inliers = inliers;
                if best_inliers.len() == pixel_indices.len() {
                    break;
                }
            }
        }
    }

    if best_inliers.len() < config.min_inliers {
        return Err(PoseEstimationError::RansacFailed);
    }

    Ok(best_inliers)
}

fn pnp_refine_with_indices(pairs: &[MarkerPair<'_>], intrinsics: &CameraIntrinsics, indices: &[usize], initial: DevicePose, refine: &PnpRefineConfig) -> DevicePose {
    let mut camera_position = translation_to_vec(&initial.translation);
    let mut rotation_wc = euler_to_matrix(&initial.rotation).transpose();

    let usable = indices.iter().filter(|&&idx| pairs[idx].1.pixel.is_some() && pairs[idx].1.weight > 0.0).count();
    if usable < 4 {
        return initial;
    }

    let max_iterations = refine.max_iterations.clamp(1, 64);
    let damping = refine.damping.max(0.0);
    let eps_t = refine.jacobian_eps_translation.abs().max(1e-9);
    let eps_r = refine.jacobian_eps_rotation.abs().max(1e-12);
    let max_step_t = refine.max_step_translation.abs();
    let max_step_r = refine.max_step_rotation.abs();

    for _ in 0..max_iterations {
        let mut jtj = SMatrix::<f64, 6, 6>::zeros();
        let mut jtr = SVector::<f64, 6>::zeros();
        let mut count = 0usize;

        for &idx in indices {
            let (marker, obs) = pairs[idx];
            let Some(pixel) = obs.pixel else {
                continue;
            };
            let weight = (obs.weight.max(0.0) as f64).sqrt();
            if weight == 0.0 {
                continue;
            }

            let Some([r_u, r_v]) = reprojection_residual_wc(marker.translation, pixel, &camera_position, &rotation_wc, intrinsics) else {
                continue;
            };
            let r0 = [r_u * weight, r_v * weight];

            let mut jac = [[0.0f64; 6]; 2];

            for axis in 0..3 {
                let mut pos_pert = camera_position;
                pos_pert[axis] += eps_t;
                let Some([p_u, p_v]) = reprojection_residual_wc(marker.translation, pixel, &pos_pert, &rotation_wc, intrinsics) else {
                    continue;
                };
                let rp = [p_u * weight, p_v * weight];
                jac[0][axis] = (rp[0] - r0[0]) / eps_t;
                jac[1][axis] = (rp[1] - r0[1]) / eps_t;
            }

            for axis in 0..3 {
                let mut delta_axis = Vector3::zeros();
                delta_axis[axis] = eps_r;
                let delta = UnitQuaternion::from_scaled_axis(delta_axis);
                let rot_pert = delta.to_rotation_matrix().matrix() * rotation_wc;
                let Some([p_u, p_v]) = reprojection_residual_wc(marker.translation, pixel, &camera_position, &rot_pert, intrinsics) else {
                    continue;
                };
                let rp = [p_u * weight, p_v * weight];
                jac[0][3 + axis] = (rp[0] - r0[0]) / eps_r;
                jac[1][3 + axis] = (rp[1] - r0[1]) / eps_r;
            }

            for a in 0..6 {
                jtr[a] += jac[0][a] * r0[0] + jac[1][a] * r0[1];
                for b in a..6 {
                    let v = jac[0][a] * jac[0][b] + jac[1][a] * jac[1][b];
                    jtj[(a, b)] += v;
                    if a != b {
                        jtj[(b, a)] += v;
                    }
                }
            }

            count += 1;
        }

        if count < 4 {
            break;
        }

        for i in 0..6 {
            jtj[(i, i)] += damping;
        }

        let Some(delta) = jtj.lu().solve(&(-jtr)) else {
            break;
        };

        let mut delta_t = Vector3::new(delta[0], delta[1], delta[2]);
        let mut delta_r = Vector3::new(delta[3], delta[4], delta[5]);

        let delta_t_norm = delta_t.norm();
        if max_step_t > 0.0 && delta_t_norm > max_step_t {
            delta_t *= max_step_t / delta_t_norm;
        }
        let delta_r_norm = delta_r.norm();
        if max_step_r > 0.0 && delta_r_norm > max_step_r {
            delta_r *= max_step_r / delta_r_norm;
        }

        if delta_t.norm() < 1e-9 && delta_r.norm() < 1e-9 {
            break;
        }

        camera_position += delta_t;
        let delta_q = UnitQuaternion::from_scaled_axis(delta_r);
        rotation_wc = delta_q.to_rotation_matrix().matrix() * rotation_wc;
    }

    let rotation_cw = rotation_wc.transpose();
    let rotation = matrix_to_euler(rotation_cw).unwrap_or(initial.rotation);
    DevicePose { translation: vec_to_translation(camera_position), rotation }
}

fn translation_to_vec(t: &Translation3) -> Vector3<f64> {
    Vector3::new(t.x, t.y, t.z)
}

fn vec_to_translation(v: Vector3<f64>) -> Translation3 {
    Translation3 { x: v.x, y: v.y, z: v.z }
}

fn matrix_to_euler(r: Matrix3<f64>) -> Result<Rotation3, PoseEstimationError> {
    let sy = (r[(0, 0)].powi(2) + r[(1, 0)].powi(2)).sqrt();
    if !sy.is_finite() {
        return Err(PoseEstimationError::DegenerateConfiguration);
    }
    let singular = sy < 1e-6;

    let (roll, pitch, yaw) =
        if !singular { (f64::atan2(r[(2, 1)], r[(2, 2)]), f64::atan2(-r[(2, 0)], sy), f64::atan2(r[(1, 0)], r[(0, 0)])) } else { (f64::atan2(-r[(1, 2)], r[(1, 1)]), f64::atan2(-r[(2, 0)], sy), 0.0) };

    if roll.is_nan() || pitch.is_nan() || yaw.is_nan() {
        return Err(PoseEstimationError::DegenerateConfiguration);
    }

    Ok(Rotation3 { roll, pitch, yaw })
}

fn euler_to_matrix(r: &Rotation3) -> Matrix3<f64> {
    let (sr, cr) = r.roll.sin_cos();
    let (sp, cp) = r.pitch.sin_cos();
    let (sy, cy) = r.yaw.sin_cos();

    Matrix3::new(cy * cp, cy * sp * sr - sy * cr, cy * sp * cr + sy * sr, sy * cp, sy * sp * sr + cy * cr, sy * sp * cr - cy * sr, -sp, cp * sr, cp * cr)
}

fn project_world_point(world: &Translation3, pose: &DevicePose, intrinsics: &CameraIntrinsics) -> Option<PixelCoordinate> {
    let rotation_cw = euler_to_matrix(&pose.rotation);
    let rotation_wc = rotation_cw.transpose();
    let camera_position = translation_to_vec(&pose.translation);
    let world_vec = translation_to_vec(world);
    let relative = world_vec - camera_position;
    let camera_coords = rotation_wc * relative;
    if camera_coords.z <= 0.0 || !camera_coords.z.is_finite() {
        return None;
    }
    let u = intrinsics.fx * (camera_coords.x / camera_coords.z) + intrinsics.cx;
    let v = intrinsics.fy * (camera_coords.y / camera_coords.z) + intrinsics.cy;
    if u.is_finite() && v.is_finite() { Some(PixelCoordinate { u, v }) } else { None }
}

fn reprojection_error(world: &Translation3, observed: PixelCoordinate, pose: &DevicePose, intrinsics: &CameraIntrinsics) -> Option<f64> {
    let projected = project_world_point(world, pose, intrinsics)?;
    let du = projected.u - observed.u;
    let dv = projected.v - observed.v;
    Some((du * du + dv * dv).sqrt())
}

fn reprojection_residual_wc(world: Translation3, observed: PixelCoordinate, camera_position: &Vector3<f64>, rotation_wc: &Matrix3<f64>, intrinsics: &CameraIntrinsics) -> Option<[f64; 2]> {
    let world_vec = translation_to_vec(&world);
    let relative = world_vec - camera_position;
    let camera_coords = rotation_wc * relative;
    if camera_coords.z <= 0.0 || !camera_coords.z.is_finite() {
        return None;
    }
    let u = intrinsics.fx * (camera_coords.x / camera_coords.z) + intrinsics.cx;
    let v = intrinsics.fy * (camera_coords.y / camera_coords.z) + intrinsics.cy;
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    Some([u - observed.u, v - observed.v])
}
