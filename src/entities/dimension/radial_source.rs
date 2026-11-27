//! Where a radial dimension's extension runs to on the curve it measures:
//! the arc, circle or region boundary its source names.

use std::f64::consts::TAU;
use document::EntityType;
use document::entities::Dimension;
use document::objects::{AssociativeData, ObjectType};
use document::types::{Handle, Vector3};
use ocs_geom::geom2d::{
    angle_within_arc, arc as tessellate_arc, arc_span, closest_point, Arc as KernelArc,
    Circle as KernelCircle, Curve as KernelCurve, DEFAULT_SEGMENTS_PER_RADIAN,
};
use ocs_geom::space::Plane;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RadialSourceGeometry {
    pub plane: Plane,
    pub center: [f64; 2],
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub limited: bool,
    pub marker: i32,
}

pub(crate) type DPoint = [f64; 3];

pub(crate) fn vector3(point: DPoint) -> Vector3 {
    Vector3::new(point[0], point[1], point[2])
}

pub(crate) fn dpoint(point: Vector3) -> DPoint {
    [point.x, point.y, point.z]
}

pub(crate) struct RegionBoundaryPiece {
    pub(crate) curve: KernelCurve,
    pub(crate) radial: Option<RadialSourceGeometry>,
}

pub(crate) fn certified_rational_quadratic_circle(
    nurbs: &ocs_geom::geom2d::NurbsCurve,
) -> Option<([f64; 2], f64, f64, f64)> {
    if nurbs.degree() != 2 {
        return None;
    }
    let knots = nurbs.knots();
    if knots.len() != 6 {
        return None;
    }
    let t0 = knots[0];
    let t1 = knots[5];
    let knot_tol = 1e-12 * (1.0 + t0.abs().max(t1.abs()));
    if (knots[1] - t0).abs() > knot_tol
        || (knots[2] - t0).abs() > knot_tol
        || (knots[3] - t1).abs() > knot_tol
        || (knots[4] - t1).abs() > knot_tol
        || t1 <= t0 + knot_tol
    {
        return None;
    }
    let cps = nurbs.control_points();
    let weights = nurbs.weights();
    if cps.len() != 3 || weights.len() != 3 {
        return None;
    }
    let w0 = weights[0];
    let w1_raw = weights[1];
    let w2 = weights[2];
    if !w0.is_finite()
        || !w1_raw.is_finite()
        || !w2.is_finite()
        || w0 <= 0.0
        || w1_raw <= 0.0
        || w2 <= 0.0
    {
        return None;
    }
    let w_scale = (w0 * w2).sqrt();
    let weight_tol = 1e-7 * (w0 + w2);
    if (w0 - w2).abs() > weight_tol {
        return None;
    }
    let w = w1_raw / w_scale;
    if w <= 1e-9 || w >= 1.0 - 1e-12 {
        return None;
    }

    let p0 = cps[0];
    let p1 = cps[1];
    let p2 = cps[2];
    let v1 = [p1[0] - p0[0], p1[1] - p0[1]];
    let v2 = [p2[0] - p0[0], p2[1] - p0[1]];
    let t1 = [v2[0] - v1[0], v2[1] - v1[1]];

    let l0_sq = v1[0] * v1[0] + v1[1] * v1[1];
    let l1_sq = t1[0] * t1[0] + t1[1] * t1[1];
    let l0 = l0_sq.sqrt();
    let l1 = l1_sq.sqrt();
    let geom_scale = l0.max(l1);
    if geom_scale <= 1e-12 {
        return None;
    }

    // Isosceles control triangle (equal tangent lengths)
    let geom_tol = 1e-7 * geom_scale;
    if (l0 - l1).abs() > geom_tol {
        return None;
    }

    // Non-degenerate chord
    let chord_sq = v2[0] * v2[0] + v2[1] * v2[1];
    let chord = chord_sq.sqrt();
    if chord <= 1e-12 {
        return None;
    }

    // Midpoint symmetry: (P1 - M) . (P2 - P0) == 0
    let m_rel = [v2[0] * 0.5, v2[1] * 0.5];
    let p1_minus_m = [v1[0] - m_rel[0], v1[1] - m_rel[1]];
    let dot = p1_minus_m[0] * v2[0] + p1_minus_m[1] * v2[1];
    if dot.abs() > 1e-6 * geom_scale * chord {
        return None;
    }

    // Conic circle identity: w = chord / (2 * l0)
    let expected_w = chord / (2.0 * l0);
    if (w - expected_w).abs() > 1e-6 {
        return None;
    }

    // Center in local coordinates relative to P0
    let w_sq = w * w;
    let denom = 1.0 - w_sq;
    if denom <= 1e-12 {
        return None;
    }
    let c_rel = [
        (m_rel[0] - w_sq * v1[0]) / denom,
        (m_rel[1] - w_sq * v1[1]) / denom,
    ];
    let center = [p0[0] + c_rel[0], p0[1] + c_rel[1]];
    let radius = c_rel[0].hypot(c_rel[1]);
    if radius <= 1e-12 {
        return None;
    }

    // Verify distance from center to P2 equals radius within relative tolerance
    let p2_rel = [v2[0] - c_rel[0], v2[1] - c_rel[1]];
    let r2 = p2_rel[0].hypot(p2_rel[1]);
    if (r2 - radius).abs() > 1e-6 * radius {
        return None;
    }

    // Angles
    let th0 = (-c_rel[1]).atan2(-c_rel[0]);
    let th2 = (p2_rel[1]).atan2(p2_rel[0]);
    let shoulder_denom = 2.0 * (1.0 + w);
    let shoulder_rel = [
        (2.0 * w * v1[0] + v2[0]) / shoulder_denom - c_rel[0],
        (2.0 * w * v1[1] + v2[1]) / shoulder_denom - c_rel[1],
    ];
    let th_mid = shoulder_rel[1].atan2(shoulder_rel[0]);
    let span_0_mid = (th_mid - th0).rem_euclid(TAU);
    let span_0_2 = (th2 - th0).rem_euclid(TAU);
    let (start_angle, end_angle) = if span_0_mid <= span_0_2 {
        (th0, th2)
    } else {
        (th2, th0)
    };

    Some((center, radius, start_angle, end_angle))
}

