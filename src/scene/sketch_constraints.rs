//! Persistent sketch-constraint data types and scope management.

use super::named_parameters::DrivingValue;
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
        Self {
            entity,
            marker: None,
        }
    }

    pub fn point(entity: Handle, marker: i32) -> Self {
        Self {
            entity,
            marker: Some(marker),
        }
    }

    /// The circle/arc-center special case (`marker == -3`), broken out as
    /// its own constructor since `-3` alone reads as a magic number
    /// everywhere it would otherwise appear.
    pub fn center(entity: Handle) -> Self {
        Self {
            entity,
            marker: Some(-3),
        }
    }
}

/// The friendly, user-facing constraint types — the "what button did they
/// click" vocabulary, one layer above the `cadkernel_constraints` primitives each maps
/// onto (that mapping is `constraint_map`, a later stage; see the design
/// doc §2). Named and grouped the same way the existing one-shot ribbon
/// tools are (`crate::modules::parametric::tools`), plus the
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
    /// Two circles/arcs share a center — `refs`: `[center(a), center(b)]`.
    /// Solves identically to `Coincident` (`sketch_solve.rs` broadens that
    /// match arm rather than duplicating it) — only the DWG-native class
    /// name (`ACCONCENTRICCONSTRAINT` vs `ACPOINTCOINCIDENCECONSTRAINT`)
    /// and the UI entry point differ.
    Concentric,
    /// A point sits at a circle/arc's center — `refs`: `[point, center(circle)]`.
    /// Same solver math as `Coincident`/`Concentric`, different DWG class
    /// name (`ACCENTERPOINTCONSTRAINT`).
    CenterPoint,
    /// Two lines share the same infinite line — `refs`: `[whole(a), whole(b)]`.
    Colinear,
    /// A point sits at another line's midpoint — `refs`: `[point, whole(line)]`.
    Midpoint,
    /// Locks a whole entity at its current position — `refs`: `[whole(entity)]`.
    /// No `driving_param`: the target is the entity's own live geometry at
    /// solve time, not a typed value (see `sketch_solve.rs`'s `Fixed` arm).
    Fixed,
    /// A point lies anywhere along a line's or circle's curve (not
    /// restricted to an endpoint/center) — `refs`: `[point, whole(entity)]`.
    PointOnCurve,
    /// The distance between one point pair equals the distance between
    /// another — `refs`: `[p1, p2, p3, p4]` (`dist(p1,p2) == dist(p3,p4)`).
    /// For the supported entity types, equal curvature is already covered
    /// by the circle/circle radius branch of `Equal`.
    EqualDistance,
    /// Two circles/arcs are mirror images of each other across a line —
    /// `refs`: `[center(a), center(b), whole(mirror_line)]`. Scoped to the
    /// circle-center-pair case (unambiguous with whole-entity selection);
    /// point-symmetry about a point, and symmetry between two lines, aren't
    /// modeled.
    Symmetric,
    /// A circle/arc's diameter (twice `Radius`'s target) — `refs`:
    /// `[whole(circle_or_arc)]`. Same DWG class as `Radius`
    /// (`ACRADIUSDIAMETERCONSTRAINT`), distinguished only by the
    /// `RadiusDiameterConstrType` mode byte
    /// (`dwg_native_constraints.rs`), rather than a distinct object type.
    Diameter,
    /// The X-only (resp. Y-only) component of the distance between two
    /// points — `refs`: `[p1, p2]`, same shape as `Distance`. Same DWG
    /// class as `Distance` (`ACDISTANCECONSTRAINT`) with its
    /// `DirectionType` set to a fixed `(1,0,0)`/`(0,1,0)` direction.
    DistanceX,
    DistanceY,
    /// A line perpendicular to a circle/arc's tangent at their point of
    /// contact — `refs`: `[whole(a), whole(b)]`, either order. For the
    /// Line/Circle-only entity model this is equivalent to "the line
    /// passes through the circle's center" (a circle's radius is always
    /// normal to its own tangent), so it solves via the same `PointOnLine`
    /// primitive `PointOnCurve` already uses. Distinct from
    /// `Perpendicular` (line-to-line only). Line-Line has no meaning here
    /// (that's plain `Perpendicular`) and isn't buildable.
    Normal,
    /// An arc's arc length (`radius * sweep angle`) — `refs`:
    /// `[whole(arc)]`. Solves against the arc's own `start_angle`/
    /// `end_angle`, registered separately from its center/radius
    /// (`sketch_solve.rs`'s `arc_angles` cache) — no native DWG
    /// representation exists for this (neither `AcExplicitConstr.h` nor
    /// `acadrust`'s `AssocConstraintNodeData` has an ArcLength-shaped
    /// class/variant), so it persists in this app's own XRecord format
    /// only, the same "DWG can't carry everything" gap the dependency-chain
    /// DXF-omission precedent already documents.
    ArcLength,
}

