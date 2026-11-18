//! BFGS — quasi-Newton with a numerically-approximated inverse Hessian.
//!
//! Ported from `System::solve_BFGS` and the free function `lineSearch`
//! (`GCS.cpp`). Defaults match planegcs's own: `convergence = 1e-10`
//! (`System::System()`), `smallF = 1e-20` (`#define smallF 1e-20`,
//! `GCS.h`), `max_iter = 100`. As with `dogleg.rs`/`lm.rs`, store snapshot/
//! revert-on-failure replaces the C++'s scratch-copy-plus-`applySolution()`
//! split — see `solvers/mod.rs`'s doc comment.

use nalgebra::DVector;

use crate::solvers::SolveStatus;
use crate::subsystem::SubSystem;
use crate::util::ParamStore;

const CONVERGENCE: f64 = 1e-10;
const SMALL_F: f64 = 1e-20;
const MAX_ITER: usize = 100;

/// Bracket-and-quadratic-fit line search along `xdir`, restricted to
/// `subsys->maxStep`. Leaves `store` holding the params at the chosen step
/// and returns that step's scale — ported from the free function
/// `lineSearch` (`GCS.cpp`).
fn line_search(sub: &SubSystem, store: &mut ParamStore, xdir: &DVector<f64>) -> f64 {
    let alpha_max = sub.max_step(store, xdir.as_slice(), 1e10);
    let x0 = sub.get_params(store);

    let alpha1 = 0.0_f64;
    let f1 = sub.error(store);

    let mut alpha2 = 1.0_f64;
    sub.set_params(store, &(&x0 + xdir * alpha2));
    let mut f2 = sub.error(store);

    let mut alpha3 = alpha2 * 2.0;
    sub.set_params(store, &(&x0 + xdir * alpha3));
    let mut f3 = sub.error(store);

    while f2 > f1 || f2 > f3 {
        if f2 > f1 {
            alpha3 = alpha2;
            f3 = f2;
            alpha2 /= 2.0;
            sub.set_params(store, &(&x0 + xdir * alpha2));
            f2 = sub.error(store);
        } else if f2 > f3 {
            if alpha3 >= alpha_max {
                break;
            }
            alpha2 = alpha3;
            f2 = f3;
            alpha3 *= 2.0;
            sub.set_params(store, &(&x0 + xdir * alpha3));
            f3 = sub.error(store);
        }
    }

    let mut alpha_star = alpha2 + ((alpha2 - alpha1) * (f1 - f3)) / (3.0 * (f1 - 2.0 * f2 + f3));
    if alpha_star >= alpha3 || alpha_star <= alpha1 {
        alpha_star = alpha2;
    }
    if alpha_star > alpha_max {
        alpha_star = alpha_max;
    }
    if alpha_star.is_nan() {
        alpha_star = 0.0;
    }

    sub.set_params(store, &(&x0 + xdir * alpha_star));
    alpha_star
}

pub fn solve_bfgs(sub: &SubSystem, store: &mut ParamStore) -> SolveStatus {
    let xsize = sub.p_size();
    if xsize == 0 {
        return SolveStatus::Success;
    }

    let snapshot = store.redirect();

    let mut d = nalgebra::DMatrix::<f64>::identity(xsize, xsize);

    let mut x = sub.get_params(store);
    let mut grad = sub.calc_grad(store);

    let mut xdir = -&grad;
    line_search(sub, store, &xdir);
    let mut err = sub.error(store);

    let mut h = &sub.get_params(store) - &x;
    x = sub.get_params(store);

    let diverging_lim = 1e6 * err + 1e12;

    for _iter in 1..MAX_ITER {
        let h_norm = h.norm();
        if h_norm <= CONVERGENCE || err <= SMALL_F {
            break;
        }
        if err > diverging_lim || err.is_nan() {
            break;
        }

        let grad_old = grad.clone();
        grad = sub.calc_grad(store);
        let y = &grad - &grad_old;

        let mut hty = h.dot(&y);
        if hty == 0.0 {
            hty = 0.0000000001;
        }

        let dy = &d * &y;
        let yt_dy = y.dot(&dy);

        d += (&h * h.transpose()) * ((1.0 + yt_dy / hty) / hty);
        d -= (&h * dy.transpose() + &dy * h.transpose()) * (1.0 / hty);

        xdir = -(&d * &grad);
        line_search(sub, store, &xdir);
        err = sub.error(store);

        h = &sub.get_params(store) - &x;
        x = sub.get_params(store);
    }

    if err <= SMALL_F {
        return SolveStatus::Success;
    }
    if h.norm() <= CONVERGENCE {
        return SolveStatus::Converged;
    }
    store.revert(&snapshot);
    SolveStatus::Failed
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
        let p1 = Point::new(store.add(0.0, true), store.add(0.0, true));
        let p2 = Point::new(store.add(1.0, false), store.add(0.0, false));
        let distance = store.add(5.0, true);

        let constr: Rc<dyn crate::constraints::Constraint> =
            Rc::new(P2PDistance::new(p1, p2, distance));
        let sub = SubSystem::new(vec![constr], &[p2.x, p2.y]);

        let status = solve_bfgs(&sub, &mut store);
        assert!(
            matches!(status, SolveStatus::Success | SolveStatus::Converged),
            "expected Success or Converged, got {status:?}"
        );
        assert!(sub.error(&store).abs() < 1e-6);

        let dx = store.get(p1.x) - store.get(p2.x);
        let dy = store.get(p1.y) - store.get(p2.y);
        assert!(((dx * dx + dy * dy).sqrt() - 5.0).abs() < 1e-4);
    }

    #[test]
    fn a_zero_parameter_subsystem_trivially_succeeds() {
        let mut store = ParamStore::new();
        let sub = SubSystem::new(vec![], &[]);
        assert_eq!(solve_bfgs(&sub, &mut store), SolveStatus::Success);
    }
}
