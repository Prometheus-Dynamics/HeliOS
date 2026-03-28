use image::GrayImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use wide::u8x32;

thread_local! {
    static COMPONENT_SCRATCH: RefCell<ComponentScratch> = RefCell::new(ComponentScratch::default());
    static COMPONENT_FEATURE_SCRATCH: RefCell<ComponentFeatureScratch> = RefCell::new(ComponentFeatureScratch::default());
}

const COMPONENT_OFFSET_RETAIN_CAP: usize = 256;
const COMPONENT_PARENT_RETAIN_CAP: usize = 16 * 1024;
const COMPONENT_STRIPE_KEEP_RETAIN_CAP: usize = 16 * 1024;
const COMPONENT_ROW_TO_STRIPE_RETAIN_CAP: usize = 2 * 1024;
const COMPONENT_FEATURE_VISITED_RETAIN_CAP: usize = 2 * 1024 * 1024;
const COMPONENT_FEATURE_STACK_RETAIN_CAP: usize = 16 * 1024;

#[inline(always)]
fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

#[derive(Default)]
struct ComponentScratch {
    component_offsets: Vec<u32>,
    global_parents: Vec<u32>,
    areas: Vec<AtomicUsize>,
    stripe_keep: Vec<Vec<u8>>,
    row_to_stripe: Vec<(usize, usize)>,
}

#[derive(Default)]
struct ComponentFeatureScratch {
    visited: Vec<u8>,
    stack: Vec<usize>,
}

pub(crate) fn compact_component_scratch_after_frame() {
    COMPONENT_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch.component_offsets, COMPONENT_OFFSET_RETAIN_CAP);
        trim_retained_vec(&mut scratch.global_parents, COMPONENT_PARENT_RETAIN_CAP);
        trim_retained_vec(&mut scratch.areas, COMPONENT_PARENT_RETAIN_CAP);
        for keep in &mut scratch.stripe_keep {
            trim_retained_vec(keep, COMPONENT_STRIPE_KEEP_RETAIN_CAP);
        }
        trim_retained_vec(&mut scratch.stripe_keep, COMPONENT_OFFSET_RETAIN_CAP);
        trim_retained_vec(&mut scratch.row_to_stripe, COMPONENT_ROW_TO_STRIPE_RETAIN_CAP);
    });
    COMPONENT_FEATURE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch.visited, COMPONENT_FEATURE_VISITED_RETAIN_CAP);
        trim_retained_vec(&mut scratch.stack, COMPONENT_FEATURE_STACK_RETAIN_CAP);
    });
}

pub(crate) fn release_component_scratch_on_idle() {
    COMPONENT_SCRATCH.with(|scratch| {
        *scratch.borrow_mut() = ComponentScratch::default();
    });
    COMPONENT_FEATURE_SCRATCH.with(|scratch| {
        *scratch.borrow_mut() = ComponentFeatureScratch::default();
    });
}

pub fn remove_small_components(mask: &GrayImage, min_area: u32) -> GrayImage {
    let width = mask.width();
    let height = mask.height();
    if width == 0 || height == 0 {
        return GrayImage::new(width, height);
    }
    let mut output = mask.clone();
    remove_small_components_in_place(&mut output, min_area);
    output
}

pub fn remove_small_components_in_place(mask: &mut GrayImage, min_area: u32) {
    let width = mask.width();
    let height = mask.height();
    if width == 0 || height == 0 {
        return;
    }
    if min_area <= 1 {
        return;
    }
    if !mask.as_flat_samples().samples.iter().any(|&v| v != 0) {
        return;
    }
    COMPONENT_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        remove_small_components_in_place_with_scratch(mask, min_area as usize, &mut scratch);
    });
}

