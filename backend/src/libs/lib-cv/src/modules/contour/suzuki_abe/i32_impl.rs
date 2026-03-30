#![allow(unsafe_code)]

use image::GrayImage;
use imageproc::contours::{BorderType, Contour};
use imageproc::point::Point;

use super::shared::*;

pub(super) fn suzuki_abe_with_scratch_i32(image: &GrayImage, image_values: &mut Vec<i32>) -> Vec<Contour<i32>> {
    let mut point_store = Vec::new();
    let mut compact_contours = Vec::new();
    let _ = suzuki_abe_with_scratch_i32_compact_mode(image, image_values, &mut point_store, &mut compact_contours, false, usize::MAX, usize::MAX);
    compact_contours.into_iter().map(|contour| Contour::new(contour.points(&point_store).to_vec(), contour.border_type, contour.parent)).collect()
}

#[inline(always)]
fn push_compact_contour(
    contours: &mut Vec<CompactContour>,
    border_type: BorderType,
    parent: Option<usize>,
    start: usize,
    end: usize,
    chain_len: usize,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
) {
    if end > start {
        contours.push(CompactContour { start, len: end - start, chain_len: chain_len.min(u32::MAX as usize) as u32, border_type, parent, min_x, min_y, max_x, max_y });
    }
}

#[inline(always)]
fn push_point_capped(point_store: &mut Vec<Point<i32>>, point: Point<i32>, contour_start: usize, point_limit: usize) -> bool {
    if point_store.len() >= point_limit {
        point_store.truncate(contour_start);
        return false;
    }
    point_store.push(point);
    true
}

#[derive(Clone, Copy, Default)]
struct CompactTraceRecorder {
    compress_turns: bool,
    chain_len: usize,
    first: Option<Point<i32>>,
    first_dir: Option<(i32, i32)>,
    prev_dir: Option<(i32, i32)>,
}

impl CompactTraceRecorder {
    #[inline(always)]
    fn new(compress_turns: bool) -> Self {
        Self { compress_turns, ..Self::default() }
    }

    #[inline(always)]
    fn record_step(&mut self, point_store: &mut Vec<Point<i32>>, point: Point<i32>, next: Point<i32>, contour_start: usize, point_limit: usize, traced_points: &mut usize) -> bool {
        self.chain_len = self.chain_len.saturating_add(1);
        *traced_points = traced_points.saturating_add(1);
        if *traced_points > point_limit {
            point_store.truncate(contour_start);
            return false;
        }
        if !self.compress_turns {
            return push_point_capped(point_store, point, contour_start, point_limit);
        }

        let dir = ((next.x - point.x).signum(), (next.y - point.y).signum());
        if let Some(prev_dir) = self.prev_dir {
            if prev_dir != dir {
                if !push_point_capped(point_store, point, contour_start, point_limit) {
                    return false;
                }
            }
        } else {
            self.first = Some(point);
            self.first_dir = Some(dir);
        }
        self.prev_dir = Some(dir);
        true
    }

    #[inline(always)]
    fn record_singleton(&mut self, point_store: &mut Vec<Point<i32>>, point: Point<i32>, contour_start: usize, point_limit: usize, traced_points: &mut usize) -> bool {
        self.chain_len = self.chain_len.saturating_add(1);
        *traced_points = traced_points.saturating_add(1);
        if *traced_points > point_limit {
            point_store.truncate(contour_start);
            return false;
        }
        push_point_capped(point_store, point, contour_start, point_limit)
    }

    #[inline(always)]
    fn finish(&self, point_store: &mut Vec<Point<i32>>, contour_start: usize, point_limit: usize) -> bool {
        if !self.compress_turns {
            return true;
        }
        if let (Some(first), Some(first_dir), Some(last_dir)) = (self.first, self.first_dir, self.prev_dir) {
            if last_dir != first_dir {
                return push_point_capped(point_store, first, contour_start, point_limit);
            } else if point_store.len() == contour_start {
                return push_point_capped(point_store, first, contour_start, point_limit);
            }
        }
        true
    }
}

