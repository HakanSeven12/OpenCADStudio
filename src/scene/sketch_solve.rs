//! Wires [`sketch_constraints::SketchConstraintSet`] into `Scene::bump_entities`
//! — design doc §4.1/§4.2, stage 3 of its §8 staged plan.
//!
//! `refresh_sketch_constraints` is one more link in `bump_entities`'s
//! existing in-line derived-geometry chain, alongside
//! `refresh_associative_dimensions`/`_hatches`/`_centerlines`: given the
//! handles that just changed, find which constraint scopes are affected,
//! rebuild each affected scope's `ocs_gcs::System` from scratch (full
//! rebuild-and-solve, no incremental state — see the design doc §4.2 for
//! why), solve, and write any moved geometry back through the same
//! undo-recording path those other passes use.
//!
//! Maps every [`ConstraintKind`]. `Tangent` covers Line-Circle
//! (`C2LDistance` with a driven zero-distance target aliased to the
//! circle's own radius via `internal: false`) and Circle-Circle
//! (`TangentCircumf`) — Line-Line has no meaning and Arc isn't a supported
//! [`EntityGeom`] yet (matches that type's own documented scope), so those
//! ref shapes build nothing, same "skip, don't panic" contract every other
//! unbuildable constraint already gets. `ccw`/`internal` are picked from
//! the pair's *current* geometry (which side of the line the circle already
//! sits on; whether the circles are already nested) so the very first solve
//! doesn't have to cross a sign-flip singularity to reach the nearest valid
//! tangent configuration.

use std::collections::HashMap;
use std::rc::Rc;

use acadrust::entities::EntityType;
use acadrust::types::Handle;

use ocs_gcs::constraints::angle_distance::L2LAngle;
use ocs_gcs::constraints::circle_arc::{C2LDistance, TangentCircumf};
use ocs_gcs::constraints::point_line::{
    Equal, EqualLineLength, Parallel as ParallelConstraint, Perpendicular as PerpendicularConstraint, P2PDistance,
};
use ocs_gcs::constraints::Constraint;
use ocs_gcs::geo::{Circle as GCircle, Line as GLine, Point as GPoint};
use ocs_gcs::solvers::dogleg::solve_dl;
use ocs_gcs::system::System;

use super::sketch_constraints::{ConstraintId, ConstraintKind, SketchConstraint, SketchConstraintSet, SketchRef};
use super::{ChangeKind, Scene};

/// One referenced entity's geometry, registered into an `ocs_gcs::System`'s
/// parameter store. Only the two entity types the existing one-shot
/// constraint commands already support (`crate::modules::draw::constrain`)
/// — extending this to arcs/ellipses is future work, not a gap introduced
/// here.
#[derive(Clone, Copy)]
enum EntityGeom {
    Line(GLine),
    Circle(GCircle),
}

impl EntityGeom {
    /// The `ocs_gcs::geo::Point` for a marker on this entity — `None` for
    /// a marker this entity type/value doesn't support (see
    /// `sketch_constraints::resolve_point` for the same convention on the
    /// read side).
    fn point_for_marker(&self, marker: i32) -> Option<GPoint> {
        match (self, marker) {
            (EntityGeom::Line(l), 0) => Some(l.p1),
            (EntityGeom::Line(l), 1) => Some(l.p2),
            (EntityGeom::Circle(c), -3) => Some(c.center),
            _ => None,
        }
    }
}

/// Reads `handle`'s live geometry and registers it as fresh, free (never
/// `driven`) parameters in `sys` — every referenced entity is solved for,
/// nothing is a priori fixed; per planegcs-style solvers, an under-
/// constrained scope simply converges to the nearest configuration to its
/// current one, which is the correct behavior for a persistent system where
/// "what stays put" is a property of how many constraints exist, not a
/// convention about which entity was picked first (contrast the one-shot
/// `constrain::apply_*` commands, which do fix the first-picked entity —
/// they solve exactly one constraint in isolation, so they need that
/// convention; a whole scope's constraint graph does not).
fn register_entity(document: &acadrust::CadDocument, sys: &mut System, handle: Handle) -> Option<EntityGeom> {
    match document.get_entity(handle)? {
        EntityType::Line(l) => {
            let p1 = GPoint::new(sys.add_param(l.start.x, false), sys.add_param(l.start.y, false));
            let p2 = GPoint::new(sys.add_param(l.end.x, false), sys.add_param(l.end.y, false));
            Some(EntityGeom::Line(GLine { p1, p2 }))
        }
        EntityType::Circle(c) => {
            let center = GPoint::new(sys.add_param(c.center.x, false), sys.add_param(c.center.y, false));
            let rad = sys.add_param(c.radius, false);
            Some(EntityGeom::Circle(GCircle { center, rad }))
        }
        _ => None,
    }
}