pub type ConstraintId = u32;

/// One persisted constraint record: a friendly [`ConstraintKind`], the
/// entities/points it relates, and — for a dimensional kind — the value
/// driving it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchConstraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
    pub refs: Vec<SketchRef>,
    /// The target for a dimensional constraint (a `Distance`'s length, an
    /// `Angle`'s degrees, a `Radius`'s radius) — a literal number or a
    /// named-parameter reference resolved through `Scene::named_parameters` at solve time by
    /// `sketch_solve::build_constraint`). `None` for every purely-geometric
    /// kind (Coincident, Horizontal, Vertical, Parallel, Perpendicular,
    /// Equal, Tangent).
    pub driving_param: Option<DrivingValue>,
    /// Lets a user suppress a constraint without losing it — a re-solve
    /// skips a disabled constraint entirely.
    pub enabled: bool,
}

/// A constraint scope: model space or one block definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SketchScope {
    ModelSpace,
    /// A block definition's `BlockRecord` handle — matches
    /// `BlockEditSession::br_handle`
    /// (`src/modules/draw/modify/block_edit.rs`).
    Block(Handle),
}

impl SketchScope {
    /// The owner handle under which this scope is persisted.
    pub fn owner_handle(&self, document: &acadrust::CadDocument) -> Handle {
        match self {
            SketchScope::ModelSpace => document.header.model_space_block_handle,
            SketchScope::Block(handle) => *handle,
        }
    }
}

/// Every persisted constraint for one [`SketchScope`]. Solver state is rebuilt
/// on demand and is not stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SketchConstraintSet {
    pub scope: SketchScope,
    pub constraints: Vec<SketchConstraint>,
    next_id: ConstraintId,
    /// Cached total remaining degrees of freedom, summed across every
    /// independent solve partition in this scope — updated by
    /// `sketch_solve::solve_scope` each time this set is resolved. `None`
    /// until the first resolve (e.g. right after loading from disk, before
    /// any edit has touched this scope yet). The status badge reads this
    /// rather than recomputing it every frame. Not persisted:
    /// it's a derived cache, not real constraint state.
    #[serde(skip)]
    pub dof: Option<usize>,
    /// Cached redundant/conflicting constraints found by the last resolve.
    /// Also a derived cache, not
    /// persisted; a `ConflictResolverPanel` reads this rather than calling
    /// `cadkernel_constraints::diagnosis::classify_redundant` itself.
    #[serde(skip)]
    pub conflicts: Vec<(
        ConstraintId,
        cadkernel_constraints::diagnosis::RedundancyKind,
    )>,
}

impl SketchConstraintSet {
    pub fn new(scope: SketchScope) -> Self {
        Self {
            scope,
            constraints: Vec::new(),
            next_id: 0,
            dof: None,
            conflicts: Vec::new(),
        }
    }

    /// Appends a constraint, assigning it a fresh id unique within this set.
    pub fn add(
        &mut self,
        kind: ConstraintKind,
        refs: Vec<SketchRef>,
        driving_param: Option<DrivingValue>,
    ) -> ConstraintId {
        let id = self.next_id;
        self.next_id += 1;
        self.constraints.push(SketchConstraint {
            id,
            kind,
            refs,
            driving_param,
            enabled: true,
        });
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

    /// Every enabled constraint referencing `entity`.
    pub fn constraints_touching(&self, entity: Handle) -> impl Iterator<Item = &SketchConstraint> {
        self.constraints
            .iter()
            .filter(move |c| c.enabled && c.refs.iter().any(|r| r.entity == entity))
    }

    /// Drops every constraint that references `entity` and returns the
    /// removed constraint ids.
    pub fn remove_all_touching(&mut self, entity: Handle) -> Vec<ConstraintId> {
        let (removed, kept): (Vec<_>, Vec<_>) = self
            .constraints
            .drain(..)
            .partition(|c| c.refs.iter().any(|r| r.entity == entity));
        self.constraints = kept;
        removed.into_iter().map(|c| c.id).collect()
    }
}

/// Resolves a [`SketchRef`] to its current world-space point, for building
/// an `cadkernel_constraints` `ParamStore` from live document geometry — the constraint
/// endpoint's equivalent of `dimension_assoc::resolve_reference`, restricted
/// to the marker conventions constraint endpoints actually use (whole-entity
/// `None`, an ordinary `source_points()` index, or the `-3` center case).
/// Returns `None` for a dangling reference (handle doesn't resolve) or a
/// marker this scheme doesn't (yet) support (e.g. `-2`, or an out-of-range
/// index).
///
/// Solver-side registration reads raw entity fields directly. This helper is
/// for UI-side consumers that need the current world-space position.
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
    super::dimension_assoc::source_points(entity)
        .get(marker as usize)
        .copied()
}

