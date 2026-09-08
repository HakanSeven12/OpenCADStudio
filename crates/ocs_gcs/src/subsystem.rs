//! Assembles a set of constraints' residual, Jacobian, and gradient against
//! a chosen set of free parameters.
//!
//! Ported from planegcs's `SubSystem.h`/`.cpp`, with one deliberate
//! simplification: the C++ `SubSystem` keeps its own working copy of
//! parameter values (`pvals`) and temporarily redirects each constraint's
//! `double*`s to point into it (`redirectParams`/`revertParams`), so solving
//! can be undone by reverting those pointers, with `applySolution()` copying
//! the working copy back at the end. Here, constraints always address
//! [`ParamStore`] directly through [`ParamId`]s (see `util.rs`), so there is
//! no separate working copy to redirect into or apply back — a solver rolls
//! back a bad iteration via [`ParamStore::redirect`]/[`ParamStore::revert`]
//! on the store itself instead. That drops `pvals`/`pmap`/`redirectParams`/
//! `revertParams`/`applySolution`/`getParamMap` as having no counterpart
//! here, and likewise drops the reduction-map constructor variant (used in
//! the C++ to merge coincident-point parameters), deferred to whichever
//! later stage needs that specific optimization.

use std::collections::HashSet;
use std::rc::Rc;

use nalgebra::{DMatrix, DVector};

use crate::constraints::Constraint;
use crate::util::{ParamId, ParamStore};

pub struct SubSystem {
    clist: Vec<Rc<dyn Constraint>>,
    /// The free parameters this subsystem solves for: the intersection of
    /// the candidate `params` passed to [`SubSystem::new`] with the union of
    /// every constraint's own parameters — mirrors the C++'s `tmpplist`
    /// (`s1 ∩ s2`) becoming `plist` in the no-reduction-map path.
    plist: Vec<ParamId>,
}

impl SubSystem {
    pub fn new(clist: Vec<Rc<dyn Constraint>>, params: &[ParamId]) -> Self {
        let candidates: HashSet<ParamId> = params.iter().copied().collect();
        let mut referenced: HashSet<ParamId> = HashSet::new();
        for constr in &clist {
            referenced.extend(constr.params());
        }
        // Preserve `params`' order, matching `std::set_intersection` over
        // ordered sets in the C++ (plist's order is otherwise unobserved by
        // any caller here, but a stable order keeps Jacobian columns and
        // solver output reproducible run to run).
        let plist: Vec<ParamId> = params.iter().copied().filter(|p| referenced.contains(p) && candidates.contains(p)).collect();

        Self { clist, plist }
    }

    pub fn p_size(&self) -> usize {
        self.plist.len()
    }

    pub fn c_size(&self) -> usize {
        self.clist.len()
    }

    pub fn plist(&self) -> &[ParamId] {
        &self.plist
    }

    /// This subsystem's constraints, in the same order [`calc_residual`]/
    /// [`calc_jacobi`]'s rows use — lets a caller (e.g. `diagnosis.rs`) map
    /// a Jacobian row index back to the constraint it came from.
    pub fn constraints(&self) -> &[Rc<dyn Constraint>] {
        &self.clist
    }

    /// `0.5 * sum(constraint.error()^2)` — the scalar objective a solver
    /// minimizes.
    pub fn error(&self, store: &ParamStore) -> f64 {
        let mut err = 0.0;
        for constr in &self.clist {
            let tmp = constr.error(store);
            err += tmp * tmp;
        }
        0.5 * err
    }

    /// One residual entry per constraint, in `clist` order.
    pub fn calc_residual(&self, store: &ParamStore) -> DVector<f64> {
        DVector::from_iterator(self.c_size(), self.clist.iter().map(|c| c.error(store)))
    }

    /// Jacobian of the residual w.r.t. `params`: `csize x params.len()`.
    pub fn calc_jacobi_for(&self, store: &ParamStore, params: &[ParamId]) -> DMatrix<f64> {
        let mut jacobi = DMatrix::zeros(self.c_size(), params.len());
        for (j, &param) in params.iter().enumerate() {
            for (i, constr) in self.clist.iter().enumerate() {
                jacobi[(i, j)] = constr.grad(store, param);
            }
        }
        jacobi
    }

    /// [`calc_jacobi_for`](Self::calc_jacobi_for) against this subsystem's
    /// own [`plist`](Self::plist) — mirrors the C++'s no-argument
    /// `calcJacobi(Eigen::MatrixXd&)` overload.
    pub fn calc_jacobi(&self, store: &ParamStore) -> DMatrix<f64> {
        self.calc_jacobi_for(store, &self.plist.clone())
    }

    /// Gradient of [`error`](Self::error) w.r.t. `params`:
    /// `sum(constraint.error() * constraint.grad(param))` per parameter.
    ///
    /// The C++ restricts this sum to constraints adjacent to `param` via a
    /// `p2c` map; here every constraint is checked for every parameter
    /// instead, relying on [`Constraint::grad`]'s own membership check to
    /// contribute `0.0` for unrelated pairs — same result, no adjacency
    /// bookkeeping to maintain yet. Revisit if profiling ever shows this
    /// O(csize * params.len()) scan mattering at real sketch sizes.
    pub fn calc_grad_for(&self, store: &ParamStore, params: &[ParamId]) -> DVector<f64> {
        let mut grad = DVector::zeros(params.len());
        for (j, &param) in params.iter().enumerate() {
            for constr in &self.clist {
                grad[j] += constr.error(store) * constr.grad(store, param);
            }
        }
        grad
    }