/// Resolves one [`SketchRef`] against the entity-geometry cache, registering
/// the entity on first use. `None` if the handle is dangling, isn't a
/// supported entity type, or (for a point ref) uses a marker that entity
/// type doesn't support.
fn resolve_ref(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    r: SketchRef,
) -> Option<EntityGeom> {
    if !cache.contains_key(&r.entity) {
        let geom = register_entity(document, sys, r.entity)?;
        cache.insert(r.entity, geom);
    }
    Some(*cache.get(&r.entity)?)
}

/// One [`SketchConstraint`]'s `ocs_gcs` construction, or `None` if it can't
/// be built (an unsupported/not-yet-mapped kind, a dangling ref, a ref
/// shape the kind doesn't expect — e.g. `Parallel` needs two whole-line
/// refs). A constraint that can't be built is simply skipped for this
/// solve, not an error: geometry it would have constrained is left alone,
/// matching how a dangling associative-dimension reference degrades today
/// rather than aborting the whole recompute.
fn build_constraint(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    c: &SketchConstraint,
) -> Option<Rc<dyn Constraint>> {
    if !c.enabled {
        return None;
    }

    let whole_line = |sys: &mut System, cache: &mut HashMap<_, _>, r: SketchRef| match resolve_ref(document, sys, cache, r)? {
        EntityGeom::Line(l) => Some(l),
        EntityGeom::Circle(_) => None,
    };
    let whole_circle = |sys: &mut System, cache: &mut HashMap<_, _>, r: SketchRef| match resolve_ref(document, sys, cache, r)? {
        EntityGeom::Circle(circ) => Some(circ),
        EntityGeom::Line(_) => None,
    };
    let point_ref = |sys: &mut System, cache: &mut HashMap<_, _>, r: SketchRef| {
        let marker = r.marker?;
        resolve_ref(document, sys, cache, r)?.point_for_marker(marker)
    };

    match c.kind {
        ConstraintKind::Coincident => {
            let [a, b] = c.refs.as_slice() else { return None };
            let (pa, pb) = (point_ref(sys, cache, *a)?, point_ref(sys, cache, *b)?);
            // Two coordinate-equal constraints packaged as one via a small
            // local combinator isn't worth it for a single call site — the
            // caller (`solve_scope`) already `extend`s a `Vec` per
            // constraint, so this returns just the X constraint and Y is
            // added as a second, independent `SketchConstraint`-less
            // push — see `solve_scope`'s handling of this arm specifically.
            Some(Rc::new(Equal::new(pa.x, pb.x, 1.0)))
        }
        ConstraintKind::Horizontal => {
            let l = whole_line(sys, cache, *c.refs.first()?)?;
            Some(Rc::new(Equal::new(l.p1.y, l.p2.y, 1.0)))
        }
        ConstraintKind::Vertical => {
            let l = whole_line(sys, cache, *c.refs.first()?)?;
            Some(Rc::new(Equal::new(l.p1.x, l.p2.x, 1.0)))
        }
        ConstraintKind::Parallel => {
            let [a, b] = c.refs.as_slice() else { return None };
            let (fixed, moving) = (whole_line(sys, cache, *a)?, whole_line(sys, cache, *b)?);
            Some(Rc::new(ParallelConstraint::new(sys.store(), moving, fixed)))
        }
        ConstraintKind::Perpendicular => {
            let [a, b] = c.refs.as_slice() else { return None };
            let (fixed, moving) = (whole_line(sys, cache, *a)?, whole_line(sys, cache, *b)?);
            Some(Rc::new(PerpendicularConstraint::new(sys.store(), moving, fixed)))
        }
        ConstraintKind::Equal => {
            let [a, b] = c.refs.as_slice() else { return None };
            if let (Some(la), Some(lb)) = (whole_line(sys, cache, *a), whole_line(sys, cache, *b)) {
                return Some(Rc::new(EqualLineLength::new(lb, la)));
            }
            let (ca, cb) = (whole_circle(sys, cache, *a)?, whole_circle(sys, cache, *b)?);
            Some(Rc::new(Equal::new(cb.rad, ca.rad, 1.0)))
        }
        ConstraintKind::Distance => {
            let [a, b] = c.refs.as_slice() else { return None };
            let (pa, pb) = (point_ref(sys, cache, *a)?, point_ref(sys, cache, *b)?);
            let target = sys.add_param(c.driving_param?, true);
            Some(Rc::new(P2PDistance::new(pa, pb, target)))
        }
        ConstraintKind::Angle => {
            let [a, b] = c.refs.as_slice() else { return None };
            let (fixed, moving) = (whole_line(sys, cache, *a)?, whole_line(sys, cache, *b)?);
            let angle = sys.add_param(c.driving_param?.to_radians(), true);
            Some(Rc::new(L2LAngle::new(fixed, moving, angle)))
        }
        ConstraintKind::Radius => {
            let circle = whole_circle(sys, cache, *c.refs.first()?)?;
            let target = sys.add_param(c.driving_param?, true);
            Some(Rc::new(Equal::new(circle.rad, target, 1.0)))
        }
        ConstraintKind::Tangent => {
            let [a, b] = c.refs.as_slice() else { return None };
            let (ga, gb) = (resolve_ref(document, sys, cache, *a)?, resolve_ref(document, sys, cache, *b)?);
            match (ga, gb) {
                (EntityGeom::Circle(c1), EntityGeom::Circle(c2)) => {
                    let store = sys.store();
                    let (x1, y1, r1) = (store.get(c1.center.x), store.get(c1.center.y), store.get(c1.rad));
                    let (x2, y2, r2) = (store.get(c2.center.x), store.get(c2.center.y), store.get(c2.rad));
                    let center_dist = ((x1 - x2).powi(2) + (y1 - y2).powi(2)).sqrt();
                    // One circle already sits inside the other's span, rather
                    // than the two side by side — pick internal tangency so
                    // the first solve doesn't have to cross the singularity
                    // between the two tangency configurations.
                    let internal = center_dist < (r1 - r2).abs();
                    Some(Rc::new(TangentCircumf::new(c1.center, c2.center, c1.rad, c2.rad, internal)))
                }
                (EntityGeom::Circle(circ), EntityGeom::Line(line)) | (EntityGeom::Line(line), EntityGeom::Circle(circ)) => {
                    let store = sys.store();
                    let (cx, cy) = (store.get(circ.center.x), store.get(circ.center.y));
                    let (x1, y1) = (store.get(line.p1.x), store.get(line.p1.y));
                    let (x2, y2) = (store.get(line.p2.x), store.get(line.p2.y));
                    // Signed area of (p2-p1) × (center-p1) — same formula
                    // `C2LDistance::signed_value` itself uses — so `ccw`'s
                    // sign matches whichever side the circle already sits on.
                    let area = (x2 - x1) * (cy - y1) - (y2 - y1) * (cx - x1);
                    let ccw = area >= 0.0;
                    // A driven, fixed zero: with `internal: false` this makes
                    // `C2LDistance`'s target exactly the circle's own radius
                    // (see its `error_grad`), i.e. plain tangency rather than
                    // an offset distance.
                    let zero = sys.add_param(0.0, true);
                    Some(Rc::new(C2LDistance::new(circ, line, zero, ccw, false)))
                }
                // Line-Line tangency has no meaning; Arc isn't a supported
                // `EntityGeom` yet (see that type's own doc comment).
                _ => None,
            }
        }
    }
}

