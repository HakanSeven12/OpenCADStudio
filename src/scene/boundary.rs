use super::*;

use kernel::geom2d::{
    bounded_faces, closest_point, contains, distance_to, gap_bridges, intersect,
    ring_nesting_depths, segment_crossing, signed_area, triangulate, Curve, Line,
    SegmentCrossing, Tolerance, Transform as CurveTransform,
};

use crate::command::WorkingPlane;
use crate::par::prelude::*;

#[derive(Clone)]
pub struct BoundarySource {
    pub segments: Vec<Line>,
    pub curves: Vec<Curve>,
}

/// Boundary welding tolerance for tessellated wires.
const WELD_TOLERANCE: f64 = 1.0e-6;

fn wire_segments_on_plane(wire: &WireModel, plane: WorkingPlane, tolerance: f64) -> Vec<Line> {
    let mut segments = Vec::new();
    let mut previous: Option<glam::DVec3> = None;
    for (index, high) in wire.points.iter().copied().enumerate() {
        if !high.iter().all(|value| value.is_finite()) {
            previous = None;
            continue;
        }
        let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
        let world = glam::DVec3::new(
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        );
        let current = plane.to_local(world);
        if current.z.abs() > tolerance {
            previous = None;
            continue;
        }
        if let Some(start) = previous {
            let length = (current.x - start.x).hypot(current.y - start.y);
            if length > tolerance {
                segments.push(Line {
                    start: [start.x, start.y],
                    end: [current.x, current.y],
                });
            }
        }
        previous = Some(current);
    }
    segments
}

fn entity_curves_on_plane(entity: &EntityType, plane: WorkingPlane, tolerance: f64) -> Vec<Curve> {
    // Straight entities have no unique support plane. Check their actual
    // points and direction against the hatch OCS instead of an arbitrary one.
    let straight = match entity {
        EntityType::Line(line) => Some((line.start, line.end, None)),
        EntityType::Ray(ray) => Some((ray.base_point, ray.base_point, Some((ray.direction, true)))),
        EntityType::XLine(line) => Some((
            line.base_point,
            line.base_point,
            Some((line.direction, false)),
        )),
        _ => None,
    };
    if let Some((start, end, direction)) = straight {
        let local = |point: codec::types::Vector3| {
            plane.to_local(glam::DVec3::new(point.x, point.y, point.z))
        };
        let start = local(start);
        let end = local(end);
        if start.z.abs() > tolerance || end.z.abs() > tolerance {
            return Vec::new();
        }
        let start = [start.x, start.y];
        let curve = if let Some((direction, ray)) = direction {
            let direction =
                plane.vector_to_local(glam::DVec3::new(direction.x, direction.y, direction.z));
            if direction.z.abs() > 1.0e-9 || direction.x.hypot(direction.y) < 1.0e-12 {
                return Vec::new();
            }
            let direction = [direction.x, direction.y];
            if ray {
                Curve::Ray(kernel::geom2d::Ray {
                    origin: start,
                    direction,
                })
            } else {
                Curve::XLine(kernel::geom2d::XLine {
                    base: start,
                    direction,
                })
            }
        } else {
            Curve::Line(Line {
                start,
                end: [end.x, end.y],
            })
        };
        return vec![curve];
    }
    let Some(planar) = crate::entities::curve::entity_curve(entity) else {
        return Vec::new();
    };
    let source = planar.plane;
    let origin = plane.to_local(glam::DVec3::from_array(source.origin));
    let x_axis = plane.vector_to_local(glam::DVec3::from_array(source.x_axis));
    let y_axis = plane.vector_to_local(glam::DVec3::from_array(source.y_axis));
    if origin.z.abs() > tolerance || x_axis.z.abs() > 1.0e-9 || y_axis.z.abs() > 1.0e-9 {
        return Vec::new();
    }
    let Some(curve) = planar.curve.transformed(&CurveTransform {
        x_axis: [x_axis.x, x_axis.y].into(),
        y_axis: [y_axis.x, y_axis.y].into(),
        origin: [origin.x, origin.y].into(),
    }) else {
        return Vec::new();
    };
    let segments = curve.segments();
    if segments.is_empty() {
        vec![curve]
    } else {
        segments
    }
}

fn has_unsupported_polyline_topology(entity: &EntityType) -> bool {
    match entity {
        EntityType::Polyline(polyline) => {
            let unsupported_flags = codec::entities::PolylineFlags::POLYGON_MESH
                | codec::entities::PolylineFlags::POLYFACE_MESH
                | codec::entities::PolylineFlags::CURVE_FIT
                | codec::entities::PolylineFlags::SPLINE_FIT;
            polyline.flags.bits() & unsupported_flags.bits() != 0
        }
        EntityType::Polyline3D(polyline) => {
            polyline.flags.is_3d_mesh
                || polyline.flags.is_polyface_mesh
                || polyline.flags.spline_fit
                || polyline.smooth_type != codec::entities::polyline3d::SmoothSurfaceType::None
        }
        _ => false,
    }
}

pub(crate) fn entity_boundary_source_on_plane(
    entity: &EntityType,
    plane: WorkingPlane,
    tolerance: f64,
    clip_bounds: Option<([f64; 2], [f64; 2])>,
) -> Option<BoundarySource> {
    // Mesh vertex order describes rows/faces; fit flags describe smooth curves,
    // not the chordal path the explode fallback would produce.
    if has_unsupported_polyline_topology(entity) {
        return None;
    }

    let curves = entity_curves_on_plane(entity, plane, tolerance);

    let curves = if curves.is_empty()
        && matches!(entity, EntityType::Polyline(_) | EntityType::Polyline3D(_))
    {
        // Heavy and 3D polylines are not single planar curves in entity_curve.
        // Their constituent edges still cut a hatch when they lie in its OCS.
        crate::modules::draw::modify::explode::explode_polyline_segments(entity)
            .iter()
            .flat_map(|piece| entity_curves_on_plane(piece, plane, tolerance))
            .collect()
    } else {
        curves
    };
    if curves.is_empty() {
        return None;
    }

    let segments = live_segments_for_curves(&curves, clip_bounds, tolerance);

    (!segments.is_empty()).then_some(BoundarySource { segments, curves })
}

/// Finite extents of boundary segments, expanded by `tolerance` so clipped
/// endpoints weld with the segments that produced the box. `None` when
/// there is nothing finite to clip against.
fn segments_bounds<'a>(
    segments: impl IntoIterator<Item = &'a Line>,
    tolerance: f64,
) -> Option<([f64; 2], [f64; 2])> {
    let mut bounds: Option<([f64; 2], [f64; 2])> = None;
    for segment in segments {
        for point in [segment.start, segment.end] {
            bounds = Some(match bounds {
                None => ([point[0], point[1]], [point[0], point[1]]),
                Some((min, max)) => (
                    [min[0].min(point[0]), min[1].min(point[1])],
                    [max[0].max(point[0]), max[1].max(point[1])],
                ),
            });
        }
    }
    bounds.map(|(min, max)| {
        (
            [min[0] - tolerance, min[1] - tolerance],
            [max[0] + tolerance, max[1] + tolerance],
        )
    })
}

fn needs_scene_boundary_sources(entity: &EntityType, plane: WorkingPlane, tolerance: f64) -> bool {
    let curves = entity_curves_on_plane(entity, plane, tolerance);
    curves.is_empty()
        || curves
            .iter()
            .any(|curve| matches!(curve, Curve::Ray(_) | Curve::XLine(_)))
}

