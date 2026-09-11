//! Builds and solves persistent two-dimensional sketch constraints through the geometry kernel.
//! Unsupported entity shapes, invalid target values, and incompatible planes are skipped.

use std::collections::HashMap;
use std::rc::Rc;

use acadrust::entities::EntityType;
use acadrust::types::Handle;

use cadkernel_constraints::constraints::angle_distance::L2LAngle;
use cadkernel_constraints::constraints::circle_arc::{
    ArcLength, C2LDistance, P2CDistance, TangentCircumf,
};
use cadkernel_constraints::constraints::curve_generic::CurveValue;
use cadkernel_constraints::constraints::point_line::{
    CenterOfGravity, Difference, Equal, EqualLineLength, MidpointOnLine, P2PDistance,
    Parallel as ParallelConstraint, Perpendicular as PerpendicularConstraint, PointOnLine,
};
use cadkernel_constraints::constraints::Constraint;
use cadkernel_constraints::geo::{
    Arc as GArc, Circle as GCircle, Ellipse as GEllipse, Line as GLine, Point as GPoint,
};
use cadkernel_constraints::solvers::dogleg::solve_dl;
use cadkernel_constraints::system::System;

use super::named_parameters::ParameterTable;
use super::sketch_constraints::{
    ConstraintId, ConstraintKind, SketchConstraint, SketchConstraintSet, SketchRef,
};
use super::{ChangeKind, Scene};

/// One referenced entity's geometry, registered into an `cadkernel_constraints::System`'s
/// parameter store.
#[derive(Clone, Copy)]
enum EntityGeom {
    Line(GLine),
    Circle(GCircle),
    /// Full `geo::Arc` — center/radius (via `.circle`), `start_angle`/
    /// `end_angle`, and real `start`/`end` points kept consistent with
    /// those by the arc-rules `CurveValue` constraints `solve_scope` adds
    /// for every registered arc. See this module's doc comment.
    Arc(GArc),
    /// Center-and-shape support only — see `register_entity`'s `Ellipse`
    /// arm for the acadrust-to-`cadkernel_constraints` parametrization conversion, the
    /// ellipse-rules `Difference` constraints `solve_scope` adds for every
    /// registered ellipse to keep `focus1` translating rigidly with
    /// `center` (this module's doc comment), and that same doc comment for
    /// what isn't wired up yet (major/minor axis dimensional constraints,
    /// `PointOnEllipse`-based Coincident/Tangent).
    Ellipse(GEllipse),
}

impl EntityGeom {
    /// The `cadkernel_constraints::geo::Point` for a marker on this entity — `None` for
    /// a marker this entity type/value doesn't support (see
    /// `sketch_constraints::resolve_point` for the same convention on the
    /// read side).
    fn point_for_marker(&self, marker: i32) -> Option<GPoint> {
        match (self, marker) {
            (EntityGeom::Line(l), 0) => Some(l.p1),
            (EntityGeom::Line(l), 1) => Some(l.p2),
            (EntityGeom::Circle(c), -3) => Some(c.center),
            (EntityGeom::Arc(a), 0) => Some(a.start),
            (EntityGeom::Arc(a), 1) => Some(a.end),
            (EntityGeom::Arc(a), -3) => Some(a.circle.center),
            (EntityGeom::Ellipse(e), -3) => Some(e.center),
            _ => None,
        }
    }
}

/// `Tangent`/`Normal` only ever care about "a whole circle-shaped curve" or
/// "a whole line" — an `Arc` collapses to its `.circle` here exactly like
/// `whole_circle` does, so those two constraints' own inline matches don't
/// need every Circle/Arc combination spelled out separately.
enum CircleOrLine {
    Circle(GCircle),
    Line(GLine),
    Other,
}

fn as_circle_or_line(g: EntityGeom) -> CircleOrLine {
    match g {
        EntityGeom::Circle(c) => CircleOrLine::Circle(c),
        EntityGeom::Arc(a) => CircleOrLine::Circle(a.circle),
        EntityGeom::Line(l) => CircleOrLine::Line(l),
        EntityGeom::Ellipse(_) => CircleOrLine::Other,
    }
}

