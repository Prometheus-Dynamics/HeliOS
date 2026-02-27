#![allow(unsafe_code)]

use core::arch::aarch64::*;

const ADAPTIVE_SHIFT: i32 = 16;

#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
pub unsafe fn threshold_to_mask_neon(dst: &mut [u8], src: &[u8], threshold: u8) {
    debug_assert_eq!(dst.len(), src.len());
    let len = src.len();
    if len == 0 {
        return;
    }

    let thr = vdupq_n_u8(threshold);
    let mut i = 0usize;
    while i + 64 <= len {
        let s0 = vld1q_u8(src.as_ptr().add(i));
        let s1 = vld1q_u8(src.as_ptr().add(i + 16));
        let s2 = vld1q_u8(src.as_ptr().add(i + 32));
        let s3 = vld1q_u8(src.as_ptr().add(i + 48));
        vst1q_u8(dst.as_mut_ptr().add(i), vcgtq_u8(s0, thr));
        vst1q_u8(dst.as_mut_ptr().add(i + 16), vcgtq_u8(s1, thr));
        vst1q_u8(dst.as_mut_ptr().add(i + 32), vcgtq_u8(s2, thr));
        vst1q_u8(dst.as_mut_ptr().add(i + 48), vcgtq_u8(s3, thr));
        i += 64;
    }
    while i + 16 <= len {
        let s = vld1q_u8(src.as_ptr().add(i));
        vst1q_u8(dst.as_mut_ptr().add(i), vcgtq_u8(s, thr));
        i += 16;
    }
    while i < len {
        dst[i] = if src[i] > threshold { 255 } else { 0 };
        i += 1;
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
pub unsafe fn threshold_to_mask_with_invert_neon(dst: &mut [u8], src: &[u8], threshold: u8, invert: bool) {
    debug_assert_eq!(dst.len(), src.len());
    let len = src.len();
    if len == 0 {
        return;
    }

    let thr = vdupq_n_u8(threshold);
    let mut i = 0usize;
    if invert {
        while i + 64 <= len {
            let s0 = vld1q_u8(src.as_ptr().add(i));
            let s1 = vld1q_u8(src.as_ptr().add(i + 16));
            let s2 = vld1q_u8(src.as_ptr().add(i + 32));
            let s3 = vld1q_u8(src.as_ptr().add(i + 48));
            vst1q_u8(dst.as_mut_ptr().add(i), vmvnq_u8(vcgtq_u8(s0, thr)));
            vst1q_u8(dst.as_mut_ptr().add(i + 16), vmvnq_u8(vcgtq_u8(s1, thr)));
            vst1q_u8(dst.as_mut_ptr().add(i + 32), vmvnq_u8(vcgtq_u8(s2, thr)));
            vst1q_u8(dst.as_mut_ptr().add(i + 48), vmvnq_u8(vcgtq_u8(s3, thr)));
            i += 64;
        }
        while i + 16 <= len {
            let s = vld1q_u8(src.as_ptr().add(i));
            vst1q_u8(dst.as_mut_ptr().add(i), vmvnq_u8(vcgtq_u8(s, thr)));
            i += 16;
        }
    } else {
        while i + 64 <= len {
            let s0 = vld1q_u8(src.as_ptr().add(i));
            let s1 = vld1q_u8(src.as_ptr().add(i + 16));
            let s2 = vld1q_u8(src.as_ptr().add(i + 32));
            let s3 = vld1q_u8(src.as_ptr().add(i + 48));
            vst1q_u8(dst.as_mut_ptr().add(i), vcgtq_u8(s0, thr));
            vst1q_u8(dst.as_mut_ptr().add(i + 16), vcgtq_u8(s1, thr));
            vst1q_u8(dst.as_mut_ptr().add(i + 32), vcgtq_u8(s2, thr));
            vst1q_u8(dst.as_mut_ptr().add(i + 48), vcgtq_u8(s3, thr));
            i += 64;
        }
        while i + 16 <= len {
            let s = vld1q_u8(src.as_ptr().add(i));
            vst1q_u8(dst.as_mut_ptr().add(i), vcgtq_u8(s, thr));
            i += 16;
        }
    }
    while i < len {
        dst[i] = if invert {
            if src[i] > threshold { 0 } else { 255 }
        } else if src[i] > threshold {
            255
        } else {
            0
        };
        i += 1;
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn prefix4_i32_with_zero(v: int32x4_t, zero: int32x4_t) -> int32x4_t {
    let t = vaddq_s32(v, vextq_s32(zero, v, 3));
    vaddq_s32(t, vextq_s32(zero, t, 2))
}

#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
pub unsafe fn horizontal_hsum_row_u16_neon(src_row: &[u8], radius: usize, out: &mut [u16], start_x: usize, end_x: usize, mut sum: i32) -> i32 {
    debug_assert!(start_x <= end_x);
    debug_assert!(end_x <= src_row.len());
    debug_assert_eq!(src_row.len(), out.len());

    let add_base = radius + 1;
    let sub_base = radius;
    let src_ptr = src_row.as_ptr();
    let mut out_ptr = out.as_mut_ptr().add(start_x);
    let mut add_ptr = src_ptr.add(start_x + add_base);
    let mut sub_ptr = src_ptr.add(start_x - sub_base);
    let mut remaining = end_x - start_x;
    let zero = vdupq_n_s32(0);

    // Guard the left edge with scalar updates. This avoids startup-lane artifacts in the first
    // SIMD block without materially affecting throughput.
    let scalar_prefix = remaining.min(16);
    for _ in 0..scalar_prefix {
        *out_ptr = sum as u16;
        sum += (*add_ptr as i32) - (*sub_ptr as i32);
        out_ptr = out_ptr.add(1);
        add_ptr = add_ptr.add(1);
        sub_ptr = sub_ptr.add(1);
        remaining -= 1;
    }

    macro_rules! hsum_block {
        () => {{
            let add_vec = vld1q_u8(add_ptr);
            let sub_vec = vld1q_u8(sub_ptr);

            let add_lo = vmovl_u8(vget_low_u8(add_vec));
            let add_hi = vmovl_u8(vget_high_u8(add_vec));
            let sub_lo = vmovl_u8(vget_low_u8(sub_vec));
            let sub_hi = vmovl_u8(vget_high_u8(sub_vec));

            let delta_lo = vsubq_s16(vreinterpretq_s16_u16(add_lo), vreinterpretq_s16_u16(sub_lo));
            let delta_hi = vsubq_s16(vreinterpretq_s16_u16(add_hi), vreinterpretq_s16_u16(sub_hi));

            let d0 = vmovl_s16(vget_low_s16(delta_lo));
            let d1 = vmovl_s16(vget_high_s16(delta_lo));
            let d2 = vmovl_s16(vget_low_s16(delta_hi));
            let d3 = vmovl_s16(vget_high_s16(delta_hi));

            let p0 = prefix4_i32_with_zero(d0, zero);
            let mut p1 = prefix4_i32_with_zero(d1, zero);
            p1 = vaddq_s32(p1, vdupq_n_s32(vgetq_lane_s32(p0, 3)));
            let mut p2 = prefix4_i32_with_zero(d2, zero);
            p2 = vaddq_s32(p2, vdupq_n_s32(vgetq_lane_s32(p1, 3)));
            let mut p3 = prefix4_i32_with_zero(d3, zero);
            p3 = vaddq_s32(p3, vdupq_n_s32(vgetq_lane_s32(p2, 3)));

            let sum_vec = vdupq_n_s32(sum);
            let s0 = vaddq_s32(sum_vec, vextq_s32(zero, p0, 3));
            let s1 = vaddq_s32(sum_vec, vextq_s32(p0, p1, 3));
            let s2 = vaddq_s32(sum_vec, vextq_s32(p1, p2, 3));
            let s3 = vaddq_s32(sum_vec, vextq_s32(p2, p3, 3));

            let s0_u16 = vqmovun_s32(s0);
            let s1_u16 = vqmovun_s32(s1);
            let s2_u16 = vqmovun_s32(s2);
            let s3_u16 = vqmovun_s32(s3);

            let out0 = vcombine_u16(s0_u16, s1_u16);
            let out1 = vcombine_u16(s2_u16, s3_u16);
            vst1q_u16(out_ptr, out0);
            vst1q_u16(out_ptr.add(8), out1);

            sum += vgetq_lane_s32(p3, 3);
        }};
    }

    while remaining >= 64 {
        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);

        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);

        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);

        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);
        remaining -= 64;
    }

    while remaining >= 32 {
        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);

        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);
        remaining -= 32;
    }

    while remaining >= 16 {
        hsum_block!();
        out_ptr = out_ptr.add(16);
        add_ptr = add_ptr.add(16);
        sub_ptr = sub_ptr.add(16);
        remaining -= 16;
    }

    while remaining > 0 {
        *out_ptr = sum as u16;
        sum += (*add_ptr as i32) - (*sub_ptr as i32);
        out_ptr = out_ptr.add(1);
        add_ptr = add_ptr.add(1);
        sub_ptr = sub_ptr.add(1);
        remaining -= 1;
    }

    sum
}

