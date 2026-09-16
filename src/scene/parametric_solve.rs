//! Builds and solves two-dimensional parametric constraints through the geometry kernel.
//! Unsupported entity shapes, invalid target values, and incompatible planes are skipped.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use acadrust::entities::EntityType;
use acadrust::types::{Handle, Vector3};

use cadkernel_constraints::constraints::angle_distance::L2LAngle;
use cadkernel_constraints::constraints::bspline::{Coord as SplineCoord, PointOnBSpline};
use cadkernel_constraints::constraints::circle_arc::{C2LDistance, P2CDistance, TangentCircumf};
use cadkernel_constraints::constraints::conic::{
    EqualMajorAxesConic, PointOnEllipse, TangentEllipseLine,
};
use cadkernel_constraints::constraints::curve_generic::{BoundedArcValue, CurveValue};
use cadkernel_constraints::constraints::point_line::{
    CenterOfGravity, Difference, Equal, EqualLineLength, MidpointOnLine, P2PDistance,
    Parallel as ParallelConstraint, Perpendicular as PerpendicularConstraint, PointOnLine,
    ProjectedDistance, ProjectedDistanceAlongLine,
};
use cadkernel_constraints::constraints::Constraint;
use cadkernel_constraints::geo::{
    Arc as GArc, BSpline as GBSpline, Circle as GCircle, Conic as GConic, Ellipse as GEllipse,
    Line as GLine, Point as GPoint,
};
use cadkernel_constraints::solvers::dogleg::solve_dl;
use cadkernel_constraints::solvers::lm::solve_lm;
use cadkernel_constraints::solvers::SolveStatus;
use cadkernel_constraints::system::System;

use super::named_parameters::ParameterTable;
use super::parametric_constraints::{
    angle_sector, distance_direction_type, ConstraintId, ConstraintKind, ParametricConstraint,
    ParametricConstraintSet, ParametricRef,
};
use super::{ChangeKind, Scene};

/// One referenced entity's geometry, registered into an `cadkernel_constraints::System`'s
/// parameter store.
#[derive(Clone)]
enum EntityGeom {
    Point(GPoint),
    /// Text insertion plus its baseline direction.  Keeping two points lets
    /// geometric constraints translate and rotate text as one linear object.
    TextAxis(GLine),
    Line(GLine),
    Ray(GLine),
    XLine(GLine),
    Polyline {
        points: Rc<Vec<GPoint>>,
        straight: Rc<Vec<bool>>,
        arcs: Rc<Vec<Option<PolylineArc>>>,
        closed: bool,
    },
    Circle(GCircle),
    /// Full `geo::Arc` — center/radius (via `.circle`), `start_angle`/
    /// `end_angle`, and real `start`/`end` points kept consistent with
    /// those by the arc-rules `CurveValue` constraints `solve_scope` adds
    /// for every registered arc. See this module's doc comment.
    Arc(GArc),
    /// See `register_entity` for the entity-to-kernel parametrization.
    Ellipse(GEllipse),
    Spline {
        curve: GBSpline,
        control_z: Rc<Vec<f64>>,
    },
}

#[derive(Clone, Copy)]
struct PolylineArc {
    arc: GArc,
    sweep: f64,
}

impl EntityGeom {
    /// The `cadkernel_constraints::geo::Point` for a marker on this entity — `None` for
    /// a marker this entity type/value doesn't support (see
    /// `parametric_constraints::resolve_point` for the same convention on the
    /// read side).
    fn point_for_marker(&self, marker: i32) -> Option<GPoint> {
        match (self, marker) {
            (EntityGeom::Point(point), 0) => Some(*point),
            (EntityGeom::TextAxis(line), 0) => Some(line.p1),
            (EntityGeom::Line(l), 0) => Some(l.p1),
            (EntityGeom::Line(l), 1) => Some(l.p2),
            (EntityGeom::Ray(l), 0) => Some(l.p1),
            (EntityGeom::Polyline { points, .. }, marker) if marker >= 0 => {
                points.get(marker as usize).copied()
            }
            (EntityGeom::Circle(c), -3) => Some(c.center),
            (EntityGeom::Arc(a), 0) => Some(a.start),
            (EntityGeom::Arc(a), 1) => Some(a.end),
            (EntityGeom::Arc(a), -3) => Some(a.circle.center),
            (EntityGeom::Ellipse(e), -3) => Some(e.center),
            (EntityGeom::Spline { curve, .. }, 0) if !curve.periodic => Some(curve.start),
            (EntityGeom::Spline { curve, .. }, 1) if !curve.periodic => Some(curve.end),
            _ => None,
        }
    }

    fn line_segment(&self, index: usize) -> Option<GLine> {
        let EntityGeom::Polyline {
            points,
            straight,
            closed,
            ..
        } = self
        else {
            return None;
        };
        if !straight.get(index).copied().unwrap_or(false) {
            return None;
        }
        let p1 = *points.get(index)?;
        let p2 = if index + 1 < points.len() {
            points[index + 1]
        } else if *closed {
            *points.first()?
        } else {
            return None;
        };
        Some(GLine { p1, p2 })
    }

    fn arc_segment(&self, index: usize) -> Option<PolylineArc> {
        let EntityGeom::Polyline { arcs, .. } = self else {
            return None;
        };
        arcs.get(index).copied().flatten()
    }
}

/// `Tangent`/`Normal` only ever care about "a whole circle-shaped curve" or
/// "a whole line" — an `Arc` collapses to its `.circle` here exactly like
/// `whole_circle` does, so those two constraints' own inline matches don't
/// need every Circle/Arc combination spelled out separately.
enum CircleOrLine {
    Circle(GCircle),
    Line(GLine),
    Ellipse(GEllipse),
    Other,
}

fn as_circle_or_line(g: EntityGeom, reference: ParametricRef) -> CircleOrLine {
    match g {
        EntityGeom::Circle(c) => CircleOrLine::Circle(c),
        EntityGeom::Arc(a) => CircleOrLine::Circle(a.circle),
        EntityGeom::Line(l)
        | EntityGeom::Ray(l)
        | EntityGeom::XLine(l)
        | EntityGeom::TextAxis(l) => CircleOrLine::Line(l),
        EntityGeom::Ellipse(ellipse) => CircleOrLine::Ellipse(ellipse),
        EntityGeom::Point(_) => CircleOrLine::Other,
        EntityGeom::Spline { .. } => CircleOrLine::Other,
        EntityGeom::Polyline { .. } => reference
            .segment_index()
            .and_then(|index| {
                g.line_segment(index)
                    .map(CircleOrLine::Line)
                    .or_else(|| {
                        g.arc_segment(index)
                            .map(|segment| CircleOrLine::Circle(segment.arc.circle))
                    })
            })
            .unwrap_or(CircleOrLine::Other),
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
    let entity = document.get_entity(handle)?;
    match entity {
        EntityType::Point(point) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(point.location.x, false),
            sys.add_param(point.location.y, false),
        ))),
        EntityType::Insert(insert) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(insert.insert_point.x, false),
            sys.add_param(insert.insert_point.y, false),
        ))),
        EntityType::Text(text) => {
            let p1 = GPoint::new(
                sys.add_param(text.insertion_point.x, false),
                sys.add_param(text.insertion_point.y, false),
            );
            let p2 = GPoint::new(
                sys.add_param(text.insertion_point.x + text.rotation.cos(), false),
                sys.add_param(text.insertion_point.y + text.rotation.sin(), false),
            );
            Some(EntityGeom::TextAxis(GLine { p1, p2 }))
        }
        EntityType::MText(text) => {
            let p1 = GPoint::new(
                sys.add_param(text.insertion_point.x, false),
                sys.add_param(text.insertion_point.y, false),
            );
            let p2 = GPoint::new(
                sys.add_param(text.insertion_point.x + text.rotation.cos(), false),
                sys.add_param(text.insertion_point.y + text.rotation.sin(), false),
            );
            Some(EntityGeom::TextAxis(GLine { p1, p2 }))
        }
        EntityType::AttributeDefinition(attribute) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(attribute.insertion_point.x, false),
            sys.add_param(attribute.insertion_point.y, false),
        ))),
        EntityType::AttributeEntity(attribute) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(attribute.insertion_point.x, false),
            sys.add_param(attribute.insertion_point.y, false),
        ))),
        EntityType::Table(table) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(table.insertion_point.x, false),
            sys.add_param(table.insertion_point.y, false),
        ))),
        EntityType::LwPolyline(polyline) => {
            let source = super::dimension_assoc::source_points(entity);
            let points: Vec<_> = source
                .iter()
                .map(|point| {
                    GPoint::new(sys.add_param(point.x, false), sys.add_param(point.y, false))
                })
                .collect();
            let arcs = (0..polyline.vertices.len())
                .map(|index| {
                    let next = if index + 1 < source.len() {
                        index + 1
                    } else if polyline.is_closed {
                        0
                    } else {
                        return None;
                    };
                    let bulge = polyline.vertices[index].bulge;
                    let shape = cadkernel::geom2d::BulgeArc::from_bulge(
                        [source[index].x, source[index].y],
                        [source[next].x, source[next].y],
                        bulge,
                    )?;
                    Some(PolylineArc {
                        arc: GArc {
                            circle: GCircle {
                                center: GPoint::new(
                                    sys.add_param(shape.center[0], false),
                                    sys.add_param(shape.center[1], false),
                                ),
                                rad: sys.add_param(shape.radius, false),
                            },
                            start: points[index],
                            end: points[next],
                            start_angle: sys.add_param(shape.start_angle, false),
                            end_angle: sys.add_param(shape.start_angle + shape.sweep, false),
                        },
                        sweep: shape.sweep,
                    })
                })
                .collect();
            Some(EntityGeom::Polyline {
                points: Rc::new(points),
                straight: Rc::new(
                    polyline
                        .vertices
                        .iter()
                        .map(|vertex| vertex.bulge.abs() <= 1e-9)
                        .collect(),
                ),
                arcs: Rc::new(arcs),
                closed: polyline.is_closed,
            })
        }
        EntityType::Polyline2D(polyline) => {
            let source = super::dimension_assoc::source_points(entity);
            let points: Vec<_> = source
                .iter()
                .map(|point| {
                    GPoint::new(sys.add_param(point.x, false), sys.add_param(point.y, false))
                })
                .collect();
            let arcs = (0..polyline.vertices.len())
                .map(|index| {
                    let next = if index + 1 < source.len() {
                        index + 1
                    } else if polyline.is_closed() {
                        0
                    } else {
                        return None;
                    };
                    let bulge = polyline.vertices[index].bulge;
                    let shape = cadkernel::geom2d::BulgeArc::from_bulge(
                        [source[index].x, source[index].y],
                        [source[next].x, source[next].y],
                        bulge,
                    )?;
                    Some(PolylineArc {
                        arc: GArc {
                            circle: GCircle {
                                center: GPoint::new(
                                    sys.add_param(shape.center[0], false),
                                    sys.add_param(shape.center[1], false),
                                ),
                                rad: sys.add_param(shape.radius, false),
                            },
                            start: points[index],
                            end: points[next],
                            start_angle: sys.add_param(shape.start_angle, false),
                            end_angle: sys.add_param(shape.start_angle + shape.sweep, false),
                        },
                        sweep: shape.sweep,
                    })
                })
                .collect();
            Some(EntityGeom::Polyline {
                points: Rc::new(points),
                straight: Rc::new(
                    polyline
                        .vertices
                        .iter()
                        .map(|vertex| vertex.bulge.abs() <= 1e-9)
                        .collect(),
                ),
                arcs: Rc::new(arcs),
                closed: polyline.is_closed(),
            })
        }
        EntityType::Line(l) => {
            let p1 = GPoint::new(
                sys.add_param(l.start.x, false),
                sys.add_param(l.start.y, false),
            );
            let p2 = GPoint::new(sys.add_param(l.end.x, false), sys.add_param(l.end.y, false));
            Some(EntityGeom::Line(GLine { p1, p2 }))
        }
        EntityType::Ray(ray) => {
            let p1 = GPoint::new(
                sys.add_param(ray.base_point.x, false),
                sys.add_param(ray.base_point.y, false),
            );
            let through = ray.base_point + ray.direction;
            let p2 = GPoint::new(
                sys.add_param(through.x, false),
                sys.add_param(through.y, false),
            );
            Some(EntityGeom::Ray(GLine { p1, p2 }))
        }
        EntityType::XLine(line) => {
            let p1 = GPoint::new(
                sys.add_param(line.base_point.x, false),
                sys.add_param(line.base_point.y, false),
            );
            let through = line.base_point + line.direction;
            let p2 = GPoint::new(
                sys.add_param(through.x, false),
                sys.add_param(through.y, false),
            );
            Some(EntityGeom::XLine(GLine { p1, p2 }))
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
            let end_angle = sys.add_param(
                a.start_angle + (a.end_angle - a.start_angle).rem_euclid(std::f64::consts::TAU),
                false,
            );
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
        EntityType::Spline(spline) => {
            let source = crate::entities::spline::nurbs3(spline)?;
            let poles: Vec<_> = source
                .control_points()
                .iter()
                .map(|point| {
                    GPoint::new(
                        sys.add_param(point[0], false),
                        sys.add_param(point[1], false),
                    )
                })
                .collect();
            let start = *poles.first()?;
            let end = *poles.last()?;
            let weights = source
                .weights()
                .iter()
                .map(|weight| sys.add_param(*weight, true))
                .collect();
            Some(EntityGeom::Spline {
                curve: GBSpline {
                    poles,
                    weights,
                    knots: source.knots().to_vec(),
                    start,
                    end,
                    degree: source.degree(),
                    periodic: source.periodicity(),
                },
                control_z: Rc::new(
                    source
                        .control_points()
                        .iter()
                        .map(|point| point[2])
                        .collect(),
                ),
            })
        }
        _ => None,
    }
}