/// Reads `handle`'s live geometry and registers it as fresh, free (never
/// `driven`) parameters in `sys` — every referenced entity is solved for,
/// nothing is a priori fixed; per the 2D kernel, an under-
/// constrained scope simply converges to the nearest configuration to its
/// current one, which is the correct behavior for a persistent system where
/// "what stays put" is a property of how many constraints exist, not a
/// convention about which entity was picked first (contrast the one-shot
/// `constrain::apply_*` commands, which do fix the first-picked entity —
/// they solve exactly one constraint in isolation, so they need that
/// convention; a whole scope's constraint graph does not).
fn register_entity(
    document: &acadrust::CadDocument,
    sys: &mut System,
    handle: Handle,
) -> Option<EntityGeom> {
    match document.get_entity(handle)? {
        EntityType::Line(l) => {
            let p1 = GPoint::new(
                sys.add_param(l.start.x, false),
                sys.add_param(l.start.y, false),
            );
            let p2 = GPoint::new(sys.add_param(l.end.x, false), sys.add_param(l.end.y, false));
            Some(EntityGeom::Line(GLine { p1, p2 }))
        }
        EntityType::Circle(c) => {
            let center = GPoint::new(
                sys.add_param(c.center.x, false),
                sys.add_param(c.center.y, false),
            );
            let rad = sys.add_param(c.radius, false);
            Some(EntityGeom::Circle(GCircle { center, rad }))
        }
        EntityType::Arc(a) => {
            // Full registration: center/radius, both angles, and real
            // start/end points seeded from acadrust's own `start_point`/
            // `end_point` (already consistent with center/radius/angle at
            // registration time) — `solve_scope` adds the arc-rules
            // constraints that keep them that way under solving. See this
            // module's doc comment.
            let center = GPoint::new(
                sys.add_param(a.center.x, false),
                sys.add_param(a.center.y, false),
            );
            let rad = sys.add_param(a.radius, false);
            let start_angle = sys.add_param(a.start_angle, false);
            let end_angle = sys.add_param(a.end_angle, false);
            let start_seed = a.start_point();
            let end_seed = a.end_point();
            let start = GPoint::new(
                sys.add_param(start_seed.x, false),
                sys.add_param(start_seed.y, false),
            );
            let end = GPoint::new(
                sys.add_param(end_seed.x, false),
                sys.add_param(end_seed.y, false),
            );
            Some(EntityGeom::Arc(GArc {
                circle: GCircle { center, rad },
                start,
                end,
                start_angle,
                end_angle,
            }))
        }
        EntityType::Ellipse(el) => {
            // acadrust's `Ellipse` is center + major-axis vector (its length
            // is the major radius) + minor/major ratio; `cadkernel_constraints::geo::
            // Ellipse` is center + one focus + minor radius. Converting:
            // minor_radius = major_radius * ratio, then the focus distance
            // c follows from a² = b² + c² (standard ellipse identity), and
            // focus1 sits `c` along the major-axis direction from center.
            let major_radius = el.major_axis.length();
            let minor_radius = major_radius * el.minor_axis_ratio;
            let focus_dist = (major_radius * major_radius - minor_radius * minor_radius)
                .max(0.0)
                .sqrt();
            // Direction is meaningless once major_radius is ~0 (a
            // degenerate point-ellipse) — arbitrarily fall back to +X
            // rather than dividing by ~0 in `normalize`.
            let unit_major = if major_radius > 1e-9 {
                el.major_axis.normalize()
            } else {
                acadrust::types::Vector3::UNIT_X
            };
            let focus1_point = el.center + unit_major * focus_dist;
            let center = GPoint::new(
                sys.add_param(el.center.x, false),
                sys.add_param(el.center.y, false),
            );
            let focus1 = GPoint::new(
                sys.add_param(focus1_point.x, false),
                sys.add_param(focus1_point.y, false),
            );
            let radmin = sys.add_param(minor_radius, false);
            Some(EntityGeom::Ellipse(GEllipse {
                center,
                focus1,
                radmin,
            }))
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
    if let Some(&geometry) = cache.get(&r.entity) {
        return Some(geometry);
    }
    let geometry = register_entity(document, sys, r.entity)?;
    cache.insert(r.entity, geometry);
    Some(geometry)
}

const PLANE_EPS: f64 = 1e-9;

fn is_world_z(normal: acadrust::types::Vector3) -> bool {
    normal.x.abs() <= PLANE_EPS
        && normal.y.abs() <= PLANE_EPS
        && (normal.z - 1.0).abs() <= PLANE_EPS
}

/// Returns the world-XY plane elevation supported by the current 2D bridge.
fn entity_plane_z(entity: &EntityType) -> Option<f64> {
    let z = match entity {
        EntityType::Line(line)
            if is_world_z(line.normal) && (line.start.z - line.end.z).abs() <= PLANE_EPS =>
        {
            line.start.z
        }
        EntityType::Circle(circle) if is_world_z(circle.normal) => circle.center.z,
        EntityType::Arc(arc) if is_world_z(arc.normal) => arc.center.z,
        EntityType::Ellipse(ellipse)
            if is_world_z(ellipse.normal) && ellipse.major_axis.z.abs() <= PLANE_EPS =>
        {
            ellipse.center.z
        }
        _ => return None,
    };
    z.is_finite().then_some(z)
}

fn refs_share_supported_plane(document: &acadrust::CadDocument, refs: &[SketchRef]) -> bool {
    let mut plane_z = None;
    for reference in refs {
        let Some(entity) = document.get_entity(reference.entity) else {
            return false;
        };
        let Some(z) = entity_plane_z(entity) else {
            return false;
        };
        if plane_z.is_some_and(|expected: f64| (expected - z).abs() > PLANE_EPS) {
            return false;
        }
        plane_z = Some(z);
    }
    plane_z.is_some()
}

fn resolved_target(params: &ParameterTable, constraint: &SketchConstraint) -> Option<f64> {
    let value = constraint.driving_param.as_ref()?.resolve(params).ok()?;
    if !value.is_finite() {
        return None;
    }
    let must_be_positive = matches!(
        constraint.kind,
        ConstraintKind::Distance
            | ConstraintKind::Radius
            | ConstraintKind::Diameter
            | ConstraintKind::ArcLength
    );
    (!must_be_positive || value > 0.0).then_some(value)
}

/// One [`SketchConstraint`]'s `cadkernel_constraints` construction: every system-level
/// constraint it contributes (most kinds contribute exactly one; a few —
/// `Coincident`/`Concentric`/`CenterPoint`'s X+Y halves, `Colinear`'s two
/// endpoints, `Midpoint`'s X+Y, `Symmetric`'s midpoint+perpendicular pair,
/// `Fixed`'s per-coordinate pins — contribute more). Empty means it can't be
/// built (an unsupported/not-yet-mapped kind, a dangling ref, a ref shape
/// the kind doesn't expect — e.g. `Parallel` needs two whole-line refs — or,
/// for a dimensional kind, a `driving_param` that fails to resolve). A constraint
/// that can't be built is simply skipped for this solve, not an error:
/// geometry it would have constrained is left alone, matching how a
/// dangling associative-dimension reference degrades today rather than
/// aborting the whole recompute. (A named-parameter *cycle* should be
/// unreachable here — `ParameterTable::set` already refuses to create one —
/// so it gets the same treatment as any other resolve failure rather than a
/// special case.)
fn build_constraint(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    params: &ParameterTable,
    c: &SketchConstraint,
) -> Vec<Rc<dyn Constraint>> {
    if !c.enabled || !refs_share_supported_plane(document, &c.refs) {
        return Vec::new();
    }

    let whole_line = |sys: &mut System, cache: &mut HashMap<_, _>, r: SketchRef| match resolve_ref(
        document, sys, cache, r,
    )? {
        EntityGeom::Line(l) => Some(l),
        EntityGeom::Circle(_) | EntityGeom::Arc(_) | EntityGeom::Ellipse(_) => None,
    };
    let whole_circle =
        |sys: &mut System, cache: &mut HashMap<_, _>, r: SketchRef| match resolve_ref(
            document, sys, cache, r,
        )? {
            EntityGeom::Circle(circ) => Some(circ),
            EntityGeom::Arc(a) => Some(a.circle),
            EntityGeom::Line(_) | EntityGeom::Ellipse(_) => None,
        };
    let point_ref = |sys: &mut System, cache: &mut HashMap<_, _>, r: SketchRef| {
        let marker = r.marker?;
        resolve_ref(document, sys, cache, r)?.point_for_marker(marker)
    };

    // `Coincident`/`Concentric`/`CenterPoint` all solve identically — two
    // points (an endpoint, a circle's center via the existing `-3` marker,
    // or a plain point) held equal on both axes. Only the DWG-native class
    // name and the UI entry point that produces their `refs` differ.
    let point_pair_equal = |sys: &mut System,
                            cache: &mut HashMap<_, _>,
                            refs: &[SketchRef]|
     -> Vec<Rc<dyn Constraint>> {
        let [a, b] = refs else { return Vec::new() };
        let (Some(pa), Some(pb)) = (point_ref(sys, cache, *a), point_ref(sys, cache, *b)) else {
            return Vec::new();
        };
        vec![
            Rc::new(Equal::new(pa.x, pb.x, 1.0)),
            Rc::new(Equal::new(pa.y, pb.y, 1.0)),
        ]
    };

    match c.kind {
        ConstraintKind::Coincident | ConstraintKind::Concentric | ConstraintKind::CenterPoint => {
            point_pair_equal(sys, cache, &c.refs)
        }
        ConstraintKind::Horizontal => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let Some(l) = whole_line(sys, cache, *r) else {
                return Vec::new();
            };
            vec![Rc::new(Equal::new(l.p1.y, l.p2.y, 1.0))]
        }
        ConstraintKind::Vertical => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let Some(l) = whole_line(sys, cache, *r) else {
                return Vec::new();
            };
            vec![Rc::new(Equal::new(l.p1.x, l.p2.x, 1.0))]
        }
        ConstraintKind::Parallel => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(fixed), Some(moving)) =
                (whole_line(sys, cache, *a), whole_line(sys, cache, *b))
            else {
                return Vec::new();
            };
            vec![Rc::new(ParallelConstraint::new(sys.store(), moving, fixed))]
        }
        ConstraintKind::Perpendicular => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(fixed), Some(moving)) =
                (whole_line(sys, cache, *a), whole_line(sys, cache, *b))
            else {
                return Vec::new();
            };
            vec![Rc::new(PerpendicularConstraint::new(
                sys.store(),
                moving,
                fixed,
            ))]
        }
        ConstraintKind::Equal => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            if let (Some(la), Some(lb)) = (whole_line(sys, cache, *a), whole_line(sys, cache, *b)) {
                return vec![Rc::new(EqualLineLength::new(lb, la))];
            }
            let (Some(ca), Some(cb)) = (whole_circle(sys, cache, *a), whole_circle(sys, cache, *b))
            else {
                return Vec::new();
            };
            vec![Rc::new(Equal::new(cb.rad, ca.rad, 1.0))]
        }
        ConstraintKind::EqualDistance => {
            let [a, b, c2, d] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(pa), Some(pb), Some(pc), Some(pd)) = (
                point_ref(sys, cache, *a),
                point_ref(sys, cache, *b),
                point_ref(sys, cache, *c2),
                point_ref(sys, cache, *d),
            ) else {
                return Vec::new();
            };
            vec![Rc::new(EqualLineLength::new(
                GLine { p1: pc, p2: pd },
                GLine { p1: pa, p2: pb },
            ))]
        }
        ConstraintKind::Colinear => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(onto), Some(moving)) =
                (whole_line(sys, cache, *a), whole_line(sys, cache, *b))
            else {
                return Vec::new();
            };
            // Pinning both of `moving`'s endpoints onto `onto`'s infinite
            // line forces the two to coincide (as long as `moving`'s own
            // two points stay distinct) — no dedicated "colinear" primitive
            // needed, `PointOnLine` applied twice does it.
            vec![
                Rc::new(PointOnLine::new(moving.p1, onto)),
                Rc::new(PointOnLine::new(moving.p2, onto)),
            ]
        }
        ConstraintKind::Midpoint => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(p), Some(l)) = (point_ref(sys, cache, *a), whole_line(sys, cache, *b)) else {
                return Vec::new();
            };
            vec![
                Rc::new(CenterOfGravity::new(
                    p.x,
                    vec![l.p1.x, l.p2.x],
                    vec![0.5, 0.5],
                )),
                Rc::new(CenterOfGravity::new(
                    p.y,
                    vec![l.p1.y, l.p2.y],
                    vec![0.5, 0.5],
                )),
            ]
        }
        ConstraintKind::PointOnCurve => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(p), Some(geom)) = (
                point_ref(sys, cache, *a),
                resolve_ref(document, sys, cache, *b),
            ) else {
                return Vec::new();
            };
            match geom {
                EntityGeom::Line(l) => vec![Rc::new(PointOnLine::new(p, l))],
                EntityGeom::Circle(circ) => {
                    let zero = sys.add_param(0.0, true);
                    vec![Rc::new(P2CDistance::new(circ, p, zero))]
                }
                // Same "distance to the underlying circle is zero" relation
                // as the `Circle` arm — doesn't restrict the point to
                // within the arc's own sweep, matching that same
                // pre-existing simplification for `Circle`.
                EntityGeom::Arc(a) => {
                    let zero = sys.add_param(0.0, true);
                    vec![Rc::new(P2CDistance::new(a.circle, p, zero))]
                }
                // `PointOnEllipse` (`cadkernel_constraints::constraints::conic`) isn't
                // wired up yet — deliberately deferred alongside
                // ellipse-tangency, same as this module's doc comment.
                EntityGeom::Ellipse(_) => Vec::new(),
            }
        }
        ConstraintKind::Symmetric => {
            let [a, b, m] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(pa), Some(pb), Some(mirror)) = (
                point_ref(sys, cache, *a),
                point_ref(sys, cache, *b),
                whole_line(sys, cache, *m),
            ) else {
                return Vec::new();
            };
            let pair = GLine { p1: pa, p2: pb };
            vec![
                Rc::new(MidpointOnLine::new(pair, mirror)),
                Rc::new(PerpendicularConstraint::new(sys.store(), pair, mirror)),
            ]
        }
        ConstraintKind::Fixed => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let Some(geom) = resolve_ref(document, sys, cache, *r) else {
                return Vec::new();
            };
            match geom {
                EntityGeom::Line(l) => {
                    let (x1, y1, x2, y2) = {
                        let store = sys.store();
                        (
                            store.get(l.p1.x),
                            store.get(l.p1.y),
                            store.get(l.p2.x),
                            store.get(l.p2.y),
                        )
                    };
                    vec![
                        Rc::new(Equal::new(l.p1.x, sys.add_param(x1, true), 1.0)),
                        Rc::new(Equal::new(l.p1.y, sys.add_param(y1, true), 1.0)),
                        Rc::new(Equal::new(l.p2.x, sys.add_param(x2, true), 1.0)),
                        Rc::new(Equal::new(l.p2.y, sys.add_param(y2, true), 1.0)),
                    ]
                }
                EntityGeom::Circle(circ) => {
                    let (cx, cy, r) = {
                        let store = sys.store();
                        (
                            store.get(circ.center.x),
                            store.get(circ.center.y),
                            store.get(circ.rad),
                        )
                    };
                    vec![
                        Rc::new(Equal::new(circ.center.x, sys.add_param(cx, true), 1.0)),
                        Rc::new(Equal::new(circ.center.y, sys.add_param(cy, true), 1.0)),
                        Rc::new(Equal::new(circ.rad, sys.add_param(r, true), 1.0)),
                    ]
                }
                // Pinning the 5 intrinsic params (center, radius, both
                // angles) is enough — `start`/`end` are already tied to
                // those via the arc-rules `CurveValue` constraints
                // `solve_scope` adds for every arc, so pinning them too
                // would just be redundant.
                EntityGeom::Arc(a) => {
                    let (cx, cy, r, sa, ea) = {
                        let store = sys.store();
                        (
                            store.get(a.circle.center.x),
                            store.get(a.circle.center.y),
                            store.get(a.circle.rad),
                            store.get(a.start_angle),
                            store.get(a.end_angle),
                        )
                    };
                    vec![
                        Rc::new(Equal::new(a.circle.center.x, sys.add_param(cx, true), 1.0)),
                        Rc::new(Equal::new(a.circle.center.y, sys.add_param(cy, true), 1.0)),
                        Rc::new(Equal::new(a.circle.rad, sys.add_param(r, true), 1.0)),
                        Rc::new(Equal::new(a.start_angle, sys.add_param(sa, true), 1.0)),
                        Rc::new(Equal::new(a.end_angle, sys.add_param(ea, true), 1.0)),
                    ]
                }
                // Pinning center + radmin is enough — `focus1` is already
                // tied to `center` by the ellipse-rules `Difference`
                // constraints `solve_scope` adds for every registered
                // ellipse (see this module's doc comment), so with `center`
                // pinned here too, pinning `focus1` again would just be
                // redundant (same reasoning as Arc's `Fixed` arm above).
                EntityGeom::Ellipse(el) => {
                    let (cx, cy, b) = {
                        let store = sys.store();
                        (
                            store.get(el.center.x),
                            store.get(el.center.y),
                            store.get(el.radmin),
                        )
                    };
                    vec![
                        Rc::new(Equal::new(el.center.x, sys.add_param(cx, true), 1.0)),
                        Rc::new(Equal::new(el.center.y, sys.add_param(cy, true), 1.0)),
                        Rc::new(Equal::new(el.radmin, sys.add_param(b, true), 1.0)),
                    ]
                }
            }
        }
        ConstraintKind::Distance => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(pa), Some(pb)) = (point_ref(sys, cache, *a), point_ref(sys, cache, *b))
            else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let target = sys.add_param(resolved, true);
            vec![Rc::new(P2PDistance::new(pa, pb, target))]
        }
        ConstraintKind::Angle => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(fixed), Some(moving)) =
                (whole_line(sys, cache, *a), whole_line(sys, cache, *b))
            else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let angle = sys.add_param(resolved.to_radians(), true);
            vec![Rc::new(L2LAngle::new(fixed, moving, angle))]
        }
        ConstraintKind::Radius => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let Some(circle) = whole_circle(sys, cache, *r) else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let target = sys.add_param(resolved, true);
            vec![Rc::new(Equal::new(circle.rad, target, 1.0))]
        }
        // Same math as `Radius`, just `radius = target / 2` instead of
        // `radius = target` — `Equal`'s `ratio` param already supports
        // this, matching how the native format stores Diameter as the same
        // `ACRADIUSDIAMETERCONSTRAINT` class with a different mode byte
        // rather than a distinct one (`dwg_native_constraints.rs`).
        ConstraintKind::Diameter => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let Some(circle) = whole_circle(sys, cache, *r) else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let target = sys.add_param(resolved, true);
            vec![Rc::new(Equal::new(circle.rad, target, 0.5))]
        }
        // Signed X-only/Y-only component of the distance between two
        // points — `Difference::new(p1, p2, d)` is `p2 - p1 == d`, matching
        // the native format's `ACDISTANCECONSTRAINT` with a fixed-direction
        // vector rather than a distinct native class
        // (`dwg_native_constraints.rs`).
        ConstraintKind::DistanceX => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(pa), Some(pb)) = (point_ref(sys, cache, *a), point_ref(sys, cache, *b))
            else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let target = sys.add_param(resolved, true);
            vec![Rc::new(Difference::new(pa.x, pb.x, target))]
        }
        ConstraintKind::DistanceY => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(pa), Some(pb)) = (point_ref(sys, cache, *a), point_ref(sys, cache, *b))
            else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let target = sys.add_param(resolved, true);
            vec![Rc::new(Difference::new(pa.y, pb.y, target))]
        }
        ConstraintKind::Tangent => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(ga), Some(gb)) = (
                resolve_ref(document, sys, cache, *a),
                resolve_ref(document, sys, cache, *b),
            ) else {
                return Vec::new();
            };
            // An `Arc`'s `.circle` behaves identically to a plain `Circle`
            // for tangency math — normalize both refs down first so the
            // match below doesn't need every Circle/Arc combination
            // written out separately.
            match (as_circle_or_line(ga), as_circle_or_line(gb)) {
                (CircleOrLine::Circle(c1), CircleOrLine::Circle(c2)) => {
                    let store = sys.store();
                    let (x1, y1, r1) = (
                        store.get(c1.center.x),
                        store.get(c1.center.y),
                        store.get(c1.rad),
                    );
                    let (x2, y2, r2) = (
                        store.get(c2.center.x),
                        store.get(c2.center.y),
                        store.get(c2.rad),
                    );
                    let center_dist = ((x1 - x2).powi(2) + (y1 - y2).powi(2)).sqrt();
                    // One circle already sits inside the other's span, rather
                    // than the two side by side — pick internal tangency so
                    // the first solve doesn't have to cross the singularity
                    // between the two tangency configurations.
                    let internal = center_dist < (r1 - r2).abs();
                    vec![Rc::new(TangentCircumf::new(
                        c1.center, c2.center, c1.rad, c2.rad, internal,
                    ))]
                }
                (CircleOrLine::Circle(circ), CircleOrLine::Line(line))
                | (CircleOrLine::Line(line), CircleOrLine::Circle(circ)) => {
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
                    vec![Rc::new(C2LDistance::new(circ, line, zero, ccw, false))]
                }
                // Line-Line tangency has no meaning; either side being an
                // Ellipse falls here too (`PointOnEllipse`/tangency isn't
                // wired up yet).
                _ => Vec::new(),
            }
        }
        // A circle's radius is always normal to its own tangent, so
        // "line normal to circle/arc" reduces to "line passes through the
        // circle's center" — the same `PointOnLine` primitive
        // `PointOnCurve` already uses for a point-on-line case.
        ConstraintKind::Normal => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(ga), Some(gb)) = (
                resolve_ref(document, sys, cache, *a),
                resolve_ref(document, sys, cache, *b),
            ) else {
                return Vec::new();
            };
            match (as_circle_or_line(ga), as_circle_or_line(gb)) {
                (CircleOrLine::Circle(circ), CircleOrLine::Line(line))
                | (CircleOrLine::Line(line), CircleOrLine::Circle(circ)) => {
                    vec![Rc::new(PointOnLine::new(circ.center, line))]
                }
                // Line-Line has no meaning here (that's `Perpendicular`);
                // Circle-Circle "normal" (orthogonal circles) needs a
                // different, not-yet-implemented relation.
                _ => Vec::new(),
            }
        }
        ConstraintKind::ArcLength => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let Some(EntityGeom::Arc(arc)) = resolve_ref(document, sys, cache, *r) else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let target = sys.add_param(resolved, true);
            vec![Rc::new(ArcLength::new(arc, target))]
        }
    }
}

