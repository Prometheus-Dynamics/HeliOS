use image::GrayImage;
use imageproc::{distance_transform::Norm, morphology as morph};
use rayon::prelude::*;
use std::cell::RefCell;
use wide::u8x16;

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

fn simd_add_assign_sub(dst: &mut [u8], lhs: &[u8], rhs: &[u8]) {
    let len = dst.len();
    let simd_width = 16;
    let simd_len = len / simd_width * simd_width;

    dst[..simd_len].par_chunks_mut(simd_width).zip(lhs[..simd_len].par_chunks(simd_width)).zip(rhs[..simd_len].par_chunks(simd_width)).for_each(|((d, a), b)| {
        let cur = u8x16::new(d.try_into().unwrap());
        let a_vec = u8x16::new(a.try_into().unwrap());
        let b_vec = u8x16::new(b.try_into().unwrap());
        let diff = a_vec.saturating_sub(b_vec);
        d.copy_from_slice(&cur.saturating_add(diff).to_array());
    });

    for i in simd_len..len {
        let diff = lhs[i].saturating_sub(rhs[i]);
        dst[i] = dst[i].saturating_add(diff);
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

pub fn internal_gradient(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let ero = erode(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let ero_buf = ero.as_flat_samples().samples;
    out_buf.par_iter_mut().zip(img_buf.par_iter()).zip(ero_buf.par_iter()).for_each(|((o, i), e)| *o = i.saturating_sub(*e));
    out
}

pub fn internal_gradient_simd(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let ero = erode(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let ero_buf = ero.as_flat_samples().samples;
    simd_sub(out_buf, img_buf, ero_buf);
    out
}

pub fn external_gradient(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let dil = dilate(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let dil_buf = dil.as_flat_samples().samples;
    out_buf.par_iter_mut().zip(dil_buf.par_iter()).zip(img_buf.par_iter()).for_each(|((o, d), i)| *o = d.saturating_sub(*i));
    out
}

pub fn external_gradient_simd(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let dil = dilate(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let img_buf = image.as_flat_samples().samples;
    let dil_buf = dil.as_flat_samples().samples;
    simd_sub(out_buf, dil_buf, img_buf);
    out
}

pub fn outline(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let int = internal_gradient(image, norm, k);
    let ext = external_gradient(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let int_buf = int.as_flat_samples().samples;
    let ext_buf = ext.as_flat_samples().samples;
    out_buf.par_iter_mut().zip(int_buf.par_iter()).zip(ext_buf.par_iter()).for_each(|((o, i), e)| {
        let val = (*i as u16 + *e as u16).min(255) as u8;
        *o = val;
    });
    out
}

pub fn outline_simd(image: &GrayImage, norm: Norm, k: u8) -> GrayImage {
    let int = internal_gradient(image, norm, k);
    let ext = external_gradient(image, norm, k);
    let mut out = GrayImage::new(image.width(), image.height());
    let out_buf = out.as_flat_samples_mut().samples;
    let int_buf = int.as_flat_samples().samples;
    let ext_buf = ext.as_flat_samples().samples;
    let len = out_buf.len();
    let simd_width = 16;
    let simd_len = len / simd_width * simd_width;

    out_buf[..simd_len].par_chunks_mut(simd_width).zip(int_buf[..simd_len].par_chunks(simd_width)).zip(ext_buf[..simd_len].par_chunks(simd_width)).for_each(|((o, i), e)| {
        let i_vec = u8x16::new(i.try_into().unwrap());
        let e_vec = u8x16::new(e.try_into().unwrap());
        o.copy_from_slice(&i_vec.saturating_add(e_vec).to_array());
    });

    for idx in simd_len..len {
        let val = int_buf[idx].saturating_add(ext_buf[idx]);
        out_buf[idx] = val;
    }
    out
}

pub fn skeleton(image: &GrayImage, norm: Norm) -> GrayImage {
    if matches!(norm, Norm::L1) {
        return skeleton_l1(image);
    }
    let mut eroded = image.clone();
    let mut result = GrayImage::new(image.width(), image.height());

    while eroded.as_flat_samples().samples.par_iter().any(|&p| p > 0) {
        let opened = open(&eroded, norm, 1);
        let res_buf = result.as_flat_samples_mut().samples;
        let ero_buf = eroded.as_flat_samples().samples;
        let open_buf = opened.as_flat_samples().samples;
        res_buf.par_iter_mut().zip(ero_buf.par_iter()).zip(open_buf.par_iter()).for_each(|((r, e), o)| {
            let diff = e.saturating_sub(*o);
            *r = r.saturating_add(diff);
        });
        eroded = erode(&eroded, norm, 1);
    }

    result
}

pub fn skeleton_simd(image: &GrayImage, norm: Norm) -> GrayImage {
    if matches!(norm, Norm::L1) {
        return skeleton_simd_l1(image);
    }
    let mut eroded = image.clone();
    let mut result = GrayImage::new(image.width(), image.height());

    while eroded.as_flat_samples().samples.par_iter().any(|&p| p > 0) {
        let opened = open(&eroded, norm, 1);
        let res_buf = result.as_flat_samples_mut().samples;
        let ero_buf = eroded.as_flat_samples().samples;
        let open_buf = opened.as_flat_samples().samples;
        simd_add_assign_sub(res_buf, ero_buf, open_buf);
        eroded = erode(&eroded, norm, 1);
    }

    result
}

fn skeleton_l1(image: &GrayImage) -> GrayImage {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return GrayImage::new(width, height);
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let needed = width_usize.saturating_mul(height_usize);
    let mut result = vec![0u8; needed];

    SKELETON_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        if scratch.current.len() != needed {
            scratch.current.resize(needed, 0);
            scratch.temp.resize(needed, 0);
            scratch.opened.resize(needed, 0);
        }

        scratch.current.copy_from_slice(image.as_raw());
        let SkeletonScratch { current, temp, opened } = &mut *scratch;

        while current.par_iter().any(|&p| p > 0) {
            cross_erode_once(current, temp, width_usize, height_usize);
            cross_dilate_once(temp, opened, width_usize, height_usize);

            result.par_iter_mut().zip(current.par_iter()).zip(opened.par_iter()).for_each(|((r, e), o)| {
                let diff = e.saturating_sub(*o);
                *r = r.saturating_add(diff);
            });

            cross_erode_once(current, temp, width_usize, height_usize);
            std::mem::swap(current, temp);
        }
    });

    GrayImage::from_raw(width, height, result).unwrap_or_else(|| GrayImage::new(width, height))
}

fn skeleton_simd_l1(image: &GrayImage) -> GrayImage {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return GrayImage::new(width, height);
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let needed = width_usize.saturating_mul(height_usize);
    let mut result = vec![0u8; needed];

    SKELETON_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        if scratch.current.len() != needed {
            scratch.current.resize(needed, 0);
            scratch.temp.resize(needed, 0);
            scratch.opened.resize(needed, 0);
        }

        scratch.current.copy_from_slice(image.as_raw());
        let SkeletonScratch { current, temp, opened } = &mut *scratch;

        while current.par_iter().any(|&p| p > 0) {
            cross_erode_once(current, temp, width_usize, height_usize);
            cross_dilate_once(temp, opened, width_usize, height_usize);

            simd_add_assign_sub(&mut result, current, opened);

            cross_erode_once(current, temp, width_usize, height_usize);
            std::mem::swap(current, temp);
        }
    });

    GrayImage::from_raw(width, height, result).unwrap_or_else(|| GrayImage::new(width, height))
}

fn open_l1(image: &GrayImage, k: u8) -> GrayImage {
    if k == 0 {
        return image.clone();
    }

    let radius = k as usize;
    let eroded = cross_iterate(image, radius, true);
    cross_iterate(&eroded, radius, false)
}

fn close_l1(image: &GrayImage, k: u8) -> GrayImage {
    if k == 0 {
        return image.clone();
    }

    let radius = k as usize;
    let dilated = cross_iterate(image, radius, false);
    cross_iterate(&dilated, radius, true)
}

fn cross_iterate(image: &GrayImage, radius: usize, is_erode: bool) -> GrayImage {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return GrayImage::new(width, height);
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let needed = width_usize.saturating_mul(height_usize);
    let mut output = None;
    MORPH_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        if scratch.current.len() != needed {
            scratch.current.resize(needed, 0);
        }
        if scratch.temp.len() != needed {
            scratch.temp.resize(needed, 0);
        }

        scratch.current.copy_from_slice(image.as_flat_samples().samples);
        let MorphScratch { current, temp } = &mut *scratch;

        for _ in 0..radius {
            if is_erode {
                cross_erode_once(current, temp, width_usize, height_usize);
            } else {
                cross_dilate_once(current, temp, width_usize, height_usize);
            }
            std::mem::swap(current, temp);
        }

        let result = std::mem::take(current);
        std::mem::swap(current, temp);
        output = Some(result);
    });
    GrayImage::from_raw(width, height, output.unwrap_or_default()).unwrap_or_else(|| GrayImage::new(width, height))
}

thread_local! {
    static MORPH_SCRATCH: RefCell<MorphScratch> = RefCell::new(MorphScratch::default());
    static SKELETON_SCRATCH: RefCell<SkeletonScratch> = RefCell::new(SkeletonScratch::default());
}

#[derive(Default)]
struct MorphScratch {
    current: Vec<u8>,
    temp: Vec<u8>,
}

#[derive(Default)]
struct SkeletonScratch {
    current: Vec<u8>,
    temp: Vec<u8>,
    opened: Vec<u8>,
}

fn cross_erode_once(src: &[u8], dst: &mut [u8], width: usize, height: usize) {
    dst.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let row_offset = y * width;
        let has_prev = y > 0;
        let has_next = y + 1 < height;

        if !has_prev || !has_next {
            row.fill(0);
            return;
        }

        row[0] = 0;
        row[width - 1] = 0;

        let prev = &src[row_offset - width..row_offset];
        let curr = &src[row_offset..row_offset + width];
        let next = &src[row_offset + width..row_offset + 2 * width];

        let mut x = 1usize;
        let max_x = width - 1;
        while x + 16 < max_x {
            let left = u8x16::new(curr[x - 1..x - 1 + 16].try_into().unwrap());
            let center = u8x16::new(curr[x..x + 16].try_into().unwrap());
            let right = u8x16::new(curr[x + 1..x + 1 + 16].try_into().unwrap());
            let up = u8x16::new(prev[x..x + 16].try_into().unwrap());
            let down = u8x16::new(next[x..x + 16].try_into().unwrap());

            let m = center.min(left).min(right).min(up).min(down);
            row[x..x + 16].copy_from_slice(&m.to_array());
            x += 16;
        }

        for (x, pixel) in row.iter_mut().enumerate().take(max_x).skip(x) {
            let idx = row_offset + x;
            let mut min_val = src[idx];
            min_val = min_val.min(src[idx - 1]);
            if min_val == 0 {
                *pixel = 0;
                continue;
            }
            min_val = min_val.min(src[idx + 1]);
            if min_val == 0 {
                *pixel = 0;
                continue;
            }
            let up = src[idx - width];
            min_val = min_val.min(up);
            if min_val == 0 {
                *pixel = 0;
                continue;
            }
            let down = src[idx + width];
            min_val = min_val.min(down);
            *pixel = min_val;
        }
    });
}

