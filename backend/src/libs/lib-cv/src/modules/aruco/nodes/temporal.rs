use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTemporalStabilizeDetectionsConfig {
    // Keep a missing detection eligible for rescue for up to N frames.
    #[port(default = 1i64, meta(ui_min = 0, ui_max = 6, ui_step = 1))]
    hold_frames: i64,
    // Optional passthrough carry: when rescue fails, emit the last good detection for up to N
    // frames with motion-compensated corner translation. Disabled by default.
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 3000, ui_step = 1))]
    carry_passthrough_frames: i64,
    // Require geometric support from current-frame quads before passthrough carry can emit a tag.
    // This prevents stale corners from surviving complete occlusion.
    #[port(default = true)]
    carry_require_quad_support: bool,
    // Max allowed area ratio between carried track and supporting quad.
    #[port(default = 2.5f64, meta(ui_min = 1.0, ui_max = 10.0, ui_step = 0.1))]
    carry_quad_max_area_ratio: f64,
    // Max mean-corner shift (in units of previous min side) allowed for supporting quad.
    #[port(default = 1.2f64, meta(ui_min = 0.1, ui_max = 4.0, ui_step = 0.05))]
    carry_quad_max_corner_shift_ratio: f64,
    // Scale applied to predicted-center search radius for passthrough-support quad matching.
    #[port(default = 1.0f64, meta(ui_min = 0.5, ui_max = 4.0, ui_step = 0.1))]
    carry_quad_search_radius_scale: f64,
    // Backward-compatibility knob from the previous temporal implementation.
    // Current implementation does not blend active detections with historical corners.
    #[port(default = 1.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01))]
    current_corner_weight: f64,
    // Only attempt temporal rescue when at least one detection exists in the current frame.
    #[port(default = true)]
    carry_requires_current: bool,
    // Cache quality gates: only tracks with at most these decode errors are eligible for rescue.
    #[port(default = 1i64, meta(ui_min = 0, ui_max = 8, ui_step = 1))]
    max_carry_border_mismatches: i64,
    #[port(default = 2i64, meta(ui_min = 0, ui_max = 8, ui_step = 1))]
    max_carry_best_distance: i64,
    // Prevent unbounded state growth.
    #[port(default = 64i64, meta(ui_min = 4, ui_max = 512, ui_step = 1))]
    max_tracks: i64,
    // Max inferred global center motion (px/frame) used for rescue search prediction.
    #[port(default = 24.0f64, meta(ui_min = 0.0, ui_max = 128.0, ui_step = 1.0))]
    max_motion_px: f64,

    // Rescue pass controls.
    #[port(default = 48.0f64, meta(ui_min = 4.0, ui_max = 256.0, ui_step = 1.0))]
    rescue_center_dist_px: f64,
    // Multiplicative expansion applied to predicted rescue radius to tolerate motion/model error.
    #[port(default = 1.0f64, meta(ui_min = 1.0, ui_max = 8.0, ui_step = 0.1))]
    rescue_radius_scale: f64,
    #[port(default = 64i64, meta(ui_min = 4, ui_max = 512, ui_step = 1))]
    rescue_max_quads: i64,
    #[port(default = "4x4_1000")]
    dictionary: ArucoDictionaryKind,
    #[port(default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))]
    rescue_sample_scale: i64,
    #[port(default = 4i64, meta(ui_min = -1, ui_max = 8, ui_step = 1))]
    rescue_max_hamming: i64,
    #[port(default = 6i64, meta(ui_min = -1, ui_max = 50, ui_step = 1))]
    rescue_border_error_divisor: i64,
    #[port(default = 6i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))]
    rescue_min_warped_patch_contrast_range: i64,
    #[port(default = 6i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    rescue_warp_fallback_max_hamming_extra: i64,
    #[port(default = 12i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    rescue_warp_fallback_border_slack: i64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 200.0, ui_step = 0.5))]
    rescue_min_quad_side_px: f64,
    #[port(default = 6.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))]
    rescue_min_cell_means_contrast_range: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))]
    rescue_min_bit_delta: f64,
    // Optional corner jitter damping for tags seen in consecutive frames.
    // This never reuses stale corners outright; it only blends after strict geometric gating.
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 0.6, ui_step = 0.01))]
    corner_smooth_alpha: f64,
    #[port(default = 0.8f64, meta(ui_min = 0.05, ui_max = 2.0, ui_step = 0.01))]
    corner_smooth_max_corner_shift_ratio: f64,
    #[port(default = 40.0f64, meta(ui_min = 0.0, ui_max = 256.0, ui_step = 1.0))]
    corner_smooth_max_center_shift_px: f64,
    #[port(default = 2.0f64, meta(ui_min = 1.0, ui_max = 10.0, ui_step = 0.1))]
    corner_smooth_max_area_ratio: f64,
    #[port(default = true)]
    corner_smooth_only_age1: bool,
}

#[derive(Clone)]
struct TemporalTrack {
    det: ArucoDetection2D,
    last_seen_frame: u64,
    center: (f64, f64),
    velocity: (f64, f64),
}

#[derive(Clone, Serialize, Deserialize)]
struct TemporalTrackState {
    det: ArucoDetection2D,
    last_seen_frame: u64,
    center: [f64; 2],
    velocity: [f64; 2],
}

impl From<&TemporalTrack> for TemporalTrackState {
    fn from(value: &TemporalTrack) -> Self {
        Self { det: value.det.clone(), last_seen_frame: value.last_seen_frame, center: [value.center.0, value.center.1], velocity: [value.velocity.0, value.velocity.1] }
    }
}