/// Resolves one [`ParametricRef`] against the entity-geometry cache, registering
/// the entity on first use. `None` if the handle is dangling, isn't a
/// supported entity type, or (for a point ref) uses a marker that entity
/// type doesn't support.
fn resolve_ref(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    r: ParametricRef,
) -> Option<EntityGeom> {
    if let Some(geometry) = cache.get(&r.entity) {
        return Some(geometry.clone());
    }
    let geometry = register_entity(document, sys, r.entity)?;
    cache.insert(r.entity, geometry.clone());
    Some(geometry)
}

/// Resolves a point reference and installs the kernel equations for standard
/// implicit midpoints. Endpoint and center markers reuse entity parameters;
/// midpoint markers are derived parameters so moving either parent geometry
/// or another constrained object recomputes them in the same solve.
fn resolve_constraint_point(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    reference: ParametricRef,
) -> Option<GPoint> {
    let marker = reference.marker?;
    let geometry = resolve_ref(document, sys, cache, reference)?;
    if let Some(point) = geometry.point_for_marker(marker) {
        return Some(point);
    }

    let midpoint_line = reference
        .segment_midpoint_index()
        .and_then(|index| geometry.line_segment(index))
        .or_else(|| {
            (marker == -2)
                .then_some(&geometry)
                .and_then(|geometry| match geometry {
                    EntityGeom::Line(line) => Some(*line),
                    _ => None,
                })
        });
    if let Some(line) = midpoint_line {
        let (x1, y1, x2, y2) = {
            let store = sys.store();
            (
                store.get(line.p1.x),
                store.get(line.p1.y),
                store.get(line.p2.x),
                store.get(line.p2.y),
            )
        };
        let point = GPoint::new(
            sys.add_param((x1 + x2) * 0.5, false),
            sys.add_param((y1 + y2) * 0.5, false),
        );
        sys.add_constraint(Rc::new(CenterOfGravity::new(
            point.x,
            vec![line.p1.x, line.p2.x],
            vec![0.5, 0.5],
        )));
        sys.add_constraint(Rc::new(CenterOfGravity::new(
            point.y,
            vec![line.p1.y, line.p2.y],
            vec![0.5, 0.5],
        )));
        return Some(point);
    }
    if let Some(segment) = reference
        .segment_midpoint_index()
        .and_then(|index| geometry.arc_segment(index))
    {
        let (start, end) = {
            let store = sys.store();
            (
                store.get(segment.arc.start_angle),
                store.get(segment.arc.end_angle),
            )
        };
        let parameter_value = (start + end) * 0.5;
        let parameter = sys.add_param(parameter_value, false);
        sys.add_constraint(Rc::new(CenterOfGravity::new(
            parameter,
            vec![segment.arc.start_angle, segment.arc.end_angle],
            vec![0.5, 0.5],
        )));
        let value = cadkernel_constraints::geo::Curve::value(
            &segment.arc,
            sys.store(),
            parameter_value,
            0.0,
            None,
        );
        let point = GPoint::new(sys.add_param(value.x, false), sys.add_param(value.y, false));
        let curve = Rc::new(segment.arc);
        sys.add_constraint(Rc::new(CurveValue::new(
            point,
            point.x,
            curve.clone(),
            parameter,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(point, point.y, curve, parameter)));
        return Some(point);
    }
    if marker != -2 {
        return None;
    }

    match geometry {
        EntityGeom::Arc(arc) => {
            let (start, end) = {
                let store = sys.store();
                (store.get(arc.start_angle), store.get(arc.end_angle))
            };
            let parameter_value = if end < start {
                (start + end + std::f64::consts::TAU) * 0.5
            } else {
                (start + end) * 0.5
            };
            let end_parameter = if end < start {
                let adjusted = sys.add_param(end + std::f64::consts::TAU, false);
                let turn = sys.add_param(std::f64::consts::TAU, true);
                sys.add_constraint(Rc::new(Difference::new(arc.end_angle, adjusted, turn)));
                adjusted
            } else {
                arc.end_angle
            };
            let parameter = sys.add_param(parameter_value, false);
            sys.add_constraint(Rc::new(CenterOfGravity::new(
                parameter,
                vec![arc.start_angle, end_parameter],
                vec![0.5, 0.5],
            )));
            let point = {
                let value = cadkernel_constraints::geo::Curve::value(
                    &arc,
                    sys.store(),
                    parameter_value,
                    0.0,
                    None,
                );
                GPoint::new(sys.add_param(value.x, false), sys.add_param(value.y, false))
            };
            let curve = Rc::new(arc);
            sys.add_constraint(Rc::new(CurveValue::new(
                point,
                point.x,
                curve.clone(),
                parameter,
            )));
            sys.add_constraint(Rc::new(CurveValue::new(point, point.y, curve, parameter)));
            Some(point)
        }
        EntityGeom::Ellipse(ellipse) => {
            let EntityType::Ellipse(source) = document.get_entity(reference.entity)? else {
                return None;
            };
            if source.is_full() {
                return None;
            }
            let parameter_value = (source.start_parameter + source.end_parameter) * 0.5;
            let value = cadkernel_constraints::geo::Curve::value(
                &ellipse,
                sys.store(),
                parameter_value,
                0.0,
                None,
            );
            let point = GPoint::new(sys.add_param(value.x, false), sys.add_param(value.y, false));
            let parameter = sys.add_param(parameter_value, true);
            let curve = Rc::new(ellipse);
            sys.add_constraint(Rc::new(CurveValue::new(
                point,
                point.x,
                curve.clone(),
                parameter,
            )));
            sys.add_constraint(Rc::new(CurveValue::new(point, point.y, curve, parameter)));
            Some(point)
        }
        EntityGeom::Spline { curve, .. } if !curve.periodic => {
            let from = curve.knots[curve.degree];
            let to = curve.knots[curve.poles.len()];
            let parameter_value = (from + to) * 0.5;
            let value = cadkernel_constraints::geo::Curve::value(
                &curve,
                sys.store(),
                parameter_value,
                0.0,
                None,
            );
            let point = GPoint::new(sys.add_param(value.x, false), sys.add_param(value.y, false));
            let parameter = sys.add_param(parameter_value, true);
            sys.add_constraint(Rc::new(PointOnBSpline::new(
                point.x,
                parameter,
                SplineCoord::X,
                curve.clone(),
            )));
            sys.add_constraint(Rc::new(PointOnBSpline::new(
                point.y,
                parameter,
                SplineCoord::Y,
                curve,
            )));
            Some(point)
        }
        _ => None,
    }
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
        EntityType::Point(point) if is_world_z(point.normal) => point.location.z,
        EntityType::Insert(insert) if is_world_z(insert.normal) => insert.insert_point.z,
        EntityType::Text(text) if is_world_z(text.normal) => text.insertion_point.z,
        EntityType::MText(text) if is_world_z(text.normal) => text.insertion_point.z,
        EntityType::AttributeDefinition(attribute) if is_world_z(attribute.normal) => {
            attribute.insertion_point.z
        }
        EntityType::AttributeEntity(attribute) if is_world_z(attribute.normal) => {
            attribute.insertion_point.z
        }
        EntityType::Table(table) if is_world_z(table.normal) => table.insertion_point.z,
        EntityType::Line(line)
            if is_world_z(line.normal) && (line.start.z - line.end.z).abs() <= PLANE_EPS =>
        {
            line.start.z
        }
        EntityType::Ray(ray)
            if ray.direction.z.abs() <= PLANE_EPS && ray.base_point.z.is_finite() =>
        {
            ray.base_point.z
        }
        EntityType::XLine(line)
            if line.direction.z.abs() <= PLANE_EPS && line.base_point.z.is_finite() =>
        {
            line.base_point.z
        }
        EntityType::Circle(circle) if is_world_z(circle.normal) => circle.center.z,
        EntityType::Arc(arc) if is_world_z(arc.normal) => arc.center.z,
        EntityType::Ellipse(ellipse)
            if is_world_z(ellipse.normal) && ellipse.major_axis.z.abs() <= PLANE_EPS =>
        {
            ellipse.center.z
        }
        EntityType::Spline(spline) => {
            let curve = crate::entities::spline::nurbs3(spline)?;
            let first = *curve.control_points().first()?;
            if !curve
                .control_points()
                .iter()
                .all(|point| (point[2] - first[2]).abs() <= PLANE_EPS)
            {
                return None;
            }
            first[2]
        }
        EntityType::LwPolyline(polyline) if is_world_z(polyline.normal) => {
            let points = super::dimension_assoc::source_points(entity);
            let first = *points.first()?;
            if !points
                .iter()
                .all(|point| (point.z - first.z).abs() <= PLANE_EPS)
            {
                return None;
            }
            first.z
        }
        EntityType::Polyline2D(polyline) if is_world_z(polyline.normal) => {
            let points = super::dimension_assoc::source_points(entity);
            let first = *points.first()?;
            if !points
                .iter()
                .all(|point| (point.z - first.z).abs() <= PLANE_EPS)
            {
                return None;
            }
            first.z
        }
        _ => return None,
    };
    z.is_finite().then_some(z)
}