pub(crate) fn curve_as_circular(curve: &KernelCurve) -> Option<([f64; 2], f64, f64, f64, bool)> {
    match curve {
        KernelCurve::Circle(circle) => Some((circle.centre, circle.radius, 0.0, 0.0, true)),
        KernelCurve::Arc(arc) => Some((
            arc.centre,
            arc.radius,
            arc.start_angle,
            arc.end_angle,
            false,
        )),
        KernelCurve::Nurbs(nurbs) => {
            let (center, radius, start, end) = certified_rational_quadratic_circle(nurbs)?;
            Some((center, radius, start, end, false))
        }
        _ => None,
    }
}

pub(crate) fn loop_is_full_circle(pieces: &[KernelCurve]) -> bool {
    if pieces.is_empty() {
        return false;
    }
    if pieces.len() == 1 {
        if let KernelCurve::Circle(_) = &pieces[0] {
            return true;
        }
    }
    let mut total_sweep = 0.0;
    let mut expected_center: Option<[f64; 2]> = None;
    let mut expected_radius = 0.0;
    for piece in pieces {
        let Some((center, radius, start, end, full)) = curve_as_circular(piece) else {
            return false;
        };
        if full {
            return true;
        }
        if let Some(c0) = expected_center {
            if (c0[0] - center[0]).hypot(c0[1] - center[1]) > 1e-4
                || (expected_radius - radius).abs() > 1e-4
            {
                return false;
            }
        } else {
            expected_center = Some(center);
            expected_radius = radius;
        }
        total_sweep += positive_sweep(start, end);
    }
    (total_sweep - TAU).abs() <= 1e-3
}