/// Below this squared distance (1e-6 world units), two points count as
/// already coincident for [`infer_coincident_refs`]/[`nearest_sketch_point`]'s
/// purposes.
const COINCIDENT_EPSILON_SQ: f64 = 1.0e-12;

/// Finds the addressable entity point nearest a snapped world position.
/// Returns `None` when no point in the scope is within the coincidence
/// tolerance.
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
        for (marker, point) in super::dimension_assoc::source_points(candidate)
            .into_iter()
            .enumerate()
        {
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

/// For each point on `new_entity`, finds an existing entity in `scope` (other
/// than `new_handle` itself) whose corresponding point already coincides,
/// and returns a `(new, existing)` `SketchRef` pair per match — the caller
/// adds each as a `Coincident` constraint.
///
/// Matching uses a tight world-space tolerance because this runs after the
/// snapped entity has been committed.
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
            for (marker, point) in super::dimension_assoc::source_points(candidate)
                .iter()
                .enumerate()
            {
                let dx = point.x - new_point.x;
                let dy = point.y - new_point.y;
                let dz = point.z - new_point.z;
                if dx * dx + dy * dy + dz * dz <= COINCIDENT_EPSILON_SQ {
                    pairs.push((
                        SketchRef::point(new_handle, new_marker as i32),
                        SketchRef::point(common.handle, marker as i32),
                    ));
                    break;
                }
            }
        }
    }
    pairs
}

impl ConstraintKind {
    /// The short symbol a constraint glyph shows — matches the existing
    /// ribbon icons (`crate::modules::parametric::{tools,value}`) for
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
            ConstraintKind::Concentric => "◎",
            ConstraintKind::CenterPoint => "⊕",
            ConstraintKind::Colinear => "L",
            ConstraintKind::Midpoint => "M",
            ConstraintKind::Fixed => "F",
            ConstraintKind::PointOnCurve => "∈",
            ConstraintKind::EqualDistance => "≐",
            ConstraintKind::Symmetric => "S",
            ConstraintKind::Diameter => "⌀",
            ConstraintKind::DistanceX => "↔ₓ",
            ConstraintKind::DistanceY => "↔ᵧ",
            ConstraintKind::Normal => "⊾",
            ConstraintKind::ArcLength => "⌢",
        }
    }
}

/// The full glyph text for one constraint: its symbol, plus the driving
/// value for a dimensional kind (Distance/Angle/Radius).
pub(crate) fn glyph_label(constraint: &SketchConstraint) -> String {
    match (constraint.kind, &constraint.driving_param) {
        (ConstraintKind::Angle, Some(DrivingValue::Literal(value))) => {
            format!("{} {value:.1}°", constraint.kind.glyph_symbol())
        }
        (_, Some(DrivingValue::Literal(value))) => {
            format!("{} {value:.2}", constraint.kind.glyph_symbol())
        }
        // A named reference has no single resolved number to show without
        // threading `ParameterTable` into every glyph-render call site
        // (`src/ui/overlay.rs`) — showing the name itself is enough for now;
        // stage 4's parameters panel is the natural place to reconsider this
        // once a named `driving_param` can actually be authored through the
        // UI (nothing can yet — this arm exists so the match is exhaustive
        // and correct ahead of that UI, not because it's reachable today).
        (_, Some(DrivingValue::Named(name))) => {
            format!("{} {name}", constraint.kind.glyph_symbol())
        }
        (_, None) => constraint.kind.glyph_symbol().to_string(),
    }
}

