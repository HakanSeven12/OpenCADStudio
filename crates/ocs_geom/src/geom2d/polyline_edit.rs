//! Shape-preserving edits for polyline segments.

use super::{circle_circle_points, fillet_between_rays, Polyline, Tolerance, Vec2};

const BULGE_EPSILON: f64 = 1.0e-9;
const TAU: f64 = std::f64::consts::TAU;

fn nearest_line_circle_intersection(
    line_point: Vec2,
    line_direction: Vec2,
    circle_center: Vec2,
    circle_radius: f64,
    near: Vec2,
) -> Option<Vec2> {
    let aa = line_direction.dot(line_direction);
    if aa <= Tolerance::default().linear().powi(2) {
        return None;
    }
    let relative = line_point - circle_center;
    let bb = 2.0 * relative.dot(line_direction);
    let cc = relative.dot(relative) - circle_radius * circle_radius;
    let discriminant = bb * bb - 4.0 * aa * cc;
    if discriminant < -Tolerance::default().linear() {
        return None;
    }
    let root = discriminant.max(0.0).sqrt();
    let first = line_point + line_direction * ((-bb - root) / (2.0 * aa));
    let second = line_point + line_direction * ((-bb + root) / (2.0 * aa));
    Some(if first.distance(near) <= second.distance(near) {
        first
    } else {
        second
    })
}

fn nearest_circle_circle_intersection(
    first_center: Vec2,
    first_radius: f64,
    second_center: Vec2,
    second_radius: f64,
    near: Vec2,
) -> Option<Vec2> {
    circle_circle_points(
        first_center.into(),
        first_radius,
        second_center.into(),
        second_radius,
    )
    .into_iter()
    .map(Vec2::from)
    .min_by(|a, b| a.distance(near).total_cmp(&b.distance(near)))
}

fn bulge_for_circle_arc(center: Vec2, start: Vec2, end: Vec2, direction: f64) -> Option<f64> {
    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let end_angle = (end.y - center.y).atan2(end.x - center.x);
    let sweep = if direction >= 0.0 {
        (end_angle - start_angle).rem_euclid(TAU)
    } else {
        -(start_angle - end_angle).rem_euclid(TAU)
    };
    let bulge = (sweep * 0.25).tan();
    bulge.is_finite().then_some(bulge.clamp(-1.0e6, 1.0e6))
}

/// Move a straight segment parallel and reconnect it to its neighboring lines or arcs.
pub fn move_polyline_segment_parallel(
    polyline: &mut Polyline,
    segment: usize,
    offset: f64,
) -> bool {
    let count = if polyline.closed {
        polyline.vertices.len()
    } else {
        polyline.vertices.len().saturating_sub(1)
    };
    if segment >= count || !offset.is_finite() {
        return false;
    }
    let vertex_count = polyline.vertices.len();
    let next_segment = (segment + 1) % vertex_count;
    if polyline.vertices[segment].bulge.abs() >= BULGE_EPSILON {
        return false;
    }
    let start = Vec2::from(polyline.vertices[segment].position);
    let end = Vec2::from(polyline.vertices[next_segment].position);
    let selected = end - start;
    let length = selected.length();
    if length <= Tolerance::default().linear() {
        return false;
    }
    let shifted_start = start + selected.perpendicular() * (offset / length);
    let line_intersection = |point: Vec2, direction: Vec2| -> Option<Vec2> {
        let cross = direction.cross(selected);
        if cross.abs() < BULGE_EPSILON * direction.length().max(1.0) * length.max(1.0) {
            return None;
        }
        Some(point + direction * ((shifted_start - point).cross(selected) / cross))
    };

    let mut previous_bulge = None;
    let new_start = if polyline.closed || segment > 0 {
        let previous = (segment + vertex_count - 1) % vertex_count;
        let outer = Vec2::from(polyline.vertices[previous].position);
        if let Some(neighbor) = polyline.segment_arc(previous) {
            let center = Vec2::from(neighbor.center);
            let Some(point) = nearest_line_circle_intersection(
                shifted_start,
                selected,
                center,
                neighbor.radius,
                start,
            ) else {
                return false;
            };
            let Some(bulge) = bulge_for_circle_arc(center, outer, point, neighbor.sweep.signum())
            else {
                return false;
            };
            previous_bulge = Some((previous, bulge));
            point
        } else {
            let Some(point) = line_intersection(outer, start - outer) else {
                return false;
            };
            point
        }
    } else {
        shifted_start
    };

    let mut next_bulge = None;
    let new_end = if polyline.closed || segment + 1 < count {
        let outer_index = (next_segment + 1) % vertex_count;
        let outer = Vec2::from(polyline.vertices[outer_index].position);
        if let Some(neighbor) = polyline.segment_arc(next_segment) {
            let center = Vec2::from(neighbor.center);
            let Some(point) = nearest_line_circle_intersection(
                shifted_start,
                selected,
                center,
                neighbor.radius,
                end,
            ) else {
                return false;
            };
            let Some(bulge) = bulge_for_circle_arc(center, point, outer, neighbor.sweep.signum())
            else {
                return false;
            };
            next_bulge = Some((next_segment, bulge));
            point
        } else {
            let Some(point) = line_intersection(end, outer - end) else {
                return false;
            };
            point
        }
    } else {
        end + selected.perpendicular() * (offset / length)
    };

    polyline.vertices[segment].position = new_start.into();
    polyline.vertices[next_segment].position = new_end.into();
    if let Some((index, bulge)) = previous_bulge {
        polyline.vertices[index].bulge = bulge;
    }
    if let Some((index, bulge)) = next_bulge {
        polyline.vertices[index].bulge = bulge;
    }
    true
}

