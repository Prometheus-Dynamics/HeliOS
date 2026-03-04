use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use lib_cv::modules::aruco::pose::TagPoseCalibration;
use lib_cv::modules::aruco::ArucoBitGrid;
use lib_cv::Translation3;
use nalgebra::{Quaternion, UnitQuaternion, Vector3};

use super::config::LocalizationSourceConfig;
use super::math::{pose_from_detection, PoseTransform};
use super::types::LocalizationQuaternion;

mod aruco;
mod detections;
mod pose;

pub trait LocalizationSourceFetcher: Send + Sync {
    fn fetch_source_value<'a>(&'a self, source: &'a LocalizationSourceConfig) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>>;
}

pub enum SourceParse {
    Detections(ParsedDetections),
    Pose(PoseSample),
}

pub struct SourceParserContext<'a> {
    pub source: &'a LocalizationSourceConfig,
    pub default_tag_size_m: Option<f64>,
    pub calibrations: &'a HashMap<String, TagPoseCalibration>,
}

pub trait LocalizationSourceParser: Send + Sync {
    fn parse(&self, value: &serde_json::Value, ctx: &SourceParserContext<'_>) -> Option<Result<SourceParse, String>>;
}

pub struct SourceParserRegistry {
    parsers: Vec<Box<dyn LocalizationSourceParser>>,
}

impl SourceParserRegistry {
    pub fn new() -> Self {
        Self { parsers: Vec::new() }
    }

    pub fn register<P: LocalizationSourceParser + 'static>(&mut self, parser: P) {
        self.parsers.push(Box::new(parser));
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(detections::DetectionPoseParser::new());
        registry.register(pose::PoseParser::new());
        registry.register(aruco::ArucoDetectionsParser::new());
        registry
    }

    pub fn parse(&self, value: &serde_json::Value, ctx: &SourceParserContext<'_>) -> Result<SourceParse, String> {
        for parser in &self.parsers {
            if let Some(result) = parser.parse(value, ctx) {
                return result;
            }
        }
        Err("unsupported source payload".to_string())
    }
}

impl Default for SourceParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct LocalizationDetection {
    pub source_id: String,
    pub camera_uid: String,
    pub tag_id: u32,
    pub camera_from_tag: PoseTransform,
    pub tag_size: Option<f64>,
    pub code_rotation: Option<u8>,
    pub tag_bits: Option<ArucoBitGrid>,
    pub weight: f32,
    pub quality: f32,
}

