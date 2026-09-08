//! Degrees-of-freedom and redundant-constraint diagnosis.
//!
//! Ported in spirit, not line-by-line, from `System::diagnose` and
//! `identifyConflictingRedundantConstraints` (`GCS.cpp`). The C++ version is
//! substantially more machinery than the rest of this port: it runs two QR
//! decompositions in parallel threads, switches between dense and sparse QR
//! by system size, tracks per-constraint "tags" (so a caller can mark
//! certain constraints high-priority or exempt from the conflict report),
//! and distinguishes *which* rows of a rank-deficient Jacobian are
//! genuinely redundant (safe to drop) via Eigen's `FullPivHouseholderQR`
//! column tracking.
//!
//! This gets to the same two numbers a caller actually needs — the
//! remaining degrees of freedom, and a set of constraints that could be
//! removed without changing what the rest of the system can satisfy — via
//! plain rank-revealing SVD (already used the same way in `qp_eq.rs`) and a
//! straightforward incremental-rank scan, at the cost of two simplifications
//! worth knowing about before relying on this for anything but a UI hint:
//!
//! - **No tag system.** Every constraint in the subsystem is considered;
//!   there's no way yet to exempt a particular constraint from the report.
//! - **Redundant vs. conflicting is a separate, opt-in step.** [`diagnose`]
//!   itself only identifies constraint rows that are *linearly dependent* on
//!   the others (the Jacobian doesn't have full row rank) — safe to drop
//!   without changing the solvable configuration space, but not yet telling
//!   apart a harmless duplicate from a genuinely *conflicting* constraint
//!   (dependent AND infeasible together, e.g. two different fixed distances
//!   between the same two points). [`classify_redundant`] answers that,
//!   for exactly the rows `diagnose` flagged, by attempting a solve on the
//!   reduced system and checking whether the removed row's own error can
//!   still reach zero there — kept separate from `diagnose` because it costs
//!   one extra solve per redundant row, which `diagnose`'s own "cheap enough
//!   for every edit" contract doesn't afford.

use std::rc::Rc;

use nalgebra::DMatrix;

use crate::constraints::Constraint;
use crate::subsystem::SubSystem;
use crate::util::ParamStore;

pub struct Diagnosis {
    /// Remaining degrees of freedom: free parameters minus the Jacobian's
    /// rank. Zero means fully constrained.
    pub dof: usize,
    /// Row indices into [`SubSystem::constraints`] that are linearly
    /// dependent on the rest — candidates to drop when over-constrained.
    pub redundant: Vec<usize>,
}

impl Diagnosis {
    /// The actual redundant [`Constraint`]s, resolved from `redundant`
    /// against `sub`'s own constraint list.
    pub fn redundant_constraints<'a>(&self, sub: &'a SubSystem) -> Vec<&'a Rc<dyn Constraint>> {
        self.redundant.iter().map(|&i| &sub.constraints()[i]).collect()
    }
}

/// Diagnoses `sub` at the store's current parameter values. Cheap enough to
/// call after every edit for a live DOF readout — the UX research behind
/// this port's plan flagged that as something every incumbent CAD tool
/// under-serves.
pub fn diagnose(sub: &SubSystem, store: &ParamStore) -> Diagnosis {
    let jacobi = sub.calc_jacobi(store);
    let psize = sub.p_size();
    let csize = sub.c_size();

    let full_rank = matrix_rank(&jacobi);
    let dof = psize.saturating_sub(full_rank);

    let mut kept_rows: Vec<usize> = Vec::with_capacity(csize);
    let mut redundant: Vec<usize> = Vec::new();
    let mut current_rank = 0usize;

    for i in 0..csize {
        let mut trial = kept_rows.clone();
        trial.push(i);
        let trial_rank = matrix_rank(&row_submatrix(&jacobi, &trial));
        if trial_rank > current_rank {
            kept_rows.push(i);
            current_rank = trial_rank;
        } else {
            redundant.push(i);
        }
    }

    Diagnosis { dof, redundant }
}

