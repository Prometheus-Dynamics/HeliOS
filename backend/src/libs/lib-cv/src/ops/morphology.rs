use image::GrayImage;
use imageproc::{distance_transform::Norm, morphology as morph};
use rayon::prelude::*;
use std::cell::RefCell;
use wide::u8x16;

thread_local! {
    // Scratch buffers for fast binary morphology (avoid per-frame allocations).
    static BINARY_SCRATCH: RefCell<BinaryMorphScratch> = RefCell::new(BinaryMorphScratch::default());
}

const BINARY_MORPH_BUFFER_RETAIN_CAP: usize = 2 * 1024 * 1024;

#[derive(Default)]
struct BinaryMorphScratch {
    a: Vec<u8>,
    b: Vec<u8>,
}

#[inline(always)]
fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

pub(crate) fn compact_binary_morph_scratch_after_frame() {
    BINARY_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch.a, BINARY_MORPH_BUFFER_RETAIN_CAP);
        trim_retained_vec(&mut scratch.b, BINARY_MORPH_BUFFER_RETAIN_CAP);
    });
}

pub fn erode(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    morph::erode(image, norm, k)
}

pub fn dilate(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    morph::dilate(image, norm, k)
}

pub fn open(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    match norm {
        Norm::L1 => open_l1(image, k),
        _ => morph::open(image, norm, k),
    }
}

pub fn close(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    match norm {
        Norm::L1 => close_l1(image, k),
        _ => morph::close(image, norm, k),
    }
}

fn simd_sub(dst: &mut [u8], lhs: &[u8], rhs: &[u8]) {
    let len = dst.len();
    let simd_width = 16;
    let simd_len = len / simd_width * simd_width;

    dst[..simd_len].par_chunks_mut(simd_width).zip(lhs[..simd_len].par_chunks(simd_width)).zip(rhs[..simd_len].par_chunks(simd_width)).for_each(|((d, a), b)| {
        let a_vec = u8x16::new(a.try_into().unwrap());
        let b_vec = u8x16::new(b.try_into().unwrap());
        d.copy_from_slice(&a_vec.saturating_sub(b_vec).to_array());
    });

    for i in simd_len..len {
        dst[i] = lhs[i].saturating_sub(rhs[i]);
    }
}

