#![allow(unsafe_code)]

use super::*;

use std::mem::size_of;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn ensure_odd_window(value: u32) -> u32 {
    // Large adaptive windows explode compute cost and can overflow fixed-point math.
    // 181x181 keeps the fixed-point compare within i32 bounds and is more than enough
    // for typical camera image adaptive thresholding.
    let candidate = value.clamp(3, 181);
    if candidate.is_multiple_of(2) { candidate + 1 } else { candidate }
}

const ADAPTIVE_SHIFT: i32 = 16;
const ADAPTIVE_PARALLEL_MIN_PIXELS_DEFAULT: usize = 1920 * 1080;

fn adaptive_parallel_min_pixels() -> usize {
    static MIN_PIXELS: OnceLock<usize> = OnceLock::new();
    *MIN_PIXELS.get_or_init(|| std::env::var("HELIOS_ADAPTIVE_PARALLEL_MIN_PIXELS").ok().and_then(|value| value.parse().ok()).unwrap_or(ADAPTIVE_PARALLEL_MIN_PIXELS_DEFAULT))
}

fn adaptive_parallel_threads_default() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(2).max(1)
}

fn adaptive_available_parallelism() -> usize {
    static THREADS: OnceLock<usize> = OnceLock::new();
    *THREADS.get_or_init(|| {
        std::env::var("HELIOS_ADAPTIVE_PARALLEL_THREADS").ok().and_then(|value| value.parse::<usize>().ok()).filter(|value| *value > 0).unwrap_or_else(adaptive_parallel_threads_default)
    })
}

fn adaptive_thread_pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = adaptive_available_parallelism().max(1);
        rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("adaptive threshold thread pool")
    })
}

fn log_adaptive_parallel_choice(use_parallel: bool, width: u32, height: u32) {
    static LOGGED: OnceLock<()> = OnceLock::new();
    if LOGGED.set(()).is_ok() && tracing::level_enabled!(tracing::Level::INFO) {
        let rayon_threads = rayon::current_num_threads();
        let avail = adaptive_available_parallelism();
        tracing::info!(w = width, h = height, use_parallel, parallel_min_pixels = adaptive_parallel_min_pixels(), available_parallelism = avail, rayon_threads, "adaptive_threshold: scheduling");
    }
}

fn should_use_adaptive_parallel(pixel_count: usize) -> bool {
    pixel_count >= adaptive_parallel_min_pixels() && adaptive_available_parallelism() > 1
}

pub fn adaptive_mean_threshold_fast(image: &GrayImage, window: u32, offset: f32) -> GrayImage {
    ADAPTIVE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        adaptive_mean_threshold_fast_inner(image, window, offset, false, &mut buffers)
    })
}

/// Adaptive mean threshold with optional polarity flip.
///
/// When `invert == true`, the output mask polarity is flipped without an extra post-pass over the image.
pub fn adaptive_mean_threshold_fast_with_invert(image: &GrayImage, window: u32, offset: f32, invert: bool) -> GrayImage {
    ADAPTIVE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        adaptive_mean_threshold_fast_inner(image, window, offset, invert, &mut buffers)
    })
}

pub fn adaptive_mean_threshold_fast_into(image: &GrayImage, window: u32, offset: f32, invert: bool, output: &mut GrayImage) {
    let (width, height) = image.dimensions();
    if output.width() != width || output.height() != height {
        *output = GrayImage::new(width, height);
    }
    if width == 0 || height == 0 {
        output.as_mut().fill(0);
        return;
    }
    ADAPTIVE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        adaptive_mean_threshold_fast_inner_into(image, window, offset, invert, &mut buffers, output.as_mut());
    });
}

pub fn with_adaptive_mean_threshold_fast<R>(image: &GrayImage, window: u32, offset: f32, invert: bool, f: impl FnOnce(&GrayImage) -> R) -> R {
    let (width, height) = image.dimensions();
    let needed = (width as usize).saturating_mul(height as usize);
    ADAPTIVE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        MASK_BUFFER.with(|mask| {
            let mut mask = mask.borrow_mut();
            let dst = ensure_mask_buffer(&mut mask, width, height);
            if dst.len() != needed {
                return f(&GrayImage::new(width, height));
            }
            adaptive_mean_threshold_fast_inner_into(image, window, offset, invert, &mut buffers, dst);
            let img = GrayImage::from_raw(width, height, std::mem::take(&mut mask.buf)).expect("mask buffer size must match image dimensions");
            let out = f(&img);
            mask.buf = img.into_raw();
            out
        })
    })
}

