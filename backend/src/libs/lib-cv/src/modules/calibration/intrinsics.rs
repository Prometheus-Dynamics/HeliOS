use super::types::*;
use nalgebra::{DMatrix, Matrix3};

pub(super) fn solve_intrinsics_from_b(b: [f64; 6]) -> Result<(IntrinsicsSolve, Option<String>), CalibrationSolveError> {
    for sign in [1.0, -1.0] {
        let b1 = b[0] * sign;
        let b2 = b[1] * sign;
        let b3 = b[2] * sign;
        let b4 = b[3] * sign;
        let b5 = b[4] * sign;
        let b6 = b[5] * sign;

        if !b1.is_finite() || b1.abs() < 1e-12 {
            continue;
        }
        let denom = b1 * b3 - b2 * b2;
        if !denom.is_finite() || denom.abs() < 1e-12 {
            continue;
        }
        let v0 = (b2 * b4 - b1 * b5) / denom;
        if !v0.is_finite() {
            continue;
        }
        let lambda = b6 - (b4 * b4 + v0 * (b2 * b4 - b1 * b5)) / b1;
        if !lambda.is_finite() || lambda <= 0.0 {
            continue;
        }
        let fx2 = lambda / b1;
        let fy2 = lambda * b1 / denom;
        if !fx2.is_finite() || !fy2.is_finite() || fx2 <= 0.0 || fy2 <= 0.0 {
            continue;
        }
        let fx = fx2.sqrt();
        let fy = fy2.sqrt();
        if !fx.is_finite() || !fy.is_finite() || fx <= 0.0 || fy <= 0.0 {
            continue;
        }

        let skew_candidate = -b2 * fx * fx * fy / lambda;
        let skew = if skew_candidate.is_finite() { skew_candidate } else { 0.0 };
        let cx = skew * v0 / fy - b4 * fx * fx / lambda;
        let cy = v0;
        if !cx.is_finite() || !cy.is_finite() {
            continue;
        }

        return Ok((IntrinsicsSolve { fx, fy, skew, cx, cy }, None));
    }

    for sign in [1.0, -1.0] {
        let b1 = b[0] * sign;
        let b2 = b[1] * sign;
        let b3 = b[2] * sign;
        let b4 = b[3] * sign;
        let b5 = b[4] * sign;
        let b6 = b[5] * sign;
        if !b1.is_finite() || !b2.is_finite() || !b3.is_finite() || !b4.is_finite() || !b5.is_finite() || !b6.is_finite() {
            continue;
        }

        let bmat = Matrix3::new(b1, b2, b4, b2, b3, b5, b4, b5, b6);
        let b_sym = (bmat + bmat.transpose()) * 0.5;
        let eig = nalgebra::SymmetricEigen::new(b_sym);
        let max_eig = eig.eigenvalues.iter().copied().fold(0.0f64, |a, b| a.max(b.abs()));
        if !max_eig.is_finite() || max_eig <= 0.0 {
            continue;
        }
        let min_eig = (max_eig * 1e-12).max(1e-12);
        let clamped = eig.eigenvalues.map(|v| if v.is_finite() { v.max(min_eig) } else { min_eig });
        let b_spd = eig.eigenvectors * Matrix3::from_diagonal(&clamped) * eig.eigenvectors.transpose();

        let inv_spd = match b_spd.try_inverse() {
            Some(inv) => inv,
            None => continue,
        };
        let chol = match nalgebra::Cholesky::new(inv_spd) {
            Some(chol) => chol,
            None => continue,
        };
        let mut k = chol.l().transpose();
        let k33 = k[(2, 2)];
        if !k33.is_finite() || k33.abs() < 1e-12 {
            continue;
        }
        k /= k33;

        let fx = k[(0, 0)];
        let skew = k[(0, 1)];
        let cx = k[(0, 2)];
        let fy = k[(1, 1)];
        let cy = k[(1, 2)];
        if !fx.is_finite() || !fy.is_finite() || !cx.is_finite() || !cy.is_finite() || fx <= 0.0 || fy <= 0.0 {
            continue;
        }

        return Ok((
            IntrinsicsSolve { fx, fy, skew: if skew.is_finite() { skew } else { 0.0 }, cx, cy },
            Some("intrinsics solve: used SPD-projection fallback (dataset may be degenerate; capture more edge/corner coverage)".into()),
        ));
    }

    Err(CalibrationSolveError::IntrinsicsSolveFailed)
}

