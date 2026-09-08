//! Persistent, parametric geometric constraints — data model.
//!
//! Design: `docs/parametric_system_design.md`. This is Stage 1 of that
//! doc's §8 staged plan: pure data types plus unit tests, no document/scene
//! integration yet (that's `refresh_sketch_constraints`, wired into
//! `Scene::bump_entities` in a later stage).
//!
//! Distinct from the one-shot "Constraints" ribbon commands
//! (`crate::modules::draw::constrain`), which solve once and forget. A
//! [`SketchConstraint`] is a persisted record: it stays attached to its
//! entities and is meant to be re-solved every time referenced geometry
//! changes, not applied once.

use acadrust::types::{Handle, Vector3};
use serde::{Deserialize, Serialize};

/// One endpoint a constraint attaches to: an entity plus which sub-element
/// of it.
///
/// Reuses the GsMarker convention `AssocDimensionReference::main_gs_marker`
/// already carries for associative-dimension endpoints
/// (`src/scene/dimension_assoc.rs`), rather than inventing a second
/// sub-element addressing scheme: `marker` indexes into
/// [`dimension_assoc::source_points`](super::dimension_assoc::source_points)'s
/// ordered per-entity-type point list when non-negative (0/1 = a line's
/// start/end, ...), or names a special case when negative (-3 = a
/// circle/arc's center; -2 = a point at a stored parameter, not currently
/// used by constraint endpoints but reserved for consistency with the
/// dimension scheme).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SketchRef {
    pub entity: Handle,
    /// `None` addresses the entity as a whole — what a Radius, Length, or
    /// whole-curve constraint (Parallel, Perpendicular, Equal, Horizontal,
    /// Vertical) needs; a point-level constraint (Coincident, Distance
    /// between two points, Angle at a shared vertex) sets `Some(marker)`.
    pub marker: Option<i32>,
}

impl SketchRef {
    pub fn whole(entity: Handle) -> Self {
        Self { entity, marker: None }
    }

    pub fn point(entity: Handle, marker: i32) -> Self {
        Self { entity, marker: Some(marker) }
    }

    /// The circle/arc-center special case (`marker == -3`), broken out as
    /// its own constructor since `-3` alone reads as a magic number
    /// everywhere it would otherwise appear.
    pub fn center(entity: Handle) -> Self {
        Self { entity, marker: Some(-3) }
    }
}

/// The friendly, user-facing constraint types — the "what button did they
/// click" vocabulary, one layer above the `ocs_gcs` primitives each maps
/// onto (that mapping is `constraint_map`, a later stage; see the design
/// doc §2). Named and grouped the same way the existing one-shot ribbon
/// tools are (`crate::modules::draw::constrain::tools`), plus the
/// endpoint-picking kinds (`Coincident`, `Radius`, `Tangent`) that one-shot
/// group never needed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConstraintKind {
    Coincident,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    /// Distance between two points, or a single line's length, or a
    /// circle/arc's diameter — which reading applies depends on `refs`'
    /// shape, mirroring how `DistanceConstraintCommand`
    /// (`src/modules/draw/constrain/value.rs`) already infers it from the
    /// selected entity.
    Distance,
    Angle,
    Radius,
    Tangent,
}

pub type ConstraintId = u32;

/// One persisted constraint record: a friendly [`ConstraintKind`], the
/// entities/points it relates, and — for a dimensional kind — the value
/// driving it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SketchConstraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
    pub refs: Vec<SketchRef>,
    /// The typed target for a dimensional constraint (a `Distance`'s
    /// length, an `Angle`'s degrees, a `Radius`'s radius). `None` for every
    /// purely-geometric kind (Coincident, Horizontal, Vertical, Parallel,
    /// Perpendicular, Equal, Tangent).
    pub driving_param: Option<f64>,
    /// Lets a user suppress a constraint without losing it — a re-solve
    /// skips a disabled constraint entirely.
    pub enabled: bool,
}

