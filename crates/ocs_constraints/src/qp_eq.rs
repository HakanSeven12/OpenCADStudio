//! Equality-constrained quadratic program: minimizes `0.5*xᵀHx + gᵀx`
//! subject to `Ax + c = 0`.
//!
//! Ported from planegcs's `qp_eq.h`/`qp_eq.cpp`. The C++ takes a
//! `FullPivHouseholderQR` of `Aᵀ` and reads its *full* square `Q`, splitting
//! its columns by rank into a basis for `A`'s row space and one for its null
//! space. nalgebra's `QR` (like its `SVD`) only ever returns the *thin*
//! Q/U — `params_num × rank`, not `params_num × params_num` — so there are
//! no extra columns to split off directly. This gets the same two bases a
//! different way: `Y` (the row-space part) via the equivalent minimum-norm
//! right inverse `Y = Aᵀ(AAᵀ)⁻¹` (Cholesky, since `AAᵀ` is symmetric
//! positive-definite for full-row-rank `A`), and `Z` (the null-space part)
//! by extending the thin Q's `rank` orthonormal columns to a full
//! orthonormal basis of R^params_num via Gram-Schmidt against the standard
//! basis, keeping only the newly-added vectors. Both give the same
//! projectors the C++'s single QR produces — the KKT system solved here has
//! a unique minimizer regardless of which orthonormal basis the row/null
//! space happen to be expressed in.

use nalgebra::{DMatrix, DVector};

/// Solution of the equality-constrained QP: the minimizer `x`, the row-space
/// basis `y`, and the null-space basis `z` of `A` (mirroring the C++'s
/// output parameters `x`, `Y`, `Z`).
pub struct QpEqSolution {
    pub x: DVector<f64>,
    pub y: DMatrix<f64>,
    pub z: DMatrix<f64>,
}

/// Returns `None` where the C++ returns `-1`: `A` doesn't have full row rank,
/// or has more constraints than parameters.
pub fn qp_eq(
    h: &DMatrix<f64>,
    g: &DVector<f64>,
    a: &DMatrix<f64>,
    c: &DVector<f64>,
) -> Option<QpEqSolution> {
    let constr_num = a.nrows();
    let params_num = a.ncols();
    if constr_num > params_num {
        return None;
    }

    // Rank check only needs the singular values, not the vectors — cheaper
    // than a full SVD and mirrors the C++'s `rank != constr_num` guard.
    let singular_values = a.clone().singular_values();
    let tol = singular_values.max() * f64::EPSILON * (params_num.max(constr_num) as f64);
    let rank = singular_values.iter().filter(|&&s| s > tol).count();
    if rank != constr_num {
        return None;
    }

    // Thin QR of A^T: Q1 is params_num x rank, an orthonormal basis of A's
    // row space (A^T's column space) — well-posed since A's already-confirmed
    // full row rank means A^T has full column rank.
    let qr = a.transpose().qr();
    let q1 = qr.q();

    // Z: null-space basis of A, found by extending Q1's columns to a full
    // orthonormal basis of R^params_num and keeping only the new vectors —
    // mirrors `Z = Q.rightCols(params_num - rank)` from the full square Q
    // the C++ has directly.
    let z = extend_to_orthonormal_basis(&q1, params_num);

    // Y = A^T (A A^T)^-1, the minimum-norm right inverse of full-row-rank A
    // — mirrors `Y = Q1 * inv(R1^T) * P^T`, the same row-space projector.
    let aat = a * a.transpose();
    let aat_chol = nalgebra::linalg::Cholesky::new(aat)?;
    let aat_inv = aat_chol.solve(&DMatrix::identity(constr_num, constr_num));
    let y = a.transpose() * &aat_inv;

    let x = if params_num == rank {
        -&y * c
    } else {
        let ztz = z.transpose() * h * &z;
        let rhs = z.transpose() * (h * &y * c - g);
        let y_step = ztz.lu().solve(&rhs)?;
        -&y * c + &z * y_step
    };

    Some(QpEqSolution { x, y, z })
}