const MOVE_EPS: f64 = 1e-9;
type SolveResult = (
    Vec<(Handle, EntityType)>,
    usize,
    Vec<(
        ConstraintId,
        cadkernel_constraints::diagnosis::RedundancyKind,
    )>,
);

/// Rebuilds `set`'s entire `cadkernel_constraints::System` from current document
/// geometry, solves it, and returns the resulting entity states for every
/// handle whose registered coordinates actually moved beyond floating-point
/// noise, plus the scope's total remaining degrees of freedom and any
/// redundant/conflicting constraints, resolved
/// back to the `ConstraintId`s a `ConflictResolverPanel` can name and offer
/// to remove. `None` if nothing in the scope could be built (no constraint
/// resolved to anything). DOF/conflicts are still returned even when every
/// subsystem fails to solve (rank is a structural property of the Jacobian,
/// not of whether Dogleg converged) — only the geometry write-back is gated
/// on a successful solve.
fn solve_scope(
    document: &acadrust::CadDocument,
    params: &ParameterTable,
    set: &SketchConstraintSet,
) -> Option<SolveResult> {
    let mut sys = System::new();
    let mut cache: HashMap<Handle, EntityGeom> = HashMap::new();
    // Tracks which system-level `cadkernel_constraints` constraint(s) came from which
    // `SketchConstraint` — a `SubSystem`'s redundant-row indices (from
    // `cadkernel_constraints::diagnosis`) are local to that partition's own constraint
    // list, not `set.constraints`' indices, and several kinds contribute
    // more than one system-level constraint per `SketchConstraint` (see
    // `build_constraint`'s doc comment). `Rc::ptr_eq` against this after
    // partitioning resolves a row back to the `ConstraintId` the UI
    // actually names.
    let mut owner: Vec<(Rc<dyn Constraint>, ConstraintId)> = Vec::new();

    for c in &set.constraints {
        for constraint in build_constraint(document, &mut sys, &mut cache, params, c) {
            owner.push((constraint.clone(), c.id));
            sys.add_constraint(constraint);
        }
    }

    if cache.is_empty() {
        return None;
    }

    // Arc rules (this module's doc comment): keep every registered arc's
    // `start`/`end` points consistent with its center/radius/angle,
    // unconditionally — the kernel adds these the moment an arc
    // exists, not only when some `SketchConstraint` happens to reference
    // its endpoint. Not tracked in `owner`: these are solver-internal
    // bookkeeping, never redundant with anything a user-facing constraint
    // could name, so there's no `ConstraintId` for them to report against.
    for geom in cache.values() {
        let EntityGeom::Arc(arc) = geom else { continue };
        let curve = Rc::new(*arc);
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.start,
            arc.start.x,
            curve.clone(),
            arc.start_angle,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.start,
            arc.start.y,
            curve.clone(),
            arc.start_angle,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.end,
            arc.end.x,
            curve.clone(),
            arc.end_angle,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.end,
            arc.end.y,
            curve,
            arc.end_angle,
        )));
    }

    // Ellipse rules (same motivation as arc rules, different mechanism):
    // `cadkernel_constraints::geo::Ellipse` stores `focus1` as an absolute point, not an
    // offset from `center`. If some constraint (e.g. Concentric) pulls only
    // on `center` and nothing references `focus1`/`radmin`, the solver
    // correctly leaves `focus1`'s *absolute* coordinates untouched — but
    // `center` moved and `focus1` didn't, so the derived vector
    // `focus1 - center` (which is what `major_axis` direction/length and
    // `minor_axis_ratio` are actually computed from on write-back) changes
    // anyway, making the ellipse appear to rotate and reshape even though
    // no shape parameter was itself pulled on. Pinning `focus1 - center` to
    // its pre-solve (seed) value forces `focus1` to translate rigidly with
    // `center`, keeping orientation/eccentricity invariant unless something
    // actually constrains `focus1`/`radmin` directly (only `Fixed` does
    // today). `radmin` needs no equivalent rule — as a lone scalar it isn't
    // coupled to `center`'s movement, so it already stays put on its own.
    for geom in cache.values() {
        let EntityGeom::Ellipse(el) = geom else {
            continue;
        };
        let (cx, cy, fx, fy) = {
            let store = sys.store();
            (
                store.get(el.center.x),
                store.get(el.center.y),
                store.get(el.focus1.x),
                store.get(el.focus1.y),
            )
        };
        let dx = sys.add_param(fx - cx, true);
        let dy = sys.add_param(fy - cy, true);
        sys.add_constraint(Rc::new(Difference::new(el.center.x, el.focus1.x, dx)));
        sys.add_constraint(Rc::new(Difference::new(el.center.y, el.focus1.y, dy)));
    }

    let partitions = sys.partition();
    for sub in &partitions {
        solve_dl(sub, sys.store_mut());
    }
    // `System::partition` only includes parameters referenced by a
    // constraint, so an otherwise registered but unconstrained coordinate (e.g. a
    // line's X after only a Horizontal constraint pins its Ys) is invisible
    // to `diagnose` entirely, undercounting DOF. Every such untouched free
    // param is unconstrained on its own, i.e. exactly 1 DOF each — added
    // back here rather than fixed in `cadkernel_constraints::diagnosis`, which correctly
    // has no opinion on params outside the `SubSystem` it was handed.
    let total_free: usize = cache
        .values()
        .map(|g| match g {
            EntityGeom::Line(_) => 4,
            EntityGeom::Circle(_) => 3,
            // Raw param count (center×2, rad, start×2, end×2, both
            // angles) — same "raw, not netted against its own
            // constraints" convention as every other arm here. The four
            // arc-rules `CurveValue` constraints (always present, added
            // just above) already reduce this to 5 *effective* DOF through
            // the normal `touched_free`/`diag.dof` accounting below, the
            // same way any other constraint would.
            EntityGeom::Arc(_) => 9,
            EntityGeom::Ellipse(_) => 5,
        })
        .sum();
    let touched_free: usize = partitions.iter().map(|s| s.p_size()).sum();
    let mut dof = total_free.saturating_sub(touched_free);
    let mut conflicts: Vec<(
        ConstraintId,
        cadkernel_constraints::diagnosis::RedundancyKind,
    )> = Vec::new();
    for sub in &partitions {
        let diag = cadkernel_constraints::diagnosis::diagnose(sub, sys.store());
        dof += diag.dof;
        if diag.redundant.is_empty() {
            continue;
        }
        // Opt-in per `classify_redundant`'s own doc comment (one extra solve
        // per redundant row) — only reached when a partition is actually
        // over-constrained, which is rare, so this never costs anything on
        // the common "no redundancy" path.
        for (row, kind) in
            cadkernel_constraints::diagnosis::classify_redundant(sub, sys.store(), &diag.redundant)
        {
            let row_constraint = &sub.constraints()[row];
            if let Some(&(_, id)) = owner.iter().find(|(rc, _)| Rc::ptr_eq(rc, row_constraint)) {
                conflicts.push((id, kind));
            }
        }
    }

    let store = sys.store();
    let mut results = Vec::new();
    for (&handle, geom) in &cache {
        let Some(entity) = document.get_entity(handle) else {
            continue;
        };
        match (entity, geom) {
            (EntityType::Line(l), EntityGeom::Line(g)) => {
                let (x1, y1, x2, y2) = (
                    store.get(g.p1.x),
                    store.get(g.p1.y),
                    store.get(g.p2.x),
                    store.get(g.p2.y),
                );
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
                let (cx, cy, r) = (
                    store.get(g.center.x),
                    store.get(g.center.y),
                    store.get(g.rad),
                );
                if (cx - c.center.x).abs() > MOVE_EPS
                    || (cy - c.center.y).abs() > MOVE_EPS
                    || (r - c.radius).abs() > MOVE_EPS
                {
                    let mut updated = c.clone();
                    updated.center.x = cx;
                    updated.center.y = cy;
                    updated.radius = r;
                    results.push((handle, EntityType::Circle(updated)));
                }
            }
            (EntityType::Arc(a), EntityGeom::Arc(g)) => {
                // Center/radius/angles only — `start`/`end` are derived
                // (acadrust's `Arc` has no separate stored fields for them;
                // `start_point()`/`end_point()` compute them from these
                // same four), so nothing further needs writing back for
                // them specifically.
                let (cx, cy, r) = (
                    store.get(g.circle.center.x),
                    store.get(g.circle.center.y),
                    store.get(g.circle.rad),
                );
                let (new_start_angle, new_end_angle) =
                    (store.get(g.start_angle), store.get(g.end_angle));
                if (cx - a.center.x).abs() > MOVE_EPS
                    || (cy - a.center.y).abs() > MOVE_EPS
                    || (r - a.radius).abs() > MOVE_EPS
                    || (new_start_angle - a.start_angle).abs() > MOVE_EPS
                    || (new_end_angle - a.end_angle).abs() > MOVE_EPS
                {
                    let mut updated = a.clone();
                    updated.center.x = cx;
                    updated.center.y = cy;
                    updated.radius = r;
                    updated.start_angle = new_start_angle;
                    updated.end_angle = new_end_angle;
                    results.push((handle, EntityType::Arc(updated)));
                }
            }
            (EntityType::Ellipse(el), EntityGeom::Ellipse(g)) => {
                // Converting back to acadrust's center + major-axis-vector +
                // minor/major-ratio parametrization — the inverse of
                // `register_entity`'s `Ellipse` arm. `rad_maj_at` (already
                // provided by `cadkernel_constraints::geo::Ellipse`) does the a²=b²+c²
                // algebra; only the major-axis *direction* needs deriving
                // here, from the (possibly moved) focus relative to center.
                let (cx, cy) = (store.get(g.center.x), store.get(g.center.y));
                let (fx, fy) = (store.get(g.focus1.x), store.get(g.focus1.y));
                let minor_radius = store.get(g.radmin);
                let (major_radius, _) = g.rad_maj_at(store, None);
                let focus_dist = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
                // Direction is ill-defined once the ellipse is (near)
                // circular — keep whatever direction it already had rather
                // than snapping to an arbitrary axis.
                let unit_major = if focus_dist > 1e-9 {
                    acadrust::types::Vector3::new(fx - cx, fy - cy, 0.0).normalize()
                } else {
                    el.major_axis.normalize()
                };
                let new_major_axis = unit_major * major_radius;
                let new_ratio = if major_radius > 1e-9 {
                    minor_radius / major_radius
                } else {
                    0.0
                };
                if (cx - el.center.x).abs() > MOVE_EPS
                    || (cy - el.center.y).abs() > MOVE_EPS
                    || (new_major_axis.x - el.major_axis.x).abs() > MOVE_EPS
                    || (new_major_axis.y - el.major_axis.y).abs() > MOVE_EPS
                    || (new_ratio - el.minor_axis_ratio).abs() > MOVE_EPS
                {
                    let mut updated = el.clone();
                    updated.center.x = cx;
                    updated.center.y = cy;
                    updated.major_axis = new_major_axis;
                    updated.minor_axis_ratio = new_ratio;
                    results.push((handle, EntityType::Ellipse(updated)));
                }
            }
            _ => {}
        }
    }
    Some((results, dof, conflicts))
}

