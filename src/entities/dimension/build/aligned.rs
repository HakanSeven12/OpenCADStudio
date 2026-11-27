//! An aligned dimension built from its three picked points.

use document::entities::*;
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;

/// An aligned dimension between two points of the working plane, its
/// dimension line through `point` — what DIMALIGNED places and what an
/// aligned dimensional constraint draws.
pub(crate) fn aligned_dimension_entity(
    first: DVec3,
    second: DVec3,
    point: DVec3,
    text_override: Option<String>,
) -> EntityType {
    let mut dim = DimensionAligned::new(v3(first), v3(second));
    // The placement point is stored so commit matches the preview.
    dim.definition_point = v3(point);
    dim.base.definition_point = v3(point);
    let (d1, d2) = dim_line_endpoints(first, second, point);
    dim.base.text_middle_point = v3((d1 + d2) * 0.5);
    dim.base.insertion_point = dim.base.text_middle_point;
    crate::entities::dimension::reset_automatic_text_position(&mut dim.base);
    dim.base.actual_measurement = dim.measurement();
    crate::entities::dimension::set_dimension_text_override(&mut dim.base, text_override);
    EntityType::Dimension(Box::new(Dimension::Aligned(dim)))
}

/// Dimension-line endpoints: the baseline `p1`–`p2` shifted to pass through
/// the cursor's perpendicular projection. Uses the XY-plane perpendicular so
/// it matches the committed entity's renderer (and DIMLINEAR). The old
/// preview used an XZ-plane perpendicular, drawing the offset in the wrong
/// spatial direction. (#150)
pub(crate) fn dim_line_endpoints(p1: DVec3, p2: DVec3, dim_pt: DVec3) -> (DVec3, DVec3) {
    let axis = (p2 - p1).normalize_or_zero();
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let offset = (dim_pt - p1).dot(perp);
    (p1 + perp * offset, p2 + perp * offset)
}

pub(crate) fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}
