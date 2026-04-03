use super::*;

mod geometry;
mod output;

pub(super) use geometry::*;
pub(super) use output::*;

pub(super) fn normalize_port_name(port: Option<&str>) -> Option<String> {
    let trimmed = port.map(str::trim).filter(|value| !value.is_empty())?;
    Some(trimmed.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoardParity {
    EvenSquares,
    OddSquares,
}

impl BoardParity {
    fn is_marker_square(self, x: u32, y: u32) -> bool {
        let even = (x + y).is_multiple_of(2);
        match self {
            BoardParity::EvenSquares => even,
            BoardParity::OddSquares => !even,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoardOrdering {
    RowMajor,
    ColMajor,
}

#[derive(Debug)]
pub(super) struct BoardLayout {
    pub(super) tag_corners_by_id: HashMap<u32, [Point; 4]>,
    pub(super) square_ij_by_id: HashMap<u32, (u32, u32)>,
    pub(super) squares_x: u32,
    pub(super) squares_y: u32,
    pub(super) square_size: f64,
}

impl BoardLayout {
    pub(super) fn new(board: &CalibrationBoard, parity: BoardParity, ordering: BoardOrdering) -> Self {
        let mut tag_corners_by_id = HashMap::new();
        let mut square_ij_by_id = HashMap::new();
        let mut id = 0u32;
        let off = ((board.square_size - board.marker_size) * 0.5).max(0.0);
        match ordering {
            BoardOrdering::RowMajor => {
                for j in 0..board.squares_y {
                    for i in 0..board.squares_x {
                        if !parity.is_marker_square(i, j) {
                            continue;
                        }
                        // Marker corners (not square corners): the decoder returns the marker border quad.
                        let sx0 = (i as f64) * board.square_size;
                        let sy0 = (j as f64) * board.square_size;
                        let mx0 = sx0 + off;
                        let my0 = sy0 + off;
                        let mx1 = mx0 + board.marker_size;
                        let my1 = my0 + board.marker_size;
                        tag_corners_by_id.insert(id, [Point { x: mx0, y: my0 }, Point { x: mx1, y: my0 }, Point { x: mx1, y: my1 }, Point { x: mx0, y: my1 }]);
                        square_ij_by_id.insert(id, (i, j));
                        id = id.wrapping_add(1);
                    }
                }
            }
            BoardOrdering::ColMajor => {
                for i in 0..board.squares_x {
                    for j in 0..board.squares_y {
                        if !parity.is_marker_square(i, j) {
                            continue;
                        }
                        let sx0 = (i as f64) * board.square_size;
                        let sy0 = (j as f64) * board.square_size;
                        let mx0 = sx0 + off;
                        let my0 = sy0 + off;
                        let mx1 = mx0 + board.marker_size;
                        let my1 = my0 + board.marker_size;
                        tag_corners_by_id.insert(id, [Point { x: mx0, y: my0 }, Point { x: mx1, y: my0 }, Point { x: mx1, y: my1 }, Point { x: mx0, y: my1 }]);
                        square_ij_by_id.insert(id, (i, j));
                        id = id.wrapping_add(1);
                    }
                }
            }
        }
        Self { tag_corners_by_id, square_ij_by_id, squares_x: board.squares_x, squares_y: board.squares_y, square_size: board.square_size }
    }
}

#[derive(Debug)]
pub(super) struct BuildViewResult {
    pub(super) view: CalibrationView,
    pub(super) marker_view: CalibrationView,
    pub(super) center_view: CalibrationView,
    pub(super) stats: DetectionStats,
    pub(super) alignment_error_sum: f64,
    pub(super) alignment_error_count: usize,
    pub(super) charuco_used: bool,
    pub(super) charuco_reason: Option<String>,
}

#[derive(Debug)]
pub(super) struct ImageCandidate {
    pub(super) image: String,
    pub(super) overlay_path: Option<String>,
    pub(super) row_even: BuildViewResult,
    pub(super) row_odd: BuildViewResult,
    pub(super) col_even: BuildViewResult,
    pub(super) col_odd: BuildViewResult,
}

#[derive(Debug)]
pub(super) struct ImageDetections {
    pub(super) image: String,
    pub(super) overlay_path: Option<String>,
    pub(super) detections: Vec<ArucoDetection2D>,
    pub(super) gray: GrayImage,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Debug)]
pub(super) enum DebugEntry {
    Skipped(CalibrationSolveDebugView),
    PendingImage(ImageDetections),
    Pending(Box<ImageCandidate>),
}

#[derive(Debug, Default)]
pub(super) struct LayoutScore {
    pub(super) usable_views: usize,
    pub(super) alignment_error_sum: f64,
    pub(super) alignment_error_count: usize,
}

impl LayoutScore {
    pub(super) fn update(&mut self, result: &BuildViewResult, min_points: usize) {
        if result.marker_view.len() >= min_points {
            self.usable_views = self.usable_views.saturating_add(1);
        }
        if result.alignment_error_count > 0 {
            self.alignment_error_sum += result.alignment_error_sum;
            self.alignment_error_count = self.alignment_error_count.saturating_add(result.alignment_error_count);
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct TagAreaStats {
    pub(super) median: Option<f64>,
    pub(super) min_area: f64,
    pub(super) count: usize,
}

#[derive(Debug, Default)]
pub(super) struct DetectionStats {
    pub(super) tags_detected: u32,
    pub(super) points_detected: u32,
    pub(super) coverage_ratio: f64,
    pub(super) raw_tags_detected: u32,
    pub(super) raw_ids: Vec<u32>,
    pub(super) raw_duplicate_ids: Vec<u32>,
    pub(super) raw_out_of_range_ids: Vec<u32>,
}

pub(super) struct BuildViewParams<'a> {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) expected_ids: &'a HashSet<u32>,
    pub(super) gray: Option<&'a GrayImage>,
    pub(super) min_points: usize,
    pub(super) min_tag_area: f64,
    pub(super) allow_charuco: bool,
    pub(super) lens_model: lib_cv::modules::calibration::LensModel,
}

pub(super) fn build_view(layout: &BoardLayout, detections: &[ArucoDetection2D], params: BuildViewParams<'_>) -> BuildViewResult {
    let BuildViewParams { width, height, expected_ids, gray, min_points, min_tag_area, allow_charuco, lens_model } = params;
    let is_fisheye = lens_model == lib_cv::modules::calibration::LensModel::Fisheye;
    let mut stats = DetectionStats { raw_tags_detected: detections.len().min(u32::MAX as usize) as u32, raw_ids: detections.iter().map(|d| d.id).collect(), ..Default::default() };
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for id in &stats.raw_ids {
        *counts.entry(*id).or_insert(0) += 1;
    }
    stats.raw_duplicate_ids = counts.iter().filter_map(|(id, count)| if *count > 1 { Some(*id) } else { None }).collect();
    stats.raw_duplicate_ids.sort_unstable();
    stats.raw_out_of_range_ids = stats.raw_ids.iter().copied().filter(|id| !expected_ids.contains(id)).collect();
    stats.raw_out_of_range_ids.sort_unstable();
    stats.raw_out_of_range_ids.dedup();

    let mut best_by_id: BTreeMap<u32, (f64, u8, [Point; 4])> = BTreeMap::new();
    for det in detections {
        let Some(obj) = layout.tag_corners_by_id.get(&det.id) else {
            continue;
        };
        let area = quad_area_px2(&det.corners);
        if !area.is_finite() || area <= 0.0 {
            continue;
        }
        if min_tag_area > 0.0 && area < min_tag_area {
            continue;
        }
        match best_by_id.get(&det.id) {
            Some((best_area, _, _)) if *best_area >= area => {}
            _ => {
                let _ = obj;
                best_by_id.insert(det.id, (area, det.rotation, det.corners));
            }
        }
    }

    // Build center pairs from the best detection per ID.
    let mut center_pairs: Vec<(u32, Point2, Point2)> = Vec::new();
    for (id, (_, _, quad)) in &best_by_id {
        let Some(obj) = layout.tag_corners_by_id.get(id) else { continue };
        let obj_center = quad_center(obj);
        let img_center = quad_center(quad);
        center_pairs.push((*id, (obj_center.x, obj_center.y), (img_center.x, img_center.y)));
    }

    // Estimate a robust homography from marker centers and use it to reject outlier detections
    // (common failure mode: black/white squares are detected as tags and decode to a valid ID).
    let mut inlier_by_index: Vec<bool> = Vec::new();
    let mut center_obj: Vec<Point2> = Vec::new();
    let mut center_img: Vec<Point2> = Vec::new();
    let mut center_homography: Option<Matrix3<f64>> = None;
    if center_pairs.len() >= 4 {
        let pair_list: Vec<(Point2, Point2)> = center_pairs.iter().map(|(_, o, i)| (*o, *i)).collect();
        let min_thr = if is_fisheye { 24.0 } else { 6.0 };
        if let Some((h, mask, threshold)) = robust_homography_pairs(&pair_list, min_thr) {
            center_homography = Some(h);
            inlier_by_index = mask;

            // Tighten inlier set a bit: robust_homography_pairs uses an adaptive threshold; we also
            // reject any pairs that exceed 2x the computed threshold.
            if let Some(h) = &center_homography {
                let thr = if is_fisheye {
                    // Fisheye distortion can make a planar homography a poor fit at the edges;
                    // use this only to reject obvious outliers (false-positive tags).
                    (threshold.max(min_thr) * 3.0).min(160.0)
                } else {
                    (threshold.max(6.0) * 2.0).min(48.0)
                };
                for (idx, (_, obj, img)) in center_pairs.iter().enumerate() {
                    if !inlier_by_index.get(idx).copied().unwrap_or(false) {
                        continue;
                    }
                    if let Some((u, v)) = project_homography(h, *obj) {
                        let dx = u - img.0;
                        let dy = v - img.1;
                        let err = (dx * dx + dy * dy).sqrt();
                        if !err.is_finite() || err > thr {
                            inlier_by_index[idx] = false;
                        }
                    } else {
                        inlier_by_index[idx] = false;
                    }
                }
            }
        } else {
            // Fallback: attempt a plain homography. No inlier mask in this case.
            let obj_pts: Vec<Point2> = center_pairs.iter().map(|(_, o, _)| *o).collect();
            let img_pts: Vec<Point2> = center_pairs.iter().map(|(_, _, i)| *i).collect();
            center_homography = solve_homography(&obj_pts, &img_pts);
        }
    }
    if inlier_by_index.is_empty() {
        inlier_by_index = vec![true; center_pairs.len()];
    }
    for (idx, (_, obj, img)) in center_pairs.iter().enumerate() {
        if inlier_by_index.get(idx).copied().unwrap_or(false) {
            center_obj.push(*obj);
            center_img.push(*img);
        }
    }

    // Drop outlier IDs from best_by_id based on the center-homography inlier mask.
    if center_pairs.len() == inlier_by_index.len() && !center_pairs.is_empty() {
        let mut keep: HashSet<u32> = HashSet::new();
        for (idx, (id, _o, _i)) in center_pairs.iter().enumerate() {
            if inlier_by_index[idx] {
                keep.insert(*id);
            }
        }
        // A planar homography is a decent outlier rejector for pinhole lenses, but it is not a
        // reliable discriminator under strong fisheye distortion. For fisheye, keep all decoded
        // detections and rely on strict decode knobs + downstream calibration residuals.
        if !is_fisheye {
            best_by_id.retain(|id, _| keep.contains(id));
        }
    }

    let mut tag_count = 0usize;
    let mut points = Vec::new();
    let mut center_pairs: Vec<CalibrationPointPair> = Vec::new();
    let mut marker_obj_points: Vec<Point2> = Vec::new();
    let mut marker_img_points: Vec<Point2> = Vec::new();
    let mut covered_area = 0.0;
    let mut alignment_error_sum = 0.0;
    let mut alignment_error_count = 0usize;

    #[derive(Default, Clone, Copy)]
    struct CornerAccum {
        sum_u: f64,
        sum_v: f64,
        sum_w: f64,
        count: u32,
    }
    let square = layout.square_size;
    let mut charuco_accum: HashMap<(i32, i32), CornerAccum> = HashMap::new();

    #[derive(Clone, Copy)]
    struct CharucoMarkerSample {
        obj_marker: [Point; 4],
        img_marker: [Point; 4],
        square_ij: (u32, u32),
        area_px2: f64,
    }

    let mut charuco_marker_samples: Vec<CharucoMarkerSample> = Vec::new();

    for (id, (area, rot, quad)) in best_by_id {
        let obj = match layout.tag_corners_by_id.get(&id) {
            Some(obj) => obj,
            None => continue,
        };
        let obj = *obj;
        tag_count += 1;
        covered_area += area;
        // Trust the decoder's rotation for corner ordering. Using a global board homography to
        // pick the best cyclic shift is fragile under heavy fisheye distortion and can
        // permute corners inconsistently across detections, which destroys calibration.
        let ordered = order_marker_corners(rot, quad);
        if let Some(h) = &center_homography {
            let obj_center = quad_center(&obj);
            let img_center = quad_center(&ordered);
            if let Some((u, v)) = project_homography(h, (obj_center.x, obj_center.y)) {
                let dx = u - img_center.x;
                let dy = v - img_center.y;
                let err = dx * dx + dy * dy;
                if err.is_finite() {
                    alignment_error_sum += err;
                    alignment_error_count = alignment_error_count.saturating_add(1);
                }
            }
        }
        for k in 0..4 {
            points.push(CalibrationPointPair::new(obj[k], ordered[k]));
            marker_obj_points.push((obj[k].x, obj[k].y));
            marker_img_points.push((ordered[k].x, ordered[k].y));
        }
        if allow_charuco && square > 0.0 {
            if let Some(&(si, sj)) = layout.square_ij_by_id.get(&id) {
                // Capture per-marker correspondences so we can estimate local homographies.
                // This tracks OpenCV's behavior better than a single global homography on fisheye lenses.
                charuco_marker_samples.push(CharucoMarkerSample { obj_marker: obj, img_marker: ordered, square_ij: (si, sj), area_px2: area });
            }
        }
        let obj_center = quad_center(&obj);
        let img_center = quad_center(&quad);
        center_obj.push((obj_center.x, obj_center.y));
        center_img.push((img_center.x, img_center.y));
        center_pairs.push(CalibrationPointPair::new(obj_center, img_center));
    }

    stats.tags_detected = tag_count.min(u32::MAX as usize) as u32;
    let denom = (width as f64) * (height as f64);
    stats.coverage_ratio = if denom > 0.0 { (covered_area / denom).clamp(0.0, 1.0) } else { 0.0 };

    let marker_view = CalibrationView::new(points);
    let center_view = CalibrationView::new(center_pairs);
    let mut charuco_used = false;
    let mut charuco_reason = None;
    let mut charuco_pairs: Vec<CalibrationPointPair> = Vec::new();
    if !allow_charuco {
        charuco_reason = Some("charuco: disabled for small-tag mode".into());
    } else if tag_count < 2 {
        charuco_reason = Some(format!("charuco: insufficient markers ({tag_count} < 2)"));
    } else if square <= 0.0 {
        charuco_reason = Some("charuco: invalid square size".into());
    } else {
        // Approximate ChArUco chessboard corners by projecting the *square* intersections through
        // per-marker homographies and averaging any repeated estimates. This is much more robust
        // than a single global homography on fisheye lenses, and matches OpenCV's interpolation
        // behavior more closely than using marker corners directly.
        let max_ix = layout.squares_x as i32;
        let max_iy = layout.squares_y as i32;
        let w = width as f64;
        let h = height as f64;
        let margin = 64.0;

        if charuco_marker_samples.is_empty() {
            charuco_reason = Some("charuco: no marker samples".into());
        } else {
            for sample in charuco_marker_samples {
                let CharucoMarkerSample { obj_marker, img_marker, square_ij: (si, sj), area_px2: area } = sample;
                let obj_pts: [Point2; 4] = [(obj_marker[0].x, obj_marker[0].y), (obj_marker[1].x, obj_marker[1].y), (obj_marker[2].x, obj_marker[2].y), (obj_marker[3].x, obj_marker[3].y)];
                let img_pts: [Point2; 4] = [(img_marker[0].x, img_marker[0].y), (img_marker[1].x, img_marker[1].y), (img_marker[2].x, img_marker[2].y), (img_marker[3].x, img_marker[3].y)];
                // Solve a local homography from this marker's object corners to image corners.
                let hm = solve_homography(&obj_pts, &img_pts);
                let Some(hm) = hm else { continue };

                // Weight larger markers more heavily; they generally have lower corner noise.
                let weight = area.max(1.0);

                let sx0 = (si as f64) * square;
                let sy0 = (sj as f64) * square;
                let sx1 = sx0 + square;
                let sy1 = sy0 + square;
                let corners = [(si as i32, sj as i32, (sx0, sy0)), ((si + 1) as i32, sj as i32, (sx1, sy0)), ((si + 1) as i32, (sj + 1) as i32, (sx1, sy1)), (si as i32, (sj + 1) as i32, (sx0, sy1))];
                for (ix, iy, obj_corner) in corners {
                    if ix <= 0 || iy <= 0 || ix >= max_ix || iy >= max_iy {
                        continue;
                    }
                    if let Some((u, v)) = project_homography(&hm, obj_corner) {
                        if u < -margin || u > (w + margin) || v < -margin || v > (h + margin) {
                            continue;
                        }
                        let entry = charuco_accum.entry((ix, iy)).or_default();
                        entry.sum_u += u * weight;
                        entry.sum_v += v * weight;
                        entry.sum_w += weight;
                        entry.count = entry.count.saturating_add(1);
                    }
                }
            }

            for ((ix, iy), acc) in charuco_accum {
                if acc.count == 0 || acc.sum_w <= 0.0 {
                    continue;
                }
                let u = acc.sum_u / acc.sum_w;
                let v = acc.sum_v / acc.sum_w;
                let obj = Point { x: (ix as f64) * square, y: (iy as f64) * square };
                charuco_pairs.push(CalibrationPointPair::new(obj, Point { x: u, y: v }));
            }
        }

        if let Some(gray) = gray {
            for pair in &mut charuco_pairs {
                if let Some((ru, rv)) = refine_charuco_corner_harris(gray, pair.image.x, pair.image.y) {
                    pair.image.x = ru;
                    pair.image.y = rv;
                }
            }
        }

        let count = charuco_pairs.len();
        if count >= min_points {
            charuco_used = true;
        } else {
            charuco_reason = Some(format!("charuco: insufficient corners ({count} < {min_points})"));
        }
    }

    // Report the chessboard corner count for ChArUco mode. Marker mode reports marker points.
    stats.points_detected = charuco_pairs.len().min(u32::MAX as usize) as u32;
    let view = if charuco_used { CalibrationView::new(charuco_pairs) } else { CalibrationView::new(Vec::new()) };

    BuildViewResult { view, marker_view, center_view, stats, alignment_error_sum, alignment_error_count, charuco_used, charuco_reason }
}