/// Tessellate live curves to finite segments. Bounded curves tessellate as
/// always; rays and infinite lines are clipped to `clip_bounds` instead —
/// tessellating them raw yields a unit stub that can never close a face.
/// With no bounds there is nothing to clip against, so unbounded curves
/// contribute nothing.
pub(crate) fn live_segments_for_curves(
    curves: &[Curve],
    clip_bounds: Option<([f64; 2], [f64; 2])>,
    tolerance: f64,
) -> Vec<Line> {
    curves
        .iter()
        .flat_map(|curve| match curve {
            Curve::Ray(ray) => clip_bounds
                .and_then(|(min, max)| {
                    clip_unbounded_to_bounds(ray.origin, ray.direction, true, min, max, tolerance)
                })
                .into_iter()
                .collect::<Vec<_>>(),
            Curve::XLine(line) => clip_bounds
                .and_then(|(min, max)| {
                    clip_unbounded_to_bounds(line.base, line.direction, false, min, max, tolerance)
                })
                .into_iter()
                .collect::<Vec<_>>(),
            _ => curve
                .tessellate_angle(kernel::tessellation::DEFAULT_ANGLE)
                .windows(2)
                .filter_map(|pair| {
                    let start = pair[0];
                    let end = pair[1];

                    (start.iter().chain(&end).all(|value| value.is_finite()) && start != end)
                        .then_some(Line { start, end })
                })
                .collect::<Vec<_>>(),
        })
        .collect()
}

fn hatch_path_seed(path: &codec::entities::BoundaryPath) -> Option<[f64; 2]> {
    // A clockwise arc edge tessellates counter-clockwise, so the edges are
    // chained tip to tail instead of appended: appended, the reversed arc
    // folds the ring and the seed lands in the neighbouring region.
    let mut edges = Vec::with_capacity(path.edges.len());
    for edge in &path.edges {
        let curve = crate::entities::hatch::edge_curve(edge)?;
        edges.push(curve.tessellate_angle(kernel::tessellation::DEFAULT_ANGLE));
    }
    let ring = super::entity::chain_path_edges(edges);
    if ring.len() < 3 {
        return None;
    }
    // The largest triangle's centroid: a sliver along a tessellated arc can
    // sit on the far side of the true curve, in the neighbouring region.
    let (points, triangles) = triangulate(&ring, &[]);
    let corners = |triangle: &[usize; 3]| triangle.map(|vertex| points[vertex]);
    let area = |[a, b, c]: [[f64; 2]; 3]| {
        ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs()
    };
    if let Some(triangle) = triangles
        .iter()
        .max_by(|a, b| area(corners(a)).total_cmp(&area(corners(b))))
    {
        let [a, b, c] = corners(triangle);
        Some([(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0])
    } else {
        Some([
            ring.iter().map(|point| point[0]).sum::<f64>() / ring.len() as f64,
            ring.iter().map(|point| point[1]).sum::<f64>() / ring.len() as f64,
        ])
    }
}

fn face_curves(face: &[[f64; 2]]) -> Vec<Curve> {
    face.iter()
        .copied()
        .zip(face.iter().copied().cycle().skip(1))
        .take(face.len())
        .map(|(start, end)| Curve::Line(Line { start, end }))
        .collect()
}

fn matching_face(faces: &[Vec<[f64; 2]>], seed: Option<[f64; 2]>) -> Option<&Vec<[f64; 2]>> {
    if faces.len() == 1 {
        return faces.first();
    }
    let seed = seed?;
    let tolerance = Tolerance::new(WELD_TOLERANCE);
    if let Some(face) = faces.iter().find(|face| {
        let curves = face_curves(face);
        contains(&curves, seed, tolerance)
    }) {
        return Some(face);
    }
    faces.iter().min_by(|a, b| {
        let nearest = |face: &Vec<[f64; 2]>| {
            face_curves(face)
                .iter()
                .map(|curve| distance_to(curve, seed))
                .fold(f64::INFINITY, f64::min)
        };
        nearest(a).total_cmp(&nearest(b))
    })
}

/// The sources with a segment reaching the box around `rings`: the only ones
/// their edges can lie on. Matching every edge against every source of a
/// large drawing took half a minute per picked hatch region. (#1813)
pub(crate) fn sources_near_rings(
    rings: &[Vec<[f64; 2]>],
    sources: &rustc_hash::FxHashMap<codec::Handle, BoundarySource>,
    pad: f64,
) -> rustc_hash::FxHashMap<codec::Handle, BoundarySource> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for point in rings.iter().flatten() {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(point[axis] - pad);
            hi[axis] = hi[axis].max(point[axis] + pad);
        }
    }
    let reaches = |line: &Line| {
        (0..2).all(|axis| {
            line.start[axis].min(line.end[axis]) <= hi[axis]
                && line.start[axis].max(line.end[axis]) >= lo[axis]
        })
    };
    sources
        .iter()
        .filter(|(_, source)| source.segments.iter().any(reaches))
        .map(|(handle, source)| (*handle, source.clone()))
        .collect()
}

pub(crate) fn ring_source_handles(
    ring: &[[f64; 2]],
    sources: &rustc_hash::FxHashMap<codec::Handle, BoundarySource>,
) -> Vec<codec::Handle> {
    let near = sources_near_rings(&[ring.to_vec()], sources, WELD_TOLERANCE);
    let sources = &near;
    let mut handles = rustc_hash::FxHashSet::default();
    for (&start, &end) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        let edge = Line { start, end };
        let edge_length = (end[0] - start[0]).hypot(end[1] - start[1]);
        for (&handle, source) in sources {
            if source.segments.iter().any(|line| {
                matches!(
                    segment_crossing(edge, *line, Tolerance::new(WELD_TOLERANCE)),
                    SegmentCrossing::Overlap { a, .. }
                        if (a[1] - a[0]).abs() * edge_length > WELD_TOLERANCE
                )
            }) {
                handles.insert(handle);
            }
        }
    }
    let mut handles: Vec<_> = handles.into_iter().collect();
    handles.sort_by_key(|handle| handle.value());
    handles
}

fn curve_forward(curve: &Curve, start: [f64; 2], next: [f64; 2]) -> bool {
    // For circular geometry, determine direction directly from the
    // geometric traversal of the detected boundary. This avoids parameter
    // wrap-around choosing the major arc instead of the minor arc.
    match curve {
        Curve::Circle(circle) => {
            let ax = start[0] - circle.centre[0];
            let ay = start[1] - circle.centre[1];
            let bx = next[0] - circle.centre[0];
            let by = next[1] - circle.centre[1];

            ax * by - ay * bx >= 0.0
        }

        Curve::Arc(arc) => {
            let ax = start[0] - arc.centre[0];
            let ay = start[1] - arc.centre[1];
            let bx = next[0] - arc.centre[0];
            let by = next[1] - arc.centre[1];

            ax * by - ay * bx >= 0.0
        }

        _ => {
            let a = curve.parameter_at(start);
            let b = curve.parameter_at(next);
            let mut delta = b - a;

            if curve.is_closed() {
                if delta > 0.5 {
                    delta -= 1.0;
                } else if delta < -0.5 {
                    delta += 1.0;
                }
            }

            delta >= 0.0
        }
    }
}

fn stored_arc_angles(start: f64, end: f64, counter_clockwise: bool, whole: bool) -> (f64, f64) {
    let stored_start =
        if counter_clockwise { start } else { -start }.rem_euclid(std::f64::consts::TAU);
    let sweep = if whole {
        std::f64::consts::TAU
    } else if counter_clockwise {
        (end - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - end).rem_euclid(std::f64::consts::TAU)
    };
    (stored_start, stored_start + sweep)
}