fn remove_small_components_in_place_with_scratch(mask: &mut GrayImage, min_area: usize, scratch: &mut ComponentScratch) {
    let width_usize = mask.width() as usize;
    let height_usize = mask.height() as usize;
    let input = mask.as_flat_samples().samples;

    let stripes = build_stripes(height_usize);
    let stripe_results: Vec<StripeResult> = stripes.par_iter().map(|&(start, end)| process_stripe(input, width_usize, start, end)).collect();

    scratch.component_offsets.clear();
    scratch.component_offsets.reserve(stripe_results.len());
    let mut total_components = 0u32;
    for stripe in &stripe_results {
        scratch.component_offsets.push(total_components);
        total_components += stripe.component_count;
    }

    let needed_parents = total_components as usize + 1;
    if scratch.global_parents.len() != needed_parents {
        scratch.global_parents.resize(needed_parents, 0);
    }
    for (i, parent) in scratch.global_parents.iter_mut().enumerate() {
        *parent = i as u32;
    }

    merge_stripe_boundaries(&stripe_results, &scratch.component_offsets, width_usize, &mut scratch.global_parents);

    for label in 1..scratch.global_parents.len() {
        let root = find(&mut scratch.global_parents, label as u32);
        scratch.global_parents[label] = root;
    }

    if scratch.areas.len() != scratch.global_parents.len() {
        scratch.areas = (0..scratch.global_parents.len()).map(|_| AtomicUsize::new(0)).collect();
    } else {
        for area in &scratch.areas {
            area.store(0, Ordering::Relaxed);
        }
    }

    stripe_results.par_iter().enumerate().for_each(|(stripe_idx, stripe)| {
        let offset = scratch.component_offsets[stripe_idx] as usize;
        for (local_label, &area_value) in stripe.component_areas.iter().enumerate().skip(1).take(stripe.component_count as usize) {
            let global = offset + local_label;
            let root = scratch.global_parents[global] as usize;
            let area = area_value as usize;
            if area != 0 {
                scratch.areas[root].fetch_add(area, Ordering::Relaxed);
            }
        }
    });

    let area_counts: Vec<usize> = scratch.areas.iter().map(|a| a.load(Ordering::Relaxed)).collect();

    if scratch.stripe_keep.len() != stripe_results.len() {
        scratch.stripe_keep.resize_with(stripe_results.len(), Vec::new);
    }
    for (stripe_idx, stripe) in stripe_results.iter().enumerate() {
        let offset = scratch.component_offsets[stripe_idx] as usize;
        let count = stripe.component_count as usize;
        let keep = &mut scratch.stripe_keep[stripe_idx];
        if keep.len() != count + 1 {
            keep.resize(count + 1, 0u8);
        } else {
            keep.fill(0);
        }
        for (local_label, keep_value) in keep.iter_mut().enumerate().skip(1).take(count) {
            let global = offset + local_label;
            let root = scratch.global_parents[global] as usize;
            *keep_value = if area_counts[root] >= min_area { 255 } else { 0 };
        }
    }

    if scratch.row_to_stripe.len() != height_usize {
        scratch.row_to_stripe.resize(height_usize, (0, 0));
    }
    for (idx, stripe) in stripe_results.iter().enumerate() {
        for (local_y, global_y) in (stripe.start_row..stripe.end_row).enumerate() {
            scratch.row_to_stripe[global_y] = (idx, local_y);
        }
    }

    let out_buf = mask.as_flat_samples_mut().samples;
    out_buf.fill(0);
    out_buf.par_chunks_mut(width_usize).enumerate().for_each(|(row, row_buf)| {
        let (stripe_idx, local_y) = scratch.row_to_stripe[row];
        let stripe = &stripe_results[stripe_idx];
        let keep = &scratch.stripe_keep[stripe_idx];
        let (run_start, run_len) = stripe.row_runs[local_y];
        let runs = &stripe.runs[run_start..run_start + run_len];
        for run in runs {
            let v = keep[run.label as usize];
            if v == 0 {
                continue;
            }
            row_buf[run.start..run.end].fill(v);
        }
    });
}

const NEIGHBOR_OFFSETS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentFeature {
    pub area: u32,
    pub bounds: (u32, u32, u32, u32),
    pub centroid: (f32, f32),
}

