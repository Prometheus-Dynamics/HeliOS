use serde_json::Value as JsonValue;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use lib_cv::modules::aruco::pose::{detections_to_tag_pose_output, TagPoseCalibration, TagPoseMethod};
use lib_cv::modules::aruco::{ArucoDetection2D, DetectionPoseOutput};
use lib_cv::modules::calibration::LensModel;
use lib_runtime_policy::{ResolvedEngineLocalizationArucoPolicy, HELIOS_ENGINE_LOCALIZATION_ARUCO_POLICY};

use super::{LocalizationSourceConfig, LocalizationSourceParser, SourceParse, SourceParserContext};

pub(crate) struct ArucoDetectionsParser;

impl ArucoDetectionsParser {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl LocalizationSourceParser for ArucoDetectionsParser {
    fn parse(&self, value: &JsonValue, ctx: &SourceParserContext<'_>) -> Option<Result<SourceParse, String>> {
        let detections = match parse_aruco_detections(value) {
            Ok(detections) => detections,
            Err(err) => return Some(Err(err)),
        };
        let mut quality_queues = detection_quality_queues(&detections);

        match build_pose_output_from_detections(&detections, ctx.source, ctx.default_tag_size_m, ctx.calibrations) {
            Ok(output) => match super::detections::parsed_from_pose_output(output) {
                Ok(mut parsed) => {
                    for detection in &mut parsed.detections {
                        let rotation = detection.code_rotation.unwrap_or(0);
                        if let Some(queue) = quality_queues.get_mut(&(detection.tag_id, rotation)) {
                            if let Some(quality) = queue.pop_front() {
                                detection.quality *= quality;
                            }
                        }
                    }
                    Some(Ok(SourceParse::Detections(parsed)))
                }
                Err(err) => Some(Err(err)),
            },
            Err(err) => Some(Err(err)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UndistortedFisheyeModel {
    Native,
    Pinhole,
}

#[derive(Debug, Clone)]
struct UndistortedFisheyeModelState {
    model: UndistortedFisheyeModel,
    updated_at: Instant,
    switch_votes: u8,
}

struct UndistortedFisheyeModelRuntime {
    state: Mutex<HashMap<String, UndistortedFisheyeModelState>>,
}

fn undistorted_fisheye_model_runtime() -> &'static UndistortedFisheyeModelRuntime {
    // Daedalus source parsing is stateless from the graph's perspective, so the per-source
    // fisheye model vote cache lives in one keyed runtime that naturally expires stale entries.
    static RUNTIME: OnceLock<UndistortedFisheyeModelRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| UndistortedFisheyeModelRuntime { state: Mutex::new(HashMap::new()) })
}

fn undistorted_fisheye_model_state() -> &'static Mutex<HashMap<String, UndistortedFisheyeModelState>> {
    &undistorted_fisheye_model_runtime().state
}

fn aruco_policy() -> &'static ResolvedEngineLocalizationArucoPolicy {
    static VALUE: OnceLock<ResolvedEngineLocalizationArucoPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_LOCALIZATION_ARUCO_POLICY.resolve())
}

fn detection_quality_queues(detections: &[ArucoDetection2D]) -> HashMap<(u32, u8), VecDeque<f32>> {
    let mut queues = HashMap::new();
    for detection in detections {
        let quality = detection_geometry_quality(detection);
        queues.entry((detection.id, detection.rotation & 3)).or_insert_with(VecDeque::new).push_back(quality);
    }
    queues
}