pub(crate) fn region_boundary_pieces(entity: &EntityType) -> Option<(Plane, Vec<RegionBoundaryPiece>)> {
    let (plane, loops, _) = crate::entities::profile::profile_geometry(entity)?;
    let unit = Plane::orthonormal(plane.origin, plane.x_axis, plane.normal()?)?;
    let transform = ocs_geom::geom2d::Transform {
        origin: unit.project(plane.origin)?.into(),
        x_axis: unit.project_vector(plane.x_axis)?.into(),
        y_axis: unit.project_vector(plane.y_axis)?.into(),
    };
    let mut pieces = Vec::new();
    for loop_curves in loops {
        let transformed_loop: Vec<KernelCurve> = loop_curves
            .into_iter()
            .map(|curve| curve.transformed(&transform))
            .collect::<Option<Vec<_>>>()?;
        let is_circle_loop = loop_is_full_circle(&transformed_loop);
        for curve in transformed_loop {
            let marker = pieces.len() as i32;
            let radial = curve_as_circular(&curve).map(|(center, radius, start, end, full)| {
                let limited = !full && !is_circle_loop;
                let (start_angle, end_angle) = if limited { (start, end) } else { (0.0, 0.0) };
                RadialSourceGeometry {
                    plane: unit,
                    center,
                    radius,
                    start_angle,
                    end_angle,
                    limited,
                    marker,
                }
            });
            pieces.push(RegionBoundaryPiece { curve, radial });
        }
    }
    Some((unit, pieces))
}