#[derive(Debug)]
pub struct SourceSample {
    pub source: LocalizationSourceConfig,
    pub detections: Vec<LocalizationDetection>,
    pub pose: Option<PoseSample>,
    pub poll_ms: f64,
    pub tag_size: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct ParsedDetections {
    pub detections: Vec<LocalizationDetection>,
    pub tag_size: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PoseSample {
    pub pose: PoseTransform,
    pub has_translation: bool,
    pub has_rotation: bool,
}

#[derive(Debug, Clone)]
struct TagPoseTemporalState {
    translation: Vector3<f64>,
    rotation: UnitQuaternion<f64>,
    updated_at: Instant,
    outlier_streak: u8,
}

#[derive(Debug, Clone)]
struct TagPairDistanceTemporalState {
    distance_m: f64,
    updated_at: Instant,
    outlier_streak: u8,
}

static TAG_POSE_TEMPORAL_STATE: OnceLock<Mutex<HashMap<String, TagPoseTemporalState>>> = OnceLock::new();
static TAG_PAIR_DISTANCE_TEMPORAL_STATE: OnceLock<Mutex<HashMap<String, TagPairDistanceTemporalState>>> = OnceLock::new();
static IMU_BACKEND_TO_VIEWER_BASIS: OnceLock<UnitQuaternion<f64>> = OnceLock::new();

pub async fn fetch_source_samples<F: LocalizationSourceFetcher>(
    fetcher: &F,
    sources: &[LocalizationSourceConfig],
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
) -> Vec<SourceSample> {
    let registry = SourceParserRegistry::with_defaults();
    fetch_source_samples_with_registry(fetcher, sources, default_tag_size_m, calibrations, &registry).await
}

pub async fn fetch_source_value<F: LocalizationSourceFetcher>(fetcher: &F, source: &LocalizationSourceConfig) -> Result<serde_json::Value, String> {
    fetcher.fetch_source_value(source).await
}

pub async fn fetch_source_samples_with_registry<F: LocalizationSourceFetcher>(
    fetcher: &F,
    sources: &[LocalizationSourceConfig],
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
    registry: &SourceParserRegistry,
) -> Vec<SourceSample> {
    let mut out = Vec::new();
    for source in sources {
        out.push(fetch_source_sample(fetcher, source, default_tag_size_m, calibrations, registry).await);
    }
    out
}

async fn fetch_source_sample<F: LocalizationSourceFetcher>(
    fetcher: &F,
    source: &LocalizationSourceConfig,
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
    registry: &SourceParserRegistry,
) -> SourceSample {
    let start = Instant::now();
    let mut detections = Vec::new();
    let mut pose = None;
    let mut tag_size = None;
    let mut error = None;

    match fetcher.fetch_source_value(source).await {
        Ok(value) => {
            let ctx = SourceParserContext { source, default_tag_size_m, calibrations };
            match registry.parse(&value, &ctx) {
                Ok(SourceParse::Detections(mut parsed)) => {
                    collapse_duplicate_tag_detections(source, &mut parsed.detections);
                    apply_pair_distance_consistency(source, &mut parsed.detections);
                    for detection in &mut parsed.detections {
                        detection.source_id = source.id.clone();
                        detection.camera_uid = source.camera_uid.clone();
                        let pose_quality = pose_reliability_quality(&detection.camera_from_tag);
                        detection.weight = source.weight.max(0.0) * detection.quality.max(0.0) * pose_quality;
                    }
                    smooth_detection_tag_poses(source, &mut parsed.detections);
                    apply_multitag_normal_consistency(&mut parsed.detections);
                    detections = parsed.detections;
                    tag_size = parsed.tag_size;
                }
                Ok(SourceParse::Pose(parsed_pose)) => {
                    pose = Some(parsed_pose);
                }
                Err(err) => error = Some(err),
            }
        }
        Err(err) => error = Some(err),
    }

    SourceSample { source: source.clone(), detections, pose, poll_ms: start.elapsed().as_secs_f64() * 1000.0, tag_size, error }
}

fn pose_reliability_quality(camera_from_tag: &PoseTransform) -> f32 {
    if !camera_from_tag.translation.iter().all(|value| value.is_finite()) {
        return 0.0;
    }

    let tag_normal_in_camera = camera_from_tag.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
    let frontal = tag_normal_in_camera.z.abs().clamp(0.0, 1.0);
    let mut quality = if frontal < 0.10 {
        0.08
    } else if frontal < 0.18 {
        0.16
    } else if frontal < 0.28 {
        0.34
    } else if frontal < 0.38 {
        0.56
    } else if frontal < 0.50 {
        0.78
    } else {
        1.0
    };

    let translation = camera_from_tag.translation;
    let depth = translation.z.abs();
    if !depth.is_finite() || depth < 0.08 {
        quality *= 0.2;
    } else if depth < 0.15 {
        quality *= 0.55;
    }

    let lateral = (translation.x * translation.x + translation.y * translation.y).sqrt();
    if lateral.is_finite() && depth.is_finite() && depth > 1e-6 {
        let skew_ratio = lateral / depth;
        if skew_ratio > 2.8 {
            quality *= 0.18;
        } else if skew_ratio > 2.2 {
            quality *= 0.35;
        } else if skew_ratio > 1.7 {
            quality *= 0.58;
        } else if skew_ratio > 1.3 {
            quality *= 0.78;
        }
    }

    quality as f32
}

fn collapse_duplicate_tag_detections(source: &LocalizationSourceConfig, detections: &mut Vec<LocalizationDetection>) {
    if detections.len() < 2 {
        return;
    }

    let mut first_seen = HashMap::<u32, usize>::new();
    let mut grouped = HashMap::<u32, Vec<LocalizationDetection>>::new();
    for (idx, detection) in std::mem::take(detections).into_iter().enumerate() {
        first_seen.entry(detection.tag_id).or_insert(idx);
        grouped.entry(detection.tag_id).or_default().push(detection);
    }
    if grouped.values().all(|group| group.len() <= 1) {
        let mut ordered_ids = grouped.keys().copied().collect::<Vec<_>>();
        ordered_ids.sort_by_key(|tag_id| first_seen.get(tag_id).copied().unwrap_or(usize::MAX));
        detections.extend(ordered_ids.into_iter().filter_map(|tag_id| grouped.remove(&tag_id).and_then(|mut group| group.pop())));
        return;
    }

    let now = Instant::now();
    let stale_after = Duration::from_millis(900);
    let state_store = TAG_POSE_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut prior_translation_by_tag = HashMap::<u32, Vector3<f64>>::new();
    if let Ok(state_store) = state_store.lock() {
        for (tag_id, group) in &grouped {
            if group.len() <= 1 {
                continue;
            }
            let key = format!("{}:{}:{tag_id}", source.stream_id.trim(), source.id.trim());
            if let Some(state) = state_store.get(&key) {
                if now.saturating_duration_since(state.updated_at) <= stale_after {
                    prior_translation_by_tag.insert(*tag_id, state.translation);
                }
            }
        }
    }

    let mut ordered_ids = grouped.keys().copied().collect::<Vec<_>>();
    ordered_ids.sort_by_key(|tag_id| first_seen.get(tag_id).copied().unwrap_or(usize::MAX));

    for tag_id in ordered_ids {
        let Some(group) = grouped.remove(&tag_id) else {
            continue;
        };
        if group.len() == 1 {
            detections.extend(group);
            continue;
        }

        let prior_translation = prior_translation_by_tag.get(&tag_id).copied();
        let mut scored = group
            .into_iter()
            .map(|detection| {
                let mut score = (detection.quality as f64).clamp(0.0, 1.0);
                let pose_quality = (pose_reliability_quality(&detection.camera_from_tag) as f64).clamp(0.0, 1.0);
                score *= 0.68 + (0.32 * pose_quality);
                if let Some(previous_translation) = prior_translation {
                    let shift = (detection.camera_from_tag.translation - previous_translation).norm();
                    if shift.is_finite() {
                        if shift > 2.0 {
                            score *= 0.22;
                        } else if shift > 1.2 {
                            score *= 0.36;
                        } else if shift > 0.7 {
                            score *= 0.55;
                        } else if shift > 0.35 {
                            score *= 0.78;
                        }
                    }
                }
                (detection, score.max(0.0))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(std::cmp::Ordering::Equal));

        let selected_score = scored.first().map(|entry| entry.1).unwrap_or(0.0);
        let mut selected = scored.remove(0).0;

        if let Some((runner_up, runner_score)) = scored.first() {
            let separation = (selected.camera_from_tag.translation - runner_up.camera_from_tag.translation).norm();
            if separation.is_finite() && selected_score > 1e-6 {
                let ambiguity = (runner_score / selected_score).clamp(0.0, 1.0);
                if ambiguity >= 0.85 && separation > 1.0 {
                    selected.quality *= 0.45;
                } else if ambiguity >= 0.72 && separation > 0.55 {
                    selected.quality *= 0.62;
                } else if ambiguity >= 0.58 && separation > 0.30 {
                    selected.quality *= 0.78;
                }
            }
        }

        detections.push(selected);
    }
}

fn detection_pose_state_key(source: &LocalizationSourceConfig, detection: &LocalizationDetection) -> String {
    // Key temporal state by tag identity only (per source stream), not by decode rotation.
    // Rotation labels can flap under blur/skew and should not reset smoothing state.
    format!("{}:{}:{}", source.stream_id.trim(), source.id.trim(), detection.tag_id)
}

fn detection_pair_state_key(source: &LocalizationSourceConfig, left_tag_id: u32, right_tag_id: u32) -> String {
    let (low_id, high_id) = if left_tag_id <= right_tag_id { (left_tag_id, right_tag_id) } else { (right_tag_id, left_tag_id) };
    format!("{}:{}:{}:{}", source.stream_id.trim(), source.id.trim(), low_id, high_id)
}

fn apply_pair_distance_consistency(source: &LocalizationSourceConfig, detections: &mut [LocalizationDetection]) {
    if detections.len() < 2 {
        return;
    }

    let state_store = TAG_PAIR_DISTANCE_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut state_store) = state_store.lock() else {
        return;
    };

    let now = Instant::now();
    let stale_after = Duration::from_millis(1500);
    let mut penalties = vec![1.0_f64; detections.len()];
    let pair_distance_lock_enabled = localization_pair_distance_translation_lock_enabled();
    let pair_distance_lock_strength = localization_pair_distance_translation_lock_strength();
    let pair_distance_lock_max_shift_m = localization_pair_distance_translation_lock_max_shift_m();
    let mut correction_accum = vec![Vector3::zeros(); detections.len()];
    let mut correction_weight_sum = vec![0.0_f64; detections.len()];

    for left_idx in 0..detections.len() {
        for right_idx in (left_idx + 1)..detections.len() {
            let left = &detections[left_idx];
            let right = &detections[right_idx];
            let delta = right.camera_from_tag.translation - left.camera_from_tag.translation;
            let observed_distance = delta.norm();
            if !observed_distance.is_finite() || observed_distance <= 0.02 {
                continue;
            }

            let key = detection_pair_state_key(source, left.tag_id, right.tag_id);
            let state = state_store.entry(key).or_insert_with(|| TagPairDistanceTemporalState { distance_m: observed_distance, updated_at: now, outlier_streak: 0 });

            let stale = now.saturating_duration_since(state.updated_at) > stale_after;
            if stale {
                state.distance_m = observed_distance;
                state.updated_at = now;
                state.outlier_streak = 0;
                continue;
            }

            let abs_err = (observed_distance - state.distance_m).abs();
            let rel_err = abs_err / state.distance_m.max(0.08);
            let (pair_penalty, severe_outlier) = if abs_err > 0.55 || rel_err > 0.45 {
                (0.08, true)
            } else if abs_err > 0.35 || rel_err > 0.30 {
                (0.25, true)
            } else if abs_err > 0.22 || rel_err > 0.20 {
                (0.52, false)
            } else if abs_err > 0.14 || rel_err > 0.12 {
                (0.76, false)
            } else {
                (1.0, false)
            };

            penalties[left_idx] = penalties[left_idx].min(pair_penalty);
            penalties[right_idx] = penalties[right_idx].min(pair_penalty);

            if pair_distance_lock_enabled {
                let left_confidence = ((left.weight as f64).clamp(0.0, 1.0) * (left.quality as f64).clamp(0.0, 1.0)).max(1e-6);
                let right_confidence = ((right.weight as f64).clamp(0.0, 1.0) * (right.quality as f64).clamp(0.0, 1.0)).max(1e-6);
                let (adjust_idx, anchor_idx, adjust_confidence, anchor_confidence) =
                    if left_confidence <= right_confidence { (left_idx, right_idx, left_confidence, right_confidence) } else { (right_idx, left_idx, right_confidence, left_confidence) };

                // Only force translation correction when there is clear disagreement and one
                // observation is materially weaker than the other.
                let disagreement = abs_err > 0.11 || rel_err > 0.12;
                let confidence_split = adjust_confidence < (anchor_confidence * 0.96) || severe_outlier;
                if disagreement && confidence_split {
                    let adjust_translation = detections[adjust_idx].camera_from_tag.translation;
                    let anchor_translation = detections[anchor_idx].camera_from_tag.translation;
                    let baseline = adjust_translation - anchor_translation;
                    let baseline_norm = baseline.norm();
                    if baseline_norm.is_finite() && baseline_norm > 1e-6 {
                        let target_distance = state.distance_m.max(0.02);
                        let direction = baseline / baseline_norm;
                        let correction_target = anchor_translation + direction * target_distance;
                        let correction_delta = correction_target - adjust_translation;
                        let correction_norm = correction_delta.norm();
                        if correction_norm.is_finite() && correction_norm > 1e-6 {
                            let rel_disagreement = (abs_err / state.distance_m.max(0.08)).clamp(0.0, 1.0);
                            let severity = if severe_outlier {
                                1.0
                            } else if pair_penalty <= 0.25 {
                                0.78
                            } else if pair_penalty <= 0.52 {
                                0.52
                            } else {
                                0.30
                            };
                            let confidence_term = (anchor_confidence.sqrt() * (1.0 - adjust_confidence).clamp(0.18, 1.0)).clamp(0.12, 1.0);
                            let blend = (pair_distance_lock_strength * rel_disagreement * severity * confidence_term).clamp(0.0, 0.85);
                            if blend > 1e-4 {
                                let max_shift = if severe_outlier { pair_distance_lock_max_shift_m } else { pair_distance_lock_max_shift_m * 0.55 };
                                let capped_target = adjust_translation + correction_delta * (max_shift / correction_norm).min(1.0);
                                correction_accum[adjust_idx] += capped_target * blend;
                                correction_weight_sum[adjust_idx] += blend;
                            }
                        }
                    }
                }
            }

            if severe_outlier {
                state.outlier_streak = state.outlier_streak.saturating_add(1);
                if state.outlier_streak >= 4 {
                    // Re-anchor only after persistent disagreement so a single bad frame cannot
                    // permanently poison pair-distance history.
                    state.distance_m = observed_distance;
                    state.outlier_streak = 0;
                }
            } else {
                state.distance_m = (0.88 * state.distance_m) + (0.12 * observed_distance);
                state.outlier_streak = 0;
            }
            state.updated_at = now;
        }
    }

    if pair_distance_lock_enabled {
        for (idx, detection) in detections.iter_mut().enumerate() {
            let weight_sum = correction_weight_sum[idx];
            if weight_sum <= 1e-6 {
                continue;
            }
            let target = correction_accum[idx] / weight_sum;
            if !target.iter().all(|value| value.is_finite()) {
                continue;
            }
            let blend = weight_sum.clamp(0.0, 0.80);
            detection.camera_from_tag.translation = detection.camera_from_tag.translation + (target - detection.camera_from_tag.translation) * blend;
        }
    }

    for (index, detection) in detections.iter_mut().enumerate() {
        let penalty = penalties.get(index).copied().unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        detection.quality *= penalty;
    }

    state_store.retain(|_key, state| now.saturating_duration_since(state.updated_at) <= stale_after);
}

fn scale_alpha_for_dt(alpha: f64, dt_s: f64) -> f64 {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return 0.0;
    }
    if alpha >= 1.0 {
        return 1.0;
    }
    let dt_scale = (dt_s / (1.0 / 14.0)).clamp(0.45, 2.5);
    1.0 - (1.0 - alpha).powf(dt_scale)
}

fn localization_pair_distance_translation_lock_enabled() -> bool {
    std::env::var("HELIOS_LOCALIZATION_PAIR_DISTANCE_TRANSLATION_LOCK")
        .ok()
        .map(|raw| {
            let value = raw.trim().to_ascii_lowercase();
            value == "1" || value == "true" || value == "yes" || value == "on"
        })
        .unwrap_or(true)
}

fn localization_pair_distance_translation_lock_strength() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_PAIR_DISTANCE_TRANSLATION_LOCK_STRENGTH").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.0, 1.0)).unwrap_or(0.62)
}

