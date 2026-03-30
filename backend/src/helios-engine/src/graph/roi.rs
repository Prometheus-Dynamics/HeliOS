use daedalus::data::model::Value as DaedalusValue;
use lib_cv::modules::aruco::ArucoDetection2D;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RoiRect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

#[derive(Debug, Default)]
pub(super) struct AutoTargetRoiState {
    pub rect: Option<RoiRect>,
    pub last_center: Option<(f64, f64)>,
    pub velocity: (f64, f64),
    pub misses: u32,
    pub ever_detected: bool,
}

pub(super) fn host_output_detection_source_port(host_output_ports: &[String]) -> Option<String> {
    host_output_ports.iter().find(|port| port.eq_ignore_ascii_case("target_detections")).cloned().or_else(|| host_output_ports.iter().find(|port| port.eq_ignore_ascii_case("detections")).cloned())
}

pub(super) fn daedalus_value_as_i64(value: &DaedalusValue) -> Option<i64> {
    match value {
        DaedalusValue::Int(value) => Some(*value),
        DaedalusValue::Float(value) if value.is_finite() => Some(*value as i64),
        _ => None,
    }
}

pub(super) fn daedalus_value_as_bool(value: &DaedalusValue) -> Option<bool> {
    match value {
        DaedalusValue::Bool(value) => Some(*value),
        DaedalusValue::Int(value) => Some(*value != 0),
        DaedalusValue::Float(value) if value.is_finite() => Some(*value != 0.0),
        _ => None,
    }
}

pub(super) fn is_roi_port(port: &str) -> bool {
    matches!(port.trim().to_ascii_lowercase().as_str(), "roi_x" | "roi_y" | "roi_w" | "roi_h")
}

pub(super) fn manual_roi_override_active(inputs: &BTreeMap<String, DaedalusValue>) -> bool {
    let w = inputs.get("roi_w").and_then(daedalus_value_as_i64).unwrap_or(0);
    let h = inputs.get("roi_h").and_then(daedalus_value_as_i64).unwrap_or(0);
    w > 0 && h > 0
}

fn detection_bbox(det: &ArucoDetection2D) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for corner in &det.corners {
        if !(corner.x.is_finite() && corner.y.is_finite()) {
            return None;
        }
        min_x = min_x.min(corner.x);
        min_y = min_y.min(corner.y);
        max_x = max_x.max(corner.x);
        max_y = max_y.max(corner.y);
    }
    if !(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()) {
        return None;
    }
    Some((min_x, min_y, max_x, max_y))
}

fn clamp_roi_rect(frame_dims: (u32, u32), cx: f64, cy: f64, w: f64, h: f64) -> Option<RoiRect> {
    let (frame_w, frame_h) = frame_dims;
    if frame_w == 0 || frame_h == 0 || !(cx.is_finite() && cy.is_finite() && w.is_finite() && h.is_finite()) {
        return None;
    }

    let fw = frame_w as f64;
    let fh = frame_h as f64;
    let roi_w = w.max(1.0).min(fw).round();
    let roi_h = h.max(1.0).min(fh).round();
    if roi_w <= 0.0 || roi_h <= 0.0 {
        return None;
    }

    let max_x = (fw - roi_w).max(0.0);
    let max_y = (fh - roi_h).max(0.0);
    let x = (cx - roi_w * 0.5).clamp(0.0, max_x).round();
    let y = (cy - roi_h * 0.5).clamp(0.0, max_y).round();
    Some(RoiRect { x: x as i64, y: y as i64, w: roi_w as i64, h: roi_h as i64 })
}

fn reset_auto_target_roi_tracking(state: &mut AutoTargetRoiState) {
    state.rect = None;
    state.last_center = None;
    state.velocity = (0.0, 0.0);
    state.misses = 0;
}

