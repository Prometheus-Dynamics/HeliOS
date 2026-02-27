use image::{DynamicImage, GrayImage};
use rayon::prelude::*;

const GAUSSIAN_3X3: [[f32; 3]; 3] = [[1.0, 2.0, 1.0], [2.0, 4.0, 2.0], [1.0, 2.0, 1.0]];

fn apply_convolution(gray: &GrayImage, kernel: &[[f32; 3]; 3], scale: f32) -> GrayImage {
    let width = gray.width() as usize;
    let height = gray.height() as usize;
    let src = gray.as_raw();
    let mut output = GrayImage::new(width as u32, height as u32);
    let dst = output.as_flat_samples_mut().samples;

    dst.par_chunks_mut(width).enumerate().for_each(|(y, dst_row)| {
        let y0 = y.saturating_sub(1);
        let y2 = (y + 1).min(height.saturating_sub(1));
        let row0 = &src[y0 * width..(y0 + 1) * width];
        let row1 = &src[y * width..(y + 1) * width];
        let row2 = &src[y2 * width..(y2 + 1) * width];

        for x in 0..width {
            let x0 = x.saturating_sub(1);
            let x2 = (x + 1).min(width.saturating_sub(1));
            let p00 = row0[x0] as f32;
            let p01 = row0[x] as f32;
            let p02 = row0[x2] as f32;
            let p10 = row1[x0] as f32;
            let p11 = row1[x] as f32;
            let p12 = row1[x2] as f32;
            let p20 = row2[x0] as f32;
            let p21 = row2[x] as f32;
            let p22 = row2[x2] as f32;

            let acc = kernel[0][0] * p00
                + kernel[0][1] * p01
                + kernel[0][2] * p02
                + kernel[1][0] * p10
                + kernel[1][1] * p11
                + kernel[1][2] * p12
                + kernel[2][0] * p20
                + kernel[2][1] * p21
                + kernel[2][2] * p22;
            dst_row[x] = (acc / scale).clamp(0.0, 255.0) as u8;
        }
    });

    output
}

pub fn sobel_edges(gray: &GrayImage) -> GrayImage {
    let width = gray.width() as usize;
    let height = gray.height() as usize;
    let src = gray.as_raw();
    let mut output = GrayImage::new(width as u32, height as u32);
    let dst = output.as_flat_samples_mut().samples;

    dst.par_chunks_mut(width).enumerate().for_each(|(y, dst_row)| {
        let y0 = y.saturating_sub(1);
        let y2 = (y + 1).min(height.saturating_sub(1));
        let row0 = &src[y0 * width..(y0 + 1) * width];
        let row1 = &src[y * width..(y + 1) * width];
        let row2 = &src[y2 * width..(y2 + 1) * width];

        for x in 0..width {
            let x0 = x.saturating_sub(1);
            let x2 = (x + 1).min(width.saturating_sub(1));
            let p00 = row0[x0] as f32;
            let p01 = row0[x] as f32;
            let p02 = row0[x2] as f32;
            let p10 = row1[x0] as f32;
            let p12 = row1[x2] as f32;
            let p20 = row2[x0] as f32;
            let p21 = row2[x] as f32;
            let p22 = row2[x2] as f32;

            let gx = -p00 + p02 - 2.0 * p10 + 2.0 * p12 - p20 + p22;
            let gy = p00 + 2.0 * p01 + p02 - p20 - 2.0 * p21 - p22;
            let mag = (gx * gx + gy * gy).sqrt();
            dst_row[x] = (mag / 1443.0 * 255.0).clamp(0.0, 255.0) as u8;
        }
    });

    output
}

fn gaussian_blur(gray: &GrayImage) -> GrayImage {
    apply_convolution(gray, &GAUSSIAN_3X3, 16.0)
}

pub fn canny_prep(gray: &GrayImage) -> GrayImage {
    let blurred = gaussian_blur(gray);
    sobel_edges(&blurred)
}

pub fn convolve_gray(gray: &GrayImage, kernel: [f32; 9], factor: f32, bias: f32) -> GrayImage {
    let width = gray.width() as usize;
    let height = gray.height() as usize;
    let src = gray.as_raw();
    let mut output = GrayImage::new(width as u32, height as u32);
    let dst = output.as_flat_samples_mut().samples;

    dst.par_chunks_mut(width).enumerate().for_each(|(y, dst_row)| {
        let y0 = y.saturating_sub(1);
        let y2 = (y + 1).min(height.saturating_sub(1));
        let row0 = &src[y0 * width..(y0 + 1) * width];
        let row1 = &src[y * width..(y + 1) * width];
        let row2 = &src[y2 * width..(y2 + 1) * width];

        for x in 0..width {
            let x0 = x.saturating_sub(1);
            let x2 = (x + 1).min(width.saturating_sub(1));
            let p00 = row0[x0] as f32;
            let p01 = row0[x] as f32;
            let p02 = row0[x2] as f32;
            let p10 = row1[x0] as f32;
            let p11 = row1[x] as f32;
            let p12 = row1[x2] as f32;
            let p20 = row2[x0] as f32;
            let p21 = row2[x] as f32;
            let p22 = row2[x2] as f32;

            let acc = kernel[0] * p00 + kernel[1] * p01 + kernel[2] * p02 + kernel[3] * p10 + kernel[4] * p11 + kernel[5] * p12 + kernel[6] * p20 + kernel[7] * p21 + kernel[8] * p22;
            dst_row[x] = (acc * factor + bias).clamp(0.0, 255.0) as u8;
        }
    });

    output
}

pub fn emboss(gray: &GrayImage) -> GrayImage {
    const EMBOSS_KERNEL: [f32; 9] = [-2.0, -1.0, 0.0, -1.0, 1.0, 1.0, 0.0, 1.0, 2.0];
    convolve_gray(gray, EMBOSS_KERNEL, 1.0, 128.0)
}

pub fn sharpen(gray: &GrayImage) -> GrayImage {
    const SHARPEN_KERNEL: [f32; 9] = [0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0];
    convolve_gray(gray, SHARPEN_KERNEL, 1.0, 0.0)
}

pub fn to_gray(frame: &DynamicImage) -> GrayImage {
    frame.to_luma8()
}
