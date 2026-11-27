//! Angular dimensions built from two lines or from three points.

use document::entities::*;
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;
use ocs_geom::geom2d::line_line;

/// A two-line angular dimension whose arc sits at `arc_point`, for a
/// dimensional constraint; `None` when the lines are parallel or the arc
/// point picks no sector.
pub(crate) fn angular_two_line_entity(
    first_start: DVec3,
    first_end: DVec3,
    second_start: DVec3,
    second_end: DVec3,
    arc_point: DVec3,
    text: Option<String>,
) -> Option<EntityType> {
    two_line_frame(first_start, first_end, second_start, second_end, arc_point)?;
    let mut dim = DimensionAngular2Ln::default();
    dim.first_point = v3(first_start);
    dim.second_point = v3(first_end);
    dim.angle_vertex = v3(second_start);
    dim.definition_point = v3(second_end);
    dim.dimension_arc = v3(arc_point);
    dim.base.definition_point = dim.definition_point;
    // No stored text point: the style places the text (DIMTAD) beside the
    // arc at render time, as the reference draws a dynamic dimension.
    dim.base.insertion_point = dim.dimension_arc;
    dim.base.actual_measurement = dim.measurement_radians();
    crate::entities::dimension::set_dimension_text_override(&mut dim.base, text);
    Some(EntityType::Dimension(Box::new(Dimension::Angular2Ln(dim))))
}

/// A three-point angular dimension (vertex, two points, arc point), for a
/// dimensional constraint.
pub(crate) fn angular_three_point_entity(
    vertex: DVec3,
    first: DVec3,
    second: DVec3,
    arc_point: DVec3,
    text: Option<String>,
) -> Option<EntityType> {
    let mut dim = DimensionAngular3Pt::new(v3(vertex), v3(first), v3(second));
    dim.definition_point = v3(arc_point);
    dim.base.definition_point = dim.definition_point;
    dim.base.text_middle_point = Vector3::new(0.0, 0.0, 0.0);
    dim.base.insertion_point = dim.definition_point;
    dim.base.actual_measurement = dim.measurement_radians();
    crate::entities::dimension::set_dimension_text_override(&mut dim.base, text);
    Some(EntityType::Dimension(Box::new(Dimension::Angular3Pt(dim))))
}

pub(crate) fn two_line_frame(
    first_start: DVec3,
    first_end: DVec3,
    second_start: DVec3,
    second_end: DVec3,
    arc_point: DVec3,
) -> Option<(DVec3, f64, f64)> {
    let first_direction = first_end - first_start;
    let second_direction = second_end - second_start;
    let (t, _) = line_line(
        [first_start.x, first_start.y],
        [first_direction.x, first_direction.y],
        [second_start.x, second_start.y],
        [second_direction.x, second_direction.y],
    )?;
    let vertex = first_start + first_direction * t;
    let target = (arc_point.y - vertex.y).atan2(arc_point.x - vertex.x);
    let mut best: Option<(f64, f64, f64)> = None;
    for first_sign in [1.0, -1.0] {
        for second_sign in [1.0, -1.0] {
            let first = first_direction * first_sign;
            let second = second_direction * second_sign;
            for (start, end) in [
                (first.y.atan2(first.x), second.y.atan2(second.x)),
                (second.y.atan2(second.x), first.y.atan2(first.x)),
            ] {
                let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
                if sweep <= 1.0e-9 || sweep > std::f64::consts::PI {
                    continue;
                }
                let selected = (target - start).rem_euclid(std::f64::consts::TAU);
                if selected <= sweep + 1.0e-9 && best.is_none_or(|known| sweep < known.2) {
                    best = Some((start, start + sweep, sweep));
                }
            }
        }
    }
    best.map(|(start, end, _)| (vertex, start, end))
}

pub(crate) fn v3(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}