pub fn component_features(mask: &GrayImage, min_area: u32) -> Vec<ComponentFeature> {
    let width = mask.width();
    let height = mask.height();
    if width == 0 || height == 0 {
        return Vec::new();
    }
    COMPONENT_FEATURE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let width_usize = width as usize;
        let height_usize = height as usize;
        let data = mask.as_flat_samples().samples;
        let needed = width_usize.saturating_mul(height_usize);
        if scratch.visited.len() != needed {
            scratch.visited.resize(needed, 0);
        } else {
            scratch.visited.fill(0);
        }
        scratch.stack.clear();

        let ComponentFeatureScratch { visited, stack } = &mut *scratch;
        let mut features = Vec::new();

        for y in 0..height_usize {
            for x in 0..width_usize {
                let idx = y * width_usize + x;
                if visited[idx] != 0 || data[idx] == 0 {
                    continue;
                }

                let mut min_x = x as u32;
                let mut min_y = y as u32;
                let mut max_x = x as u32;
                let mut max_y = y as u32;
                let mut sum_x = 0u64;
                let mut sum_y = 0u64;
                let mut area = 0u32;

                stack.clear();
                stack.push(idx);
                visited[idx] = 1;

                while let Some(current) = stack.pop() {
                    let cx = current % width_usize;
                    let cy = current / width_usize;
                    area += 1;
                    let cx_u32 = cx as u32;
                    let cy_u32 = cy as u32;
                    min_x = min_x.min(cx_u32);
                    min_y = min_y.min(cy_u32);
                    max_x = max_x.max(cx_u32);
                    max_y = max_y.max(cy_u32);
                    sum_x += cx_u32 as u64;
                    sum_y += cy_u32 as u64;

                    for (dx, dy) in NEIGHBOR_OFFSETS {
                        let nx = cx as isize + dx;
                        let ny = cy as isize + dy;
                        if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                            continue;
                        }
                        let nidx = ny as usize * width_usize + nx as usize;
                        if visited[nidx] != 0 || data[nidx] == 0 {
                            continue;
                        }
                        visited[nidx] = 1;
                        stack.push(nidx);
                    }
                }

                if area < min_area {
                    continue;
                }

                let centroid = (sum_x as f32 / area as f32, sum_y as f32 / area as f32);
                features.push(ComponentFeature { area, bounds: (min_x, min_y, max_x, max_y), centroid });
            }
        }

        features
    })
}

fn build_stripes(height: usize) -> Vec<(usize, usize)> {
    let threads = rayon::current_num_threads().max(1);
    let stripe_height = (height / threads).max(64);
    let mut stripes = Vec::new();
    let mut start = 0;
    while start < height {
        let end = (start + stripe_height).min(height);
        stripes.push((start, end));
        start = end;
    }
    stripes
}

struct StripeResult {
    start_row: usize,
    end_row: usize,
    runs: Vec<Run>,
    /// For each local row in the stripe: `(start_index, run_count)` into `runs`.
    row_runs: Vec<(usize, usize)>,
    top_runs: Vec<Run>,
    bottom_runs: Vec<Run>,
    component_areas: Vec<u32>,
    component_count: u32,
}

#[derive(Clone, Copy)]
struct Run {
    start: usize,
    end: usize,
    label: u32,
}

fn process_stripe(input: &[u8], width: usize, start_row: usize, end_row: usize) -> StripeResult {
    let height = end_row - start_row;
    let mut parents = vec![0u32; 1];
    let mut next_label = 0u32;
    let mut prev_runs: Vec<Run> = Vec::new();
    let mut curr_runs: Vec<Run> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut row_runs: Vec<(usize, usize)> = Vec::with_capacity(height);

    for local_y in 0..height {
        let row = &input[(start_row + local_y) * width..(start_row + local_y + 1) * width];
        extract_runs(row, width, &mut curr_runs);

        let mut prev_idx = 0usize;
        for run in curr_runs.iter_mut() {
            while prev_idx < prev_runs.len() && prev_runs[prev_idx].end <= run.start {
                prev_idx += 1;
            }

            let mut assigned_label = 0u32;
            let mut scan_idx = prev_idx;
            while scan_idx < prev_runs.len() && prev_runs[scan_idx].start < run.end {
                let prev_label = prev_runs[scan_idx].label;
                if assigned_label == 0 {
                    assigned_label = prev_label;
                } else if assigned_label != prev_label {
                    union(&mut parents, assigned_label, prev_label);
                }
                scan_idx += 1;
            }

            if assigned_label == 0 {
                next_label += 1;
                parents.push(next_label);
                assigned_label = next_label;
            }

            run.label = assigned_label;
        }

        let start = runs.len();
        runs.extend(curr_runs.iter().copied());
        row_runs.push((start, curr_runs.len()));

        std::mem::swap(&mut prev_runs, &mut curr_runs);
        curr_runs.clear();
    }

    let mut root_ids = vec![0u32; parents.len()];
    let mut component_count = 0u32;
    for label in 1..parents.len() {
        let root = find(&mut parents, label as u32);
        if root_ids[root as usize] == 0 {
            component_count += 1;
            root_ids[root as usize] = component_count;
        }
        root_ids[label] = root_ids[root as usize];
    }

    let mut component_areas = vec![0u32; component_count as usize + 1];
    for run in runs.iter_mut() {
        let remapped = root_ids[run.label as usize];
        run.label = remapped;
        component_areas[remapped as usize] += (run.end - run.start) as u32;
    }

    let top_runs = if height > 0 {
        let (start, len) = row_runs[0];
        runs[start..start + len].to_vec()
    } else {
        Vec::new()
    };
    let bottom_runs = if height > 0 {
        let (start, len) = row_runs[height - 1];
        runs[start..start + len].to_vec()
    } else {
        Vec::new()
    };

    StripeResult { start_row, end_row, runs, row_runs, top_runs, bottom_runs, component_areas, component_count }
}