/// Extends the orthonormal columns of `q1` (`dim x rank`) to a full
/// orthonormal basis of R^dim via modified Gram-Schmidt against the standard
/// basis vectors, returning only the newly-added `dim - rank` columns.
fn extend_to_orthonormal_basis(q1: &DMatrix<f64>, dim: usize) -> DMatrix<f64> {
    const TOL: f64 = 1e-10;
    let rank = q1.ncols();
    if dim == rank {
        return DMatrix::zeros(dim, 0);
    }
    let mut basis: Vec<DVector<f64>> = (0..rank).map(|i| q1.column(i).into_owned()).collect();
    let mut extra: Vec<DVector<f64>> = Vec::with_capacity(dim - rank);

    for i in 0..dim {
        if extra.len() == dim - rank {
            break;
        }
        let mut candidate = DVector::zeros(dim);
        candidate[i] = 1.0;
        for b in &basis {
            let proj = b.dot(&candidate);
            candidate -= b * proj;
        }
        let norm = candidate.norm();
        if norm > TOL {
            candidate /= norm;
            basis.push(candidate.clone());
            extra.push(candidate);
        }
    }

    DMatrix::from_columns(&extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-8;

    #[test]
    fn minimizes_sum_of_squares_on_a_line() {
        // minimize x1^2 + x2^2 s.t. x1 + x2 = 1  ->  x1 = x2 = 0.5
        let h = DMatrix::identity(2, 2) * 2.0;
        let g = DVector::from_vec(vec![0.0, 0.0]);
        let a = DMatrix::from_row_slice(1, 2, &[1.0, 1.0]);
        let c = DVector::from_vec(vec![-1.0]);

        let sol = qp_eq(&h, &g, &a, &c).expect("full row rank system should solve");
        assert!((sol.x[0] - 0.5).abs() < EPS);
        assert!((sol.x[1] - 0.5).abs() < EPS);
    }

    #[test]
    fn fully_determined_system_matches_the_direct_solution() {
        // Square A: no null space (params_num == rank), x = -Y*c is the whole answer.
        // x1 + x2 = 3, x1 - x2 = 1  ->  x1=2, x2=1
        let h = DMatrix::identity(2, 2);
        let g = DVector::from_vec(vec![0.0, 0.0]);
        let a = DMatrix::from_row_slice(2, 2, &[1.0, 1.0, 1.0, -1.0]);
        let c = DVector::from_vec(vec![-3.0, -1.0]);

        let sol = qp_eq(&h, &g, &a, &c).expect("square full-rank system should solve");
        assert!((sol.x[0] - 2.0).abs() < EPS);
        assert!((sol.x[1] - 1.0).abs() < EPS);
    }

    #[test]
    fn rank_deficient_constraints_return_none() {
        // Two identical rows: rank(A) = 1 != constr_num = 2.
        let h = DMatrix::identity(2, 2);
        let g = DVector::from_vec(vec![0.0, 0.0]);
        let a = DMatrix::from_row_slice(2, 2, &[1.0, 1.0, 1.0, 1.0]);
        let c = DVector::from_vec(vec![-1.0, -1.0]);

        assert!(qp_eq(&h, &g, &a, &c).is_none());
    }

    #[test]
    fn more_constraints_than_parameters_return_none() {
        let h = DMatrix::identity(1, 1);
        let g = DVector::from_vec(vec![0.0]);
        let a = DMatrix::from_row_slice(2, 1, &[1.0, 1.0]);
        let c = DVector::from_vec(vec![-1.0, -1.0]);

        assert!(qp_eq(&h, &g, &a, &c).is_none());
    }

    #[test]
    fn z_spans_the_null_space_of_a() {
        let h = DMatrix::identity(3, 3);
        let g = DVector::from_vec(vec![0.0, 0.0, 0.0]);
        let a = DMatrix::from_row_slice(1, 3, &[1.0, 1.0, 1.0]);
        let c = DVector::from_vec(vec![0.0]);

        let sol = qp_eq(&h, &g, &a, &c).unwrap();
        assert_eq!(sol.z.nrows(), 3);
        assert_eq!(sol.z.ncols(), 2);
        let should_be_zero = &a * &sol.z;
        for v in should_be_zero.iter() {
            assert!(v.abs() < EPS, "A*Z should be zero, got {should_be_zero}");
        }
    }
}
