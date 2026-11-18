//! Centre-line construction between two finite straight segments.
//!
//! Selection points are part of the geometric input: for intersecting source
//! lines they select which of the four angular sectors is bisected. Finite
//! source endpoints determine the natural (unextended) result span. A pick
//! exactly at the intersection takes the half towards the source's start.

use super::curve::Line;
use super::intersect::line_line;

const EPSILON: f64 = 1.0e-10;

/// Constructed centre line and its unextended endpoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CenterLineGeometry {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub base_start: [f64; 2],
    pub base_end: [f64; 2],
}

fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: [f64; 2], scale: f64) -> [f64; 2] {
    [a[0] * scale, a[1] * scale]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn unit(vector: [f64; 2]) -> Option<[f64; 2]> {
    let length = dot(vector, vector).sqrt();
    (length > EPSILON).then(|| mul(vector, length.recip()))
}

fn oriented_towards(direction: [f64; 2], origin: [f64; 2], pick: [f64; 2]) -> [f64; 2] {
    if dot(sub(pick, origin), direction) <= 0.0 {
        mul(direction, -1.0)
    } else {
        direction
    }
}

/// A source segment's endpoints ordered by their position along `axis`.
fn ordered(line: Line, origin: [f64; 2], axis: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    if dot(sub(line.start, origin), axis) <= dot(sub(line.end, origin), axis) {
        (line.start, line.end)
    } else {
        (line.end, line.start)
    }
}

/// Parameter along `axis` where the bisector through `apex` crosses the
/// segment joining `a` and `b`; `None` when the segment does not reach it.
fn bisector_crossing(apex: [f64; 2], axis: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Option<f64> {
    let join = sub(b, a);
    if dot(join, join) <= EPSILON * EPSILON {
        return Some(dot(sub(a, apex), axis));
    }
    let (along, across) = line_line(apex, axis, a, join)?;
    (-EPSILON..=1.0 + EPSILON).contains(&across).then_some(along)
}

/// Construct the bisector/midline selected by `first_pick` and `second_pick`.
///
/// The endpoints of both sources are paired by their position along the
/// result: the near pair and the far pair. A parallel pair gives the midline
/// between the midpoints of each pair, starting on the first source's start
/// side. An intersecting pair gives the bisector of the picked sector, ending
/// where it crosses the segment joining each endpoint pair — at the
/// intersection itself when that segment does not reach it — and starting on
/// the intersection side. The two extension values are applied independently
/// beyond that natural span.
pub fn centerline_between(
    first: Line,
    second: Line,
    first_pick: [f64; 2],
    second_pick: [f64; 2],
    start_extension: f64,
    end_extension: f64,
) -> Option<CenterLineGeometry> {
    let first_direction = unit(first.direction())?;
    let second_direction = unit(second.direction())?;

    let (base_start, base_end, axis) = if let Some((parameter, _)) = line_line(
        first.start,
        first_direction,
        second.start,
        second_direction,
    ) {
        let apex = add(first.start, mul(first_direction, parameter));
        let first_ray = oriented_towards(first_direction, apex, first_pick);
        let second_ray = oriented_towards(second_direction, apex, second_pick);
        let axis = unit(add(first_ray, second_ray)).or_else(|| unit(sub(first_ray, second_ray)))?;
        let (first_near, first_far) = ordered(first, apex, axis);
        let (second_near, second_far) = ordered(second, apex, axis);
        let near = bisector_crossing(apex, axis, first_near, second_near).unwrap_or(0.0);
        let far = bisector_crossing(apex, axis, first_far, second_far).unwrap_or(0.0);
        let (near, far) = (near.min(far), near.max(far));
        (add(apex, mul(axis, near)), add(apex, mul(axis, far)), axis)
    } else {
        let axis = first_direction;
        let (first_near, first_far) = ordered(first, first.start, axis);
        let (second_near, second_far) = ordered(second, first.start, axis);
        (
            mul(add(first_near, second_near), 0.5),
            mul(add(first_far, second_far), 0.5),
            axis,
        )
    };
    if dot(sub(base_end, base_start), axis) <= EPSILON {
        return None;
    }
    let start = add(base_start, mul(axis, -start_extension));
    let end = add(base_end, mul(axis, end_extension));
    if dot(sub(end, start), axis) <= EPSILON {
        return None;
    }
    Some(CenterLineGeometry {
        start,
        end,
        base_start,
        base_end,
    })
}
