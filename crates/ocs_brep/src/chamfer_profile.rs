//! Shared exact profile trimming for analytic chamfers.

use super::{ChamferError, EdgeKey, FaceKey};
use ocs_geom::geom2d::{intersect, Arc, Curve, Line, Tolerance, Vec2};
use std::collections::HashMap;

pub(super) fn chamfer_profile(
    segments: &[Curve],
    points: &[[f64; 2]],
    selected: &HashMap<usize, EdgeKey>,
    segment_faces: &[Option<FaceKey>],
    base_face: FaceKey,
    base_distance: f64,
    other_distance: f64,
    tolerance: f64,
    avoid_axis: bool,
) -> Result<Vec<Curve>, ChamferError> {
    let count = points.len();
    if count < 3 || segments.len() != count || segment_faces.len() != count {
        return Err(ChamferError::InvalidResult);
    }
    let mut trim_start = vec![0.0f64; count];
    let mut trim_end = vec![0.0f64; count];
    for (node, edge) in selected {
        let previous = (*node + count - 1) % count;
        let next = *node;
        let Some(previous_face) = segment_faces[previous] else {
            return Err(ChamferError::EdgeOutsideBaseFace(*edge));
        };
        let Some(next_face) = segment_faces[next] else {
            return Err(ChamferError::EdgeOutsideBaseFace(*edge));
        };
        let (previous_distance, next_distance) = if previous_face == base_face
            && next_face != base_face
        {
            (base_distance, other_distance)
        } else if next_face == base_face && previous_face != base_face {
            (other_distance, base_distance)
        } else {
            return Err(ChamferError::EdgeOutsideBaseFace(*edge));
        };
        trim_end[previous] = trim_end[previous].max(previous_distance);
        trim_start[next] = trim_start[next].max(next_distance);
    }

    for index in 0..count {
        let length = segments[index].length();
        if !length.is_finite()
            || length <= tolerance
            || trim_start[index] + trim_end[index] >= length - tolerance
        {
            return Err(ChamferError::DistanceTooLargeOrInteracting);
        }
    }

    let mut profile = Vec::with_capacity(count + selected.len());
    for index in 0..count {
        if let Some(edge) = selected.get(&index) {
            let previous = (index + count - 1) % count;
            let incoming = point_from_end(&segments[previous], trim_end[previous]);
            let outgoing = point_from_start(&segments[index], trim_start[index]);
            if Vec2::from(incoming).distance(Vec2::from(outgoing)) <= tolerance
                || (avoid_axis
                    && (incoming[0] <= tolerance
                        || outgoing[0] <= tolerance
                        || incoming[0] * outgoing[0] <= 0.0))
            {
                return Err(ChamferError::DistanceTooLargeOrInteracting);
            }
            let _ = edge;
            profile.push(Curve::Line(Line {
                start: incoming,
                end: outgoing,
            }));
        }
        profile.push(trim_segment(
            &segments[index],
            trim_start[index],
            trim_end[index],
        )?);
    }

    for first in 0..profile.len() {
        for second in first + 1..profile.len() {
            let adjacent = second == first + 1
                || (first == 0 && second + 1 == profile.len());
            for hit in intersect(
                &profile[first],
                &profile[second],
                Tolerance::new(tolerance),
            ) {
                let endpoint = |curve: &Curve| {
                    [0.0, 1.0].iter().any(|parameter| {
                        Vec2::from(curve.point_at(*parameter))
                            .distance(Vec2::from(hit.point))
                            <= tolerance
                    })
                };
                if !adjacent || !endpoint(&profile[first]) || !endpoint(&profile[second]) {
                    return Err(ChamferError::DistanceTooLargeOrInteracting);
                }
            }
        }
    }
    Ok(profile)
}

fn point_from_start(curve: &Curve, distance: f64) -> [f64; 2] {
    curve.point_at(curve.parameter_at_distance(distance))
}

fn point_from_end(curve: &Curve, distance: f64) -> [f64; 2] {
    curve.point_at(curve.parameter_at_distance(curve.length() - distance))
}

fn trim_segment(
    curve: &Curve,
    from_start: f64,
    from_end: f64,
) -> Result<Curve, ChamferError> {
    let length = curve.length();
    let start = curve.parameter_at_distance(from_start);
    let end = curve.parameter_at_distance(length - from_end);
    match curve {
        Curve::Line(_) => Ok(Curve::Line(Line {
            start: curve.point_at(start),
            end: curve.point_at(end),
        })),
        Curve::Arc(arc) => {
            let sweep = arc.sweep();
            Ok(Curve::Arc(Arc {
                centre: arc.centre,
                radius: arc.radius,
                start_angle: arc.start_angle + sweep * start,
                end_angle: arc.start_angle + sweep * end,
            }))
        }
        _ => Err(ChamferError::UnsupportedBodySurface),
    }
}