fn cross_dilate_once(src: &[u8], dst: &mut [u8], width: usize, height: usize) {
    dst.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let row_offset = y * width;
        let has_prev = y > 0;
        let has_next = y + 1 < height;

        if has_prev && has_next && width > 2 {
            let prev = &src[row_offset - width..row_offset];
            let curr = &src[row_offset..row_offset + width];
            let next = &src[row_offset + width..row_offset + 2 * width];

            let mut x = 1usize;
            let max_x = width - 1;
            while x + 16 < max_x {
                let left = u8x16::new(curr[x - 1..x - 1 + 16].try_into().unwrap());
                let center = u8x16::new(curr[x..x + 16].try_into().unwrap());
                let right = u8x16::new(curr[x + 1..x + 1 + 16].try_into().unwrap());
                let up = u8x16::new(prev[x..x + 16].try_into().unwrap());
                let down = u8x16::new(next[x..x + 16].try_into().unwrap());

                let m = center.max(left).max(right).max(up).max(down);
                row[x..x + 16].copy_from_slice(&m.to_array());
                x += 16;
            }

            for (x, pixel) in row.iter_mut().enumerate().take(max_x).skip(x) {
                let idx = row_offset + x;
                let mut max_val = src[idx];
                max_val = max_val.max(src[idx - 1]);
                max_val = max_val.max(src[idx + 1]);
                max_val = max_val.max(src[idx - width]);
                max_val = max_val.max(src[idx + width]);
                *pixel = max_val;
            }

            // Borders (x=0 and x=width-1) fall back to scalar with bounds.
            let x = 0;
            let idx = row_offset + x;
            let mut max_val = src[idx];
            max_val = max_val.max(src[idx + 1]);
            if has_prev {
                max_val = max_val.max(src[idx - width]);
            }
            if has_next {
                max_val = max_val.max(src[idx + width]);
            }
            row[x] = max_val;

            let x = width - 1;
            let idx = row_offset + x;
            let mut max_val = src[idx];
            max_val = max_val.max(src[idx - 1]);
            if has_prev {
                max_val = max_val.max(src[idx - width]);
            }
            if has_next {
                max_val = max_val.max(src[idx + width]);
            }
            row[x] = max_val;
            return;
        }

        for (x, pixel) in row.iter_mut().enumerate() {
            let idx = row_offset + x;
            let mut max_val = src[idx];
            if x > 0 {
                max_val = max_val.max(src[idx - 1]);
            }
            if x + 1 < width {
                max_val = max_val.max(src[idx + 1]);
            }
            if has_prev {
                max_val = max_val.max(src[idx - width]);
            }
            if has_next {
                max_val = max_val.max(src[idx + width]);
            }
            *pixel = max_val;
        }
    });
}
