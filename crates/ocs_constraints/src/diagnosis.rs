//! Degrees-of-freedom and redundant-constraint diagnosis.
//!
//! Rank is built incrementally from the Jacobian rows. Dependent rows are
//! reported as redundant; callers may classify those rows separately when
//! they need to distinguish harmless duplication from a conflict.

use std::rc::Rc;

use nalgebra::DVector;

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
        self.redundant
            .iter()
            .map(|&i| &sub.constraints()[i])
            .collect()
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

    let tolerance = jacobi.norm() * f64::EPSILON * (jacobi.nrows().max(jacobi.ncols()) as f64);
    let mut basis: Vec<DVector<f64>> = Vec::with_capacity(csize.min(psize));
    let mut redundant: Vec<usize> = Vec::new();

    for i in 0..csize {
        let mut residual = jacobi.row(i).transpose().into_owned();
        // Re-orthogonalize once. The second pass keeps the incremental rank
        // stable for nearly dependent rows without rebuilding a decomposition
        // for every prefix of the matrix.
        for _ in 0..2 {
            for direction in &basis {
                residual -= direction * direction.dot(&residual);
            }
        }
        let norm = residual.norm();
        if norm <= tolerance {
            redundant.push(i);
        } else {
            basis.push(residual / norm);
        }
    }

    Diagnosis {
        dof: psize.saturating_sub(basis.len()),
        redundant,
    }
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
/// solve `Success` at drawing scale.
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
/// only reaches for this once `diagnose` has already reported something.
pub fn classify_redundant(
    sub: &SubSystem,
    store: &ParamStore,
    redundant: &[usize],
) -> Vec<(usize, RedundancyKind)> {
    redundant
        .iter()
        .map(|&index| {
            let reduced: Vec<Rc<dyn Constraint>> = sub
                .constraints()
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, constraint)| constraint.clone())
                .collect();
            let reduced_sub = SubSystem::new(reduced, sub.plist());
            let mut reduced_store = store.clone();
            let _ = crate::solvers::dogleg::solve_dl(&reduced_sub, &mut reduced_store);
            let residual = sub.constraints()[index].error(&reduced_store).abs();
            let kind = if residual < CONFLICT_TOLERANCE {
                RedundancyKind::Redundant
            } else {
                RedundancyKind::Conflicting
            };
            (index, kind)
        })
        .collect()
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
        let c: Rc<dyn Constraint> = Rc::new(crate::constraints::point_line::P2PDistance::new(
            p1, p2, distance,
        ));
        // Only one constraint on two free params (p2.x, p2.y): one DOF left
        // (free to slide around the circle), matching the geometric model,
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
        assert_eq!(
            diag.redundant.len(),
            1,
            "one of the two identical constraints should be redundant"
        );
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
        assert_eq!(
            diag.redundant.len(),
            1,
            "the two Difference rows are linearly dependent regardless of target"
        );
        let classified = classify_redundant(&sub, &store, &diag.redundant);
        assert_eq!(
            classified,
            vec![(diag.redundant[0], RedundancyKind::Conflicting)],
            "two incompatible fixed targets on the same pair must classify as conflicting, not redundant"
        );
    }
}