pub fn with_adaptive_mean_threshold_fast_timed<R>(image: &GrayImage, window: u32, offset: f32, invert: bool, f: impl FnOnce(&GrayImage) -> R) -> (R, Duration) {
    let start = Instant::now();
    let mut elapsed = Duration::from_secs(0);
    let out = with_adaptive_mean_threshold_fast(image, window, offset, invert, |mask| {
        elapsed = start.elapsed();
        f(mask)
    });
    (out, elapsed)
}

fn adaptive_mean_threshold_fast_inner(image: &GrayImage, window: u32, offset: f32, invert: bool, buffers: &mut AdaptiveBuffers) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut output = GrayImage::new(width, height);
    if width == 0 || height == 0 {
        return output;
    }
    adaptive_mean_threshold_fast_inner_into(image, window, offset, invert, buffers, output.as_mut());
    output
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::needless_range_loop)]
#[allow(unsafe_code)]
fn adaptive_threshold_row_scalar(dst_row: &mut [u8], src_row: &[u8], col_sum: &mut [i32], sub_row: &[u16], add_row: &[u16], invert: bool, area_half_scaled: i32, inv_area: i32, offset_scaled: i32) {
    debug_assert_eq!(dst_row.len(), src_row.len());
    debug_assert_eq!(dst_row.len(), col_sum.len());
    debug_assert_eq!(dst_row.len(), sub_row.len());
    debug_assert_eq!(dst_row.len(), add_row.len());

    let width = dst_row.len();
    if width == 0 {
        return;
    }

    let invert_mask = invert as u8;
    let mut x = 0usize;

    unsafe {
        let dst_ptr = dst_row.as_mut_ptr();
        let src_ptr = src_row.as_ptr();
        let col_ptr = col_sum.as_mut_ptr();
        let sub_ptr = sub_row.as_ptr();
        let add_ptr = add_row.as_ptr();

        while x + 4 <= width {
            let mut idx = x;
            while idx < x + 4 {
                let sum_i32 = *col_ptr.add(idx);
                let mean_scaled = sum_i32 * inv_area + area_half_scaled;
                let threshold_scaled = mean_scaled - offset_scaled;
                let src_scaled = (*src_ptr.add(idx) as i32) << ADAPTIVE_SHIFT;
                let fg = ((src_scaled > threshold_scaled) as u8) ^ invert_mask;
                *dst_ptr.add(idx) = 0u8.wrapping_sub(fg);
                *col_ptr.add(idx) = sum_i32 - (*sub_ptr.add(idx) as i32) + (*add_ptr.add(idx) as i32);
                idx += 1;
            }
            x += 4;
        }

        while x < width {
            let sum_i32 = *col_ptr.add(x);
            let mean_scaled = sum_i32 * inv_area + area_half_scaled;
            let threshold_scaled = mean_scaled - offset_scaled;
            let src_scaled = (*src_ptr.add(x) as i32) << ADAPTIVE_SHIFT;
            let fg = ((src_scaled > threshold_scaled) as u8) ^ invert_mask;
            *dst_ptr.add(x) = 0u8.wrapping_sub(fg);
            *col_ptr.add(x) = sum_i32 - (*sub_ptr.add(x) as i32) + (*add_ptr.add(x) as i32);
            x += 1;
        }
    }
}

#[inline(always)]
fn add_u16_row_to_col_sum_scalar(col_sum: &mut [i32], row: &[u16]) {
    debug_assert_eq!(col_sum.len(), row.len());
    for x in 0..col_sum.len() {
        col_sum[x] += row[x] as i32;
    }
}

