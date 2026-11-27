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
    const MAX_LS_ITER: usize = 64;

    let alpha_max = sub.max_step(store, xdir.as_slice(), 1e10);
    let x0 = sub.get_params(store);

    let alpha1 = 0.0_f64;
    let f1 = sub.error(store);
    if !f1.is_finite()
        || !alpha_max.is_finite()
        || alpha_max <= 0.0
        || !xdir.iter().all(|v| v.is_finite())
    {
        return 0.0;
    }

    let mut alpha2 = 1.0_f64.min(alpha_max);
    sub.set_params(store, &(&x0 + xdir * alpha2));
    let mut f2 = sub.error(store);

    let mut alpha3 = (alpha2 * 2.0).min(alpha_max);
    sub.set_params(store, &(&x0 + xdir * alpha3));
    let mut f3 = sub.error(store);

    let mut ls_iter = 0usize;
    while ls_iter < MAX_LS_ITER && (f2 > f1 || f2 > f3) {
        ls_iter += 1;
        if !f2.is_finite() || f2 > f1 {
            alpha3 = alpha2;
            f3 = f2;
            alpha2 *= 0.5;
            if !alpha2.is_finite() || alpha2 <= 1e-16 {
                sub.set_params(store, &x0);
                return 0.0;
            }
            sub.set_params(store, &(&x0 + xdir * alpha2));
            f2 = sub.error(store);
        } else if !f3.is_finite() {
            break;
        } else if f2 > f3 {
            if alpha3 >= alpha_max {
                break;
            }
            alpha2 = alpha3;
            f2 = f3;
            alpha3 *= 2.0;
            if !alpha3.is_finite() || alpha3 > alpha_max {
                alpha3 = alpha_max;
                sub.set_params(store, &(&x0 + xdir * alpha3));
                f3 = sub.error(store);
                break;
            }
            sub.set_params(store, &(&x0 + xdir * alpha3));
            f3 = sub.error(store);
        }
    }

    let denom = 3.0 * (f1 - 2.0 * f2 + f3);
    let mut alpha_star = if denom.is_finite() && denom.abs() > 1e-30 {
        alpha2 + ((alpha2 - alpha1) * (f1 - f3)) / denom
    } else {
        alpha2
    };
    if !alpha_star.is_finite() || alpha_star >= alpha3 || alpha_star <= alpha1 {
        alpha_star = alpha2;
    }
    if alpha_star > alpha_max {
        alpha_star = alpha_max;
    }
    if !alpha_star.is_finite() || alpha_star < 0.0 || (f2 > f1 && ls_iter >= MAX_LS_ITER) {
        alpha_star = 0.0;
    }

    sub.set_params(store, &(&x0 + xdir * alpha_star));
    if !sub.error(store).is_finite() {
        sub.set_params(store, &x0);
        return 0.0;
    }
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
    if !grad.iter().all(|v| v.is_finite()) || !sub.error(store).is_finite() {
        store.revert(&snapshot);
        return SolveStatus::Failed;
    }

    let mut xdir = -&grad;
    line_search(sub, store, &xdir);
    let mut err = sub.error(store);
    if !err.is_finite() {
        store.revert(&snapshot);
        return SolveStatus::Failed;
    }

    let mut h = &sub.get_params(store) - &x;
    x = sub.get_params(store);

    let diverging_lim = 1e6 * err + 1e12;

    for _iter in 1..MAX_ITER {
        let h_norm = h.norm();
        if h_norm <= CONVERGENCE || err <= SMALL_F {
            break;
        }
        if err > diverging_lim || !err.is_finite() || !h_norm.is_finite() {
            break;
        }

        let grad_old = grad.clone();
        grad = sub.calc_grad(store);
        if !grad.iter().all(|v| v.is_finite()) {
            break;
        }
        let y = &grad - &grad_old;

        let hty = h.dot(&y);
        let y_norm = y.norm();
        let curv_min = (1e-12 * h_norm * y_norm).max(1e-24);
        if hty.is_finite() && curv_min.is_finite() && hty > curv_min {
            let dy = &d * &y;
            let yt_dy = y.dot(&dy);
            if yt_dy.is_finite() {
                let inv_hty = 1.0 / hty;
                let scale = (1.0 + yt_dy * inv_hty) * inv_hty;
                if inv_hty.is_finite() && scale.is_finite() {
                    let next_d = &d
                        + (&h * h.transpose()) * scale
                        - (&h * dy.transpose() + &dy * h.transpose()) * inv_hty;
                    if next_d.iter().all(|v| v.is_finite()) {
                        d = next_d;
                    } else {
                        d = nalgebra::DMatrix::<f64>::identity(xsize, xsize);
                    }
                }
            }
        }

        xdir = -(&d * &grad);
        if !xdir.iter().all(|v| v.is_finite()) || grad.dot(&xdir) >= 0.0 {
            d = nalgebra::DMatrix::<f64>::identity(xsize, xsize);
            xdir = -&grad;
        }
        line_search(sub, store, &xdir);
        err = sub.error(store);

        h = &sub.get_params(store) - &x;
        x = sub.get_params(store);
    }

    if err.is_finite() && err <= SMALL_F {
        return SolveStatus::Success;
    }
    if err.is_finite() && err <= diverging_lim && h.norm().is_finite() && h.norm() <= CONVERGENCE {
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
    use crate::util::ParamId;
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

    #[test]
    fn non_finite_gradient_fails_cleanly_and_reverts_store() {
        struct NanGrad(ParamId);
        impl crate::constraints::Constraint for NanGrad {
            fn params(&self) -> Vec<ParamId> {
                vec![self.0]
            }
            fn error_value(&self, _store: &ParamStore) -> f64 {
                1.0
            }
            fn grad_value(&self, _store: &ParamStore, _param: ParamId) -> f64 {
                f64::NAN
            }
        }
        let mut store = ParamStore::new();
        let x = store.add(7.5, false);
        let c: Rc<dyn crate::constraints::Constraint> = Rc::new(NanGrad(x));
        let sub = SubSystem::new(vec![c], &[x]);
        assert_eq!(solve_bfgs(&sub, &mut store), SolveStatus::Failed);
        assert_eq!(store.get(x), 7.5);
    }

    #[test]
    fn flat_line_search_curvature_terminates_without_hanging() {
        // Error stays flat (1.0) at a distant point while grad tries to push
        // across zero-curvature plateau: line_search must exit within MAX_LS_ITER.
        struct Plateau(ParamId);
        impl crate::constraints::Constraint for Plateau {
            fn params(&self) -> Vec<ParamId> {
                vec![self.0]
            }
            fn error_value(&self, store: &ParamStore) -> f64 {
                let x = store.get(self.0);
                if x > 0.0 {
                    1.0
                } else {
                    2.0
                }
            }
            fn grad_value(&self, _store: &ParamStore, _param: ParamId) -> f64 {
                1.0
            }
        }
        let mut store = ParamStore::new();
        let x = store.add(0.5, false);
        let c: Rc<dyn crate::constraints::Constraint> = Rc::new(Plateau(x));
        let sub = SubSystem::new(vec![c], &[x]);
        let status = solve_bfgs(&sub, &mut store);
        assert_ne!(status, SolveStatus::Success);
    }
}
