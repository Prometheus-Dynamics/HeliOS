use anyhow::{Result, anyhow};

use super::charts::{COLORCHECKER_CLASSIC_24_SRGB, srgb_to_linear};

pub(super) fn order_corners_tl_tr_br_bl(corners: &mut [[f64; 2]; 4]) {
    let mut pts = corners.to_vec();
    pts.sort_by(|a, b| a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal));
    let mut top = [pts[0], pts[1]];
    let mut bottom = [pts[2], pts[3]];
    top.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    bottom.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
    *corners = [top[0], top[1], bottom[1], bottom[0]];
}

pub(super) fn solve_colorchecker24_ccm(image_bytes: &[u8], corners: [[f64; 2]; 4]) -> Result<([[f64; 3]; 3], f64)> {
    use nalgebra::{DMatrix, Matrix3, Vector3};

    let img = image::load_from_memory(image_bytes)?.to_rgb8();
    let (w, h) = img.dimensions();
    if w < 8 || h < 8 {
        anyhow::bail!("image too small");
    }

    let h_mat = homography_from_unit_square(corners)?;
    let refs_srgb: [[f64; 3]; 24] = COLORCHECKER_CLASSIC_24_SRGB;

    let mut cam_samples = Vec::with_capacity(24);
    let mut ref_samples = Vec::with_capacity(24);
    let ctx = SamplePatchContext { img: &img, w: w as f64, h: h as f64, h_mat: &h_mat, grid: 5 };
    for (idx, srgb) in refs_srgb.iter().enumerate() {
        let row = idx / 6;
        let col = idx % 6;
        let (u0, u1) = (col as f64 / 6.0, (col as f64 + 1.0) / 6.0);
        let (v0, v1) = (row as f64 / 4.0, (row as f64 + 1.0) / 4.0);
        let uc = (u0 + u1) * 0.5;
        let vc = (v0 + v1) * 0.5;
        let du = (u1 - u0) * 0.18;
        let dv = (v1 - v0) * 0.18;
        let cam = sample_patch_rgb_linear(&ctx, uc, vc, du, dv)?;
        cam_samples.push(cam);

        let r = srgb_to_linear(srgb[0] / 255.0);
        let g = srgb_to_linear(srgb[1] / 255.0);
        let b = srgb_to_linear(srgb[2] / 255.0);
        ref_samples.push([r, g, b]);
    }

    let a = DMatrix::from_row_slice(24, 3, &cam_samples.iter().flat_map(|v| v.iter()).copied().collect::<Vec<_>>());
    let b = DMatrix::from_row_slice(24, 3, &ref_samples.iter().flat_map(|v| v.iter()).copied().collect::<Vec<_>>());
    let ata = &a.transpose() * &a;
    let atb = &a.transpose() * &b;
    let solved = ata.lu().solve(&atb).ok_or_else(|| anyhow!("singular solve"))?;

    let m = Matrix3::new(solved[(0, 0)], solved[(0, 1)], solved[(0, 2)], solved[(1, 0)], solved[(1, 1)], solved[(1, 2)], solved[(2, 0)], solved[(2, 1)], solved[(2, 2)]);

    let mut mse = 0.0;
    for i in 0..24 {
        let cam_v = Vector3::new(cam_samples[i][0], cam_samples[i][1], cam_samples[i][2]);
        let ref_v = Vector3::new(ref_samples[i][0], ref_samples[i][1], ref_samples[i][2]);
        let out = m.transpose() * cam_v;
        let diff = out - ref_v;
        mse += diff.dot(&diff);
    }
    let rms = (mse / 24.0).sqrt();

    Ok(([[m[(0, 0)], m[(0, 1)], m[(0, 2)]], [m[(1, 0)], m[(1, 1)], m[(1, 2)]], [m[(2, 0)], m[(2, 1)], m[(2, 2)]]], rms))
}

fn homography_from_unit_square(corners: [[f64; 2]; 4]) -> Result<nalgebra::Matrix3<f64>> {
    use nalgebra::{DMatrix, DVector, Matrix3};

    let src = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut a = DMatrix::<f64>::zeros(8, 8);
    let mut b = DVector::<f64>::zeros(8);
    for i in 0..4 {
        let u = src[i][0];
        let v = src[i][1];
        let x = corners[i][0];
        let y = corners[i][1];
        let r0 = i * 2;
        let r1 = r0 + 1;
        a[(r0, 0)] = u;
        a[(r0, 1)] = v;
        a[(r0, 2)] = 1.0;
        a[(r0, 6)] = -u * x;
        a[(r0, 7)] = -v * x;
        b[r0] = x;

        a[(r1, 3)] = u;
        a[(r1, 4)] = v;
        a[(r1, 5)] = 1.0;
        a[(r1, 6)] = -u * y;
        a[(r1, 7)] = -v * y;
        b[r1] = y;
    }
    let h = a.lu().solve(&b).ok_or_else(|| anyhow!("homography solve failed"))?;
    Ok(Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0))
}

struct SamplePatchContext<'a> {
    img: &'a image::RgbImage,
    w: f64,
    h: f64,
    h_mat: &'a nalgebra::Matrix3<f64>,
    grid: i32,
}

fn sample_patch_rgb_linear(ctx: &SamplePatchContext<'_>, u: f64, v: f64, du: f64, dv: f64) -> Result<[f64; 3]> {
    use nalgebra::Vector3;

    if ctx.grid <= 1 {
        anyhow::bail!("invalid grid");
    }
    let mut sum = [0.0f64; 3];
    let mut count = 0.0f64;
    for yi in 0..ctx.grid {
        for xi in 0..ctx.grid {
            let fu = u + (xi as f64 / (ctx.grid as f64 - 1.0) - 0.5) * 2.0 * du;
            let fv = v + (yi as f64 / (ctx.grid as f64 - 1.0) - 0.5) * 2.0 * dv;
            let p = ctx.h_mat * Vector3::new(fu, fv, 1.0);
            if p[2].abs() < 1e-9 {
                continue;
            }
            let x = p[0] / p[2];
            let y = p[1] / p[2];
            if !(0.0..(ctx.w - 1.0)).contains(&x) || !(0.0..(ctx.h - 1.0)).contains(&y) {
                continue;
            }
            let px = ctx.img.get_pixel(x.round() as u32, y.round() as u32).0;
            sum[0] += srgb_to_linear(px[0] as f64 / 255.0);
            sum[1] += srgb_to_linear(px[1] as f64 / 255.0);
            sum[2] += srgb_to_linear(px[2] as f64 / 255.0);
            count += 1.0;
        }
    }
    if count < 1.0 {
        anyhow::bail!("no samples in patch region (check corners)");
    }
    Ok([sum[0] / count, sum[1] / count, sum[2] / count])
}
