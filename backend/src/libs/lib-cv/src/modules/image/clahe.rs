#![allow(unsafe_code)]

use image::GrayImage;
use rayon::prelude::*;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::OnceLock;

thread_local! {
    static VERT_LUT_SCRATCH: RefCell<Vec<u16>> = const { RefCell::new(Vec::new()) };
}

const VERT_LUT_RETAIN_CAP: usize = 128 * 256;

const CLAHE_LUT_PAR_MIN_TILES: usize = 24;
const CLAHE_APPLY_PAR_MIN_PIXELS_DEFAULT: usize = 1024 * 768;

fn clahe_apply_parallel_min_pixels() -> usize {
    static MIN_PIXELS: OnceLock<usize> = OnceLock::new();
    *MIN_PIXELS.get_or_init(|| std::env::var("HELIOS_CLAHE_PARALLEL_MIN_PIXELS").ok().and_then(|value| value.parse::<usize>().ok()).unwrap_or(CLAHE_APPLY_PAR_MIN_PIXELS_DEFAULT))
}

fn clahe_should_parallelize(width: u32, height: u32) -> bool {
    (width as usize).saturating_mul(height as usize) >= clahe_apply_parallel_min_pixels() && rayon::current_num_threads() > 1
}

pub struct ClaheTiles {
    pub luts: Vec<[u8; 256]>,
    pub tiles_x: u32,
    pub tiles_y: u32,
    pub tile_w: u32,
    pub tile_h: u32,
}

/// Precompute per-tile LUTs for CLAHE.
///
/// `tile_size` is the requested tile grid size (number of tiles per axis),
/// matching OpenCV's `tileGridSize` semantics (e.g. `4` => ~4x4 tiles).
pub fn prepare_clahe(gray: &GrayImage, tile_size: u32, clip_limit: f32) -> ClaheTiles {
    let width = gray.width();
    let height = gray.height();
    if width == 0 || height == 0 {
        return ClaheTiles { luts: vec![[0u8; 256]], tiles_x: 1, tiles_y: 1, tile_w: 1, tile_h: 1 };
    }

    let grid = tile_size.max(1);
    let tile_w = width.div_ceil(grid).max(4).min(width);
    let tile_h = height.div_ceil(grid).max(4).min(height);
    let tiles_x = width.div_ceil(tile_w).max(1);
    let tiles_y = height.div_ceil(tile_h).max(1);

    let mut luts = vec![[0u8; 256]; (tiles_x * tiles_y) as usize];
    if luts.len() >= CLAHE_LUT_PAR_MIN_TILES && clahe_should_parallelize(width, height) {
        luts.par_iter_mut().enumerate().with_min_len(8).for_each(|(idx, lut)| {
            let ty = idx as u32 / tiles_x;
            let tx = idx as u32 % tiles_x;
            let x0 = tx * tile_w;
            let x1 = ((tx + 1) * tile_w).min(width);
            let y0 = ty * tile_h;
            let y1 = ((ty + 1) * tile_h).min(height);
            compute_tile_lut(gray, x0, y0, x1, y1, clip_limit, lut);
        });
    } else {
        for (idx, lut) in luts.iter_mut().enumerate() {
            let ty = idx as u32 / tiles_x;
            let tx = idx as u32 % tiles_x;
            let x0 = tx * tile_w;
            let x1 = ((tx + 1) * tile_w).min(width);
            let y0 = ty * tile_h;
            let y1 = ((ty + 1) * tile_h).min(height);
            compute_tile_lut(gray, x0, y0, x1, y1, clip_limit, lut);
        }
    }

    ClaheTiles { luts, tiles_x, tiles_y, tile_w, tile_h }
}

#[derive(Clone, Copy)]
struct Interp {
    idx0: u32,
    idx1: u32,
    weight_fp: u16,
}

#[derive(Clone)]
struct ClaheCoordCache {
    key: (u32, u32, u32, u32, u32, u32),
    row_interp: Arc<[Interp]>,
    col_weight_fp: Arc<[u16]>,
}

thread_local! {
    static CLAHE_COORD_CACHE: RefCell<Option<ClaheCoordCache>> = const { RefCell::new(None) };
}

pub(crate) fn compact_clahe_scratch_after_frame() {
    compact_clahe_scratch_current_thread();
    rayon::broadcast(|_| {
        compact_clahe_scratch_current_thread();
    });
}

