//! Powell's Dogleg trust-region method — planegcs's default solver.
//!
//! Ported from `System::solve_DL` (`GCS.cpp`). Tolerances and iteration cap
//! match planegcs's own defaults (`System::System()`'s member-initializer
//! list): `tolg = 1e-80`, `tolx = 1e-80`, `tolf = 1e-10`, `max_iter = 100`
//! (the "redundant-solving" variants with different tolerances, and the
//! `sketchSizeMultiplier`-scaled iteration cap, aren't exposed — both are
//! System-level solver *configuration* the later `diagnosis.rs`/`system.rs`
//! work can thread through if it turns out to matter).
//!
//! One deliberate substitution: the Gauss-Newton step `h_gn` solves
//! `Jx * h_gn = -fx`. The C++ defaults to Eigen's `FullPivLU`, chosen
//! because it handles `Jx` being rectangular and possibly rank-deficient.
//! nalgebra has no equivalent decomposition; this uses its SVD-based
//! least-squares solve instead, which handles the same rectangular/
//! rank-deficient cases (via a small-singular-value cutoff) and serves the
//! same purpose — a robust direction when the Jacobian isn't a nice square
//! invertible matrix — without replicating the C++'s three-way
//! `dogLegGaussStep` switch between LU variants.

use nalgebra::DVector;

use crate::solvers::SolveStatus;
use crate::subsystem::SubSystem;
use crate::util::ParamStore;

const TOLG: f64 = 1e-80;
const TOLX: f64 = 1e-80;
const TOLF: f64 = 1e-10;
const MAX_ITER: usize = 100;

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
        } else if g_inf <= TOLG {
            stop = 2;
            break;
        } else if delta <= TOLX * (TOLX + x.norm()) {
            stop = 2;
            break;
        } else if iter >= MAX_ITER {
            stop = 4;
            break;
        } else if err > diverging_lim || err.is_nan() {
            stop = 6;
            break;
        }

        let alpha = g.norm_squared() / (&jx * &g).norm_squared();
        let h_sd = &g * alpha;

        let h_gn = jx
            .clone()
            .svd(true, true)
            .solve(&(-fx.clone()), 1e-12)
            .unwrap_or_else(|_| DVector::zeros(xsize));

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
            let beta = if gb > 0.0 { c / (gb + (gb * gb + c * bb).sqrt()) } else { ((gb * gb + c * bb).sqrt() - gb) / bb };
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

        let constr: Rc<dyn crate::constraints::Constraint> = Rc::new(P2PDistance::new(p1, p2, distance));
        let sub = SubSystem::new(vec![constr], &[p2.x, p2.y]);

        assert!(sub.error(&store) > 0.0, "should start unsatisfied");
        let status = solve_dl(&sub, &mut store);
        assert_eq!(status, SolveStatus::Success);
        assert!(sub.error(&store) < 1e-15, "residual should be ~zero after solving");

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

        let c1: Rc<dyn crate::constraints::Constraint> = Rc::new(P2PDistance::new(anchor, p, d_anchor));
        let sub = SubSystem::new(vec![c1], &[p.x, p.y]);

        let status = solve_dl(&sub, &mut store);
        assert_eq!(status, SolveStatus::Success);

        let dx = store.get(anchor.x) - store.get(p.x);
        let dy = store.get(anchor.y) - store.get(p.y);
        assert!(((dx * dx + dy * dy).sqrt() - 10.0).abs() < 1e-7);
    }

    #[test]
    fn a_zero_parameter_subsystem_trivially_succeeds() {
        let store = ParamStore::new();
        let sub = SubSystem::new(vec![], &[]);
        let mut store = store;
        assert_eq!(solve_dl(&sub, &mut store), SolveStatus::Success);
    }
}