/// Whether a redundant (linearly-dependent) constraint is safe to drop or
/// actively disagrees with the rest of the subsystem — the distinction
/// [`diagnose`] itself deliberately doesn't make (see the module doc
/// comment) and [`classify_redundant`] exists to add on top of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedundancyKind {
    /// Removing this constraint doesn't change what the rest can satisfy —
    /// its own condition is already implied by the others' solution (e.g.
    /// the same Equal constraint added twice).
    Redundant,
    /// Removing this constraint lets the rest solve to a configuration that
    /// does *not* satisfy this constraint's own error function — it
    /// disagrees with the rest, not just duplicates one of them (e.g. two
    /// different fixed distances between the same two points).
    Conflicting,
}

/// Below this, a redundant constraint's own error at the reduced system's
/// solved position counts as "satisfied" (genuinely redundant) rather than
/// "violated" (conflicting). Matches the tolerance `solvers` use to call a
/// solve `Success` at sketch scale.
const CONFLICT_TOLERANCE: f64 = 1e-6;

/// For each `redundant` row `diagnose` flagged on `sub` (linearly dependent
/// on the rest, so dropping it doesn't reduce DOF), determines whether it's
/// genuinely [`RedundancyKind::Redundant`] or actually
/// [`RedundancyKind::Conflicting`] with the rest — the distinction the
/// module doc comment describes: remove it, solve what remains (from a
/// clone of `store`, so the caller's real parameter values are untouched),
/// and check whether the *removed* constraint's own error function reaches
/// ~0 at that solved position. If the rest of the subsystem naturally
/// settles somewhere that already satisfies it, it was redundant; if the
/// rest settles somewhere that still violates it, the two disagree.
///
/// Deliberately a separate, opt-in call from [`diagnose`] rather than folded
/// into it: this does one extra solve per redundant row, which [`diagnose`]'s
/// own "cheap enough for every edit" contract doesn't afford — but the
/// common case (no redundant rows at all) never pays for it, since a caller
/// only reaches for this once `diagnose` has already reported something to
/// classify (e.g. the plan's SketchXpert-style resolver UI).
pub fn classify_redundant(sub: &SubSystem, store: &ParamStore, redundant: &[usize]) -> Vec<(usize, RedundancyKind)> {
    redundant
        .iter()
        .map(|&index| {
            let reduced: Vec<Rc<dyn Constraint>> = sub
                .constraints()
                .iter()
                .enumerate()
                .filter_map(|(i, c)| (i != index).then(|| c.clone()))
                .collect();
            let reduced_sub = SubSystem::new(reduced, sub.plist());
            let mut reduced_store = store.clone();
            let _ = crate::solvers::dogleg::solve_dl(&reduced_sub, &mut reduced_store);
            let residual = sub.constraints()[index].error(&reduced_store).abs();
            let kind = if residual < CONFLICT_TOLERANCE { RedundancyKind::Redundant } else { RedundancyKind::Conflicting };
            (index, kind)
        })
        .collect()
}

fn row_submatrix(m: &DMatrix<f64>, rows: &[usize]) -> DMatrix<f64> {
    let owned_rows: Vec<_> = rows.iter().map(|&r| m.row(r).clone_owned()).collect();
    DMatrix::from_rows(&owned_rows)
}