fn localization_pair_distance_translation_lock_max_shift_m() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_PAIR_DISTANCE_TRANSLATION_LOCK_MAX_SHIFT_M").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.02, 2.0)).unwrap_or(0.40)
}

fn smooth_detection_tag_poses(source: &LocalizationSourceConfig, detections: &mut [LocalizationDetection]) {
    if detections.is_empty() {
        return;
    }
    let state_store = TAG_POSE_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut state_store) = state_store.lock() else {
        return;
    };
    let now = Instant::now();
    let stale_after = Duration::from_millis(1200);
    let fast_reanchor_after = Duration::from_millis(420);

    for detection in detections {
        let measurement_translation = detection.camera_from_tag.translation;
        let measurement_rotation = detection.camera_from_tag.rotation;
        if !measurement_translation.iter().all(|value| value.is_finite()) {
            continue;
        }

        let key = detection_pose_state_key(source, detection);
        let weight_quality = (detection.weight as f64).clamp(0.0, 1.0);
        let geometric_quality = (detection.quality as f64).clamp(0.0, 1.0);
        let confidence = (geometric_quality * weight_quality.sqrt()).clamp(0.0, 1.0);
        let distance_m = measurement_translation.norm();
        let mut alpha_t = if confidence >= 0.80 {
            0.42
        } else if confidence >= 0.60 {
            0.28
        } else if confidence >= 0.40 {
            0.18
        } else {
            0.08
        };
        let mut alpha_r = if confidence >= 0.80 {
            0.36
        } else if confidence >= 0.60 {
            0.22
        } else if confidence >= 0.40 {
            0.14
        } else {
            0.06
        };
        if distance_m.is_finite() {
            if distance_m > 2.5 {
                alpha_t *= 0.65;
                alpha_r *= 0.55;
            }
            if distance_m > 4.0 {
                alpha_t *= 0.55;
                alpha_r *= 0.45;
            }
        }

        let mut smoothed_translation = measurement_translation;
        let mut smoothed_rotation = measurement_rotation;
        let mut next_outlier_streak: u8 = 0;
        if let Some(previous) = state_store.get(&key) {
            let elapsed = now.saturating_duration_since(previous.updated_at);
            if elapsed <= fast_reanchor_after {
                let dt_s = elapsed.as_secs_f64().max(1e-3);
                alpha_t = scale_alpha_for_dt(alpha_t, dt_s);
                alpha_r = scale_alpha_for_dt(alpha_r, dt_s);

                let delta_t_m = (measurement_translation - previous.translation).norm();
                let delta_r_deg = previous.rotation.angle_to(&measurement_rotation).to_degrees();
                let translation_jump_limit = (0.15 + (2.8 * dt_s) + (0.03 * distance_m)).clamp(0.18, 0.55);
                let rotation_jump_limit_deg = (14.0 + (120.0 * dt_s) + (4.0 * distance_m)).clamp(18.0, 42.0);
                let severe_jump = delta_t_m > (translation_jump_limit * 2.4) || delta_r_deg > (rotation_jump_limit_deg * 2.4);
                let moderate_jump = delta_t_m > translation_jump_limit || delta_r_deg > rotation_jump_limit_deg;
                let strong_measurement = confidence >= 0.78;

                if moderate_jump {
                    next_outlier_streak = previous.outlier_streak.saturating_add(1);
                } else {
                    next_outlier_streak = 0;
                }

                if severe_jump && strong_measurement && previous.outlier_streak >= 3 {
                    // Re-anchor only after sustained, high-confidence disagreement.
                    smoothed_translation = measurement_translation;
                    smoothed_rotation = measurement_rotation;
                    next_outlier_streak = 0;
                } else {
                    // Jumpy distant tags are the dominant local-space failure mode. Force much
                    // lower gains under jump conditions unless confidence remains high.
                    if moderate_jump {
                        if confidence < 0.65 {
                            alpha_t = alpha_t.min(0.04);
                            alpha_r = alpha_r.min(0.03);
                        } else if confidence < 0.80 {
                            alpha_t = alpha_t.min(0.09);
                            alpha_r = alpha_r.min(0.07);
                        } else {
                            alpha_t = alpha_t.min(0.16);
                            alpha_r = alpha_r.min(0.13);
                        }
                    }

                    if next_outlier_streak >= 4 && confidence >= 0.58 {
                        // Accept a genuine transition once it persists across several frames.
                        smoothed_translation = measurement_translation;
                        smoothed_rotation = measurement_rotation;
                        next_outlier_streak = 0;
                    } else {
                        smoothed_translation = previous.translation + (measurement_translation - previous.translation) * alpha_t;
                        smoothed_rotation = previous.rotation.slerp(&measurement_rotation, alpha_r);
                    }
                }
            }
        }

        detection.camera_from_tag = PoseTransform { translation: smoothed_translation, rotation: smoothed_rotation };
        state_store.insert(key, TagPoseTemporalState { translation: smoothed_translation, rotation: smoothed_rotation, updated_at: now, outlier_streak: next_outlier_streak });
    }

    state_store.retain(|_key, state| now.saturating_duration_since(state.updated_at) <= stale_after);
}