pub(super) fn bootstrap_auto_target_roi_rect(frame_dims: (u32, u32), inputs: &BTreeMap<String, DaedalusValue>) -> Option<RoiRect> {
    const DEFAULT_FRAME_RATIO: f64 = 0.60;
    const DEFAULT_MAX_RATIO: f64 = 0.72;
    const CROSSHAIR_FRAME_RATIO: f64 = 0.42;
    const CROSSHAIR_MAX_RATIO: f64 = 0.55;
    const MIN_SIZE_PX: f64 = 224.0;

    let (frame_w, frame_h) = frame_dims;
    if frame_w == 0 || frame_h == 0 {
        return None;
    }

    let crosshair_x = inputs.get("crosshair_x").and_then(daedalus_value_as_i64);
    let crosshair_y = inputs.get("crosshair_y").and_then(daedalus_value_as_i64);
    let draw_crosshair = inputs.get("draw_crosshair").and_then(daedalus_value_as_bool).unwrap_or(false);
    let order_uses_crosshair = matches!(inputs.get("order_mode"), Some(DaedalusValue::String(mode)) if mode.eq_ignore_ascii_case("crosshair"));
    let explicit_crosshair = draw_crosshair || order_uses_crosshair || crosshair_x.zip(crosshair_y).is_some_and(|(x, y)| x > 0 || y > 0);

    let (center_x, center_y, frame_ratio, max_ratio) = if explicit_crosshair {
        let max_x = i64::from(frame_w.saturating_sub(1));
        let max_y = i64::from(frame_h.saturating_sub(1));
        let x = crosshair_x.unwrap_or(max_x / 2).clamp(0, max_x) as f64;
        let y = crosshair_y.unwrap_or(max_y / 2).clamp(0, max_y) as f64;
        (x, y, CROSSHAIR_FRAME_RATIO, CROSSHAIR_MAX_RATIO)
    } else {
        (frame_w as f64 * 0.5, frame_h as f64 * 0.5, DEFAULT_FRAME_RATIO, DEFAULT_MAX_RATIO)
    };

    let max_w = (frame_w as f64 * max_ratio).max(1.0).min(frame_w as f64);
    let max_h = (frame_h as f64 * max_ratio).max(1.0).min(frame_h as f64);
    let roi_w = (frame_w as f64 * frame_ratio).max(MIN_SIZE_PX.min(max_w)).min(max_w);
    let roi_h = (frame_h as f64 * frame_ratio).max(MIN_SIZE_PX.min(max_h)).min(max_h);
    clamp_roi_rect(frame_dims, center_x, center_y, roi_w, roi_h)
}

pub(super) fn update_auto_target_roi_state(state: &mut AutoTargetRoiState, frame_dims: (u32, u32), detections: &[ArucoDetection2D]) -> Option<RoiRect> {
    const HOLD_FRAMES: u32 = 3;
    const EXPAND_RATIO: f64 = 2.25;
    const MIN_FRAME_RATIO: f64 = 0.18;
    const MIN_SIZE_PX: f64 = 96.0;
    const MAX_SIZE_RATIO: f64 = 0.65;
    const MISS_GROWTH: f64 = 1.35;
    const BASE_PAD_PX: f64 = 24.0;

    let (frame_w, frame_h) = frame_dims;
    if frame_w == 0 || frame_h == 0 {
        *state = AutoTargetRoiState::default();
        return None;
    }

    let frame_min = frame_w.min(frame_h) as f64;
    let min_roi_size = (frame_min * MIN_FRAME_RATIO).max(MIN_SIZE_PX);
    let max_roi_w = (frame_w as f64 * MAX_SIZE_RATIO).max(min_roi_size).min(frame_w as f64);
    let max_roi_h = (frame_h as f64 * MAX_SIZE_RATIO).max(min_roi_size).min(frame_h as f64);

    if let Some(det) = detections.first() {
        let Some((min_x, min_y, max_x, max_y)) = detection_bbox(det) else {
            return state.rect;
        };
        let bbox_w = (max_x - min_x).max(1.0);
        let bbox_h = (max_y - min_y).max(1.0);
        let center = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
        let velocity = state.last_center.map(|prev| (center.0 - prev.0, center.1 - prev.1)).unwrap_or((0.0, 0.0));
        let velocity_pad = (velocity.0.abs().max(velocity.1.abs()) * 2.0).min(frame_min * 0.1);
        let roi_w = (bbox_w * EXPAND_RATIO + BASE_PAD_PX + velocity_pad).clamp(min_roi_size, max_roi_w);
        let roi_h = (bbox_h * EXPAND_RATIO + BASE_PAD_PX + velocity_pad).clamp(min_roi_size, max_roi_h);
        let rect = clamp_roi_rect(frame_dims, center.0, center.1, roi_w, roi_h)?;
        state.rect = Some(rect);
        state.last_center = Some(center);
        state.velocity = velocity;
        state.misses = 0;
        state.ever_detected = true;
        return Some(rect);
    }

    let Some(prev_rect) = state.rect else {
        reset_auto_target_roi_tracking(state);
        return None;
    };

    if state.misses >= HOLD_FRAMES {
        reset_auto_target_roi_tracking(state);
        return None;
    }

    state.misses = state.misses.saturating_add(1);
    let growth = MISS_GROWTH.powi(state.misses as i32);
    let base_center = state.last_center.unwrap_or((prev_rect.x as f64 + prev_rect.w as f64 * 0.5, prev_rect.y as f64 + prev_rect.h as f64 * 0.5));
    let predicted_center = (base_center.0 + state.velocity.0, base_center.1 + state.velocity.1);
    let roi_w = (prev_rect.w as f64 * growth).clamp(min_roi_size, max_roi_w);
    let roi_h = (prev_rect.h as f64 * growth).clamp(min_roi_size, max_roi_h);
    let rect = clamp_roi_rect(frame_dims, predicted_center.0, predicted_center.1, roi_w, roi_h)?;
    state.rect = Some(rect);
    state.last_center = Some(predicted_center);
    Some(rect)
}
