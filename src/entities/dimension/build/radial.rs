//! Radius and diameter dimensions built from a centre and a point on the
//! curve.

use document::entities::*;
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;

/// A radius dimension for a dimensional constraint: the leader leaves the
/// centre towards `location`, and the style places the text.
pub(crate) fn radius_constraint_entity(
    center: DVec3,
    radius: f64,
    location: DVec3,
    text: Option<String>,
) -> Option<EntityType> {
    let direction = radial_direction(center, location)?;
    let chord = center + direction * radius;
    let mut dim = DimensionRadius::new(v3(center), v3(chord));
    dim.base.definition_point = v3(chord);
    dim.leader_length = chord.distance(location).max(radius * 0.5);
    dim.base.actual_measurement = dim.measurement();
    crate::entities::dimension::set_dimension_text_override(&mut dim.base, text);
    Some(EntityType::Dimension(Box::new(Dimension::Radius(dim))))
}

/// The unit direction a radial constraint's dimension points in, from the
/// centre to where its dimension line was picked.
pub(crate) fn radial_direction(center: DVec3, location: DVec3) -> Option<DVec3> {
    let delta = location - center;
    (delta.length() > 1.0e-9).then(|| delta.normalize())
}

pub(crate) fn v3(pt: DVec3) -> Vector3 {
    Vector3::new(pt.x, pt.y, pt.z)
}

/// A diameter dimension for a dimensional constraint: the dimension line
/// runs through the centre towards `location`, and the style places the
/// text.
pub(crate) fn diameter_constraint_entity(
    center: DVec3,
    radius: f64,
    location: DVec3,
    text: Option<String>,
) -> Option<EntityType> {
    let direction = radial_direction(center, location)?;
    let chord = center + direction * radius;
    let far_chord = center - direction * radius;
    let mut dim = DimensionDiameter::new(v3(chord), v3(far_chord));
    dim.leader_length = chord.distance(location).max(radius * 0.5);
    dim.base.actual_measurement = dim.measurement();
    crate::entities::dimension::set_dimension_text_override(&mut dim.base, text);
    Some(EntityType::Dimension(Box::new(Dimension::Diameter(dim))))
}