fn localization_multitag_normal_lock_enabled() -> bool {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK")
        .ok()
        .map(|raw| {
            let value = raw.trim().to_ascii_lowercase();
            value == "1" || value == "true" || value == "yes" || value == "on"
        })
        .unwrap_or(true)
}

fn localization_multitag_normal_lock_strength() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK_STRENGTH").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.0, 1.0)).unwrap_or(0.78)
}

fn localization_multitag_normal_lock_max_spread_deg() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK_MAX_SPREAD_DEG").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(8.0, 140.0)).unwrap_or(80.0)
}

fn localization_multitag_normal_lock_max_depth_spread_m() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK_MAX_DEPTH_SPREAD_M").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.05, 4.0)).unwrap_or(0.85)
}

fn localization_multitag_full_lock_same_code_rot_enabled() -> bool {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_FULL_LOCK_SAME_CODE_ROT")
        .ok()
        .map(|raw| {
            let value = raw.trim().to_ascii_lowercase();
            value == "1" || value == "true" || value == "yes" || value == "on"
        })
        .unwrap_or(true)
}

fn localization_multitag_full_lock_strength() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_FULL_LOCK_STRENGTH").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.0, 1.0)).unwrap_or(0.62)
}

fn localization_multitag_full_lock_max_spread_deg() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_FULL_LOCK_MAX_SPREAD_DEG").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(6.0, 120.0)).unwrap_or(50.0)
}