impl From<TemporalTrackState> for TemporalTrack {
    fn from(value: TemporalTrackState) -> Self {
        Self { det: value.det, last_seen_frame: value.last_seen_frame, center: (value.center[0], value.center[1]), velocity: (value.velocity[0], value.velocity[1]) }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct TemporalState {
    frame_idx: u64,
    tracks: HashMap<u32, TemporalTrackState>,
    frame_width: u32,
    frame_height: u32,
    last_signature_8x8: Option<Vec<u8>>,
}

fn frame_signature_8x8(frame: &DynamicImage) -> [u8; 64] {
    let (w, h) = frame.dimensions();
    let mut sig = [0u8; 64];
    let w = w.max(1);
    let h = h.max(1);
    for gy in 0..8u32 {
        for gx in 0..8u32 {
            let x = (((gx * 2 + 1) * w) / 16).min(w - 1);
            let y = (((gy * 2 + 1) * h) / 16).min(h - 1);
            let px = frame.get_pixel(x, y).0;
            let lum = ((u16::from(px[0]) * 77 + u16::from(px[1]) * 150 + u16::from(px[2]) * 29) >> 8) as u8;
            sig[(gy * 8 + gx) as usize] = lum;
        }
    }
    sig
}

fn signature_diff_norm(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != 64 || b.len() != 64 {
        return 1.0;
    }
    let mut acc = 0.0f64;
    for i in 0..64usize {
        acc += (f64::from(a[i]) - f64::from(b[i])).abs();
    }
    acc / (64.0 * 255.0)
}

fn quad_center(corners: &[Point; 4]) -> (f64, f64) {
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    for p in corners {
        x += p.x;
        y += p.y;
    }
    (x * 0.25, y * 0.25)
}

fn quad_area(corners: &[Point; 4]) -> f64 {
    let mut area = 0.0f64;
    for i in 0..4usize {
        let j = (i + 1) & 3;
        area += corners[i].x * corners[j].y - corners[j].x * corners[i].y;
    }
    0.5 * area.abs()
}

fn quad_min_side(corners: &[Point; 4]) -> f64 {
    let mut min_side_sq = f64::MAX;
    for i in 0..4usize {
        let a = corners[i];
        let b = corners[(i + 1) & 3];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        min_side_sq = min_side_sq.min(dx * dx + dy * dy);
    }
    min_side_sq.sqrt()
}

fn edges_cross(corners: &[Point; 4]) -> bool {
    #[inline(always)]
    fn orient(a: &Point, b: &Point, c: &Point) -> f64 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }
    let c = corners;
    let o1 = orient(&c[0], &c[1], &c[2]);
    let o2 = orient(&c[0], &c[1], &c[3]);
    let o3 = orient(&c[2], &c[3], &c[0]);
    let o4 = orient(&c[2], &c[3], &c[1]);
    if (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0) {
        return true;
    }
    let o5 = orient(&c[1], &c[2], &c[3]);
    let o6 = orient(&c[1], &c[2], &c[0]);
    let o7 = orient(&c[3], &c[0], &c[1]);
    let o8 = orient(&c[3], &c[0], &c[2]);
    (o5 > 0.0) != (o6 > 0.0) && (o7 > 0.0) != (o8 > 0.0)
}

fn quad_convex(corners: &[Point; 4]) -> bool {
    let mut sign = 0.0f64;
    for i in 0..4usize {
        let a = corners[i];
        let b = corners[(i + 1) & 3];
        let c = corners[(i + 2) & 3];
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() <= f64::EPSILON {
            return false;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if sign * cross < 0.0 {
            return false;
        }
    }
    true
}

fn quad_valid_in_frame(corners: &[Point; 4], width: u32, height: u32) -> bool {
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let margin = 6.0f64;
    let mut min_edge_sq = f64::MAX;
    for i in 0..4usize {
        let a = corners[i];
        let b = corners[(i + 1) & 3];
        if !a.x.is_finite() || !a.y.is_finite() {
            return false;
        }
        if a.x < -margin || a.y < -margin || a.x > (w - 1.0 + margin) || a.y > (h - 1.0 + margin) {
            return false;
        }
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        min_edge_sq = min_edge_sq.min(dx * dx + dy * dy);
    }
    if min_edge_sq < 4.0 {
        return false;
    }
    if quad_area(corners) < 4.0 {
        return false;
    }
    if edges_cross(corners) {
        return false;
    }
    quad_convex(corners)
}

fn rotated_corners(corners: &[Point; 4], shift: usize) -> [Point; 4] {
    [corners[shift & 3], corners[(shift + 1) & 3], corners[(shift + 2) & 3], corners[(shift + 3) & 3]]
}

fn best_corner_alignment_shift(prev: &[Point; 4], curr: &[Point; 4]) -> usize {
    let mut best_shift = 0usize;
    let mut best_score = f64::INFINITY;
    for shift in 0..4usize {
        let cand = rotated_corners(curr, shift);
        let mut score = 0.0f64;
        for i in 0..4usize {
            let dx = cand[i].x - prev[i].x;
            let dy = cand[i].y - prev[i].y;
            score += dx * dx + dy * dy;
        }
        if score < best_score {
            best_score = score;
            best_shift = shift;
        }
    }
    best_shift
}

fn smooth_corners(prev: &[Point; 4], curr: &[Point; 4], alpha: f64) -> [Point; 4] {
    let keep = (1.0 - alpha).clamp(0.0, 1.0);
    let hist = alpha.clamp(0.0, 1.0);
    let mut out = *curr;
    for i in 0..4usize {
        out[i].x = curr[i].x * keep + prev[i].x * hist;
        out[i].y = curr[i].y * keep + prev[i].y * hist;
    }
    out
}

fn shifted_corners(corners: &[Point; 4], dx: f64, dy: f64) -> [Point; 4] {
    let mut out = *corners;
    for p in &mut out {
        p.x += dx;
        p.y += dy;
    }
    out
}

fn mean_corner_distance(a: &[Point; 4], b: &[Point; 4]) -> f64 {
    let mut acc = 0.0f64;
    for i in 0..4usize {
        let dx = a[i].x - b[i].x;
        let dy = a[i].y - b[i].y;
        acc += (dx * dx + dy * dy).sqrt();
    }
    acc * 0.25
}

fn carry_quality_ok(det: &ArucoDetection2D, cfg: &ArucoTemporalStabilizeDetectionsConfig) -> bool {
    let max_bm = usize::try_from(cfg.max_carry_border_mismatches.max(0)).unwrap_or(0);
    let max_best = u32::try_from(cfg.max_carry_best_distance.max(0)).unwrap_or(0);
    if let Some(bm) = det.border_mismatches
        && bm > max_bm
    {
        return false;
    }
    if let Some(best) = det.best_distance
        && best > max_best
    {
        return false;
    }
    true
}

fn better_detection(a: &ArucoDetection2D, b: &ArucoDetection2D) -> bool {
    let a_best = a.best_distance.unwrap_or(u32::MAX);
    let b_best = b.best_distance.unwrap_or(u32::MAX);
    if a_best != b_best {
        return a_best < b_best;
    }

    let a_bm = a.border_mismatches.unwrap_or(usize::MAX);
    let b_bm = b.border_mismatches.unwrap_or(usize::MAX);
    if a_bm != b_bm {
        return a_bm < b_bm;
    }

    let a_score = a.score.unwrap_or(-1.0);
    let b_score = b.score.unwrap_or(-1.0);
    if a_score.total_cmp(&b_score).is_ne() {
        return a_score.total_cmp(&b_score).is_gt();
    }

    quad_area(&a.corners) > quad_area(&b.corners)
}

fn decode_relaxed_candidates(frame: &DynamicImage, quads: &[[CvPoint<f32>; 4]], cfg: &ArucoTemporalStabilizeDetectionsConfig) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let sample_scale = u32::try_from(cfg.rescue_sample_scale.max(1)).unwrap_or(1).max(1);

    if let Some(dict_name) = cfg.dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };

        let mut decode_cfg = ArucoDecodeConfig {
            min_warped_patch_contrast_range: u8::try_from(cfg.rescue_min_warped_patch_contrast_range.max(0)).unwrap_or(6),
            min_cell_means_contrast_range: cfg.rescue_min_cell_means_contrast_range.max(0.0) as f32,
            min_quad_side_px: cfg.rescue_min_quad_side_px.max(0.0) as f32,
            min_bit_delta: cfg.rescue_min_bit_delta.max(0.0) as f32,
            ..ArucoDecodeConfig::default()
        };

        if cfg.rescue_max_hamming >= 0 {
            let max_hamming = u32::try_from(cfg.rescue_max_hamming).unwrap_or(0);
            let max_corr = dict.max_correction_bits() as u32;
            decode_cfg.error_correction_rate = if max_corr == 0 { 0.0 } else { (max_hamming as f32 / max_corr as f32).clamp(0.0, 1.0) };
        }

        if cfg.rescue_border_error_divisor > 0 {
            let div = (cfg.rescue_border_error_divisor as f32).max(1.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min((1.0 / div).clamp(0.0, 1.0));
            if cfg.rescue_border_error_divisor >= 32 {
                decode_cfg.max_border_error_rate = 0.0;
            }
        }

        return Ok(decode_quads_aruco_with_config_no_bits(frame, quads, sample_scale, &dict, &decode_cfg));
    }

    let Some(family_kind) = cfg.dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    };

    let mut family = family_kind.into_family();
    if cfg.rescue_max_hamming >= 0 {
        family = family.with_max_hamming(u8::try_from(cfg.rescue_max_hamming).unwrap_or(0));
    }
    if cfg.rescue_border_error_divisor > 0 {
        family = family.with_border_error_divisor(u8::try_from(cfg.rescue_border_error_divisor).unwrap_or(6));
    }

    let decode_cfg = ArucoTagDecodeConfig {
        min_warped_patch_contrast_range: u8::try_from(cfg.rescue_min_warped_patch_contrast_range.max(0)).unwrap_or(6),
        warp_fallback_on_decode_fail: true,
        warp_fallback_on_low_contrast: true,
        warp_fallback_max_hamming_extra: u32::try_from(cfg.rescue_warp_fallback_max_hamming_extra.max(0)).unwrap_or(6),
        warp_fallback_border_slack: usize::try_from(cfg.rescue_warp_fallback_border_slack.max(0)).unwrap_or(12),
        warp_min_sample_scale: sample_scale.max(1),
        min_quad_side_px: cfg.rescue_min_quad_side_px.max(0.0) as f32,
        min_decode_score: -1.0,
        cell_decode: crate::modules::aruco::tag::ArucoTagDecodeTuning {
            min_cell_means_contrast_range: cfg.rescue_min_cell_means_contrast_range.max(0.0) as f32,
            min_hamming_margin: 0,
            min_hamming_margin_min_dist: 0,
            min_hamming_margin_only_if_border_mismatch: false,
            min_bit_delta: cfg.rescue_min_bit_delta.max(0.0) as f32,
        },
        ..ArucoTagDecodeConfig::default()
    };

    Ok(decode_quads_with_config_no_bits(frame, quads, sample_scale, &family, &decode_cfg))
}