pub(crate) fn release_clahe_scratch_on_idle() {
    release_clahe_scratch_current_thread();
    rayon::broadcast(|_| {
        release_clahe_scratch_current_thread();
    });
}

fn compact_clahe_scratch_current_thread() {
    VERT_LUT_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.clear();
        if scratch.capacity() > VERT_LUT_RETAIN_CAP {
            scratch.shrink_to(VERT_LUT_RETAIN_CAP);
        }
    });
    CLAHE_COORD_CACHE.with(|cache| {
        cache.borrow_mut().take();
    });
}

fn release_clahe_scratch_current_thread() {
    VERT_LUT_SCRATCH.with(|scratch| {
        *scratch.borrow_mut() = Vec::new();
    });
    CLAHE_COORD_CACHE.with(|cache| {
        cache.borrow_mut().take();
    });
}

/// Apply CLAHE (Contrast Limited Adaptive Histogram Equalization) with bilinear blending between tiles.
pub fn apply_clahe(gray: &GrayImage, tile_size: u32, clip_limit: f32) -> GrayImage {
    let tiles = prepare_clahe(gray, tile_size, clip_limit);
    apply_clahe_with_tiles(gray, &tiles)
}

/// Apply CLAHE using precomputed per-tile LUTs.
pub fn apply_clahe_with_tiles(gray: &GrayImage, tiles: &ClaheTiles) -> GrayImage {
    let width = gray.width();
    let height = gray.height();
    let mut output = GrayImage::new(width, height);
    apply_clahe_with_tiles_into(gray, tiles, &mut output);
    output
}

/// Apply CLAHE using precomputed per-tile LUTs into a reusable output image.
pub fn apply_clahe_with_tiles_into(gray: &GrayImage, tiles: &ClaheTiles, output: &mut GrayImage) {
    let width = gray.width();
    let height = gray.height();
    if width == 0 || height == 0 {
        if output.width() != width || output.height() != height {
            *output = GrayImage::new(width, height);
        } else {
            output.as_mut().fill(0);
        }
        return;
    }

    // Fast path: with a single tile there is no spatial interpolation. Apply one LUT directly.
    if tiles.tiles_x == 1 && tiles.tiles_y == 1 {
        let lut = &tiles.luts[0];
        if output.width() != width || output.height() != height {
            *output = GrayImage::new(width, height);
        }
        for (dst, src) in output.as_mut().iter_mut().zip(gray.as_raw().iter()) {
            *dst = lut[*src as usize];
        }
        return;
    }

    let key = (width, height, tiles.tile_w, tiles.tile_h, tiles.tiles_x, tiles.tiles_y);
    let (row_interp, col_weight_fp): (Arc<[Interp]>, Arc<[u16]>) = CLAHE_COORD_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(entry) = cache.as_ref()
            && entry.key == key
        {
            return (entry.row_interp.clone(), entry.col_weight_fp.clone());
        }
        let row_interp: Arc<[Interp]> = Arc::from(build_interp(height, tiles.tile_h, tiles.tiles_y, height));
        let col_weight_fp: Arc<[u16]> = Arc::from(build_weight_fp(width, tiles.tile_w, tiles.tiles_x, width));
        *cache = Some(ClaheCoordCache { key, row_interp: row_interp.clone(), col_weight_fp: col_weight_fp.clone() });
        (row_interp, col_weight_fp)
    });

    let input = gray.as_raw();
    if output.width() != width || output.height() != height {
        *output = GrayImage::new(width, height);
    }
    let out_buf = output.as_mut();
    let width_usize = width as usize;
    #[cfg(target_arch = "aarch64")]
    let use_neon = crate::simd::neon_enabled();

    const ROWS_PER_JOB: usize = 128;

    let apply_rows = |job_idx: usize, rows: &mut [u8]| {
        VERT_LUT_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let tiles_x = tiles.tiles_x as usize;
            let needed = tiles_x.saturating_mul(256);
            if scratch.len() != needed {
                scratch.resize(needed, 0);
            }

            let vert_luts: &mut [u16] = &mut scratch[..];
            let y0 = job_idx * ROWS_PER_JOB;
            let row_count = rows.len() / width_usize;
            let mut last_idx0 = u32::MAX;
            let mut last_idx1 = u32::MAX;
            let mut last_wy = i32::MIN;
            for row_offset in 0..row_count {
                let y = y0 + row_offset;
                if y >= height as usize {
                    break;
                }

                let interp_y = row_interp[y];
                let wy = interp_y.weight_fp as i32;
                if interp_y.idx0 != last_idx0 || interp_y.idx1 != last_idx1 || wy != last_wy {
                    let row0_base = (interp_y.idx0 * tiles.tiles_x) as usize;
                    let row1_base = (interp_y.idx1 * tiles.tiles_x) as usize;
                    let row0_luts = &tiles.luts[row0_base..row0_base + tiles.tiles_x as usize];
                    let row1_luts = &tiles.luts[row1_base..row1_base + tiles.tiles_x as usize];
                    compute_vertical_luts_into(
                        vert_luts,
                        row0_luts,
                        row1_luts,
                        wy,
                        #[cfg(target_arch = "aarch64")]
                        use_neon,
                    );
                    last_idx0 = interp_y.idx0;
                    last_idx1 = interp_y.idx1;
                    last_wy = wy;
                }

                let src_row = &input[y * width_usize..(y + 1) * width_usize];
                let dst_row = &mut rows[row_offset * width_usize..(row_offset + 1) * width_usize];
                apply_clahe_row_segmented_vert(dst_row, src_row, col_weight_fp.as_ref(), vert_luts, tiles.tile_w as usize);
            }
        });
    };

    let chunk_len = width_usize.saturating_mul(ROWS_PER_JOB).max(width_usize);
    if clahe_should_parallelize(width, height) {
        out_buf.par_chunks_mut(chunk_len).enumerate().for_each(|(job_idx, rows)| apply_rows(job_idx, rows));
    } else {
        for (job_idx, rows) in out_buf.chunks_mut(chunk_len).enumerate() {
            apply_rows(job_idx, rows);
        }
    }
}