fn resize_arc_concentrically(polyline: &mut Polyline, segment: usize, radius: f64) -> bool {
    let count = if polyline.closed {
        polyline.vertices.len()
    } else {
        polyline.vertices.len().saturating_sub(1)
    };
    if segment >= count || !radius.is_finite() || radius <= Tolerance::default().linear() {
        return false;
    }
    let vertex_count = polyline.vertices.len();
    let next_segment = (segment + 1) % vertex_count;
    let start = Vec2::from(polyline.vertices[segment].position);
    let end = Vec2::from(polyline.vertices[next_segment].position);
    let Some(arc) = polyline.segment_arc(segment) else {
        return false;
    };
    let center = Vec2::from(arc.center);
    let linear_squared = Tolerance::default().linear().powi(2);

    let mut sync_previous = None;
    let mut previous_bulge = None;
    let new_start = if polyline.closed || segment > 0 {
        let previous = (segment + vertex_count - 1) % vertex_count;
        let outer = Vec2::from(polyline.vertices[previous].position);
        if let Some(neighbor) = polyline.segment_arc(previous) {
            let neighbor_center = Vec2::from(neighbor.center);
            let Some(point) = nearest_circle_circle_intersection(
                center,
                radius,
                neighbor_center,
                neighbor.radius,
                start,
            ) else {
                return false;
            };
            let Some(bulge) =
                bulge_for_circle_arc(neighbor_center, outer, point, neighbor.sweep.signum())
            else {
                return false;
            };
            previous_bulge = Some((previous, bulge));
            point
        } else if (start - outer).length_squared() <= linear_squared {
            sync_previous = Some(previous);
            center + (start - center) * (radius / arc.radius)
        } else {
            let Some(point) =
                nearest_line_circle_intersection(outer, start - outer, center, radius, start)
            else {
                return false;
            };
            point
        }
    } else {
        center + (start - center) * (radius / arc.radius)
    };

    let mut sync_next = None;
    let mut next_bulge = None;
    let new_end = if polyline.closed || segment + 1 < count {
        let outer_index = (next_segment + 1) % vertex_count;
        let outer = Vec2::from(polyline.vertices[outer_index].position);
        if let Some(neighbor) = polyline.segment_arc(next_segment) {
            let neighbor_center = Vec2::from(neighbor.center);
            let Some(point) = nearest_circle_circle_intersection(
                center,
                radius,
                neighbor_center,
                neighbor.radius,
                end,
            ) else {
                return false;
            };
            let Some(bulge) =
                bulge_for_circle_arc(neighbor_center, point, outer, neighbor.sweep.signum())
            else {
                return false;
            };
            next_bulge = Some((next_segment, bulge));
            point
        } else if (outer - end).length_squared() <= linear_squared {
            sync_next = Some(outer_index);
            center + (end - center) * (radius / arc.radius)
        } else {
            let Some(point) =
                nearest_line_circle_intersection(end, outer - end, center, radius, end)
            else {
                return false;
            };
            point
        }
    } else {
        center + (end - center) * (radius / arc.radius)
    };

    let Some(new_bulge) = bulge_for_circle_arc(center, new_start, new_end, arc.sweep.signum())
    else {
        return false;
    };
    polyline.vertices[segment].position = new_start.into();
    polyline.vertices[next_segment].position = new_end.into();
    if let Some(index) = sync_previous {
        polyline.vertices[index].position = new_start.into();
    }
    if let Some(index) = sync_next {
        polyline.vertices[index].position = new_end.into();
    }
    if let Some((index, bulge)) = previous_bulge {
        polyline.vertices[index].bulge = bulge;
    }
    if let Some((index, bulge)) = next_bulge {
        polyline.vertices[index].bulge = bulge;
    }
    polyline.vertices[segment].bulge = new_bulge;
    true
}