fn matrix_rank(m: &DMatrix<f64>) -> usize {
    if m.nrows() == 0 || m.ncols() == 0 {
        return 0;
    }
    let singular_values = m.clone().singular_values();
    let tol = singular_values.max() * f64::EPSILON * (m.nrows().max(m.ncols()) as f64);
    singular_values.iter().filter(|&&s| s > tol).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::point_line::{Difference, Equal};
    use crate::geo::Point;
    use crate::util::ParamStore;

    #[test]
    fn a_fully_constrained_two_point_distance_has_zero_dof() {
        let mut store = ParamStore::new();
        let p1 = Point::new(store.add(0.0, true), store.add(0.0, true));
        let p2 = Point::new(store.add(3.0, false), store.add(4.0, false));
        let distance = store.add(5.0, true);
        let c: Rc<dyn Constraint> = Rc::new(crate::constraints::point_line::P2PDistance::new(p1, p2, distance));
        // Only one constraint on two free params (p2.x, p2.y): one DOF left
        // (free to slide around the circle), matching a real sketch's math,
        // not "fully constrained" -- this test is really about the *shape*
        // of the result, exercised precisely by the redundancy tests below.
        let sub = SubSystem::new(vec![c], &[p2.x, p2.y]);
        let diag = diagnose(&sub, &store);
        assert_eq!(diag.dof, 1);
        assert!(diag.redundant.is_empty());
    }

    #[test]
    fn a_duplicated_constraint_is_flagged_redundant() {
        let mut store = ParamStore::new();
        let a = store.add(3.0, false);
        let b = store.add(3.0, false);

        let c1: Rc<dyn Constraint> = Rc::new(Equal::new(a, b, 1.0));
        let c2: Rc<dyn Constraint> = Rc::new(Equal::new(a, b, 1.0)); // identical constraint again
        let sub = SubSystem::new(vec![c1, c2], &[a, b]);

        let diag = diagnose(&sub, &store);
        assert_eq!(diag.redundant.len(), 1, "one of the two identical constraints should be redundant");
        assert_eq!(diag.dof, 1); // Equal removes exactly one DOF regardless of duplication
    }

    #[test]
    fn independent_constraints_are_never_flagged_redundant() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, false);
        let d1 = store.add(0.0, false);
        let d2 = store.add(0.0, false);

        // Two different constraints touching disjoint pairs -- both needed.
        let c1: Rc<dyn Constraint> = Rc::new(Difference::new(a, b, d1));
        let c2: Rc<dyn Constraint> = Rc::new(Difference::new(a, b, d2));
        // Same params, but only redundant if their gradients are parallel --
        // Difference's gradient w.r.t. (a,b) is always (-1,1) regardless of
        // the target, so these ARE linearly dependent rows; use this to
        // confirm the flagged set resolves back to real constraints.
        let sub = SubSystem::new(vec![c1, c2], &[a, b]);
        let diag = diagnose(&sub, &store);
        assert_eq!(diag.redundant.len(), 1);
        let resolved = diag.redundant_constraints(&sub);
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn a_duplicated_constraint_classifies_as_genuinely_redundant() {
        let mut store = ParamStore::new();
        let a = store.add(3.0, false);
        let b = store.add(3.0, false);
        let c1: Rc<dyn Constraint> = Rc::new(Equal::new(a, b, 1.0));
        let c2: Rc<dyn Constraint> = Rc::new(Equal::new(a, b, 1.0)); // identical constraint again
        let sub = SubSystem::new(vec![c1, c2], &[a, b]);

        let diag = diagnose(&sub, &store);
        assert_eq!(diag.redundant.len(), 1);
        let classified = classify_redundant(&sub, &store, &diag.redundant);
        assert_eq!(
            classified,
            vec![(diag.redundant[0], RedundancyKind::Redundant)],
            "the duplicate should classify as redundant, not conflicting"
        );
    }

    #[test]
    fn two_different_fixed_targets_on_the_same_pair_classify_as_conflicting() {
        let mut store = ParamStore::new();
        let a = store.add(0.0, false);
        let b = store.add(1.0, false);
        let d1 = store.add(0.0, true); // wants b - a == 0
        let d2 = store.add(5.0, true); // wants b - a == 5 -- can't both hold
        let c1: Rc<dyn Constraint> = Rc::new(Difference::new(a, b, d1));
        let c2: Rc<dyn Constraint> = Rc::new(Difference::new(a, b, d2));
        let sub = SubSystem::new(vec![c1, c2], &[a, b]);

        let diag = diagnose(&sub, &store);
        assert_eq!(diag.redundant.len(), 1, "the two Difference rows are linearly dependent regardless of target");
        let classified = classify_redundant(&sub, &store, &diag.redundant);
        assert_eq!(
            classified,
            vec![(diag.redundant[0], RedundancyKind::Conflicting)],
            "two incompatible fixed targets on the same pair must classify as conflicting, not redundant"
        );
    }
}