fn exact_boundary_edge(
    curve: Option<&Curve>,
    start: [f64; 2],
    end: [f64; 2],
    next: [f64; 2],
    whole_curve: bool,
) -> codec::entities::BoundaryEdge {
    use codec::entities::{BoundaryEdge, CircularArcEdge, EllipticArcEdge, LineEdge, SplineEdge};
    use codec::types::{Vector2, Vector3};

    let Some(curve) = curve else {
        return BoundaryEdge::Line(LineEdge {
            start: Vector2::new(start[0], start[1]),
            end: Vector2::new(end[0], end[1]),
        });
    };
    let forward = curve_forward(curve, start, next);
    match curve {
        Curve::Line(_) => BoundaryEdge::Line(LineEdge {
            start: Vector2::new(start[0], start[1]),
            end: Vector2::new(end[0], end[1]),
        }),
        Curve::Circle(circle) => {
            let true_start = (start[1] - circle.centre[1]).atan2(start[0] - circle.centre[0]);
            let true_end = (end[1] - circle.centre[1]).atan2(end[0] - circle.centre[0]);
            let (start_angle, end_angle) =
                stored_arc_angles(true_start, true_end, forward, whole_curve);
            BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(circle.centre[0], circle.centre[1]),
                radius: circle.radius,
                start_angle,
                end_angle,
                counter_clockwise: forward,
            })
        }
        Curve::Arc(arc) => {
            let true_start = (start[1] - arc.centre[1]).atan2(start[0] - arc.centre[0]);
            let true_end = (end[1] - arc.centre[1]).atan2(end[0] - arc.centre[0]);
            let (start_angle, end_angle) =
                stored_arc_angles(true_start, true_end, forward, whole_curve);
            BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(arc.centre[0], arc.centre[1]),
                radius: arc.radius,
                start_angle,
                end_angle,
                counter_clockwise: forward,
            })
        }
        Curve::Ellipse(arc) => {
            let ellipse = arc.ellipse;
            let true_start = arc.start_parameter + curve.parameter_at(start) * arc.sweep();
            let true_end = arc.start_parameter + curve.parameter_at(end) * arc.sweep();
            let (start_parameter, end_parameter) =
                stored_arc_angles(true_start, true_end, forward, whole_curve);
            BoundaryEdge::EllipticArc(EllipticArcEdge {
                center: Vector2::new(ellipse.centre[0], ellipse.centre[1]),
                major_axis_endpoint: Vector2::new(
                    ellipse.major_axis[0] * ellipse.major_radius,
                    ellipse.major_axis[1] * ellipse.major_radius,
                ),
                minor_axis_ratio: ellipse.minor_radius / ellipse.major_radius,
                start_angle: start_parameter,
                end_angle: end_parameter,
                counter_clockwise: forward,
            })
        }
        Curve::Nurbs(source) => {
            let trimmed = if whole_curve {
                Some(if forward {
                    source.clone()
                } else {
                    source.reversed()
                })
            } else {
                source.trimmed(source.parameter_at(start), source.parameter_at(end))
            };
            let Some(trimmed) = trimmed else {
                return BoundaryEdge::Line(LineEdge {
                    start: Vector2::new(start[0], start[1]),
                    end: Vector2::new(end[0], end[1]),
                });
            };
            let rational = trimmed.is_rational();
            BoundaryEdge::Spline(SplineEdge {
                degree: trimmed.degree() as i32,
                rational,
                periodic: trimmed.is_closed(),
                knots: trimmed.knots().to_vec(),
                control_points: trimmed
                    .control_points()
                    .iter()
                    .zip(trimmed.weights())
                    .map(|(point, weight)| {
                        Vector3::new(point[0], point[1], if rational { *weight } else { 1.0 })
                    })
                    .collect(),
                fit_points: Vec::new(),
                start_tangent: Vector2::new(0.0, 0.0),
                end_tangent: Vector2::new(0.0, 0.0),
            })
        }
        Curve::Polyline(_) | Curve::Ray(_) | Curve::XLine(_) => BoundaryEdge::Line(LineEdge {
            start: Vector2::new(start[0], start[1]),
            end: Vector2::new(end[0], end[1]),
        }),
    }
}

/// Rebuild detected rings from exact source curves.
pub(crate) fn exact_hatch_paths(
    rings: &[Vec<[f64; 2]>],
    exterior: &[bool],
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Vec<codec::entities::BoundaryPath> {
    use codec::entities::{BoundaryPath, BoundaryPathFlags};

    let near = sources_near_rings(rings, sources, tolerance.max(WELD_TOLERANCE));
    let sources = &near;
    rings
        .iter()
        .enumerate()
        .filter_map(|(ring_index, ring)| {
            let (points, curves) = refined_boundary_ring(ring, sources, tolerance);
            let count = points.len();
            if count < 3 {
                return None;
            }
            let handles = ring_source_handles(ring, sources);
            let mut bits = 0;
            if exterior.get(ring_index).copied().unwrap_or(ring_index == 0) {
                bits |= BoundaryPathFlags::OUTERMOST.bits();
            }
            if !handles.is_empty() {
                bits |= BoundaryPathFlags::EXTERNAL.bits();
            }
            let mut path = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(bits));

            let all_same = curves
                .first()
                .is_some_and(|first| first.is_some() && curves.iter().all(|curve| curve == first));
            if all_same {
                let curve = curves[0].as_ref();
                path.add_edge(exact_boundary_edge(
                    curve, points[0], points[0], points[1], true,
                ));
            } else {
                let start_index = (0..count)
                    .find(|index| curves[*index] != curves[(*index + count - 1) % count])
                    .unwrap_or(0);
                let mut consumed = 0usize;
                while consumed < count {
                    let edge_index = (start_index + consumed) % count;
                    let curve = curves.get(edge_index).and_then(Option::as_ref);
                    let mut length = 1usize;
                    if curve.is_some() {
                        while consumed + length < count
                            && curves[(edge_index + length) % count].as_ref() == curve
                        {
                            length += 1;
                        }
                    }
                    let end_index = (edge_index + length) % count;
                    path.add_edge(exact_boundary_edge(
                        curve,
                        points[edge_index],
                        points[end_index],
                        points[(edge_index + 1) % count],
                        false,
                    ));
                    consumed += length;
                }
            }
            for handle in handles {
                path.add_boundary_handle(handle);
            }
            Some(path)
        })
        .collect()
}

pub(crate) fn boundary_entities(
    rings: &[Vec<[f64; 2]>],
    plane: WorkingPlane,
) -> Vec<codec::EntityType> {
    rings
        .iter()
        .filter_map(|ring| {
            let mut points = Vec::with_capacity(ring.len());
            for &point in ring {
                if point.iter().all(|value| value.is_finite()) && points.last() != Some(&point) {
                    points.push(point);
                }
            }
            if points.len() > 1 && points.first() == points.last() {
                points.pop();
            }
            if points.len() < 3 {
                return None;
            }
            let mut polyline = codec::entities::LwPolyline::new();
            polyline.is_closed = true;
            polyline.vertices = points
                .into_iter()
                .map(|[x, y]| codec::entities::LwVertex::new(codec::types::Vector2::new(x, y)))
                .collect();
            Some(plane.place_entity(codec::EntityType::LwPolyline(polyline)))
        })
        .collect()
}