#[inline]
pub fn blend_clahe_with_base_into(base: &GrayImage, enhanced: &GrayImage, mix: f32, out: &mut GrayImage) {
    let alpha = mix.clamp(0.0, 1.0);
    if out.width() != base.width() || out.height() != base.height() {
        *out = GrayImage::new(base.width(), base.height());
    }
    if alpha >= 0.999 {
        out.as_mut().copy_from_slice(enhanced.as_raw());
        return;
    }
    if alpha <= 0.001 {
        out.as_mut().copy_from_slice(base.as_raw());
        return;
    }

    let a = (alpha * 256.0).round().clamp(0.0, 256.0) as u32;
    let ia = 256u32.saturating_sub(a);
    out.as_mut().par_iter_mut().zip(base.as_raw().par_iter().zip(enhanced.as_raw().par_iter())).for_each(|(dst, (&b, &e))| {
        let mixed = (e as u32).saturating_mul(a).saturating_add((b as u32).saturating_mul(ia)).saturating_add(128) >> 8;
        *dst = mixed as u8;
    });
}

fn build_interp(length: u32, tile_extent: u32, tiles: u32, max_coord: u32) -> Vec<Interp> {
    if tiles == 1 || tile_extent == 0 {
        return vec![Interp { idx0: 0, idx1: 0, weight_fp: 0 }; length as usize];
    }
    (0..length)
        .map(|coord| {
            let idx0 = coord / tile_extent;
            let idx1 = (idx0 + 1).min(tiles - 1);
            if idx0 == idx1 {
                return Interp { idx0, idx1, weight_fp: 0 };
            }
            let start = idx0 * tile_extent;
            let end = if idx1 == tiles - 1 { max_coord.saturating_sub(1) } else { idx1 * tile_extent };
            let denom = (end.max(start + 1) - start) as f32;
            let weight = (coord.saturating_sub(start) as f32 / denom).clamp(0.0, 1.0);
            let weight_fp = (weight * 256.0).round().clamp(0.0, 256.0) as u16;
            Interp { idx0, idx1, weight_fp }
        })
        .collect()
}