fn arc_fillet_frame(polyline: &Polyline, segment: usize) -> Option<(Vec2, Vec2, Vec2)> {
    let count = if polyline.closed {
        polyline.vertices.len()
    } else {
        polyline.vertices.len().checked_sub(1)?
    };
    if segment >= count || (!polyline.closed && (segment == 0 || segment + 1 == count)) {
        return None;
    }
    let vertex_count = polyline.vertices.len();
    let next_segment = (segment + 1) % vertex_count;
    let previous = (segment + vertex_count - 1) % vertex_count;
    let outer_index = (next_segment + 1) % vertex_count;
    if polyline.vertices[previous].bulge.abs() >= BULGE_EPSILON
        || polyline.vertices[next_segment].bulge.abs() >= BULGE_EPSILON
    {
        return None;
    }
    let outer_start = Vec2::from(polyline.vertices[previous].position);
    let tangent_start = Vec2::from(polyline.vertices[segment].position);
    let tangent_end = Vec2::from(polyline.vertices[next_segment].position);
    let outer_end = Vec2::from(polyline.vertices[outer_index].position);
    let incoming = (tangent_start - outer_start).normalize()?;
    let outgoing = (outer_end - tangent_end).normalize()?;
    let arc = polyline.segment_arc(segment)?;
    let center = Vec2::from(arc.center);
    let tangent_tolerance = (arc.radius * 1.0e-6).max(Tolerance::default().linear());
    if (tangent_start - center).dot(incoming).abs() > tangent_tolerance
        || (tangent_end - center).dot(outgoing).abs() > tangent_tolerance
    {
        return None;
    }
    let cross = incoming.cross(outgoing);
    if cross.abs() <= BULGE_EPSILON {
        return None;
    }
    let apex = outer_start + incoming * ((tangent_end - outer_start).cross(outgoing) / cross);
    Some((
        apex,
        (outer_start - apex).normalize()?,
        (outer_end - apex).normalize()?,
    ))
}

/// Radius selected by a cursor point for an arc segment.
pub fn polyline_arc_radius_from_point(
    polyline: &Polyline,
    segment: usize,
    point: [f64; 2],
) -> Option<f64> {
    let arc = polyline.segment_arc(segment)?;
    let point = Vec2::from(point);
    let radius = if let Some((apex, _, _)) = arc_fillet_frame(polyline, segment) {
        let midpoint = Vec2::from(arc.sample(0.5));
        let ray = midpoint - apex;
        arc.radius * (point - apex).dot(ray) / ray.dot(ray)
    } else {
        point.distance(Vec2::from(arc.center))
    };
    (radius > Tolerance::default().linear()).then_some(radius)
}

