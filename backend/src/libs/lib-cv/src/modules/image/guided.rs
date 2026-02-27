use image::{GrayImage, Luma};

pub fn guided_filter_gray(gray: &GrayImage, radius: u32, epsilon: f32) -> GrayImage {
    let width = gray.width() as usize;
    let height = gray.height() as usize;
    if width == 0 || height == 0 {
        return gray.clone();
    }

    let r = radius as usize;
    let eps = epsilon.max(1e-6);

    let mut i = vec![0f32; width * height];
    for (idx, pixel) in gray.pixels().enumerate() {
        i[idx] = pixel.0[0] as f32 / 255.0;
    }

    let mean_i = box_mean(&i, width, height, r);
    let mut ii = vec![0f32; i.len()];
    for (dst, src) in ii.iter_mut().zip(i.iter()) {
        *dst = src * src;
    }
    let mean_ii = box_mean(&ii, width, height, r);
    let mut var_i = vec![0f32; i.len()];
    for idx in 0..i.len() {
        var_i[idx] = (mean_ii[idx] - mean_i[idx] * mean_i[idx]).max(0.0);
    }

    let mut a = vec![0f32; i.len()];
    let mut b = vec![0f32; i.len()];
    for idx in 0..i.len() {
        a[idx] = var_i[idx] / (var_i[idx] + eps);
        b[idx] = mean_i[idx] - a[idx] * mean_i[idx];
    }

    let mean_a = box_mean(&a, width, height, r);
    let mean_b = box_mean(&b, width, height, r);

    let mut out = GrayImage::new(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let value = mean_a[idx] * i[idx] + mean_b[idx];
            let clamped = (value * 255.0).round().clamp(0.0, 255.0) as u8;
            out.put_pixel(x as u32, y as u32, Luma([clamped]));
        }
    }

    out
}

fn box_mean(data: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut integral = vec![0f32; (width + 1) * (height + 1)];
    for y in 0..height {
        let mut row_sum = 0f32;
        for x in 0..width {
            row_sum += data[y * width + x];
            let idx = (y + 1) * (width + 1) + (x + 1);
            integral[idx] = integral[idx - width - 1] + row_sum;
        }
    }

    let mut out = vec![0f32; width * height];
    for y in 0..height {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius + 1).min(height);
        for x in 0..width {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius + 1).min(width);
            let idx = y * width + x;
            let sum = get_integral(&integral, width, x0, y0, x1, y1);
            let area = (x1 - x0) * (y1 - y0);
            out[idx] = sum / area.max(1) as f32;
        }
    }
    out
}

fn get_integral(integral: &[f32], width: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> f32 {
    let stride = width + 1;
    let a = integral[y0 * stride + x0];
    let b = integral[y0 * stride + x1];
    let c = integral[y1 * stride + x0];
    let d = integral[y1 * stride + x1];
    d - b - c + a
}