/// What "one sketch" scopes to, given the app has no dedicated Sketch
/// grouping (design doc §3.1): the only existing sub-document boundary is a
/// block record, so model space and each block definition get their own
/// independent constraint set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SketchScope {
    ModelSpace,
    /// A block definition's `BlockRecord` handle — matches
    /// `BlockEditSession::br_handle`
    /// (`src/modules/draw/modify/block_edit.rs`).
    Block(Handle),
}

impl SketchScope {
    /// The handle a [`SketchConstraintSet`] for this scope is persisted
    /// under — `CadDocument::ensure_xrecord`/`xrecord`/`xrecord_mut`'s
    /// `owner` argument (design doc §1.4/§5.2). Model space resolves
    /// through the document rather than being a fixed handle because a
    /// document's model-space block record handle is assigned when the
    /// document is built, not a constant.
    pub fn owner_handle(&self, document: &acadrust::CadDocument) -> Handle {
        match self {
            SketchScope::ModelSpace => document.header.model_space_block_handle,
            SketchScope::Block(handle) => *handle,
        }
    }
}

/// Every persisted constraint for one [`SketchScope`]. Rebuilt-and-solved
/// wholesale on every trigger (design doc §4.2) — this struct holds only
/// the constraint records themselves, no solver state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SketchConstraintSet {
    pub scope: SketchScope,
    pub constraints: Vec<SketchConstraint>,
    next_id: ConstraintId,
    /// Cached total remaining degrees of freedom, summed across every
    /// independent solve partition in this scope — updated by
    /// `sketch_solve::solve_scope` each time this set is resolved. `None`
    /// until the first resolve (e.g. right after loading from disk, before
    /// any edit has touched this scope yet). Design doc §6.3's DOF badge
    /// reads this rather than recomputing it every frame. Not persisted:
    /// it's a derived cache, not real constraint state.
    #[serde(skip)]
    pub dof: Option<usize>,
    /// Cached redundant/conflicting constraints found by the last resolve
    /// (design doc §6.4, stage 11) — empty whenever the scope isn't
    /// over-constrained, which is the common case. Also a derived cache, not
    /// persisted; a `ConflictResolverPanel` reads this rather than calling
    /// `ocs_gcs::diagnosis::classify_redundant` itself.
    #[serde(skip)]
    pub conflicts: Vec<(ConstraintId, ocs_gcs::diagnosis::RedundancyKind)>,
}

impl SketchConstraintSet {
    pub fn new(scope: SketchScope) -> Self {
        Self { scope, constraints: Vec::new(), next_id: 0, dof: None, conflicts: Vec::new() }
    }

    /// Appends a constraint, assigning it a fresh id unique within this set.
    pub fn add(&mut self, kind: ConstraintKind, refs: Vec<SketchRef>, driving_param: Option<f64>) -> ConstraintId {
        let id = self.next_id;
        self.next_id += 1;
        self.constraints.push(SketchConstraint { id, kind, refs, driving_param, enabled: true });
        id
    }

    /// Removes a constraint by id. Returns whether one was actually removed.
    pub fn remove(&mut self, id: ConstraintId) -> bool {
        let before = self.constraints.len();
        self.constraints.retain(|c| c.id != id);
        self.constraints.len() != before
    }

    pub fn get(&self, id: ConstraintId) -> Option<&SketchConstraint> {
        self.constraints.iter().find(|c| c.id == id)
    }

    /// Every enabled constraint referencing `entity`, regardless of which
    /// of its sub-elements — the query `refresh_sketch_constraints`'
    /// reverse index (design doc §4.1) is built from.
    pub fn constraints_touching(&self, entity: Handle) -> impl Iterator<Item = &SketchConstraint> {
        self.constraints.iter().filter(move |c| c.enabled && c.refs.iter().any(|r| r.entity == entity))
    }