/// Signed parallel offset selected by a cursor point for a straight segment.
pub fn polyline_segment_parallel_offset(
    polyline: &Polyline,
    segment: usize,
    point: [f64; 2],
) -> Option<f64> {
    let count = if polyline.closed {
        polyline.vertices.len()
    } else {
        polyline.vertices.len().saturating_sub(1)
    };
    if segment >= count || polyline.vertices[segment].bulge.abs() >= BULGE_EPSILON {
        return None;
    }
    let start = Vec2::from(polyline.vertices[segment].position);
    let end = Vec2::from(polyline.vertices[(segment + 1) % polyline.vertices.len()].position);
    let direction = end - start;
    let length = direction.length();
    if length <= Tolerance::default().linear() {
        return None;
    }
    Some((Vec2::from(point) - start).dot(direction.perpendicular() / length))
}

/// Change an arc segment's radius while preserving its connected geometry.
pub fn resize_polyline_arc(polyline: &mut Polyline, segment: usize, radius: f64) -> bool {
    if radius <= Tolerance::default().linear() || !radius.is_finite() {
        return false;
    }
    if polyline.segment_arc(segment).is_none() {
        return false;
    }
    if let Some((apex, keep_start, keep_end)) = arc_fillet_frame(polyline, segment) {
        let Some(fillet) =
            fillet_between_rays(apex.into(), keep_start.into(), keep_end.into(), radius)
        else {
            return false;
        };
        let new_start = Vec2::from(fillet.tangent1);
        let new_end = Vec2::from(fillet.tangent2);
        let center = Vec2::from(fillet.centre);
        let (Some(radial_start), Some(radial_end)) = (
            (new_start - center).normalize(),
            (new_end - center).normalize(),
        ) else {
            return false;
        };
        let sweep = radial_start.dot(radial_end).clamp(-1.0, 1.0).acos();
        let bulge = polyline.vertices[segment].bulge.signum() * (sweep * 0.25).tan();
        if !bulge.is_finite() {
            return false;
        }
        let next = (segment + 1) % polyline.vertices.len();
        polyline.vertices[segment].position = new_start.into();
        polyline.vertices[next].position = new_end.into();
        polyline.vertices[segment].bulge = bulge;
        true
    } else {
        resize_arc_concentrically(polyline, segment, radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom2d::PolylineVertex;

    fn polyline(points: &[[f64; 2]], closed: bool) -> Polyline {
        Polyline {
            vertices: points
                .iter()
                .copied()
                .map(PolylineVertex::straight)
                .collect(),
            closed,
        }
    }

    #[test]
    fn moving_an_internal_segment_reconnects_its_neighboring_lines() {
        let mut shape = polyline(&[[0.0, 0.0], [2.0, 2.0], [8.0, 2.0], [10.0, 0.0]], false);
        assert!(move_polyline_segment_parallel(&mut shape, 1, 2.0));
        assert_eq!(shape.vertices[1].position, [4.0, 4.0]);
        assert_eq!(shape.vertices[2].position, [6.0, 4.0]);
    }

    #[test]
    fn resizing_an_open_arc_keeps_its_center() {
        let mut shape = polyline(&[[0.0, 0.0], [10.0, 0.0]], false);
        shape.vertices[0].bulge = 1.0;
        assert!(resize_polyline_arc(&mut shape, 0, 8.0));
        let arc = shape.segment_arc(0).unwrap();
        assert!((arc.radius - 8.0).abs() < 1.0e-9);
        assert!(Vec2::from(arc.center).distance(Vec2::new(5.0, 0.0)) < 1.0e-9);
    }

    #[test]
    fn an_impossible_parallel_edit_leaves_the_polyline_unchanged() {
        let mut shape = polyline(&[[0.0, 0.0], [2.0, 0.0], [5.0, 0.0], [6.0, 2.0]], false);
        let original = shape.clone();
        assert!(!move_polyline_segment_parallel(&mut shape, 1, 1.0));
        assert_eq!(shape, original);
    }
}
