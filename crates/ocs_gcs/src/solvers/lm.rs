//! Levenberg-Marquardt — damped Gauss-Newton least squares.
//!
//! Ported from `System::solve_LM` (`GCS.cpp`). Defaults match planegcs's
//! own (`System::System()`): `eps = 1e-10`, `eps1 = 1e-80`, `tau = 1e-3`,
//! `max_iter = 100` (as with `dogleg.rs`, the "redundant-solving" tolerance
//! variants and the `sketchSizeMultiplier`-scaled iteration cap aren't
//! exposed here). Store snapshot/revert-on-failure replaces the C++'s
//! scratch-copy-plus-separate-`applySolution()` pattern exactly as in
//! `dogleg.rs` — see that module's doc comment and `subsystem.rs`'s for why.
//!
//! One faithful-but-odd detail kept as-is: the inner damping-adjustment loop
//! counts rejected steps in `k` and stops after `k` reaches 50, but the
//! C++'s `if (k > 50) { stop = 7; ... }` check after that loop can never be
//! true (`k` never exceeds 50, only reaches it) — so that branch is
//! effectively dead code upstream, and stays dead code here rather than
//! being "corrected" into the probably-intended `k >= 50`.

use crate::solvers::SolveStatus;
use crate::subsystem::SubSystem;
use crate::util::ParamStore;

const EPS: f64 = 1e-10;
const EPS1: f64 = 1e-80;
const TAU: f64 = 1e-3;
const MAX_ITER: usize = 100;

pub fn solve_lm(sub: &SubSystem, store: &mut ParamStore) -> SolveStatus {
    let xsize = sub.p_size();
    if xsize == 0 {
        return SolveStatus::Success;
    }

    let snapshot = store.redirect();

    let mut x = sub.get_params(store);
    let mut e = -sub.calc_residual(store);

    let diverging_lim = 1e6 * e.norm_squared() + 1e12;
    let mut nu = 2.0_f64;
    let mut mu = 0.0_f64;
    let mut stop = 0u8;

    'outer: for iter in 0..MAX_ITER {
        let err = e.norm_squared();
        if err <= EPS * EPS {
            stop = 1;
            break;
        } else if err > diverging_lim || err.is_nan() {
            stop = 6;
            break;
        }

        let j = sub.calc_jacobi(store);
        let mut a = j.transpose() * &j;
        let g = j.transpose() * &e;

        let g_inf = g.amax();
        let diag_a = a.diagonal().clone_owned();

        if g_inf <= EPS1 {
            stop = 2;
            break;
        }

        if iter == 0 {
            mu = TAU * diag_a.amax();
        }

        let mut k = 0u32;
        loop {
            if k >= 50 {
                break;
            }

            for i in 0..xsize {
                a[(i, i)] += mu;
            }

            let h = a.clone().lu().solve(&g).unwrap_or_else(|| nalgebra::DVector::zeros(xsize));
            let rel_error = (&a * &h - &g).norm() / g.norm();

            if rel_error < 1e-5 {
                let scale = sub.max_step(store, h.as_slice(), 1e10);
                let h = if scale < 1.0 { &h * scale } else { h };

                let x_new = &x + &h;
                let h_norm = h.norm_squared();

                if h_norm <= EPS1 * EPS1 * x.norm() {
                    stop = 3;
                    break;
                } else if h_norm >= (x.norm() + EPS1) / (f64::EPSILON * f64::EPSILON) {
                    stop = 4;
                    break;
                }

                sub.set_params(store, &x_new);
                let e_new = -sub.calc_residual(store);

                let df = e.norm_squared() - e_new.norm_squared();
                let dl = h.dot(&(&h * mu + &g));

                if df > 0.0 && dl > 0.0 {
                    let tmp = 2.0 * df / dl - 1.0;
                    mu *= (1.0 / 3.0_f64).max(1.0 - tmp * tmp * tmp);
                    nu = 2.0;
                    x = x_new;
                    e = e_new;
                    break;
                }
                // Rejected: undo the trial write into the store (no
                // separate scratch copy here to have kept it out of in the
                // first place -- see the module doc).
                sub.set_params(store, &x);
            }

            // Rejected (or the linear solve was unreliable): back off and
            // retry with a fresh damping factor.
            mu *= nu;
            nu *= 2.0;
            for i in 0..xsize {
                a[(i, i)] = diag_a[i];
            }
            k += 1;
        }

        if stop != 0 {
            break 'outer;
        }
        if k > 50 {
            // Dead code upstream (see module doc) -- kept for fidelity.
            stop = 7;
            break 'outer;
        }
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
        let p1 = Point::new(store.add(0.0, true), store.add(0.0, true));
        let p2 = Point::new(store.add(1.0, false), store.add(0.0, false));
        let distance = store.add(5.0, true);

        let constr: Rc<dyn crate::constraints::Constraint> = Rc::new(P2PDistance::new(p1, p2, distance));
        let sub = SubSystem::new(vec![constr], &[p2.x, p2.y]);

        let status = solve_lm(&sub, &mut store);
        assert_eq!(status, SolveStatus::Success);
        assert!(sub.error(&store).abs() < 1e-9);

        let dx = store.get(p1.x) - store.get(p2.x);
        let dy = store.get(p1.y) - store.get(p2.y);
        assert!(((dx * dx + dy * dy).sqrt() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn a_zero_parameter_subsystem_trivially_succeeds() {
        let mut store = ParamStore::new();
        let sub = SubSystem::new(vec![], &[]);
        assert_eq!(solve_lm(&sub, &mut store), SolveStatus::Success);
    }
}