    /// Drops every constraint that references `entity` at all — the
    /// dangling-constraint-on-delete policy (design doc §5.3, open question
    /// 4): entities silently take their constraints with them when erased,
    /// rather than leaving a dangling reference around. Returns the
    /// removed constraints' ids, so a caller can log/report what vanished.
    pub fn remove_all_touching(&mut self, entity: Handle) -> Vec<ConstraintId> {
        let (removed, kept): (Vec<_>, Vec<_>) =
            self.constraints.drain(..).partition(|c| c.refs.iter().any(|r| r.entity == entity));
        self.constraints = kept;
        removed.into_iter().map(|c| c.id).collect()
    }
}

/// Resolves a [`SketchRef`] to its current world-space point, for building
/// an `ocs_gcs` `ParamStore` from live document geometry — the constraint
/// endpoint's equivalent of `dimension_assoc::resolve_reference`, restricted
/// to the marker conventions constraint endpoints actually use (whole-entity
/// `None`, an ordinary `source_points()` index, or the `-3` center case).
/// Returns `None` for a dangling reference (handle doesn't resolve) or a
/// marker this scheme doesn't (yet) support (e.g. `-2`, or an out-of-range
/// index).
///
/// `sketch_solve`'s solver-side entity registration reads entity fields
/// directly rather than through this (it needs raw, not-yet-WCS-resolved
/// coordinates to seed `ParamId`s) — this is for UI-side consumers instead:
/// currently `glyph_anchor` (below), eventually Phase B live inference
/// (design doc §6.2) querying "what point is near the cursor".
pub(crate) fn resolve_point(entity: &acadrust::EntityType, marker: i32) -> Option<Vector3> {
    if marker == -3 {
        return match entity {
            acadrust::EntityType::Circle(circle) => Some(circle.center_wcs()),
            acadrust::EntityType::Arc(arc) => Some(arc.center_wcs()),
            _ => None,
        };
    }
    if marker < 0 {
        return None;
    }
    super::dimension_assoc::source_points(entity).get(marker as usize).copied()
}

/// Below this squared distance (1e-6 world units), two points count as
/// already coincident for [`infer_coincident_refs`]/[`nearest_sketch_point`]'s
/// purposes.
const COINCIDENT_EPSILON_SQ: f64 = 1.0e-12;

/// Design doc §6.1's manual Coincident UI: given a world point (from a
/// `CadCommand::on_point` pick — trusted to already be OSNAP-snapped onto a
/// real feature, the same trust `infer_coincident_refs` places in a
/// just-drawn entity's own coordinates, for the same reason: no camera or
/// pixel radius is available once a point reaches this far from the click),
/// finds the closest addressable point (an ordinary `source_points()` index,
/// or a circle/arc's center) on any entity in `scope` other than `exclude`,
/// within [`COINCIDENT_EPSILON_SQ`]. `None` means the pick didn't land on a
/// real point — the caller should ask the user to enable an Endpoint/Center
/// object snap and try again, not silently constrain nothing.
pub(crate) fn nearest_sketch_point(
    document: &acadrust::CadDocument,
    scope: SketchScope,
    world_point: Vector3,
    exclude: Option<Handle>,
) -> Option<SketchRef> {
    let owner = scope.owner_handle(document);
    let mut best: Option<(f64, SketchRef)> = None;
    let mut consider = |handle: Handle, marker: i32, point: Vector3| {
        let dx = point.x - world_point.x;
        let dy = point.y - world_point.y;
        let dz = point.z - world_point.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq <= COINCIDENT_EPSILON_SQ && best.as_ref().is_none_or(|(d, _)| dist_sq < *d) {
            best = Some((dist_sq, SketchRef::point(handle, marker)));
        }
    };
    for candidate in document.entities() {
        let common = candidate.common();
        if common.owner_handle != owner || Some(common.handle) == exclude {
            continue;
        }
        for (marker, point) in super::dimension_assoc::source_points(candidate).into_iter().enumerate() {
            consider(common.handle, marker as i32, point);
        }
        match candidate {
            acadrust::EntityType::Circle(c) => consider(common.handle, -3, c.center_wcs()),
            acadrust::EntityType::Arc(a) => consider(common.handle, -3, a.center_wcs()),
            _ => {}
        }
    }
    best.map(|(_, r)| r)
}