fn localization_multitag_coplanar_depth_lock_enabled() -> bool {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_COPLANAR_DEPTH_LOCK")
        .ok()
        .map(|raw| {
            let value = raw.trim().to_ascii_lowercase();
            value == "1" || value == "true" || value == "yes" || value == "on"
        })
        .unwrap_or(true)
}

fn localization_multitag_coplanar_depth_lock_strength() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_COPLANAR_DEPTH_LOCK_STRENGTH").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.0, 1.0)).unwrap_or(0.72)
}

fn localization_multitag_coplanar_depth_lock_max_shift_m() -> f64 {
    std::env::var("HELIOS_LOCALIZATION_MULTITAG_COPLANAR_DEPTH_LOCK_MAX_SHIFT_M").ok().and_then(|raw| raw.trim().parse::<f64>().ok()).map(|value| value.clamp(0.01, 2.0)).unwrap_or(0.45)
}

fn rotation_with_locked_normal(current: UnitQuaternion<f64>, target_normal: Vector3<f64>) -> Option<UnitQuaternion<f64>> {
    let z = target_normal.try_normalize(1e-12)?;
    let mut x = current.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
    x -= z * x.dot(&z);
    let x = if let Some(xn) = x.try_normalize(1e-12) {
        xn
    } else {
        let mut y = current.transform_vector(&Vector3::new(0.0, 1.0, 0.0));
        y -= z * y.dot(&z);
        y.try_normalize(1e-12)?
    };
    let y = z.cross(&x).try_normalize(1e-12)?;
    let x = y.cross(&z).try_normalize(1e-12)?;

    let r = nalgebra::Matrix3::from_columns(&[x, y, z]);
    let r = nalgebra::Rotation3::from_matrix_unchecked(r);
    Some(UnitQuaternion::from_rotation_matrix(&r))
}