fn extract_runs(row: &[u8], width: usize, out: &mut Vec<Run>) {
    out.clear();
    let mut current_start: Option<usize> = None;
    let mut x = 0;
    let simd_width = 32;
    while x + simd_width <= width {
        let chunk = u8x32::from(<[u8; 32]>::try_from(&row[x..x + simd_width]).expect("slice len"));
        if chunk == u8x32::splat(0) {
            if let Some(start) = current_start.take() {
                out.push(Run { start, end: x, label: 0 });
            }
            x += simd_width;
            continue;
        }

        for i in 0..simd_width {
            let value = row[x + i];
            if value != 0 {
                if current_start.is_none() {
                    current_start = Some(x + i);
                }
            } else if let Some(start) = current_start.take() {
                out.push(Run { start, end: x + i, label: 0 });
            }
        }
        x += simd_width;
    }

    while x < width {
        let value = row[x];
        if value != 0 {
            if current_start.is_none() {
                current_start = Some(x);
            }
        } else if let Some(start) = current_start.take() {
            out.push(Run { start, end: x, label: 0 });
        }
        x += 1;
    }

    if let Some(start) = current_start.take() {
        out.push(Run { start, end: width, label: 0 });
    }
}

fn merge_stripe_boundaries(stripes: &[StripeResult], offsets: &[u32], width: usize, global_parents: &mut [u32]) {
    let _ = width;
    for (stripe_pair, offset_pair) in stripes.windows(2).zip(offsets.windows(2)) {
        let upper = &stripe_pair[0];
        let lower = &stripe_pair[1];
        let upper_offset = offset_pair[0];
        let lower_offset = offset_pair[1];
        merge_boundary_runs(&upper.bottom_runs, &lower.top_runs, upper_offset, lower_offset, global_parents);
    }
}

fn merge_boundary_runs(upper: &[Run], lower: &[Run], upper_offset: u32, lower_offset: u32, global_parents: &mut [u32]) {
    let mut i = 0usize;
    let mut j = 0usize;
    while i < upper.len() && j < lower.len() {
        let a = upper[i];
        let b = lower[j];
        let start = a.start.max(b.start);
        let end = a.end.min(b.end);
        if start < end {
            union(global_parents, upper_offset + a.label, lower_offset + b.label);
        }
        if a.end <= b.end {
            i += 1;
        } else {
            j += 1;
        }
    }
}

fn find(parents: &mut [u32], label: u32) -> u32 {
    let mut parent = parents[label as usize];
    if parent == label {
        return label;
    }
    while parent != parents[parent as usize] {
        parent = parents[parent as usize];
    }
    let mut node = label;
    while parents[node as usize] != parent {
        let next = parents[node as usize];
        parents[node as usize] = parent;
        node = next;
    }
    parent
}

fn union(parents: &mut [u32], a: u32, b: u32) {
    let root_a = find(parents, a);
    let root_b = find(parents, b);
    if root_a == root_b {
        return;
    }
    if root_a < root_b {
        parents[root_b as usize] = root_a;
    } else {
        parents[root_a as usize] = root_b;
    }
}
