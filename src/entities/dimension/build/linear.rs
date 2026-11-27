//! A linear dimension built from its points and the axis it measures along.

use document::entities::*;
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;

/// A linear dimension between two points of the working plane, its
/// dimension line through `point` along `axis` — what DIMLINEAR places and
/// what a dynamic dimensional constraint draws.
pub(crate) fn linear_dimension_entity(
    first: DVec3,
    second: DVec3,
    point: DVec3,
    axis: DVec3,
    text_override: Option<String>,
) -> EntityType {
    let mut dim = DimensionLinear::new(v3(first), v3(second));
    dim.rotation = axis.y.atan2(axis.x);
    dim.set_offset(dimension_line_offset(second, point, axis));
    dim.base.definition_point = dim.definition_point;
    dim.base.text_middle_point = v3(linear_text_pos(first, second, point, axis));
    dim.base.insertion_point = dim.base.text_middle_point;
    crate::entities::dimension::reset_automatic_text_position(&mut dim.base);
    dim.base.actual_measurement = dim.measurement();
    crate::entities::dimension::set_dimension_text_override(&mut dim.base, text_override);
    EntityType::Dimension(Box::new(Dimension::Linear(dim)))
}

pub(crate) fn dimension_line_offset(second: DVec3, point: DVec3, axis: DVec3) -> f64 {
    let perpendicular = DVec3::new(-axis.y, axis.x, 0.0);
    (point - second).dot(perpendicular)
}

pub(crate) fn linear_text_pos(first: DVec3, second: DVec3, def: DVec3, axis: DVec3) -> DVec3 {
    let (d1, d2) = dim_line_endpoints(first, second, def, axis);
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    (d1 + d2) * 0.5 + perp * 0.15
}

/// Project the two extension origins onto the dimension line, which passes
/// through `def` along `axis`. Each origin is projected *independently*: a
/// single shared offset only lands both on the line when they are level, and
/// tilts the dimension line when they are not (e.g. measuring across sloped
/// points). See #181.
pub(crate) fn dim_line_endpoints(first: DVec3, second: DVec3, def: DVec3, axis: DVec3) -> (DVec3, DVec3) {
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let dperp = def.dot(perp);
    let d1 = first + perp * (dperp - first.dot(perp));
    let d2 = second + perp * (dperp - second.dot(perp));
    (d1, d2)
}

pub(crate) fn v3(pt: DVec3) -> Vector3 {
    Vector3::new(pt.x, pt.y, pt.z)
}