fn apply_multitag_normal_consistency(detections: &mut [LocalizationDetection]) {
    if detections.len() < 2 || !localization_multitag_normal_lock_enabled() {
        return;
    }

    let mut normal_sum = Vector3::zeros();
    let mut weighted_normals = Vec::with_capacity(detections.len());
    let mut min_depth = f64::INFINITY;
    let mut max_depth = f64::NEG_INFINITY;
    for detection in detections.iter() {
        let normal = detection.camera_from_tag.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
        if !normal.iter().all(|value| value.is_finite()) {
            continue;
        }
        let depth = detection.camera_from_tag.translation.z.abs();
        if depth.is_finite() {
            min_depth = min_depth.min(depth);
            max_depth = max_depth.max(depth);
        }
        let weight = ((detection.weight as f64).clamp(0.0, 1.0) * (detection.quality as f64).clamp(0.0, 1.0)).max(1e-6);
        normal_sum += normal * weight;
        weighted_normals.push((normal, weight));
    }
    if weighted_normals.len() < 2 {
        return;
    }
    if min_depth.is_finite() && max_depth.is_finite() {
        let depth_spread = (max_depth - min_depth).abs();
        if depth_spread > localization_multitag_normal_lock_max_depth_spread_m() {
            return;
        }
    }

    let Some(consensus_normal) = normal_sum.try_normalize(1e-12) else {
        return;
    };
    let max_spread_deg = localization_multitag_normal_lock_max_spread_deg();
    let strength = localization_multitag_normal_lock_strength();

    let mut max_spread_seen = 0.0f64;
    for (normal, _weight) in &weighted_normals {
        let dot = normal.normalize().dot(&consensus_normal).clamp(-1.0, 1.0);
        let spread = dot.acos().to_degrees().abs();
        max_spread_seen = max_spread_seen.max(spread);
    }
    if !max_spread_seen.is_finite() || max_spread_seen > max_spread_deg {
        return;
    }

    for detection in detections.iter_mut() {
        let normal = detection.camera_from_tag.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
        if !normal.iter().all(|value| value.is_finite()) {
            continue;
        }
        let dot = normal.normalize().dot(&consensus_normal).clamp(-1.0, 1.0);
        let spread_deg = dot.acos().to_degrees().abs();
        if spread_deg < 0.5 {
            continue;
        }
        let rel = (spread_deg / max_spread_deg).clamp(0.0, 1.0);
        let blend = (strength * (1.0 - rel).sqrt()).clamp(0.0, 1.0);
        if blend <= 1e-4 {
            continue;
        }
        if let Some(locked) = rotation_with_locked_normal(detection.camera_from_tag.rotation, consensus_normal) {
            detection.camera_from_tag.rotation = detection.camera_from_tag.rotation.slerp(&locked, blend);
        }
    }

    let same_code_rotation =
        detections.first().and_then(|detection| detection.code_rotation).is_some_and(|code_rotation| detections.iter().all(|detection| detection.code_rotation == Some(code_rotation)));

    if localization_multitag_coplanar_depth_lock_enabled() && same_code_rotation {
        let mut depth_samples: Vec<(usize, f64, f64)> = Vec::with_capacity(detections.len());
        let mut depth_weight_sum = 0.0f64;
        let mut depth_weighted_sum = 0.0f64;
        for (idx, detection) in detections.iter().enumerate() {
            let translation = detection.camera_from_tag.translation;
            if !translation.iter().all(|value| value.is_finite()) {
                continue;
            }
            let depth_along_normal = consensus_normal.dot(&translation);
            if !depth_along_normal.is_finite() {
                continue;
            }
            let weight = ((detection.weight as f64).clamp(0.0, 1.0) * (detection.quality as f64).clamp(0.0, 1.0)).max(1e-6);
            depth_samples.push((idx, depth_along_normal, weight));
            depth_weight_sum += weight;
            depth_weighted_sum += weight * depth_along_normal;
        }

        if depth_samples.len() >= 2 && depth_weight_sum > f64::EPSILON {
            let anchor_weight = depth_samples.iter().map(|(_idx, _depth, weight)| *weight).fold(0.0f64, f64::max);
            if anchor_weight < 0.08 {
                return;
            }

            let depth_mean = depth_weighted_sum / depth_weight_sum;
            let max_depth_err = depth_samples.iter().map(|(_idx, depth, _weight)| (depth - depth_mean).abs()).fold(0.0f64, f64::max);

            if max_depth_err > 0.01 {
                let strength = localization_multitag_coplanar_depth_lock_strength();
                let max_shift = localization_multitag_coplanar_depth_lock_max_shift_m();
                for (idx, depth, weight) in depth_samples {
                    let error = depth_mean - depth;
                    if !error.is_finite() {
                        continue;
                    }
                    let rel = (error.abs() / max_depth_err.max(1e-6)).clamp(0.0, 1.0);
                    let confidence = weight.sqrt().clamp(0.35, 1.0);
                    let blend = (strength * rel.sqrt() * confidence).clamp(0.0, 1.0);
                    if blend <= 1e-4 {
                        continue;
                    }
                    let delta = (error * blend).clamp(-max_shift, max_shift);
                    if delta.abs() <= 1e-6 {
                        continue;
                    }

                    let translation = detections[idx].camera_from_tag.translation;
                    let depth_now = consensus_normal.dot(&translation);
                    let tangent = translation - consensus_normal * depth_now;
                    detections[idx].camera_from_tag.translation = tangent + consensus_normal * (depth_now + delta);
                }
            }
        }
    }

    if !localization_multitag_full_lock_same_code_rot_enabled() || !same_code_rotation {
        return;
    }

    let ref_q = *detections[0].camera_from_tag.rotation.quaternion();
    let ref_v = Vector3::new(ref_q.i, ref_q.j, ref_q.k);
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_z = 0.0f64;
    let mut sum_w = 0.0f64;
    let mut total_w = 0.0f64;
    for detection in detections.iter() {
        let q = *detection.camera_from_tag.rotation.quaternion();
        let mut x = q.i;
        let mut y = q.j;
        let mut z = q.k;
        let mut w = q.w;
        let dot = x * ref_v.x + y * ref_v.y + z * ref_v.z + w * ref_q.w;
        if dot < 0.0 {
            x = -x;
            y = -y;
            z = -z;
            w = -w;
        }
        let weight = ((detection.weight as f64).clamp(0.0, 1.0) * (detection.quality as f64).clamp(0.0, 1.0)).max(1e-6);
        sum_x += weight * x;
        sum_y += weight * y;
        sum_z += weight * z;
        sum_w += weight * w;
        total_w += weight;
    }
    if total_w <= 0.0 {
        return;
    }
    let norm = (sum_x * sum_x + sum_y * sum_y + sum_z * sum_z + sum_w * sum_w).sqrt();
    if !norm.is_finite() || norm <= 1e-12 {
        return;
    }
    let consensus_q = UnitQuaternion::new_normalize(nalgebra::Quaternion::new(sum_w / norm, sum_x / norm, sum_y / norm, sum_z / norm));

    let max_spread = localization_multitag_full_lock_max_spread_deg();
    let mut worst_spread = 0.0f64;
    for detection in detections.iter() {
        let spread = detection.camera_from_tag.rotation.angle_to(&consensus_q).to_degrees().abs();
        if spread.is_finite() {
            worst_spread = worst_spread.max(spread);
        }
    }
    if !worst_spread.is_finite() || worst_spread > max_spread {
        return;
    }

    let strength = localization_multitag_full_lock_strength();
    for detection in detections.iter_mut() {
        let spread = detection.camera_from_tag.rotation.angle_to(&consensus_q).to_degrees().abs();
        if !spread.is_finite() || spread < 0.2 {
            continue;
        }
        let rel = (spread / max_spread).clamp(0.0, 1.0);
        let blend = (strength * (1.0 - rel).sqrt()).clamp(0.0, 1.0);
        if blend <= 1e-4 {
            continue;
        }
        detection.camera_from_tag.rotation = detection.camera_from_tag.rotation.slerp(&consensus_q, blend);
    }
}