pub(crate) fn radial_candidates(entity: &EntityType) -> Vec<RadialSourceGeometry> {
    if matches!(entity, EntityType::Region(_)) {
        let Some((_, pieces)) = region_boundary_pieces(entity) else {
            return Vec::new();
        };
        return pieces
            .into_iter()
            .filter_map(|piece| piece.radial)
            .collect();
    }
    let Some(planar) = crate::entities::curve::entity_curve(entity) else {
        return Vec::new();
    };
    match planar.curve {
        KernelCurve::Circle(circle) => vec![RadialSourceGeometry {
            plane: planar.plane,
            center: circle.centre,
            radius: circle.radius,
            start_angle: 0.0,
            end_angle: 0.0,
            limited: false,
            marker: 0,
        }],
        KernelCurve::Arc(arc) => vec![RadialSourceGeometry {
            plane: planar.plane,
            center: arc.centre,
            radius: arc.radius,
            start_angle: arc.start_angle,
            end_angle: arc.end_angle,
            limited: true,
            marker: 0,
        }],
        KernelCurve::Polyline(polyline) => (0..polyline.vertices.len())
            .filter_map(|index| {
                let arc = polyline.segment_arc(index)?;
                let (start_angle, end_angle) = if arc.sweep >= 0.0 {
                    (arc.start_angle, arc.end_angle)
                } else {
                    (arc.end_angle, arc.start_angle)
                };
                Some(RadialSourceGeometry {
                    plane: planar.plane,
                    center: arc.center,
                    radius: arc.radius,
                    start_angle,
                    end_angle,
                    limited: true,
                    marker: index as i32,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn radial_source_for_marker(entity: &EntityType, marker: i32) -> Option<RadialSourceGeometry> {
    if matches!(entity, EntityType::Region(_)) {
        let (_, pieces) = region_boundary_pieces(entity)?;
        let index = usize::try_from(marker).ok()?;
        return pieces.get(index).and_then(|piece| piece.radial);
    }
    radial_candidates(entity)
        .into_iter()
        .find(|candidate| candidate.marker == marker)
}

pub(crate) fn positive_sweep(start: f64, end: f64) -> f64 {
    let raw = end - start;
    let mut sweep = raw.rem_euclid(TAU);
    if sweep <= 1.0e-12 && raw.abs() > 1.0e-12 {
        sweep = TAU;
    }
    sweep
}

pub(crate) fn radial_extension_points(
    document: &document::Document,
    dimension: Handle,
    gap: f64,
    extension: f64,
) -> Option<Vec<Vector3>> {
    let EntityType::Dimension(dimension_entity) = document.get_entity(dimension)? else {
        return None;
    };
    let target_point = match &**dimension_entity {
        Dimension::Radius(radius) => radius.definition_point,
        Dimension::Diameter(diameter) => diameter.angle_vertex,
        Dimension::LargeRadial(radial) => radial.chord_point,
        _ => return None,
    };
    let association = document.objects.values().find_map(|object| {
        let ObjectType::Associative(object) = object else {
            return None;
        };
        let AssociativeData::DimensionAssociation(association) = &object.data else {
            return None;
        };
        (association.dimension == dimension).then_some(association)
    })?;
    let reference = association.references[0].first()?;
    let source = document.get_entity(*reference.xrefs.first()?)?;
    let radial = radial_source_for_marker(source, reference.main_gs_marker)?;
    if !radial.limited || radial.radius <= 1e-12 {
        return None;
    }
    let target = radial.angle_at(dpoint(target_point));
    if radial.contains_angle(target) {
        return None;
    }

    let from_start = arc_span(target, radial.start_angle);
    let from_end = arc_span(radial.end_angle, target);
    let gap_angle = gap.max(0.0) / radial.radius;
    let extension_angle = extension.max(0.0) / radial.radius;
    let span = from_start.min(from_end);
    if span <= gap_angle + 1e-10 {
        return None;
    }
    let (start, end, reverse) = if from_start <= from_end {
        (
            target - extension_angle,
            radial.start_angle - gap_angle,
            true,
        )
    } else {
        (
            radial.end_angle + gap_angle,
            target + extension_angle,
            false,
        )
    };
    let mut points: Vec<_> = tessellate_arc(
        radial.center,
        radial.radius,
        start,
        end,
        0.0,
        DEFAULT_SEGMENTS_PER_RADIAN,
    )
    .into_iter()
    .map(|point| vector3(radial.plane.point_at([point[0], point[1]])))
    .collect();
    if reverse {
        points.reverse();
    }
    Some(points)
}

impl RadialSourceGeometry {
    pub(crate) fn center_world(self) -> Vector3 {
        vector3(self.plane.point_at(self.center))
    }

    pub(crate) fn point_at_angle(self, angle: f64) -> Vector3 {
        vector3(self.plane.point_at([
            self.center[0] + self.radius * angle.cos(),
            self.center[1] + self.radius * angle.sin(),
        ]))
    }

    pub(crate) fn angle_at(self, point: DPoint) -> f64 {
        let projected = self.plane.project(point).unwrap_or(self.center);
        (projected[1] - self.center[1]).atan2(projected[0] - self.center[0])
    }

    pub(crate) fn chord_at(self, point: DPoint) -> Vector3 {
        self.point_at_angle(self.angle_at(point))
    }

    pub(crate) fn opposite_chord_at(self, point: DPoint) -> Vector3 {
        self.point_at_angle(self.angle_at(point) + std::f64::consts::PI)
    }

    pub(crate) fn contains_angle(self, angle: f64) -> bool {
        !self.limited || angle_within_arc(angle, self.start_angle, self.end_angle)
    }

    pub(crate) fn curve(self) -> KernelCurve {
        if self.limited {
            KernelCurve::Arc(KernelArc {
                centre: self.center,
                radius: self.radius,
                start_angle: self.start_angle,
                end_angle: self.end_angle,
            })
        } else {
            KernelCurve::Circle(KernelCircle {
                centre: self.center,
                radius: self.radius,
            })
        }
    }

    pub(crate) fn distance_squared_to(self, point: DPoint) -> f64 {
        let Some(projected) = self.plane.project(point) else {
            return f64::INFINITY;
        };
        let planar = closest_point(&self.curve(), projected).distance;
        let world = self.plane.point_at(projected);
        let dx = world[0] - point[0];
        let dy = world[1] - point[1];
        let dz = world[2] - point[2];
        planar * planar + dx * dx + dy * dy + dz * dz
    }
}