/// Design doc §6.2 (stage 8, Phase B live inference): for each of
/// `new_entity`'s own points, finds an existing entity in `scope` (other
/// than `new_handle` itself) whose corresponding point already coincides,
/// and returns a `(new, existing)` `SketchRef` pair per match — the caller
/// adds each as a `Coincident` constraint.
///
/// **Deviation from this doc**: the doc's own §6.2 suggests reusing OSNAP's
/// screen-space tolerance/query; this instead checks for near-exact
/// world-space coincidence. The natural hook point is where a committed
/// entity becomes a document write (`CmdResult::CommitEntity`'s dispatch in
/// `command_driver.rs`) — well after the cursor's pixel position and camera
/// are available, with only the entity's final world coordinates left. If
/// OSNAP's endpoint mode grabbed the point while the user was drawing, the
/// new entity's coordinate already matches the existing one to floating-point
/// precision, so a tight world-space epsilon reproduces the same result
/// without re-deriving OSNAP's screen-space math or threading camera state
/// into this dispatch site.
pub(crate) fn infer_coincident_refs(
    document: &acadrust::CadDocument,
    scope: SketchScope,
    new_handle: Handle,
    new_entity: &acadrust::EntityType,
) -> Vec<(SketchRef, SketchRef)> {
    let owner = scope.owner_handle(document);
    let new_points = super::dimension_assoc::source_points(new_entity);
    if new_points.is_empty() {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    for (new_marker, new_point) in new_points.iter().enumerate() {
        for candidate in document.entities() {
            let common = candidate.common();
            if common.handle == new_handle || common.owner_handle != owner {
                continue;
            }
            for (marker, point) in super::dimension_assoc::source_points(candidate).iter().enumerate() {
                let dx = point.x - new_point.x;
                let dy = point.y - new_point.y;
                let dz = point.z - new_point.z;
                if dx * dx + dy * dy + dz * dz <= COINCIDENT_EPSILON_SQ {
                    pairs.push((SketchRef::point(new_handle, new_marker as i32), SketchRef::point(common.handle, marker as i32)));
                    break;
                }
            }
        }
    }
    pairs
}

impl ConstraintKind {
    /// The short symbol a constraint glyph shows — matches the existing
    /// ribbon icons (`crate::modules::draw::constrain::{tools,value}`) for
    /// the kinds that have a one-click button, so the same glyph means the
    /// same thing in both places.
    pub fn glyph_symbol(&self) -> &'static str {
        match self {
            ConstraintKind::Coincident => "≡",
            ConstraintKind::Horizontal => "—",
            ConstraintKind::Vertical => "│",
            ConstraintKind::Parallel => "∥",
            ConstraintKind::Perpendicular => "⊥",
            ConstraintKind::Equal => "=",
            ConstraintKind::Distance => "↔",
            ConstraintKind::Angle => "∠",
            ConstraintKind::Radius => "R",
            ConstraintKind::Tangent => "T",
        }
    }
}

/// The full glyph text for one constraint: its symbol, plus the driving
/// value for a dimensional kind (Distance/Angle/Radius).
pub(crate) fn glyph_label(constraint: &SketchConstraint) -> String {
    match (constraint.kind, constraint.driving_param) {
        (ConstraintKind::Angle, Some(value)) => format!("{} {value:.1}°", constraint.kind.glyph_symbol()),
        (_, Some(value)) => format!("{} {value:.2}", constraint.kind.glyph_symbol()),
        (_, None) => constraint.kind.glyph_symbol().to_string(),
    }
}