pub fn tophat(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let opened = open(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let open_buf = opened.as_flat_samples().samples;
    out_buf.par_iter_mut().zip(img_buf.par_iter()).zip(open_buf.par_iter()).for_each(|((o, i), op)| *o = i.saturating_sub(*op));
    out
}

pub fn tophat_simd(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let opened = open(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let open_buf = opened.as_flat_samples().samples;
    simd_sub(out_buf, img_buf, open_buf);
    out
}

pub fn blackhat(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let closed = close(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let closed_buf = closed.as_flat_samples().samples;
    out_buf.par_iter_mut().zip(closed_buf.par_iter()).zip(img_buf.par_iter()).for_each(|((o, c), i)| *o = c.saturating_sub(*i));
    out
}

pub fn blackhat_simd(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let closed = close(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let closed_buf = closed.as_flat_samples().samples;
    simd_sub(out_buf, closed_buf, img_buf);
    out
}

pub fn gradient(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let dil = dilate(image, norm, k);
    let ero = erode(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let dil_buf = dil.as_flat_samples().samples;
    let ero_buf = ero.as_flat_samples().samples;
    out_buf.par_iter_mut().zip(dil_buf.par_iter()).zip(ero_buf.par_iter()).for_each(|((o, d), e)| *o = d.saturating_sub(*e));
    out
}

pub fn gradient_simd(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let dil = dilate(image, norm, k);
    let ero = erode(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let dil_buf = dil.as_flat_samples().samples;
    let ero_buf = ero.as_flat_samples().samples;
    simd_sub(out_buf, dil_buf, ero_buf);
    out
}

pub fn open_l1(image: &GrayImage, k: u8) -> GrayImage {
    if k == 0 {
        return image.clone();
    }
    // `imageproc::morphology::{erode,dilate}` are implemented via an L1 distance transform.
    // The generic implementation is surprisingly expensive on ARM due to per-pixel image access
    // overhead. Reimplement the same semantics on raw slices.
    BINARY_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        open_l1_binary(image, k, &mut scratch)
    })
}

pub fn close_l1(image: &GrayImage, k: u8) -> GrayImage {
    if k == 0 {
        return image.clone();
    }
    let dilated = dilate(image, Norm::L1, k);
    erode(&dilated, Norm::L1, k)
}

fn open_l1_binary(image: &GrayImage, k: u8, scratch: &mut BinaryMorphScratch) -> GrayImage {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return image.clone();
    }
    if k == 0 {
        return image.clone();
    }
    let w = w as usize;
    let h = h as usize;
    let len = w.saturating_mul(h);
    if len == 0 {
        return image.clone();
    }

    // Ensure scratch buffers.
    if scratch.a.len() != len {
        scratch.a.resize(len, 0);
    }
    if scratch.b.len() != len {
        scratch.b.resize(len, 0);
    }

    let input = image.as_raw();
    let (a, b) = (&mut scratch.a[..], &mut scratch.b[..]);

    let max_distance = ((w as u32) + (h as u32)).min(255) as u8;

    if k == 1 {
        // Fast path: for k=1 the L1 distance transform based morphology reduces to a single
        // 4-neighborhood erosion + dilation. This is much faster than running two full distance
        // transforms over the whole image.
        //
        // `imageproc` treats foreground as "non-zero", so binarize first (0 vs 255).
        for i in 0..len {
            a[i] = if input[i] == 0 { 0 } else { 255 };
        }
        erode_cross_k1(a, b, w, h);

        let mut out = GrayImage::new(w as u32, h as u32);
        let out_buf = out.as_flat_samples_mut().samples;
        dilate_cross_k1(b, out_buf, w, h);
        return out;
    }

    // General path: identical semantics to `imageproc`, but using raw slices.
    // Erode: compute distance from background (0).
    for i in 0..len {
        a[i] = if input[i] == 0 { 0 } else { max_distance };
    }
    dt_l1_in_place(a, w, h);
    for i in 0..len {
        b[i] = if a[i] <= k { 0 } else { 255 };
    }

    // Dilate: compute distance from foreground (!=0) of eroded image.
    for i in 0..len {
        a[i] = if b[i] != 0 { 0 } else { max_distance };
    }
    dt_l1_in_place(a, w, h);

    let mut out = GrayImage::new(w as u32, h as u32);
    let out_buf = out.as_flat_samples_mut().samples;
    for i in 0..len {
        out_buf[i] = if a[i] <= k { 255 } else { 0 };
    }
    out
}

#[inline(always)]
fn dt_l1_in_place(dist: &mut [u8], w: usize, h: usize) {
    debug_assert_eq!(dist.len(), w.saturating_mul(h));
    if w == 0 || h == 0 {
        return;
    }

    // Keep the inner loops branch-free; bounds checks + conditionals dominate on ARM.
    if w == 1 {
        // Forward (up).
        for y in 1..h {
            let i = y;
            let cand = dist[i - 1] as u16 + 1;
            let cur = dist[i] as u16;
            dist[i] = cur.min(cand) as u8;
        }
        // Backward (down).
        for y in (0..(h - 1)).rev() {
            let i = y;
            let cand = dist[i + 1] as u16 + 1;
            let cur = dist[i] as u16;
            dist[i] = cur.min(cand) as u8;
        }
        return;
    }

    if h == 1 {
        // Forward (left).
        for x in 1..w {
            let cand = dist[x - 1] as u16 + 1;
            let cur = dist[x] as u16;
            dist[x] = cur.min(cand) as u8;
        }
        // Backward (right).
        for x in (0..(w - 1)).rev() {
            let cand = dist[x + 1] as u16 + 1;
            let cur = dist[x] as u16;
            dist[x] = cur.min(cand) as u8;
        }
        return;
    }

    // Forward pass (top-left -> bottom-right).
    // Row 0: left neighbor only.
    for x in 1..w {
        let i = x;
        let cand = dist[i - 1] as u16 + 1;
        let cur = dist[i] as u16;
        dist[i] = cur.min(cand) as u8;
    }
    // Rows 1..: up and left neighbors.
    for y in 1..h {
        let row = y * w;
        // Col 0: up only.
        {
            let i = row;
            let cand = dist[i - w] as u16 + 1;
            let cur = dist[i] as u16;
            dist[i] = cur.min(cand) as u8;
        }
        // Interior: min(current, left+1, up+1).
        for x in 1..w {
            let i = row + x;
            let mut d = dist[i] as u16;
            let cand_l = dist[i - 1] as u16 + 1;
            if cand_l < d {
                d = cand_l;
            }
            let cand_u = dist[i - w] as u16 + 1;
            if cand_u < d {
                d = cand_u;
            }
            dist[i] = d as u8;
        }
    }

    // Backward pass (bottom-right -> top-left).
    // Last row: right neighbor only.
    {
        let row = (h - 1) * w;
        for x in (0..(w - 1)).rev() {
            let i = row + x;
            let cand = dist[i + 1] as u16 + 1;
            let cur = dist[i] as u16;
            dist[i] = cur.min(cand) as u8;
        }
    }
    // Rows h-2..0: down and right neighbors.
    for y in (0..(h - 1)).rev() {
        let row = y * w;
        // Col w-1: down only.
        {
            let i = row + (w - 1);
            let cand = dist[i + w] as u16 + 1;
            let cur = dist[i] as u16;
            dist[i] = cur.min(cand) as u8;
        }
        // Interior: min(current, right+1, down+1).
        for x in (0..(w - 1)).rev() {
            let i = row + x;
            let mut d = dist[i] as u16;
            let cand_r = dist[i + 1] as u16 + 1;
            if cand_r < d {
                d = cand_r;
            }
            let cand_d = dist[i + w] as u16 + 1;
            if cand_d < d {
                d = cand_d;
            }
            dist[i] = d as u8;
        }
    }
}

#[inline(always)]
fn erode_cross_k1(src: &[u8], dst: &mut [u8], w: usize, h: usize) {
    debug_assert_eq!(src.len(), dst.len());
    if w == 0 || h == 0 {
        return;
    }

    if w == 1 && h == 1 {
        dst[0] = src[0];
        return;
    }

    if w == 1 {
        // 1D vertical (up/down). Missing neighbors are ignored (treated as foreground).
        dst[0] = src[0] & src[1];
        for y in 1..(h - 1) {
            let i = y;
            dst[i] = src[i] & src[i - 1] & src[i + 1];
        }
        dst[h - 1] = src[h - 1] & src[h - 2];
        return;
    }

    if h == 1 {
        // 1D horizontal (left/right). Missing neighbors are ignored (treated as foreground).
        dst[0] = src[0] & src[1];
        for x in 1..(w - 1) {
            dst[x] = src[x] & src[x - 1] & src[x + 1];
        }
        dst[w - 1] = src[w - 1] & src[w - 2];
        return;
    }

    // Interior.
    for y in 1..(h - 1) {
        let row = y * w;
        let up = row - w;
        let down = row + w;
        for x in 1..(w - 1) {
            let i = row + x;
            dst[i] = src[i] & src[i - 1] & src[i + 1] & src[up + x] & src[down + x];
        }
    }

    // Top row (no up).
    {
        let row = 0usize;
        for x in 0..w {
            let i = row + x;
            let mut v = src[i] & src[w + x]; // down always exists (h>=2)
            if x > 0 {
                v &= src[i - 1];
            }
            if x + 1 < w {
                v &= src[i + 1];
            }
            dst[i] = v;
        }
    }

    // Bottom row (no down).
    {
        let row = (h - 1) * w;
        let up = row - w;
        for x in 0..w {
            let i = row + x;
            let mut v = src[i] & src[up + x];
            if x > 0 {
                v &= src[i - 1];
            }
            if x + 1 < w {
                v &= src[i + 1];
            }
            dst[i] = v;
        }
    }

    // Left col (no left).
    for y in 1..(h - 1) {
        let i = y * w;
        dst[i] = src[i] & src[i + 1] & src[i - w] & src[i + w];
    }

    // Right col (no right).
    for y in 1..(h - 1) {
        let i = y * w + (w - 1);
        dst[i] = src[i] & src[i - 1] & src[i - w] & src[i + w];
    }
}

#[inline(always)]
fn dilate_cross_k1(src: &[u8], dst: &mut [u8], w: usize, h: usize) {
    debug_assert_eq!(src.len(), dst.len());
    if w == 0 || h == 0 {
        return;
    }

    if w == 1 && h == 1 {
        dst[0] = src[0];
        return;
    }

    if w == 1 {
        dst[0] = src[0] | src[1];
        for y in 1..(h - 1) {
            let i = y;
            dst[i] = src[i] | src[i - 1] | src[i + 1];
        }
        dst[h - 1] = src[h - 1] | src[h - 2];
        return;
    }

    if h == 1 {
        dst[0] = src[0] | src[1];
        for x in 1..(w - 1) {
            dst[x] = src[x] | src[x - 1] | src[x + 1];
        }
        dst[w - 1] = src[w - 1] | src[w - 2];
        return;
    }

    // Interior.
    for y in 1..(h - 1) {
        let row = y * w;
        let up = row - w;
        let down = row + w;
        for x in 1..(w - 1) {
            let i = row + x;
            dst[i] = src[i] | src[i - 1] | src[i + 1] | src[up + x] | src[down + x];
        }
    }

    // Top row (no up).
    for x in 0..w {
        let i = x;
        let mut v = src[i] | src[w + x];
        if x > 0 {
            v |= src[i - 1];
        }
        if x + 1 < w {
            v |= src[i + 1];
        }
        dst[i] = v;
    }

    // Bottom row (no down).
    {
        let row = (h - 1) * w;
        let up = row - w;
        for x in 0..w {
            let i = row + x;
            let mut v = src[i] | src[up + x];
            if x > 0 {
                v |= src[i - 1];
            }
            if x + 1 < w {
                v |= src[i + 1];
            }
            dst[i] = v;
        }
    }

    // Left col (no left).
    for y in 1..(h - 1) {
        let i = y * w;
        dst[i] = src[i] | src[i + 1] | src[i - w] | src[i + w];
    }

    // Right col (no right).
    for y in 1..(h - 1) {
        let i = y * w + (w - 1);
        dst[i] = src[i] | src[i - 1] | src[i - w] | src[i + w];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};
    use imageproc::distance_transform::Norm;

    fn make_binary_image(w: u32, h: u32, seed: u64) -> GrayImage {
        let mut img = GrayImage::new(w, h);
        // Tiny LCG to avoid pulling in RNG state from threads; deterministic across platforms.
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        for p in img.pixels_mut() {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let bit = (x >> 63) as u8;
            *p = Luma([if bit == 0 { 0u8 } else { 255u8 }]);
        }
        img
    }

    fn make_gray_image(w: u32, h: u32, seed: u64) -> GrayImage {
        let mut img = GrayImage::new(w, h);
        // Deterministic "random".
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        for p in img.pixels_mut() {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *p = Luma([(x >> 56) as u8]);
        }
        img
    }

    #[test]
    fn open_l1_binary_matches_imageproc() {
        // Sizes include small edge cases and typical mask-ish dimensions.
        let cases = [(1, 1), (2, 2), (3, 3), (8, 8), (31, 17), (64, 48)];
        for (w, h) in cases {
            for k in [0u8, 1u8, 2u8, 3u8] {
                let img = make_binary_image(w, h, (w as u64) << 32 | (h as u64) ^ (k as u64));
                let expected = imageproc::morphology::open(&img, Norm::L1, k);
                let got = open(&img, Norm::L1, k);
                assert_eq!(expected.as_raw(), got.as_raw(), "mismatch w={w} h={h} k={k}");
            }
        }
    }

    #[test]
    fn open_l1_general_matches_imageproc_for_k_gt_0() {
        let cases = [(1, 1), (2, 2), (3, 3), (8, 8), (31, 17), (64, 48)];
        for (w, h) in cases {
            for k in [1u8, 2u8, 3u8] {
                let img = make_gray_image(w, h, (w as u64) << 32 | (h as u64) ^ (k as u64) ^ 0x9e3779b97f4a7c15);
                let expected = imageproc::morphology::open(&img, Norm::L1, k);
                let got = open(&img, Norm::L1, k);
                assert_eq!(expected.as_raw(), got.as_raw(), "mismatch w={w} h={h} k={k}");
            }
        }
    }
}
