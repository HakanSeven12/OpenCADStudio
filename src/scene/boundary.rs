use super::*;

use cadkernel::geom2d::{
    bounded_faces, closest_point, contains, distance_to, intersect, ring_nesting_depths,
    segment_crossing, signed_area, triangulate, Curve, Line, SegmentCrossing, Tolerance,
    Transform as CurveTransform,
};

use crate::command::WorkingPlane;

#[derive(Clone)]
pub struct BoundarySource {
    pub segments: Vec<Line>,
    pub curves: Vec<Curve>,
}

/// Boundary welding tolerance for tessellated wires.
const WELD_TOLERANCE: f64 = 1.0e-6;

fn wire_segments_on_plane(
    wire: &WireModel,
    plane: WorkingPlane,
    tolerance: f64,
) -> Vec<Line> {
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

fn entity_curves_on_plane(
    entity: &EntityType,
    plane: WorkingPlane,
    tolerance: f64,
) -> Vec<Curve> {
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

fn hatch_path_seed(path: &acadrust::entities::BoundaryPath) -> Option<[f64; 2]> {
    let mut ring = Vec::new();
    for edge in &path.edges {
        let curve = crate::entities::hatch::edge_curve(edge)?;
        let points = curve.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE);
        ring.extend(points.into_iter().skip(usize::from(!ring.is_empty())));
    }
    if ring.len() < 3 {
        return None;
    }
    let (points, triangles) = triangulate(&ring, &[]);
    if let Some(triangle) = triangles.first() {
        let [a, b, c] = triangle.map(|vertex| points[vertex]);
        Some([
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
        ])
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

pub(crate) fn ring_source_handles(
    ring: &[[f64; 2]],
    sources: &rustc_hash::FxHashMap<acadrust::Handle, BoundarySource>,
) -> Vec<acadrust::Handle> {
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

fn stored_arc_angles(start: f64, end: f64, counter_clockwise: bool, whole: bool) -> (f64, f64) {
    let stored_start = if counter_clockwise { start } else { -start }
        .rem_euclid(std::f64::consts::TAU);
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
) -> acadrust::entities::BoundaryEdge {
    use acadrust::entities::{
        BoundaryEdge, CircularArcEdge, EllipticArcEdge, LineEdge, SplineEdge,
    };
    use acadrust::types::{Vector2, Vector3};

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
                Some(if forward { source.clone() } else { source.reversed() })
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
        Curve::Polyline(_) | Curve::Ray(_) | Curve::XLine(_) => {
            BoundaryEdge::Line(LineEdge {
                start: Vector2::new(start[0], start[1]),
                end: Vector2::new(end[0], end[1]),
            })
        }
    }
}

/// Rebuild detected rings from exact source curves.
pub(crate) fn exact_hatch_paths(
    rings: &[Vec<[f64; 2]>],
    exterior: &[bool],
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Vec<acadrust::entities::BoundaryPath> {
    use acadrust::entities::{BoundaryPath, BoundaryPathFlags};

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

            let all_same = curves.first().is_some_and(|first| {
                first.is_some() && curves.iter().all(|curve| curve == first)
            });
            if all_same {
                let curve = curves[0].as_ref();
                path.add_edge(exact_boundary_edge(
                    curve,
                    points[0],
                    points[0],
                    points[1],
                    true,
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
) -> Vec<acadrust::EntityType> {
    rings
        .iter()
        .filter_map(|ring| {
            let mut points = Vec::with_capacity(ring.len());
            for &point in ring {
                if point.iter().all(|value| value.is_finite())
                    && points.last() != Some(&point)
                {
                    points.push(point);
                }
            }
            if points.len() > 1 && points.first() == points.last() {
                points.pop();
            }
            if points.len() < 3 {
                return None;
            }
            let mut polyline = acadrust::entities::LwPolyline::new();
            polyline.is_closed = true;
            polyline.vertices = points
                .into_iter()
                .map(|[x, y]| {
                    acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(x, y))
                })
                .collect();
            Some(plane.place_entity(acadrust::EntityType::LwPolyline(polyline)))
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
                distance_to(curve, edge.start).min(distance_to(curve, edge.end))
                    <= tolerance * 4.0
            })
            .collect(),
    };
    candidates
        .into_iter()
        .min_by(|left, right| {
            let error = |curve: &Curve| {
                distance_to(curve, edge.start) + distance_to(curve, edge.end)
            };
            error(left).total_cmp(&error(right))
        })
        .cloned()
}

fn nearest_crossing(a: &Curve, b: &Curve, point: [f64; 2], tolerance: f64) -> Option<[f64; 2]> {
    intersect(a, b, Tolerance::new(tolerance))
        .into_iter()
        .min_by(|left, right| {
            let distance = |candidate: [f64; 2]| {
                (candidate[0] - point[0]).hypot(candidate[1] - point[1])
            };
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
    (to - from + std::f64::consts::PI)
        .rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI
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
    let mut polyline = acadrust::entities::LwPolyline::new();
    polyline.is_closed = true;
    polyline.vertices = points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let mut vertex = acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(
                point[0], point[1],
            ));
            vertex.bulge = edge_curves
                .get(index)
                .and_then(|curve| curve.as_ref())
                .map(|curve| {
                    curve_edge_bulge(curve, *point, points[(index + 1) % points.len()])
                })
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

impl Scene {
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
            path.boundary_handles.retain(|handle| !handles.contains(handle));
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

    fn associative_hatch_dependents(
        &self,
        changed: &rustc_hash::FxHashSet<Handle>,
    ) -> Vec<Handle> {
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
        let changed: rustc_hash::FxHashSet<_> =
            changes.iter().map(|(handle, _)| *handle).collect();
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

        let mut refreshed = Vec::new();
        for (handle, mut hatch, seeds) in candidates {
            let storage = crate::entities::curve::ocs_plane(hatch.normal, hatch.elevation);
            let plane = WorkingPlane::new(
                glam::DVec3::from_array(storage.origin),
                glam::DVec3::from_array(storage.x_axis),
                glam::DVec3::from_array(storage.y_axis),
            );
            let all_sources = self.boundary_sources_on_plane(plane, WELD_TOLERANCE);
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
                let old_count = path.boundary_handles.len();
                path.boundary_handles
                    .retain(|source| self.document.get_entity(*source).is_some());
                if path.boundary_handles.is_empty() {
                    path.flags.set_external(false);
                }
                association_changed |= path.boundary_handles.len() != old_count;
                modified |= association_changed;
                let sources: rustc_hash::FxHashMap<_, _> = path
                    .boundary_handles
                    .iter()
                    .filter_map(|source| {
                        all_sources
                            .get(source)
                            .cloned()
                            .map(|geometry| (*source, geometry))
                    })
                    .collect();
                let segments: Vec<_> = sources
                    .values()
                    .flat_map(|source| source.segments.iter().copied())
                    .collect();
                let faces = bounded_faces(&segments, Tolerance::new(WELD_TOLERANCE));
                let Some(face) = matching_face(&faces, seeds.get(index).copied().flatten()) else {
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
        refreshed
    }

    /// Boundary candidates in the active working plane, with exact curves
    /// where the source entity exposes them.
    pub fn boundary_sources_on_plane(
        &self,
        plane: WorkingPlane,
        tolerance: f64,
    ) -> rustc_hash::FxHashMap<Handle, BoundarySource> {
        let mut sources: rustc_hash::FxHashMap<Handle, BoundarySource> =
            rustc_hash::FxHashMap::default();
        let wires = if let Some(viewport) = self.active_viewport {
            self.model_wires_for_viewport_arc(viewport, 0.0)
        } else if self.current_layout == "Model" {
            let camera = self.camera.borrow().clone();
            self.model_tile_wires_arc(0, &camera, 1.0, 1.0)
        } else {
            self.paper_sheet_wires_arc()
        };
        for wire in wires.iter() {
            let Some(handle) = Self::handle_from_wire_name(&wire.name) else {
                continue;
            };
            let segments = wire_segments_on_plane(wire, plane, tolerance);
            if segments.is_empty() {
                continue;
            }
            sources
                .entry(handle)
                .or_insert_with(|| BoundarySource {
                    segments: Vec::new(),
                    curves: Vec::new(),
                })
                .segments
                .extend(segments);
        }
        for (&handle, source) in &mut sources {
            source.curves = self
                .document
                .get_entity(handle)
                .map(|entity| entity_curves_on_plane(entity, plane, tolerance))
                .unwrap_or_default();
            source.curves.retain(|curve| {
                source.segments.iter().any(|segment| {
                    distance_to(curve, segment.start).max(distance_to(curve, segment.end))
                        <= tolerance * 4.0
                })
            });
        }
        sources
    }
}

fn hatch_path_geometry(
    path: &acadrust::entities::BoundaryPath,
) -> (Vec<[f64; 2]>, Vec<f64>) {
    let edges: Vec<_> = path
        .edges
        .iter()
        .filter_map(crate::entities::hatch::edge_curve)
        .map(|curve| curve.tessellate_angle(cadkernel::tessellation::DEFAULT_ANGLE))
        .collect();
    super::entity::chain_path_edges_with_directions(edges)
}

pub(crate) fn hatch_path_ring(path: &acadrust::entities::BoundaryPath) -> Option<Vec<[f64; 2]>> {
    let (ring, _) = hatch_path_geometry(path);
    (ring.len() >= 3).then_some(ring)
}

pub(crate) fn hatch_path_directions(path: &acadrust::entities::BoundaryPath) -> Vec<f64> {
    hatch_path_geometry(path).1
}

pub(crate) fn hatch_boundary_rings(hatch: &acadrust::entities::Hatch) -> Vec<Vec<[f64; 2]>> {
    hatch.paths.iter().filter_map(hatch_path_ring).collect()
}

pub(crate) fn separated_hatch_path_groups(
    hatch: &acadrust::entities::Hatch,
) -> Vec<Vec<acadrust::entities::BoundaryPath>> {
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

pub(crate) fn boundary_faces(
    sources: &rustc_hash::FxHashMap<Handle, BoundarySource>,
    tolerance: f64,
) -> Vec<Vec<[f64; 2]>> {
    let segments: Vec<_> = sources
        .values()
        .flat_map(|source| source.segments.iter().copied())
        .collect();
    bounded_faces(&segments, Tolerance::new(tolerance))
}