#[node(
    id = "temporal_stabilize_detections",
    summary = "Rescue one-frame ArUco dropouts using current-frame quads + looser decode.",
    inputs(
        "frame",
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        config = ArucoTemporalStabilizeDetectionsConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_temporal_stabilize_detections(
    frame: &DynamicImage,
    detections: Vec<ArucoDetection2D>,
    quads: Vec<Quad>,
    cfg: ArucoTemporalStabilizeDetectionsConfig,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    const STATE_KEY: &str = "cv:aruco:temporal_stabilize_detections:state";
    let _ = cfg.current_corner_weight; // kept only for backwards-compatible graphs/UI.
    let hold_frames = u64::try_from(cfg.hold_frames.max(0)).unwrap_or(0);
    let carry_passthrough_frames = u64::try_from(cfg.carry_passthrough_frames.max(0)).unwrap_or(0);
    let max_tracks = usize::try_from(cfg.max_tracks.clamp(4, 512)).unwrap_or(64);
    let rescue_radius_scale = if cfg.rescue_radius_scale.is_finite() { cfg.rescue_radius_scale.clamp(1.0, 8.0) } else { 1.0 };
    let rescue_radius_scale_sq = rescue_radius_scale * rescue_radius_scale;
    let (frame_w, frame_h) = frame.dimensions();
    let corner_smooth_alpha = if cfg.corner_smooth_alpha.is_finite() { cfg.corner_smooth_alpha.clamp(0.0, 0.6) } else { 0.0 };
    let corner_smooth_ratio = if cfg.corner_smooth_max_corner_shift_ratio.is_finite() { cfg.corner_smooth_max_corner_shift_ratio.clamp(0.05, 2.0) } else { 0.8 };
    let corner_smooth_center_px = if cfg.corner_smooth_max_center_shift_px.is_finite() { cfg.corner_smooth_max_center_shift_px.max(0.0) } else { 40.0 };
    let corner_smooth_max_area_ratio = if cfg.corner_smooth_max_area_ratio.is_finite() { cfg.corner_smooth_max_area_ratio.clamp(1.0, 10.0) } else { 2.0 };
    let rescue_gate_max_area_ratio = if cfg.carry_quad_max_area_ratio.is_finite() { cfg.carry_quad_max_area_ratio.clamp(1.0, 10.0) } else { 2.5 };
    let rescue_gate_max_corner_shift_ratio = if cfg.carry_quad_max_corner_shift_ratio.is_finite() { cfg.carry_quad_max_corner_shift_ratio.clamp(0.1, 4.0) } else { 1.2 };

    // Keep at most one detection per id; prefer larger area.
    let mut current_by_id: HashMap<u32, ArucoDetection2D> = HashMap::with_capacity(detections.len());
    for det in detections {
        match current_by_id.get(&det.id) {
            Some(existing) => {
                if quad_area(&det.corners) > quad_area(&existing.corners) {
                    current_by_id.insert(det.id, det);
                }
            }
            None => {
                current_by_id.insert(det.id, det);
            }
        }
    }

    // Fast-path: no temporal rescue/carry/smoothing requested.
    // Keep one detection per id and skip all stateful temporal work.
    if hold_frames == 0 && carry_passthrough_frames == 0 && corner_smooth_alpha <= f64::EPSILON {
        let mut out: Vec<ArucoDetection2D> = current_by_id.into_values().collect();
        out.sort_by_key(|d| d.id);
        return Ok(out);
    }

    let mut state: TemporalState = exec_ctx.state.get_checked::<TemporalState>(STATE_KEY).map_err(NodeError::Handler)?.unwrap_or_default();
    if state.frame_width != frame_w || state.frame_height != frame_h {
        state = TemporalState { frame_width: frame_w, frame_height: frame_h, ..Default::default() };
    }
    state.frame_idx = state.frame_idx.saturating_add(1);
    let frame_idx = state.frame_idx;
    let curr_sig = frame_signature_8x8(frame);
    let scene_cut = state.last_signature_8x8.as_ref().map(|prev| signature_diff_norm(prev, &curr_sig) > 0.12).unwrap_or(false);
    if scene_cut {
        state.tracks.clear();
    }
    state.last_signature_8x8 = Some(curr_sig.to_vec());

    // Optional corner jitter damping against previous-frame corners (same id only).
    if corner_smooth_alpha > 0.0 {
        for det in current_by_id.values_mut() {
            let Some(prev_state) = state.tracks.get(&det.id) else { continue };
            let prev: TemporalTrack = prev_state.clone().into();
            let age = frame_idx.saturating_sub(prev.last_seen_frame);
            if cfg.corner_smooth_only_age1 && age != 1 {
                continue;
            }
            if age == 0 {
                continue;
            }
            if !quad_valid_in_frame(&prev.det.corners, frame_w, frame_h) || !quad_valid_in_frame(&det.corners, frame_w, frame_h) {
                continue;
            }

            let prev_area = quad_area(&prev.det.corners);
            let curr_area = quad_area(&det.corners);
            if prev_area <= 0.0 || curr_area <= 0.0 {
                continue;
            }
            let area_ratio = (prev_area / curr_area).max(curr_area / prev_area);
            if area_ratio > corner_smooth_max_area_ratio {
                continue;
            }

            let shift = best_corner_alignment_shift(&prev.det.corners, &det.corners);
            let aligned = rotated_corners(&det.corners, shift);
            let mut mean_corner_dist = 0.0f64;
            for (aligned_corner, prev_corner) in aligned.iter().zip(prev.det.corners.iter()) {
                let dx = aligned_corner.x - prev_corner.x;
                let dy = aligned_corner.y - prev_corner.y;
                mean_corner_dist += (dx * dx + dy * dy).sqrt();
            }
            mean_corner_dist *= 0.25;

            let prev_min_side = quad_min_side(&prev.det.corners).max(1.0);
            if mean_corner_dist > corner_smooth_ratio * prev_min_side {
                continue;
            }

            let (pcx, pcy) = quad_center(&prev.det.corners);
            let (ccx, ccy) = quad_center(&aligned);
            let cdx = ccx - pcx;
            let cdy = ccy - pcy;
            if (cdx * cdx + cdy * cdy).sqrt() > corner_smooth_center_px {
                continue;
            }

            let smoothed = smooth_corners(&prev.det.corners, &aligned, corner_smooth_alpha);
            if !quad_valid_in_frame(&smoothed, frame_w, frame_h) {
                continue;
            }

            det.corners = smoothed;
            det.rotation = det.rotation.wrapping_add((shift & 3) as u8) & 3;
        }
    }

    // Infer coarse global motion from ids present in both current and previous frame.
    let mut sum_dx = 0.0f64;
    let mut sum_dy = 0.0f64;
    let mut shared = 0usize;
    for (id, det) in &current_by_id {
        if let Some(track_state) = state.tracks.get(id) {
            let track: TemporalTrack = track_state.clone().into();
            let (cx, cy) = quad_center(&det.corners);
            let (px, py) = quad_center(&track.det.corners);
            sum_dx += cx - px;
            sum_dy += cy - py;
            shared += 1;
        }
    }
    let (mut global_dx, mut global_dy) = if shared > 0 { (sum_dx / shared as f64, sum_dy / shared as f64) } else { (0.0, 0.0) };
    let motion_norm = (global_dx * global_dx + global_dy * global_dy).sqrt();
    let max_motion_px = cfg.max_motion_px.max(0.0);
    if max_motion_px > 0.0 && motion_norm > max_motion_px && motion_norm.is_finite() {
        let scale = max_motion_px / motion_norm;
        global_dx *= scale;
        global_dy *= scale;
    }

    let mut out: Vec<ArucoDetection2D> = current_by_id.values().cloned().collect();
    // IDs emitted only by passthrough carry this frame. These must never refresh temporal state,
    // otherwise stale detections can self-sustain indefinitely.
    let mut carried_ids: HashSet<u32> = HashSet::new();

    let carry_possible = !cfg.carry_requires_current || !out.is_empty();
    let mut missing_ids: HashSet<u32> = HashSet::new();
    // Per-id prediction tuple: (pred_x, pred_y, allowed_radius_sq).
    let mut predicted_centers: HashMap<u32, (f64, f64, f64)> = HashMap::new();

    if hold_frames > 0 && carry_possible {
        for (id, track_state) in &state.tracks {
            let track: TemporalTrack = track_state.clone().into();
            if current_by_id.contains_key(id) {
                // Rule 1: if regular detection already has this id this frame, temporal pass must ignore it.
                continue;
            }
            let age = frame_idx.saturating_sub(track.last_seen_frame);
            if age == 0 || age > hold_frames {
                continue;
            }
            if !carry_quality_ok(&track.det, &cfg) {
                continue;
            }
            let (cx, cy) = track.center;
            let age_f = age as f64;
            // Blend per-track velocity with coarse global motion so camera motion doesn't collapse rescue.
            let pred_x = cx + (track.velocity.0 + global_dx) * age_f;
            let pred_y = cy + (track.velocity.1 + global_dy) * age_f;
            let base_radius = cfg.rescue_center_dist_px.max(0.0);
            let speed = (track.velocity.0 * track.velocity.0 + track.velocity.1 * track.velocity.1).sqrt();
            let mut radius = base_radius + age_f * 6.0 + speed * 0.75;
            let max_radius = (base_radius * 3.0 + 24.0).max(base_radius);
            if radius > max_radius {
                radius = max_radius;
            }
            missing_ids.insert(*id);
            predicted_centers.insert(*id, (pred_x, pred_y, radius * radius));
        }
    }

    if !missing_ids.is_empty() && !quads.is_empty() {
        let min_quad_side = cfg.rescue_min_quad_side_px.max(0.0);
        let rescue_cap = usize::try_from(cfg.rescue_max_quads.clamp(4, 512)).unwrap_or(64);

        // (quad, normalized_distance, area)
        let mut candidates: Vec<(Quad, f64, f64)> = Vec::with_capacity(quads.len());

        for quad in &quads {
            if min_quad_side > 0.0 && quad_min_side(quad) < min_quad_side {
                continue;
            }
            if quad_area(quad) < 4.0 {
                continue;
            }
            let (qx, qy) = quad_center(quad);
            let mut best_norm_dist = f64::INFINITY;
            for (px, py, allowed_dist_sq) in predicted_centers.values() {
                let allowed_dist_sq = *allowed_dist_sq * rescue_radius_scale_sq;
                if allowed_dist_sq <= 0.0 {
                    continue;
                }
                let dx = qx - px;
                let dy = qy - py;
                let dist_sq = dx * dx + dy * dy;
                if dist_sq <= allowed_dist_sq {
                    let norm = dist_sq / allowed_dist_sq;
                    if norm < best_norm_dist {
                        best_norm_dist = norm;
                    }
                }
            }
            if best_norm_dist.is_finite() {
                candidates.push((*quad, best_norm_dist, quad_area(quad)));
            }
        }

        if !candidates.is_empty() {
            // Prioritize closest-to-prediction quads; area is only a tiebreaker.
            candidates.sort_by(|a, b| match a.1.total_cmp(&b.1) {
                std::cmp::Ordering::Equal => b.2.total_cmp(&a.2),
                ord => ord,
            });
            if candidates.len() > rescue_cap {
                candidates.truncate(rescue_cap);
            }

            let mut cv_quads: Vec<[CvPoint<f32>; 4]> = Vec::with_capacity(candidates.len());
            for (quad, _, _) in &candidates {
                cv_quads.push([
                    CvPoint::new(quad[0].x as f32, quad[0].y as f32),
                    CvPoint::new(quad[1].x as f32, quad[1].y as f32),
                    CvPoint::new(quad[2].x as f32, quad[2].y as f32),
                    CvPoint::new(quad[3].x as f32, quad[3].y as f32),
                ]);
            }

            let decoded = decode_relaxed_candidates(frame, &cv_quads, &cfg)?;

            let mut best_for_id: HashMap<u32, ArucoDetection2D> = HashMap::new();
            for mut det in decoded {
                if !missing_ids.contains(&det.id) {
                    continue;
                }
                if !carry_quality_ok(&det, &cfg) {
                    continue;
                }
                let Some(track_state) = state.tracks.get(&det.id) else {
                    continue;
                };
                let track: TemporalTrack = track_state.clone().into();
                if !quad_valid_in_frame(&track.det.corners, frame_w, frame_h) {
                    continue;
                }
                if !quad_valid_in_frame(&det.corners, frame_w, frame_h) {
                    continue;
                }

                let prev_area = quad_area(&track.det.corners).max(1.0);
                let det_area = quad_area(&det.corners).max(1.0);
                let area_ratio = (det_area / prev_area).max(prev_area / det_area);
                if area_ratio > rescue_gate_max_area_ratio {
                    continue;
                }

                let prev_min_side = quad_min_side(&track.det.corners).max(1.0);
                let shift = best_corner_alignment_shift(&track.det.corners, &det.corners);
                let aligned = rotated_corners(&det.corners, shift);
                if mean_corner_distance(&track.det.corners, &aligned) > rescue_gate_max_corner_shift_ratio * prev_min_side {
                    continue;
                }

                let (dx, dy) = quad_center(&det.corners);
                let Some((px, py, allowed_dist_sq)) = predicted_centers.get(&det.id) else {
                    continue;
                };
                let allowed_dist_sq = *allowed_dist_sq * rescue_radius_scale_sq;
                let ddx = dx - px;
                let ddy = dy - py;
                if ddx * ddx + ddy * ddy > allowed_dist_sq {
                    continue;
                }
                det.corners = aligned;
                det.rotation = det.rotation.wrapping_add((shift & 3) as u8) & 3;

                match best_for_id.get(&det.id) {
                    Some(prev) => {
                        if better_detection(&det, prev) {
                            best_for_id.insert(det.id, det);
                        }
                    }
                    None => {
                        best_for_id.insert(det.id, det);
                    }
                }
            }

            out.extend(best_for_id.into_values());
        }
    }

    let carry_passthrough_possible = !cfg.carry_requires_current || !current_by_id.is_empty();
    if carry_passthrough_frames > 0 && carry_passthrough_possible {
        let present: HashSet<u32> = out.iter().map(|d| d.id).collect();
        let carry_quad_max_area_ratio = if cfg.carry_quad_max_area_ratio.is_finite() { cfg.carry_quad_max_area_ratio.clamp(1.0, 10.0) } else { 2.5 };
        let carry_quad_max_corner_shift_ratio = if cfg.carry_quad_max_corner_shift_ratio.is_finite() { cfg.carry_quad_max_corner_shift_ratio.clamp(0.1, 4.0) } else { 1.2 };
        let carry_quad_search_radius_scale = if cfg.carry_quad_search_radius_scale.is_finite() { cfg.carry_quad_search_radius_scale.clamp(0.5, 4.0) } else { 1.0 };
        for (id, track_state) in &state.tracks {
            if present.contains(id) || current_by_id.contains_key(id) {
                continue;
            }
            let track: TemporalTrack = track_state.clone().into();
            let age = frame_idx.saturating_sub(track.last_seen_frame);
            if age == 0 || age > carry_passthrough_frames {
                continue;
            }
            if !carry_quality_ok(&track.det, &cfg) {
                continue;
            }

            let age_f = age as f64;
            let shift_x = (track.velocity.0 + global_dx) * age_f;
            let shift_y = (track.velocity.1 + global_dy) * age_f;
            let predicted = shifted_corners(&track.det.corners, shift_x, shift_y);
            let (pred_x, pred_y) = quad_center(&predicted);

            let mut support_corners: Option<[Point; 4]> = None;
            if cfg.carry_require_quad_support {
                if quads.is_empty() {
                    continue;
                }
                let prev_area = quad_area(&track.det.corners).max(1.0);
                let prev_min_side = quad_min_side(&track.det.corners).max(1.0);
                let base_radius = cfg.rescue_center_dist_px.max(0.0).max(8.0);
                let speed = (track.velocity.0 * track.velocity.0 + track.velocity.1 * track.velocity.1).sqrt();
                let mut radius = base_radius + age_f * 6.0 + speed * 0.75;
                let max_radius = (base_radius * 3.0 + 24.0).max(base_radius);
                if radius > max_radius {
                    radius = max_radius;
                }
                radius *= carry_quad_search_radius_scale * rescue_radius_scale.max(1.0);
                let radius_sq = radius * radius;

                let mut best_score = f64::INFINITY;
                for quad in &quads {
                    if !quad_valid_in_frame(quad, frame_w, frame_h) {
                        continue;
                    }
                    let qa = quad_area(quad).max(1.0);
                    let area_ratio = (qa / prev_area).max(prev_area / qa);
                    if area_ratio > carry_quad_max_area_ratio {
                        continue;
                    }
                    let (qx, qy) = quad_center(quad);
                    let dx = qx - pred_x;
                    let dy = qy - pred_y;
                    let center_dist_sq = dx * dx + dy * dy;
                    if center_dist_sq > radius_sq {
                        continue;
                    }

                    let shift = best_corner_alignment_shift(&predicted, quad);
                    let aligned = rotated_corners(quad, shift);
                    let mean_corner = mean_corner_distance(&predicted, &aligned);
                    if mean_corner > carry_quad_max_corner_shift_ratio * prev_min_side {
                        continue;
                    }

                    let score = (center_dist_sq / radius_sq) + (mean_corner / prev_min_side);
                    if score < best_score {
                        best_score = score;
                        support_corners = Some(aligned);
                    }
                }

                let Some(supported) = support_corners else {
                    continue;
                };
                support_corners = Some(supported);
            }

            let mut det = track.det.clone();
            det.corners = support_corners.unwrap_or(predicted);
            if !quad_valid_in_frame(&det.corners, frame_w, frame_h) {
                continue;
            }
            carried_ids.insert(det.id);
            out.push(det);
        }
    }

    // Update temporal tracks only from current-frame detections (regular + rescued), never stale corners.
    for det in &out {
        if carried_ids.contains(&det.id) {
            continue;
        }
        let center = quad_center(&det.corners);
        let velocity = if let Some(prev_state) = state.tracks.get(&det.id) {
            let prev: TemporalTrack = prev_state.clone().into();
            let dt = frame_idx.saturating_sub(prev.last_seen_frame).max(1) as f64;
            let obs_vx = (center.0 - prev.center.0) / dt;
            let obs_vy = (center.1 - prev.center.1) / dt;
            let blend = 0.45f64;
            (prev.velocity.0 * (1.0 - blend) + obs_vx * blend, prev.velocity.1 * (1.0 - blend) + obs_vy * blend)
        } else {
            (0.0, 0.0)
        };
        state.tracks.insert(det.id, TemporalTrackState::from(&TemporalTrack { det: det.clone(), last_seen_frame: frame_idx, center, velocity }));
    }

    // Prune stale tracks and cap map size.
    state.tracks.retain(|_, track| frame_idx.saturating_sub(track.last_seen_frame) <= hold_frames.saturating_add(1));
    if state.tracks.len() > max_tracks {
        let mut keys: Vec<(u32, u64)> = state.tracks.iter().map(|(id, t)| (*id, t.last_seen_frame)).collect();
        keys.sort_by_key(|(_, seen)| *seen);
        let drop_n = state.tracks.len() - max_tracks;
        for (id, _) in keys.into_iter().take(drop_n) {
            state.tracks.remove(&id);
        }
    }

    out.sort_by_key(|d| d.id);
    exec_ctx.state.set_typed(STATE_KEY, &state).map_err(NodeError::Handler)?;
    Ok(out)
}

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTemporalSmoothDetectionsConfig {
    // Prevent unbounded state growth.
    #[port(default = 96i64, meta(ui_min = 4, ui_max = 512, ui_step = 1))]
    max_tracks: i64,
    // Number of frames to keep stale tracks for smoothing reference.
    #[port(default = 2i64, meta(ui_min = 1, ui_max = 16, ui_step = 1))]
    max_track_age_frames: i64,
    // Scene-cut threshold based on compact 8x8 frame signature diff.
    #[port(default = 0.12f64, meta(ui_min = 0.02, ui_max = 0.5, ui_step = 0.01))]
    scene_cut_threshold: f64,
    // Blend factor from previous corners -> current corners.
    #[port(default = 0.18f64, meta(ui_min = 0.0, ui_max = 0.6, ui_step = 0.01))]
    corner_smooth_alpha: f64,
    // Max allowed mean-corner shift (in units of previous min side) for smoothing eligibility.
    #[port(default = 0.55f64, meta(ui_min = 0.05, ui_max = 2.0, ui_step = 0.01))]
    corner_smooth_max_corner_shift_ratio: f64,
    // Max allowed center shift in pixels for smoothing eligibility.
    #[port(default = 18.0f64, meta(ui_min = 0.0, ui_max = 256.0, ui_step = 1.0))]
    corner_smooth_max_center_shift_px: f64,
    // Max area ratio between previous and current quad for smoothing eligibility.
    #[port(default = 1.45f64, meta(ui_min = 1.0, ui_max = 10.0, ui_step = 0.05))]
    corner_smooth_max_area_ratio: f64,
    // Restrict smoothing to immediate consecutive frames only.
    #[port(default = true)]
    corner_smooth_only_age1: bool,
}

#[node(
    id = "temporal_smooth_detections",
    summary = "Temporal corner smoothing for ArUco detections (no rescue/passthrough).",
    inputs(
        "frame",
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        config = ArucoTemporalSmoothDetectionsConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_temporal_smooth_detections(
    frame: &DynamicImage,
    detections: Vec<ArucoDetection2D>,
    cfg: ArucoTemporalSmoothDetectionsConfig,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    const STATE_KEY: &str = "cv:aruco:temporal_smooth_detections:state";

    let max_tracks = usize::try_from(cfg.max_tracks.clamp(4, 512)).unwrap_or(96);
    let max_track_age_frames = u64::try_from(cfg.max_track_age_frames.clamp(1, 16)).unwrap_or(2);
    let corner_smooth_alpha = if cfg.corner_smooth_alpha.is_finite() { cfg.corner_smooth_alpha.clamp(0.0, 0.6) } else { 0.18 };
    let corner_smooth_ratio = if cfg.corner_smooth_max_corner_shift_ratio.is_finite() { cfg.corner_smooth_max_corner_shift_ratio.clamp(0.05, 2.0) } else { 0.55 };
    let corner_smooth_center_px = if cfg.corner_smooth_max_center_shift_px.is_finite() { cfg.corner_smooth_max_center_shift_px.max(0.0) } else { 18.0 };
    let corner_smooth_max_area_ratio = if cfg.corner_smooth_max_area_ratio.is_finite() { cfg.corner_smooth_max_area_ratio.clamp(1.0, 10.0) } else { 1.45 };
    let scene_cut_threshold = if cfg.scene_cut_threshold.is_finite() { cfg.scene_cut_threshold.clamp(0.02, 0.5) } else { 0.12 };
    let (frame_w, frame_h) = frame.dimensions();

    // Keep at most one detection per id; prefer larger area.
    let mut current_by_id: HashMap<u32, ArucoDetection2D> = HashMap::with_capacity(detections.len());
    for det in detections {
        match current_by_id.get(&det.id) {
            Some(existing) => {
                if quad_area(&det.corners) > quad_area(&existing.corners) {
                    current_by_id.insert(det.id, det);
                }
            }
            None => {
                current_by_id.insert(det.id, det);
            }
        }
    }

    let mut state: TemporalState = exec_ctx.state.get_checked::<TemporalState>(STATE_KEY).map_err(NodeError::Handler)?.unwrap_or_default();
    if state.frame_width != frame_w || state.frame_height != frame_h {
        state = TemporalState { frame_width: frame_w, frame_height: frame_h, ..Default::default() };
    }
    state.frame_idx = state.frame_idx.saturating_add(1);
    let frame_idx = state.frame_idx;
    let curr_sig = frame_signature_8x8(frame);
    let scene_cut = state.last_signature_8x8.as_ref().map(|prev| signature_diff_norm(prev, &curr_sig) > scene_cut_threshold).unwrap_or(false);
    if scene_cut {
        state.tracks.clear();
    }
    state.last_signature_8x8 = Some(curr_sig.to_vec());

    if corner_smooth_alpha > 0.0 {
        for det in current_by_id.values_mut() {
            let Some(prev_state) = state.tracks.get(&det.id) else { continue };
            let prev: TemporalTrack = prev_state.clone().into();
            let age = frame_idx.saturating_sub(prev.last_seen_frame);
            if cfg.corner_smooth_only_age1 && age != 1 {
                continue;
            }
            if age == 0 || age > max_track_age_frames {
                continue;
            }
            if !quad_valid_in_frame(&prev.det.corners, frame_w, frame_h) || !quad_valid_in_frame(&det.corners, frame_w, frame_h) {
                continue;
            }

            let prev_area = quad_area(&prev.det.corners);
            let curr_area = quad_area(&det.corners);
            if prev_area <= 0.0 || curr_area <= 0.0 {
                continue;
            }
            let area_ratio = (prev_area / curr_area).max(curr_area / prev_area);
            if area_ratio > corner_smooth_max_area_ratio {
                continue;
            }

            let shift = best_corner_alignment_shift(&prev.det.corners, &det.corners);
            let aligned = rotated_corners(&det.corners, shift);
            let mut mean_corner_dist = 0.0f64;
            for (aligned_corner, prev_corner) in aligned.iter().zip(prev.det.corners.iter()) {
                let dx = aligned_corner.x - prev_corner.x;
                let dy = aligned_corner.y - prev_corner.y;
                mean_corner_dist += (dx * dx + dy * dy).sqrt();
            }
            mean_corner_dist *= 0.25;

            let prev_min_side = quad_min_side(&prev.det.corners).max(1.0);
            if mean_corner_dist > corner_smooth_ratio * prev_min_side {
                continue;
            }

            let (pcx, pcy) = quad_center(&prev.det.corners);
            let (ccx, ccy) = quad_center(&aligned);
            let cdx = ccx - pcx;
            let cdy = ccy - pcy;
            if (cdx * cdx + cdy * cdy).sqrt() > corner_smooth_center_px {
                continue;
            }

            let smoothed = smooth_corners(&prev.det.corners, &aligned, corner_smooth_alpha);
            if !quad_valid_in_frame(&smoothed, frame_w, frame_h) {
                continue;
            }

            det.corners = smoothed;
            det.rotation = det.rotation.wrapping_add((shift & 3) as u8) & 3;
        }
    }

    let mut out: Vec<ArucoDetection2D> = current_by_id.values().cloned().collect();

    for det in &out {
        let center = quad_center(&det.corners);
        let velocity = if let Some(prev_state) = state.tracks.get(&det.id) {
            let prev: TemporalTrack = prev_state.clone().into();
            let dt = frame_idx.saturating_sub(prev.last_seen_frame).max(1) as f64;
            let obs_vx = (center.0 - prev.center.0) / dt;
            let obs_vy = (center.1 - prev.center.1) / dt;
            let blend = 0.45f64;
            (prev.velocity.0 * (1.0 - blend) + obs_vx * blend, prev.velocity.1 * (1.0 - blend) + obs_vy * blend)
        } else {
            (0.0, 0.0)
        };
        state.tracks.insert(det.id, TemporalTrackState::from(&TemporalTrack { det: det.clone(), last_seen_frame: frame_idx, center, velocity }));
    }

    state.tracks.retain(|_, track| frame_idx.saturating_sub(track.last_seen_frame) <= max_track_age_frames);
    if state.tracks.len() > max_tracks {
        let mut keys: Vec<(u32, u64)> = state.tracks.iter().map(|(id, t)| (*id, t.last_seen_frame)).collect();
        keys.sort_by_key(|(_, seen)| *seen);
        let drop_n = state.tracks.len() - max_tracks;
        for (id, _) in keys.into_iter().take(drop_n) {
            state.tracks.remove(&id);
        }
    }

    out.sort_by_key(|d| d.id);
    exec_ctx.state.set_typed(STATE_KEY, &state).map_err(NodeError::Handler)?;
    Ok(out)
}