#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
pub unsafe fn adaptive_threshold_row_neon(
    dst_row: &mut [u8],
    src_row: &[u8],
    col_sum: &mut [i32],
    sub_row: &[u16],
    add_row: &[u16],
    invert: bool,
    area_half_scaled: i32,
    inv_area: i32,
    offset_scaled: i32,
) {
    debug_assert_eq!(dst_row.len(), src_row.len());
    debug_assert_eq!(dst_row.len(), col_sum.len());
    debug_assert_eq!(dst_row.len(), sub_row.len());
    debug_assert_eq!(dst_row.len(), add_row.len());

    let width = dst_row.len();
    if width == 0 {
        return;
    }

    let inv_area_vec = vdupq_n_s32(inv_area);
    let area_half_scaled_vec = vdupq_n_s32(area_half_scaled);
    let offset_vec = vdupq_n_s32(offset_scaled);

    let dst_ptr = dst_row.as_mut_ptr();
    let src_ptr = src_row.as_ptr();
    let col_ptr = col_sum.as_mut_ptr() as *mut u32;
    let sub_ptr = sub_row.as_ptr();
    let add_ptr = add_row.as_ptr();

    // Compute a small left-edge prefix with scalar math. This prevents deterministic left-side
    // striping when SIMD startup lanes interact with border-conditioned sums.
    let scalar_prefix = width.min(16);
    for xx in 0..scalar_prefix {
        let sum_i32 = col_sum[xx];
        let mean_scaled = sum_i32 * inv_area + area_half_scaled;
        let threshold_scaled = mean_scaled - offset_scaled;
        let src_scaled = (src_row[xx] as i32) << ADAPTIVE_SHIFT;
        let mut is_foreground = src_scaled > threshold_scaled;
        if invert {
            is_foreground = !is_foreground;
        }
        dst_row[xx] = if is_foreground { 255 } else { 0 };
        col_sum[xx] = sum_i32 - sub_row[xx] as i32 + add_row[xx] as i32;
    }

    macro_rules! threshold_block {
        ($dst:expr, $src:expr, $col:expr, $sub:expr, $add:expr, $invert:expr) => {{
            let src_vec = vld1q_u8($src);

            let col0_u = vld1q_u32($col);
            let col1_u = vld1q_u32($col.add(4));
            let col2_u = vld1q_u32($col.add(8));
            let col3_u = vld1q_u32($col.add(12));

            let col0 = vreinterpretq_s32_u32(col0_u);
            let col1 = vreinterpretq_s32_u32(col1_u);
            let col2 = vreinterpretq_s32_u32(col2_u);
            let col3 = vreinterpretq_s32_u32(col3_u);

            let mean0 = vmlaq_s32(area_half_scaled_vec, col0, inv_area_vec);
            let mean1 = vmlaq_s32(area_half_scaled_vec, col1, inv_area_vec);
            let mean2 = vmlaq_s32(area_half_scaled_vec, col2, inv_area_vec);
            let mean3 = vmlaq_s32(area_half_scaled_vec, col3, inv_area_vec);

            let thr0 = vsubq_s32(mean0, offset_vec);
            let thr1 = vsubq_s32(mean1, offset_vec);
            let thr2 = vsubq_s32(mean2, offset_vec);
            let thr3 = vsubq_s32(mean3, offset_vec);

            let src_u16_lo = vmovl_u8(vget_low_u8(src_vec));
            let src_u16_hi = vmovl_u8(vget_high_u8(src_vec));

            let src_scaled0 = vreinterpretq_s32_u32(vshll_n_u16(vget_low_u16(src_u16_lo), ADAPTIVE_SHIFT));
            let src_scaled1 = vreinterpretq_s32_u32(vshll_n_u16(vget_high_u16(src_u16_lo), ADAPTIVE_SHIFT));
            let src_scaled2 = vreinterpretq_s32_u32(vshll_n_u16(vget_low_u16(src_u16_hi), ADAPTIVE_SHIFT));
            let src_scaled3 = vreinterpretq_s32_u32(vshll_n_u16(vget_high_u16(src_u16_hi), ADAPTIVE_SHIFT));

            let mask0 = vcgtq_s32(src_scaled0, thr0);
            let mask1 = vcgtq_s32(src_scaled1, thr1);
            let mask2 = vcgtq_s32(src_scaled2, thr2);
            let mask3 = vcgtq_s32(src_scaled3, thr3);

            let m16_0 = vqmovn_u32(mask0);
            let m16_1 = vqmovn_u32(mask1);
            let m16_2 = vqmovn_u32(mask2);
            let m16_3 = vqmovn_u32(mask3);

            let m8_0 = vqmovn_u16(vcombine_u16(m16_0, m16_1));
            let m8_1 = vqmovn_u16(vcombine_u16(m16_2, m16_3));
            let out = vcombine_u8(m8_0, m8_1);
            let out = if $invert { vmvnq_u8(out) } else { out };
            vst1q_u8($dst, out);

            let sub0 = vld1q_u16($sub);
            let sub1 = vld1q_u16($sub.add(8));
            let add0 = vld1q_u16($add);
            let add1 = vld1q_u16($add.add(8));

            let col0_sub = vsubw_u16(col0_u, vget_low_u16(sub0));
            let col1_sub = vsubw_u16(col1_u, vget_high_u16(sub0));
            let col2_sub = vsubw_u16(col2_u, vget_low_u16(sub1));
            let col3_sub = vsubw_u16(col3_u, vget_high_u16(sub1));

            let col0_new = vaddw_u16(col0_sub, vget_low_u16(add0));
            let col1_new = vaddw_u16(col1_sub, vget_high_u16(add0));
            let col2_new = vaddw_u16(col2_sub, vget_low_u16(add1));
            let col3_new = vaddw_u16(col3_sub, vget_high_u16(add1));

            vst1q_u32($col, col0_new);
            vst1q_u32($col.add(4), col1_new);
            vst1q_u32($col.add(8), col2_new);
            vst1q_u32($col.add(12), col3_new);
        }};
    }

    let mut x = scalar_prefix;
    if invert {
        let mut dst = dst_ptr.add(x);
        let mut src = src_ptr.add(x);
        let mut col = col_ptr.add(x);
        let mut sub = sub_ptr.add(x);
        let mut add = add_ptr.add(x);
        while x + 64 <= width {
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            x += 64;
        }
        while x + 32 <= width {
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            x += 32;
        }
        while x + 16 <= width {
            threshold_block!(dst, src, col, sub, add, true);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            x += 16;
        }
    } else {
        let mut dst = dst_ptr.add(x);
        let mut src = src_ptr.add(x);
        let mut col = col_ptr.add(x);
        let mut sub = sub_ptr.add(x);
        let mut add = add_ptr.add(x);
        while x + 64 <= width {
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            x += 64;
        }
        while x + 32 <= width {
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            x += 32;
        }
        while x + 16 <= width {
            threshold_block!(dst, src, col, sub, add, false);
            dst = dst.add(16);
            src = src.add(16);
            col = col.add(16);
            sub = sub.add(16);
            add = add.add(16);
            x += 16;
        }
    }

    for xx in x..width {
        let sum_i32 = col_sum[xx];
        let mean_scaled = sum_i32 * inv_area + area_half_scaled;
        let threshold_scaled = mean_scaled - offset_scaled;
        let src_scaled = (src_row[xx] as i32) << ADAPTIVE_SHIFT;
        let mut is_foreground = src_scaled > threshold_scaled;
        if invert {
            is_foreground = !is_foreground;
        }
        dst_row[xx] = if is_foreground { 255 } else { 0 };
        col_sum[xx] = sum_i32 - sub_row[xx] as i32 + add_row[xx] as i32;
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
pub unsafe fn add_u16_row_to_i32_col_sum_neon(col_sum: &mut [i32], row: &[u16]) {
    debug_assert_eq!(col_sum.len(), row.len());
    let width = row.len();
    let col_ptr = col_sum.as_mut_ptr() as *mut u32;
    let mut x = 0usize;
    while x + 16 <= width {
        let row_vec0 = vld1q_u16(row.as_ptr().add(x));
        let row_vec1 = vld1q_u16(row.as_ptr().add(x + 8));
        let col0 = vld1q_u32(col_ptr.add(x));
        let col1 = vld1q_u32(col_ptr.add(x + 4));
        let col2 = vld1q_u32(col_ptr.add(x + 8));
        let col3 = vld1q_u32(col_ptr.add(x + 12));

        let col0 = vaddw_u16(col0, vget_low_u16(row_vec0));
        let col1 = vaddw_u16(col1, vget_high_u16(row_vec0));
        let col2 = vaddw_u16(col2, vget_low_u16(row_vec1));
        let col3 = vaddw_u16(col3, vget_high_u16(row_vec1));

        vst1q_u32(col_ptr.add(x), col0);
        vst1q_u32(col_ptr.add(x + 4), col1);
        vst1q_u32(col_ptr.add(x + 8), col2);
        vst1q_u32(col_ptr.add(x + 12), col3);

        x += 16;
    }
    while x + 8 <= width {
        let row_vec = vld1q_u16(row.as_ptr().add(x));
        let col0 = vld1q_u32(col_ptr.add(x));
        let col1 = vld1q_u32(col_ptr.add(x + 4));

        let col0 = vaddw_u16(col0, vget_low_u16(row_vec));
        let col1 = vaddw_u16(col1, vget_high_u16(row_vec));

        vst1q_u32(col_ptr.add(x), col0);
        vst1q_u32(col_ptr.add(x + 4), col1);

        x += 8;
    }

    for xx in x..width {
        col_sum[xx] += row[xx] as i32;
    }
}