#[allow(clippy::needless_range_loop)]
fn compute_horizontal_hsum_row_u16(src_row: &[u8], radius: usize, out: &mut [u16], use_neon: bool) {
    debug_assert_eq!(src_row.len(), out.len());
    #[cfg(not(target_arch = "aarch64"))]
    let _ = use_neon;
    #[cfg(not(target_arch = "aarch64"))]
    let _ = use_neon;
    let width = src_row.len();
    if width == 0 {
        return;
    }
    let last_x = width - 1;
    let radius_i32 = radius as i32;

    let mut sum: i32 = src_row[0] as i32 * (radius_i32 + 1);
    if radius < width {
        for i in 1..=radius {
            sum += src_row[i] as i32;
        }
    } else {
        for i in 1..=radius {
            sum += src_row[i.min(last_x)] as i32;
        }
    }

    let has_inner = width > radius.saturating_mul(2);
    if !has_inner {
        for (x, out_cell) in out.iter_mut().enumerate() {
            debug_assert!(sum <= u16::MAX as i32);
            *out_cell = sum as u16;
            let add_x = (x + radius + 1).min(last_x);
            let sub_x = x.saturating_sub(radius);
            sum += src_row[add_x] as i32 - src_row[sub_x] as i32;
        }
        return;
    }

    let inner_start = radius;
    let inner_end_excl = last_x - radius;
    let first_val = src_row[0] as i32;
    let last_val = src_row[last_x] as i32;

    for x in 0..inner_start {
        debug_assert!(sum <= u16::MAX as i32);
        out[x] = sum as u16;
        let add_x = x + radius + 1;
        sum += src_row[add_x] as i32 - first_val;
    }

    let mut x = inner_start;
    if inner_end_excl > inner_start {
        #[cfg(target_arch = "aarch64")]
        if use_neon {
            // SAFETY: guarded by runtime feature detection and aarch64 target.
            unsafe {
                sum = neon::horizontal_hsum_row_u16_neon(src_row, radius, out, inner_start, inner_end_excl, sum);
            }
            x = inner_end_excl;
        } else {
            for xx in inner_start..inner_end_excl {
                debug_assert!(sum <= u16::MAX as i32);
                out[xx] = sum as u16;
                sum += src_row[xx + radius + 1] as i32 - src_row[xx - radius] as i32;
            }
            x = inner_end_excl;
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            for xx in inner_start..inner_end_excl {
                debug_assert!(sum <= u16::MAX as i32);
                out[xx] = sum as u16;
                sum += src_row[xx + radius + 1] as i32 - src_row[xx - radius] as i32;
            }
            x = inner_end_excl;
        }
    }
    while x < width {
        debug_assert!(sum <= u16::MAX as i32);
        out[x] = sum as u16;
        let sub_x = x - radius;
        sum += last_val - src_row[sub_x] as i32;
        x += 1;
    }
}

