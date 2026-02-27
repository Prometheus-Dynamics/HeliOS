use super::projection;
use super::types::*;
use nalgebra::{DMatrix, DVector};

pub(super) fn estimate_distortion(views: &[ViewObservations], extrinsics: &[Extrinsics], intrinsics: IntrinsicsSolve) -> (f64, f64, f64, f64, f64) {
    let mut rows: Vec<[f64; 5]> = Vec::new();
    let mut rhs: Vec<f64> = Vec::new();

    for (view, ext) in views.iter().zip(extrinsics.iter()) {
        for (obj, img) in &view.points {
            let (x, y) = projection::project_normalized(*obj, ext);
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let xd = (img.0 - intrinsics.cx) / intrinsics.fx;
            let yd = (img.1 - intrinsics.cy) / intrinsics.fy;
            if !xd.is_finite() || !yd.is_finite() {
                continue;
            }
            let dx = xd - x;
            let dy = yd - y;

            let r2 = x * x + y * y;
            let r4 = r2 * r2;
            let r6 = r4 * r2;

            rows.push([x * r2, x * r4, x * r6, 2.0 * x * y, r2 + 2.0 * x * x]);
            rhs.push(dx);
            rows.push([y * r2, y * r4, y * r6, r2 + 2.0 * y * y, 2.0 * x * y]);
            rhs.push(dy);
        }
    }

    if rows.len() < 20 {
        return (0.0, 0.0, 0.0, 0.0, 0.0);
    }

    let flat: Vec<f64> = rows.iter().flat_map(|r| r.iter().copied()).collect();
    let a = DMatrix::<f64>::from_row_slice(rows.len(), 5, &flat);
    let b = DVector::<f64>::from_vec(rhs);
    let svd = a.svd(true, true);
    let x = svd.solve(&b, 1e-12).unwrap_or_else(|_| DVector::zeros(5));
    (x[0], x[1], x[2], x[3], x[4])
}

pub(super) fn distortion_is_reasonable(dist: DistortionCoefficients, lens_model: LensModel) -> bool {
    let max_abs = match lens_model {
        LensModel::Pinhole => 100.0,
        // Fisheye k1..k4 are typically small (often |k| < 0.5). Allowing huge coefficients makes
        // the optimizer happily trade focal length against distortion and converge to garbage.
        LensModel::Fisheye => 1.0,
    };
    // For fisheye, OpenCV uses 4 distortion coefficients (k1..k4). We store k4 in `p1`
    // and keep `p2` unused/zero.
    let vals = match lens_model {
        LensModel::Pinhole => [dist.k1, dist.k2, dist.k3, dist.p1, dist.p2],
        LensModel::Fisheye => [dist.k1, dist.k2, dist.k3, dist.p1, 0.0],
    };
    vals.iter().all(|v| v.is_finite() && v.abs() <= max_abs)
}

pub(super) fn distortion_param_count(lens_model: LensModel) -> usize {
    match lens_model {
        LensModel::Pinhole => 5,
        LensModel::Fisheye => 4,
    }
}

pub(super) fn distortion_from_params(params: &[f64], offset: usize, lens_model: LensModel) -> DistortionCoefficients {
    match lens_model {
        LensModel::Pinhole => DistortionCoefficients {
            k1: params.get(offset).copied().unwrap_or(0.0),
            k2: params.get(offset + 1).copied().unwrap_or(0.0),
            k3: params.get(offset + 2).copied().unwrap_or(0.0),
            p1: params.get(offset + 3).copied().unwrap_or(0.0),
            p2: params.get(offset + 4).copied().unwrap_or(0.0),
        },
        LensModel::Fisheye => DistortionCoefficients {
            k1: params.get(offset).copied().unwrap_or(0.0),
            k2: params.get(offset + 1).copied().unwrap_or(0.0),
            k3: params.get(offset + 2).copied().unwrap_or(0.0),
            // OpenCV fisheye uses k4; we store it in p1.
            p1: params.get(offset + 3).copied().unwrap_or(0.0),
            // Not used for fisheye.
            p2: 0.0,
        },
    }
}

pub(super) fn push_distortion_params(out: &mut Vec<f64>, dist: DistortionCoefficients, lens_model: LensModel) {
    out.push(dist.k1);
    out.push(dist.k2);
    out.push(dist.k3);
    out.push(dist.p1);
    if lens_model == LensModel::Pinhole {
        out.push(dist.p2);
    }
}