pub(crate) fn translation_from_value(value: &serde_json::Value) -> Option<Translation3> {
    Some(Translation3 { x: value.get("x")?.as_f64()?, y: value.get("y")?.as_f64()?, z: value.get("z")?.as_f64()? })
}

pub(crate) fn translation_from_pose_value(value: &serde_json::Value) -> Option<Translation3> {
    if let Some(translation) = value.get("translation").or_else(|| value.get("position")) {
        return translation_from_value(translation);
    }
    let x = value.get("x").and_then(|value| value.as_f64())?;
    let y = value.get("y").and_then(|value| value.as_f64())?;
    let z = value.get("z").and_then(|value| value.as_f64())?;
    Some(Translation3 { x, y, z })
}

pub(crate) fn rotation_from_value(value: &serde_json::Value) -> (Option<LocalizationQuaternion>, Option<(f64, f64, f64)>) {
    let quat =
        value.get("quaternion").and_then(|quat| Some(LocalizationQuaternion { x: quat.get("x")?.as_f64()?, y: quat.get("y")?.as_f64()?, z: quat.get("z")?.as_f64()?, w: quat.get("w")?.as_f64()? }));

    let roll = value.get("roll").and_then(|value| value.as_f64());
    let pitch = value.get("pitch").and_then(|value| value.as_f64());
    let yaw = value.get("yaw").and_then(|value| value.as_f64());
    let euler = if roll.is_some() || pitch.is_some() || yaw.is_some() { Some((roll.unwrap_or(0.0), pitch.unwrap_or(0.0), yaw.unwrap_or(0.0))) } else { None };

    (quat, euler)
}

