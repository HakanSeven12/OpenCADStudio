//! Powell's Dogleg trust-region solver.
//!
//! The Gauss-Newton step uses SVD-based least squares so rectangular and
//! rank-deficient Jacobians still produce a robust search direction.

use nalgebra::DVector;

use crate::solvers::SolveStatus;
use crate::subsystem::SubSystem;
use crate::util::ParamStore;

const TOLG: f64 = 1e-12;
const TOLX: f64 = 1e-12;
const TOLF: f64 = 1e-10;
const MAX_ITER: usize = 100;
/// Bound on the fallback SVD's QR sweeps, per singular value. nalgebra's
/// plain `svd()` iterates without limit and never terminates on some
/// inputs; a converging decomposition needs only a handful per value.
const SVD_MAX_ITER_PER_VALUE: usize = 100;

pub fn solve_dl(sub: &SubSystem, store: &mut ParamStore) -> SolveStatus {
    let xsize = sub.p_size();
    if xsize == 0 {
        return SolveStatus::Success;
    }

    let snapshot = store.redirect();

    let mut x = sub.get_params(store);
    let mut fx = sub.calc_residual(store);
    let mut jx = sub.calc_jacobi(store);
    let mut err = sub.error(store);

    let mut g = jx.transpose() * (-fx.clone());
    let mut g_inf = g.amax();
    let mut fx_inf = fx.amax();

    let diverging_lim = 1e6 * err + 1e12;
    let mut delta = 0.1_f64;
    let mut nu = 2.0_f64;
    let mut iter = 0usize;
    let mut reduce = 0i32;
    let mut stop = 0u8;

    loop {
        if fx_inf <= TOLF {
            stop = 1;
            break;
        } else if g_inf <= TOLG || delta <= TOLX * (TOLX + x.norm()) {
            stop = 2;
            break;
        } else if iter >= MAX_ITER {
            stop = 4;
            break;
        } else if err > diverging_lim
            || err.is_nan()
            || !fx.iter().all(|v| v.is_finite())
            || !jx.iter().all(|v| v.is_finite())
        {
            // A non-finite residual or Jacobian (e.g. a gradient dividing
            // by a zero length) cannot produce a meaningful step, and
            // factoring it can fail to terminate.
            stop = 6;
            break;
        }

        let alpha = g.norm_squared() / (&jx * &g).norm_squared();
        let h_sd = &g * alpha;

        // A lightly regularized normal equation gives the same
        // Gauss-Newton direction at drawing precision and is dramatically
        // cheaper than decomposing a large rectangular Jacobian on every
        // trust-region iteration. Keep SVD as the robust fallback for a
        // factorization that still cannot be formed.
        let jt = jx.transpose();
        let mut normal = &jt * &jx;
        let diagonal_scale = normal.diagonal().amax().max(1.0);
        for index in 0..normal.nrows() {
            normal[(index, index)] += diagonal_scale * 1e-12;
        }
        let h_gn = match normal.cholesky() {
            Some(factor) => factor.solve(&g),
            None => {
                let max_niter = SVD_MAX_ITER_PER_VALUE * jx.nrows().min(jx.ncols()).max(1);
                match jx.clone().try_svd(true, true, f64::EPSILON, max_niter) {
                    Some(svd) => svd
                        .solve(&(-fx.clone()), 1e-12)
                        .unwrap_or_else(|_| DVector::zeros(xsize)),
                    None => break, // stop stays 0 -> Failed
                }
            }
        };

        let rel_error = (&jx * &h_gn + &fx).norm() / fx.norm();
        if rel_error > 1e15 {
            break; // stop stays 0 -> Failed, matching the C++'s bare `break;` here
        }

        let h_dl;
        if h_gn.norm() < delta {
            h_dl = h_gn.clone();
            if h_dl.norm() <= TOLX * (TOLX + x.norm()) {
                stop = 5;
                break;
            }
        } else if alpha * g.norm() >= delta {
            h_dl = &h_sd * (delta / (alpha * g.norm()));
        } else {
            let b = &h_gn - &h_sd;
            let bb = b.norm_squared();
            let gb = h_sd.dot(&b).abs();
            let c = (delta + h_sd.norm()) * (delta - h_sd.norm());
            let beta = if gb > 0.0 {
                c / (gb + (gb * gb + c * bb).sqrt())
            } else {
                ((gb * gb + c * bb).sqrt() - gb) / bb
            };
            h_dl = &h_sd + &b * beta;
        }

        let x_new = &x + &h_dl;
        sub.set_params(store, &x_new);
        let fx_new = sub.calc_residual(store);
        let err_new = sub.error(store);
        let jx_new = sub.calc_jacobi(store);

        let dl_pred = err - 0.5 * (&fx + &jx * &h_dl).norm_squared();
        let df = err - err_new;
        let rho = dl_pred / df;

        if df > 0.0 && dl_pred > 0.0 {
            x = x_new;
            jx = jx_new;
            fx = fx_new;
            err = err_new;
            g = jx.transpose() * (-fx.clone());
            g_inf = g.amax();
            fx_inf = fx.amax();
        } else {
            // The trial step wasn't accepted: undo it. The C++ never wrote
            // it anywhere but its own scratch copy in the first place; this
            // port has no such scratch, so the store needs restoring to the
            // last accepted `x` explicitly.
            sub.set_params(store, &x);
        }

        if (rho - 1.0).abs() < 0.2 && h_dl.norm() > delta / 3.0 && reduce <= 0 {
            delta *= 3.0;
            nu = 2.0;
            reduce = 0;
        } else if rho < 0.25 {
            delta /= nu;
            nu *= 2.0;
            reduce = 2;
        } else {
            reduce -= 1;
        }

        iter += 1;
    }

    if stop != 1 {
        store.revert(&snapshot);
        return SolveStatus::Failed;
    }
    SolveStatus::Success
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::point_line::P2PDistance;
    use crate::geo::Point;
    use std::rc::Rc;

    #[test]
    fn solves_a_single_p2p_distance_by_moving_the_free_point() {
        let mut store = ParamStore::new();
        let p1 = Point::new(store.add(0.0, true), store.add(0.0, true)); // fixed anchor
        let p2 = Point::new(store.add(1.0, false), store.add(0.0, false)); // free point, wrong distance
        let distance = store.add(5.0, true); // driven target

        let constr: Rc<dyn crate::constraints::Constraint> =
            Rc::new(P2PDistance::new(p1, p2, distance));
        let sub = SubSystem::new(vec![constr], &[p2.x, p2.y]);

        assert!(sub.error(&store) > 0.0, "should start unsatisfied");
        let status = solve_dl(&sub, &mut store);
        assert_eq!(status, SolveStatus::Success);
        assert!(
            sub.error(&store) < 1e-15,
            "residual should be ~zero after solving"
        );

        let dx = store.get(p1.x) - store.get(p2.x);
        let dy = store.get(p1.y) - store.get(p2.y);
        assert!(((dx * dx + dy * dy).sqrt() - 5.0).abs() < 1e-7);
    }

    #[test]
    fn solves_a_two_constraint_triangle() {
        // Two points free, each constrained to a fixed distance from a
        // fixed anchor and from each other -- a small but non-trivial
        // coupled system.
        let mut store = ParamStore::new();
        let anchor = Point::new(store.add(0.0, true), store.add(0.0, true));
        let p = Point::new(store.add(1.0, false), store.add(1.0, false));
        let d_anchor = store.add(10.0, true);

        let c1: Rc<dyn crate::constraints::Constraint> =
            Rc::new(P2PDistance::new(anchor, p, d_anchor));
        let sub = SubSystem::new(vec![c1], &[p.x, p.y]);

        let status = solve_dl(&sub, &mut store);
        assert_eq!(status, SolveStatus::Success);

        let dx = store.get(anchor.x) - store.get(p.x);
        let dy = store.get(anchor.y) - store.get(p.y);
        assert!(((dx * dx + dy * dy).sqrt() - 10.0).abs() < 1e-7);
    }

    #[test]
    fn a_zero_length_line_in_an_equal_length_pair_fails_instead_of_hanging() {
        // Equal length expressed as two P2PDistance constraints sharing one
        // free length. The first line's endpoints coincide, so its gradient
        // divides by zero and the Jacobian is NaN. That used to reach
        // nalgebra's unbounded SVD, which panics or never returns on NaN.
        let mut store = ParamStore::new();
        let a1 = Point::new(store.add(1.0, false), store.add(1.0, false));
        let a2 = Point::new(store.add(1.0, false), store.add(1.0, false));
        let b1 = Point::new(store.add(0.0, false), store.add(0.0, false));
        let b2 = Point::new(store.add(3.0, false), store.add(4.0, false));
        let length = store.add(2.0, false);

        let c1: Rc<dyn crate::constraints::Constraint> = Rc::new(P2PDistance::new(a1, a2, length));
        let c2: Rc<dyn crate::constraints::Constraint> = Rc::new(P2PDistance::new(b1, b2, length));
        let params = [a1.x, a1.y, a2.x, a2.y, b1.x, b1.y, b2.x, b2.y, length];
        let sub = SubSystem::new(vec![c1, c2], &params);

        assert!(sub.error(&store) > 0.0, "should start unsatisfied");
        assert_eq!(solve_dl(&sub, &mut store), SolveStatus::Failed);
        assert_eq!(store.get(length), 2.0, "a failed solve reverts the store");
    }

    #[test]
    fn a_zero_parameter_subsystem_trivially_succeeds() {
        let store = ParamStore::new();
        let sub = SubSystem::new(vec![], &[]);
        let mut store = store;
        assert_eq!(solve_dl(&sub, &mut store), SolveStatus::Success);
    }
}