#[inline(always)]
fn update_bounds(min_x: &mut i32, min_y: &mut i32, max_x: &mut i32, max_y: &mut i32, point: Point<i32>) {
    *min_x = (*min_x).min(point.x);
    *min_y = (*min_y).min(point.y);
    *max_x = (*max_x).max(point.x);
    *max_y = (*max_y).max(point.y);
}

pub(super) fn suzuki_abe_with_scratch_i32_compact(image: &GrayImage, image_values: &mut Vec<i32>, contour_points_store: &mut Vec<Point<i32>>, contours: &mut Vec<CompactContour>) {
    let _ = suzuki_abe_with_scratch_i32_compact_mode(image, image_values, contour_points_store, contours, false, usize::MAX, usize::MAX);
}

pub(super) fn suzuki_abe_with_scratch_i32_compact_capped(
    image: &GrayImage,
    image_values: &mut Vec<i32>,
    contour_points_store: &mut Vec<Point<i32>>,
    contours: &mut Vec<CompactContour>,
    max_points: usize,
    max_contours: usize,
) -> bool {
    suzuki_abe_with_scratch_i32_compact_mode(image, image_values, contour_points_store, contours, false, max_points, max_contours)
}

pub(super) fn suzuki_abe_with_scratch_i32_turn_compact_capped(
    image: &GrayImage,
    image_values: &mut Vec<i32>,
    contour_points_store: &mut Vec<Point<i32>>,
    contours: &mut Vec<CompactContour>,
    max_points: usize,
    max_contours: usize,
) -> bool {
    suzuki_abe_with_scratch_i32_compact_mode(image, image_values, contour_points_store, contours, true, max_points, max_contours)
}