fn adaptive_mean_threshold_fast_inner_into(image: &GrayImage, window: u32, offset: f32, invert: bool, buffers: &mut AdaptiveBuffers, dst: &mut [u8]) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    let expected_len = (width as usize).saturating_mul(height as usize);
    if dst.len() != expected_len {
        return;
    }

    let window = ensure_odd_window(window);
    let radius = (window / 2) as usize;
    let use_parallel = should_use_adaptive_parallel(expected_len);
    log_adaptive_parallel_choice(use_parallel, width, height);
    if use_parallel {
        adaptive_mean_threshold_fast_parallel_into(image, window, radius, offset, invert, buffers, dst);
        return;
    }
    let radius_isize = radius as isize;
    let width_usize = width as usize;
    let height_usize = height as usize;
    let src = image.as_raw();
    let use_neon = false;

    let ring_rows = window as usize;
    let ring_len = width_usize.saturating_mul(ring_rows);
    if buffers.hsum.len() != ring_len {
        buffers.hsum.resize(ring_len, 0u16);
        crate::diagnostics::report_scratch_high_water("image.adaptive_hsum", buffers.hsum.capacity() * size_of::<u16>());
    }
    if buffers.col_sum.len() != width_usize {
        buffers.col_sum.resize(width_usize, 0);
        crate::diagnostics::report_scratch_high_water("image.adaptive_col_sum", buffers.col_sum.capacity() * size_of::<i32>());
    }

    let area = (window as i32) * (window as i32);
    let area_half = area / 2;
    let inv_area = (((1i64) << ADAPTIVE_SHIFT) + (area as i64 / 2)) / (area as i64);
    let inv_area = inv_area as i32;
    let area_half_scaled = area_half.saturating_mul(inv_area);
    let offset_scaled = (offset * ((1i64 << ADAPTIVE_SHIFT) as f32)).round() as i32;
    let hsum_ring = &mut buffers.hsum;
    let col_sum = &mut buffers.col_sum;

    let clamp_y = |y: isize| -> usize {
        if y < 0 {
            0
        } else if y as usize >= height_usize {
            height_usize - 1
        } else {
            y as usize
        }
    };

    let initial_max_y = radius.min(height_usize.saturating_sub(1));
    for y in 0..=initial_max_y {
        let src_row = &src[y * width_usize..(y + 1) * width_usize];
        let slot = (y % ring_rows) * width_usize;
        compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
    }

    col_sum.fill(0);
    if use_neon {
        #[cfg(target_arch = "aarch64")]
        {
            for dy in -radius_isize..=radius_isize {
                let yy = clamp_y(dy);
                let slot = (yy % ring_rows) * width_usize;
                let row = &hsum_ring[slot..slot + width_usize];
                // SAFETY: guarded by runtime feature detection.
                unsafe {
                    neon::add_u16_row_to_i32_col_sum_neon(col_sum, row);
                }
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        unreachable!("neon-enabled path on non-aarch64");
    } else {
        for dy in -radius_isize..=radius_isize {
            let yy = clamp_y(dy);
            let slot = (yy % ring_rows) * width_usize;
            let row = &hsum_ring[slot..slot + width_usize];
            add_u16_row_to_col_sum_scalar(col_sum, row);
        }
    }

    let mut computed_up_to = initial_max_y;
    let mut inner_start = radius;
    if inner_start > height_usize {
        inner_start = height_usize;
    }
    let mut inner_end = height_usize.saturating_sub(radius + 1);
    if inner_end < inner_start {
        inner_end = inner_start;
    }

    if use_neon {
        #[cfg(target_arch = "aarch64")]
        {
            for y in 0..inner_start {
                let src_row = &src[y * width_usize..(y + 1) * width_usize];
                let dst_row = &mut dst[y * width_usize..(y + 1) * width_usize];

                let sub_y = clamp_y(y as isize - radius_isize);
                let add_y = clamp_y(y as isize + radius_isize + 1);
                if add_y > computed_up_to {
                    for yy in (computed_up_to + 1)..=add_y {
                        let src_row = &src[yy * width_usize..(yy + 1) * width_usize];
                        let slot = (yy % ring_rows) * width_usize;
                        compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
                    }
                    computed_up_to = add_y;
                }
                let sub_slot = (sub_y % ring_rows) * width_usize;
                let sub_row = &hsum_ring[sub_slot..sub_slot + width_usize];
                let add_slot = (add_y % ring_rows) * width_usize;
                let add_row = &hsum_ring[add_slot..add_slot + width_usize];

                // SAFETY: guarded by runtime feature detection.
                unsafe {
                    neon::adaptive_threshold_row_neon(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                }
            }
            for y in inner_start..inner_end {
                let src_row = &src[y * width_usize..(y + 1) * width_usize];
                let dst_row = &mut dst[y * width_usize..(y + 1) * width_usize];

                let sub_y = y - radius;
                let add_y = y + radius + 1;
                if add_y > computed_up_to {
                    let src_row = &src[add_y * width_usize..(add_y + 1) * width_usize];
                    let slot = (add_y % ring_rows) * width_usize;
                    compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
                    computed_up_to = add_y;
                }
                let sub_slot = (sub_y % ring_rows) * width_usize;
                let sub_row = &hsum_ring[sub_slot..sub_slot + width_usize];
                let add_slot = (add_y % ring_rows) * width_usize;
                let add_row = &hsum_ring[add_slot..add_slot + width_usize];

                // SAFETY: guarded by runtime feature detection.
                unsafe {
                    neon::adaptive_threshold_row_neon(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                }
            }
            for y in inner_end..height_usize {
                let src_row = &src[y * width_usize..(y + 1) * width_usize];
                let dst_row = &mut dst[y * width_usize..(y + 1) * width_usize];

                let sub_y = clamp_y(y as isize - radius_isize);
                let add_y = clamp_y(y as isize + radius_isize + 1);
                if add_y > computed_up_to {
                    for yy in (computed_up_to + 1)..=add_y {
                        let src_row = &src[yy * width_usize..(yy + 1) * width_usize];
                        let slot = (yy % ring_rows) * width_usize;
                        compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
                    }
                    computed_up_to = add_y;
                }
                let sub_slot = (sub_y % ring_rows) * width_usize;
                let sub_row = &hsum_ring[sub_slot..sub_slot + width_usize];
                let add_slot = (add_y % ring_rows) * width_usize;
                let add_row = &hsum_ring[add_slot..add_slot + width_usize];

                // SAFETY: guarded by runtime feature detection.
                unsafe {
                    neon::adaptive_threshold_row_neon(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                }
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        unreachable!("neon-enabled path on non-aarch64");
    } else {
        for y in 0..inner_start {
            let src_row = &src[y * width_usize..(y + 1) * width_usize];
            let dst_row = &mut dst[y * width_usize..(y + 1) * width_usize];

            let sub_y = clamp_y(y as isize - radius_isize);
            let add_y = clamp_y(y as isize + radius_isize + 1);
            if add_y > computed_up_to {
                for yy in (computed_up_to + 1)..=add_y {
                    let src_row = &src[yy * width_usize..(yy + 1) * width_usize];
                    let slot = (yy % ring_rows) * width_usize;
                    compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
                }
                computed_up_to = add_y;
            }
            let sub_slot = (sub_y % ring_rows) * width_usize;
            let sub_row = &hsum_ring[sub_slot..sub_slot + width_usize];
            let add_slot = (add_y % ring_rows) * width_usize;
            let add_row = &hsum_ring[add_slot..add_slot + width_usize];

            adaptive_threshold_row_scalar(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
        }
        for y in inner_start..inner_end {
            let src_row = &src[y * width_usize..(y + 1) * width_usize];
            let dst_row = &mut dst[y * width_usize..(y + 1) * width_usize];

            let sub_y = y - radius;
            let add_y = y + radius + 1;
            if add_y > computed_up_to {
                let src_row = &src[add_y * width_usize..(add_y + 1) * width_usize];
                let slot = (add_y % ring_rows) * width_usize;
                compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
                computed_up_to = add_y;
            }
            let sub_slot = (sub_y % ring_rows) * width_usize;
            let sub_row = &hsum_ring[sub_slot..sub_slot + width_usize];
            let add_slot = (add_y % ring_rows) * width_usize;
            let add_row = &hsum_ring[add_slot..add_slot + width_usize];

            adaptive_threshold_row_scalar(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
        }
        for y in inner_end..height_usize {
            let src_row = &src[y * width_usize..(y + 1) * width_usize];
            let dst_row = &mut dst[y * width_usize..(y + 1) * width_usize];

            let sub_y = clamp_y(y as isize - radius_isize);
            let add_y = clamp_y(y as isize + radius_isize + 1);
            if add_y > computed_up_to {
                for yy in (computed_up_to + 1)..=add_y {
                    let src_row = &src[yy * width_usize..(yy + 1) * width_usize];
                    let slot = (yy % ring_rows) * width_usize;
                    compute_horizontal_hsum_row_u16(src_row, radius, &mut hsum_ring[slot..slot + width_usize], use_neon);
                }
                computed_up_to = add_y;
            }
            let sub_slot = (sub_y % ring_rows) * width_usize;
            let sub_row = &hsum_ring[sub_slot..sub_slot + width_usize];
            let add_slot = (add_y % ring_rows) * width_usize;
            let add_row = &hsum_ring[add_slot..add_slot + width_usize];

            adaptive_threshold_row_scalar(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
        }
    }
}

fn adaptive_mean_threshold_fast_parallel_into(image: &GrayImage, window: u32, radius: usize, offset: f32, invert: bool, buffers: &mut AdaptiveBuffers, dst: &mut [u8]) {
    let (width, height) = image.dimensions();
    let width_usize = width as usize;
    let height_usize = height as usize;
    if width_usize == 0 || height_usize == 0 {
        return;
    }
    let src = image.as_raw();
    let hsum_len = width_usize.saturating_mul(height_usize);
    if buffers.hsum.len() != hsum_len {
        buffers.hsum.resize(hsum_len, 0u16);
        crate::diagnostics::report_scratch_high_water("image.adaptive_hsum", buffers.hsum.capacity() * size_of::<u16>());
    }
    let area = (window as i32) * (window as i32);
    let area_half = area / 2;
    let inv_area = (((1i64) << ADAPTIVE_SHIFT) + (area as i64 / 2)) / (area as i64);
    let inv_area = inv_area as i32;
    let area_half_scaled = area_half.saturating_mul(inv_area);
    let offset_scaled = (offset * ((1i64 << ADAPTIVE_SHIFT) as f32)).round() as i32;
    let radius_isize = radius as isize;
    let use_neon = false;

    let pool = adaptive_thread_pool();
    pool.install(|| {
        {
            let hsum = &mut buffers.hsum;
            hsum.par_chunks_mut(width_usize).enumerate().for_each(|(y, row)| {
                let src_row = &src[y * width_usize..(y + 1) * width_usize];
                compute_horizontal_hsum_row_u16(src_row, radius, row, use_neon);
            });
        }

        let hsum = &buffers.hsum;
        let threads = pool.current_num_threads().max(1);
        let mut rows_per_chunk = height_usize.div_ceil(threads);
        rows_per_chunk = rows_per_chunk.max(window as usize).max(32);

        let clamp_y = |y: isize| -> usize {
            if y < 0 {
                0
            } else if y as usize >= height_usize {
                height_usize - 1
            } else {
                y as usize
            }
        };

        dst.par_chunks_mut(width_usize * rows_per_chunk).enumerate().for_each(|(chunk_idx, dst_chunk)| {
            let y0 = chunk_idx * rows_per_chunk;
            let chunk_rows = dst_chunk.len() / width_usize;
            if chunk_rows == 0 {
                return;
            }
            let chunk_end = (y0 + chunk_rows).min(height_usize);
            let mut inner_start = radius;
            if inner_start > height_usize {
                inner_start = height_usize;
            }
            let mut inner_end = height_usize.saturating_sub(radius + 1);
            if inner_end < inner_start {
                inner_end = inner_start;
            }
            let interior_start = inner_start.max(y0);
            let interior_end = inner_end.min(chunk_end);
            ADAPTIVE_COL_SUM_SCRATCH.with(|scratch| {
                let mut col_sum = scratch.borrow_mut();
                if col_sum.len() != width_usize {
                    col_sum.resize(width_usize, 0);
                    crate::diagnostics::report_scratch_high_water("image.adaptive_parallel_col_sum", col_sum.capacity() * size_of::<i32>());
                } else {
                    col_sum.fill(0);
                }
                let col_sum = col_sum.as_mut_slice();
                if y0 >= radius && y0 + radius < height_usize {
                    if use_neon {
                        #[cfg(target_arch = "aarch64")]
                        {
                            for dy in -radius_isize..=radius_isize {
                                let yy = (y0 as isize + dy) as usize;
                                let row = &hsum[yy * width_usize..(yy + 1) * width_usize];
                                // SAFETY: guarded by runtime feature detection.
                                unsafe {
                                    neon::add_u16_row_to_i32_col_sum_neon(col_sum, row);
                                }
                            }
                        }
                        #[cfg(not(target_arch = "aarch64"))]
                        unreachable!("neon-enabled path on non-aarch64");
                    } else {
                        for dy in -radius_isize..=radius_isize {
                            let yy = (y0 as isize + dy) as usize;
                            let row = &hsum[yy * width_usize..(yy + 1) * width_usize];
                            add_u16_row_to_col_sum_scalar(col_sum, row);
                        }
                    }
                } else if use_neon {
                    #[cfg(target_arch = "aarch64")]
                    {
                        for dy in -radius_isize..=radius_isize {
                            let yy = clamp_y(y0 as isize + dy);
                            let row = &hsum[yy * width_usize..(yy + 1) * width_usize];
                            // SAFETY: guarded by runtime feature detection.
                            unsafe {
                                neon::add_u16_row_to_i32_col_sum_neon(col_sum, row);
                            }
                        }
                    }
                    #[cfg(not(target_arch = "aarch64"))]
                    unreachable!("neon-enabled path on non-aarch64");
                } else {
                    for dy in -radius_isize..=radius_isize {
                        let yy = clamp_y(y0 as isize + dy);
                        let row = &hsum[yy * width_usize..(yy + 1) * width_usize];
                        add_u16_row_to_col_sum_scalar(col_sum, row);
                    }
                }

                if use_neon {
                    #[cfg(target_arch = "aarch64")]
                    {
                        for y in y0..interior_start {
                            let local_y = y.saturating_sub(y0);
                            let src_row = &src[y * width_usize..(y + 1) * width_usize];
                            let dst_row = &mut dst_chunk[local_y * width_usize..(local_y + 1) * width_usize];
                            let sub_y = clamp_y(y as isize - radius_isize);
                            let add_y = clamp_y(y as isize + radius_isize + 1);
                            let sub_row = &hsum[sub_y * width_usize..(sub_y + 1) * width_usize];
                            let add_row = &hsum[add_y * width_usize..(add_y + 1) * width_usize];

                            // SAFETY: guarded by runtime feature detection.
                            unsafe {
                                neon::adaptive_threshold_row_neon(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                            }
                        }

                        for y in interior_start..interior_end {
                            let local_y = y.saturating_sub(y0);
                            let src_row = &src[y * width_usize..(y + 1) * width_usize];
                            let dst_row = &mut dst_chunk[local_y * width_usize..(local_y + 1) * width_usize];
                            let sub_y = y - radius;
                            let add_y = y + radius + 1;
                            let sub_row = &hsum[sub_y * width_usize..(sub_y + 1) * width_usize];
                            let add_row = &hsum[add_y * width_usize..(add_y + 1) * width_usize];

                            // SAFETY: guarded by runtime feature detection.
                            unsafe {
                                neon::adaptive_threshold_row_neon(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                            }
                        }

                        for y in interior_end..chunk_end {
                            let local_y = y.saturating_sub(y0);
                            let src_row = &src[y * width_usize..(y + 1) * width_usize];
                            let dst_row = &mut dst_chunk[local_y * width_usize..(local_y + 1) * width_usize];
                            let sub_y = clamp_y(y as isize - radius_isize);
                            let add_y = clamp_y(y as isize + radius_isize + 1);
                            let sub_row = &hsum[sub_y * width_usize..(sub_y + 1) * width_usize];
                            let add_row = &hsum[add_y * width_usize..(add_y + 1) * width_usize];

                            // SAFETY: guarded by runtime feature detection.
                            unsafe {
                                neon::adaptive_threshold_row_neon(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                            }
                        }
                    }
                    #[cfg(not(target_arch = "aarch64"))]
                    unreachable!("neon-enabled path on non-aarch64");
                } else {
                    for y in y0..interior_start {
                        let local_y = y.saturating_sub(y0);
                        let src_row = &src[y * width_usize..(y + 1) * width_usize];
                        let dst_row = &mut dst_chunk[local_y * width_usize..(local_y + 1) * width_usize];
                        let sub_y = clamp_y(y as isize - radius_isize);
                        let add_y = clamp_y(y as isize + radius_isize + 1);
                        let sub_row = &hsum[sub_y * width_usize..(sub_y + 1) * width_usize];
                        let add_row = &hsum[add_y * width_usize..(add_y + 1) * width_usize];

                        adaptive_threshold_row_scalar(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                    }

                    for y in interior_start..interior_end {
                        let local_y = y.saturating_sub(y0);
                        let src_row = &src[y * width_usize..(y + 1) * width_usize];
                        let dst_row = &mut dst_chunk[local_y * width_usize..(local_y + 1) * width_usize];
                        let sub_y = y - radius;
                        let add_y = y + radius + 1;
                        let sub_row = &hsum[sub_y * width_usize..(sub_y + 1) * width_usize];
                        let add_row = &hsum[add_y * width_usize..(add_y + 1) * width_usize];

                        adaptive_threshold_row_scalar(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                    }

                    for y in interior_end..chunk_end {
                        let local_y = y.saturating_sub(y0);
                        let src_row = &src[y * width_usize..(y + 1) * width_usize];
                        let dst_row = &mut dst_chunk[local_y * width_usize..(local_y + 1) * width_usize];
                        let sub_y = clamp_y(y as isize - radius_isize);
                        let add_y = clamp_y(y as isize + radius_isize + 1);
                        let sub_row = &hsum[sub_y * width_usize..(sub_y + 1) * width_usize];
                        let add_row = &hsum[add_y * width_usize..(add_y + 1) * width_usize];

                        adaptive_threshold_row_scalar(dst_row, src_row, col_sum, sub_row, add_row, invert, area_half_scaled, inv_area, offset_scaled);
                    }
                }
            });
        });
    });
}