/// World-space point to anchor `constraint`'s glyph at, or `None` if it
/// can't be resolved (dangling ref, unsupported entity/marker). A point ref
/// resolves straight through [`resolve_point`]; a whole-entity ref falls
/// back to a representative point on the entity (a line's midpoint, a
/// circle's rightmost point) since there's no single named point to anchor
/// on otherwise.
pub(crate) fn glyph_anchor(document: &acadrust::CadDocument, constraint: &SketchConstraint) -> Option<Vector3> {
    let r = constraint.refs.first()?;
    let entity = document.get_entity(r.entity)?;
    if let Some(marker) = r.marker {
        return resolve_point(entity, marker);
    }
    match entity {
        acadrust::EntityType::Line(l) => {
            Some(Vector3::new((l.start.x + l.end.x) * 0.5, (l.start.y + l.end.y) * 0.5, (l.start.z + l.end.z) * 0.5))
        }
        acadrust::EntityType::Circle(c) => Some(c.point_at_angle_wcs(0.0)),
        _ => None,
    }
}

impl super::Scene {
    /// The constraint set for `scope`, if one has been created.
    pub fn sketch_constraint_set(&self, scope: SketchScope) -> Option<&SketchConstraintSet> {
        self.sketch_constraints.iter().find(|s| s.scope == scope)
    }

    /// The constraint set for `scope`, creating an empty one on first use.
    /// The `pub(crate)` `sketch_constraints` field itself stays private so
    /// nothing outside this module can end up with two sets for the same
    /// scope — this is the one way to reach a scope's set for both reading
    /// and mutating.
    pub fn sketch_constraint_set_mut(&mut self, scope: SketchScope) -> &mut SketchConstraintSet {
        if let Some(index) = self.sketch_constraints.iter().position(|s| s.scope == scope) {
            &mut self.sketch_constraints[index]
        } else {
            self.sketch_constraints.push(SketchConstraintSet::new(scope));
            self.sketch_constraints.last_mut().expect("just pushed")
        }
    }