fn suzuki_abe_with_scratch_i32_compact_mode(
    image: &GrayImage,
    image_values: &mut Vec<i32>,
    contour_points_store: &mut Vec<Point<i32>>,
    contours: &mut Vec<CompactContour>,
    compress_turns: bool,
    max_points: usize,
    max_contours: usize,
) -> bool {
    let width = image.width() as usize;
    let width_i32 = width as i32;
    let height = image.height() as usize;
    let height_i32 = height as i32;
    let width_isize = width as isize;
    let offsets_lin = [-1, -width_isize - 1, -width_isize, -width_isize + 1, 1, width_isize + 1, width_isize, width_isize - 1];

    let image_data = image.as_raw();
    ensure_scratch_len(image_values, image_data.len());

    let mut neighborhood = Neighborhood::default();
    let steps_fwd = build_neighbor_steps(&offsets_lin, &neighborhood.offsets, &NEIGHBOR_FORWARD);
    let steps_rev = build_neighbor_steps(&offsets_lin, &neighborhood.offsets, &NEIGHBOR_REVERSE);
    contour_points_store.clear();
    contours.clear();
    let mut curr_border_num = 1;
    let point_limit = max_points.max(1);
    let contour_limit = max_contours.max(1);
    let mut traced_points = 0usize;

    for y in 0..height {
        let mut parent_border_num = 1;
        let row_start = y * width;

        let row = &image_data[row_start..row_start + width];
        let inner_row = y > 0 && y + 1 < height;
        let mut x = 0usize;

        if inner_row && width > 2 {
            if unsafe { *row.get_unchecked(0) } == 0 {
                if let Some(next) = find_next_255(row, 1) {
                    x = next;
                } else {
                    continue;
                }
            }
            if x == 0 {
                let idx = row_start + x;
                let xi = x as i32;
                let yi = y as i32;

                let state = unsafe { *image_values.get_unchecked(idx) };

                if let Some((adj, border_type)) = if state == 0 && x > 0 && unsafe { *image_data.get_unchecked(idx - 1) } == 0 {
                    Some((Point::new(xi - 1, yi), BorderType::Outer))
                } else if state > 0 && x + 1 < width && unsafe { *image_data.get_unchecked(idx + 1) } == 0 {
                    if state > 1 {
                        parent_border_num = state as usize;
                    }
                    Some((Point::new(xi + 1, yi), BorderType::Hole))
                } else {
                    None
                } {
                    if contours.len() >= contour_limit {
                        return false;
                    }
                    curr_border_num += 1;

                    let parent = if parent_border_num > 1 {
                        let parent_index = parent_border_num - 2;
                        if let Some(parent_contour) = contours.get(parent_index) {
                            if (border_type == BorderType::Outer) ^ (parent_contour.border_type == BorderType::Outer) { Some(parent_index) } else { parent_contour.parent }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let contour_start = contour_points_store.len();
                    let mut recorder = CompactTraceRecorder::new(compress_turns);
                    let curr = Point::new(xi, yi);
                    let (mut min_x, mut min_y, mut max_x, mut max_y) = (curr.x, curr.y, curr.x, curr.y);
                    neighborhood.rotate_to_value(adj - curr);

                    let start = neighborhood.start;
                    let first_pos = first_neighbor_outer_fwd(start, &steps_fwd, image_data, width, width_i32, height_i32, curr);

                    if let Some((mut start_dir, pos1)) = first_pos {
                        let mut pos3 = curr;
                        let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                        loop {
                            update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, pos3);
                            let start = start_dir;
                            let pos3_x = pos3.x;
                            let pos3_y = pos3.y;
                            let is_inner_pos = pos3_x > 0 && pos3_x < max_x && pos3_y > 0 && pos3_y < max_y;
                            let base = pos_idx as isize;

                            let (next_idx, pos4) = if is_inner_pos {
                                next_neighbor_inner_rev(base, start, &steps_rev, image_data, pos3_x, pos3_y)
                            } else {
                                next_neighbor_outer_rev(start, &steps_rev, image_data, width, width_i32, height_i32, pos3_x, pos3_y)
                            };
                            if !recorder.record_step(contour_points_store, pos3, pos4, contour_start, point_limit, &mut traced_points) {
                                return false;
                            }
                            let next_pos_idx = if is_inner_pos { (base + offsets_lin[next_idx]) as usize } else { (pos4.y as usize) * width + pos4.x as usize };

                            let is_right_edge = unsafe { *RIGHT_EDGE.get_unchecked(start).get_unchecked(next_idx) };
                            unsafe {
                                let slot = image_values.get_unchecked_mut(pos_idx);
                                if (!is_inner_pos && pos3_x == max_x) || is_right_edge {
                                    *slot = -curr_border_num;
                                } else if *slot == 0 {
                                    *slot = curr_border_num;
                                }
                            }

                            if pos4 == curr && pos3 == pos1 {
                                break;
                            }

                            pos3 = pos4;
                            pos_idx = next_pos_idx;
                            start_dir = unsafe { *OPPOSITE.get_unchecked(next_idx) };
                        }
                        if !recorder.finish(contour_points_store, contour_start, point_limit) {
                            return false;
                        }
                    } else {
                        update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, curr);
                        if !recorder.record_singleton(contour_points_store, curr, contour_start, point_limit, &mut traced_points) {
                            return false;
                        }
                        unsafe {
                            *image_values.get_unchecked_mut(idx) = -curr_border_num;
                        }
                    }

                    push_compact_contour(contours, border_type, parent, contour_start, contour_points_store.len(), recorder.chain_len, min_x, min_y, max_x, max_y);
                }

                let new_state = unsafe { *image_values.get_unchecked(idx) };
                if new_state != 0 {
                    parent_border_num = new_state.unsigned_abs() as usize;
                }
                x = 1;
            }

            while x + 1 < width {
                let idx = row_start + x;
                if unsafe { *row.get_unchecked(x) } == 0 {
                    if let Some(next) = find_next_255(row, x + 1) {
                        x = next;
                        if x + 1 >= width {
                            break;
                        }
                        continue;
                    }
                    x = width;
                    break;
                }

                let xi = x as i32;
                let yi = y as i32;

                let state = unsafe { *image_values.get_unchecked(idx) };

                if let Some((adj, border_type)) = if state == 0 && unsafe { *image_data.get_unchecked(idx - 1) } == 0 {
                    Some((Point::new(xi - 1, yi), BorderType::Outer))
                } else if state > 0 && unsafe { *image_data.get_unchecked(idx + 1) } == 0 {
                    if state > 1 {
                        parent_border_num = state as usize;
                    }
                    Some((Point::new(xi + 1, yi), BorderType::Hole))
                } else {
                    None
                } {
                    if contours.len() >= contour_limit {
                        return false;
                    }
                    curr_border_num += 1;

                    let parent = if parent_border_num > 1 {
                        let parent_index = parent_border_num - 2;
                        if let Some(parent_contour) = contours.get(parent_index) {
                            if (border_type == BorderType::Outer) ^ (parent_contour.border_type == BorderType::Outer) { Some(parent_index) } else { parent_contour.parent }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let contour_start = contour_points_store.len();
                    let mut recorder = CompactTraceRecorder::new(compress_turns);
                    let curr = Point::new(xi, yi);
                    let (mut min_x, mut min_y, mut max_x, mut max_y) = (curr.x, curr.y, curr.x, curr.y);
                    neighborhood.rotate_to_value(adj - curr);

                    let start = neighborhood.start;
                    let base = (yi as isize) * width_isize + (xi as isize);
                    let first_pos = first_neighbor_inner_fwd(base, start, &steps_fwd, image_data, curr);

                    if let Some((mut start_dir, pos1)) = first_pos {
                        let mut pos3 = curr;
                        let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                        loop {
                            update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, pos3);
                            let start = start_dir;
                            let pos3_x = pos3.x;
                            let pos3_y = pos3.y;
                            let is_inner_pos = pos3_x > 0 && pos3_x < max_x && pos3_y > 0 && pos3_y < max_y;
                            let base = pos_idx as isize;

                            let (next_idx, pos4) = if is_inner_pos {
                                next_neighbor_inner_rev(base, start, &steps_rev, image_data, pos3_x, pos3_y)
                            } else {
                                next_neighbor_outer_rev(start, &steps_rev, image_data, width, width_i32, height_i32, pos3_x, pos3_y)
                            };
                            if !recorder.record_step(contour_points_store, pos3, pos4, contour_start, point_limit, &mut traced_points) {
                                return false;
                            }
                            let next_pos_idx = if is_inner_pos { (base + offsets_lin[next_idx]) as usize } else { (pos4.y as usize) * width + pos4.x as usize };

                            let is_right_edge = unsafe { *RIGHT_EDGE.get_unchecked(start).get_unchecked(next_idx) };

                            unsafe {
                                let slot = image_values.get_unchecked_mut(pos_idx);
                                if (!is_inner_pos && pos3_x == max_x) || is_right_edge {
                                    *slot = -curr_border_num;
                                } else if *slot == 0 {
                                    *slot = curr_border_num;
                                }
                            }

                            if pos4 == curr && pos3 == pos1 {
                                break;
                            }

                            pos3 = pos4;
                            pos_idx = next_pos_idx;
                            start_dir = unsafe { *OPPOSITE.get_unchecked(next_idx) };
                        }
                        if !recorder.finish(contour_points_store, contour_start, point_limit) {
                            return false;
                        }
                    } else {
                        update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, curr);
                        if !recorder.record_singleton(contour_points_store, curr, contour_start, point_limit, &mut traced_points) {
                            return false;
                        }
                        unsafe {
                            *image_values.get_unchecked_mut(idx) = -curr_border_num;
                        }
                    }

                    push_compact_contour(contours, border_type, parent, contour_start, contour_points_store.len(), recorder.chain_len, min_x, min_y, max_x, max_y);
                }

                let new_state = unsafe { *image_values.get_unchecked(idx) };
                if new_state != 0 {
                    parent_border_num = new_state.unsigned_abs() as usize;
                }
                x += 1;
            }

            if x < width {
                let idx = row_start + x;
                if unsafe { *row.get_unchecked(x) } != 0 {
                    let xi = x as i32;
                    let yi = y as i32;

                    let state = unsafe { *image_values.get_unchecked(idx) };

                    if let Some((adj, border_type)) = if state == 0 && x > 0 && unsafe { *image_data.get_unchecked(idx - 1) } == 0 {
                        Some((Point::new(xi - 1, yi), BorderType::Outer))
                    } else if state > 0 && x + 1 < width && unsafe { *image_data.get_unchecked(idx + 1) } == 0 {
                        if state > 1 {
                            parent_border_num = state as usize;
                        }
                        Some((Point::new(xi + 1, yi), BorderType::Hole))
                    } else {
                        None
                    } {
                        if contours.len() >= contour_limit {
                            return false;
                        }
                        curr_border_num += 1;

                        let parent = if parent_border_num > 1 {
                            let parent_index = parent_border_num - 2;
                            if let Some(parent_contour) = contours.get(parent_index) {
                                if (border_type == BorderType::Outer) ^ (parent_contour.border_type == BorderType::Outer) { Some(parent_index) } else { parent_contour.parent }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let contour_start = contour_points_store.len();
                        let mut recorder = CompactTraceRecorder::new(compress_turns);
                        let curr = Point::new(xi, yi);
                        let (mut min_x, mut min_y, mut max_x, mut max_y) = (curr.x, curr.y, curr.x, curr.y);
                        neighborhood.rotate_to_value(adj - curr);

                        let start = neighborhood.start;
                        let first_pos = first_neighbor_outer_fwd(start, &steps_fwd, image_data, width, width_i32, height_i32, curr);

                        if let Some((mut start_dir, pos1)) = first_pos {
                            let mut pos3 = curr;
                            let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                            loop {
                                update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, pos3);
                                let start = start_dir;
                                let pos3_x = pos3.x;
                                let pos3_y = pos3.y;
                                let is_inner_pos = pos3_x > 0 && pos3_x < max_x && pos3_y > 0 && pos3_y < max_y;
                                let base = pos_idx as isize;

                                let (next_idx, pos4) = if is_inner_pos {
                                    next_neighbor_inner_rev(base, start, &steps_rev, image_data, pos3_x, pos3_y)
                                } else {
                                    next_neighbor_outer_rev(start, &steps_rev, image_data, width, width_i32, height_i32, pos3_x, pos3_y)
                                };
                                if !recorder.record_step(contour_points_store, pos3, pos4, contour_start, point_limit, &mut traced_points) {
                                    return false;
                                }
                                let next_pos_idx = if is_inner_pos { (base + offsets_lin[next_idx]) as usize } else { (pos4.y as usize) * width + pos4.x as usize };

                                let is_right_edge = unsafe { *RIGHT_EDGE.get_unchecked(start).get_unchecked(next_idx) };

                                unsafe {
                                    let slot = image_values.get_unchecked_mut(pos_idx);
                                    if (!is_inner_pos && pos3_x == max_x) || is_right_edge {
                                        *slot = -curr_border_num;
                                    } else if *slot == 0 {
                                        *slot = curr_border_num;
                                    }
                                }

                                if pos4 == curr && pos3 == pos1 {
                                    break;
                                }

                                pos3 = pos4;
                                pos_idx = next_pos_idx;
                                start_dir = unsafe { *OPPOSITE.get_unchecked(next_idx) };
                            }
                            if !recorder.finish(contour_points_store, contour_start, point_limit) {
                                return false;
                            }
                        } else {
                            update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, curr);
                            if !recorder.record_singleton(contour_points_store, curr, contour_start, point_limit, &mut traced_points) {
                                return false;
                            }
                            unsafe {
                                *image_values.get_unchecked_mut(idx) = -curr_border_num;
                            }
                        }

                        push_compact_contour(contours, border_type, parent, contour_start, contour_points_store.len(), recorder.chain_len, min_x, min_y, max_x, max_y);
                    }
                }
            }
        } else {
            while x < width {
                let idx = row_start + x;
                if unsafe { *row.get_unchecked(x) } == 0 {
                    if x + 1 >= width {
                        break;
                    }
                    if let Some(next) = find_next_255(row, x + 1) {
                        x = next;
                        continue;
                    }
                    break;
                }

                let xi = x as i32;
                let yi = y as i32;
                let is_inner_start = inner_row && x > 0 && x + 1 < width;

                let state = unsafe { *image_values.get_unchecked(idx) };

                if let Some((adj, border_type)) = if state == 0 && x > 0 && unsafe { *image_data.get_unchecked(idx - 1) } == 0 {
                    Some((Point::new(xi - 1, yi), BorderType::Outer))
                } else if state > 0 && x + 1 < width && unsafe { *image_data.get_unchecked(idx + 1) } == 0 {
                    if state > 1 {
                        parent_border_num = state as usize;
                    }
                    Some((Point::new(xi + 1, yi), BorderType::Hole))
                } else {
                    None
                } {
                    if contours.len() >= contour_limit {
                        return false;
                    }
                    curr_border_num += 1;

                    let parent = if parent_border_num > 1 {
                        let parent_index = parent_border_num - 2;
                        if let Some(parent_contour) = contours.get(parent_index) {
                            if (border_type == BorderType::Outer) ^ (parent_contour.border_type == BorderType::Outer) { Some(parent_index) } else { parent_contour.parent }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let contour_start = contour_points_store.len();
                    let mut recorder = CompactTraceRecorder::new(compress_turns);
                    let curr = Point::new(xi, yi);
                    let (mut min_x, mut min_y, mut max_x, mut max_y) = (curr.x, curr.y, curr.x, curr.y);
                    neighborhood.rotate_to_value(adj - curr);

                    let start = neighborhood.start;
                    let first_pos = if is_inner_start {
                        let base = (yi as isize) * width_isize + (xi as isize);
                        first_neighbor_inner_fwd(base, start, &steps_fwd, image_data, curr)
                    } else {
                        first_neighbor_outer_fwd(start, &steps_fwd, image_data, width, width_i32, height_i32, curr)
                    };

                    if let Some((mut start_dir, pos1)) = first_pos {
                        let mut pos3 = curr;
                        let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                        loop {
                            update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, pos3);
                            let start = start_dir;
                            let pos3_x = pos3.x;
                            let pos3_y = pos3.y;
                            let is_inner_pos = pos3_x > 0 && pos3_x < max_x && pos3_y > 0 && pos3_y < max_y;
                            let base = pos_idx as isize;

                            let (next_idx, pos4) = if is_inner_pos {
                                next_neighbor_inner_rev(base, start, &steps_rev, image_data, pos3_x, pos3_y)
                            } else {
                                next_neighbor_outer_rev(start, &steps_rev, image_data, width, width_i32, height_i32, pos3_x, pos3_y)
                            };
                            if !recorder.record_step(contour_points_store, pos3, pos4, contour_start, point_limit, &mut traced_points) {
                                return false;
                            }
                            let next_pos_idx = if is_inner_pos { (base + offsets_lin[next_idx]) as usize } else { (pos4.y as usize) * width + pos4.x as usize };

                            let is_right_edge = unsafe { *RIGHT_EDGE.get_unchecked(start).get_unchecked(next_idx) };
                            unsafe {
                                let slot = image_values.get_unchecked_mut(pos_idx);
                                if (!is_inner_pos && pos3_x == max_x) || is_right_edge {
                                    *slot = -curr_border_num;
                                } else if *slot == 0 {
                                    *slot = curr_border_num;
                                }
                            }

                            if pos4 == curr && pos3 == pos1 {
                                break;
                            }

                            pos3 = pos4;
                            pos_idx = next_pos_idx;
                            start_dir = unsafe { *OPPOSITE.get_unchecked(next_idx) };
                        }
                        if !recorder.finish(contour_points_store, contour_start, point_limit) {
                            return false;
                        }
                    } else {
                        update_bounds(&mut min_x, &mut min_y, &mut max_x, &mut max_y, curr);
                        if !recorder.record_singleton(contour_points_store, curr, contour_start, point_limit, &mut traced_points) {
                            return false;
                        }
                        unsafe {
                            *image_values.get_unchecked_mut(idx) = -curr_border_num;
                        }
                    }

                    push_compact_contour(contours, border_type, parent, contour_start, contour_points_store.len(), recorder.chain_len, min_x, min_y, max_x, max_y);
                }

                let new_state = unsafe { *image_values.get_unchecked(idx) };
                if new_state != 0 {
                    parent_border_num = new_state.unsigned_abs() as usize;
                }
                x += 1;
            }
        }
    }

    crate::diagnostics::report_scratch_high_water("contour.suzuki_compact_contours", compact_contours_bytes(contour_points_store.capacity(), contours.capacity()));
    true
}