fn edge_curve(
    edge: Line,
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Option<Curve> {
    let edge_length = (edge.end[0] - edge.start[0]).hypot(edge.end[1] - edge.start[1]);
    let mut best = None;
    let mut best_overlap = 0.0;
    for source in sources.values() {
        for segment in &source.segments {
            if let SegmentCrossing::Overlap { a, .. } =
                segment_crossing(edge, *segment, Tolerance::new(tolerance))
            {
                let overlap = (a[1] - a[0]).abs() * edge_length;
                if overlap > best_overlap {
                    best = Some(source);
                    best_overlap = overlap;
                }
            }
        }
    }
    let candidates: Vec<&Curve> = match best {
        Some(source) => source.curves.iter().collect(),
        None => sources
            .values()
            .flat_map(|source| source.curves.iter())
            .filter(|curve| {
                distance_to(curve, edge.start).max(distance_to(curve, edge.end)) <= tolerance * 4.0
            })
            .collect(),
    };
    candidates
        .into_iter()
        .min_by(|left, right| {
            let error =
                |curve: &Curve| distance_to(curve, edge.start) + distance_to(curve, edge.end);
            error(left).total_cmp(&error(right))
        })
        .cloned()
}

fn nearest_crossing(a: &Curve, b: &Curve, point: [f64; 2], tolerance: f64) -> Option<[f64; 2]> {
    intersect(a, b, Tolerance::new(tolerance))
        .into_iter()
        .min_by(|left, right| {
            let distance =
                |candidate: [f64; 2]| (candidate[0] - point[0]).hypot(candidate[1] - point[1]);
            distance(left.point).total_cmp(&distance(right.point))
        })
        .map(|crossing| crossing.point)
}

fn project_to_curve(curve: &Curve, point: [f64; 2]) -> [f64; 2] {
    closest_point(curve, point).point
}

fn refined_boundary_ring(
    ring: &[[f64; 2]],
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> (Vec<[f64; 2]>, Vec<Option<Curve>>) {
    let count = ring.len();
    if count < 3 {
        return (ring.to_vec(), Vec::new());
    }
    let near = sources_near_rings(&[ring.to_vec()], sources, tolerance.max(WELD_TOLERANCE));
    let sources = &near;
    let edge_curves: Vec<_> = (0..count)
        .map(|index| {
            edge_curve(
                Line {
                    start: ring[index],
                    end: ring[(index + 1) % count],
                },
                sources,
                tolerance,
            )
        })
        .collect();
    let points = (0..count)
        .map(|index| {
            let point = ring[index];
            let incoming = edge_curves[(index + count - 1) % count].as_ref();
            let outgoing = edge_curves[index].as_ref();
            match (incoming, outgoing) {
                (Some(a), Some(b)) if a != b => nearest_crossing(a, b, point, tolerance)
                    .unwrap_or_else(|| project_to_curve(b, point)),
                (Some(curve), _) | (_, Some(curve)) => project_to_curve(curve, point),
                _ => point,
            }
        })
        .collect();
    (points, edge_curves)
}

fn normalized_delta(from: f64, to: f64) -> f64 {
    (to - from + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

fn curve_edge_bulge(curve: &Curve, start: [f64; 2], end: [f64; 2]) -> f64 {
    let direct = match curve {
        Curve::Circle(circle) => Some((circle.centre, circle.radius)),
        Curve::Arc(arc) => Some((arc.centre, arc.radius)),
        _ => None,
    };
    if let Some((centre, radius)) = direct {
        if radius.abs() <= 1.0e-12 {
            return 0.0;
        }
        let start_angle = (start[1] - centre[1]).atan2(start[0] - centre[0]);
        let end_angle = (end[1] - centre[1]).atan2(end[0] - centre[0]);
        return (normalized_delta(start_angle, end_angle) * 0.25).tan();
    }
    0.0
}

fn boundary_polyline(
    ring: &[[f64; 2]],
    plane: WorkingPlane,
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Option<EntityType> {
    let (points, edge_curves) = refined_boundary_ring(ring, sources, tolerance);
    if points.len() < 3 {
        return None;
    }
    let mut polyline = codec::entities::LwPolyline::new();
    polyline.is_closed = true;
    polyline.vertices = points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let mut vertex =
                codec::entities::LwVertex::new(codec::types::Vector2::new(point[0], point[1]));
            vertex.bulge = edge_curves
                .get(index)
                .and_then(|curve| curve.as_ref())
                .map(|curve| curve_edge_bulge(curve, *point, points[(index + 1) % points.len()]))
                .unwrap_or(0.0);
            vertex
        })
        .collect();
    Some(plane.place_entity(EntityType::LwPolyline(polyline)))
}

pub(crate) fn boundary_polyline_entities(
    regions: &[Vec<Vec<[f64; 2]>>],
    plane: WorkingPlane,
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Vec<EntityType> {
    let mut rings = Vec::new();
    for ring in regions.iter().flat_map(|region| region.iter()) {
        if !rings.iter().any(|existing| *existing == ring) {
            rings.push(ring);
        }
    }
    rings
        .into_iter()
        .filter_map(|ring| boundary_polyline(ring, plane, sources, tolerance))
        .collect()
}

pub(crate) fn boundary_entities_from_sources(
    rings: &[Vec<[f64; 2]>],
    plane: WorkingPlane,
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Vec<EntityType> {
    rings
        .iter()
        .filter_map(|ring| boundary_polyline(ring, plane, sources, tolerance))
        .collect()
}

/// A duplicated associative hatch must follow the duplicated boundary, not the
/// source one — otherwise editing the source regenerates the duplicate onto
/// the source's outline and it seems to vanish. A path whose boundary was not
/// duplicated along loses its association. `handle_map` maps source handles
/// to their duplicates (COPY and paste both build one). (#1370)
pub(crate) fn remap_hatch_associations(
    doc: &mut codec::Document,
    handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
) -> bool {
    let mut touched = false;
    for &copy in handle_map.values() {
        let Some(EntityType::Hatch(hatch)) = doc.get_entity_mut(copy) else {
            continue;
        };
        if !hatch.is_associative {
            continue;
        }
        for path in &mut hatch.paths {
            if path
                .boundary_handles
                .iter()
                .all(|source| handle_map.contains_key(source))
            {
                for source in &mut path.boundary_handles {
                    *source = handle_map[source];
                }
            } else {
                path.boundary_handles.clear();
                path.flags.set_external(false);
            }
        }
        hatch.is_associative = hatch
            .paths
            .iter()
            .any(|path| !path.boundary_handles.is_empty());
        touched = true;
    }
    touched
}

impl Scene {
    /// Each associative hatch among `handles` that names no source yet
    /// takes the object just before it (its outline) as its source.
    pub(crate) fn bind_outline_hatches(&mut self, handles: &[Handle]) {
        for pair in handles.windows(2) {
            let Some(EntityType::Hatch(hatch)) = self.document.get_entity_mut(pair[1]) else {
                continue;
            };
            if hatch.is_associative && hatch.paths.len() == 1 && hatch.paths[0].boundary_handles.is_empty() {
                hatch.paths[0].boundary_handles = vec![pair[0]];
                self.associative_hatch_source_cache.borrow_mut().take();
            }
        }
    }

    pub(crate) fn replace_hatch_association(
        &mut self,
        handle: Handle,
        paths: Vec<codec::entities::BoundaryPath>,
    ) {
        let Some(EntityType::Hatch(hatch)) = self.document.get_entity_mut(handle) else {
            return;
        };
        hatch.paths = paths;
        hatch.is_associative = true;
        self.associative_hatch_source_cache.borrow_mut().take();
        self.refresh_fill_model(handle);
    }

    /// COPY's half of [`remap_hatch_associations`], plus the cache it feeds.
    pub(crate) fn copy_hatch_associations(
        &mut self,
        handle_map: &rustc_hash::FxHashMap<Handle, Handle>,
    ) {
        if remap_hatch_associations(&mut self.document, handle_map) {
            self.associative_hatch_source_cache.borrow_mut().take();
        }
    }

    pub(crate) fn edit_hatch_boundary_handles(
        &mut self,
        hatch_handle: Handle,
        handles: &[Handle],
        add: bool,
    ) -> bool {
        let Some(EntityType::Hatch(hatch)) = self.document.get_entity_mut(hatch_handle) else {
            return false;
        };
        let Some(path) = hatch.paths.first_mut() else {
            return false;
        };
        if add {
            for handle in handles.iter().copied().filter(|handle| handle.is_valid()) {
                if !path.boundary_handles.contains(&handle) {
                    path.boundary_handles.push(handle);
                }
            }
        } else {
            path.boundary_handles
                .retain(|handle| !handles.contains(handle));
        }
        if path.boundary_handles.is_empty() {
            path.flags.set_external(false);
        } else {
            path.flags.set_external(true);
        }
        hatch.is_associative = hatch
            .paths
            .iter()
            .any(|candidate| !candidate.boundary_handles.is_empty());
        self.associative_hatch_source_cache.borrow_mut().take();
        if add && !handles.is_empty() {
            let changes: Vec<_> = handles
                .iter()
                .copied()
                .map(|handle| (handle, ChangeKind::Modified))
                .collect();
            self.refresh_associative_hatches(&changes);
        } else {
            self.refresh_fill_model(hatch_handle);
        }
        true
    }

    /// A boundary source split into pieces (TRIM, BREAK): every associative
    /// hatch bounded by `old` is bounded by all of `pieces` instead, so the
    /// region survives `old` being erased.
    pub(crate) fn split_hatch_source(&mut self, old: Handle, pieces: &[Handle]) {
        if pieces.is_empty() {
            return;
        }
        let dependents = self.associative_hatch_dependents(&std::iter::once(old).collect());
        if dependents.is_empty() {
            return;
        }
        for &hatch_handle in &dependents {
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(hatch_handle);
                self.record_undo_before(hatch_handle, before);
            }
            let Some(EntityType::Hatch(hatch)) = self.document.get_entity_mut(hatch_handle) else {
                continue;
            };
            for path in &mut hatch.paths {
                if !path.boundary_handles.contains(&old) {
                    continue;
                }
                path.boundary_handles.retain(|source| *source != old);
                for &piece in pieces {
                    if !path.boundary_handles.contains(&piece) {
                        path.boundary_handles.push(piece);
                    }
                }
            }
        }
        self.associative_hatch_source_cache.borrow_mut().take();
        let changes: Vec<_> = pieces
            .iter()
            .map(|&piece| (piece, ChangeKind::Modified))
            .collect();
        let refreshed = self.refresh_associative_hatches(&changes);
        if !refreshed.is_empty() {
            self.bump_entities(&refreshed);
        }
    }

    fn associative_hatch_dependents(&self, changed: &rustc_hash::FxHashSet<Handle>) -> Vec<Handle> {
        if self.associative_hatch_source_cache.borrow().is_none() {
            let mut index: HashMap<Handle, Vec<Handle>> = HashMap::default();
            for entity in self.document.entities() {
                let EntityType::Hatch(hatch) = entity else {
                    continue;
                };
                if !hatch.is_associative {
                    continue;
                }
                for source in hatch
                    .paths
                    .iter()
                    .flat_map(|path| path.boundary_handles.iter().copied())
                {
                    let dependents = index.entry(source).or_default();
                    if !dependents.contains(&hatch.common.handle) {
                        dependents.push(hatch.common.handle);
                    }
                }
            }
            *self.associative_hatch_source_cache.borrow_mut() = Some(index);
        }
        let cache = self.associative_hatch_source_cache.borrow();
        let index = cache.as_ref().expect("associative hatch index");
        let mut handles = rustc_hash::FxHashSet::default();
        for source in changed {
            if let Some(dependents) = index.get(source) {
                handles.extend(dependents.iter().copied());
            }
        }
        handles.into_iter().collect()
    }

    pub(crate) fn refresh_associative_hatches(
        &mut self,
        changes: &[(Handle, ChangeKind)],
    ) -> Vec<(Handle, ChangeKind)> {
        if changes.is_empty() {
            return Vec::new();
        }
        let perf_refresh = crate::perf::enabled();
        let refresh_started = perf_refresh.then(iced::time::Instant::now);
        let changed: rustc_hash::FxHashSet<_> = changes.iter().map(|(handle, _)| *handle).collect();
        let candidates: Vec<_> = self
            .associative_hatch_dependents(&changed)
            .into_iter()
            .filter_map(|handle| {
                let EntityType::Hatch(hatch) = self.document.get_entity(handle)? else {
                    return None;
                };
                Some({
                    let seeds = hatch.paths.iter().map(hatch_path_seed).collect::<Vec<_>>();
                    (handle, hatch.clone(), seeds)
                })
            })
            .collect();

        // Hatches that share an OCS plane need the same scene-wide source map
        // and clipping bounds. Building those inside the hatch loop rescans
        // every resident wire once per affected hatch during a bulk MOVE.
        let candidate_count = candidates.len();
        let mut candidates_by_plane: rustc_hash::FxHashMap<[u64; 4], Vec<_>> =
            rustc_hash::FxHashMap::default();
        for candidate in candidates {
            let hatch = &candidate.1;
            let plane_key = [
                hatch.normal.x.to_bits(),
                hatch.normal.y.to_bits(),
                hatch.normal.z.to_bits(),
                hatch.elevation.to_bits(),
            ];
            candidates_by_plane
                .entry(plane_key)
                .or_default()
                .push(candidate);
        }
        let plane_group_count = candidates_by_plane.len();
        let mut source_scan_count = 0;
        let mut refreshed = Vec::new();
        for candidates in candidates_by_plane.into_values() {
            let (_, sample_hatch, _) = candidates
                .first()
                .expect("plane group contains at least one hatch");
            let storage =
                crate::entities::curve::ocs_plane(sample_hatch.normal, sample_hatch.elevation);
            let plane = WorkingPlane::new(
                glam::DVec3::from_array(storage.origin),
                glam::DVec3::from_array(storage.x_axis),
                glam::DVec3::from_array(storage.y_axis),
            );
            // Native finite boundary curves can be refreshed directly from the
            // associated entities. Scan the scene only when a changed path
            // includes an unbounded or unsupported source that needs wire fallback.
            let needs_scene_sources = candidates.iter().any(|(_, hatch, _)| {
                hatch
                    .paths
                    .iter()
                    .filter(|path| {
                        path.boundary_handles
                            .iter()
                            .any(|source| changed.contains(source))
                    })
                    .flat_map(|path| path.boundary_handles.iter())
                    .filter_map(|source| self.document.get_entity(*source))
                    .any(|entity| needs_scene_boundary_sources(entity, plane, WELD_TOLERANCE))
            });
            let all_sources = needs_scene_sources.then(|| {
                source_scan_count += 1;
                self.boundary_sources_on_plane(plane, WELD_TOLERANCE)
            });
            let clip_bounds = all_sources.as_ref().and_then(|sources| {
                let segments = sources.values().flat_map(|source| &source.segments);
                segments_bounds(segments, WELD_TOLERANCE)
            });
            for (handle, mut hatch, seeds) in candidates {
                let mut modified = false;
                let mut association_changed = false;
                for (index, path) in hatch.paths.iter_mut().enumerate() {
                    if !path
                        .boundary_handles
                        .iter()
                        .any(|source| changed.contains(source))
                    {
                        continue;
                    }
                    // An erased source leaves the path as it is, no longer
                    // associative: the remaining sources alone would bound
                    // a different region.
                    if path
                        .boundary_handles
                        .iter()
                        .any(|source| self.document.get_entity(*source).is_none())
                    {
                        path.boundary_handles.clear();
                        path.flags.set_external(false);
                        association_changed = true;
                        modified = true;
                        continue;
                    }
                    let sources: rustc_hash::FxHashMap<_, _> = path
                        .boundary_handles
                        .iter()
                        .filter_map(|source| {
                            // Associative boundaries are explicit document references.
                            // During a grip drag their display wire can be preview-hidden,
                            // so prefer the live entity geometry over the resident wire cache.
                            let geometry = self
                                .document
                                .get_entity(*source)
                                .and_then(|entity| {
                                    entity_boundary_source_on_plane(
                                        entity,
                                        plane,
                                        WELD_TOLERANCE,
                                        clip_bounds,
                                    )
                                })
                                .or_else(|| {
                                    all_sources
                                        .as_ref()
                                        .and_then(|sources| sources.get(source).cloned())
                                });

                            geometry.map(|geometry| (*source, geometry))
                        })
                        .collect();
                    let segments: Vec<_> = sources
                        .values()
                        .flat_map(|source| source.segments.iter().copied())
                        .collect();
                    let faces = bounded_faces(&segments, Tolerance::new(WELD_TOLERANCE));
                    let Some(face) = matching_face(&faces, seeds.get(index).copied().flatten())
                    else {
                        continue;
                    };
                    let exterior = [path.flags.is_outermost()];
                    if let Some(exact) = exact_hatch_paths(
                        std::slice::from_ref(face),
                        &exterior,
                        &sources,
                        WELD_TOLERANCE,
                    )
                    .into_iter()
                    .next()
                    {
                        path.edges = exact.edges;
                    }
                    modified = true;
                }
                hatch.is_associative = hatch
                    .paths
                    .iter()
                    .any(|path| !path.boundary_handles.is_empty());
                if !modified {
                    continue;
                }
                if association_changed {
                    self.associative_hatch_source_cache.borrow_mut().take();
                }
                if self.is_recording_undo() {
                    let before = self.document.get_entity_arc(handle);
                    self.record_undo_before(handle, before);
                }
                if let Some(slot) = self.document.get_entity_mut(handle) {
                    *slot = EntityType::Hatch(hatch);
                    self.refresh_fill_model(handle);
                    refreshed.push((handle, ChangeKind::Modified));
                }
            }
        }
        let refresh_ms = crate::perf::elapsed_ms(refresh_started);
        if perf_refresh && refresh_ms >= 1.0 {
            crate::perf_record!(
                "[perf] hatch-associative-refresh total={refresh_ms:.1}ms changed_sources={} candidate_hatches={candidate_count} planes={plane_group_count} source_scans={source_scan_count} refreshed={}",
                changed.len(),
                refreshed.len(),
            );
        }
        refreshed
    }

    /// Keep the boundary sources drawn in the current view. HATCH, GRADIENT
    /// and BOUNDARY find regions among what is on screen; an object off the
    /// view, or on a layer that is off or frozen (never drawn), takes no part
    /// (#2126). A model view seen through a layout viewport is left whole.
    pub fn retain_sources_on_screen(
        &self,
        sources: &mut rustc_hash::FxHashMap<Handle, BoundarySource>,
        plane: WorkingPlane,
    ) {
        if self.active_viewport.is_some() {
            return;
        }
        let (width, height) = self.selection.borrow().view.vp_size;
        if !(width > 1.0 && height > 1.0) {
            return;
        }
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let camera = self.camera.borrow();
        let corners = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)].map(|(x, y)| {
            let world = camera.unproject_on_plane(
                iced::Point::new(x, y),
                bounds,
                plane.z.as_vec3(),
                plane.origin,
            );
            plane.to_local(world)
        });
        if !corners.iter().all(|corner| corner.is_finite()) {
            return;
        }
        let (min_x, max_x) = corners
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c.x), hi.max(c.x)));
        let (min_y, max_y) = corners
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c.y), hi.max(c.y)));
        let on_screen = |line: &Line| {
            line.start[0].max(line.end[0]) >= min_x
                && line.start[0].min(line.end[0]) <= max_x
                && line.start[1].max(line.end[1]) >= min_y
                && line.start[1].min(line.end[1]) <= max_y
        };
        let shown = |handle: &Handle| {
            self.document.get_entity(*handle).is_none_or(|entity| {
                self.document
                    .layers
                    .get(&entity.common().layer)
                    .is_none_or(|layer| !layer.is_off() && !layer.is_frozen())
            })
        };
        sources.retain(|handle, source| {
            shown(handle) && source.segments.iter().any(on_screen)
        });
    }

    /// Boundary candidates in the active working plane, with exact curves
    /// where the source entity exposes them.
    pub fn boundary_sources_on_plane(
        &self,
        plane: WorkingPlane,
        tolerance: f64,
    ) -> rustc_hash::FxHashMap<Handle, BoundarySource> {
        let wires = if let Some(viewport) = self.active_viewport {
            self.model_wires_for_viewport_arc(viewport, 0.0)
        } else if self.current_layout == "Model" {
            let camera = self.camera.borrow().clone();
            self.model_tile_wires_arc(0, &camera, 1.0, 1.0)
        } else {
            self.paper_sheet_wires_arc()
        };
        let wires = crate::scene::model::instance_model::expand_stubs(&wires);
        let wire_started = crate::perf::enabled().then(iced::time::Instant::now);
        // Clip in parallel, merge per handle in order. `filter_map` + `collect`
        // also exist on the sequential wasm32 shim; `fold`/`reduce` do not.
        let clipped: Vec<_> = wires
            .par_iter()
            .filter_map(|wire| {
                let handle = Self::handle_from_wire_name(&wire.name)?;
                let segments = wire_segments_on_plane(wire, plane, tolerance);
                (!segments.is_empty()).then_some((handle, segments))
            })
            .collect();
        let mut sources: rustc_hash::FxHashMap<Handle, BoundarySource> =
            rustc_hash::FxHashMap::default();
        for (handle, segments) in clipped {
            sources
                .entry(handle)
                .or_insert_with(|| BoundarySource {
                    segments: Vec::new(),
                    curves: Vec::new(),
                })
                .segments
                .extend(segments);
        }
        let wire_ms = crate::perf::elapsed_ms(wire_started);
        // Objects on a layer that is turned off (not frozen) stay sources, so
        // an associative hatch keeps a boundary whose layer was switched off.
        // They draw no wire, so they come from the document. A pick in HATCH
        // or BOUNDARY drops them again (`retain_sources_on_screen`).
        let space = if self.active_viewport.is_some() || self.current_layout == "Model" {
            self.content_render_block_handle()
        } else {
            self.current_layout_block_handle()
        };
        for entity in self.document.entities() {
            let common = entity.common();
            if common.owner_handle != space || sources.contains_key(&common.handle) {
                continue;
            }
            let hidden = self
                .document
                .layers
                .get(&common.layer)
                .is_some_and(|layer| layer.is_off() && !layer.is_frozen());
            if !hidden {
                continue;
            }
            if let Some(source) = entity_boundary_source_on_plane(entity, plane, tolerance, None) {
                sources.insert(common.handle, source);
            }
        }
        // Finite extents of the wire geometry, so rays and infinite
        // construction lines clip to something real instead of tessellating
        // to a unit stub (see `live_segments_for_curves`).
        let curves_started = crate::perf::enabled().then(iced::time::Instant::now);
        let bounds = segments_bounds(
            sources.values().flat_map(|source| &source.segments),
            tolerance,
        );
        let mut curve_count = 0usize;
        for (&handle, source) in &mut sources {
            let curves = self
                .document
                .get_entity(handle)
                .map(|entity| entity_curves_on_plane(entity, plane, tolerance))
                .unwrap_or_default();
            curve_count += curves.len();

            // Associative hatch regeneration must use the entity's live geometry,
            // not a potentially one-frame-old display wire. Grip edits and STRETCH
            // mutate the document before the resident wire cache catches up.
            let live_segments = live_segments_for_curves(&curves, bounds, tolerance);

            if !live_segments.is_empty() {
                source.segments = live_segments;
                source.curves = curves;
            } else {
                // Fallback for entity types that do not expose usable native curves:
                // retain the existing display-wire geometry.
                source.curves = curves;
                source.curves.retain(|curve| {
                    source.segments.iter().any(|segment| {
                        distance_to(curve, segment.start).max(distance_to(curve, segment.end))
                            <= tolerance * 4.0
                    })
                });
            }
        }
        let curves_ms = crate::perf::elapsed_ms(curves_started);
        if crate::perf::enabled() && (wire_ms >= 1.0 || curves_ms >= 1.0) {
            crate::perf_record!(
                "[perf] hatch-boundary-source wires={} sources={} curves={} parallel-wire-pass={wire_ms:.1}ms live-curves-and-bounds={curves_ms:.1}ms",
                wires.len(),
                sources.len(),
                curve_count,
            );
        }
        sources
    }
}