fn build_weight_fp(length: u32, tile_extent: u32, tiles: u32, max_coord: u32) -> Vec<u16> {
    if tiles == 1 || tile_extent == 0 {
        return vec![0u16; length as usize];
    }
    (0..length)
        .map(|coord| {
            let idx0 = coord / tile_extent;
            let idx1 = (idx0 + 1).min(tiles - 1);
            if idx0 == idx1 {
                return 0;
            }
            let start = idx0 * tile_extent;
            let end = if idx1 == tiles - 1 { max_coord.saturating_sub(1) } else { idx1 * tile_extent };
            let denom = (end.max(start + 1) - start) as f32;
            let weight = (coord.saturating_sub(start) as f32 / denom).clamp(0.0, 1.0);
            (weight * 256.0).round().clamp(0.0, 256.0) as u16
        })
        .collect()
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn compute_vertical_luts_into(out: &mut [u16], row0_luts: &[[u8; 256]], row1_luts: &[[u8; 256]], wy: i32, use_neon: bool) {
    let tiles_x = row0_luts.len().min(row1_luts.len());
    if tiles_x == 0 {
        return;
    }
    if out.len() < tiles_x.saturating_mul(256) {
        return;
    }
    let wy = wy.clamp(0, 256);

    for tx in 0..tiles_x {
        let dst = &mut out[tx * 256..tx * 256 + 256];
        if use_neon {
            // SAFETY: guarded by runtime feature detection + target_feature.
            unsafe {
                compute_vertical_lut_neon(dst, &row0_luts[tx], &row1_luts[tx], wy);
            }
            continue;
        }
        compute_vertical_lut_scalar(dst, &row0_luts[tx], &row1_luts[tx], wy);
    }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn compute_vertical_luts_into(out: &mut [u16], row0_luts: &[[u8; 256]], row1_luts: &[[u8; 256]], wy: i32) {
    let tiles_x = row0_luts.len().min(row1_luts.len());
    if tiles_x == 0 {
        return;
    }
    if out.len() < tiles_x.saturating_mul(256) {
        return;
    }
    let wy = wy.clamp(0, 256);

    for tx in 0..tiles_x {
        let dst = &mut out[tx * 256..tx * 256 + 256];
        compute_vertical_lut_scalar(dst, &row0_luts[tx], &row1_luts[tx], wy);
    }
}

#[inline(always)]
fn compute_vertical_lut_scalar(out: &mut [u16], top: &[u8; 256], bottom: &[u8; 256], wy: i32) {
    debug_assert_eq!(out.len(), 256);
    debug_assert!((0..=256).contains(&wy));
    for (dst, (&t, &b)) in out.iter_mut().zip(top.iter().zip(bottom.iter())) {
        let t = t as i32;
        let b = b as i32;
        // Fixed-point (Q8): t*256 + (b - t)*wy
        *dst = (t * 256 + (b - t) * wy) as u16;
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn compute_vertical_lut_neon(out: &mut [u16], top: &[u8; 256], bottom: &[u8; 256], wy: i32) {
    use core::arch::aarch64::*;

    debug_assert_eq!(out.len(), 256);
    debug_assert!((0..=256).contains(&wy));
    let wy = wy as i16;

    let out_ptr = out.as_mut_ptr();
    let top_ptr = top.as_ptr();
    let bottom_ptr = bottom.as_ptr();

    // 8 pixels at a time (u8x8 → u16x8).
    for i in (0..256).step_by(8) {
        let t8 = vld1_u8(top_ptr.add(i));
        let b8 = vld1_u8(bottom_ptr.add(i));
        let t16_u = vmovl_u8(t8);
        let b16_u = vmovl_u8(b8);

        // Values are <= 255, so reinterpreting as i16 is safe and keeps the numeric value.
        let t16 = vreinterpretq_s16_u16(t16_u);
        let b16 = vreinterpretq_s16_u16(b16_u);
        let diff16 = vsubq_s16(b16, t16);

        // Q8 fixed-point: t*256 + (b - t)*wy
        let mut lo = vmull_n_s16(vget_low_s16(t16), 256);
        lo = vmlal_n_s16(lo, vget_low_s16(diff16), wy);
        let mut hi = vmull_n_s16(vget_high_s16(t16), 256);
        hi = vmlal_n_s16(hi, vget_high_s16(diff16), wy);

        let res = vcombine_u16(vqmovun_s32(lo), vqmovun_s32(hi));
        vst1q_u16(out_ptr.add(i), res);
    }
}

#[inline(always)]
fn apply_clahe_row_segmented_vert(dst_row: &mut [u8], src_row: &[u8], col_weight_fp: &[u16], vert_luts: &[u16], tile_w: usize) {
    debug_assert_eq!(dst_row.len(), src_row.len());
    debug_assert_eq!(dst_row.len(), col_weight_fp.len());
    debug_assert!(tile_w > 0);

    let width = dst_row.len();
    let tiles_x = vert_luts.len() / 256;
    if tiles_x == 0 {
        dst_row.copy_from_slice(src_row);
        return;
    }

    for tx in 0..tiles_x {
        let idx0 = tx;
        let idx1 = (tx + 1).min(tiles_x.saturating_sub(1));
        let x0 = tx * tile_w;
        if x0 >= width {
            break;
        }
        let x1 = ((tx + 1) * tile_w).min(width);

        let base0 = idx0 * 256;
        let base1 = idx1 * 256;
        let lut0 = &vert_luts[base0..base0 + 256];
        let lut1 = &vert_luts[base1..base1 + 256];

        for x in x0..x1 {
            let wx = col_weight_fp[x] as i32;
            let value = src_row[x] as usize;
            let left = lut0[value] as i32;
            let right = lut1[value] as i32;
            let out_fp16 = (left << 8) + (right - left) * wx;
            dst_row[x] = ((out_fp16 + 0x8000) >> 16) as u8;
        }
    }
}

fn compute_tile_lut(src: &GrayImage, x0: u32, y0: u32, x1: u32, y1: u32, clip_limit: f32, lut: &mut [u8; 256]) {
    let width = src.width() as usize;
    let tile_pixels = ((x1 - x0) * (y1 - y0)).max(1);
    let x0 = x0 as usize;
    let x1 = x1 as usize;
    let mut hist0 = [0u16; 256];
    let mut hist1 = [0u16; 256];
    let mut hist2 = [0u16; 256];
    let mut hist3 = [0u16; 256];
    let mut hist4 = [0u16; 256];
    let mut hist5 = [0u16; 256];
    let mut hist6 = [0u16; 256];
    let mut hist7 = [0u16; 256];

    let src_buf = src.as_raw();
    for y in y0..y1 {
        let y = y as usize;
        let row = &src_buf[y * width + x0..y * width + x1];
        let mut chunks = row.chunks_exact(8);
        for chunk in chunks.by_ref() {
            hist0[chunk[0] as usize] += 1;
            hist1[chunk[1] as usize] += 1;
            hist2[chunk[2] as usize] += 1;
            hist3[chunk[3] as usize] += 1;
            hist4[chunk[4] as usize] += 1;
            hist5[chunk[5] as usize] += 1;
            hist6[chunk[6] as usize] += 1;
            hist7[chunk[7] as usize] += 1;
        }
        for &value in chunks.remainder() {
            hist0[value as usize] += 1;
        }
    }

    let mut hist = [0u32; 256];
    for i in 0..256 {
        hist[i] = hist0[i] as u32 + hist1[i] as u32 + hist2[i] as u32 + hist3[i] as u32 + hist4[i] as u32 + hist5[i] as u32 + hist6[i] as u32 + hist7[i] as u32;
    }

    let avg_per_bin = tile_pixels as f32 / 256.0;
    let clip_value = if clip_limit <= 0.0 { tile_pixels } else { ((clip_limit.max(1.0) * avg_per_bin).round() as u32).max(1) };

    let mut excess = 0u32;
    for bin in hist.iter_mut() {
        if *bin > clip_value {
            excess += *bin - clip_value;
            *bin = clip_value;
        }
    }

    let redistribute = excess / 256;
    let remainder = excess % 256;
    for bin in hist.iter_mut() {
        *bin += redistribute;
    }
    hist.iter_mut().take(remainder as usize).for_each(|bin| *bin += 1);

    let mut cdf_min = 0u32;
    for &count in &hist {
        if count != 0 {
            cdf_min = count;
            break;
        }
    }

    let denom = (tile_pixels - cdf_min).max(1);
    let mut cumulative = 0u32;
    for (idx, out) in lut.iter_mut().enumerate() {
        cumulative = cumulative.saturating_add(hist[idx]);
        let val = cumulative.saturating_sub(cdf_min);
        *out = ((val * 255) / denom) as u8;
    }
}
