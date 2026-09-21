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
    ProjectedDistance, ProjectedDistanceAlongLine, SymmetricLineDirections,
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
use cadkernel_constraints::util::ParamId;

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
    /// A text insertion point plus its displayed baseline direction. The
    /// second point is a solver guide only; write-back converts it to the
    /// entity's rotation/alignment fields.
    TextLine(GLine),
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
            (EntityGeom::TextLine(line), 0) => Some(line.p1),
            (EntityGeom::Line(l), 0) => Some(l.p1),
            (EntityGeom::Line(l), 1) => Some(l.p2),
            (EntityGeom::Ray(l), 0) => Some(l.p1),
            (EntityGeom::Polyline { points, closed, .. }, marker) if marker >= 0 => {
                // The closing segment's end marker (one past the last vertex)
                // is vertex 0 on a closed polyline, as in `line_segment`.
                let index = marker as usize;
                let index = if *closed && index == points.len() && !points.is_empty() {
                    0
                } else {
                    index
                };
                points.get(index).copied()
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
        EntityGeom::TextLine(l)
        | EntityGeom::Line(l)
        | EntityGeom::Ray(l)
        | EntityGeom::XLine(l) => CircleOrLine::Line(l),
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
    register_entity_geom(sys, handle, entity)
}

/// `register_entity` for an entity value that isn't the document's — the
/// retained pre-edit original a persistent `Fixed` pins to.
fn register_entity_geom(
    sys: &mut System,
    handle: Handle,
    entity: &EntityType,
) -> Option<EntityGeom> {
    match entity {
        EntityType::Point(point) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(point.location.x, false),
            sys.add_param(point.location.y, false),
        ))),
        EntityType::Insert(insert) => Some(EntityGeom::Point(GPoint::new(
            sys.add_param(insert.insert_point.x, false),
            sys.add_param(insert.insert_point.y, false),
        ))),
        EntityType::Text(_) | EntityType::MText(_) => {
            let reference = ParametricRef::text_baseline(handle);
            let [start, end] = super::parametric_constraints::directional_axis_endpoints(
                entity,
                reference,
            )?;
            Some(EntityGeom::TextLine(GLine {
                p1: GPoint::new(
                    sys.add_param(start.x, false),
                    sys.add_param(start.y, false),
                ),
                p2: GPoint::new(sys.add_param(end.x, false), sys.add_param(end.y, false)),
            }))
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
    if let Some(segment) = reference
        .segment_center_index()
        .and_then(|index| geometry.arc_segment(index))
    {
        return Some(segment.arc.circle.center);
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

fn concentric_reference_geometry(
    document: &acadrust::CadDocument,
    reference: ParametricRef,
) -> Option<(Vector3, cadkernel::space::Plane)> {
    let marker = reference.marker?;
    if marker != -3 && reference.segment_center_index().is_none() {
        return None;
    }
    let entity = document.get_entity(reference.entity)?;
    let center = super::parametric_constraints::resolve_point(entity, marker)?;
    let plane = crate::entities::curve::entity_curve(entity)?.plane;
    Some((center, plane))
}

fn concentric_refs_share_plane(
    document: &acadrust::CadDocument,
    refs: &[ParametricRef],
) -> bool {
    let [first, second] = refs else {
        return false;
    };
    if first == second {
        return false;
    }
    let Some((first_center, first_plane)) = concentric_reference_geometry(document, *first) else {
        return false;
    };
    let Some((second_center, second_plane)) = concentric_reference_geometry(document, *second)
    else {
        return false;
    };
    let (Some(first_normal), Some(second_normal)) = (first_plane.normal(), second_plane.normal())
    else {
        return false;
    };
    let parallel = (first_normal[0] * second_normal[0]
        + first_normal[1] * second_normal[1]
        + first_normal[2] * second_normal[2])
        .abs()
        >= 1.0 - 1.0e-9;
    let points = [
        first_plane.origin,
        second_plane.origin,
        [first_center.x, first_center.y, first_center.z],
        [second_center.x, second_center.y, second_center.z],
    ];
    let tolerance = cadkernel::space::coplanarity_tolerance(&points);
    parallel
        && first_plane
            .distance_to(second_plane.origin)
            .is_some_and(|distance| distance.abs() <= tolerance)
        && first_plane
            .distance_to([second_center.x, second_center.y, second_center.z])
            .is_some_and(|distance| distance.abs() <= tolerance)
}

fn ref_is_driven(driven: &[ParametricRef], reference: ParametricRef) -> bool {
    driven.iter().any(|candidate| {
        candidate.entity == reference.entity
            && (candidate.marker == reference.marker || candidate.marker.is_none())
    })
}

fn apply_spatial_concentric_constraints(
    document: &acadrust::CadDocument,
    set: &ParametricConstraintSet,
    driven_refs: &[ParametricRef],
    initial_fixed_refs: &[ParametricRef],
    results: &mut Vec<(Handle, EntityType)>,
) {
    let current_entity = |handle: Handle, results: &[(Handle, EntityType)]| {
        results
            .iter()
            .rev()
            .find(|(candidate, _)| *candidate == handle)
            .map(|(_, entity)| entity.clone())
            .or_else(|| document.get_entity(handle).cloned())
    };

    let mut handles = Vec::<Handle>::new();
    let mut relations = Vec::new();
    let mut fixed = Vec::new();
    let mut points = Vec::new();
    for constraint in set.constraints.iter().filter(|constraint| {
        constraint.enabled
            && constraint.kind == ConstraintKind::Concentric
            && !refs_share_supported_plane(document, &constraint.refs)
            && concentric_refs_share_plane(document, &constraint.refs)
    }) {
        let [first, second] = constraint.refs.as_slice() else {
            continue;
        };
        if first.entity == second.entity {
            continue;
        }
        let (Some(first_entity), Some(second_entity)) = (
            current_entity(first.entity, results),
            current_entity(second.entity, results),
        ) else {
            continue;
        };
        let (Some(first_marker), Some(second_marker)) = (first.marker, second.marker) else {
            continue;
        };
        let (Some(first_center), Some(second_center)) = (
            super::parametric_constraints::resolve_point(&first_entity, first_marker),
            super::parametric_constraints::resolve_point(&second_entity, second_marker),
        ) else {
            continue;
        };
        let first_index = handles
            .iter()
            .position(|handle| *handle == first.entity)
            .unwrap_or_else(|| {
                handles.push(first.entity);
                handles.len() - 1
            });
        let second_index = handles
            .iter()
            .position(|handle| *handle == second.entity)
            .unwrap_or_else(|| {
                handles.push(second.entity);
                handles.len() - 1
            });
        let first_point = [first_center.x, first_center.y, first_center.z];
        let second_point = [second_center.x, second_center.y, second_center.z];
        relations.push((first_index, first_point, second_index, second_point));
        points.extend([first_point, second_point]);
        if ref_is_driven(driven_refs, *first) || ref_is_driven(initial_fixed_refs, *first) {
            fixed.push(first_index);
        }
        if ref_is_driven(driven_refs, *second) || ref_is_driven(initial_fixed_refs, *second) {
            fixed.push(second_index);
        }
    }
    if relations.is_empty() {
        return;
    }
    fixed.sort_unstable();
    fixed.dedup();
    let tolerance = cadkernel::space::coplanarity_tolerance(&points).max(MOVE_EPS);
    let Some(translations) = cadkernel::space::solve_rigid_point_coincidence(
        handles.len(),
        &relations,
        &fixed,
        tolerance,
    ) else {
        return;
    };
    for (handle, translation) in handles.into_iter().zip(translations) {
        let translation = glam::DVec3::from_array(translation);
        if translation.length() <= tolerance {
            continue;
        }
        let Some(mut entity) = current_entity(handle, results) else {
            continue;
        };
        crate::scene::view::dispatch::apply_transform(
            &mut entity,
            &crate::command::EntityTransform::Translate(translation),
        );
        if let Some((_, current)) = results.iter_mut().find(|(candidate, _)| *candidate == handle) {
            *current = entity;
        } else {
            results.push((handle, entity));
        }
    }
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

/// A solve result with no extent left — the only way the solver could
/// honour every constraint after an edit that contradicted them.
fn collapsed_by_solve(entity: &EntityType) -> bool {
    const EPS: f64 = 1.0e-9;
    match entity {
        EntityType::Line(line) => (line.end - line.start).length() <= EPS,
        EntityType::Circle(circle) => circle.radius <= EPS,
        EntityType::Arc(arc) => arc.radius <= EPS,
        _ => false,
    }
}

/// The kernel parameters a `Fixed` constraint on `r` holds in place: a
/// polyline segment's endpoints (plus center/radius for an arc segment), one
/// addressable point, or every intrinsic parameter of a whole entity. Arc
/// pins its five intrinsic params only — `start`/`end` are already tied to
/// those by the arc-rules `CurveValue` constraints `solve_scope` adds;
/// Ellipse likewise pins center + radmin, `focus1` being tied to `center`
/// by the ellipse-rules `Difference` constraints.
fn fixed_pin_params(
    document: &acadrust::CadDocument,
    sys: &mut System,
    cache: &mut HashMap<Handle, EntityGeom>,
    r: ParametricRef,
) -> Vec<ParamId> {
    if let Some(index) = r.segment_index() {
        let Some(geometry) = resolve_ref(document, sys, cache, r) else {
            return Vec::new();
        };
        if let Some(line) = geometry.line_segment(index) {
            return vec![line.p1.x, line.p1.y, line.p2.x, line.p2.y];
        }
        return geometry
            .arc_segment(index)
            .map(|segment| {
                let arc = segment.arc;
                vec![
                    arc.start.x,
                    arc.start.y,
                    arc.end.x,
                    arc.end.y,
                    arc.circle.center.x,
                    arc.circle.center.y,
                    arc.circle.rad,
                ]
            })
            .unwrap_or_default();
    }
    if r.marker.is_some() {
        return resolve_constraint_point(document, sys, cache, r)
            .map(|point| vec![point.x, point.y])
            .unwrap_or_default();
    }
    match resolve_ref(document, sys, cache, r) {
        Some(EntityGeom::Point(point)) => vec![point.x, point.y],
        Some(EntityGeom::Polyline { points, .. }) => {
            points.iter().flat_map(|point| [point.x, point.y]).collect()
        }
        Some(EntityGeom::TextLine(line))
        | Some(EntityGeom::Line(line))
        | Some(EntityGeom::Ray(line))
        | Some(EntityGeom::XLine(line)) => vec![line.p1.x, line.p1.y, line.p2.x, line.p2.y],
        Some(EntityGeom::Circle(circle)) => vec![circle.center.x, circle.center.y, circle.rad],
        Some(EntityGeom::Arc(arc)) => vec![
            arc.circle.center.x,
            arc.circle.center.y,
            arc.circle.rad,
            arc.start_angle,
            arc.end_angle,
        ],
        Some(EntityGeom::Ellipse(ellipse)) => {
            vec![ellipse.center.x, ellipse.center.y, ellipse.radmin]
        }
        Some(EntityGeom::Spline { curve, .. }) => {
            curve.poles.iter().flat_map(|point| [point.x, point.y]).collect()
        }
        None => Vec::new(),
    }
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
    originals: &HashMap<Handle, std::sync::Arc<EntityType>>,
) -> Vec<Rc<dyn Constraint>> {
    if !c.enabled || !refs_share_supported_plane(document, &c.refs) {
        return Vec::new();
    }

    let whole_line = |sys: &mut System, cache: &mut HashMap<_, _>, r: ParametricRef| {
        let geometry = resolve_ref(document, sys, cache, r)?;
        match geometry {
            EntityGeom::Line(line) | EntityGeom::Ray(line) | EntityGeom::XLine(line) => Some(line),
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
            | EntityGeom::TextLine(_)
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
    let directional_line =
        |sys: &mut System, cache: &mut HashMap<_, _>, r: ParametricRef| {
            let geometry = resolve_ref(document, sys, cache, r)?;
            let minor_axis = matches!(
                r.directional_axis(),
                Some(super::parametric_constraints::DirectionalAxis::EllipseMinor)
            );
            let line = match geometry {
                EntityGeom::TextLine(line)
                    if matches!(
                        r.directional_axis(),
                        Some(super::parametric_constraints::DirectionalAxis::TextBaseline)
                    ) => line,
                EntityGeom::Line(line) | EntityGeom::Ray(line) | EntityGeom::XLine(line)
                    if r.marker.is_none() => line,
                EntityGeom::Polyline { .. } => geometry.line_segment(r.segment_index()?)?,
                EntityGeom::Ellipse(ellipse)
                    if matches!(
                        r.directional_axis(),
                        Some(
                            super::parametric_constraints::DirectionalAxis::EllipseMajor
                                | super::parametric_constraints::DirectionalAxis::EllipseMinor
                        )
                    ) => GLine {
                        p1: ellipse.center,
                        p2: ellipse.focus1,
                    },
                _ => return None,
            };
            Some((line, minor_axis))
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
        ConstraintKind::Horizontal | ConstraintKind::Vertical => {
            let fallback = if c.kind == ConstraintKind::Vertical {
                Vector3::UNIT_Y
            } else {
                Vector3::UNIT_X
            };
            let direction = c.axis_direction.unwrap_or(fallback);
            let length = direction.x.hypot(direction.y);
            let (dx, dy) = if length > 1.0e-12 {
                (direction.x / length, direction.y / length)
            } else {
                (fallback.x, fallback.y)
            };
            let datum = GLine {
                p1: GPoint::new(sys.add_param(0.0, true), sys.add_param(0.0, true)),
                p2: GPoint::new(sys.add_param(dx, true), sys.add_param(dy, true)),
            };
            match c.refs.as_slice() {
                [reference] => directional_line(sys, cache, *reference)
                    .map(|(line, minor_axis)| {
                        if minor_axis {
                            vec![Rc::new(PerpendicularConstraint::new(
                                sys.store(),
                                line,
                                datum,
                            )) as Rc<dyn Constraint>]
                        } else {
                            vec![Rc::new(ParallelConstraint::new(sys.store(), line, datum))
                                as Rc<dyn Constraint>]
                        }
                    })
                    .unwrap_or_default(),
                [a, b] => match (point_ref(sys, cache, *a), point_ref(sys, cache, *b)) {
                    (Some(a), Some(b)) => vec![Rc::new(ParallelConstraint::new(
                        sys.store(),
                        GLine { p1: a, p2: b },
                        datum,
                    ))],
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            }
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
            let (Some((fixed, fixed_minor)), Some((moving, moving_minor))) =
                (directional_line(sys, cache, *a), directional_line(sys, cache, *b))
            else {
                return Vec::new();
            };
            if fixed_minor ^ moving_minor {
                vec![Rc::new(ParallelConstraint::new(sys.store(), moving, fixed))]
            } else {
                vec![Rc::new(PerpendicularConstraint::new(
                    sys.store(),
                    moving,
                    fixed,
                ))]
            }
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
                EntityGeom::TextLine(l)
                | EntityGeom::Line(l)
                | EntityGeom::Ray(l)
                | EntityGeom::XLine(l) => {
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
            let Some(mirror) = whole_line(sys, cache, *m) else {
                return Vec::new();
            };
            if let (Some(pa), Some(pb)) =
                (point_ref(sys, cache, *a), point_ref(sys, cache, *b))
            {
                let pair = GLine { p1: pa, p2: pb };
                return vec![
                    Rc::new(MidpointOnLine::new(pair, mirror)),
                    Rc::new(PerpendicularConstraint::new(sys.store(), pair, mirror)),
                ];
            }
            let (Some(first), Some(second)) = (
                resolve_ref(document, sys, cache, *a),
                resolve_ref(document, sys, cache, *b),
            ) else {
                return Vec::new();
            };
            match (
                as_circle_or_line(first, *a),
                as_circle_or_line(second, *b),
            ) {
                (CircleOrLine::Line(first), CircleOrLine::Line(second)) => {
                    vec![Rc::new(SymmetricLineDirections::new(
                        sys.store(),
                        first,
                        second,
                        mirror,
                    ))]
                }
                (CircleOrLine::Circle(first), CircleOrLine::Circle(second)) => {
                    let centers = GLine {
                        p1: first.center,
                        p2: second.center,
                    };
                    vec![
                        Rc::new(MidpointOnLine::new(centers, mirror)),
                        Rc::new(PerpendicularConstraint::new(
                            sys.store(),
                            centers,
                            mirror,
                        )),
                        Rc::new(Equal::new(second.rad, first.rad, 1.0)),
                    ]
                }
                (CircleOrLine::Ellipse(first), CircleOrLine::Ellipse(second)) => {
                    let centers = GLine {
                        p1: first.center,
                        p2: second.center,
                    };
                    let first_axis = GLine {
                        p1: first.center,
                        p2: first.focus1,
                    };
                    let second_axis = GLine {
                        p1: second.center,
                        p2: second.focus1,
                    };
                    vec![
                        Rc::new(MidpointOnLine::new(centers, mirror)),
                        Rc::new(PerpendicularConstraint::new(
                            sys.store(),
                            centers,
                            mirror,
                        )),
                        Rc::new(SymmetricLineDirections::new(
                            sys.store(),
                            first_axis,
                            second_axis,
                            mirror,
                        )),
                        Rc::new(EqualMajorAxesConic::new(
                            GConic::Ellipse(first),
                            GConic::Ellipse(second),
                        )),
                        Rc::new(Equal::new(second.radmin, first.radmin, 1.0)),
                    ]
                }
                _ => Vec::new(),
            }
        }
        ConstraintKind::Fixed => {
            let Some(r) = c.refs.first() else {
                return Vec::new();
            };
            let pinned = fixed_pin_params(document, sys, cache, *r);
            // A persistent Fixed holds the *pre-edit* geometry: after MOVE /
            // STRETCH / a grip drag the entity in `document` is already
            // displaced, so the target comes from the caller's retained
            // original when there is one — the entity's own live values
            // otherwise (a freshly applied Fixed pins where it stands).
            let mut targets: Option<Vec<f64>> = None;
            if let Some(original) = originals.get(&r.entity) {
                let mut original_cache = HashMap::new();
                if let Some(geometry) = register_entity_geom(sys, r.entity, original.as_ref()) {
                    original_cache.insert(r.entity, geometry);
                    let params = fixed_pin_params(document, sys, &mut original_cache, *r);
                    if params.len() == pinned.len() {
                        let store = sys.store();
                        targets = Some(params.iter().map(|param| store.get(*param)).collect());
                    }
                }
            }
            let targets = targets.unwrap_or_else(|| {
                let store = sys.store();
                pinned.iter().map(|param| store.get(*param)).collect()
            });
            pinned
                .into_iter()
                .zip(targets)
                .map(|(param, value)| {
                    Rc::new(Equal::new(param, sys.add_param(value, true), 1.0))
                        as Rc<dyn Constraint>
                })
                .collect()
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

fn smooth_refs(refs: &[ParametricRef]) -> Option<(ParametricRef, ParametricRef, ParametricRef)> {
    match refs {
        [source, target_endpoint] => Some((
            *source,
            *target_endpoint,
            ParametricRef::whole(target_endpoint.entity),
        )),
        [source, target_endpoint, target_curve]
            if target_endpoint.entity == target_curve.entity =>
        {
            Some((*source, *target_endpoint, *target_curve))
        }
        _ => None,
    }
}

fn smooth_target_planar_curve(
    entity: &EntityType,
    curve_reference: ParametricRef,
) -> Option<cadkernel::space::PlanarCurve> {
    let planar = crate::entities::curve::entity_curve(entity)?;
    let Some(segment) = curve_reference.segment_index() else {
        return (!planar.is_closed()).then_some(planar);
    };
    let curve = planar.curve.segments().into_iter().nth(segment)?;
    Some(cadkernel::space::PlanarCurve::new(planar.plane, curve))
}

fn smooth_endpoint_parameter(
    entity: &EntityType,
    endpoint_reference: ParametricRef,
    curve: &cadkernel::space::PlanarCurve,
) -> Option<f64> {
    let marker = endpoint_reference.marker?;
    let endpoint = super::parametric_constraints::resolve_point(entity, marker)?;
    let endpoint = cadkernel::space::Vec3::new(endpoint.x, endpoint.y, endpoint.z);
    let start = cadkernel::space::Vec3::from(curve.point_at(0.0));
    let end = cadkernel::space::Vec3::from(curve.point_at(1.0));
    Some(if endpoint.distance(start) <= endpoint.distance(end) {
        0.0
    } else {
        1.0
    })
}

fn smooth_target_jet(
    entity: &EntityType,
    endpoint_reference: ParametricRef,
    curve_reference: ParametricRef,
) -> Option<cadkernel::space::CurveJet> {
    if curve_reference.segment_index().is_none() {
        if let EntityType::Spline(spline) = entity {
            let endpoint = match endpoint_reference.marker {
                Some(0) => cadkernel::space::SplineEnd::Start,
                Some(1) => cadkernel::space::SplineEnd::End,
                _ => return None,
            };
            let nurbs = crate::entities::spline::nurbs3(spline)?;
            return cadkernel::space::CurveJet::from_nurbs(&nurbs, endpoint);
        }
    }

    let curve = smooth_target_planar_curve(entity, curve_reference)?;
    let parameter = smooth_endpoint_parameter(entity, endpoint_reference, &curve)?;

    let point = curve.point_at(parameter);
    let tangent = curve.tangent_at(parameter);
    match &curve.curve {
        cadkernel::geom2d::Curve::Line(_) => {
            cadkernel::space::CurveJet::linear(point, tangent)
        }
        cadkernel::geom2d::Curve::Arc(arc) => cadkernel::space::CurveJet::circular(
            point,
            tangent,
            curve.plane.point_at(arc.centre),
        ),
        _ => None,
    }
}

fn smooth_spline_plane(spline: &acadrust::entities::Spline) -> Option<cadkernel::space::Plane> {
    let source = if crate::entities::spline::uses_fit_method(spline) {
        &spline.fit_points
    } else {
        &spline.control_points
    };
    let origin = source.first()?;
    let origin_vector = cadkernel::space::Vec3::new(origin.x, origin.y, origin.z);
    let along = source
        .iter()
        .skip(1)
        .map(|point| cadkernel::space::Vec3::new(point.x, point.y, point.z) - origin_vector)
        .find(|vector| vector.length_squared() > 1.0e-24)?;
    let normal = source
        .iter()
        .skip(1)
        .map(|point| cadkernel::space::Vec3::new(point.x, point.y, point.z) - origin_vector)
        .map(|vector| along.cross(vector))
        .find(|vector| vector.length_squared() > 1.0e-24)?;
    let plane = cadkernel::space::Plane::orthonormal(
        origin_vector.to_array(),
        along.to_array(),
        normal.to_array(),
    )?;
    let points: Vec<_> = source.iter().map(|point| [point.x, point.y, point.z]).collect();
    let tolerance = cadkernel::space::coplanarity_tolerance(&points);
    source
        .iter()
        .all(|point| plane.contains([point.x, point.y, point.z], tolerance))
        .then_some(plane)
}

fn smooth_entity_plane(
    entity: &EntityType,
    curve_reference: ParametricRef,
) -> Option<cadkernel::space::Plane> {
    if let EntityType::Spline(spline) = entity {
        return crate::entities::curve::entity_curve(entity)
            .map(|curve| curve.plane)
            .or_else(|| smooth_spline_plane(spline));
    }
    smooth_target_planar_curve(entity, curve_reference).map(|curve| curve.plane)
}

fn smooth_refs_share_plane(
    document: &acadrust::CadDocument,
    source_ref: ParametricRef,
    target_curve_ref: ParametricRef,
) -> bool {
    let Some(source_entity) = document.get_entity(source_ref.entity) else {
        return false;
    };
    let Some(source_plane) = smooth_entity_plane(source_entity, ParametricRef::whole(source_ref.entity))
    else {
        return false;
    };
    let Some(target_entity) = document.get_entity(target_curve_ref.entity) else {
        return false;
    };
    let Some(target_plane) = smooth_entity_plane(target_entity, target_curve_ref) else {
        return false;
    };
    let points = [source_plane.origin, target_plane.origin];
    let tolerance = cadkernel::space::coplanarity_tolerance(&points);
    if let EntityType::Line(line) = target_entity {
        return source_plane
            .contains([line.start.x, line.start.y, line.start.z], tolerance)
            && source_plane
                .contains([line.end.x, line.end.y, line.end.z], tolerance);
    }
    let (Some(source_normal), Some(target_normal)) =
        (source_plane.normal(), target_plane.normal())
    else {
        return false;
    };
    cadkernel::space::Vec3::from(source_normal)
        .cross(cadkernel::space::Vec3::from(target_normal))
        .length()
        <= 1.0e-9
        && source_plane.contains(target_plane.origin, tolerance)
        && target_plane.contains(source_plane.origin, tolerance)
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
        let Some((source_ref, target_ref, target_curve_ref)) =
            smooth_refs(&constraint.refs)
        else {
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
        let Some(target_jet) = smooth_target_jet(&target, target_ref, target_curve_ref) else {
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
        (EntityType::Text(_) | EntityType::MText(_), None) => {
            let reference = ParametricRef::text_baseline(entity.common().handle);
            let [start, end] =
                super::parametric_constraints::directional_axis_endpoints(entity, reference)?;
            Some((end - start).length())
        }
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

/// Hold the distance between two points at `length`.
///
/// `P2PDistance` divides by the current distance, so a zero-length segment
/// gives the solver a NaN Jacobian. Keep coincident endpoints together with
/// per-axis differences instead.
fn retain_segment_length(
    sys: &mut System,
    a: GPoint,
    b: GPoint,
    current_length: f64,
    length: f64,
) {
    if current_length > f64::EPSILON && length > f64::EPSILON {
        let target = sys.add_param(length, true);
        sys.add_constraint(Rc::new(P2PDistance::new(a, b, target)));
        return;
    }
    let dx = sys.add_param(0.0, true);
    let dy = sys.add_param(0.0, true);
    sys.add_constraint(Rc::new(Difference::new(a.x, b.x, dx)));
    sys.add_constraint(Rc::new(Difference::new(a.y, b.y, dy)));
}

/// Visit vertices and segments in stored drawing order, including the closing edge last.
fn polyline_ref_order(
    document: &acadrust::CadDocument,
    reference: ParametricRef,
) -> Option<(u64, usize)> {
    if !matches!(document.get_entity(reference.entity),
        Some(EntityType::LwPolyline(_) | EntityType::Polyline2D(_)))
    {
        return None;
    }
    let position = if let Some(index) = reference.segment_index()
        .or_else(|| reference.segment_midpoint_index())
        .or_else(|| reference.segment_center_index())
    {
        2 * index + 1
    } else {
        2 * usize::try_from(reference.marker?).ok()?
    };
    Some((reference.entity.value(), position))
}

fn constraints_in_drawing_order<'a>(
    document: &acadrust::CadDocument,
    set: &'a ParametricConstraintSet,
) -> Vec<&'a ParametricConstraint> {
    let mut constraints: Vec<_> = set.constraints.iter().collect();
    // A relation is visited when its last referenced element is reached.
    // Keep reference roles and persisted constraint IDs unchanged.
    constraints.sort_by_cached_key(|constraint| constraint.refs.iter()
        .filter_map(|reference| polyline_ref_order(document, *reference)).max());
    constraints
}

/// Propagate explicit axis constraints through parallel/perpendicular relations.
fn constrained_line_axes(constraints: &[&ParametricConstraint]) -> HashMap<ParametricRef, bool> {
    let mut axes = HashMap::new();
    for c in constraints.iter().filter(|c| c.enabled) {
        if let [reference] = c.refs.as_slice() {
            if matches!(c.kind, ConstraintKind::Horizontal | ConstraintKind::Vertical) {
                let fallback = if c.kind == ConstraintKind::Vertical {
                    Vector3::UNIT_Y
                } else {
                    Vector3::UNIT_X
                };
                let direction = c.axis_direction.unwrap_or(fallback);
                let length = direction.x.hypot(direction.y);
                if length > 1.0e-12 {
                    let (x, y) = (direction.x.abs() / length, direction.y.abs() / length);
                    if x >= 1.0 - 1.0e-10 {
                        axes.insert(*reference, false);
                    } else if y >= 1.0 - 1.0e-10 {
                        axes.insert(*reference, true);
                    }
                }
            }
        }
    }
    loop {
        let before = axes.len();
        for c in constraints.iter().filter(|c| c.enabled) {
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

/// Solve the scope through the kernel and return changed geometry plus diagnostics.
/// Failed solves still return DOF/conflicts, but never write back geometry.
/// Returns `None` when no supported geometry or smooth constraint can be built.
fn solve_scope(
    document: &acadrust::CadDocument,
    drawing_params: &ParameterTable,
    set: &ParametricConstraintSet,
    driven_refs: &[ParametricRef],
    retain_size: bool,
    retain_lengths: bool,
    retained_before: &HashMap<Handle, std::sync::Arc<EntityType>>,
    initial_fixed_refs: &[ParametricRef],
    originals: &HashMap<Handle, std::sync::Arc<EntityType>>,
) -> Option<SolveResult> {
    let params = if set.local_parameters.is_empty() {
        drawing_params
    } else {
        &set.local_parameters
    };
    let mut sys = System::new();
    let mut cache: HashMap<Handle, EntityGeom> = HashMap::new();
    let constraints = constraints_in_drawing_order(document, set);
    // A persistent Fixed outranks any edit: a point it holds is never a
    // driven grip / STRETCH anchor, and a moved entity it holds gets no
    // temporary whole-entity pin — the Fixed one, targeting the retained
    // original, pulls it back instead (a MOVE leaves a fixed object, or the
    // fixed points of a partly fixed one, where they were).
    let fixed_refs: Vec<ParametricRef> = constraints
        .iter()
        .filter(|c| c.enabled && c.kind == ConstraintKind::Fixed)
        .filter_map(|c| c.refs.first().copied())
        .collect();
    let held_by_fixed = |r: &ParametricRef| {
        fixed_refs.iter().any(|f| {
            f.entity == r.entity
                && (f.marker.is_none()
                    || f.marker == r.marker
                    || f.segment_index().is_some_and(|index| {
                        matches!(r.marker, Some(marker)
                            if marker == index as i32 || marker == index as i32 + 1)
                    }))
        })
    };
    // A moved entity a Fixed holds only in part gets no temporary pin and
    // (below) no size retention, so its free points simply stay where the
    // edit put them while the fixed ones pull their own coordinates back —
    // a MOVE on a line with one fixed end stretches the line instead of
    // sliding it — and any other constraint on it reconciles the rest.
    let driven_refs: Vec<ParametricRef> =
        driven_refs.iter().copied().filter(|r| !held_by_fixed(r)).collect();
    let driven_refs = driven_refs.as_slice();
    let fixed_touches = |handle: Handle| fixed_refs.iter().any(|f| f.entity == handle);
    let line_axes = if retain_lengths { HashMap::new() } else { constrained_line_axes(&constraints) };
    // Map partition-local kernel rows back to persistent constraint IDs by Rc identity.
    let mut owner: Vec<(Rc<dyn Constraint>, ConstraintId)> = Vec::new();

    for c in &constraints {
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
            .then(|| constraints.iter().find_map(|connection| {
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
            originals,
        ) {
            owner.push((constraint.clone(), c.id));
            sys.add_constraint(constraint);
        }
    }

    // Ordered two-object constraints use the first pick as a reference for
    // their initial placement. Temporary Fixed equations stay out of `owner`
    // and the persistent native graph. Symmetric references are handled as
    // exact driven anchors below, so they are excluded from this approximate
    // equality-equation path.
    let symmetric_constraints = constraints
        .iter()
        .filter(|constraint| {
            constraint.enabled
                && constraint.kind == ConstraintKind::Symmetric
                && constraint.refs.len() == 3
        })
        .collect::<Vec<_>>();
    let temporary_fixed_refs = initial_fixed_refs
        .iter()
        .copied()
        .filter(|reference| {
            !symmetric_constraints
                .iter()
                .any(|constraint| constraint.refs.contains(reference))
        })
        .collect::<Vec<_>>();
    for reference in &temporary_fixed_refs {
        if fixed_touches(reference.entity) {
            continue;
        }
        // A transformed entity with a Horizontal/Vertical relation keeps
        // only its end point where the transform put it; the rest re-aligns
        // to the axis at the retained length (a rotated vertical line stays
        // vertical below its moved end, as in the reference) instead of the
        // whole-entity pin contradicting the axis and failing the solve.
        let axis_pin = reference.marker.is_none().then(|| {
            constraints.iter().find_map(|c| {
                (c.enabled && matches!(c.kind, ConstraintKind::Horizontal | ConstraintKind::Vertical))
                    .then(|| c.refs.iter().find(|r| r.entity == reference.entity))
                    .flatten()
                    .map(|r| match r.directional_axis() {
                        Some(super::parametric_constraints::DirectionalAxis::TextBaseline) => {
                            ParametricRef::point(r.entity, 0)
                        }
                        Some(_) => ParametricRef::center(r.entity),
                        None => ParametricRef::point(
                            r.entity,
                            r.segment_index().map_or(1, |index| index as i32 + 1),
                        ),
                    })
            })
        })
        .flatten();
        let pinned = axis_pin.unwrap_or(*reference);
        // The re-aligned entity keeps the length the transform gave it (a
        // scaled vertical line stays scaled), not its pre-edit length.
        if axis_pin.is_some() {
            let segment = reference
                .segment_index()
                .or_else(|| {
                    constraints.iter().find_map(|c| {
                        c.refs
                            .iter()
                            .find(|r| r.entity == reference.entity)
                            .and_then(|r| r.segment_index())
                    })
                });
            let line = resolve_ref(document, &mut sys, &mut cache, *reference).and_then(|geometry| {
                match (&geometry, segment) {
                    (EntityGeom::Polyline { .. }, Some(index)) => geometry.line_segment(index),
                    (EntityGeom::Line(line) | EntityGeom::Ray(line) | EntityGeom::XLine(line), _) => {
                        Some(*line)
                    }
                    _ => None,
                }
            });
            if let Some(line) = line {
                let length = {
                    let store = sys.store();
                    let dx = store.get(line.p2.x) - store.get(line.p1.x);
                    let dy = store.get(line.p2.y) - store.get(line.p1.y);
                    (dx * dx + dy * dy).sqrt()
                };
                retain_segment_length(&mut sys, line.p1, line.p2, length, length);
            }
        }
        let temporary = ParametricConstraint {
            id: ConstraintId::MAX,
            kind: ConstraintKind::Fixed,
            refs: vec![pinned],
            driving_param: None,
            enabled: true,
            native_origin: None,
            rigid_points: Vec::new(),
            distance_direction_type: distance_direction_type::UNDIRECTED,
            distance_direction: None,
            axis_direction: None,
            angle_sector: angle_sector::PARALLEL_COUNTERCLOCKWISE,
        };
        for constraint in build_constraint(
            document,
            &mut sys,
            &mut cache,
            params,
            &temporary,
            None,
            None,
            &HashMap::new(),
        ) {
            sys.add_constraint(constraint);
        }
    }

    let has_smooth = constraints.iter()
        .any(|constraint| constraint.enabled && constraint.kind == ConstraintKind::Smooth);
    let has_spatial_concentric = constraints.iter().any(|constraint| {
        constraint.enabled
            && constraint.kind == ConstraintKind::Concentric
            && !refs_share_supported_plane(document, &constraint.refs)
            && concentric_refs_share_plane(document, &constraint.refs)
    });
    if cache.is_empty() && !has_smooth && !has_spatial_concentric {
        return None;
    }

    let mut anchors = driven_refs.to_vec();
    for constraint in &symmetric_constraints {
        if let Some(axis) = constraint.refs.get(2).copied() {
            if !anchors.contains(&axis) {
                anchors.push(axis);
            }
        }
        for reference in initial_fixed_refs {
            if constraint.refs[..2].contains(reference) && !anchors.contains(reference) {
                anchors.push(*reference);
            }
        }
    }
    // Equal: the side that is not being edited follows the other's length
    // or radius the way the reference does it — its start (a circle its
    // center) and direction stay and only its end moves — so its size is
    // not retained below, its other points are held and its direction is
    // kept while the edit lasts. A chain of Equal relations follows along.
    let edited = |handle: Handle| {
        driven_refs.iter().any(|reference| reference.entity == handle)
            || initial_fixed_refs.iter().any(|reference| reference.entity == handle)
    };
    let mut equal_followers: Vec<ParametricRef> = Vec::new();
    let mut moving: Vec<Handle> = cache.keys().copied().filter(|handle| edited(*handle)).collect();
    loop {
        let before = equal_followers.len();
        for constraint in &constraints {
            if !constraint.enabled || constraint.kind != ConstraintKind::Equal {
                continue;
            }
            let [a, b] = constraint.refs.as_slice() else { continue };
            let follower = match (moving.contains(&a.entity), moving.contains(&b.entity)) {
                (true, false) => *b,
                (false, true) => *a,
                _ => continue,
            };
            if !equal_followers.contains(&follower) {
                equal_followers.push(follower);
                moving.push(follower.entity);
            }
        }
        if equal_followers.len() == before {
            break;
        }
    }
    for follower in &equal_followers {
        let Some(geometry) = resolve_ref(document, &mut sys, &mut cache, *follower) else {
            continue;
        };
        let mut hold = |anchor: ParametricRef| {
            if !anchors.contains(&anchor) {
                anchors.push(anchor);
            }
        };
        let line = match &geometry {
            EntityGeom::Line(line) => Some(*line),
            EntityGeom::Polyline { .. } => follower
                .segment_index()
                .and_then(|index| geometry.line_segment(index)),
            _ => None,
        };
        match &geometry {
            EntityGeom::Circle(_) | EntityGeom::Arc(_) => {
                hold(ParametricRef::center(follower.entity));
            }
            EntityGeom::Line(_) => hold(ParametricRef::point(follower.entity, 0)),
            EntityGeom::Polyline { points, .. } => {
                let moving_vertex = follower
                    .segment_index()
                    .map(|index| (index + 1) % points.len().max(1));
                for index in (0..points.len()).filter(|index| Some(*index) != moving_vertex) {
                    hold(ParametricRef::point(follower.entity, index as i32));
                }
            }
            _ => {}
        }
        if let Some(line) = line {
            let (dx, dy) = {
                let store = sys.store();
                (
                    store.get(line.p2.x) - store.get(line.p1.x),
                    store.get(line.p2.y) - store.get(line.p1.y),
                )
            };
            if dx.hypot(dy) > f64::EPSILON {
                let datum = GLine {
                    p1: GPoint::new(sys.add_param(0.0, true), sys.add_param(0.0, true)),
                    p2: GPoint::new(sys.add_param(dx, true), sys.add_param(dy, true)),
                };
                let parallel = ParallelConstraint::new(sys.store(), line, datum);
                sys.add_constraint(Rc::new(parallel));
            }
        }
    }
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
        axis_lines.sort_by_key(|(reference, _, _)| (reference.entity.value(), reference.segment_index()));
        let driven_points: Vec<_> = driven_refs.iter().filter_map(|reference|
            resolve_constraint_point(document, &mut sys, &mut cache, *reference)).collect();
        let coincident: Vec<_> = constraints.iter().filter(|c|
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
        let perpendicular: Vec<_> = constraints.iter().filter(|c|
            c.enabled && c.kind == ConstraintKind::Perpendicular && c.refs.len() == 2).collect();
        let mut retained_axes = HashSet::new();
        for point in &driven_points {
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
            let editable = if free_legs.is_empty() { &incident } else { &free_legs };
            retained_axes.extend([false, true].into_iter().filter(|axis|
                !editable.iter().any(|(_, _, vertical)| vertical == axis)));
            for (reference, line, vertical) in editable.iter().copied() {
                let opposite = if group.contains(&line.p1) { line.p2 } else { line.p1 };
                if !driven_points.contains(&opposite) {
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
            let paired_edge_is_dragged = constraints.iter().any(|c| {
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
                && !axis_lines.iter().any(|(line_ref, _, _)| line_ref.entity == reference.entity)
            {
                anchors.push(ParametricRef::point(reference.entity, 1 - reference.marker.unwrap()));
            }
        }
    }

    // Grip edits retain radii while connected lines resize to meet their
    // endpoints. Other edits can retain line lengths as well.
    if retain_size {
        for (handle, geom) in &cache {
            // A Fixed entity's size is whatever its pins say: whole → all
            // pinned anyway, partial → the reference lets it resize (MOVE
            // on a one-end-fixed line shortens it).
            if fixed_touches(*handle)
                || initial_fixed_refs
                    .iter()
                    .any(|reference| reference.entity == *handle && reference.marker.is_none())
                || symmetric_constraints.iter().any(|constraint| {
                    constraint.refs.get(2).is_some_and(|axis| axis.entity == *handle)
                })
                || equal_followers.iter().any(|follower| follower.entity == *handle)
            {
                continue;
            }
            match geom {
                EntityGeom::Line(line) | EntityGeom::TextLine(line) if retain_lengths => {
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
                    retain_segment_length(&mut sys, line.p1, line.p2, current_length, length);
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
                        retain_segment_length(&mut sys, a, b, current_length, length);
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
                EntityGeom::Ellipse(ellipse) => {
                    let (focus_distance, minor_radius) = {
                        let store = sys.store();
                        let dx = store.get(ellipse.focus1.x) - store.get(ellipse.center.x);
                        let dy = store.get(ellipse.focus1.y) - store.get(ellipse.center.y);
                        ((dx * dx + dy * dy).sqrt(), store.get(ellipse.radmin))
                    };
                    let focus_target = sys.add_param(focus_distance, true);
                    let minor_target = sys.add_param(minor_radius, true);
                    sys.add_constraint(Rc::new(P2PDistance::new(
                        ellipse.center,
                        ellipse.focus1,
                        focus_target,
                    )));
                    sys.add_constraint(Rc::new(Equal::new(
                        ellipse.radmin,
                        minor_target,
                        1.0,
                    )));
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

    // Ellipse rules (same motivation as arc rules, different mechanism):
    // `cadkernel_constraints::geo::Ellipse` stores `focus1` as an absolute point, not an
    // offset from `center`. If some constraint (e.g. Concentric) pulls only
    // on `center` and nothing references `focus1`/`radmin`, the solver
    // correctly leaves `focus1`'s *absolute* coordinates untouched — but
    // `center` moved and `focus1` didn't, so the derived vector
    // `focus1 - center` (which is what `major_axis` direction/length and
    // `minor_axis_ratio` are actually computed from on write-back) changes
    // anyway, making the ellipse appear to rotate and reshape even though
    // no shape parameter was itself pulled on. Most ellipse relations keep
    // that vector exact. A directional axis relation instead preserves its
    // length while allowing the vector to rotate into the requested angle.
    for (handle, geom) in &cache {
        let EntityGeom::Ellipse(el) = geom else {
            continue;
        };
        let symmetric = set.constraints.iter().any(|constraint| {
            constraint.enabled
                && constraint.kind == ConstraintKind::Symmetric
                && constraint.refs.get(..2).is_some_and(|references| {
                    references.iter().any(|reference| {
                        reference.entity == *handle && reference.marker.is_none()
                    })
                })
        });
        if symmetric {
            continue;
        }
        let (cx, cy, fx, fy) = {
            let store = sys.store();
            (
                store.get(el.center.x),
                store.get(el.center.y),
                store.get(el.focus1.x),
                store.get(el.focus1.y),
            )
        };
        let directional = set.constraints.iter().any(|constraint| {
            constraint.enabled
                && matches!(
                    constraint.kind,
                    ConstraintKind::Horizontal
                        | ConstraintKind::Vertical
                        | ConstraintKind::Perpendicular
                )
                && constraint.refs.iter().any(|reference| {
                    reference.entity == *handle && reference.directional_axis().is_some()
                })
        });
        if directional && !retain_size {
            let distance = sys.add_param((fx - cx).hypot(fy - cy), true);
            sys.add_constraint(Rc::new(P2PDistance::new(
                el.center,
                el.focus1,
                distance,
            )));
            let minor_value = sys.store().get(el.radmin);
            let minor = sys.add_param(minor_value, true);
            sys.add_constraint(Rc::new(Equal::new(el.radmin, minor, 1.0)));
        } else if !directional {
            let dx = sys.add_param(fx - cx, true);
            let dy = sys.add_param(fy - cy, true);
            sys.add_constraint(Rc::new(Difference::new(el.center.x, el.focus1.x, dx)));
            sys.add_constraint(Rc::new(Difference::new(el.center.y, el.focus1.y, dy)));
        }
    }

    // Text keeps its displayed baseline rigid during ordinary point edits.
    // A perpendicular baseline reference keeps only the span length so the
    // command can rotate the text about its insertion point.
    for (handle, geom) in &cache {
        let EntityGeom::TextLine(line) = geom else {
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
        let directional = set.constraints.iter().any(|constraint| {
            constraint.enabled
                && matches!(
                    constraint.kind,
                    ConstraintKind::Horizontal
                        | ConstraintKind::Vertical
                        | ConstraintKind::Perpendicular
                )
                && constraint.refs.iter().any(|reference| {
                    reference.entity == *handle
                        && matches!(
                            reference.directional_axis(),
                            Some(super::parametric_constraints::DirectionalAxis::TextBaseline)
                        )
                })
        });
        if directional && !retain_size {
            let length = (x2 - x1).hypot(y2 - y1);
            retain_segment_length(&mut sys, line.p1, line.p2, length, length);
        } else if !directional {
            let dx = sys.add_param(x2 - x1, true);
            let dy = sys.add_param(y2 - y1, true);
            sys.add_constraint(Rc::new(Difference::new(line.p1.x, line.p2.x, dx)));
            sys.add_constraint(Rc::new(Difference::new(line.p1.y, line.p2.y, dy)));
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
        } else if reference.marker.is_some() {
            resolve_constraint_point(document, &mut sys, &mut cache, *reference)
                .map(|point| vec![point.x, point.y])
                .unwrap_or_default()
        } else {
            match resolve_ref(document, &mut sys, &mut cache, *reference) {
                Some(EntityGeom::Point(point)) => vec![point.x, point.y],
                Some(EntityGeom::TextLine(line))
                | Some(EntityGeom::Line(line))
                | Some(EntityGeom::Ray(line))
                | Some(EntityGeom::XLine(line)) => {
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
            sys.store_mut().set_driven(parameter, true);
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
            EntityGeom::TextLine(_)
            | EntityGeom::Line(_)
            | EntityGeom::Ray(_)
            | EntityGeom::XLine(_) => 4,
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
            (EntityType::Text(text), EntityGeom::TextLine(geometry)) => {
                let mut updated = text.clone();
                let next = Vector3::new(
                    store.get(geometry.p1.x),
                    store.get(geometry.p1.y),
                    text.insertion_point.z,
                );
                let end = Vector3::new(
                    store.get(geometry.p2.x),
                    store.get(geometry.p2.y),
                    text.insertion_point.z,
                );
                let delta = next - text.insertion_point;
                let direction = end - next;
                let rotation = direction.y.atan2(direction.x);
                let aligned = matches!(
                    text.horizontal_alignment,
                    acadrust::entities::TextHorizontalAlignment::Aligned
                        | acadrust::entities::TextHorizontalAlignment::Fit
                );
                let current_rotation = if aligned {
                    text.alignment_point
                        .filter(|point| (*point - text.insertion_point).length_squared() > 1.0e-18)
                        .map(|point| {
                            (point.y - text.insertion_point.y)
                                .atan2(point.x - text.insertion_point.x)
                        })
                        .unwrap_or(text.rotation)
                } else {
                    text.rotation
                };
                if delta.length() > MOVE_EPS
                    || (rotation - current_rotation).abs() > MOVE_EPS
                {
                    updated.insertion_point = next;
                    if aligned {
                        updated.alignment_point = Some(end);
                    } else {
                        if let Some(alignment) = &mut updated.alignment_point {
                            *alignment = *alignment + delta;
                        }
                        updated.rotation = rotation;
                    }
                    results.push((handle, EntityType::Text(updated)));
                }
            }
            (EntityType::MText(text), EntityGeom::TextLine(geometry)) => {
                let mut updated = text.clone();
                let next = Vector3::new(
                    store.get(geometry.p1.x),
                    store.get(geometry.p1.y),
                    text.insertion_point.z,
                );
                let dx = store.get(geometry.p2.x) - next.x;
                let dy = store.get(geometry.p2.y) - next.y;
                let rotation = dy.atan2(dx);
                if (next - text.insertion_point).length() > MOVE_EPS
                    || (rotation - text.rotation).abs() > MOVE_EPS
                {
                    updated.insertion_point = next;
                    updated.rotation = rotation;
                    updated.dwg_x_direction = Some(Vector3::new(
                        rotation.cos(),
                        rotation.sin(),
                        0.0,
                    ));
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
                let mut changed = crate::entities::spline::uses_fit_method(spline)
                    || spline.control_points.len() != curve.poles.len();
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
                    if crate::entities::spline::uses_fit_method(spline) {
                        updated.cv_frame_visible = true;
                    }
                    updated.degree = curve.degree as i32;
                    updated.knots = curve.knots.clone();
                    updated.control_points = control_points;
                    updated.weights = curve.weights.iter().map(|id| store.get(*id)).collect();
                    if !updated.fit_points.is_empty() {
                        if let (Some(first), Some(point)) =
                            (updated.fit_points.first_mut(), updated.control_points.first())
                        {
                            *first = *point;
                        }
                        if let (Some(last), Some(point)) =
                            (updated.fit_points.last_mut(), updated.control_points.last())
                        {
                            *last = *point;
                        }
                    }
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
    apply_spatial_concentric_constraints(
        document,
        set,
        driven_refs,
        initial_fixed_refs,
        &mut results,
    );
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
        if kind == ConstraintKind::Smooth {
            let Some((source_ref, target_ref, target_curve_ref)) = smooth_refs(refs) else {
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
            let Some(_) = target_ref.marker else {
                return Err("Smooth requires a target endpoint.");
            };
            let Some(target) = self.document.get_entity(target_ref.entity) else {
                return Err("Smooth target does not exist.");
            };
            if !smooth_refs_share_plane(&self.document, source_ref, target_curve_ref) {
                return Err("Smooth requires both curves to lie on the same plane.");
            }
            let Some(target_jet) = smooth_target_jet(target, target_ref, target_curve_ref) else {
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
        if kind == ConstraintKind::Concentric {
            if !concentric_refs_share_plane(&self.document, refs) {
                return Err(
                    "Concentric requires two supported circular curves on the same plane.",
                );
            }
            if !refs_share_supported_plane(&self.document, refs) {
                if refs[0].entity == refs[1].entity {
                    return Err(
                        "Concentric cannot move two curved segments of the same object on this plane.",
                    );
                }
                return Ok(());
            }
        }
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
            axis_direction: None,
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
            &HashMap::new(),
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
        self.refresh_parametric_constraints_with_initial_policy(
            changes,
            driven_refs,
            retain_size,
            retained_originals,
            &[],
        )
    }

    pub(crate) fn refresh_parametric_constraints_with_initial_policy(
        &mut self,
        changes: &[(Handle, ChangeKind)],
        driven_refs: &[ParametricRef],
        retain_size: bool,
        retained_originals: &[(Handle, EntityType)],
        initial_fixed_refs: &[ParametricRef],
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

        // Pre-edit images of everything this transaction touched: what a
        // persistent Fixed pins to, and (under CONSTRAINTSOLVEMODE) what
        // sizes are retained from.
        let mut originals: HashMap<Handle, std::sync::Arc<EntityType>> = self
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
            originals.insert(*handle, std::sync::Arc::new(entity.clone()));
        }
        let retained_before: HashMap<Handle, std::sync::Arc<EntityType>> = if retain_size {
            originals.clone()
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
                initial_fixed_refs,
                &originals,
            ) else {
                continue;
            };
            if initial_fixed_refs.is_empty() {
                self.parametric_constraints[i].dof = Some(dof);
                self.parametric_constraints[i].conflicts = conflicts;
            } else {
                self.parametric_constraints[i].dof = None;
                self.parametric_constraints[i].conflicts.clear();
            }
            for (handle, new_entity) in solved {
                // An edit the constraints can only satisfy by collapsing the
                // entity (a rotated line whose start is fixed and direction
                // constrained) is refused: the entity goes back to its
                // pre-edit shape, as the reference does.
                let new_entity = match originals.get(&handle) {
                    Some(original)
                        if collapsed_by_solve(&new_entity)
                            && !collapsed_by_solve(original.as_ref()) =>
                    {
                        original.as_ref().clone()
                    }
                    _ => new_entity,
                };
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
                &[],
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

    fn spatial_circle(center: Vector3, radius: f64) -> acadrust::entities::Circle {
        let normal = Vector3::UNIT_Y;
        let (x, y, z) = crate::scene::view::transform::wcs_point_to_ocs(
            (center.x, center.y, center.z),
            (normal.x, normal.y, normal.z),
        );
        let mut circle = acadrust::entities::Circle::from_coords(x, y, z, radius);
        circle.normal = normal;
        circle
    }

    #[test]
    fn polyline_constraint_order_follows_stored_vertices_for_open_and_closed_polylines() {
        use acadrust::entities::{LwPolyline, LwVertex, Polyline2D, Vertex2D};
        for closed in [false, true] {
            let mut scene = Scene::new();
            let points = [(8.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)];
            let mut light = LwPolyline::new();
            light.vertices = points.iter().map(|&(x, y)| LwVertex::from_coords(x, y)).collect();
            light.is_closed = closed;
            let mut heavy = Polyline2D::new();
            heavy.vertices = points.iter().map(|&(x, y)| Vertex2D::new(Vector3::new(x, y, 0.0))).collect();
            heavy.flags.set_closed(closed);
            for entity in [EntityType::LwPolyline(light), EntityType::Polyline2D(heavy)] {
                let handle = scene.add_entity(entity);
                let mut refs = vec![
                    ParametricRef::point(handle, 0),
                    ParametricRef::segment(handle, 0),
                    ParametricRef::point(handle, 1),
                    ParametricRef::segment_midpoint(handle, 1),
                    ParametricRef::point(handle, 2),
                    ParametricRef::segment(handle, 2),
                    ParametricRef::point(handle, 3),
                ];
                if closed {
                    refs.push(ParametricRef::segment(handle, 3));
                }
                let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
                for reference in refs.iter().rev() {
                    set.add(ConstraintKind::Fixed, vec![*reference], None);
                }
                let ordered = super::constraints_in_drawing_order(&scene.document, &set);
                assert_eq!(ordered.iter().map(|c| c.refs[0]).collect::<Vec<_>>(), refs);
                assert_eq!(super::polyline_ref_order(&scene.document, ParametricRef::segment(handle, 1)),
                    super::polyline_ref_order(&scene.document, ParametricRef::segment_midpoint(handle, 1)));
                assert_eq!(set.constraints[0].refs[0], *refs.last().unwrap(), "persistent order must not change");
            }
        }
    }

    #[test]
    fn polyline_relations_follow_the_last_edge_without_swapping_reference_roles() {
        let mut scene = Scene::new();
        let mut polyline = acadrust::entities::LwPolyline::new();
        polyline.vertices = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)]
            .into_iter().map(|(x, y)| acadrust::entities::LwVertex::from_coords(x, y)).collect();
        polyline.is_closed = true;
        let handle = scene.add_entity(EntityType::LwPolyline(polyline));
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        for (a, b) in [(3, 0), (1, 2), (1, 0)] {
            set.add(ConstraintKind::Perpendicular,
                vec![ParametricRef::segment(handle, a), ParametricRef::segment(handle, b)], None);
        }
        let ordered = super::constraints_in_drawing_order(&scene.document, &set);
        assert_eq!(ordered.iter().map(|c| c.id).collect::<Vec<_>>(), vec![2, 1, 0]);
        for constraint in ordered {
            assert_eq!(constraint.refs, set.constraints[constraint.id as usize].refs);
        }
    }

    #[test]
    fn non_polyline_constraint_order_is_unchanged() {
        let mut scene = Scene::new();
        let line = scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::ZERO, Vector3::new(4.0, 0.0, 0.0))));
        let mut set = ParametricConstraintSet::new(ParametricScope::ModelSpace);
        for marker in [1, 0] {
            set.add(ConstraintKind::Fixed, vec![ParametricRef::point(line, marker)], None);
        }
        set.add(ConstraintKind::Horizontal, vec![ParametricRef::whole(line)], None);
        let ordered = super::constraints_in_drawing_order(&scene.document, &set);
        assert_eq!(ordered.iter().map(|c| c.id).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn polyline_preview_satisfies_the_same_constraints_regardless_of_insertion_order() {
        for closed in [false, true] {
            let mut results = Vec::new();
            for reverse in [false, true] {
                let mut scene = Scene::new();
                let mut polyline = acadrust::entities::LwPolyline::new();
                polyline.vertices = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)]
                    .into_iter().map(|(x, y)| acadrust::entities::LwVertex::from_coords(x, y)).collect();
                polyline.is_closed = closed;
                let handle = scene.add_entity(EntityType::LwPolyline(polyline));
                let mut constraints = vec![
                    (ConstraintKind::Horizontal, vec![ParametricRef::segment(handle, 0)]),
                    (ConstraintKind::Perpendicular, vec![ParametricRef::segment(handle, 0), ParametricRef::segment(handle, 1)]),
                    (ConstraintKind::Parallel, vec![ParametricRef::segment(handle, 0), ParametricRef::segment(handle, 2)]),
                ];
                if closed {
                    constraints.push((ConstraintKind::Perpendicular,
                        vec![ParametricRef::segment(handle, 2), ParametricRef::segment(handle, 3)]));
                }
                if reverse { constraints.reverse(); }
                let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
                for (kind, refs) in constraints { set.add(kind, refs, None); }
                let before = scene.document.get_entity(handle).unwrap().clone();
                let Some(EntityType::LwPolyline(polyline)) = scene.document.get_entity_mut(handle) else { panic!("polyline") };
                polyline.vertices[0].location = acadrust::types::Vector2::new(-1.0, -2.0);
                let solved = scene.solve_parametric_constraints_preview(
                    &[handle], &[ParametricRef::point(handle, 0)], true, &[(handle, before)]);
                let entity = &solved.iter().find(|(h, _)| *h == handle).expect("connected vertices must move").1;
                let EntityType::LwPolyline(polyline) = entity else { panic!("polyline") };
                let points: Vec<_> = polyline.vertices.iter().map(|v| v.location).collect();
                assert_eq!(points[0], acadrust::types::Vector2::new(-1.0, -2.0));
                assert!((points[0].y - points[1].y).abs() < 1e-7);
                assert!((points[1].x - points[2].x).abs() < 1e-7);
                assert!((points[2].y - points[3].y).abs() < 1e-7);
                if closed { assert!((points[3].x - points[0].x).abs() < 1e-7); }
                results.push(points);
            }
            for (a, b) in results[0].iter().zip(&results[1]) {
                assert!((a.x - b.x).abs() < 1e-7 && (a.y - b.y).abs() < 1e-7);
            }
        }
    }

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

    #[test]
    fn smooth_constraint_uses_the_selected_polyline_arc_curvature() {
        let mut scene = Scene::new();
        let mut target = acadrust::entities::LwPolyline::new();
        target.vertices = vec![
            acadrust::entities::LwVertex::with_bulge(
                acadrust::types::Vector2::new(0.0, 0.0),
                1.0,
            ),
            acadrust::entities::LwVertex::from_coords(10.0, 0.0),
        ];
        let target = scene.add_entity(EntityType::LwPolyline(target));
        let mut spline = acadrust::entities::Spline::new();
        spline.degree = 3;
        spline.control_points = vec![
            Vector3::new(2.0, 1.0, 0.0),
            Vector3::new(3.0, 2.0, 0.0),
            Vector3::new(4.0, 3.0, 0.0),
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
                    ParametricRef::segment(target, 0),
                ],
                None,
            );

        scene.bump_entities(&[(target, super::ChangeKind::Modified)]);

        let EntityType::Spline(spline) = scene.document.get_entity(spline).unwrap() else {
            panic!("expected spline");
        };
        let curve = crate::entities::spline::nurbs3(spline).unwrap();
        let jet =
            cadkernel::space::CurveJet::from_nurbs(&curve, cadkernel::space::SplineEnd::Start)
                .unwrap();
        assert!(cadkernel::space::Vec3::from(jet.point).length() < 1.0e-9);
        assert!(jet.tangent[0].abs() < 1.0e-9, "jet={jet:?}");
        assert!(
            (cadkernel::space::Vec3::from(jet.curvature).length() - 0.2).abs() < 2.0e-4,
            "jet={jet:?}"
        );
    }

    #[test]
    fn smooth_constraint_accepts_a_stale_normal_on_an_arbitrary_plane() {
        let mut scene = Scene::new();
        let target = scene.add_entity(EntityType::Line(
            acadrust::entities::Line::from_points(
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(5.0, 0.0, 1.0),
            ),
        ));
        let mut source = acadrust::entities::Spline::new();
        source.degree = 3;
        source.normal = Vector3::UNIT_Z;
        source.control_points = vec![
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 2.0),
            Vector3::new(4.0, 0.0, 2.0),
            Vector3::new(5.0, 0.0, 3.0),
        ];
        source.knots = cadkernel::space::clamped_uniform_knots(3, 4);
        let source = scene.add_entity(EntityType::Spline(source));
        let refs = vec![
            ParametricRef::point(source, 0),
            ParametricRef::point(target, 0),
        ];
        assert!(scene
            .validate_parametric_constraint(ConstraintKind::Smooth, &refs, None)
            .is_ok());
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(ConstraintKind::Smooth, refs, None);

        scene.bump_entities(&[(target, super::ChangeKind::Modified)]);

        let EntityType::Spline(source) = scene.document.get_entity(source).unwrap() else {
            panic!("expected spline");
        };
        let curve = crate::entities::spline::nurbs3(source).unwrap();
        let jet =
            cadkernel::space::CurveJet::from_nurbs(&curve, cadkernel::space::SplineEnd::Start)
                .unwrap();
        assert!(cadkernel::space::Vec3::from(jet.point)
            .distance(cadkernel::space::Vec3::new(0.0, 0.0, 1.0))
            < 1.0e-9);
        assert!(jet.tangent[1].abs() < 1.0e-9 && jet.tangent[2].abs() < 1.0e-9);
    }

    #[test]
    fn spatial_concentric_initial_solve_keeps_the_first_circle_fixed() {
        let mut scene = Scene::new();
        let first = scene.add_entity(EntityType::Circle(spatial_circle(Vector3::ZERO, 2.0)));
        let second = scene.add_entity(EntityType::Circle(spatial_circle(
            Vector3::new(5.0, 0.0, 3.0),
            4.0,
        )));
        let first_ref = ParametricRef::center(first);
        let second_ref = ParametricRef::center(second);
        scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Concentric,
                vec![first_ref, second_ref],
                None,
            );

        scene.bump_entities_with_initial_parametric_policy(
            &[
                (first, super::ChangeKind::Modified),
                (second, super::ChangeKind::Modified),
            ],
            &[first_ref],
            true,
        );

        let EntityType::Circle(first_circle) = scene.document.get_entity(first).unwrap() else {
            panic!("expected circle");
        };
        let EntityType::Circle(second_circle) = scene.document.get_entity(second).unwrap() else {
            panic!("expected circle");
        };
        assert!(first_circle.center_wcs().length() < 1.0e-9);
        assert!(second_circle.center_wcs().length() < 1.0e-9);
        assert!((second_circle.radius - 4.0).abs() < 1.0e-12);
    }

    #[test]
    fn spatial_concentric_chain_follows_the_driven_end() {
        let mut scene = Scene::new();
        let handles = [1.0, 2.0, 3.0].map(|radius| {
            scene.add_entity(EntityType::Circle(spatial_circle(Vector3::ZERO, radius)))
        });
        let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Concentric,
            vec![ParametricRef::center(handles[0]), ParametricRef::center(handles[1])],
            None,
        );
        set.add(
            ConstraintKind::Concentric,
            vec![ParametricRef::center(handles[1]), ParametricRef::center(handles[2])],
            None,
        );
        let driven_center = Vector3::new(8.0, 0.0, -3.0);
        *scene.document.get_entity_mut(handles[2]).unwrap() =
            EntityType::Circle(spatial_circle(driven_center, 3.0));

        scene.bump_entities_with_parametric_driven(
            &[(handles[2], super::ChangeKind::Modified)],
            &[ParametricRef::whole(handles[2])],
        );

        for handle in handles {
            let EntityType::Circle(circle) = scene.document.get_entity(handle).unwrap() else {
                panic!("expected circle");
            };
            assert!((circle.center_wcs() - driven_center).length() < 1.0e-9);
        }
    }
}