/// World-space point to anchor `constraint`'s glyph at, or `None` if it
/// can't be resolved (dangling ref, unsupported entity/marker). A point ref
/// resolves straight through [`resolve_point`]; a whole-entity ref falls
/// back to a representative point on the entity (a line's midpoint, a
/// circle's rightmost point) since there's no single named point to anchor
/// on otherwise.
pub(crate) fn glyph_anchor(
    document: &acadrust::CadDocument,
    constraint: &SketchConstraint,
) -> Option<Vector3> {
    let r = constraint.refs.first()?;
    let entity = document.get_entity(r.entity)?;
    if let Some(marker) = r.marker {
        return resolve_point(entity, marker);
    }
    match entity {
        acadrust::EntityType::Line(l) => Some(Vector3::new(
            (l.start.x + l.end.x) * 0.5,
            (l.start.y + l.end.y) * 0.5,
            (l.start.z + l.end.z) * 0.5,
        )),
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
        if let Some(index) = self
            .sketch_constraints
            .iter()
            .position(|s| s.scope == scope)
        {
            &mut self.sketch_constraints[index]
        } else {
            self.sketch_constraints
                .push(SketchConstraintSet::new(scope));
            self.sketch_constraints.last_mut().expect("just pushed")
        }
    }

    /// Handle remapping lives in each command that duplicates
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
    pub fn duplicate_sketch_constraints_for(
        &mut self,
        handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    ) {
        if handle_map.is_empty() {
            return;
        }
        let mut to_add: Vec<(usize, ConstraintKind, Vec<SketchRef>, Option<DrivingValue>)> =
            Vec::new();
        for (scope_index, set) in self.sketch_constraints.iter().enumerate() {
            for c in &set.constraints {
                if !c.enabled || !c.refs.iter().all(|r| handle_map.contains_key(&r.entity)) {
                    continue;
                }
                let new_refs: Vec<SketchRef> = c
                    .refs
                    .iter()
                    .map(|r| SketchRef {
                        entity: handle_map[&r.entity],
                        marker: r.marker,
                    })
                    .collect();
                to_add.push((scope_index, c.kind, new_refs, c.driving_param.clone()));
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
        let changes: Vec<(Handle, super::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, super::ChangeKind::Modified))
            .collect();
        self.bump_entities(&changes);
    }

    /// Every persistent constraint, in any scope, currently driven by the
    /// named parameter `name` — what the Named Parameters panel's "used by"
    /// column shows. `entities` is the constraint's own referenced handles
    /// (deduplicated; a two-point constraint on the same entity's own two
    /// markers would otherwise list it twice), not resolved against the
    /// live document — a caller wanting an entity's current type/position
    /// still needs `Scene::document.get_entity`.
    pub fn parameter_usage(&self, name: &str) -> Vec<ParameterUsage> {
        let mut out = Vec::new();
        for set in &self.sketch_constraints {
            for c in &set.constraints {
                let Some(DrivingValue::Named(n)) = &c.driving_param else {
                    continue;
                };
                if n != name {
                    continue;
                }
                let mut entities: Vec<Handle> = c.refs.iter().map(|r| r.entity).collect();
                entities.sort();
                entities.dedup();
                out.push(ParameterUsage {
                    scope: set.scope,
                    constraint_id: c.id,
                    kind: c.kind,
                    entities,
                });
            }
        }
        out
    }
}

/// One persistent constraint driven by a named parameter — [`Scene::parameter_usage`]'s
/// result type.
#[derive(Debug, Clone)]
pub struct ParameterUsage {
    pub scope: SketchScope,
    pub constraint_id: ConstraintId,
    pub kind: ConstraintKind,
    pub entities: Vec<Handle>,
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
        let a = set.add(
            ConstraintKind::Horizontal,
            vec![SketchRef::whole(h(1))],
            None,
        );
        let b = set.add(
            ConstraintKind::Distance,
            vec![SketchRef::whole(h(1))],
            Some(DrivingValue::Literal(25.0)),
        );
        assert_ne!(a, b);
        assert_eq!(set.get(a).unwrap().kind, ConstraintKind::Horizontal);
        assert_eq!(
            set.get(b).unwrap().driving_param,
            Some(DrivingValue::Literal(25.0))
        );
    }

    #[test]
    fn remove_drops_only_the_matching_id() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let a = set.add(
            ConstraintKind::Horizontal,
            vec![SketchRef::whole(h(1))],
            None,
        );
        let b = set.add(ConstraintKind::Vertical, vec![SketchRef::whole(h(2))], None);
        assert!(set.remove(a));
        assert!(
            !set.remove(a),
            "removing twice should report nothing removed the second time"
        );
        assert!(set.get(a).is_none());
        assert!(set.get(b).is_some());
    }

    #[test]
    fn constraints_touching_finds_entity_regardless_of_marker() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        set.add(
            ConstraintKind::Coincident,
            vec![SketchRef::point(h(1), 0), SketchRef::point(h(2), 1)],
            None,
        );
        set.add(
            ConstraintKind::Horizontal,
            vec![SketchRef::whole(h(3))],
            None,
        );

        let touching_1: Vec<_> = set.constraints_touching(h(1)).collect();
        assert_eq!(touching_1.len(), 1);
        let touching_3: Vec<_> = set.constraints_touching(h(3)).collect();
        assert_eq!(touching_3.len(), 1);
        assert_eq!(set.constraints_touching(h(99)).count(), 0);
    }

    #[test]
    fn constraints_touching_skips_disabled() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let id = set.add(
            ConstraintKind::Horizontal,
            vec![SketchRef::whole(h(1))],
            None,
        );
        set.constraints
            .iter_mut()
            .find(|c| c.id == id)
            .unwrap()
            .enabled = false;
        assert_eq!(set.constraints_touching(h(1)).count(), 0);
    }

    #[test]
    fn remove_all_touching_drops_every_constraint_referencing_the_entity() {
        let mut set = SketchConstraintSet::new(SketchScope::ModelSpace);
        let coincident = set.add(
            ConstraintKind::Coincident,
            vec![SketchRef::point(h(1), 0), SketchRef::point(h(2), 1)],
            None,
        );
        let horizontal_other = set.add(
            ConstraintKind::Horizontal,
            vec![SketchRef::whole(h(3))],
            None,
        );

        let removed = set.remove_all_touching(h(1));
        assert_eq!(removed, vec![coincident]);
        assert!(set.get(coincident).is_none());
        assert!(
            set.get(horizontal_other).is_some(),
            "unrelated entity's constraint must survive"
        );
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
        assert_eq!(
            r,
            SketchRef {
                entity: h(7),
                marker: Some(-3)
            }
        );
    }

    #[test]
    fn sketch_constraint_set_round_trips_through_bincode() {
        let mut set = SketchConstraintSet::new(SketchScope::Block(h(5)));
        set.add(
            ConstraintKind::Coincident,
            vec![SketchRef::point(h(1), 0), SketchRef::point(h(2), 1)],
            None,
        );
        set.add(
            ConstraintKind::Distance,
            vec![SketchRef::whole(h(3))],
            Some(DrivingValue::Literal(12.5)),
        );

        let bytes = bincode::serialize(&set).expect("serialize");
        let restored: SketchConstraintSet = bincode::deserialize(&bytes).expect("deserialize");

        assert_eq!(restored.scope, set.scope);
        assert_eq!(restored.constraints.len(), set.constraints.len());
        assert_eq!(
            restored.constraints[1].driving_param,
            Some(DrivingValue::Literal(12.5))
        );
        assert_eq!(restored.constraints[0].refs, set.constraints[0].refs);
    }

    #[test]
    fn parameter_usage_finds_every_constraint_driven_by_the_named_parameter() {
        let mut scene = super::super::Scene::new();
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![SketchRef::point(h(1), 0), SketchRef::point(h(1), 1)],
                Some(DrivingValue::Named("gap".to_string())),
            );
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Radius,
                vec![SketchRef::whole(h(2))],
                Some(DrivingValue::Named("gap".to_string())),
            );
        // Unrelated: a literal-driven constraint and one driven by a
        // different name must not show up.
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Radius,
                vec![SketchRef::whole(h(3))],
                Some(DrivingValue::Literal(5.0)),
            );
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![SketchRef::point(h(4), 0), SketchRef::point(h(4), 1)],
                Some(DrivingValue::Named("other".to_string())),
            );

        let usage = scene.parameter_usage("gap");
        assert_eq!(
            usage.len(),
            2,
            "exactly the two constraints driven by 'gap', got {usage:?}"
        );
        assert!(usage
            .iter()
            .any(|u| u.kind == ConstraintKind::Distance && u.entities == vec![h(1)]));
        assert!(usage
            .iter()
            .any(|u| u.kind == ConstraintKind::Radius && u.entities == vec![h(2)]));

        assert_eq!(scene.parameter_usage("nonexistent").len(), 0);
    }

    #[test]
    fn parameter_usage_searches_every_scope_not_just_model_space() {
        let mut scene = super::super::Scene::new();
        let block = h(99);
        scene
            .sketch_constraint_set_mut(SketchScope::Block(block))
            .add(
                ConstraintKind::Radius,
                vec![SketchRef::whole(h(1))],
                Some(DrivingValue::Named("r".to_string())),
            );
        let usage = scene.parameter_usage("r");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].scope, SketchScope::Block(block));
    }

    #[test]
    fn parameter_usage_deduplicates_an_entity_referenced_by_two_markers() {
        let mut scene = super::super::Scene::new();
        // A Distance constraint whose two points are both on the same
        // entity (e.g. a line's own start and end) must list that entity
        // once, not twice.
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![SketchRef::point(h(1), 0), SketchRef::point(h(1), 1)],
                Some(DrivingValue::Named("len".to_string())),
            );
        let usage = scene.parameter_usage("len");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].entities, vec![h(1)]);
    }
}
