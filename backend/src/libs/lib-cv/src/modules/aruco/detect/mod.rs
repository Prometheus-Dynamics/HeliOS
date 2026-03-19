#![allow(unsafe_code)]

use crate::modules::calibration::LensModel;
use crate::modules::image::luma::with_luma8_frame;
use image::{DynamicImage, GrayImage, Luma};
use imageproc::{
    geometric_transformations::{Interpolation, Projection, warp_into},
    point::Point,
};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use tracing::{Level, trace};
use utoipa::ToSchema;

use super::{ArucoBitGrid, ArucoDetection2D};
use crate::aruco::tag::{ArucoTagDecodeTuning, ArucoTagDecoding, ArucoTagFamily, DecodeFromCellMeansError, decode_from_cell_means_result_detailed_with_tuning, decode_marker_grid};
use crate::modules::aruco::ArucoDictionary;

mod aruco_calibrated;
mod aruco_decode;
mod aruco_warp;
mod calibrated;
mod candidates;
mod config;
mod debug;
mod decode_quads;
mod distortion;
mod quad_decode;
mod refine;
mod sampled;
mod sampling;
mod types;
mod utils;
mod warp;

// Keep the split file tree behaving like the old single-module `detect.rs` by lifting shared
// internals into this module scope. Submodules continue to `use super::*;` and call helpers
// unqualified without having to thread paths everywhere.
#[allow(unused_imports)]
use aruco_calibrated::*;
#[allow(unused_imports)]
use aruco_decode::*;
#[allow(unused_imports)]
use aruco_warp::*;
#[allow(unused_imports)]
use calibrated::*;
#[allow(unused_imports)]
use candidates::*;
#[allow(unused_imports)]
use debug::*;
#[allow(unused_imports)]
use decode_quads::*;
#[allow(unused_imports)]
use distortion::*;
#[allow(unused_imports)]
use quad_decode::*;
#[allow(unused_imports)]
use refine::*;
#[allow(unused_imports)]
use sampled::*;
#[allow(unused_imports)]
use sampling::*;
#[allow(unused_imports)]
use types::*;
#[allow(unused_imports)]
use utils::*;
#[allow(unused_imports)]
use warp::*;

pub use aruco_calibrated::{
    decode_quads_aruco_calibrated_with_config, decode_quads_aruco_calibrated_with_config_no_bits, decode_quads_aruco_with_config, decode_quads_aruco_with_config_gray,
    decode_quads_aruco_with_config_no_bits, decode_quads_aruco_with_config_no_bits_gray,
};
pub use calibrated::{decode_quads_calibrated, decode_quads_calibrated_with_config};
pub use candidates::{candidate_quad_from_contour, candidate_quad_from_contour_fast, filter_candidates, filter_candidates_fast, quad_satisfies_config};
pub use config::{ArucoDecodeConfig, ArucoTagDecodeConfig, ArucoTagDetectorConfig, CameraCalibration};
pub use debug::{DecodeQuadFailure, DecodeQuadOutcome, decode_quad_debug};
pub use decode_quads::{
    DecodeQuadsStats, decode_quads, decode_quads_gray, decode_quads_with_config, decode_quads_with_config_gray, decode_quads_with_config_no_bits, decode_quads_with_config_no_bits_gray,
    decode_quads_with_stats, decode_quads_with_stats_config,
};
pub use refine::{refine_detection_corners_warp, refine_detection_corners_warp_aruco};
pub(crate) use types::{compact_decode_scratch_after_frame, compact_detect_scratch_after_frame};
pub use utils::sort_corners_clockwise;
pub use warp::{decode_quads_warp, decode_quads_warp_with_config};