/// Clip an unbounded curve (ray when `forward_only`, infinite line
/// otherwise) to a finite bounding box, so it can participate in face
/// detection. Returns `None` when the direction is degenerate, when the
/// curve misses the box, or when the clipped piece is shorter than
/// `tolerance` and could never close a face on its own.
fn clip_unbounded_to_bounds(
    base: [f64; 2],
    direction: [f64; 2],
    forward_only: bool,
    min: [f64; 2],
    max: [f64; 2],
    tolerance: f64,
) -> Option<Line> {
    let (dx, dy) = (direction[0], direction[1]);
    if dx == 0.0 && dy == 0.0 {
        return None;
    }
    // Liang-Barsky: narrow the parameter interval per axis.
    let mut entered = f64::NEG_INFINITY;
    let mut exited = f64::INFINITY;
    for axis in 0..2 {
        let (base, delta, low, high) = (base[axis], direction[axis], min[axis], max[axis]);
        if delta.abs() <= f64::EPSILON {
            if base < low || base > high {
                return None;
            }
        } else {
            let (mut near, mut far) = ((low - base) / delta, (high - base) / delta);
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            entered = entered.max(near);
            exited = exited.min(far);
            if entered > exited {
                return None;
            }
        }
    }
    if forward_only {
        entered = entered.max(0.0);
    }
    if !(entered <= exited) || !entered.is_finite() || !exited.is_finite() {
        return None;
    }
    let start = [base[0] + entered * dx, base[1] + entered * dy];
    let end = [base[0] + exited * dx, base[1] + exited * dy];
    let length = (end[0] - start[0]).hypot(end[1] - start[1]);
    (length > tolerance).then_some(Line { start, end })
}