    /// Design doc §5.3/§12 (open question 3, now resolved): copy/paste's
    /// handle remapping lives locally in each command that duplicates
    /// entities — `Scene::copy_entities`' `handle_map` (COPY/ARRAY/MIRROR,
    /// `src/scene/modify.rs`) and `OpenCADStudio::finalize_paste`'s own
    /// (clipboard paste, `src/app/command_driver.rs`) — rather than in one
    /// shared table, so each call site passes its own `handle_map` here
    /// after adding the duplicated entities.
    ///
    /// For every enabled constraint whose *every* referenced entity was
    /// duplicated (a constraint straddling a duplicated and a
    /// non-duplicated entity can't sensibly follow — only one side moved),
    /// adds an equivalent constraint over the new handles to the same
    /// scope, then triggers a solve for the newly duplicated geometry the
    /// same way any other edit would. A no-op when `handle_map` is empty or
    /// nothing constrained was duplicated.
    pub fn duplicate_sketch_constraints_for(&mut self, handle_map: &rustc_hash::FxHashMap<Handle, Handle>) {
        if handle_map.is_empty() {
            return;
        }
        let mut to_add: Vec<(usize, ConstraintKind, Vec<SketchRef>, Option<f64>)> = Vec::new();
        for (scope_index, set) in self.sketch_constraints.iter().enumerate() {
            for c in &set.constraints {
                if !c.enabled || !c.refs.iter().all(|r| handle_map.contains_key(&r.entity)) {
                    continue;
                }
                let new_refs: Vec<SketchRef> =
                    c.refs.iter().map(|r| SketchRef { entity: handle_map[&r.entity], marker: r.marker }).collect();
                to_add.push((scope_index, c.kind, new_refs, c.driving_param));
            }
        }
        if to_add.is_empty() {
            return;
        }
        let mut touched: Vec<Handle> = Vec::new();
        for (scope_index, kind, refs, driving_param) in to_add {
            touched.extend(refs.iter().map(|r| r.entity));
            self.sketch_constraints[scope_index].add(kind, refs, driving_param);
        }
        touched.sort();
        touched.dedup();
        let changes: Vec<(Handle, super::ChangeKind)> = touched.into_iter().map(|h| (h, super::ChangeKind::Modified)).collect();
        self.bump_entities(&changes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: u64) -> Handle {
        Handle::new(v)
    }

    #[test]
    fn add_assigns_increasing_ids_and_get_finds_them() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let a = set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(h(1))], None);
        let b = set.add(ConstraintKind::Distance, vec![SketchRef::whole(h(1))], Some(25.0));
        assert_ne!(a, b);
        assert_eq!(set.get(a).unwrap().kind, ConstraintKind::Horizontal);
        assert_eq!(set.get(b).unwrap().driving_param, Some(25.0));
    }

    #[test]
    fn remove_drops_only_the_matching_id() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let a = set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(h(1))], None);
        let b = set.add(ConstraintKind::Vertical, vec![SketchRef::whole(h(2))], None);
        assert!(set.remove(a));
        assert!(!set.remove(a), "removing twice should report nothing removed the second time");
        assert!(set.get(a).is_none());
        assert!(set.get(b).is_some());
    }

    #[test]
    fn constraints_touching_finds_entity_regardless_of_marker() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        set.add(ConstraintKind::Coincident, vec![SketchRef::point(h(1), 0), SketchRef::point(h(2), 1)], None);
        set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(h(3))], None);

        let touching_1: Vec<_> = set.constraints_touching(h(1)).collect();
        assert_eq!(touching_1.len(), 1);
        let touching_3: Vec<_> = set.constraints_touching(h(3)).collect();
        assert_eq!(touching_3.len(), 1);
        assert_eq!(set.constraints_touching(h(99)).count(), 0);
    }

    #[test]
    fn constraints_touching_skips_disabled() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let id = set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(h(1))], None);
        set.constraints.iter_mut().find(|c| c.id == id).unwrap().enabled = false;
        assert_eq!(set.constraints_touching(h(1)).count(), 0);
    }

    #[test]
    fn remove_all_touching_drops_every_constraint_referencing_the_entity() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let coincident = set.add(ConstraintKind::Coincident, vec![SketchRef::point(h(1), 0), SketchRef::point(h(2), 1)], None);
        let horizontal_other = set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(h(3))], None);

        let removed = set.remove_all_touching(h(1));
        assert_eq!(removed, vec![coincident]);
        assert!(set.get(coincident).is_none());
        assert!(set.get(horizontal_other).is_some(), "unrelated entity's constraint must survive");
    }

    #[test]
    fn scope_owner_handle_resolves_block_directly() {
        let block_handle = h(42);
        let scope = SketchScope::Block(block_handle);
        let doc = acadrust::CadDocument::new();
        assert_eq!(scope.owner_handle(&doc), block_handle);
    }

    #[test]
    fn ref_center_constructor_matches_the_dash_three_convention() {
        let r = SketchRef::center(h(7));
        assert_eq!(r, SketchRef { entity: h(7), marker: Some(-3) });
    }

    #[test]
    fn sketch_constraint_set_round_trips_through_bincode() {
        let mut set = SketchConstraintSet::new(SketchScope::Block(h(5)));
        set.add(ConstraintKind::Coincident, vec![SketchRef::point(h(1), 0), SketchRef::point(h(2), 1)], None);
        set.add(ConstraintKind::Distance, vec![SketchRef::whole(h(3))], Some(12.5));

        let bytes = bincode::serialize(&set).expect("serialize");
        let restored: SketchConstraintSet = bincode::deserialize(&bytes).expect("deserialize");

        assert_eq!(restored.scope, set.scope);
        assert_eq!(restored.constraints.len(), set.constraints.len());
        assert_eq!(restored.constraints[1].driving_param, Some(12.5));
        assert_eq!(restored.constraints[0].refs, set.constraints[0].refs);
    }
}