fn refs_share_supported_plane(document: &acadrust::CadDocument, refs: &[ParametricRef]) -> bool {
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

fn resolved_target(params: &ParameterTable, constraint: &ParametricConstraint) -> Option<f64> {
    let value = constraint.driving_param.as_ref()?.resolve(params).ok()?;
    if !value.is_finite() {
        return None;
    }
    let must_be_positive = matches!(
        constraint.kind,
        ConstraintKind::Distance | ConstraintKind::Radius | ConstraintKind::Diameter
    );
    (!must_be_positive || value > 0.0).then_some(value)
}

fn oriented_distance(target: f64, current_projection: f64) -> f64 {
    if target >= 0.0 && current_projection < 0.0 {
        -target
    } else {
        target
    }
}

fn retained_tangent_side(
    document: &acadrust::CadDocument,
    retained_before: &HashMap<Handle, std::sync::Arc<EntityType>>,
    constraint: &ParametricConstraint,
) -> Option<bool> {
    let [a, b] = constraint.refs.as_slice() else {
        return None;
    };
    if constraint.kind != ConstraintKind::Tangent
        || !retained_before.contains_key(&a.entity) && !retained_before.contains_key(&b.entity)
    {
        return None;
    }
    let curve = |reference: ParametricRef| {
        let entity = retained_before
            .get(&reference.entity)
            .map(|entity| entity.as_ref())
            .or_else(|| document.get_entity(reference.entity))?;
        let curve = crate::entities::curve::entity_curve_xy(entity)?;
        reference
            .segment_index()
            .map(|index| curve.segments().into_iter().nth(index))
            .unwrap_or(Some(curve))
    };
    let circle_center = |curve: &cadkernel::geom2d::Curve| match curve {
        cadkernel::geom2d::Curve::Circle(circle) => Some(circle.centre),
        cadkernel::geom2d::Curve::Arc(arc) => Some(arc.centre),
        _ => None,
    };
    let (a, b) = (curve(*a)?, curve(*b)?);
    let (line, center) = a
        .as_ray()
        .zip(circle_center(&b))
        .or_else(|| b.as_ray().zip(circle_center(&a)))?;
    let ([x1, y1], [dx, dy]) = line;
    Some(dx * (center[1] - y1) - dy * (center[0] - x1) >= 0.0)
}

/// Builds the kernel equations for one constraint. Unsupported references or
/// invalid dimensional targets produce no equations.
fn build_constraint(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    params: &ParameterTable,
    c: &ParametricConstraint,
    tangent_side: Option<bool>,
    tangent_point: Option<ParametricRef>,
) -> Vec<Rc<dyn Constraint>> {
    if !c.enabled || !refs_share_supported_plane(document, &c.refs) {
        return Vec::new();
    }

    let whole_line = |sys: &mut System, cache: &mut HashMap<_, _>, r: ParametricRef| {
        let geometry = resolve_ref(document, sys, cache, r)?;
        match geometry {
            EntityGeom::Line(line)
            | EntityGeom::Ray(line)
            | EntityGeom::XLine(line)
            | EntityGeom::TextAxis(line) => Some(line),
            EntityGeom::Polyline { .. } => geometry.line_segment(r.segment_index()?),
            _ => None,
        }
    };
    let bounded_line = |sys: &mut System, cache: &mut HashMap<_, _>, r: ParametricRef| {
        let geometry = resolve_ref(document, sys, cache, r)?;
        match geometry {
            EntityGeom::Line(line) => Some(line),
            EntityGeom::Polyline { .. } => geometry.line_segment(r.segment_index()?),
            _ => None,
        }
    };
    let whole_circle =
        |sys: &mut System, cache: &mut HashMap<_, _>, r: ParametricRef| match resolve_ref(
            document, sys, cache, r,
        )? {
            EntityGeom::Circle(circ) => Some(circ),
            EntityGeom::Arc(a) => Some(a.circle),
            EntityGeom::Point(_)
            | EntityGeom::TextAxis(_)
            | EntityGeom::Line(_)
            | EntityGeom::Ray(_)
            | EntityGeom::XLine(_)
            | EntityGeom::Polyline { .. }
            | EntityGeom::Ellipse(_)
            | EntityGeom::Spline { .. } => None,
        };
    let point_ref = |sys: &mut System, cache: &mut HashMap<_, _>, r: ParametricRef| {
        resolve_constraint_point(document, sys, cache, r)
    };

    let point_on_bounded_arc = |sys: &mut System, point: GPoint, arc: GArc| {
        let initial = BoundedArcValue::initial_parameter(sys.store(), point, arc);
        let parameter = sys.add_param(initial, false);
        vec![
            Rc::new(BoundedArcValue::new(point, point.x, arc, parameter)) as Rc<dyn Constraint>,
            Rc::new(BoundedArcValue::new(point, point.y, arc, parameter)) as Rc<dyn Constraint>,
        ]
    };

    // `Coincident`/`Concentric`/`CenterPoint` all solve identically — two
    // points (an endpoint, a circle's center via the existing `-3` marker,
    // or a plain point) held equal on both axes. Only the DWG-native class
    // name and the UI entry point that produces their `refs` differ.
    let point_pair_equal = |sys: &mut System,
                            cache: &mut HashMap<_, _>,
                            refs: &[ParametricRef]|
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
        ConstraintKind::Horizontal => match c.refs.as_slice() {
            [r] => whole_line(sys, cache, *r)
                .map(|line| {
                    vec![Rc::new(Equal::new(line.p1.y, line.p2.y, 1.0)) as Rc<dyn Constraint>]
                })
                .unwrap_or_default(),
            [a, b] => match (point_ref(sys, cache, *a), point_ref(sys, cache, *b)) {
                (Some(a), Some(b)) => vec![Rc::new(Equal::new(a.y, b.y, 1.0))],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
        ConstraintKind::Vertical => match c.refs.as_slice() {
            [r] => whole_line(sys, cache, *r)
                .map(|line| {
                    vec![Rc::new(Equal::new(line.p1.x, line.p2.x, 1.0)) as Rc<dyn Constraint>]
                })
                .unwrap_or_default(),
            [a, b] => match (point_ref(sys, cache, *a), point_ref(sys, cache, *b)) {
                (Some(a), Some(b)) => vec![Rc::new(Equal::new(a.x, b.x, 1.0))],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
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
            if let (Some(la), Some(lb)) =
                (bounded_line(sys, cache, *a), bounded_line(sys, cache, *b))
            {
                return vec![Rc::new(EqualLineLength::new(lb, la))];
            }
            if let (Some(EntityGeom::Ellipse(a)), Some(EntityGeom::Ellipse(b))) = (
                resolve_ref(document, sys, cache, *a),
                resolve_ref(document, sys, cache, *b),
            ) {
                return vec![Rc::new(EqualMajorAxesConic::new(
                    GConic::Ellipse(a),
                    GConic::Ellipse(b),
                ))];
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
            let axis = |sys: &mut System,
                        cache: &mut HashMap<Handle, EntityGeom>,
                        reference: ParametricRef|
             -> Option<(GLine, bool)> {
                let geometry = resolve_ref(document, sys, cache, reference)?;
                match geometry {
                    EntityGeom::Line(line)
                    | EntityGeom::Ray(line)
                    | EntityGeom::XLine(line)
                    | EntityGeom::TextAxis(line) => Some((line, false)),
                    EntityGeom::Polyline { .. } => {
                        Some((geometry.line_segment(reference.segment_index()?)?, false))
                    }
                    EntityGeom::Ellipse(ellipse) => Some((
                        GLine {
                            p1: ellipse.center,
                            p2: ellipse.focus1,
                        },
                        reference.is_ellipse_minor_axis(),
                    )),
                    _ => None,
                }
            };
            let (Some((onto_major, onto_minor)), Some((moving, moving_minor))) =
                (axis(sys, cache, *a), axis(sys, cache, *b))
            else {
                return Vec::new();
            };
            let onto = if onto_minor {
                let (cx, cy, fx, fy) = {
                    let store = sys.store();
                    (
                        store.get(onto_major.p1.x),
                        store.get(onto_major.p1.y),
                        store.get(onto_major.p2.x),
                        store.get(onto_major.p2.y),
                    )
                };
                GLine {
                    p1: onto_major.p1,
                    p2: GPoint::new(
                        sys.add_param(cx - (fy - cy), true),
                        sys.add_param(cy + (fx - cx), true),
                    ),
                }
            } else {
                onto_major
            };
            let direction_constraint: Rc<dyn Constraint> = if moving_minor {
                Rc::new(PerpendicularConstraint::new(sys.store(), moving, onto))
            } else {
                Rc::new(ParallelConstraint::new(sys.store(), moving, onto))
            };
            vec![
                Rc::new(PointOnLine::new(moving.p1, onto)),
                direction_constraint,
            ]
        }
        ConstraintKind::Midpoint => {
            let [a, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(p), Some(l)) = (point_ref(sys, cache, *a), bounded_line(sys, cache, *b))
            else {
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
                EntityGeom::Line(l)
                | EntityGeom::Ray(l)
                | EntityGeom::XLine(l)
                | EntityGeom::TextAxis(l) => {
                    vec![Rc::new(PointOnLine::new(p, l))]
                }
                EntityGeom::Circle(circ) => {
                    let zero = sys.add_param(0.0, true);
                    vec![Rc::new(P2CDistance::new(circ, p, zero))]
                }
                EntityGeom::Arc(a) => point_on_bounded_arc(sys, p, a),
                EntityGeom::Ellipse(ellipse) => {
                    vec![Rc::new(PointOnEllipse::new(p, ellipse))]
                }
                EntityGeom::Spline { curve, .. } => {
                    let (x, y) = {
                        let store = sys.store();
                        (store.get(p.x), store.get(p.y))
                    };
                    let Some(EntityType::Spline(source)) = document.get_entity(b.entity) else {
                        return Vec::new();
                    };
                    let Some(source_curve) = crate::entities::spline::nurbs3(source) else {
                        return Vec::new();
                    };
                    let Some(z) = source_curve.control_points().first().map(|point| point[2])
                    else {
                        return Vec::new();
                    };
                    let parameter = sys.add_param(source_curve.parameter_at([x, y, z]), false);
                    vec![
                        Rc::new(PointOnBSpline::new(
                            p.x,
                            parameter,
                            SplineCoord::X,
                            curve.clone(),
                        )),
                        Rc::new(PointOnBSpline::new(p.y, parameter, SplineCoord::Y, curve)),
                    ]
                }
                EntityGeom::Polyline { .. } => b
                    .segment_index()
                    .and_then(|index| {
                        geom.line_segment(index)
                            .map(|line| {
                                vec![Rc::new(PointOnLine::new(p, line)) as Rc<dyn Constraint>]
                            })
                            .or_else(|| {
                                geom.arc_segment(index)
                                    .map(|segment| point_on_bounded_arc(sys, p, segment.arc))
                            })
                    })
                    .unwrap_or_default(),
                EntityGeom::Point(_) => Vec::new(),
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
            if r.segment_index().is_some() {
                let Some(line) = whole_line(sys, cache, *r) else {
                    return Vec::new();
                };
                let (x1, y1, x2, y2) = {
                    let store = sys.store();
                    (
                        store.get(line.p1.x),
                        store.get(line.p1.y),
                        store.get(line.p2.x),
                        store.get(line.p2.y),
                    )
                };
                return vec![
                    Rc::new(Equal::new(line.p1.x, sys.add_param(x1, true), 1.0)),
                    Rc::new(Equal::new(line.p1.y, sys.add_param(y1, true), 1.0)),
                    Rc::new(Equal::new(line.p2.x, sys.add_param(x2, true), 1.0)),
                    Rc::new(Equal::new(line.p2.y, sys.add_param(y2, true), 1.0)),
                ];
            }
            if r.marker.is_some() {
                let Some(point) = point_ref(sys, cache, *r) else {
                    return Vec::new();
                };
                let (x, y) = {
                    let store = sys.store();
                    (store.get(point.x), store.get(point.y))
                };
                return vec![
                    Rc::new(Equal::new(point.x, sys.add_param(x, true), 1.0)),
                    Rc::new(Equal::new(point.y, sys.add_param(y, true), 1.0)),
                ];
            }
            let Some(geom) = resolve_ref(document, sys, cache, *r) else {
                return Vec::new();
            };
            match geom {
                EntityGeom::Point(point) => {
                    let (x, y) = {
                        let store = sys.store();
                        (store.get(point.x), store.get(point.y))
                    };
                    vec![
                        Rc::new(Equal::new(point.x, sys.add_param(x, true), 1.0)),
                        Rc::new(Equal::new(point.y, sys.add_param(y, true), 1.0)),
                    ]
                }
                EntityGeom::Polyline { points, .. } => {
                    let values: Vec<_> = points
                        .iter()
                        .map(|point| {
                            let store = sys.store();
                            (point, store.get(point.x), store.get(point.y))
                        })
                        .collect();
                    values
                        .into_iter()
                        .flat_map(|(point, x, y)| {
                            [
                                Rc::new(Equal::new(point.x, sys.add_param(x, true), 1.0))
                                    as Rc<dyn Constraint>,
                                Rc::new(Equal::new(point.y, sys.add_param(y, true), 1.0))
                                    as Rc<dyn Constraint>,
                            ]
                        })
                        .collect()
                }
                EntityGeom::Line(l)
                | EntityGeom::Ray(l)
                | EntityGeom::XLine(l)
                | EntityGeom::TextAxis(l) => {
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
                EntityGeom::Spline { curve, .. } => {
                    let values: Vec<_> = curve
                        .poles
                        .iter()
                        .map(|point| {
                            let store = sys.store();
                            (point, store.get(point.x), store.get(point.y))
                        })
                        .collect();
                    values
                        .into_iter()
                        .flat_map(|(point, x, y)| {
                            [
                                Rc::new(Equal::new(point.x, sys.add_param(x, true), 1.0))
                                    as Rc<dyn Constraint>,
                                Rc::new(Equal::new(point.y, sys.add_param(y, true), 1.0))
                                    as Rc<dyn Constraint>,
                            ]
                        })
                        .collect()
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
        ConstraintKind::DistanceDirected => {
            let Some((&a, rest)) = c.refs.split_first() else {
                return Vec::new();
            };
            let Some((&b, direction_ref)) = rest.split_first() else {
                return Vec::new();
            };
            let (Some(pa), Some(pb)) = (point_ref(sys, cache, a), point_ref(sys, cache, b)) else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            if matches!(
                c.distance_direction_type,
                distance_direction_type::PARALLEL_TO_LINE
                    | distance_direction_type::PERPENDICULAR_TO_LINE
            ) {
                if let Some(line) = direction_ref
                    .first()
                    .and_then(|reference| whole_line(sys, cache, *reference))
                {
                    let current = {
                        let store = sys.store();
                        let delta = [
                            store.get(pb.x) - store.get(pa.x),
                            store.get(pb.y) - store.get(pa.y),
                        ];
                        let axis = [
                            store.get(line.p2.x) - store.get(line.p1.x),
                            store.get(line.p2.y) - store.get(line.p1.y),
                        ];
                        let length = axis[0].hypot(axis[1]);
                        if length <= f64::EPSILON {
                            0.0
                        } else if c.distance_direction_type
                            == distance_direction_type::PERPENDICULAR_TO_LINE
                        {
                            delta[0] * (-axis[1] / length) + delta[1] * (axis[0] / length)
                        } else {
                            delta[0] * (axis[0] / length) + delta[1] * (axis[1] / length)
                        }
                    };
                    let target = sys.add_param(oriented_distance(resolved, current), true);
                    return vec![Rc::new(ProjectedDistanceAlongLine::new(
                        pa,
                        pb,
                        target,
                        line,
                        c.distance_direction_type == distance_direction_type::PERPENDICULAR_TO_LINE,
                    ))];
                }
            }
            let Some(direction) = c.distance_direction else {
                return Vec::new();
            };
            let current = {
                let store = sys.store();
                let length = direction.x.hypot(direction.y);
                if length <= f64::EPSILON {
                    0.0
                } else {
                    (store.get(pb.x) - store.get(pa.x)) * direction.x / length
                        + (store.get(pb.y) - store.get(pa.y)) * direction.y / length
                }
            };
            let target = sys.add_param(oriented_distance(resolved, current), true);
            ProjectedDistance::new(pa, pb, target, [direction.x, direction.y])
                .map(|constraint| vec![Rc::new(constraint) as Rc<dyn Constraint>])
                .unwrap_or_default()
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
            let angle = resolved.to_radians();
            let angle = match c.angle_sector {
                angle_sector::ANTIPARALLEL_CLOCKWISE => std::f64::consts::PI - angle,
                angle_sector::PARALLEL_CLOCKWISE => -angle,
                angle_sector::ANTIPARALLEL_COUNTERCLOCKWISE => std::f64::consts::PI + angle,
                _ => angle,
            };
            let angle = sys.add_param(angle, true);
            vec![Rc::new(L2LAngle::new(fixed, moving, angle))]
        }
        ConstraintKind::Angle3Point => {
            let [a, vertex, b] = c.refs.as_slice() else {
                return Vec::new();
            };
            let (Some(a), Some(vertex), Some(b)) = (
                point_ref(sys, cache, *a),
                point_ref(sys, cache, *vertex),
                point_ref(sys, cache, *b),
            ) else {
                return Vec::new();
            };
            let Some(resolved) = resolved_target(params, c) else {
                return Vec::new();
            };
            let angle = resolved.to_radians();
            let angle = match c.angle_sector {
                angle_sector::ANTIPARALLEL_CLOCKWISE => std::f64::consts::PI - angle,
                angle_sector::PARALLEL_CLOCKWISE => -angle,
                angle_sector::ANTIPARALLEL_COUNTERCLOCKWISE => std::f64::consts::PI + angle,
                _ => angle,
            };
            let angle = sys.add_param(angle, true);
            vec![Rc::new(L2LAngle::new(
                GLine { p1: vertex, p2: a },
                GLine { p1: vertex, p2: b },
                angle,
            ))]
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
        // X-only/Y-only distance between two points. Positive standard
        // dimension values are magnitudes, so retain the geometry's current
        // orientation; an explicitly negative command value remains signed.
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
            let current = {
                let store = sys.store();
                store.get(pb.x) - store.get(pa.x)
            };
            let target = sys.add_param(oriented_distance(resolved, current), true);
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
            let current = {
                let store = sys.store();
                store.get(pb.y) - store.get(pa.y)
            };
            let target = sys.add_param(oriented_distance(resolved, current), true);
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
            match (as_circle_or_line(ga, *a), as_circle_or_line(gb, *b)) {
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
                    if let Some(point) = tangent_point.and_then(|reference|
                        resolve_constraint_point(document, sys, cache, reference))
                    {
                        // At a coincident endpoint, constrain the tangent direction
                        // directly; a circle-to-line distance has a singular derivative.
                        let radius = GLine { p1: circ.center, p2: point };
                        return vec![Rc::new(PerpendicularConstraint::new(sys.store(), line, radius))];
                    }
                    let store = sys.store();
                    let (cx, cy) = (store.get(circ.center.x), store.get(circ.center.y));
                    let (x1, y1) = (store.get(line.p1.x), store.get(line.p1.y));
                    let (x2, y2) = (store.get(line.p2.x), store.get(line.p2.y));
                    // Signed area of (p2-p1) × (center-p1) — same formula
                    // `C2LDistance::signed_value` itself uses — so `ccw`'s
                    // sign matches whichever side the circle already sits on.
                    let area = (x2 - x1) * (cy - y1) - (y2 - y1) * (cx - x1);
                    let ccw = tangent_side.unwrap_or(area >= 0.0);
                    // A driven, fixed zero: with `internal: false` this makes
                    // `C2LDistance`'s target exactly the circle's own radius
                    // (see its `error_grad`), i.e. plain tangency rather than
                    // an offset distance.
                    let zero = sys.add_param(0.0, true);
                    vec![Rc::new(C2LDistance::new(circ, line, zero, ccw, false))]
                }
                (CircleOrLine::Ellipse(ellipse), CircleOrLine::Line(line))
                | (CircleOrLine::Line(line), CircleOrLine::Ellipse(ellipse)) => {
                    vec![Rc::new(TangentEllipseLine::new(line, ellipse))]
                }
                _ => Vec::new(),
            }
        }
        // Smooth edits the endpoint controls through cadkernel's spatial
        // NURBS operation after the ordinary 2D system has settled.
        ConstraintKind::Smooth => Vec::new(),
        ConstraintKind::RigidSet => {
            let points: Vec<_> = c
                .rigid_points
                .iter()
                .filter_map(|(reference, original)| {
                    point_ref(sys, cache, *reference).map(|point| (point, *original))
                })
                .collect();
            let Some(second) = (1..points.len()).find(|&index| {
                let delta = points[index].1 - points[0].1;
                delta.x * delta.x + delta.y * delta.y > 1.0e-18
            }) else {
                return Vec::new();
            };
            let mut pairs = vec![(0, second)];
            for index in 1..points.len() {
                if index == second {
                    continue;
                }
                pairs.push((0, index));
                pairs.push((second, index));
            }
            pairs
                .into_iter()
                .map(|(a, b)| {
                    let delta = points[b].1 - points[a].1;
                    let target =
                        sys.add_param((delta.x * delta.x + delta.y * delta.y).sqrt(), true);
                    Rc::new(P2PDistance::new(points[a].0, points[b].0, target))
                        as Rc<dyn Constraint>
                })
                .collect()
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
            match (as_circle_or_line(ga, *a), as_circle_or_line(gb, *b)) {
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
    }
}

fn smooth_target_jet(entity: &EntityType, marker: i32) -> Option<cadkernel::space::CurveJet> {
    let parameter = match marker {
        0 => 0.0,
        1 => 1.0,
        _ => return None,
    };
    let curve = crate::entities::curve::entity_curve(entity)?;
    if curve.is_closed() {
        return None;
    }
    let step = if marker == 0 { 1.0e-4 } else { -1.0e-4 };
    let point = curve.point_at(parameter);
    let tangent = curve.tangent_at(parameter);
    let next = curve.point_at(parameter + step);
    let third = curve.point_at(parameter + 2.0 * step);
    Some(cadkernel::space::CurveJet {
        point,
        tangent,
        curvature: cadkernel::space::curve::curvature_through(point, next, third),
    })
}

fn apply_smooth_constraints(
    document: &acadrust::CadDocument,
    set: &ParametricConstraintSet,
    results: &mut Vec<(Handle, EntityType)>,
) {
    for constraint in set
        .constraints
        .iter()
        .filter(|constraint| constraint.enabled && constraint.kind == ConstraintKind::Smooth)
    {
        let [source_ref, target_ref] = constraint.refs.as_slice() else {
            continue;
        };
        let source = results
            .iter()
            .find(|(handle, _)| *handle == source_ref.entity)
            .map(|(_, entity)| entity.clone())
            .or_else(|| document.get_entity(source_ref.entity).cloned());
        let target = results
            .iter()
            .find(|(handle, _)| *handle == target_ref.entity)
            .map(|(_, entity)| entity.clone())
            .or_else(|| document.get_entity(target_ref.entity).cloned());
        let (Some(EntityType::Spline(mut spline)), Some(target)) = (source, target) else {
            continue;
        };
        let endpoint = match source_ref.marker {
            Some(0) => cadkernel::space::SplineEnd::Start,
            Some(1) => cadkernel::space::SplineEnd::End,
            _ => continue,
        };
        let Some(target_marker) = target_ref.marker else {
            continue;
        };
        let Some(target_jet) = smooth_target_jet(&target, target_marker) else {
            continue;
        };
        let Some(curve) = crate::entities::spline::nurbs3(&spline) else {
            continue;
        };
        let Some(smoothed) = cadkernel::space::smooth_nurbs_endpoint(&curve, endpoint, target_jet)
        else {
            continue;
        };
        crate::entities::spline::replace_with_nurbs(&mut spline, &smoothed);
        let updated = EntityType::Spline(spline);
        if let Some((_, entity)) = results
            .iter_mut()
            .find(|(handle, _)| *handle == source_ref.entity)
        {
            *entity = updated;
        } else {
            results.push((source_ref.entity, updated));
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

fn retained_line_length(entity: &EntityType, segment: Option<usize>) -> Option<f64> {
    match (entity, segment) {
        (EntityType::Line(line), None) => Some((line.end - line.start).length()),
        (EntityType::LwPolyline(polyline), Some(index)) => {
            let world = crate::entities::curve::lwpolyline_world_xy(polyline)?;
            let a = world.vertices.get(index)?.location;
            let b = if let Some(vertex) = world.vertices.get(index + 1) {
                vertex.location
            } else if world.is_closed {
                world.vertices.first()?.location
            } else {
                return None;
            };
            Some((b - a).length())
        }
        (EntityType::Polyline2D(polyline), Some(index)) => {
            let a = polyline.vertices.get(index)?.location;
            let b = if let Some(vertex) = polyline.vertices.get(index + 1) {
                vertex.location
            } else if polyline.is_closed() {
                polyline.vertices.first()?.location
            } else {
                return None;
            };
            Some((b - a).length())
        }
        _ => None,
    }
}

fn retained_radius(entity: &EntityType) -> Option<f64> {
    match entity {
        EntityType::Circle(circle) => Some(circle.radius),
        EntityType::Arc(arc) => Some(arc.radius),
        _ => None,
    }
}

/// Propagate explicit axis constraints through parallel/perpendicular relations.
fn constrained_line_axes(set: &ParametricConstraintSet) -> HashMap<ParametricRef, bool> {
    let mut axes = HashMap::new();
    for c in set.constraints.iter().filter(|c| c.enabled) {
        if let [reference] = c.refs.as_slice() {
            if matches!(c.kind, ConstraintKind::Horizontal | ConstraintKind::Vertical) {
                axes.insert(*reference, c.kind == ConstraintKind::Vertical);
            }
        }
    }
    loop {
        let before = axes.len();
        for c in set.constraints.iter().filter(|c| c.enabled) {
            if !matches!(c.kind, ConstraintKind::Parallel | ConstraintKind::Perpendicular | ConstraintKind::Colinear) {
                continue;
            }
            let [a, b] = c.refs.as_slice() else { continue };
            let perpendicular = c.kind == ConstraintKind::Perpendicular;
            if let Some(axis) = axes.get(a).copied() {
                axes.entry(*b).or_insert(axis ^ perpendicular);
            }
            if let Some(axis) = axes.get(b).copied() {
                axes.entry(*a).or_insert(axis ^ perpendicular);
            }
        }
        if before == axes.len() {
            return axes;
        }
    }
}

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
    drawing_params: &ParameterTable,
    set: &ParametricConstraintSet,
    driven_refs: &[ParametricRef],
    retain_size: bool,
    retain_lengths: bool,
    retained_before: &HashMap<Handle, std::sync::Arc<EntityType>>,
) -> Option<SolveResult> {
    let params = if set.local_parameters.is_empty() {
        drawing_params
    } else {
        &set.local_parameters
    };
    let mut sys = System::new();
    let mut cache: HashMap<Handle, EntityGeom> = HashMap::new();
    let line_axes = if retain_lengths { HashMap::new() } else { constrained_line_axes(set) };
    // Tracks which system-level `cadkernel_constraints` constraint(s) came from which
    // `ParametricConstraint` — a `SubSystem`'s redundant-row indices (from
    // `cadkernel_constraints::diagnosis`) are local to that partition's own constraint
    // list, not `set.constraints`' indices, and several kinds contribute
    // more than one system-level constraint per `ParametricConstraint` (see
    // `build_constraint`'s doc comment). `Rc::ptr_eq` against this after
    // partitioning resolves a row back to the `ConstraintId` the UI
    // actually names.
    let mut owner: Vec<(Rc<dyn Constraint>, ConstraintId)> = Vec::new();

    for c in &set.constraints {
        if c.enabled && matches!(c.kind, ConstraintKind::Parallel | ConstraintKind::Perpendicular)
            && c.refs.len() == 2 && c.refs.iter().all(|r| line_axes.contains_key(r))
            && (line_axes[&c.refs[0]] ^ line_axes[&c.refs[1]]) == (c.kind == ConstraintKind::Perpendicular)
            && refs_share_supported_plane(document, &c.refs)
            && c.refs.iter().all(|reference| {
                match resolve_ref(document, &mut sys, &mut cache, *reference) {
                    Some(EntityGeom::Line(_)) => true,
                    Some(geometry @ EntityGeom::Polyline { .. }) => reference.segment_index()
                        .and_then(|index| geometry.line_segment(index)).is_some(),
                    _ => false,
                }
            })
        {
            // Implied axis equations below replace normalized angle equations,
            // which become ill-conditioned when a dragged edge crosses zero.
            continue;
        }
        let tangent_side = retained_tangent_side(document, retained_before, c);
        let tangent_point = (c.kind == ConstraintKind::Tangent && c.refs.len() == 2
            && c.refs[0].entity != c.refs[1].entity
            && c.refs.iter().all(|r| r.marker.is_none()))
            .then(|| set.constraints.iter().find_map(|connection| {
                (connection.enabled && connection.kind == ConstraintKind::Coincident
                    && connection.refs.len() == 2
                    && connection.refs.iter().all(|r| matches!(r.marker, Some(0 | 1)))
                    && c.refs.iter().all(|r| connection.refs.iter().any(|p| p.entity == r.entity)))
                    .then(|| connection.refs[0])
            })).flatten();
        for constraint in build_constraint(
            document,
            &mut sys,
            &mut cache,
            params,
            c,
            tangent_side,
            tangent_point,
        ) {
            owner.push((constraint.clone(), c.id));
            sys.add_constraint(constraint);
        }
    }

    let has_smooth = set
        .constraints
        .iter()
        .any(|constraint| constraint.enabled && constraint.kind == ConstraintKind::Smooth);
    if cache.is_empty() && !has_smooth {
        return None;
    }

    let mut anchors = driven_refs.to_vec();
    let mut released_driver_params = HashSet::new();
    if !retain_lengths {
        let mut axis_lines: Vec<_> = line_axes.into_iter().filter_map(|(reference, vertical)| {
            let geometry = cache.get(&reference.entity)?;
            let line = match geometry {
                EntityGeom::Line(line) => *line,
                EntityGeom::Polyline { .. } => geometry.line_segment(reference.segment_index()?)?,
                _ => return None,
            };
            Some((reference, line, vertical))
        }).collect();
        axis_lines.sort_by_key(|(reference, _, _)| (reference.entity.value(), reference.marker));
        let driven_points: Vec<_> = driven_refs.iter().filter_map(|reference|
            resolve_constraint_point(document, &mut sys, &mut cache, *reference)
                .map(|point| (*reference, point))).collect();
        let coincident: Vec<_> = set.constraints.iter().filter(|c|
            c.enabled && c.kind == ConstraintKind::Coincident).filter_map(|c| {
                let [a, b] = c.refs.as_slice() else { return None };
                Some((resolve_constraint_point(document, &mut sys, &mut cache, *a)?,
                    resolve_constraint_point(document, &mut sys, &mut cache, *b)?))
            }).collect();
        let point_group = |point| {
            let mut points = vec![point];
            let mut cursor = 0;
            while cursor < points.len() {
                for (a, b) in &coincident {
                    let next = if points[cursor] == *a { Some(*b) }
                        else if points[cursor] == *b { Some(*a) } else { None };
                    if let Some(next) = next.filter(|next| !points.contains(next)) {
                        points.push(next);
                    }
                }
                cursor += 1;
            }
            points
        };
        let perpendicular: Vec<_> = set.constraints.iter().filter(|c|
            c.enabled && c.kind == ConstraintKind::Perpendicular && c.refs.len() == 2).collect();
        let explicit_axis_directions: HashSet<_> = set.constraints.iter().filter_map(|constraint| {
            if !constraint.enabled || constraint.refs.len() != 1 {
                return None;
            }
            match constraint.kind {
                ConstraintKind::Horizontal => Some(false),
                ConstraintKind::Vertical => Some(true),
                _ => None,
            }
        }).collect();
        let mut retained_axes = HashSet::new();
        for (driven_reference, point) in &driven_points {
            let group = point_group(*point);
            let incident: Vec<_> = axis_lines.iter().filter(|(_, line, _)|
                group.contains(&line.p1) || group.contains(&line.p2)).collect();
            if incident.is_empty() {
                continue;
            }
            let common_corner = perpendicular.iter().any(|c| c.refs.iter()
                .all(|reference| incident.iter().any(|(r, _, _)| r == reference)));
            if common_corner && retain_size {
                // The shared right-angle corner carries its two legs together.
                retained_axes.extend([false, true]);
                continue;
            }
            let free_legs: Vec<_> = incident.iter().copied().filter(|(reference, _, _)|
                perpendicular.iter().any(|c| c.refs.contains(reference))).collect();
            if free_legs.is_empty() && explicit_axis_directions.len() == 1 {
                let explicit_axis = *explicit_axis_directions.iter().next().unwrap();
                let opposite_leg = incident.iter().copied()
                    .find(|(_, _, vertical)| *vertical != explicit_axis);
                if let Some((reference, line, vertical)) = opposite_leg {
                    let rectangle_diagonal_index = (reference.entity == driven_reference.entity)
                        .then(|| retained_before.get(&reference.entity))
                        .flatten()
                        .filter(|entity| {
                            matches!(entity.as_ref(), EntityType::LwPolyline(polyline)
                                if polyline.is_closed && polyline.vertices.len() == 4)
                                || matches!(entity.as_ref(), EntityType::Polyline2D(polyline)
                                    if polyline.is_closed() && polyline.vertices.len() == 4)
                        })
                        .and_then(|_| driven_reference.marker)
                        .and_then(|marker| usize::try_from(marker).ok())
                        .filter(|index| *index < 4)
                        .map(|index| (index + 2) % 4);
                    if let Some(diagonal_index) = rectangle_diagonal_index {
                        let diagonal_reference = ParametricRef::point(
                            driven_reference.entity,
                            diagonal_index as i32,
                        );
                        if let Some(diagonal) = resolve_constraint_point(
                            document,
                            &mut sys,
                            &mut cache,
                            diagonal_reference,
                        ) {
                            if let Some(original_point) = retained_before
                                .get(&driven_reference.entity)
                                .and_then(|entity| {
                                    super::dimension_assoc::source_points(entity)
                                        .get(diagonal_index)
                                        .copied()
                                })
                            {
                                sys.store_mut().set(diagonal.x, original_point.x);
                                sys.store_mut().set(diagonal.y, original_point.y);
                                sys.store_mut().set_driven(diagonal.x, true);
                                sys.store_mut().set_driven(diagonal.y, true);
                                // The diagonal corner is the sole anchor. The
                                // grabbed point retains both cursor coordinates,
                                // and the remaining two corners follow the axis
                                // and parallel relations.
                                continue;
                            }
                        }
                    }
                    let opposite = if group.contains(&line.p1) { line.p2 } else { line.p1 };
                    if let Some(original) = retained_before.get(&reference.entity) {
                        let points = super::dimension_assoc::source_points(original);
                        let start = reference.segment_index().unwrap_or(0);
                        let index = if opposite == line.p1 {
                            start
                        } else {
                            (start + 1) % points.len().max(1)
                        };
                        if let Some(original_point) = points.get(index) {
                            sys.store_mut().set(opposite.x, original_point.x);
                            sys.store_mut().set(opposite.y, original_point.y);
                        }
                    }
                    sys.store_mut().set_driven(opposite.x, true);
                    sys.store_mut().set_driven(opposite.y, true);
                    // This is the corner opposite the explicit right-angle
                    // corner. Its far endpoint is the sole temporary anchor.
                    // Release the dragged point's normal coordinate so the
                    // axis relation can keep that far endpoint fully fixed.
                    released_driver_params.insert(if *vertical { point.x } else { point.y });
                    continue;
                }
            }
            // When one explicit Horizontal/Vertical direction orients the
            // component, the leg parallel to it is the corner's stretch leg.
            // The perpendicular leg keeps its original length. This also
            // distinguishes the two corners adjacent to the constrained
            // right-angle corner without relying on polyline vertex numbers.
            let preferred_axis_legs: Vec<_> = if free_legs.len() == 1
                && explicit_axis_directions.len() == 1
            {
                let axis = *explicit_axis_directions.iter().next().unwrap();
                incident.iter().copied().filter(|(_, _, vertical)| *vertical == axis).collect()
            } else {
                Vec::new()
            };
            let editable = if !preferred_axis_legs.is_empty() {
                &preferred_axis_legs
            } else if free_legs.is_empty() {
                &incident
            } else {
                &free_legs
            };
            retained_axes.extend([false, true].into_iter().filter(|axis|
                !editable.iter().any(|(_, _, vertical)| vertical == axis)));
            for (reference, line, vertical) in editable.iter().copied() {
                let opposite = if group.contains(&line.p1) { line.p2 } else { line.p1 };
                if !driven_points.iter().any(|(_, point)| *point == opposite) {
                    let parameter = if *vertical { opposite.y } else { opposite.x };
                    // Only the along-edge coordinate stays fixed. The normal
                    // coordinate follows the grabbed point through the kernel.
                    if let Some(original) = retained_before.get(&reference.entity) {
                        let points = super::dimension_assoc::source_points(original);
                        let start = reference.segment_index().unwrap_or(0);
                        let index = if opposite == line.p1 { start } else { (start + 1) % points.len().max(1) };
                        if let Some(point) = points.get(index) {
                            sys.store_mut().set(parameter, if *vertical { point.y } else { point.x });
                        }
                    }
                    sys.store_mut().set_driven(parameter, true);
                }
            }
        }
        for (reference, line, vertical) in &axis_lines {
            // These implied linear equations remain defined even while an
            // edge crosses zero length; cross-product equations do not.
            let (a, b) = if *vertical { (line.p1.x, line.p2.x) } else { (line.p1.y, line.p2.y) };
            sys.add_constraint(Rc::new(Equal::new(a, b, 1.0)));
            let paired_edge_is_dragged = set.constraints.iter().any(|c| {
                c.enabled && c.kind == ConstraintKind::Parallel && c.refs.contains(reference)
                    && c.refs.iter().any(|other| other != reference && driven_refs.contains(other))
            });
            if paired_edge_is_dragged && !driven_refs.contains(reference) {
                // Moving a whole edge changes its separation from the opposite
                // parallel edge, whose normal coordinate remains the reference.
                if let Some(original) = retained_before.get(&reference.entity) {
                    let points = super::dimension_assoc::source_points(original);
                    if let Some(point) = points.get(reference.segment_index().unwrap_or(0)) {
                        sys.store_mut().set(a, if *vertical { point.x } else { point.y });
                        sys.store_mut().set_driven(a, true);
                    }
                }
            }
            if retain_size && retained_axes.contains(vertical) {
                let Some(original) = retained_before.get(&reference.entity) else { continue };
                let points = super::dimension_assoc::source_points(original);
                let index = reference.segment_index().unwrap_or(0);
                let (Some(a), Some(b)) = (points.get(index), points.get(index + 1).or_else(|| points.first()))
                    else { continue };
                let (a_param, b_param, delta) = if *vertical {
                    (line.p1.y, line.p2.y, b.y - a.y)
                } else {
                    (line.p1.x, line.p2.x, b.x - a.x)
                };
                let target = sys.add_param(delta, true);
                sys.add_constraint(Rc::new(Difference::new(a_param, b_param, target)));
            }
        }
        for reference in driven_refs {
            if matches!(document.get_entity(reference.entity), Some(EntityType::Line(_)))
                && matches!(reference.marker, Some(0 | 1))
                && !axis_lines
                    .iter()
                    .any(|(line_ref, _, _)| line_ref.entity == reference.entity)
            {
                anchors.push(ParametricRef::point(
                    reference.entity,
                    1 - reference.marker.unwrap(),
                ));
            }
        }
    }

    // Size-preserving grip edits retain radii while connected lines resize to
    // meet their endpoints. Shape grips leave unconstrained radii to the
    // parametric network. Non-preview edits can retain line lengths as well.
    if retain_size {
        for (handle, geom) in &cache {
            match geom {
                EntityGeom::Line(line) if retain_lengths => {
                    let current_length = {
                        let store = sys.store();
                        let dx = store.get(line.p2.x) - store.get(line.p1.x);
                        let dy = store.get(line.p2.y) - store.get(line.p1.y);
                        (dx * dx + dy * dy).sqrt()
                    };
                    let length = retained_before
                        .get(handle)
                        .and_then(|entity| retained_line_length(entity, None))
                        .unwrap_or(current_length);
                    let target = sys.add_param(length, true);
                    sys.add_constraint(Rc::new(P2PDistance::new(line.p1, line.p2, target)));
                }
                EntityGeom::Polyline {
                    points,
                    closed,
                    ..
                } if retain_lengths => {
                    for index in 0..points.len() {
                        let Some(a) = points.get(index).copied() else {
                            continue;
                        };
                        let b = if let Some(point) = points.get(index + 1).copied() {
                            Some(point)
                        } else if *closed {
                            points.first().copied()
                        } else {
                            None
                        };
                        let Some(b) = b else { continue };
                        let current_length = {
                            let store = sys.store();
                            let dx = store.get(b.x) - store.get(a.x);
                            let dy = store.get(b.y) - store.get(a.y);
                            (dx * dx + dy * dy).sqrt()
                        };
                        let length = retained_before
                            .get(handle)
                            .and_then(|entity| retained_line_length(entity, Some(index)))
                            .unwrap_or(current_length);
                        let target = sys.add_param(length, true);
                        sys.add_constraint(Rc::new(P2PDistance::new(a, b, target)));
                    }
                }
                EntityGeom::Circle(circle) => {
                    let radius = retained_before
                        .get(handle)
                        .and_then(|entity| retained_radius(entity))
                        .unwrap_or_else(|| sys.store().get(circle.rad));
                    let target = sys.add_param(radius, true);
                    sys.add_constraint(Rc::new(Equal::new(circle.rad, target, 1.0)));
                }
                EntityGeom::Arc(arc) => {
                    let radius = retained_before
                        .get(handle)
                        .and_then(|entity| retained_radius(entity))
                        .unwrap_or_else(|| sys.store().get(arc.circle.rad));
                    let target = sys.add_param(radius, true);
                    sys.add_constraint(Rc::new(Equal::new(arc.circle.rad, target, 1.0)));
                }
                _ => {}
            }
        }
    }

    // Arc rules (this module's doc comment): keep every registered arc's
    // `start`/`end` points consistent with its center/radius/angle,
    // unconditionally — the kernel adds these the moment an arc
    // exists, not only when some `ParametricConstraint` happens to reference
    // its endpoint. Not tracked in `owner`: these are solver-internal
    // bookkeeping, never redundant with anything a user-facing constraint
    // could name, so there's no `ConstraintId` for them to report against.
    let add_arc_rules = |sys: &mut System, arc: GArc, sweep: Option<f64>| {
        let curve = Rc::new(arc);
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
        if let Some(sweep) = sweep {
            let target = sys.add_param(sweep, true);
            sys.add_constraint(Rc::new(Difference::new(
                arc.start_angle,
                arc.end_angle,
                target,
            )));
        }
    };
    for geom in cache.values() {
        match geom {
            EntityGeom::Arc(arc) => add_arc_rules(&mut sys, *arc, None),
            EntityGeom::Polyline { arcs, .. } => {
                for segment in arcs.iter().flatten() {
                    add_arc_rules(&mut sys, segment.arc, Some(segment.sweep));
                }
            }
            _ => {}
        }
    }

    // Text is a point plus a unit baseline. Point constraints must translate
    // that baseline without changing its rotation; Collinear is the one
    // geometric relation that intentionally releases the baseline angle.
    for (handle, geom) in &cache {
        let EntityGeom::TextAxis(line) = geom else {
            continue;
        };
        let (x1, y1, x2, y2) = {
            let store = sys.store();
            (
                store.get(line.p1.x),
                store.get(line.p1.y),
                store.get(line.p2.x),
                store.get(line.p2.y),
            )
        };
        let rotates_for_collinear = set.constraints.iter().any(|constraint| {
            constraint.enabled
                && constraint.kind == ConstraintKind::Colinear
                && constraint.refs.iter().any(|reference| reference.entity == *handle)
        });
        if rotates_for_collinear {
            let length = sys.add_param((x2 - x1).hypot(y2 - y1), true);
            sys.add_constraint(Rc::new(P2PDistance::new(line.p1, line.p2, length)));
        } else {
            let dx = sys.add_param(x2 - x1, true);
            let dy = sys.add_param(y2 - y1, true);
            sys.add_constraint(Rc::new(Difference::new(line.p1.x, line.p2.x, dx)));
            sys.add_constraint(Rc::new(Difference::new(line.p1.y, line.p2.y, dy)));
        }
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
    for (handle, geom) in &cache {
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
        let rotates_for_collinear = set.constraints.iter().any(|constraint| {
            constraint.enabled
                && constraint.kind == ConstraintKind::Colinear
                && constraint.refs.iter().any(|reference| reference.entity == *handle)
        });
        if rotates_for_collinear {
            let distance = sys.add_param((fx - cx).hypot(fy - cy), true);
            sys.add_constraint(Rc::new(P2PDistance::new(el.center, el.focus1, distance)));
        } else {
            let dx = sys.add_param(fx - cx, true);
            let dy = sys.add_param(fy - cy, true);
            sys.add_constraint(Rc::new(Difference::new(el.center.x, el.focus1.x, dx)));
            sys.add_constraint(Rc::new(Difference::new(el.center.y, el.focus1.y, dy)));
        }
    }

    // Temporarily make each requested anchor a fixed kernel input. Marking the
    // geometry parameters as driven keeps them exact and out of the solver's
    // free-variable list; an approximate equality equation would still allow
    // visible drift after repeated grip frames.
    for reference in &anchors {
        if !set
            .constraints
            .iter()
            .any(|constraint| constraint.refs.iter().any(|item| item.entity == reference.entity))
        {
            continue;
        }
        let params_to_pin = if let Some(index) = reference.segment_index() {
            resolve_ref(document, &mut sys, &mut cache, *reference)
                .and_then(|geometry| geometry.line_segment(index))
                .map(|line| vec![line.p1.x, line.p1.y, line.p2.x, line.p2.y])
                .unwrap_or_default()
        } else if reference.is_text_axis()
            || reference.is_ellipse_major_axis()
            || reference.is_ellipse_minor_axis()
        {
            match resolve_ref(document, &mut sys, &mut cache, *reference) {
                Some(EntityGeom::TextAxis(line)) => {
                    vec![line.p1.x, line.p1.y, line.p2.x, line.p2.y]
                }
                Some(EntityGeom::Ellipse(ellipse)) => vec![
                    ellipse.center.x,
                    ellipse.center.y,
                    ellipse.focus1.x,
                    ellipse.focus1.y,
                    ellipse.radmin,
                ],
                _ => Vec::new(),
            }
        } else if reference.marker.is_some() {
            resolve_constraint_point(document, &mut sys, &mut cache, *reference)
                .map(|point| vec![point.x, point.y])
                .unwrap_or_default()
        } else {
            match resolve_ref(document, &mut sys, &mut cache, *reference) {
                Some(EntityGeom::Point(point)) => vec![point.x, point.y],
                Some(EntityGeom::Line(line))
                | Some(EntityGeom::Ray(line))
                | Some(EntityGeom::XLine(line))
                | Some(EntityGeom::TextAxis(line)) => {
                    vec![line.p1.x, line.p1.y, line.p2.x, line.p2.y]
                }
                Some(EntityGeom::Polyline { points, .. }) => points
                    .iter()
                    .flat_map(|point| [point.x, point.y])
                    .collect(),
                Some(EntityGeom::Circle(circle)) => {
                    vec![circle.center.x, circle.center.y, circle.rad]
                }
                Some(EntityGeom::Arc(arc)) => vec![
                    arc.circle.center.x,
                    arc.circle.center.y,
                    arc.circle.rad,
                    arc.start_angle,
                    arc.end_angle,
                ],
                Some(EntityGeom::Ellipse(ellipse)) => vec![
                    ellipse.center.x,
                    ellipse.center.y,
                    ellipse.focus1.x,
                    ellipse.focus1.y,
                    ellipse.radmin,
                ],
                Some(EntityGeom::Spline { curve, .. }) => curve
                    .poles
                    .iter()
                    .flat_map(|point| [point.x, point.y])
                    .collect(),
                None => Vec::new(),
            }
        };
        for parameter in params_to_pin {
            if !released_driver_params.contains(&parameter) {
                sys.store_mut().set_driven(parameter, true);
            }
        }
    }

    let partitions = sys.partition();
    for sub in &partitions {
        if solve_dl(sub, sys.store_mut()) == SolveStatus::Failed {
            solve_lm(sub, sys.store_mut());
        }
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
            EntityGeom::Point(_) => 2,
            EntityGeom::Line(_)
            | EntityGeom::Ray(_)
            | EntityGeom::XLine(_)
            | EntityGeom::TextAxis(_) => 4,
            EntityGeom::Polyline { points, arcs, .. } => {
                points.len() * 2 + arcs.iter().flatten().count() * 5
            }
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
            EntityGeom::Spline { curve, .. } => curve.poles.len() * 2,
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
        let mut seen = HashSet::new();
        let reportable: Vec<_> = diag
            .redundant
            .iter()
            .copied()
            .filter(|row| {
                owner
                    .iter()
                    .find(|(rc, _)| Rc::ptr_eq(rc, &sub.constraints()[*row]))
                    .is_some_and(|(_, id)| seen.insert(*id))
            })
            .collect();
        for (row, kind) in
            cadkernel_constraints::diagnosis::classify_redundant(sub, sys.store(), &reportable)
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
            (EntityType::Point(point), EntityGeom::Point(geometry)) => {
                let x = store.get(geometry.x);
                let y = store.get(geometry.y);
                if (x - point.location.x).abs() > MOVE_EPS
                    || (y - point.location.y).abs() > MOVE_EPS
                {
                    let mut updated = point.clone();
                    updated.location.x = x;
                    updated.location.y = y;
                    results.push((handle, EntityType::Point(updated)));
                }
            }
            (EntityType::Insert(insert), EntityGeom::Point(geometry)) => {
                let mut updated = insert.clone();
                let next = Vector3::new(
                    store.get(geometry.x),
                    store.get(geometry.y),
                    insert.insert_point.z,
                );
                if (next - insert.insert_point).length() > MOVE_EPS {
                    updated.insert_point = next;
                    results.push((handle, EntityType::Insert(updated)));
                }
            }
            (EntityType::Text(text), EntityGeom::TextAxis(axis)) => {
                let mut updated = text.clone();
                let next = Vector3::new(
                    store.get(axis.p1.x),
                    store.get(axis.p1.y),
                    text.insertion_point.z,
                );
                let delta = next - text.insertion_point;
                let dx = store.get(axis.p2.x) - store.get(axis.p1.x);
                let dy = store.get(axis.p2.y) - store.get(axis.p1.y);
                let rotation = if dx.hypot(dy) > MOVE_EPS {
                    dy.atan2(dx)
                } else {
                    text.rotation
                };
                if delta.length() > MOVE_EPS || (rotation - text.rotation).abs() > MOVE_EPS {
                    updated.insertion_point = next;
                    if let Some(alignment) = &mut updated.alignment_point {
                        *alignment = *alignment + delta;
                    }
                    updated.rotation = rotation;
                    results.push((handle, EntityType::Text(updated)));
                }
            }
            (EntityType::MText(text), EntityGeom::TextAxis(axis)) => {
                let mut updated = text.clone();
                let next = Vector3::new(
                    store.get(axis.p1.x),
                    store.get(axis.p1.y),
                    text.insertion_point.z,
                );
                let dx = store.get(axis.p2.x) - store.get(axis.p1.x);
                let dy = store.get(axis.p2.y) - store.get(axis.p1.y);
                let rotation = if dx.hypot(dy) > MOVE_EPS {
                    dy.atan2(dx)
                } else {
                    text.rotation
                };
                if (next - text.insertion_point).length() > MOVE_EPS
                    || (rotation - text.rotation).abs() > MOVE_EPS
                {
                    updated.insertion_point = next;
                    updated.rotation = rotation;
                    updated.dwg_x_direction = None;
                    results.push((handle, EntityType::MText(updated)));
                }
            }
            (EntityType::AttributeDefinition(attribute), EntityGeom::Point(geometry)) => {
                let mut updated = attribute.clone();
                let next = Vector3::new(
                    store.get(geometry.x),
                    store.get(geometry.y),
                    attribute.insertion_point.z,
                );
                let delta = next - attribute.insertion_point;
                if delta.length() > MOVE_EPS {
                    updated.insertion_point = next;
                    updated.alignment_point = updated.alignment_point + delta;
                    results.push((handle, EntityType::AttributeDefinition(updated)));
                }
            }
            (EntityType::AttributeEntity(attribute), EntityGeom::Point(geometry)) => {
                let mut updated = attribute.clone();
                let next = Vector3::new(
                    store.get(geometry.x),
                    store.get(geometry.y),
                    attribute.insertion_point.z,
                );
                let delta = next - attribute.insertion_point;
                if delta.length() > MOVE_EPS {
                    updated.insertion_point = next;
                    updated.alignment_point = updated.alignment_point + delta;
                    results.push((handle, EntityType::AttributeEntity(updated)));
                }
            }
            (EntityType::Table(table), EntityGeom::Point(geometry)) => {
                let mut updated = table.clone();
                let next = Vector3::new(
                    store.get(geometry.x),
                    store.get(geometry.y),
                    table.insertion_point.z,
                );
                if (next - table.insertion_point).length() > MOVE_EPS {
                    updated.insertion_point = next;
                    results.push((handle, EntityType::Table(updated)));
                }
            }
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
            (EntityType::Ray(ray), EntityGeom::Ray(g)) => {
                let (x1, y1, x2, y2) = (
                    store.get(g.p1.x),
                    store.get(g.p1.y),
                    store.get(g.p2.x),
                    store.get(g.p2.y),
                );
                let length = (x2 - x1).hypot(y2 - y1);
                let direction = if length > MOVE_EPS {
                    Vector3::new((x2 - x1) / length, (y2 - y1) / length, 0.0)
                } else {
                    ray.direction
                };
                if (x1 - ray.base_point.x).abs() > MOVE_EPS
                    || (y1 - ray.base_point.y).abs() > MOVE_EPS
                    || (direction - ray.direction).length() > MOVE_EPS
                {
                    let mut updated = ray.clone();
                    updated.base_point.x = x1;
                    updated.base_point.y = y1;
                    updated.direction = direction;
                    results.push((handle, EntityType::Ray(updated)));
                }
            }
            (EntityType::XLine(line), EntityGeom::XLine(g)) => {
                let (x1, y1, x2, y2) = (
                    store.get(g.p1.x),
                    store.get(g.p1.y),
                    store.get(g.p2.x),
                    store.get(g.p2.y),
                );
                let length = (x2 - x1).hypot(y2 - y1);
                let direction = if length > MOVE_EPS {
                    Vector3::new((x2 - x1) / length, (y2 - y1) / length, 0.0)
                } else {
                    line.direction
                };
                if (x1 - line.base_point.x).abs() > MOVE_EPS
                    || (y1 - line.base_point.y).abs() > MOVE_EPS
                    || (direction - line.direction).length() > MOVE_EPS
                {
                    let mut updated = line.clone();
                    updated.base_point.x = x1;
                    updated.base_point.y = y1;
                    updated.direction = direction;
                    results.push((handle, EntityType::XLine(updated)));
                }
            }
            (EntityType::LwPolyline(polyline), EntityGeom::Polyline { points, .. }) => {
                let mut updated = polyline.clone();
                let mut changed = false;
                for (vertex, point) in updated.vertices.iter_mut().zip(points.iter()) {
                    let x = store.get(point.x);
                    let y = store.get(point.y);
                    if (x - vertex.location.x).abs() > MOVE_EPS
                        || (y - vertex.location.y).abs() > MOVE_EPS
                    {
                        vertex.location.x = x;
                        vertex.location.y = y;
                        changed = true;
                    }
                }
                if changed {
                    results.push((handle, EntityType::LwPolyline(updated)));
                }
            }
            (EntityType::Polyline2D(polyline), EntityGeom::Polyline { points, .. }) => {
                let mut updated = polyline.clone();
                let mut changed = false;
                for (vertex, point) in updated.vertices.iter_mut().zip(points.iter()) {
                    let x = store.get(point.x);
                    let y = store.get(point.y);
                    if (x - vertex.location.x).abs() > MOVE_EPS
                        || (y - vertex.location.y).abs() > MOVE_EPS
                    {
                        vertex.location.x = x;
                        vertex.location.y = y;
                        changed = true;
                    }
                }
                if changed {
                    results.push((handle, EntityType::Polyline2D(updated)));
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
            (EntityType::Spline(spline), EntityGeom::Spline { curve, control_z }) => {
                let mut changed =
                    spline.fit_points.len() > 0 || spline.control_points.len() != curve.poles.len();
                let control_points: Vec<_> = curve
                    .poles
                    .iter()
                    .zip(control_z.iter())
                    .enumerate()
                    .map(|(index, (point, z))| {
                        let next = Vector3::new(store.get(point.x), store.get(point.y), *z);
                        changed |= spline
                            .control_points
                            .get(index)
                            .is_none_or(|old| (next - *old).length() > MOVE_EPS);
                        next
                    })
                    .collect();
                if changed {
                    let mut updated = spline.clone();
                    updated.degree = curve.degree as i32;
                    updated.knots = curve.knots.clone();
                    updated.control_points = control_points;
                    updated.weights = curve.weights.iter().map(|id| store.get(*id)).collect();
                    updated.fit_points.clear();
                    updated.begin_tangent = Vector3::ZERO;
                    updated.end_tangent = Vector3::ZERO;
                    updated.flags.rational = updated
                        .weights
                        .windows(2)
                        .any(|pair| (pair[0] - pair[1]).abs() > 1.0e-12);
                    updated.flags.closed = curve.periodic;
                    updated.flags.periodic = curve.periodic;
                    results.push((handle, EntityType::Spline(updated)));
                }
            }
            _ => {}
        }
    }
    apply_smooth_constraints(document, set, &mut results);
    Some((results, dof, conflicts))
}

impl Scene {
    /// Rejects constraints the 2D kernel bridge cannot represent before they
    /// are persisted or reported as applied.
    pub(crate) fn validate_parametric_constraint(
        &self,
        kind: ConstraintKind,
        refs: &[ParametricRef],
        driving_param: Option<&super::named_parameters::DrivingValue>,
    ) -> Result<(), &'static str> {
        if !refs_share_supported_plane(&self.document, refs) {
            return Err(
                "Constraint references must be supported entities on the same world-XY plane.",
            );
        }
        if kind == ConstraintKind::Smooth {
            let [source_ref, target_ref] = refs else {
                return Err("Smooth requires one spline endpoint and one target endpoint.");
            };
            if source_ref.entity == target_ref.entity {
                return Err("Smooth requires two different entities.");
            }
            let Some(EntityType::Spline(spline)) = self.document.get_entity(source_ref.entity)
            else {
                return Err("Smooth requires an open spline as its first reference.");
            };
            if spline.flags.closed || spline.flags.periodic {
                return Err("Smooth requires an open spline.");
            }
            let endpoint = match source_ref.marker {
                Some(0) => cadkernel::space::SplineEnd::Start,
                Some(1) => cadkernel::space::SplineEnd::End,
                _ => return Err("Smooth requires a spline endpoint."),
            };
            let Some(target_marker) = target_ref.marker else {
                return Err("Smooth requires a target endpoint.");
            };
            let Some(target) = self.document.get_entity(target_ref.entity) else {
                return Err("Smooth target does not exist.");
            };
            let Some(target_jet) = smooth_target_jet(target, target_marker) else {
                return Err("The selected target does not support endpoint smoothing.");
            };
            let Some(curve) = crate::entities::spline::nurbs3(spline) else {
                return Err("The selected spline cannot be evaluated.");
            };
            if cadkernel::space::smooth_nurbs_endpoint(&curve, endpoint, target_jet).is_none() {
                return Err("The selected spline cannot satisfy curvature continuity.");
            }
            return Ok(());
        }
        let validation_target = match driving_param {
            Some(super::named_parameters::DrivingValue::Named(name))
                if !self.named_parameters.contains(name) =>
            {
                Some(super::named_parameters::DrivingValue::Literal(1.0))
            }
            value => value.cloned(),
        };
        let candidate = ParametricConstraint {
            id: 0,
            kind,
            refs: refs.to_vec(),
            driving_param: validation_target,
            enabled: true,
            native_origin: None,
            rigid_points: Vec::new(),
            distance_direction_type: 0,
            distance_direction: None,
            angle_sector: angle_sector::PARALLEL_COUNTERCLOCKWISE,
        };
        let mut system = System::new();
        let mut cache = HashMap::new();
        if build_constraint(
            &self.document,
            &mut system,
            &mut cache,
            &self.named_parameters,
            &candidate,
            None,
            None,
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
    #[allow(dead_code)]
    pub(crate) fn refresh_parametric_constraints(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        self.refresh_parametric_constraints_with_policy(changes, &[], false)
    }

    #[allow(dead_code)]
    pub(crate) fn refresh_parametric_constraints_with_driven(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[ParametricRef],
    ) -> Vec<(Handle, ChangeKind)> {
        self.refresh_parametric_constraints_with_policy(changes, driven_refs, false)
    }

    pub(crate) fn refresh_parametric_constraints_with_policy(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[ParametricRef],
        retain_size: bool,
    ) -> Vec<(Handle, ChangeKind)> {
        self.refresh_parametric_constraints_with_originals(
            changes,
            driven_refs,
            retain_size,
            &[],
        )
    }

    pub(crate) fn refresh_parametric_constraints_with_originals(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, EntityType)],
    ) -> Vec<(Handle, ChangeKind)> {
        // An erased entity takes its constraints with it instead of leaving
        // dangling references. Done first,
        // and unconditionally over every scope (not just ones a `touched`
        // check would catch), so a scope left with zero constraints after
        // this doesn't attempt a pointless resolve below.
        //
        // Recorded into the *same* undo transaction as the entity removal
        // that caused it (audit finding: ERASE undo silently lost constraint
        // state, since `parametric_constraints` lives outside `document`/
        // `document.objects` and neither of those directories ever saw this
        // mutation) — one undo press now restores both the entity and its
        // constraints together.
        for (handle, kind) in changes {
            if *kind != ChangeKind::Removed {
                continue;
            }
            for i in 0..self.parametric_constraints.len() {
                if self.parametric_constraints[i]
                    .constraints
                    .iter()
                    .any(|constraint| constraint.refs.iter().any(|r| r.entity == *handle))
                {
                    let scope = self.parametric_constraints[i].scope;
                    let before = self.parametric_constraints[i].clone();
                    self.record_undo_parametric_constraints_before(scope, before);
                    self.parametric_constraints[i].remove_all_touching(*handle);
                }
            }
        }

        let retained_before: HashMap<Handle, std::sync::Arc<EntityType>> = if retain_size {
            let mut retained: HashMap<Handle, std::sync::Arc<EntityType>> = self
                .undo_recording
                .as_ref()
                .into_iter()
                .flat_map(|recording| recording.before.iter())
                .filter_map(|(handle, entity)| {
                    entity
                        .as_ref()
                        .map(|entity| (*handle, std::sync::Arc::clone(entity)))
                })
                .collect();
            for (handle, entity) in retained_originals {
                retained.insert(*handle, std::sync::Arc::new(entity.clone()));
            }
            retained
        } else {
            HashMap::new()
        };
        let mut result = Vec::new();
        for i in 0..self.parametric_constraints.len() {
            let touched = changes.iter().any(|(handle, _)| {
                self.parametric_constraints[i]
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
                &self.parametric_constraints[i],
                driven_refs,
                retain_size,
                true,
                &retained_before,
            ) else {
                continue;
            };
            self.parametric_constraints[i].dof = Some(dof);
            self.parametric_constraints[i].conflicts = conflicts;
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
    pub(crate) fn solve_parametric_constraints_preview(
        &self,
        touched: &[Handle],
        driven_refs: &[ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, EntityType)],
    ) -> Vec<(Handle, EntityType)> {
        if self.parametric_constraints.is_empty() || touched.is_empty() {
            return Vec::new();
        }
        let retained_before: HashMap<Handle, std::sync::Arc<EntityType>> = retained_originals
            .iter()
            .map(|(handle, entity)| (*handle, std::sync::Arc::new(entity.clone())))
            .collect();
        let mut result = Vec::new();
        for set in &self.parametric_constraints {
            let is_touched = touched
                .iter()
                .any(|handle| set.constraints_touching(*handle).next().is_some());
            if !is_touched {
                continue;
            }
            let connected = self.parametric_connected_handles(set.scope, touched, true);
            let mut component = set.clone();
            component.constraints.retain(|constraint| constraint.refs.iter()
                .any(|reference| connected.contains(&reference.entity)));
            let Some((solved, _dof, _conflicts)) = solve_scope(
                &self.document,
                &self.named_parameters,
                &component,
                driven_refs,
                retain_size,
                false,
                &retained_before,
            )
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
    use super::super::parametric_constraints::{
        ConstraintKind, ParametricConstraintSet, ParametricRef, ParametricScope,
    };
    use super::Scene;
    use acadrust::entities::EntityType;
    use acadrust::types::Vector3;

    #[test]
    fn perpendicular_line_grips_distinguish_shared_corner_and_free_ends() {
        use acadrust::entities::Line;
        for (vertical, marker) in [(false, 0), (false, 1), (true, 1)] {
            let mut scene = Scene::new();
            let a = scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO, Vector3::new(4.0, 0.0, 0.0))));
            let b = scene.add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO, Vector3::new(0.0, 2.0, 0.0))));
            let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
            set.add(ConstraintKind::Horizontal, vec![ParametricRef::whole(a)], None);
            set.add(ConstraintKind::Perpendicular, vec![ParametricRef::whole(a), ParametricRef::whole(b)], None);
            set.add(ConstraintKind::Coincident, vec![ParametricRef::point(a, 0), ParametricRef::point(b, 0)], None);
            let originals = [a, b].map(|handle| (handle, scene.document.get_entity(handle).unwrap().clone()));
            let handle = if vertical { b } else { a };
            let target = Vector3::new(6.0, 3.0, 0.0);
            let Some(EntityType::Line(line)) = scene.document.get_entity_mut(handle) else { panic!("line") };
            if marker == 0 { line.start = target; } else { line.end = target; }
            let solved = scene.solve_parametric_constraints_preview(
                &[handle], &[ParametricRef::point(handle, marker)], true, &originals);
            for (handle, entity) in solved {
                *scene.document.get_entity_mut(handle).unwrap() = entity;
            }
            let expected = if marker == 0 {
                [[target, Vector3::new(10.0, 3.0, 0.0)], [target, Vector3::new(6.0, 5.0, 0.0)]]
            } else if vertical {
                [[Vector3::new(6.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0)],
                 [Vector3::new(6.0, 0.0, 0.0), target]]
            } else {
                [[Vector3::new(0.0, 3.0, 0.0), target],
                 [Vector3::new(0.0, 3.0, 0.0), Vector3::new(0.0, 5.0, 0.0)]]
            };
            for (handle, points) in [a, b].into_iter().zip(expected) {
                let Some(EntityType::Line(line)) = scene.document.get_entity(handle) else { panic!("line") };
                assert!((line.start - points[0]).length() < 1e-7, "{vertical}/{marker}: {line:?}");
                assert!((line.end - points[1]).length() < 1e-7, "{vertical}/{marker}: {line:?}");
            }
        }
    }

    #[test]
    fn validation_rejects_unsupported_entity_shapes_and_planes() {
        let mut scene = Scene::new();
        let circle = scene.add_entity(EntityType::Circle(
            acadrust::entities::Circle::from_center_radius(Vector3::ZERO, 2.0),
        ));
        assert!(scene
            .validate_parametric_constraint(
                ConstraintKind::Horizontal,
                &[ParametricRef::whole(circle)],
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
            .validate_parametric_constraint(
                ConstraintKind::Parallel,
                &[ParametricRef::whole(lower), ParametricRef::whole(upper)],
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
        let refs = [ParametricRef::point(line, 0), ParametricRef::point(line, 1)];
        assert!(scene
            .validate_parametric_constraint(
                ConstraintKind::Distance,
                &refs,
                Some(&DrivingValue::Literal(-1.0)),
            )
            .is_err());
        assert!(scene
            .validate_parametric_constraint(
                ConstraintKind::DistanceX,
                &refs,
                Some(&DrivingValue::Literal(-1.0)),
            )
            .is_ok());
        assert!(scene
            .validate_parametric_constraint(
                ConstraintKind::DistanceX,
                &refs,
                Some(&DrivingValue::Literal(f64::NAN)),
            )
            .is_err());
    }

    #[test]
    fn bulged_polyline_segment_registers_as_a_kernel_arc() {
        let mut scene = Scene::new();
        let mut polyline = acadrust::entities::LwPolyline::new();
        polyline.vertices = vec![
            acadrust::entities::LwVertex::with_bulge(
                acadrust::types::Vector2::new(0.0, 0.0),
                1.0,
            ),
            acadrust::entities::LwVertex::from_coords(10.0, 0.0),
        ];
        let handle = scene.add_entity(EntityType::LwPolyline(polyline));
        let mut system = cadkernel_constraints::system::System::new();
        let geometry = super::register_entity(&scene.document, &mut system, handle).unwrap();
        let segment = geometry.arc_segment(0).expect("expected an arc segment");

        assert!((system.store().get(segment.arc.circle.rad) - 5.0).abs() < 1.0e-9);
        assert!((segment.sweep - std::f64::consts::PI).abs() < 1.0e-9);
    }

    #[test]
    fn ray_and_construction_line_constraints_solve_through_the_kernel() {
        let mut scene = Scene::new();
        let ray = scene.add_entity(EntityType::Ray(acadrust::entities::Ray::new(
            Vector3::ZERO,
            Vector3::new(1.0, 1.0, 0.0),
        )));
        let xline = scene.add_entity(EntityType::XLine(acadrust::entities::XLine::new(
            Vector3::new(5.0, 5.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
        )));
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(ray)],
            None,
        );
        set.add(
            ConstraintKind::Vertical,
            vec![ParametricRef::whole(xline)],
            None,
        );

        scene.refresh_parametric_constraints(&[
            (ray, super::ChangeKind::Modified),
            (xline, super::ChangeKind::Modified),
        ]);

        let Some(EntityType::Ray(ray)) = scene.document.get_entity(ray) else {
            panic!("expected ray")
        };
        assert!(ray.direction.y.abs() < 1e-7);
        let Some(EntityType::XLine(xline)) = scene.document.get_entity(xline) else {
            panic!("expected construction line")
        };
        assert!(xline.direction.x.abs() < 1e-7);
    }

    // The bulk of this stage's coverage (Horizontal/Parallel/Distance/
    // Coincident solving, unrelated edits not triggering a resolve) lives
    // in `tests/parametric_constraints_solve.rs`, which only needs `pub` API.
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

        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
        scene.parametric_constraints.push(set);

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

        let (entities, _objects, _parametric_constraints, _named_parameters) =
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
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Coincident,
                vec![ParametricRef::point(a, 1), ParametricRef::point(b, 0)],
                None,
            );
        assert_eq!(
            scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
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
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map_or(0, |s| s.constraints.len()),
            0,
            "the constraint touching the erased line should be gone from the live set"
        );

        let recording = scene
            .take_undo_recording()
            .expect("an undo recording should still be open");
        let (_entities, _objects, parametric_constraints, _named_parameters) =
            recording.into_recorded_images();
        assert_eq!(
            parametric_constraints.len(),
            1,
            "the touched scope's before-image must be captured"
        );
        let (scope, before) = &parametric_constraints[0];
        assert_eq!(*scope, ParametricScope::ModelSpace);
        assert_eq!(
            before.constraints.len(),
            1,
            "the before-image must still hold the constraint as it was before the erase"
        );
        assert_eq!(before.constraints[0].kind, ConstraintKind::Coincident);
        assert_eq!(
            before.constraints[0].refs,
            vec![ParametricRef::point(a, 1), ParametricRef::point(b, 0)]
        );

        // What `apply_delta_state` does with this on undo: install the
        // before-image back as the live scope state.
        *scene.parametric_constraint_set_mut(*scope) = before.clone();
        assert_eq!(
            scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1,
            "restoring the before-image must bring the constraint back"
        );
    }

    /// A grip preview lets the paired line and tangent circle follow the
    /// edited line while preserving their sizes and parallel separation.
    #[test]
    fn preview_solve_preserves_parallel_spacing_through_tangent_radius() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 10.0, 0.0),
        )));
        let b = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(2.0, 10.0, 0.0),
        )));
        let circle = scene.add_entity(EntityType::Circle(
            acadrust::entities::Circle::from_center_radius(Vector3::new(1.0, 5.0, 0.0), 1.0),
        ));
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
        set.add(
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(a), ParametricRef::whole(circle)],
            None,
        );
        set.add(
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(b), ParametricRef::whole(circle)],
            None,
        );
        scene.parametric_constraints.push(set);

        let a_before = scene.document.get_entity(a).unwrap().clone();
        if let Some(EntityType::Line(l)) = scene.document.get_entity_mut(a) {
            l.end = Vector3::new(2.0, 10.0, 0.0);
        }
        let b_before = scene.document.get_entity(b).cloned();

        let solved = scene.solve_parametric_constraints_preview(
            &[a],
            &[ParametricRef::point(a, 0), ParametricRef::point(a, 1)],
            true,
            &[(a, a_before)],
        );
        assert_eq!(
            scene.document.get_entity(b),
            b_before.as_ref(),
            "preview must not mutate the document"
        );
        let solved_entity = |handle| {
            solved
                .iter()
                .find(|(candidate, _)| *candidate == handle)
                .map(|(_, entity)| entity)
                .unwrap_or_else(|| scene.document.get_entity(handle).unwrap())
        };
        let EntityType::Line(moved_a) = solved_entity(a) else {
            panic!("expected a Line")
        };
        let EntityType::Line(moved_b) = solved_entity(b) else {
            panic!("expected a Line")
        };
        let EntityType::Circle(moved_circle) = solved_entity(circle) else {
            panic!("expected a Circle")
        };
        assert_eq!(moved_a.start, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(moved_a.end, Vector3::new(2.0, 10.0, 0.0));
        assert_ne!(solved_entity(b), b_before.as_ref().unwrap());
        let dir_a = moved_a.end - moved_a.start;
        let dir_b = moved_b.end - moved_b.start;
        let cross = dir_a.x * dir_b.y - dir_a.y * dir_b.x;
        assert!(cross.abs() < 1.0e-6);
        let offset = moved_b.start - moved_a.start;
        let separation = (dir_a.x * offset.y - dir_a.y * offset.x).abs() / dir_a.length();
        assert!(
            (separation - 2.0).abs() < 1.0e-6,
            "separation={separation}, a={moved_a:?}, b={moved_b:?}, circle={moved_circle:?}"
        );
        assert!((moved_circle.radius - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn rigid_set_preserves_relative_geometry_after_one_member_moves() {
        let mut scene = Scene::new();
        let a = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        let b = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
        )));
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::RigidSet,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
        set.constraints[0].rigid_points = vec![
            (ParametricRef::point(a, 0), Vector3::new(0.0, 0.0, 0.0)),
            (ParametricRef::point(a, 1), Vector3::new(10.0, 0.0, 0.0)),
            (ParametricRef::point(b, 0), Vector3::new(0.0, 5.0, 0.0)),
            (ParametricRef::point(b, 1), Vector3::new(10.0, 5.0, 0.0)),
        ];
        scene.parametric_constraints.push(set);

        if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(b) {
            line.start.y += 10.0;
            line.end.y += 10.0;
        }
        scene.bump_entities(&[(b, super::ChangeKind::Modified)]);

        let endpoints = |handle| {
            let EntityType::Line(line) = scene.document.get_entity(handle).unwrap() else {
                panic!("expected a line")
            };
            (line.start, line.end)
        };
        let (a0, a1) = endpoints(a);
        let (b0, b1) = endpoints(b);
        let distance = |first: Vector3, second: Vector3| {
            let delta = second - first;
            (delta.x * delta.x + delta.y * delta.y).sqrt()
        };
        assert!((distance(a0, a1) - 10.0).abs() < 1.0e-6);
        assert!((distance(b0, b1) - 10.0).abs() < 1.0e-6);
        assert!((distance(a0, b0) - 5.0).abs() < 1.0e-6);
        assert!((distance(a1, b1) - 5.0).abs() < 1.0e-6);
    }

    #[test]
    fn implicit_midpoint_recomputes_with_its_parent_line() {
        let mut scene = Scene::new();
        let carrier = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        )));
        let anchor = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(7.0, 3.0, 0.0),
            Vector3::new(7.0, 8.0, 0.0),
        )));
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Fixed,
            vec![ParametricRef::whole(anchor)],
            None,
        );
        set.add(
            ConstraintKind::Coincident,
            vec![
                ParametricRef::point(carrier, -2),
                ParametricRef::point(anchor, 0),
            ],
            None,
        );

        scene.bump_entities(&[(carrier, super::ChangeKind::Modified)]);

        let EntityType::Line(line) = scene.document.get_entity(carrier).unwrap() else {
            panic!("expected line");
        };
        let midpoint = (line.start + line.end) * 0.5;
        assert!((midpoint - Vector3::new(7.0, 3.0, 0.0)).length() < 1.0e-7);
    }

    #[test]
    fn spline_endpoint_participates_in_the_kernel_constraint_system() {
        let mut scene = Scene::new();
        let spline = scene.add_entity(EntityType::Spline(
            acadrust::entities::Spline::from_control_points(
                3,
                vec![
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(2.0, 1.0, 0.0),
                    Vector3::new(4.0, 1.0, 0.0),
                    Vector3::new(6.0, 0.0, 0.0),
                ],
            ),
        ));
        let anchor = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(9.0, 4.0, 0.0),
            Vector3::new(9.0, 7.0, 0.0),
        )));
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Fixed,
            vec![ParametricRef::whole(anchor)],
            None,
        );
        set.add(
            ConstraintKind::Coincident,
            vec![
                ParametricRef::point(spline, 1),
                ParametricRef::point(anchor, 0),
            ],
            None,
        );

        scene.bump_entities(&[(anchor, super::ChangeKind::Modified)]);

        let EntityType::Spline(spline) = scene.document.get_entity(spline).unwrap() else {
            panic!("expected spline");
        };
        let end = spline.control_points.last().copied().unwrap();
        assert!((end - Vector3::new(9.0, 4.0, 0.0)).length() < 1.0e-7);
    }

    #[test]
    fn smooth_constraint_recomputes_the_spline_after_its_target_moves() {
        let mut scene = Scene::new();
        let target = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
        )));
        let mut spline = acadrust::entities::Spline::new();
        spline.degree = 3;
        spline.control_points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 2.0, 0.0),
            Vector3::new(3.0, 2.0, 0.0),
            Vector3::new(5.0, 3.0, 0.0),
        ];
        spline.knots = cadkernel::space::clamped_uniform_knots(3, 4);
        let spline = scene.add_entity(EntityType::Spline(spline));
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Smooth,
                vec![
                    ParametricRef::point(spline, 0),
                    ParametricRef::point(target, 0),
                ],
                None,
            );

        if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(target) {
            line.start.y = 1.0;
            line.end.y = 1.0;
        }
        scene.bump_entities(&[(target, super::ChangeKind::Modified)]);

        let EntityType::Spline(spline) = scene.document.get_entity(spline).unwrap() else {
            panic!("expected spline");
        };
        let curve = crate::entities::spline::nurbs3(spline).unwrap();
        let jet =
            cadkernel::space::CurveJet::from_nurbs(&curve, cadkernel::space::SplineEnd::Start)
                .unwrap();
        assert!((jet.point[0] - 0.0).abs() < 1e-9);
        assert!((jet.point[1] - 1.0).abs() < 1e-9);
        assert!(jet.tangent[1].abs() < 1e-9, "jet={jet:?}");
        assert!(cadkernel::space::Vec3::from(jet.curvature).length() < 5e-3);
    }
}