impl Scene {
    /// Rejects constraints the 2D kernel bridge cannot represent before they
    /// are persisted or reported as applied.
    pub(crate) fn validate_sketch_constraint(
        &self,
        kind: ConstraintKind,
        refs: &[SketchRef],
        driving_param: Option<&super::named_parameters::DrivingValue>,
    ) -> Result<(), &'static str> {
        if !refs_share_supported_plane(&self.document, refs) {
            return Err(
                "Constraint references must be supported entities on the same world-XY plane.",
            );
        }
        let validation_target = match driving_param {
            Some(super::named_parameters::DrivingValue::Named(name))
                if !self.named_parameters.contains(name) =>
            {
                Some(super::named_parameters::DrivingValue::Literal(1.0))
            }
            value => value.cloned(),
        };
        let candidate = SketchConstraint {
            id: 0,
            kind,
            refs: refs.to_vec(),
            driving_param: validation_target,
            enabled: true,
        };
        let mut system = System::new();
        let mut cache = HashMap::new();
        if build_constraint(
            &self.document,
            &mut system,
            &mut cache,
            &self.named_parameters,
            &candidate,
        )
        .is_empty()
        {
            return Err("The selected entities or target value do not support this constraint.");
        }
        Ok(())
    }

    /// Re-solves every constraint scope touched by `changes`. Returns
    /// the resulting `(Handle, ChangeKind::Modified)` entries the same way
    /// `refresh_associative_dimensions`/`_hatches` do, for `bump_entities`
    /// to fold into its own `changes` vec.
    pub(crate) fn refresh_sketch_constraints(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        // An erased entity takes its constraints with it instead of leaving
        // dangling references. Done first,
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
                if self.sketch_constraints[i]
                    .constraints_touching(*handle)
                    .next()
                    .is_some()
                {
                    let scope = self.sketch_constraints[i].scope;
                    let before = self.sketch_constraints[i].clone();
                    self.record_undo_sketch_constraints_before(scope, before);
                    self.sketch_constraints[i].remove_all_touching(*handle);
                }
            }
        }

        let mut result = Vec::new();
        for i in 0..self.sketch_constraints.len() {
            let touched = changes.iter().any(|(handle, _)| {
                self.sketch_constraints[i]
                    .constraints_touching(*handle)
                    .next()
                    .is_some()
            });
            if !touched {
                continue;
            }
            let Some((solved, dof, conflicts)) = solve_scope(
                &self.document,
                &self.named_parameters,
                &self.sketch_constraints[i],
            ) else {
                continue;
            };
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

    /// Re-solves touched scopes during a grip drag without mutating the
    /// document or recording undo. The caller owns preview write-back and the
    /// final gesture history entry.
    pub(crate) fn solve_sketch_constraints_preview(
        &self,
        touched: &[Handle],
    ) -> Vec<(Handle, EntityType)> {
        if self.sketch_constraints.is_empty() || touched.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        for set in &self.sketch_constraints {
            let is_touched = touched
                .iter()
                .any(|handle| set.constraints_touching(*handle).next().is_some());
            if !is_touched {
                continue;
            }
            let Some((solved, _dof, _conflicts)) =
                solve_scope(&self.document, &self.named_parameters, set)
            else {
                continue;
            };
            result.extend(solved);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::super::named_parameters::DrivingValue;
    use super::super::sketch_constraints::{
        ConstraintKind, SketchConstraintSet, SketchRef, SketchScope,
    };
    use super::Scene;
    use acadrust::entities::EntityType;
    use acadrust::types::Vector3;

    #[test]
    fn validation_rejects_unsupported_entity_shapes_and_planes() {
        let mut scene = Scene::new();
        let circle = scene.add_entity(EntityType::Circle(
            acadrust::entities::Circle::from_center_radius(Vector3::ZERO, 2.0),
        ));
        assert!(scene
            .validate_sketch_constraint(
                ConstraintKind::Horizontal,
                &[SketchRef::whole(circle)],
                None
            )
            .is_err());

        let lower = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        let upper = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 1.0),
        )));
        assert!(scene
            .validate_sketch_constraint(
                ConstraintKind::Parallel,
                &[SketchRef::whole(lower), SketchRef::whole(upper)],
                None,
            )
            .is_err());
    }

    #[test]
    fn validation_enforces_dimensional_value_domains() {
        let mut scene = Scene::new();
        let line = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )));
        let refs = [SketchRef::point(line, 0), SketchRef::point(line, 1)];
        assert!(scene
            .validate_sketch_constraint(
                ConstraintKind::Distance,
                &refs,
                Some(&DrivingValue::Literal(-1.0)),
            )
            .is_err());
        assert!(scene
            .validate_sketch_constraint(
                ConstraintKind::DistanceX,
                &refs,
                Some(&DrivingValue::Literal(-1.0)),
            )
            .is_ok());
        assert!(scene
            .validate_sketch_constraint(
                ConstraintKind::DistanceX,
                &refs,
                Some(&DrivingValue::Literal(f64::NAN)),
            )
            .is_err());
    }

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
        set.add(
            ConstraintKind::Parallel,
            vec![SketchRef::whole(a), SketchRef::whole(b)],
            None,
        );
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
        let recording = scene
            .take_undo_recording()
            .expect("an undo recording should still be open");

        let (entities, _objects, _sketch_constraints, _named_parameters) =
            recording.into_recorded_images();
        let touched: std::collections::HashSet<_> = entities.iter().map(|(h, _)| *h).collect();
        assert!(
            touched.contains(&a),
            "the directly-edited line must be in the undo delta"
        );
        assert!(
            touched.contains(&b),
            "the constraint-solved neighbor must ride the same undo delta"
        );
    }

    /// Erasing an entity records the removed constraints in the same undo
    /// transaction as the geometry.
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
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Coincident,
                vec![SketchRef::point(a, 1), SketchRef::point(b, 0)],
                None,
            );
        assert_eq!(
            scene
                .sketch_constraint_set(SketchScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1
        );

        scene.begin_undo_recording();
        scene.erase_entities(&[a]);

        // Ensure the test exercises restoration rather than a no-op.
        assert_eq!(
            scene
                .sketch_constraint_set(SketchScope::ModelSpace)
                .map_or(0, |s| s.constraints.len()),
            0,
            "the constraint touching the erased line should be gone from the live set"
        );

        let recording = scene
            .take_undo_recording()
            .expect("an undo recording should still be open");
        let (_entities, _objects, sketch_constraints, _named_parameters) =
            recording.into_recorded_images();
        assert_eq!(
            sketch_constraints.len(),
            1,
            "the touched scope's before-image must be captured"
        );
        let (scope, before) = &sketch_constraints[0];
        assert_eq!(*scope, SketchScope::ModelSpace);
        assert_eq!(
            before.constraints.len(),
            1,
            "the before-image must still hold the constraint as it was before the erase"
        );
        assert_eq!(before.constraints[0].kind, ConstraintKind::Coincident);
        assert_eq!(
            before.constraints[0].refs,
            vec![SketchRef::point(a, 1), SketchRef::point(b, 0)]
        );

        // What `apply_delta_state` does with this on undo: install the
        // before-image back as the live scope state.
        *scene.sketch_constraint_set_mut(*scope) = before.clone();
        assert_eq!(
            scene
                .sketch_constraint_set(SketchScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1,
            "restoring the before-image must bring the constraint back"
        );
    }

    /// `solve_sketch_constraints_preview` reports the constrained
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
        set.add(
            ConstraintKind::Parallel,
            vec![SketchRef::whole(a), SketchRef::whole(b)],
            None,
        );
        scene.sketch_constraints.push(set);

        // Simulate a live grip drag: mutate `a` directly (as `apply_grip`
        // would each frame), without going through `bump_entities` at all.
        if let Some(EntityType::Line(l)) = scene.document.get_entity_mut(a) {
            l.end = Vector3::new(10.0, 6.0, 0.0);
        }
        let b_before = scene.document.get_entity(b).cloned();

        let solved = scene.solve_sketch_constraints_preview(&[a]);

        assert_eq!(
            scene.document.get_entity(b),
            b_before.as_ref(),
            "preview must not mutate the document"
        );
        // The whole scope solves together (every registered entity's params
        // are free, not just `b`'s — see `register_entity`'s doc comment),
        // so `a` itself may also have shifted slightly to reach the nearest
        // mutually-parallel configuration; read whichever position `a` ends
        // up at from `solved` too, rather than assuming it stayed exactly
        // where the simulated drag put it.
        let line_dir = |entity: &EntityType| {
            let EntityType::Line(l) = entity else {
                panic!("expected a Line")
            };
            (l.end.x - l.start.x, l.end.y - l.start.y)
        };
        let dir_a = solved
            .iter()
            .find(|(h, _)| *h == a)
            .map(|(_, e)| line_dir(e))
            .unwrap_or((10.0, 6.0));
        let (_, moved_entity) = solved
            .iter()
            .find(|(h, _)| *h == b)
            .expect("b should be reported as moved");
        let dir_b = line_dir(moved_entity);
        let cross = dir_a.0 * dir_b.1 - dir_a.1 * dir_b.0;
        assert!(
            cross.abs() < 1e-6,
            "the previewed positions should be mutually parallel: dir_a={dir_a:?} dir_b={dir_b:?}"
        );
    }
}