pub(crate) fn pose_from_translation_rotation(translation: Translation3, quaternion: Option<LocalizationQuaternion>, euler: Option<(f64, f64, f64)>) -> PoseTransform {
    pose_from_detection(translation, quaternion, euler)
}

fn has_imu_token(value: &str) -> bool {
    value.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|token| token.eq_ignore_ascii_case("imu"))
}

pub(crate) fn source_looks_like_imu(source: &LocalizationSourceConfig) -> bool {
    [source.id.as_str(), source.stream_id.as_str(), source.output_key.as_str(), source.camera_uid.as_str()].iter().any(|value| has_imu_token(value))
}

pub fn imu_backend_to_viewer_basis() -> UnitQuaternion<f64> {
    *IMU_BACKEND_TO_VIEWER_BASIS.get_or_init(|| {
        // Matches frontend imuQuaternionToThree() basis conversion:
        // backend IMU (+X forward, +Y right, +Z up) -> viewer (+X right, +Y up, +Z forward).
        UnitQuaternion::new_normalize(Quaternion::new(0.5, -0.5, -0.5, -0.5))
    })
}

pub fn imu_vec_to_viewer_frame(vector: Vector3<f64>) -> Vector3<f64> {
    imu_backend_to_viewer_basis().transform_vector(&vector)
}

pub(crate) fn imu_pose_to_viewer_frame(pose: PoseTransform) -> PoseTransform {
    let basis = imu_backend_to_viewer_basis();
    let translation = basis.transform_vector(&pose.translation);
    let rotation = basis * pose.rotation * basis.inverse();
    PoseTransform { translation, rotation }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::config::LocalizationSourceConfig;

    fn source_config() -> LocalizationSourceConfig {
        LocalizationSourceConfig {
            id: "cam-front".to_string(),
            stream_id: "stream-a".to_string(),
            output_key: "detections".to_string(),
            camera_uid: "cam-front".to_string(),
            pose_space: None,
            input_key: None,
            enabled: true,
            weight: 1.0,
        }
    }

    fn detection(tag_id: u32, quality: f32, x: f64, y: f64, z: f64) -> LocalizationDetection {
        LocalizationDetection {
            source_id: String::new(),
            camera_uid: String::new(),
            tag_id,
            camera_from_tag: PoseTransform { translation: Vector3::new(x, y, z), rotation: UnitQuaternion::identity() },
            tag_size: Some(0.165),
            code_rotation: Some(0),
            tag_bits: None,
            weight: 1.0,
            quality,
        }
    }

    #[test]
    fn collapse_duplicates_keeps_best_quality_detection() {
        let source = source_config();
        let mut detections = vec![detection(7, 0.25, 0.0, 0.0, 2.0), detection(7, 0.91, 0.12, 0.0, 2.1)];
        collapse_duplicate_tag_detections(&source, &mut detections);
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].tag_id, 7);
        assert!((detections[0].camera_from_tag.translation.x - 0.12).abs() < 1e-6);
    }

    #[test]
    fn collapse_duplicates_penalizes_ambiguous_far_apart_candidates() {
        let source = source_config();
        let mut detections = vec![detection(3, 0.90, 0.0, 0.0, 2.0), detection(3, 0.82, 1.45, 0.0, 2.0)];
        collapse_duplicate_tag_detections(&source, &mut detections);
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].tag_id, 3);
        assert!(detections[0].quality <= 0.5);
    }
}