const MOVE_EPS: f64 = 1e-9;

/// Rebuilds `set`'s entire `ocs_gcs::System` from current document
/// geometry, solves it, and returns the resulting entity states for every
/// handle whose registered coordinates actually moved beyond floating-point
/// noise, plus the scope's total remaining degrees of freedom (summed across
/// every independent `SubSystem` — design doc §6.3's DOF badge), plus (design
/// doc §6.4, stage 11) any redundant/conflicting constraints found, resolved
/// back to the `ConstraintId`s a `ConflictResolverPanel` can name and offer
/// to remove. `None` if nothing in the scope could be built (no constraint
/// resolved to anything). DOF/conflicts are still returned even when every
/// subsystem fails to solve (rank is a structural property of the Jacobian,
/// not of whether Dogleg converged) — only the geometry write-back is gated
/// on a successful solve.
fn solve_scope(
    document: &acadrust::CadDocument,
    set: &SketchConstraintSet,
) -> Option<(Vec<(Handle, EntityType)>, usize, Vec<(ConstraintId, ocs_gcs::diagnosis::RedundancyKind)>)> {
    let mut sys = System::new();
    let mut cache: HashMap<Handle, EntityGeom> = HashMap::new();
    // Tracks which system-level `ocs_gcs` constraint came from which
    // `SketchConstraint` — a `SubSystem`'s redundant-row indices (from
    // `ocs_gcs::diagnosis`) are local to that partition's own constraint
    // list, not `set.constraints`' indices, and Coincident contributes two
    // system-level constraints (X and Y halves) for one `SketchConstraint`.
    // `Rc::ptr_eq` against this after partitioning resolves a row back to
    // the `ConstraintId` the UI actually names.
    let mut owner: Vec<(Rc<dyn Constraint>, ConstraintId)> = Vec::new();

    for c in &set.constraints {
        let Some(constraint) = build_constraint(document, &mut sys, &mut cache, c) else { continue };
        owner.push((constraint.clone(), c.id));
        sys.add_constraint(constraint);
        // Coincident needs both X and Y equal; `build_constraint` returns
        // only the X half (see its doc comment there) since one
        // `SketchConstraint` maps to one `Rc<dyn Constraint>` everywhere
        // else — add the Y half here instead of complicating that
        // one-constraint-per-kind contract for a single kind.
        if c.kind == ConstraintKind::Coincident {
            if let [a, b] = c.refs.as_slice() {
                if let (Some(pa), Some(pb)) = (
                    a.marker.and_then(|m| cache.get(&a.entity).and_then(|g| g.point_for_marker(m))),
                    b.marker.and_then(|m| cache.get(&b.entity).and_then(|g| g.point_for_marker(m))),
                ) {
                    let y_half: Rc<dyn Constraint> = Rc::new(Equal::new(pa.y, pb.y, 1.0));
                    owner.push((y_half.clone(), c.id));
                    sys.add_constraint(y_half);
                }
            }
        }
    }

    if cache.is_empty() {
        return None;
    }

    let partitions = sys.partition();
    for sub in &partitions {
        solve_dl(sub, sys.store_mut());
    }
    // `System::partition`'s subsystems only include params actually
    // referenced by some constraint (design doc's own note on
    // `System::partition` — the driven-param fix found while building stage
    // 3) — so a registered entity coordinate nothing constrains yet (e.g. a
    // line's X after only a Horizontal constraint pins its Ys) is invisible
    // to `diagnose` entirely, undercounting DOF. Every such untouched free
    // param is unconstrained on its own, i.e. exactly 1 DOF each — added
    // back here rather than fixed in `ocs_gcs::diagnosis`, which correctly
    // has no opinion on params outside the `SubSystem` it was handed.
    let total_free: usize = cache
        .values()
        .map(|g| match g {
            EntityGeom::Line(_) => 4,
            EntityGeom::Circle(_) => 3,
        })
        .sum();
    let touched_free: usize = partitions.iter().map(|s| s.p_size()).sum();
    let mut dof = total_free.saturating_sub(touched_free);
    let mut conflicts: Vec<(ConstraintId, ocs_gcs::diagnosis::RedundancyKind)> = Vec::new();
    for sub in &partitions {
        let diag = ocs_gcs::diagnosis::diagnose(sub, sys.store());
        dof += diag.dof;
        if diag.redundant.is_empty() {
            continue;
        }
        // Opt-in per `classify_redundant`'s own doc comment (one extra solve
        // per redundant row) — only reached when a partition is actually
        // over-constrained, which is rare, so this never costs anything on
        // the common "no redundancy" path.
        for (row, kind) in ocs_gcs::diagnosis::classify_redundant(sub, sys.store(), &diag.redundant) {
            let row_constraint = &sub.constraints()[row];
            if let Some(&(_, id)) = owner.iter().find(|(rc, _)| Rc::ptr_eq(rc, row_constraint)) {
                conflicts.push((id, kind));
            }
        }
    }

    let store = sys.store();
    let mut results = Vec::new();
    for (&handle, geom) in &cache {
        let Some(entity) = document.get_entity(handle) else { continue };
        match (entity, geom) {
            (EntityType::Line(l), EntityGeom::Line(g)) => {
                let (x1, y1, x2, y2) = (store.get(g.p1.x), store.get(g.p1.y), store.get(g.p2.x), store.get(g.p2.y));
                if (x1 - l.start.x).abs() > MOVE_EPS
                    || (y1 - l.start.y).abs() > MOVE_EPS
                    || (x2 - l.end.x).abs() > MOVE_EPS
                    || (y2 - l.end.y).abs() > MOVE_EPS
                {
                    let mut updated = l.clone();
                    updated.start.x = x1;
                    updated.start.y = y1;
                    updated.end.x = x2;
                    updated.end.y = y2;
                    results.push((handle, EntityType::Line(updated)));
                }
            }
            (EntityType::Circle(c), EntityGeom::Circle(g)) => {
                let (cx, cy, r) = (store.get(g.center.x), store.get(g.center.y), store.get(g.rad));
                if (cx - c.center.x).abs() > MOVE_EPS || (cy - c.center.y).abs() > MOVE_EPS || (r - c.radius).abs() > MOVE_EPS
                {
                    let mut updated = c.clone();
                    updated.center.x = cx;
                    updated.center.y = cy;
                    updated.radius = r;
                    results.push((handle, EntityType::Circle(updated)));
                }
            }
            _ => {}
        }
    }
    Some((results, dof, conflicts))
}