    pub fn calc_grad(&self, store: &ParamStore) -> DVector<f64> {
        self.calc_grad_for(store, &self.plist.clone())
    }

    /// Largest step scale in `(0, lim]` every constraint accepts for a
    /// proposed direction, keyed the same way as `plist`/`dir`.
    pub fn max_step_for(&self, store: &ParamStore, params: &[ParamId], dir: &[f64], lim: f64) -> f64 {
        assert_eq!(params.len(), dir.len());
        let dir_map: std::collections::HashMap<ParamId, f64> =
            params.iter().copied().zip(dir.iter().copied()).collect();
        let mut alpha = lim;
        for constr in &self.clist {
            alpha = constr.max_step(store, &dir_map, alpha);
        }
        alpha
    }

    pub fn max_step(&self, store: &ParamStore, dir: &[f64], lim: f64) -> f64 {
        self.max_step_for(store, &self.plist.clone(), dir, lim)
    }

    /// Reads this subsystem's free parameters' current values from `store`,
    /// in [`plist`](Self::plist) order.
    pub fn get_params(&self, store: &ParamStore) -> DVector<f64> {
        DVector::from_iterator(self.p_size(), self.plist.iter().map(|&p| store.get(p)))
    }

    /// Writes `x` into `store` at this subsystem's free parameters, in
    /// [`plist`](Self::plist) order.
    pub fn set_params(&self, store: &mut ParamStore, x: &DVector<f64>) {
        assert_eq!(x.len(), self.p_size());
        for (i, &p) in self.plist.iter().enumerate() {
            store.set(p, x[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal test-only constraint: `error = a - b` (so `a == b` at the
    /// zero). Keeps these tests independent of any concrete constraint from
    /// a later port stage.
    struct Difference {
        a: ParamId,
        b: ParamId,
    }

    impl Constraint for Difference {
        fn params(&self) -> Vec<ParamId> {
            vec![self.a, self.b]
        }

        fn error_value(&self, store: &ParamStore) -> f64 {
            store.get(self.a) - store.get(self.b)
        }

        fn grad_value(&self, _store: &ParamStore, param: ParamId) -> f64 {
            if param == self.a {
                1.0
            } else if param == self.b {
                -1.0
            } else {
                0.0
            }
        }
    }

    #[test]
    fn plist_is_the_intersection_of_candidates_and_constraint_params() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, false);
        let unrelated = store.add(3.0, false);

        let constr: Rc<dyn Constraint> = Rc::new(Difference { a, b });
        let sub = SubSystem::new(vec![constr], &[a, b, unrelated]);

        assert_eq!(sub.plist(), &[a, b]);
        assert_eq!(sub.p_size(), 2);
        assert_eq!(sub.c_size(), 1);
    }

    #[test]
    fn error_and_residual_reflect_the_constraint_error() {
        let mut store = ParamStore::new();
        let a = store.add(5.0, false);
        let b = store.add(2.0, false);
        let constr: Rc<dyn Constraint> = Rc::new(Difference { a, b });
        let sub = SubSystem::new(vec![constr], &[a, b]);

        assert_eq!(sub.calc_residual(&store)[0], 3.0);
        assert_eq!(sub.error(&store), 0.5 * 3.0 * 3.0);
    }

    #[test]
    fn jacobi_and_grad_match_the_hand_derived_formulas() {
        let mut store = ParamStore::new();
        let a = store.add(5.0, false);
        let b = store.add(2.0, false);
        let constr: Rc<dyn Constraint> = Rc::new(Difference { a, b });
        let sub = SubSystem::new(vec![constr], &[a, b]);

        let jacobi = sub.calc_jacobi(&store);
        assert_eq!(jacobi.nrows(), 1);
        assert_eq!(jacobi.ncols(), 2);
        assert_eq!(jacobi[(0, 0)], 1.0);
        assert_eq!(jacobi[(0, 1)], -1.0);

        // grad[j] = error * d(error)/d(param_j) = 3.0 * (+-1.0)
        let grad = sub.calc_grad(&store);
        assert_eq!(grad[0], 3.0);
        assert_eq!(grad[1], -3.0);
    }

    #[test]
    fn get_and_set_params_round_trip_through_the_store() {
        let mut store = ParamStore::new();
        let a = store.add(5.0, false);
        let b = store.add(2.0, false);
        let constr: Rc<dyn Constraint> = Rc::new(Difference { a, b });
        let sub = SubSystem::new(vec![constr], &[a, b]);

        let x = sub.get_params(&store);
        assert_eq!(x[0], 5.0);
        assert_eq!(x[1], 2.0);

        sub.set_params(&mut store, &DVector::from_vec(vec![10.0, 4.0]));
        assert_eq!(store.get(a), 10.0);
        assert_eq!(store.get(b), 4.0);
    }

    #[test]
    fn max_step_defaults_to_the_limit_when_no_constraint_restricts_it() {
        let mut store = ParamStore::new();
        let a = store.add(5.0, false);
        let b = store.add(2.0, false);
        let constr: Rc<dyn Constraint> = Rc::new(Difference { a, b });
        let sub = SubSystem::new(vec![constr], &[a, b]);

        let alpha = sub.max_step(&store, &[1.0, 1.0], 42.0);
        assert_eq!(alpha, 42.0);
    }
}