fn detection_geometry_quality(detection: &ArucoDetection2D) -> f32 {
    let corners = detection.corners;

    let edge_lengths = [edge_length(corners[0], corners[1]), edge_length(corners[1], corners[2]), edge_length(corners[2], corners[3]), edge_length(corners[3], corners[0])];

    if edge_lengths.iter().any(|value| !value.is_finite() || *value <= f64::EPSILON) {
        return 0.0;
    }

    let min_edge = edge_lengths.iter().copied().fold(f64::INFINITY, f64::min);
    let max_edge = edge_lengths.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min_edge.is_finite() || !max_edge.is_finite() || max_edge <= f64::EPSILON {
        return 0.0;
    }

    let perimeter = edge_lengths.iter().sum::<f64>();
    let area = quad_area(corners).abs();
    let compactness = if perimeter > f64::EPSILON { (4.0 * std::f64::consts::PI * area / (perimeter * perimeter)).clamp(0.0, 1.0) } else { 0.0 };
    let edge_ratio = (min_edge / max_edge).clamp(0.0, 1.0);

    let mut quality = 1.0_f32;

    // Tiny quads and extremely stretched quads are the most unstable under blur/skew.
    // Keep far/small tags in play at reduced weight instead of hard-dropping early.
    if min_edge < 4.0 {
        quality *= 0.06;
    } else if min_edge < 6.0 {
        quality *= 0.22;
    } else if min_edge < 8.0 {
        quality *= 0.42;
    } else if min_edge < 10.0 {
        quality *= 0.62;
    } else if min_edge < 14.0 {
        quality *= 0.82;
    }

    if edge_ratio < 0.18 {
        quality *= 0.12;
    } else if edge_ratio < 0.28 {
        quality *= 0.3;
    } else if edge_ratio < 0.38 {
        quality *= 0.55;
    } else if edge_ratio < 0.48 {
        quality *= 0.78;
    }

    if compactness < 0.1 {
        quality *= 0.2;
    } else if compactness < 0.18 {
        quality *= 0.45;
    } else if compactness < 0.28 {
        quality *= 0.72;
    }

    if let Some(score) = detection.score {
        if score.is_finite() {
            if score < 8.0 {
                quality *= 0.2;
            } else if score < 12.0 {
                quality *= 0.45;
            } else if score < 18.0 {
                quality *= 0.72;
            }
        }
    }

    if let Some(best) = detection.best_distance {
        if best >= 4 {
            quality *= 0.25;
        } else if best == 3 {
            quality *= 0.45;
        } else if best == 2 {
            quality *= 0.72;
        }
    }

    if let (Some(best), Some(second)) = (detection.best_distance, detection.second_distance) {
        let margin = second.saturating_sub(best);
        if margin == 0 {
            quality *= 0.2;
        } else if margin == 1 {
            quality *= 0.42;
        } else if margin == 2 {
            quality *= 0.62;
        } else if margin <= 4 {
            quality *= 0.82;
        }
    }

    if let Some(mismatches) = detection.border_mismatches {
        if mismatches >= 6 {
            quality *= 0.28;
        } else if mismatches >= 4 {
            quality *= 0.45;
        } else if mismatches >= 2 {
            quality *= 0.68;
        } else if mismatches >= 1 {
            quality *= 0.84;
        }
    }

    if let Some(contrast) = detection.contrast_range {
        if contrast.is_finite() {
            if contrast < 8.0 {
                quality *= 0.18;
            } else if contrast < 12.0 {
                quality *= 0.4;
            } else if contrast < 18.0 {
                quality *= 0.62;
            } else if contrast < 24.0 {
                quality *= 0.82;
            }
        }
    }

    quality.clamp(0.0, 1.0)
}

