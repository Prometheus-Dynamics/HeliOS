use super::common::*;
use super::*;

#[derive(Clone, Debug, NodeConfig)]
pub(super) struct ArucoTemporalSmoothDetectionsConfig {
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
pub(super) fn cv_aruco_temporal_smooth_detections(
    frame: &GrayImage,
    detections: &Vec<ArucoDetection2D>,
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
    for det in detections.iter().cloned() {
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

    let mut state: TemporalState =
        exec_ctx.state.take_native::<TemporalState>(STATE_KEY).or_else(|_| exec_ctx.state.get_checked::<TemporalState>(STATE_KEY)).map_err(NodeError::Handler)?.unwrap_or_default();
    if state.frame_width != frame_w || state.frame_height != frame_h {
        state = TemporalState { frame_width: frame_w, frame_height: frame_h, ..Default::default() };
    }
    state.frame_idx = state.frame_idx.saturating_add(1);
    let frame_idx = state.frame_idx;
    let curr_sig = gray_signature_8x8(frame);
    let scene_cut = state.last_signature_8x8.as_ref().map(|prev| signature_diff_norm(prev, &curr_sig) > scene_cut_threshold).unwrap_or(false);
    if scene_cut {
        state.tracks.clear();
    }
    state.last_signature_8x8 = Some(curr_sig.to_vec());

    if corner_smooth_alpha > 0.0 {
        for det in current_by_id.values_mut() {
            let Some(prev) = state.tracks.get(&det.id) else { continue };
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

    let mut out: Vec<ArucoDetection2D> = current_by_id.into_values().collect();

    for det in &out {
        let center = quad_center(&det.corners);
        let velocity = if let Some(prev) = state.tracks.get(&det.id) {
            let dt = frame_idx.saturating_sub(prev.last_seen_frame).max(1) as f64;
            let obs_vx = (center.0 - prev.center.0) / dt;
            let obs_vy = (center.1 - prev.center.1) / dt;
            let blend = 0.45f64;
            (prev.velocity.0 * (1.0 - blend) + obs_vx * blend, prev.velocity.1 * (1.0 - blend) + obs_vy * blend)
        } else {
            (0.0, 0.0)
        };
        state.tracks.insert(det.id, TemporalTrack::from_detection(det, frame_idx, center, velocity));
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
    exec_ctx.state.set_native(STATE_KEY, state).map_err(NodeError::Handler)?;
    Ok(out)
}
