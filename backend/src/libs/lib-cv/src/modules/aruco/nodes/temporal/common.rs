use super::stabilize::ArucoTemporalStabilizeDetectionsConfig;
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct TemporalDetection {
    pub(super) id: u32,
    pub(super) rotation: u8,
    pub(super) corners: [Point; 4],
    pub(super) score: Option<f32>,
    pub(super) best_distance: Option<u32>,
    pub(super) second_distance: Option<u32>,
    pub(super) border_mismatches: Option<usize>,
    pub(super) contrast_range: Option<f32>,
    pub(super) border_width: Option<u8>,
    pub(super) data_width: Option<u8>,
}

impl TemporalDetection {
    pub(super) fn from_detection(det: &ArucoDetection2D) -> Self {
        Self {
            id: det.id,
            rotation: det.rotation,
            corners: det.corners,
            score: det.score,
            best_distance: det.best_distance,
            second_distance: det.second_distance,
            border_mismatches: det.border_mismatches,
            contrast_range: det.contrast_range,
            border_width: det.border_width,
            data_width: det.data_width,
        }
    }

    pub(super) fn to_detection(&self) -> ArucoDetection2D {
        ArucoDetection2D {
            id: self.id,
            rotation: self.rotation,
            corners: self.corners,
            score: self.score,
            best_distance: self.best_distance,
            second_distance: self.second_distance,
            border_mismatches: self.border_mismatches,
            contrast_range: self.contrast_range,
            border_width: self.border_width,
            data_width: self.data_width,
            bits: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct TemporalTrack {
    pub(super) det: TemporalDetection,
    pub(super) last_seen_frame: u64,
    pub(super) center: (f64, f64),
    pub(super) velocity: (f64, f64),
}

impl TemporalTrack {
    pub(super) fn from_detection(det: &ArucoDetection2D, last_seen_frame: u64, center: (f64, f64), velocity: (f64, f64)) -> Self {
        Self { det: TemporalDetection::from_detection(det), last_seen_frame, center, velocity }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct TemporalState {
    pub(super) frame_idx: u64,
    pub(super) tracks: HashMap<u32, TemporalTrack>,
    pub(super) frame_width: u32,
    pub(super) frame_height: u32,
    pub(super) last_signature_8x8: Option<Vec<u8>>,
}

pub(super) fn frame_signature_8x8(frame: &DynamicImage) -> [u8; 64] {
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

pub(super) fn gray_signature_8x8(frame: &GrayImage) -> [u8; 64] {
    let (w, h) = frame.dimensions();
    let mut sig = [0u8; 64];
    let w = w.max(1);
    let h = h.max(1);
    for gy in 0..8u32 {
        for gx in 0..8u32 {
            let x = (((gx * 2 + 1) * w) / 16).min(w - 1);
            let y = (((gy * 2 + 1) * h) / 16).min(h - 1);
            sig[(gy * 8 + gx) as usize] = frame.get_pixel(x, y).0[0];
        }
    }
    sig
}

pub(super) fn signature_diff_norm(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != 64 || b.len() != 64 {
        return 1.0;
    }
    let mut acc = 0.0f64;
    for i in 0..64usize {
        acc += (f64::from(a[i]) - f64::from(b[i])).abs();
    }
    acc / (64.0 * 255.0)
}

pub(super) fn quad_center(corners: &[Point; 4]) -> (f64, f64) {
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    for p in corners {
        x += p.x;
        y += p.y;
    }
    (x * 0.25, y * 0.25)
}

pub(super) fn quad_area(corners: &[Point; 4]) -> f64 {
    let mut area = 0.0f64;
    for i in 0..4usize {
        let j = (i + 1) & 3;
        area += corners[i].x * corners[j].y - corners[j].x * corners[i].y;
    }
    0.5 * area.abs()
}

pub(super) fn quad_min_side(corners: &[Point; 4]) -> f64 {
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

pub(super) fn quad_valid_in_frame(corners: &[Point; 4], width: u32, height: u32) -> bool {
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

pub(super) fn rotated_corners(corners: &[Point; 4], shift: usize) -> [Point; 4] {
    [corners[shift & 3], corners[(shift + 1) & 3], corners[(shift + 2) & 3], corners[(shift + 3) & 3]]
}

pub(super) fn best_corner_alignment_shift(prev: &[Point; 4], curr: &[Point; 4]) -> usize {
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

pub(super) fn smooth_corners(prev: &[Point; 4], curr: &[Point; 4], alpha: f64) -> [Point; 4] {
    let keep = (1.0 - alpha).clamp(0.0, 1.0);
    let hist = alpha.clamp(0.0, 1.0);
    let mut out = *curr;
    for i in 0..4usize {
        out[i].x = curr[i].x * keep + prev[i].x * hist;
        out[i].y = curr[i].y * keep + prev[i].y * hist;
    }
    out
}

pub(super) fn shifted_corners(corners: &[Point; 4], dx: f64, dy: f64) -> [Point; 4] {
    let mut out = *corners;
    for p in &mut out {
        p.x += dx;
        p.y += dy;
    }
    out
}

pub(super) fn mean_corner_distance(a: &[Point; 4], b: &[Point; 4]) -> f64 {
    let mut acc = 0.0f64;
    for i in 0..4usize {
        let dx = a[i].x - b[i].x;
        let dy = a[i].y - b[i].y;
        acc += (dx * dx + dy * dy).sqrt();
    }
    acc * 0.25
}

pub(super) fn carry_quality_ok(border_mismatches: Option<usize>, best_distance: Option<u32>, cfg: &ArucoTemporalStabilizeDetectionsConfig) -> bool {
    let max_bm = usize::try_from(cfg.max_carry_border_mismatches.max(0)).unwrap_or(0);
    let max_best = u32::try_from(cfg.max_carry_best_distance.max(0)).unwrap_or(0);
    if let Some(bm) = border_mismatches
        && bm > max_bm
    {
        return false;
    }
    if let Some(best) = best_distance
        && best > max_best
    {
        return false;
    }
    true
}

pub(super) fn better_detection(a: &ArucoDetection2D, b: &ArucoDetection2D) -> bool {
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