fn hatch_path_geometry(path: &codec::entities::BoundaryPath) -> (Vec<[f64; 2]>, Vec<f64>) {
    let edges: Vec<_> = path
        .edges
        .iter()
        .filter_map(crate::entities::hatch::edge_curve)
        .map(|curve| curve.tessellate_angle(kernel::tessellation::DEFAULT_ANGLE))
        .collect();
    super::entity::chain_path_edges_with_directions(edges)
}

pub(crate) fn hatch_path_ring(path: &codec::entities::BoundaryPath) -> Option<Vec<[f64; 2]>> {
    let (ring, _) = hatch_path_geometry(path);
    (ring.len() >= 3).then_some(ring)
}

pub(crate) fn hatch_boundary_rings(hatch: &codec::entities::Hatch) -> Vec<Vec<[f64; 2]>> {
    hatch.paths.iter().filter_map(hatch_path_ring).collect()
}

pub(crate) fn separated_hatch_path_groups(
    hatch: &codec::entities::Hatch,
) -> Vec<Vec<codec::entities::BoundaryPath>> {
    let items: Vec<_> = hatch
        .paths
        .iter()
        .filter_map(|path| hatch_path_ring(path).map(|ring| (path.clone(), ring)))
        .collect();
    let rings: Vec<_> = items.iter().map(|(_, ring)| ring.clone()).collect();
    let depths = ring_nesting_depths(&rings);
    let outer_indices: Vec<_> = depths
        .iter()
        .enumerate()
        .filter_map(|(index, depth)| (*depth == 0).then_some(index))
        .collect();
    let mut groups: Vec<_> = outer_indices
        .iter()
        .map(|index| vec![items[*index].0.clone()])
        .collect();
    for (index, (path, ring)) in items.iter().enumerate() {
        if depths.get(index) == Some(&0) {
            continue;
        }
        let Some(seed) = ring.first().copied() else {
            continue;
        };
        let owner = outer_indices
            .iter()
            .enumerate()
            .filter(|(_, outer)| {
                contains(
                    &face_curves(&rings[**outer]),
                    seed,
                    Tolerance::new(WELD_TOLERANCE),
                )
            })
            .min_by(|(_, left), (_, right)| {
                signed_area(&rings[**left])
                    .abs()
                    .total_cmp(&signed_area(&rings[**right]).abs())
            })
            .map(|(group, _)| group);
        if let Some(owner) = owner {
            groups[owner].push(path.clone());
        } else {
            groups.push(vec![path.clone()]);
        }
    }
    groups
}

