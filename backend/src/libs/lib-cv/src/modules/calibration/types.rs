use crate::Point;
use crate::modules::localization::CameraIntrinsics;
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub(super) type Point2 = (f64, f64);
pub(super) type Points2 = Vec<Point2>;
pub(super) type ExtrinsicsSolveResult = (Vec<Extrinsics>, Vec<usize>, Vec<String>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistortionCoefficients {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub p1: f64,
    pub p2: f64,
}

impl Default for DistortionCoefficients {
    fn default() -> Self {
        Self { k1: 0.0, k2: 0.0, k3: 0.0, p1: 0.0, p2: 0.0 }
    }
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:lens_model"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum LensModel {
    #[default]
    Pinhole,
    Fisheye,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraCalibration {
    pub intrinsics: CameraIntrinsics,
    pub distortion: DistortionCoefficients,
    pub undistort_iters: u8,
    pub lens_model: LensModel,
}

impl CameraCalibration {
    pub fn new(intrinsics: CameraIntrinsics, distortion: DistortionCoefficients, undistort_iters: u8, lens_model: LensModel) -> Self {
        Self { intrinsics, distortion, undistort_iters, lens_model }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationPointPair {
    pub object: Point,
    pub image: Point,
}

impl CalibrationPointPair {
    pub fn new(object: Point, image: Point) -> Self {
        Self { object, image }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationView {
    pub points: Vec<CalibrationPointPair>,
}

impl CalibrationView {
    pub fn new(points: Vec<CalibrationPointPair>) -> Self {
        Self { points }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationSolveConfig {
    pub min_views: usize,
    pub min_points_per_view: usize,
    pub refine_distortion: bool,
    pub undistort_iters: u8,
    pub refine_undistort_iters: u8,
    pub lens_model: LensModel,
}

impl Default for CalibrationSolveConfig {
    fn default() -> Self {
        Self { min_views: 2, min_points_per_view: 12, refine_distortion: true, undistort_iters: 5, refine_undistort_iters: 8, lens_model: LensModel::Pinhole }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationSolveResult {
    pub calibration: CameraCalibration,
    pub reprojection_error_px: f64,
    pub views_used: usize,
    pub points_used: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationSolveError {
    InsufficientViews { required: usize, provided: usize },
    InsufficientPoints { required: usize, provided: usize },
    DegenerateHomography,
    IntrinsicsSolveFailed,
    ExtrinsicsSolveFailed,
}

#[derive(Debug, Clone)]
pub(super) struct ViewObservations {
    pub(super) index: usize,
    pub(super) points: Vec<(Point2, Point2)>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct IntrinsicsSolve {
    pub(super) fx: f64,
    pub(super) fy: f64,
    pub(super) skew: f64,
    pub(super) cx: f64,
    pub(super) cy: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Extrinsics {
    pub(super) r: Matrix3<f64>,
    pub(super) t: Vector3<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct SolveState {
    pub(super) intrinsics: IntrinsicsSolve,
    pub(super) distortion: DistortionCoefficients,
    pub(super) reprojection_error_px: f64,
    pub(super) warnings: Vec<String>,
    pub(super) views_used: usize,
    pub(super) points_used: usize,
    pub(super) intrinsics_degenerate: bool,
    pub(super) extrinsics: Vec<Extrinsics>,
    pub(super) view_indices: Vec<usize>,
    pub(super) lens_model: LensModel,
}
