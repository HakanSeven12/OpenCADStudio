//! The top-level solver system: owns the parameter store and the full
//! constraint list, and partitions them into independent [`SubSystem`]s.
//!
//! Ported from planegcs's `GCS::System` (`GCS.h`/`.cpp`), scoped to what
//! this port stage needs: parameter/constraint bookkeeping and partitioning.
//! The C++'s per-constraint-type `addConstraintXxx(...)` factory methods
//! aren't ported as named wrappers yet — `add_constraint` takes any already-
//! constructed [`Constraint`] (from `constraints::point_line`,
//! `::angle_distance`, `::circle_arc`) directly, which is equivalent for
//! everything actually ported so far, just less convenient at a call site
//! than a per-type method would be. The three solving algorithms
//! (`solve_BFGS`/`solve_LM`/`solve_DL`) and redundant/conflicting-constraint
//! diagnosis are separate, later port stages (`solvers/`, `diagnosis.rs`).

use std::rc::Rc;

use crate::constraints::Constraint;
use crate::graph;
use crate::subsystem::SubSystem;
use crate::util::{ParamId, ParamStore};

pub struct System {
    store: ParamStore,
    constraints: Vec<Rc<dyn Constraint>>,
}

impl System {
    pub fn new() -> Self {
        Self { store: ParamStore::new(), constraints: Vec::new() }
    }

    pub fn store(&self) -> &ParamStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut ParamStore {
        &mut self.store
    }

    pub fn add_param(&mut self, value: f64, driven: bool) -> ParamId {
        self.store.add(value, driven)
    }

    pub fn add_constraint(&mut self, constraint: Rc<dyn Constraint>) {
        self.constraints.push(constraint);
    }

    pub fn constraints(&self) -> &[Rc<dyn Constraint>] {
        &self.constraints
    }

    /// Splits the current constraint list into independent [`SubSystem`]s —
    /// mirrors `System::initSolution`'s partitioning step (`GCS.cpp`),
    /// minus the equality-constraint parameter-reduction and
    /// redundant/conflicting-constraint exclusion that step also does
    /// (those belong to the later `diagnosis.rs` stage; every constraint
    /// here is treated as driving and undiagnosed).
    ///
    /// A `SubSystem`'s free-parameter list is every one of its constraints'
    /// referenced params *excluding* the ones marked `driven` in the store
    /// (a dimensional constraint's target value, or any point the caller
    /// added with `driven: true` to keep fixed) — the solver moves the
    /// former to satisfy error = 0, and must never move the latter. Every
    /// hand-built `SubSystem::new(..., candidates)` call elsewhere in this
    /// port (and every one-shot bridge in the app crate) passes its own
    /// explicit non-driven candidate list for exactly this reason; this is
    /// `System`'s equivalent, computed automatically from the store instead
    /// of by the caller.
    pub fn partition(&self) -> Vec<SubSystem> {
        graph::partition_by_shared_params(&self.constraints)
            .into_iter()
            .map(|indices| {
                let clist: Vec<Rc<dyn Constraint>> = indices.iter().map(|&i| self.constraints[i].clone()).collect();
                let mut params: Vec<ParamId> =
                    clist.iter().flat_map(|c| c.params()).filter(|&p| !self.store.is_driven(p)).collect();
                params.sort();
                params.dedup();
                SubSystem::new(clist, &params)
            })
            .collect()
    }
}

impl Default for System {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::point_line::{Difference, P2PDistance};
    use crate::geo::Point;

    #[test]
    fn a_two_point_distance_sketch_builds_partitions_and_round_trips() {
        // Two independent constraints on disjoint geometry: a P2PDistance
        // between two points, and an unrelated Difference — should land in
        // two separate subsystems.
        let mut sys = System::new();

        let p1 = Point::new(sys.add_param(0.0, false), sys.add_param(0.0, false));
        let p2 = Point::new(sys.add_param(3.0, false), sys.add_param(4.0, false));
        let distance = sys.add_param(5.0, true); // driven: the target distance
        sys.add_constraint(Rc::new(P2PDistance::new(p1, p2, distance)));

        let a = sys.add_param(1.0, false);
        let b = sys.add_param(2.0, false);
        let d = sys.add_param(1.0, true);
        sys.add_constraint(Rc::new(Difference::new(a, b, d)));

        assert_eq!(sys.constraints().len(), 2);

        let mut subsystems = sys.partition();
        assert_eq!(subsystems.len(), 2, "the two constraints share no parameters");

        // "Stub solve": no actual iterative solver yet (stage 8) — just
        // confirm the assembled pipeline (partition -> residual/jacobi ->
        // get/set params) round-trips correctly end to end.
        subsystems.sort_by_key(|s| s.c_size().max(s.p_size()));
        for sub in &subsystems {
            let residual_before = sub.calc_residual(sys.store());
            let x = sub.get_params(sys.store());
            sub.set_params(sys.store_mut(), &x); // no-op round trip
            let residual_after = sub.calc_residual(sys.store());
            assert_eq!(residual_before, residual_after);
        }

        // The P2PDistance subsystem: 4 free params (p1.x, p1.y, p2.x, p2.y)
        // — its `distance` target is `driven`, so `partition` must exclude
        // it from the free list despite `P2PDistance::params()` returning
        // it alongside the point coordinates.
        let p2p_sub = subsystems.iter().find(|s| s.p_size() == 4).expect("the 4-param P2PDistance subsystem");
        // Points already satisfy distance=5 (3-4-5 triangle), so its
        // residual should already be ~zero.
        assert!(p2p_sub.error(sys.store()).abs() < 1e-9);
    }

    #[test]
    fn partition_excludes_driven_params_from_each_subsystems_free_list() {
        let mut sys = System::new();
        let p1 = Point::new(sys.add_param(0.0, true), sys.add_param(0.0, true)); // fixed anchor
        let p2 = Point::new(sys.add_param(1.0, false), sys.add_param(0.0, false)); // free, wrong distance
        let distance = sys.add_param(5.0, true); // driven target
        sys.add_constraint(Rc::new(P2PDistance::new(p1, p2, distance)));

        let subsystems = sys.partition();
        assert_eq!(subsystems.len(), 1);
        // Only p2.x/p2.y are free — p1's coords and the distance target are
        // all `driven`, so despite being referenced by `P2PDistance::params()`
        // they must not appear in the subsystem's free-parameter list.
        assert_eq!(subsystems[0].p_size(), 2);
    }
}