impl Scene {
    /// Design doc §4.1's `bump_entities` hook: for every constraint scope
    /// touched by `changes`, re-solve it and write back what moved. Returns
    /// the resulting `(Handle, ChangeKind::Modified)` entries the same way
    /// `refresh_associative_dimensions`/`_hatches` do, for `bump_entities`
    /// to fold into its own `changes` vec.
    pub(crate) fn refresh_sketch_constraints(&mut self, changes: &[(Handle, ChangeKind)]) -> Vec<(Handle, ChangeKind)> {
        // Deletion policy (design doc §5.3/§12, open question 4): an erased
        // entity silently takes its constraints with it — matching FreeCAD
        // — rather than leaving a dangling `SketchRef` around. Done first,
        // and unconditionally over every scope (not just ones a `touched`
        // check would catch), so a scope left with zero constraints after
        // this doesn't attempt a pointless resolve below.
        //
        // Recorded into the *same* undo transaction as the entity removal
        // that caused it (audit finding: ERASE undo silently lost constraint
        // state, since `sketch_constraints` lives outside `document`/
        // `document.objects` and neither of those directories ever saw this
        // mutation) — one undo press now restores both the entity and its
        // constraints together.
        for (handle, kind) in changes {
            if *kind != ChangeKind::Removed {
                continue;
            }
            for i in 0..self.sketch_constraints.len() {
                if self.sketch_constraints[i].constraints_touching(*handle).next().is_some() {
                    let scope = self.sketch_constraints[i].scope;
                    let before = self.sketch_constraints[i].clone();
                    self.record_undo_sketch_constraints_before(scope, before);
                    self.sketch_constraints[i].remove_all_touching(*handle);
                }
            }
        }

        let mut result = Vec::new();
        for i in 0..self.sketch_constraints.len() {
            let touched = changes
                .iter()
                .any(|(handle, _)| self.sketch_constraints[i].constraints_touching(*handle).next().is_some());
            if !touched {
                continue;
            }
            let Some((solved, dof, conflicts)) = solve_scope(&self.document, &self.sketch_constraints[i]) else { continue };
            self.sketch_constraints[i].dof = Some(dof);
            self.sketch_constraints[i].conflicts = conflicts;
            for (handle, new_entity) in solved {
                if let Some(before) = self.document.get_entity_arc(handle) {
                    self.record_undo_before(handle, Some(before));
                }
                if let Some(slot) = self.document.get_entity_mut(handle) {
                    *slot = new_entity;
                }
                result.push((handle, ChangeKind::Modified));
            }
        }
        result
    }

