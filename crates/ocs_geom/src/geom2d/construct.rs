//! Circular arc construction.

use super::curve::Arc;
use super::polyline::BulgeArc;
use super::vec::Vec2;
use std::f64::consts::TAU;

const ANGLE_EPSILON: f64 = 1.0e-9;

/// A non-degenerate bounded arc from centre, radius and angles.
pub fn bounded_arc(
    centre: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> Option<Arc> {
    let sweep = (end_angle - start_angle).rem_euclid(TAU);
    if !radius.is_finite()
        || !start_angle.is_finite()
        || !end_angle.is_finite()
        || radius <= ANGLE_EPSILON
        || sweep <= ANGLE_EPSILON
    {
        return None;
    }
    Some(Arc {
        centre,
        radius,
        start_angle,
        end_angle,
    })
}

fn stored_arc(start: [f64; 2], end: [f64; 2], bulge: f64) -> Option<Arc> {
    let arc = BulgeArc::from_bulge(start, end, bulge)?;
    let (start_angle, end_angle) = if arc.sweep > 0.0 {
        (arc.start_angle, arc.end_angle)
    } else {
        (arc.end_angle, arc.start_angle)
    };
    Some(Arc {
        centre: arc.center,
        radius: arc.radius,
        start_angle,
        end_angle,
    })
}

/// Arc through two endpoints with a signed included angle.
pub fn arc_from_endpoints_angle(
    start: [f64; 2],
    end: [f64; 2],
    included: f64,
) -> Option<Arc> {
    let sweep = included.abs();
    if !included.is_finite()
        || sweep <= ANGLE_EPSILON
        || sweep >= TAU - ANGLE_EPSILON
    {
        return None;
    }
    stored_arc(start, end, (included * 0.25).tan())
}

/// Arc through two endpoints with minor/major selected by radius sign.
pub fn arc_from_endpoints_radius(
    start: [f64; 2],
    end: [f64; 2],
    signed_radius: f64,
) -> Option<Arc> {
    let chord = Vec2::from(start).distance(Vec2::from(end));
    let radius = signed_radius.abs();
    if !radius.is_finite() || radius <= ANGLE_EPSILON || chord > 2.0 * radius {
        return None;
    }
    let signed_chord = chord.copysign(signed_radius);
    let sweep = arc_sweep_from_chord(radius, signed_chord)?;
    arc_from_endpoints_angle(start, end, sweep)
}

/// Arc through two endpoints with a signed sagitta.
pub fn arc_from_sagitta(
    start: [f64; 2],
    end: [f64; 2],
    sagitta: f64,
) -> Option<Arc> {
    let chord = Vec2::from(start).distance(Vec2::from(end));
    if !sagitta.is_finite() || chord <= ANGLE_EPSILON {
        return None;
    }
    stored_arc(start, end, -2.0 * sagitta / chord)
}

/// Arc from a start tangent to an endpoint.
pub fn arc_from_start_tangent(
    start: [f64; 2],
    tangent: [f64; 2],
    end: [f64; 2],
    flip: bool,
) -> Option<Arc> {
    let start = Vec2::from(start);
    let end = Vec2::from(end);
    let tangent = Vec2::from(tangent).normalize()?;
    let chord = end - start;
    let chord_squared = chord.length_squared();
    if chord_squared <= ANGLE_EPSILON * ANGLE_EPSILON {
        return None;
    }
    let normal = tangent.perpendicular();
    let denominator = normal.dot(chord);
    if denominator.abs() <= chord.length() * ANGLE_EPSILON {
        return None;
    }
    let centre = start + normal * (chord_squared / (2.0 * denominator));
    let radius = centre.distance(start);
    let start_angle = (start - centre).angle();
    let end_angle = (end - centre).angle();
    let ccw_tangent = (start - centre).perpendicular();
    let forward = ccw_tangent.dot(tangent) >= 0.0;
    let (start_angle, end_angle) = if forward ^ flip {
        (start_angle, end_angle)
    } else {
        (end_angle, start_angle)
    };
    Some(Arc {
        centre: centre.to_array(),
        radius,
        start_angle,
        end_angle,
    })
}

/// Arc through three ordered points.
pub fn arc_through_points(
    start: [f64; 2],
    middle: [f64; 2],
    end: [f64; 2],
) -> Option<Arc> {
    let start = Vec2::from(start);
    let middle = Vec2::from(middle);
    let end = Vec2::from(end);
    let u = middle - start;
    let v = end - start;
    let scale_squared = u
        .length_squared()
        .max(v.length_squared())
        .max((end - middle).length_squared());
    let determinant = 2.0 * u.cross(v);
    if scale_squared <= ANGLE_EPSILON * ANGLE_EPSILON
        || determinant.abs() <= scale_squared * 1.0e-12
    {
        return None;
    }
    let centre = start
        + Vec2::new(
            (u.length_squared() * v.y - v.length_squared() * u.y) / determinant,
            (u.x * v.length_squared() - v.x * u.length_squared()) / determinant,
        );
    let radius = centre.distance(start);
    let start_angle = (start - centre).angle();
    let middle_angle = (middle - centre).angle();
    let end_angle = (end - centre).angle();
    let sweep = (end_angle - start_angle).rem_euclid(TAU);
    let to_middle = (middle_angle - start_angle).rem_euclid(TAU);
    let (start_angle, end_angle) = if to_middle <= sweep + ANGLE_EPSILON {
        (start_angle, end_angle)
    } else {
        (end_angle, start_angle)
    };
    Some(Arc {
        centre: centre.to_array(),
        radius,
        start_angle,
        end_angle,
    })
}

/// Minor/major sweep selected by chord sign.
pub fn arc_sweep_from_chord(radius: f64, signed_chord: f64) -> Option<f64> {
    let chord = signed_chord.abs();
    if !radius.is_finite()
        || !signed_chord.is_finite()
        || radius <= ANGLE_EPSILON
        || chord <= ANGLE_EPSILON
        || chord > 2.0 * radius
    {
        return None;
    }
    let minor = 2.0 * (chord / (2.0 * radius)).clamp(0.0, 1.0).asin();
    Some(if signed_chord > 0.0 {
        minor
    } else {
        TAU - minor
    })
}