pub(super) fn solve_intrinsics_zero_skew(v: &DMatrix<f64>) -> Option<(IntrinsicsSolve, Option<String>)> {
    if v.nrows() < 2 || v.ncols() != 6 {
        return None;
    }
    let mut v_reduced = DMatrix::<f64>::zeros(v.nrows(), 5);
    for r in 0..v.nrows() {
        v_reduced[(r, 0)] = v[(r, 0)]; // b1
        v_reduced[(r, 1)] = v[(r, 2)]; // b3
        v_reduced[(r, 2)] = v[(r, 3)]; // b4
        v_reduced[(r, 3)] = v[(r, 4)]; // b5
        v_reduced[(r, 4)] = v[(r, 5)]; // b6
    }
    let svd = v_reduced.svd(true, true);
    let vt = svd.v_t?;
    let b = vt.row(vt.nrows() - 1).transpose();
    if b.nrows() != 5 {
        return None;
    }
    let bvec: [f64; 6] = [b[0], 0.0, b[1], b[2], b[3], b[4]];
    let (mut intrinsics, warning) = solve_intrinsics_from_b(bvec).ok()?;
    intrinsics.skew = 0.0;
    Some((intrinsics, Some(warning.unwrap_or_else(|| "intrinsics solve: forced zero skew fallback (dataset may be degenerate)".into()))))
}

pub(super) fn solve_intrinsics_from_homographies(homographies: &[(Matrix3<f64>, &ViewObservations)], cx: f64, cy: f64) -> Option<IntrinsicsSolve> {
    if !cx.is_finite() || !cy.is_finite() {
        return None;
    }
    let mut fx_samples: Vec<f64> = Vec::new();
    let mut fy_samples: Vec<f64> = Vec::new();
    let mut f_samples: Vec<f64> = Vec::new();

    for (h, _view) in homographies {
        let h11 = h[(0, 0)];
        let h21 = h[(1, 0)];
        let h31 = h[(2, 0)];
        let h12 = h[(0, 1)];
        let h22 = h[(1, 1)];
        let h32 = h[(2, 1)];

        let x1 = h11 - cx * h31;
        let y1 = h21 - cy * h31;
        let x2 = h12 - cx * h32;
        let y2 = h22 - cy * h32;

        let a1 = x1 * x2;
        let b1 = y1 * y2;
        let c1 = h31 * h32;
        let a2 = x1 * x1 - x2 * x2;
        let b2 = y1 * y1 - y2 * y2;
        let c2 = h31 * h31 - h32 * h32;

        let det_ab = a1 * b2 - a2 * b1;
        if det_ab.abs() > 1e-12 {
            let inv_fx2 = (-c1 * b2 + c2 * b1) / det_ab;
            let inv_fy2 = (-a1 * c2 + a2 * c1) / det_ab;
            if inv_fx2.is_finite() && inv_fy2.is_finite() && inv_fx2 > 0.0 && inv_fy2 > 0.0 {
                let fx = (1.0 / inv_fx2).sqrt();
                let fy = (1.0 / inv_fy2).sqrt();
                if fx.is_finite() && fy.is_finite() && (50.0..=50_000.0).contains(&fx) && (50.0..=50_000.0).contains(&fy) {
                    fx_samples.push(fx);
                    fy_samples.push(fy);
                }
            }
        }

        let a = a1 + b1;
        if a.abs() > 1e-12 && c1.is_finite() && c1 != 0.0 {
            let inv_f2 = -c1 / a;
            if inv_f2.is_finite() && inv_f2 > 0.0 {
                let f = (1.0 / inv_f2).sqrt();
                if f.is_finite() && (50.0..=50_000.0).contains(&f) {
                    f_samples.push(f);
                }
            }
        }
    }

    fn median_in_place(v: &mut [f64]) -> Option<f64> {
        if v.is_empty() {
            return None;
        }
        v.sort_by(|a, b| a.total_cmp(b));
        Some(v[v.len() / 2])
    }

    if fx_samples.len() >= 2 && fy_samples.len() >= 2 {
        let fx = median_in_place(&mut fx_samples)?;
        let fy = median_in_place(&mut fy_samples)?;
        return Some(IntrinsicsSolve { fx, fy, skew: 0.0, cx, cy });
    }
    if f_samples.len() >= 2 {
        let f = median_in_place(&mut f_samples)?;
        return Some(IntrinsicsSolve { fx: f, fy: f, skew: 0.0, cx, cy });
    }
    None
}