    /// Design doc §7 open question 6, live-drag re-solve: a lighter sibling
    /// of `refresh_sketch_constraints` for a grip drag's per-mouse-move
    /// update — same "find touched scopes, rebuild the `ocs_gcs::System`
    /// from scratch, solve" core, but:
    /// - only reports what moved (`(Handle, EntityType)`, the actual new
    ///   state) rather than writing it into `self.document` itself. A live
    ///   grip drag already writes the directly-dragged handle's geometry
    ///   straight into the document each frame (`Scene::apply_grip`), so
    ///   this can read that live state via `solve_scope`'s normal document
    ///   read — but the caller (the drag's per-frame update in
    ///   `viewport.rs`) owns deciding how/when to write the *solved*
    ///   result back, since it also has to fold the touched handles into
    ///   this frame's preview/mesh/hatch refresh and the eventual
    ///   grip-release undo group.
    /// - skips `record_undo_before` entirely: a grip drag tracks its
    ///   before/after through its own `grip_originals`/`grip_preview_handles`
    ///   arrays, committed as one `push_entity_group_history` group on
    ///   release, not through the recording session
    ///   `refresh_sketch_constraints` otherwise participates in — calling
    ///   `record_undo_before` here with no such session active for a grip
    ///   drag would either no-op uselessly or, worse, assume a recording
    ///   context that doesn't exist for this call path.
    /// - skips the deletion-policy pass: nothing is erased mid-drag.
    ///
    /// Cheap no-op when there are no sketch constraints at all (the common
    /// case), and per the design doc's own §4.2 architecture this still
    /// does a full rebuild-and-solve per touched scope on every call — i.e.
    /// every mouse-move frame during a drag that touches constrained
    /// geometry. Acceptable for the sketch scales this system has been
    /// exercised at so far; §7 open question 9 ("performance at scale
    /// untested") already flags this as an accepted, unprofiled risk this
    /// doesn't newly introduce.
    pub(crate) fn solve_sketch_constraints_preview(&self, touched: &[Handle]) -> Vec<(Handle, EntityType)> {
        if self.sketch_constraints.is_empty() || touched.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        for set in &self.sketch_constraints {
            let is_touched = touched.iter().any(|handle| set.constraints_touching(*handle).next().is_some());
            if !is_touched {
                continue;
            }
            let Some((solved, _dof, _conflicts)) = solve_scope(&self.document, set) else { continue };
            result.extend(solved);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::sketch_constraints::{ConstraintKind, SketchConstraintSet, SketchRef, SketchScope};
    use super::Scene;
    use acadrust::entities::EntityType;
    use acadrust::types::Vector3;

    // The bulk of this stage's coverage (Horizontal/Parallel/Distance/
    // Coincident solving, unrelated edits not triggering a resolve) lives
    // in `tests/sketch_constraints_solve.rs`, which only needs `pub` API.
    // This one test needs `record_undo_before`/`take_undo_recording`
    // (`pub(crate)`), so it stays internal.
    #[test]
    fn one_edit_that_ripples_through_a_constraint_still_records_as_one_undo_step() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        let b = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
        )));

        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        set.add(ConstraintKind::Parallel, vec![SketchRef::whole(a), SketchRef::whole(b)], None);
        scene.sketch_constraints.push(set);

        scene.begin_undo_recording();
        // A real command (Move, grip-drag commit, ...) records its own
        // before-image before mutating.
        let before_a = scene.document.get_entity_arc(a);
        scene.record_undo_before(a, before_a);
        if let Some(EntityType::Line(l)) = scene.document.get_entity_mut(a) {
            l.end = Vector3::new(10.0, 6.0, 0.0);
        }
        scene.bump_entities(&[(a, super::ChangeKind::Modified)]);
        let recording = scene.take_undo_recording().expect("an undo recording should still be open");

        let (entities, _objects, _sketch_constraints) = recording.into_recorded_images();
        let touched: std::collections::HashSet<_> = entities.iter().map(|(h, _)| *h).collect();
        assert!(touched.contains(&a), "the directly-edited line must be in the undo delta");
        assert!(touched.contains(&b), "the constraint-solved neighbor must ride the same undo delta");
    }

    /// Audit-flagged gap: ERASE removing an entity takes its constraints with
    /// it (design doc's deletion policy, above), but `sketch_constraints`
    /// lives on `Scene`, outside `document`/`document.objects`, so nothing
    /// used to capture that mutation for undo at all — the erased geometry
    /// came back on undo, but its constraints didn't. This proves the fix at
    /// the level it actually lives: `refresh_sketch_constraints`'s deletion
    /// pass must record the scope's whole before-image into the same
    /// `UndoRecording` the entity removal itself rides in, so one recovered
    /// image is enough to restore both together (the app-level wiring that
    /// turns this into one committed, one-undo-press `DeltaSnapshot` is
    /// `OpenCADStudio::commit_undo_delta`/`apply_delta_state`, exercised by
    /// the app, not `Scene`, so it's out of this test's reach — this covers
    /// the root cause, not that outer plumbing).
    #[test]
    fn erasing_a_constrained_entity_records_its_scope_for_undo() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        let b = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(20.0, 5.0, 0.0),
        )));
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
            ConstraintKind::Coincident,
            vec![SketchRef::point(a, 1), SketchRef::point(b, 0)],
            None,
        );
        assert_eq!(scene.sketch_constraint_set(SketchScope::ModelSpace).unwrap().constraints.len(), 1);

        scene.begin_undo_recording();
        scene.erase_entities(&[a]);

        // The live scope must already have lost the constraint (design doc's
        // deletion policy) — this ensures the test actually exercises restore,
        // not a no-op.
        assert_eq!(
            scene.sketch_constraint_set(SketchScope::ModelSpace).map_or(0, |s| s.constraints.len()),
            0,
            "the constraint touching the erased line should be gone from the live set"
        );

        let recording = scene.take_undo_recording().expect("an undo recording should still be open");
        let (_entities, _objects, sketch_constraints) = recording.into_recorded_images();
        assert_eq!(sketch_constraints.len(), 1, "the touched scope's before-image must be captured");
        let (scope, before) = &sketch_constraints[0];
        assert_eq!(*scope, SketchScope::ModelSpace);
        assert_eq!(before.constraints.len(), 1, "the before-image must still hold the constraint as it was before the erase");
        assert_eq!(before.constraints[0].kind, ConstraintKind::Coincident);
        assert_eq!(before.constraints[0].refs, vec![SketchRef::point(a, 1), SketchRef::point(b, 0)]);

        // What `apply_delta_state` does with this on undo: install the
        // before-image back as the live scope state.
        *scene.sketch_constraint_set_mut(*scope) = before.clone();
        assert_eq!(scene.sketch_constraint_set(SketchScope::ModelSpace).unwrap().constraints.len(), 1, "restoring the before-image must bring the constraint back");
    }

    /// Design doc §7 open question 6 (live-drag re-solve):
    /// `solve_sketch_constraints_preview` must report the constrained
    /// neighbor's solved position without writing anything into the
    /// document itself — the grip-drag caller owns applying it.
    #[test]
    fn preview_solve_reports_the_neighbors_new_state_without_mutating_the_document() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        let b = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
        )));
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        set.add(ConstraintKind::Parallel, vec![SketchRef::whole(a), SketchRef::whole(b)], None);
        scene.sketch_constraints.push(set);

        // Simulate a live grip drag: mutate `a` directly (as `apply_grip`
        // would each frame), without going through `bump_entities` at all.
        if let Some(EntityType::Line(l)) = scene.document.get_entity_mut(a) {
            l.end = Vector3::new(10.0, 6.0, 0.0);
        }
        let b_before = scene.document.get_entity(b).cloned();

        let solved = scene.solve_sketch_constraints_preview(&[a]);

        assert_eq!(scene.document.get_entity(b), b_before.as_ref(), "preview must not mutate the document");
        // The whole scope solves together (every registered entity's params
        // are free, not just `b`'s — see `register_entity`'s doc comment),
        // so `a` itself may also have shifted slightly to reach the nearest
        // mutually-parallel configuration; read whichever position `a` ends
        // up at from `solved` too, rather than assuming it stayed exactly
        // where the simulated drag put it.
        let line_dir = |entity: &EntityType| {
            let EntityType::Line(l) = entity else { panic!("expected a Line") };
            (l.end.x - l.start.x, l.end.y - l.start.y)
        };
        let dir_a = solved
            .iter()
            .find(|(h, _)| *h == a)
            .map(|(_, e)| line_dir(e))
            .unwrap_or((10.0, 6.0));
        let (_, moved_entity) = solved.iter().find(|(h, _)| *h == b).expect("b should be reported as moved");
        let dir_b = line_dir(moved_entity);
        let cross = dir_a.0 * dir_b.1 - dir_a.1 * dir_b.0;
        assert!(cross.abs() < 1e-6, "the previewed positions should be mutually parallel: dir_a={dir_a:?} dir_b={dir_b:?}");
    }
}
