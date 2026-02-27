#![allow(unsafe_code)]

use image::GrayImage;
use imageproc::contours::{BorderType, Contour};
use imageproc::point::Point;

use super::shared::*;

pub(super) fn suzuki_abe_with_scratch_i32(image: &GrayImage, image_values: &mut Vec<i32>) -> Vec<Contour<i32>> {
    let width = image.width() as usize;
    let width_i32 = width as i32;
    let height = image.height() as usize;
    let height_i32 = height as i32;
    let max_x = width_i32 - 1;
    let max_y = height_i32 - 1;
    let width_isize = width as isize;
    let offsets_lin = [-1, -width_isize - 1, -width_isize, -width_isize + 1, 1, width_isize + 1, width_isize, width_isize - 1];

    let image_data = image.as_raw();
    ensure_scratch_len(image_values, image_data.len());

    let mut neighborhood = Neighborhood::default();
    let steps_fwd = build_neighbor_steps(&offsets_lin, &neighborhood.offsets, &NEIGHBOR_FORWARD);
    let steps_rev = build_neighbor_steps(&offsets_lin, &neighborhood.offsets, &NEIGHBOR_REVERSE);
    let mut contours: Vec<Contour<i32>> = Vec::new();
    let mut curr_border_num = 1;

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

                    let mut contour_points: ContourPoints = ContourPoints::with_capacity(32);
                    let curr = Point::new(xi, yi);
                    neighborhood.rotate_to_value(adj - curr);

                    let start = neighborhood.start;
                    let first_pos = first_neighbor_outer_fwd(start, &steps_fwd, image_data, width, width_i32, height_i32, curr);

                    if let Some((mut start_dir, pos1)) = first_pos {
                        let mut pos3 = curr;
                        let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                        loop {
                            contour_points.push(pos3);
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
                    } else {
                        contour_points.push(curr);
                        unsafe {
                            *image_values.get_unchecked_mut(idx) = -curr_border_num;
                        }
                    }

                    contours.push(Contour::new(contour_points.into_vec(), border_type, parent));
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

                    let mut contour_points: ContourPoints = ContourPoints::with_capacity(32);
                    let curr = Point::new(xi, yi);
                    neighborhood.rotate_to_value(adj - curr);

                    let start = neighborhood.start;
                    let base = (yi as isize) * width_isize + (xi as isize);
                    let first_pos = first_neighbor_inner_fwd(base, start, &steps_fwd, image_data, curr);

                    if let Some((mut start_dir, pos1)) = first_pos {
                        let mut pos3 = curr;
                        let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                        loop {
                            contour_points.push(pos3);
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
                    } else {
                        contour_points.push(curr);
                        unsafe {
                            *image_values.get_unchecked_mut(idx) = -curr_border_num;
                        }
                    }

                    contours.push(Contour::new(contour_points.into_vec(), border_type, parent));
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

                        let mut contour_points: ContourPoints = ContourPoints::with_capacity(32);
                        let curr = Point::new(xi, yi);
                        neighborhood.rotate_to_value(adj - curr);

                        let start = neighborhood.start;
                        let first_pos = first_neighbor_outer_fwd(start, &steps_fwd, image_data, width, width_i32, height_i32, curr);

                        if let Some((mut start_dir, pos1)) = first_pos {
                            let mut pos3 = curr;
                            let mut pos_idx = (pos3.y as usize) * width + pos3.x as usize;

                            loop {
                                contour_points.push(pos3);
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
                        } else {
                            contour_points.push(curr);
                            unsafe {
                                *image_values.get_unchecked_mut(idx) = -curr_border_num;
                            }
                        }

                        contours.push(Contour::new(contour_points.into_vec(), border_type, parent));
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

                    let mut contour_points: ContourPoints = ContourPoints::with_capacity(32);
                    let curr = Point::new(xi, yi);
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
                            contour_points.push(pos3);
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
                    } else {
                        contour_points.push(curr);
                        unsafe {
                            *image_values.get_unchecked_mut(idx) = -curr_border_num;
                        }
                    }

                    contours.push(Contour::new(contour_points.into_vec(), border_type, parent));
                }

                let new_state = unsafe { *image_values.get_unchecked(idx) };
                if new_state != 0 {
                    parent_border_num = new_state.unsigned_abs() as usize;
                }
                x += 1;
            }
        }
    }

    contours
}