/// The faces the segments enclose once gaps up to `gap` are bridged.
fn faces_with_gaps(mut segments: Vec<Line>, gap: f64) -> Vec<Vec<[f64; 2]>> {
    let bridges = gap_bridges(&segments, gap, Tolerance::new(WELD_TOLERANCE));
    segments.extend(bridges);
    bounded_faces(&segments, Tolerance::new(WELD_TOLERANCE))
}

/// Every face the sources enclose, treating gaps up to `gap` as closed.
pub(crate) fn boundary_faces(
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    gap: f64,
) -> Vec<Vec<[f64; 2]>> {
    let segments: Vec<_> = sources
        .values()
        .flat_map(|source| source.segments.iter().copied())
        .collect();
    faces_with_gaps(segments, gap)
}

/// The faces a pick at `point` needs, built from the sources near it only.
///
/// The search box starts just past the nearest geometry and doubles until
/// the innermost face holding the point lies inside it. A source that misses
/// the box lies wholly outside it, so it cannot cut that face or sit in it
/// as an island: the result matches `boundary_faces` over every source for
/// this pick, without building the arrangement of the whole drawing (#1869).
///
/// Gaps up to `gap` between loose ends count as closed (`gap_bridges`).
pub(crate) fn boundary_faces_around(
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    point: [f64; 2],
    gap: f64,
) -> Vec<Vec<[f64; 2]>> {
    let tolerance = gap.max(WELD_TOLERANCE);
    let boxes: Vec<([f64; 2], [f64; 2], &BoundarySource)> = sources
        .values()
        .filter_map(|source| {
            segments_bounds(&source.segments, tolerance).map(|(min, max)| (min, max, source))
        })
        .collect();
    let Some((all_min, all_max)) = boxes.iter().map(|&(min, max, _)| (min, max)).reduce(
        |(a_min, a_max), (b_min, b_max)| {
            (
                [a_min[0].min(b_min[0]), a_min[1].min(b_min[1])],
                [a_max[0].max(b_max[0]), a_max[1].max(b_max[1])],
            )
        },
    ) else {
        return Vec::new();
    };
    let nearest = boxes
        .iter()
        .flat_map(|(_, _, source)| &source.segments)
        .map(|segment| distance_to(&Curve::Line(*segment), point))
        .fold(f64::INFINITY, f64::min);
    let extent = (all_max[0] - all_min[0]).max(all_max[1] - all_min[1]);
    let mut half = (nearest * 2.0).max(extent / 1024.0).max(tolerance * 4.0);
    loop {
        let (min, max) = (
            [point[0] - half, point[1] - half],
            [point[0] + half, point[1] + half],
        );
        let covers_all = min[0] <= all_min[0]
            && min[1] <= all_min[1]
            && max[0] >= all_max[0]
            && max[1] >= all_max[1];
        let segments: Vec<Line> = boxes
            .iter()
            .filter(|(lo, hi, _)| {
                lo[0] <= max[0] && hi[0] >= min[0] && lo[1] <= max[1] && hi[1] >= min[1]
            })
            .flat_map(|(_, _, source)| source.segments.iter().copied())
            .collect();
        let faces = faces_with_gaps(segments, gap);
        let innermost = faces
            .iter()
            .filter(|face| contains(&face_curves(face), point, Tolerance::new(WELD_TOLERANCE)))
            .min_by(|a, b| signed_area(a).abs().total_cmp(&signed_area(b).abs()));
        let settled = innermost.is_some_and(|face| {
            face.iter().all(|p| {
                p[0] > min[0] + tolerance
                    && p[0] < max[0] - tolerance
                    && p[1] > min[1] + tolerance
                    && p[1] < max[1] - tolerance
            })
        });
        if settled || covers_all {
            return faces;
        }
        half *= 2.0;
    }
}

#[cfg(test)]
mod boundary_xline_tests {
    use super::*;