fn edge_length(a: lib_cv::Point, b: lib_cv::Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn quad_area(corners: [lib_cv::Point; 4]) -> f64 {
    let mut sum = 0.0;
    for idx in 0..4 {
        let next = (idx + 1) & 3;
        sum += corners[idx].x * corners[next].y - corners[next].x * corners[idx].y;
    }
    0.5 * sum
}

pub(crate) fn parse_aruco_detections(value: &JsonValue) -> Result<Vec<ArucoDetection2D>, String> {
    if let Some(raw) = value.as_str() {
        let parsed: JsonValue = serde_json::from_str(raw).map_err(|err| format!("detections_json parse failed: {err}"))?;
        if parsed.is_string() {
            return Err("detections_json parse failed: nested string".to_string());
        }
        return parse_aruco_detections(&parsed);
    }
    if let Ok(list) = serde_json::from_value::<Vec<ArucoDetection2D>>(value.clone()) {
        return Ok(list);
    }
    if let Some(array) = value.get("detections") {
        return serde_json::from_value::<Vec<ArucoDetection2D>>(array.clone()).map_err(|err| err.to_string());
    }
    Err("missing detections array".to_string())
}

pub(crate) fn build_pose_output_from_detections(
    detections: &[ArucoDetection2D],
    source: &LocalizationSourceConfig,
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
) -> Result<DetectionPoseOutput, String> {
    let tag_size = default_tag_size_m.ok_or_else(|| "missing tag size for detections".to_string())?;
    let calibration = calibrations.get(&source.stream_id).cloned().ok_or_else(|| "missing camera calibration".to_string())?;

    if detections.is_empty() {
        return Err("no detections".to_string());
    }

    // `detections_to_tag_pose_output` always undistorts corners internally before pose solve.
    // If detections are already in an undistorted/rectified image space we must avoid applying
    // distortion again.
    //
    // Some sources do not reliably publish whether corners are raw-vs-undistorted. For those,
    // evaluate both interpretations and pick the one with better solve quality.
    match source_input_space(source) {
        SourceInputSpace::Raw => Ok(detections_to_tag_pose_output(detections, calibration, tag_size, localization_tag_pose_method())),
        SourceInputSpace::Undistorted => Ok(best_undistorted_pose_output(detections, source, calibration, tag_size)),
        SourceInputSpace::Unknown => {
            if !calibration_has_distortion(&calibration) {
                return Ok(detections_to_tag_pose_output(detections, calibration, tag_size, localization_tag_pose_method()));
            }
            let raw_output = detections_to_tag_pose_output(detections, calibration, tag_size, localization_tag_pose_method());
            let undist_output = best_undistorted_pose_output(detections, source, calibration, tag_size);
            Ok(select_best_pose_output(raw_output, undist_output))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceInputSpace {
    Raw,
    Undistorted,
    Unknown,
}

fn source_input_space(source: &LocalizationSourceConfig) -> SourceInputSpace {
    // Prefer explicit source input binding when provided, then fall back to output key.
    // Both are treated as hints and only recognized for known image-space tokens.
    classify_input_space(source.input_key.as_deref()).or_else(|| classify_input_space(Some(source.output_key.as_str()))).unwrap_or(SourceInputSpace::Unknown)
}

fn classify_input_space(key: Option<&str>) -> Option<SourceInputSpace> {
    let key = key?.trim();
    if key.is_empty() {
        return None;
    }
    let normalized = key.to_ascii_lowercase();
    if normalized == "undistorted" || normalized == "rectified" {
        return Some(SourceInputSpace::Undistorted);
    }
    if normalized == "raw" || normalized == "frame" || normalized == "distorted" {
        return Some(SourceInputSpace::Raw);
    }
    None
}

fn as_pinhole_calibration(mut calibration: TagPoseCalibration) -> TagPoseCalibration {
    calibration.k1 = 0.0;
    calibration.k2 = 0.0;
    calibration.p1 = 0.0;
    calibration.p2 = 0.0;
    calibration.k3 = 0.0;
    calibration.undistort_iters = 1;
    calibration.lens_model = LensModel::Pinhole;
    calibration
}

fn as_native_rectified_calibration(mut calibration: TagPoseCalibration) -> TagPoseCalibration {
    calibration.k1 = 0.0;
    calibration.k2 = 0.0;
    calibration.p1 = 0.0;
    calibration.p2 = 0.0;
    calibration.k3 = 0.0;
    calibration.undistort_iters = 1;
    calibration
}

fn scale_calibration_focal(mut calibration: TagPoseCalibration, scale: f64) -> TagPoseCalibration {
    let s = if scale.is_finite() { scale.clamp(0.5, 1.5) } else { 1.0 };
    calibration.fx *= s;
    calibration.fy *= s;
    calibration
}

fn localization_pose_scale_sweep_enabled() -> bool {
    aruco_policy().pose_scale_sweep
}

fn localization_dual_model_eval_enabled() -> bool {
    aruco_policy().dual_model_eval
}

fn localization_undistorted_fisheye_model_override() -> Option<UndistortedFisheyeModel> {
    aruco_policy().undistorted_fisheye_model.as_deref().and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
        "native" | "fisheye" => Some(UndistortedFisheyeModel::Native),
        "pinhole" => Some(UndistortedFisheyeModel::Pinhole),
        "auto" | "" => None,
        _ => None,
    })
}

fn localization_tag_pose_method() -> TagPoseMethod {
    TagPoseMethod::parse(&aruco_policy().tag_pose_method)
}

fn fisheye_model_state_key(source: &LocalizationSourceConfig) -> String {
    format!("{}:{}", source.stream_id.trim(), source.id.trim())
}

fn preferred_undistorted_model(native_count: usize, native_quality: f64, pinhole_count: usize, pinhole_quality: f64) -> UndistortedFisheyeModel {
    if pinhole_count != native_count {
        return if pinhole_count > native_count { UndistortedFisheyeModel::Pinhole } else { UndistortedFisheyeModel::Native };
    }
    if pinhole_quality + 0.1 < native_quality {
        UndistortedFisheyeModel::Pinhole
    } else {
        UndistortedFisheyeModel::Native
    }
}

fn select_fisheye_model_with_hysteresis(source: &LocalizationSourceConfig, native_count: usize, native_quality: f64, pinhole_count: usize, pinhole_quality: f64) -> UndistortedFisheyeModel {
    let preferred = preferred_undistorted_model(native_count, native_quality, pinhole_count, pinhole_quality);
    let key = fisheye_model_state_key(source);
    let now = Instant::now();
    let store = undistorted_fisheye_model_state();
    let Ok(mut store) = store.lock() else {
        return preferred;
    };

    let stale_after = Duration::from_millis(1800);
    let required_votes: u8 = 3;
    let strong_quality_margin = 0.32;
    let strong_count_delta: isize = 2;

    let state = store.entry(key).or_insert_with(|| UndistortedFisheyeModelState { model: preferred, updated_at: now, switch_votes: 0 });

    if now.saturating_duration_since(state.updated_at) > stale_after {
        state.model = preferred;
        state.switch_votes = 0;
        state.updated_at = now;
        return state.model;
    }

    if preferred == state.model {
        state.switch_votes = 0;
        state.updated_at = now;
        return state.model;
    }

    let (candidate_count, candidate_quality, current_count, current_quality) = match preferred {
        UndistortedFisheyeModel::Pinhole => (pinhole_count as isize, pinhole_quality, native_count as isize, native_quality),
        UndistortedFisheyeModel::Native => (native_count as isize, native_quality, pinhole_count as isize, pinhole_quality),
    };

    let count_gain = candidate_count - current_count;
    let quality_gain = current_quality - candidate_quality;
    let strong_switch = count_gain >= strong_count_delta || quality_gain >= strong_quality_margin;

    if strong_switch {
        state.model = preferred;
        state.switch_votes = 0;
        state.updated_at = now;
        return state.model;
    }

    state.switch_votes = state.switch_votes.saturating_add(1);
    if state.switch_votes >= required_votes {
        state.model = preferred;
        state.switch_votes = 0;
    }
    state.updated_at = now;
    state.model
}

fn best_scaled_pose_output(detections: &[ArucoDetection2D], calibration: TagPoseCalibration, tag_size: f64) -> DetectionPoseOutput {
    let pose_method = localization_tag_pose_method();
    // Scale sweep can improve hit-rate with stale/misaligned intrinsics, but it can also
    // introduce frame-to-frame branch hopping that destabilizes 3D tag poses at range.
    // Keep stable-by-default behavior for localization; enable sweep only for diagnostics.
    if !localization_pose_scale_sweep_enabled() {
        return detections_to_tag_pose_output(detections, scale_calibration_focal(calibration, 1.0), tag_size, pose_method);
    }

    // Try a narrow focal-scale sweep around 1.0 to absorb mild effective-intrinsics drift
    // (for example from undistort remap zoom/fill or stale calibration-by-a-few-percent).
    const SCALE_CANDIDATES: [f64; 5] = [1.0, 0.93, 1.07, 0.87, 1.13];

    let mut best_scale = 1.0_f64;
    let mut best_output = detections_to_tag_pose_output(detections, scale_calibration_focal(calibration, best_scale), tag_size, pose_method);
    let mut best_quality = pose_output_quality(&best_output);
    let mut best_count = best_output.stats.output_detections;

    for scale in SCALE_CANDIDATES.iter().copied().skip(1) {
        let candidate = detections_to_tag_pose_output(detections, scale_calibration_focal(calibration, scale), tag_size, pose_method);
        let candidate_count = candidate.stats.output_detections;
        let candidate_quality = pose_output_quality(&candidate);

        let better_count = candidate_count > best_count;
        let better_quality = candidate_count == best_count && candidate_quality + 0.05 < best_quality;
        let similar_quality = candidate_count == best_count && (candidate_quality - best_quality).abs() <= 0.05;
        let better_scale_stability = similar_quality && (scale - 1.0).abs() + 1e-9 < (best_scale - 1.0).abs();

        if better_count || better_quality || better_scale_stability {
            best_output = candidate;
            best_quality = candidate_quality;
            best_count = candidate_count;
            best_scale = scale;
        }
    }

    best_output
}

fn best_undistorted_pose_output(detections: &[ArucoDetection2D], source: &LocalizationSourceConfig, calibration: TagPoseCalibration, tag_size: f64) -> DetectionPoseOutput {
    // Build lens-aware candidates instead of forcing pinhole:
    // - Always test a pinhole-rectified interpretation.
    // - If source calibration is fisheye, also test a fisheye-native rectified interpretation.
    // Pick whichever solves better.
    let pinhole_candidate = best_scaled_pose_output(detections, as_pinhole_calibration(calibration), tag_size);
    if calibration.lens_model == LensModel::Pinhole {
        return pinhole_candidate;
    }

    let native_candidate = best_scaled_pose_output(detections, as_native_rectified_calibration(calibration), tag_size);

    if let Some(force_model) = localization_undistorted_fisheye_model_override() {
        return match force_model {
            UndistortedFisheyeModel::Native => native_candidate,
            UndistortedFisheyeModel::Pinhole => pinhole_candidate,
        };
    }

    // Optional diagnostics path: evaluate both candidates frame-by-frame directly.
    if localization_dual_model_eval_enabled() {
        select_best_pose_output(native_candidate, pinhole_candidate)
    } else {
        // Default path: evaluate both, but select with source-local hysteresis so we keep a
        // stable lens interpretation unless the alternative is consistently better.
        let native_count = native_candidate.stats.output_detections;
        let pinhole_count = pinhole_candidate.stats.output_detections;
        let native_quality = pose_output_quality(&native_candidate);
        let pinhole_quality = pose_output_quality(&pinhole_candidate);
        match select_fisheye_model_with_hysteresis(source, native_count, native_quality, pinhole_count, pinhole_quality) {
            UndistortedFisheyeModel::Native => native_candidate,
            UndistortedFisheyeModel::Pinhole => pinhole_candidate,
        }
    }
}

fn calibration_has_distortion(calibration: &TagPoseCalibration) -> bool {
    let tol = 1e-9;
    calibration.k1.abs() > tol || calibration.k2.abs() > tol || calibration.p1.abs() > tol || calibration.p2.abs() > tol || calibration.k3.abs() > tol
}

fn select_best_pose_output(raw_output: DetectionPoseOutput, undistorted_output: DetectionPoseOutput) -> DetectionPoseOutput {
    let raw_count = raw_output.stats.output_detections;
    let undistorted_count = undistorted_output.stats.output_detections;
    if raw_count != undistorted_count {
        return if undistorted_count > raw_count { undistorted_output } else { raw_output };
    }

    let raw_quality = pose_output_quality(&raw_output);
    let undistorted_quality = pose_output_quality(&undistorted_output);
    // Keep `raw` as the tie-breaker so behavior stays stable when both interpretations are
    // effectively equivalent.
    if undistorted_quality + 0.2 < raw_quality {
        undistorted_output
    } else {
        raw_output
    }
}

fn pose_output_quality(output: &DetectionPoseOutput) -> f64 {
    if output.detections.is_empty() {
        return f64::INFINITY;
    }

    let mut errors = output.detections.iter().filter_map(|det| det.reprojection_error_px).filter(|err| err.is_finite() && *err >= 0.0).collect::<Vec<_>>();

    if errors.is_empty() {
        return 1.5;
    }

    errors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if errors.len() % 2 == 1 {
        errors[errors.len() / 2]
    } else {
        let hi = errors.len() / 2;
        (errors[hi - 1] + errors[hi]) * 0.5
    };
    let mean = errors.iter().sum::<f64>() / errors.len() as f64;
    let decompose_failures = output.stats.failures.decompose as f64;

    // Reprojection quality dominates. Penalize decompose failures to avoid branch flips under skew.
    median + (0.25 * mean) + (0.1 * decompose_failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lib_cv::modules::aruco::{DetectionPose, DetectionPoseCalibrationSummary, DetectionPoseFailureCounts, DetectionPoseOutput, DetectionPoseRotation, DetectionPoseStats, DetectionQuat};
    use lib_cv::Translation3;

    fn make_pose_output(reprojection_errors: &[f64], decompose_failures: usize) -> DetectionPoseOutput {
        let detections = reprojection_errors
            .iter()
            .map(|err| DetectionPose {
                id: 1,
                translation: Translation3 { x: 0.0, y: 0.0, z: 1.0 },
                rotation: DetectionPoseRotation { roll: 0.0, pitch: 0.0, yaw: 0.0, quaternion: DetectionQuat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } },
                reprojection_error_px: Some(*err),
                code_rotation: 0,
                bits: None,
                tag_size: 0.165,
                pose_method: "pnp_refine".to_string(),
            })
            .collect::<Vec<_>>();

        DetectionPoseOutput {
            detections,
            stats: DetectionPoseStats {
                input_detections: reprojection_errors.len(),
                output_detections: reprojection_errors.len(),
                pose_method: "pnp_refine".to_string(),
                tag_size: 0.165,
                calibration: DetectionPoseCalibrationSummary { fx: 300.0, fy: 300.0, cx: 320.0, cy: 240.0 },
                estimated_intrinsics: false,
                failures: DetectionPoseFailureCounts { invalid: 0, homography: 0, decompose: decompose_failures },
                first_failure: None,
            },
        }
    }

    #[test]
    fn select_best_prefers_higher_detection_count() {
        let raw = make_pose_output(&[0.8], 0);
        let undistorted = make_pose_output(&[1.4, 1.5], 0);
        let selected = select_best_pose_output(raw, undistorted.clone());
        assert_eq!(selected.stats.output_detections, undistorted.stats.output_detections);
    }

    #[test]
    fn select_best_prefers_lower_reprojection_error_when_counts_match() {
        let raw = make_pose_output(&[1.9, 2.2], 0);
        let undistorted = make_pose_output(&[0.7, 0.8], 0);
        let selected = select_best_pose_output(raw, undistorted.clone());
        assert!(selected.detections.iter().all(|det| det.reprojection_error_px.is_some_and(|err| err < 1.0)));
    }

    #[test]
    fn select_best_prefers_raw_when_quality_is_near_tie() {
        let raw = make_pose_output(&[0.90, 0.95], 0);
        let undistorted = make_pose_output(&[0.92, 0.98], 0);
        let selected = select_best_pose_output(raw.clone(), undistorted);
        assert_eq!(selected.stats.output_detections, raw.stats.output_detections);
        assert!(selected.detections.iter().all(|det| det.reprojection_error_px.is_some_and(|err| err <= 0.95)));
    }
}
