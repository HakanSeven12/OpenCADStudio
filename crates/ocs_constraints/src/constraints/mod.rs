//! The `Constraint` trait: one primitive geometric relationship the solver
//! can evaluate an error and gradient for.
//!
//! Ported from planegcs's base `Constraint` class (`Constraints.h`/`.cpp`).
//! Concrete constraint types (Equal, P2PDistance, Parallel, ...) are added in
//! later port stages, grouped by geometric complexity, under this module.

use std::collections::HashMap;

use crate::util::{ParamId, ParamStore};

/// A geometric constraint: an unscaled error function of its own parameters,
/// differentiable w.r.t. any one of them.
///
/// `error()`/`grad()` provide the *scaled* values (`scale * error_value()`,
/// `scale * grad_value()`) a [`SubSystem`](crate::subsystem::SubSystem)
/// assembles into its residual/Jacobian/gradient — mirroring the base
/// `Constraint::error()`/`::grad()` in the C++, which wrap `errorgrad()`
/// (or, for most concrete constraints, an overridden `error()`/`grad()`
/// pair) the same way.
pub trait Constraint {
    /// This constraint's own parameters, in a fixed order specific to the
    /// constraint type — the C++'s `pvec`.
    fn params(&self) -> Vec<ParamId>;

    /// Defaults to `1.0`, matching the C++ constructor's `scale(1.)`.
    fn scale(&self) -> f64 {
        1.0
    }

    /// Unscaled constraint error at the store's current values.
    fn error_value(&self, store: &ParamStore) -> f64;

    /// Unscaled derivative of [`error_value`](Self::error_value) w.r.t.
    /// `param`. Only ever called for a `param` known to be one of
    /// [`params`](Self::params) — [`grad`](Self::grad) is what does that
    /// membership check, matching the C++'s `findParamInPvec` guard in the
    /// base class's `grad()`.
    fn grad_value(&self, store: &ParamStore, param: ParamId) -> f64;

    fn error(&self, store: &ParamStore) -> f64 {
        self.scale() * self.error_value(store)
    }

    fn grad(&self, store: &ParamStore, param: ParamId) -> f64 {
        if !self.params().contains(&param) {
            return 0.0;
        }
        self.scale() * self.grad_value(store, param)
    }

    /// Limits a proposed solver step so this constraint stays satisfiable
    /// (e.g. a radius must not go negative). `dir` gives the proposed
    /// per-parameter step for whichever of this constraint's parameters are
    /// moving. Defaults to leaving `lim` unchanged, matching the C++ base
    /// class's default `return lim;`.
    fn max_step(&self, _store: &ParamStore, _dir: &HashMap<ParamId, f64>, lim: f64) -> f64 {
        lim
    }
}

pub mod angle_distance;
pub mod bspline;
pub mod circle_arc;
pub mod conic;
pub mod curve_generic;
pub mod point_line;

/// Shared test support: cross-checks a constraint's analytic `grad_value`
/// against central-difference numerical differentiation of `error_value`,
/// for every one of its own parameters. This is Oracle B from the port
/// implementation — cheap, independent of external fixtures, and effective at
/// catching a mistranscribed formula (a wrong sign or dropped term in
/// `grad_value` almost never happens to match the numeric derivative by
/// chance).
#[cfg(test)]
pub(crate) mod test_support {
    use super::Constraint;
    use crate::util::{ParamId, ParamStore};

    pub fn assert_grad_matches_finite_difference(constr: &dyn Constraint, store: &mut ParamStore) {
        for param in constr.params() {
            assert_single_param_grad_matches_fd(constr, store, param);
        }
    }

    /// Like [`assert_grad_matches_finite_difference`] but for one named
    /// parameter, so a caller can skip a specific param known to have a
    /// documented upstream quirk instead of the whole constraint.
    pub fn assert_single_param_grad_matches_fd(
        constr: &dyn Constraint,
        store: &mut ParamStore,
        param: ParamId,
    ) {
        const H: f64 = 1e-6;
        const TOL: f64 = 1e-5;
        let original = store.get(param);

        store.set(param, original + H);
        let plus = constr.error_value(store);
        store.set(param, original - H);
        let minus = constr.error_value(store);
        store.set(param, original);

        let numeric = (plus - minus) / (2.0 * H);
        let analytic = constr.grad_value(store, param);
        assert!(
            (numeric - analytic).abs() < TOL,
            "grad mismatch for param {param:?}: analytic={analytic}, numeric={numeric}"
        );
    }
}