    fn rect_edge(scene: &mut Scene, start: [f64; 3], end: [f64; 3]) -> codec::Handle {
        scene.add_entity(EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::new(start[0], start[1], start[2]),
            codec::types::Vector3::new(end[0], end[1], end[2]),
        )))
    }

    #[test]
    fn hatch_seed_stays_in_a_region_bounded_by_a_clockwise_arc() {
        use codec::entities::{BoundaryEdge, BoundaryPath, CircularArcEdge, LineEdge};
        use codec::types::Vector2;
        // The part of a 50 x 30 rectangle right of a radius-20 circle at
        // (25, 15), as HATCH stores it: three lines and a clockwise arc.
        let x = 25.0 + 175f64.sqrt();
        let line = |a: [f64; 2], b: [f64; 2]| {
            BoundaryEdge::Line(LineEdge {
                start: Vector2::new(a[0], a[1]),
                end: Vector2::new(b[0], b[1]),
            })
        };
        let angle = (15f64).atan2(x - 25.0);
        let mut path = BoundaryPath::new();
        path.edges = vec![
            line([x, 0.0], [50.0, 0.0]),
            line([50.0, 0.0], [50.0, 30.0]),
            line([50.0, 30.0], [x, 30.0]),
            BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(25.0, 15.0),
                radius: 20.0,
                start_angle: std::f64::consts::TAU - angle,
                end_angle: std::f64::consts::TAU + angle,
                counter_clockwise: false,
            }),
        ];
        let [sx, sy] = hatch_path_seed(&path).expect("a seed");
        assert!(sx < 50.0 && (0.0..30.0).contains(&sy));
        assert!((sx - 25.0).hypot(sy - 15.0) > 20.0, "seed {sx},{sy} is inside the circle");
    }

    #[test]
    fn xline_splits_rectangle_into_two_faces() {
        let mut scene = Scene::new();
        rect_edge(&mut scene, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        rect_edge(&mut scene, [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]);
        rect_edge(&mut scene, [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]);
        rect_edge(&mut scene, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]);
        // Infinite vertical construction line through x=5.
        scene.add_entity(EntityType::XLine(codec::entities::XLine::new(
            codec::types::Vector3::new(5.0, 0.0, 0.0),
            codec::types::Vector3::new(0.0, 1.0, 0.0),
        )));

        let sources =
            scene.boundary_sources_on_plane(crate::command::WorkingPlane::default(), 1.0e-6);
        let faces = boundary_faces(&sources, 1.0e-6);
        assert_eq!(
            faces.len(),
            2,
            "vertical xline must split the 10x10 rectangle into two faces"
        );
    }

    #[test]
    fn ray_splits_rectangle_into_two_faces() {
        let mut scene = Scene::new();
        rect_edge(&mut scene, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        rect_edge(&mut scene, [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]);
        rect_edge(&mut scene, [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]);
        rect_edge(&mut scene, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]);
        // Ray starting below the rectangle, pointing up through x=5.
        scene.add_entity(EntityType::Ray(codec::entities::Ray::new(
            codec::types::Vector3::new(5.0, -5.0, 0.0),
            codec::types::Vector3::new(0.0, 1.0, 0.0),
        )));

        let sources =
            scene.boundary_sources_on_plane(crate::command::WorkingPlane::default(), 1.0e-6);
        let faces = boundary_faces(&sources, 1.0e-6);
        assert_eq!(
            faces.len(),
            2,
            "upward ray must split the 10x10 rectangle into two faces"
        );
    }

    #[test]
    fn xline_source_clips_to_given_bounds() {
        let xline = EntityType::XLine(codec::entities::XLine::new(
            codec::types::Vector3::new(5.0, 0.0, 0.0),
            codec::types::Vector3::new(0.0, 1.0, 0.0),
        ));
        let source = entity_boundary_source_on_plane(
            &xline,
            crate::command::WorkingPlane::default(),
            1.0e-6,
            Some(([0.0, 0.0], [10.0, 10.0])),
        )
        .expect("xline crossing the box must produce a source");
        assert_eq!(source.segments.len(), 1);
        let segment = &source.segments[0];
        assert!(
            (segment.start[0] - 5.0).abs() < 1e-6,
            "got {:?}",
            segment.start
        );
        assert!(
            (segment.start[1] - 0.0).abs() < 1e-6,
            "got {:?}",
            segment.start
        );
        assert!((segment.end[0] - 5.0).abs() < 1e-6, "got {:?}", segment.end);
        assert!(
            (segment.end[1] - 10.0).abs() < 1e-6,
            "got {:?}",
            segment.end
        );
    }

    #[test]
    fn finite_boundary_curves_skip_scene_scan_but_xlines_require_it() {
        let plane = crate::command::WorkingPlane::default();
        let line = EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::new(0.0, 0.0, 0.0),
            codec::types::Vector3::new(10.0, 0.0, 0.0),
        ));
        let xline = EntityType::XLine(codec::entities::XLine::new(
            codec::types::Vector3::new(5.0, 0.0, 0.0),
            codec::types::Vector3::new(0.0, 1.0, 0.0),
        ));

        assert!(!needs_scene_boundary_sources(&line, plane, WELD_TOLERANCE));
        assert!(needs_scene_boundary_sources(&xline, plane, WELD_TOLERANCE));
    }

    #[test]
    fn xline_missing_the_geometry_contributes_no_face() {
        let mut scene = Scene::new();
        rect_edge(&mut scene, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        rect_edge(&mut scene, [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]);
        rect_edge(&mut scene, [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]);
        rect_edge(&mut scene, [0.0, 10.0, 0.0], [0.0, 0.0, 0.0]);
        // Far-away vertical construction line: never touches the rectangle.
        scene.add_entity(EntityType::XLine(codec::entities::XLine::new(
            codec::types::Vector3::new(50.0, 0.0, 0.0),
            codec::types::Vector3::new(0.0, 1.0, 0.0),
        )));

        let sources =
            scene.boundary_sources_on_plane(crate::command::WorkingPlane::default(), 1.0e-6);
        let faces = boundary_faces(&sources, 1.0e-6);
        assert_eq!(
            faces.len(),
            1,
            "a distant xline must leave the single rectangle face alone"
        );
    }

    #[test]
    fn faces_around_a_pick_match_the_whole_drawing() {
        // A 12×12 grid of rooms, one holding an island. Every wall runs the
        // full grid, so each search box pulls in walls reaching far beyond it.
        let mut segments = Vec::new();
        for k in 0..=12 {
            let k = k as f64 * 10.0;
            segments.push(Line {
                start: [0.0, k],
                end: [120.0, k],
            });
            segments.push(Line {
                start: [k, 0.0],
                end: [k, 120.0],
            });
        }
        let island = [[53.0, 53.0], [57.0, 53.0], [57.0, 57.0], [53.0, 57.0]];
        for k in 0..4 {
            segments.push(Line {
                start: island[k],
                end: island[(k + 1) % 4],
            });
        }
        let sources: rustc_hash::FxHashMap<_, _> = segments
            .into_iter()
            .enumerate()
            .map(|(k, line)| {
                let source = BoundarySource {
                    segments: vec![line],
                    curves: vec![Curve::Line(line)],
                };
                (Handle::new(k as u64 + 1), source)
            })
            .collect();
        let tolerance = 1.0e-6;
        let innermost = |faces: &[Vec<[f64; 2]>], point: [f64; 2]| {
            let mut found: Vec<_> = faces
                .iter()
                .filter(|face| contains(&face_curves(face), point, Tolerance::new(tolerance)))
                .map(|face| (signed_area(face).abs(), face.clone()))
                .collect();
            found.sort_by(|a, b| a.0.total_cmp(&b.0));
            found.first().map(|(area, _)| *area)
        };
        let islands = |faces: &[Vec<[f64; 2]>]| {
            faces
                .iter()
                .filter(|face| (signed_area(face).abs() - 16.0).abs() < 1e-9)
                .count()
        };
        let all = boundary_faces(&sources, tolerance);
        for point in [[51.0, 51.0], [55.0, 55.0], [5.0, 115.0], [119.0, 1.0], [200.0, 5.0]] {
            let near = boundary_faces_around(&sources, point, tolerance);
            assert_eq!(innermost(&near, point), innermost(&all, point), "{point:?}");
            assert!(near.len() <= all.len());
        }
        let room = boundary_faces_around(&sources, [51.0, 51.0], tolerance);
        assert_eq!(islands(&room), 1, "the island in the picked room is kept");
        assert!(room.len() < all.len() / 4, "only the neighbourhood is built");
    }

    #[test]
    fn gap_tolerance_closes_a_corner_gap_that_fits() {
        // A square whose right side stops 0.5 short of the bottom one.
        let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 0.5], [10.0, 10.0], [0.0, 10.0]];
        let sources: rustc_hash::FxHashMap<_, _> = [(0, 1), (2, 3), (3, 4), (4, 0)]
            .into_iter()
            .enumerate()
            .map(|(k, (a, b))| {
                let line = Line {
                    start: corners[a],
                    end: corners[b],
                };
                let source = BoundarySource {
                    segments: vec![line],
                    curves: vec![Curve::Line(line)],
                };
                (Handle::new(k as u64 + 1), source)
            })
            .collect();
        assert!(boundary_faces(&sources, 0.0).is_empty());
        assert!(boundary_faces_around(&sources, [5.0, 5.0], 0.4).is_empty());
        let faces = boundary_faces_around(&sources, [5.0, 5.0], 0.6);
        assert_eq!(faces.len(), 1);
        assert!((signed_area(&faces[0]).abs() - 100.0).abs() < 1e-9);
    }
}
