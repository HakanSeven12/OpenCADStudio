//! Source-directed joins which retain the source curve type and direction.

use super::Vec3;
use super::NurbsCurve3;

/// Represent a straight segment as a clamped polynomial curve of a requested degree.
pub fn line_as_nurbs(points: [[f64; 3]; 2], degree: usize) -> Option<NurbsCurve3> {
    if degree == 0 || degree > 26 || points.iter().flatten().any(|v| !v.is_finite()) { return None; }
    let a = Vec3::from(points[0]); let b = Vec3::from(points[1]);
    let controls = (0..=degree).map(|i| a.lerp(b,i as f64 / degree as f64).to_array()).collect();
    NurbsCurve3::new_strict(degree,controls,[vec![0.0;degree+1],vec![1.0;degree+1]].concat(),vec![1.0;degree+1])
}

/// Join touching clamped NURBS without fitting or sampling.
/// The source direction is retained; the other curve can be reversed or prepended.
/// Rational weights are rescaled at the seam, which has C0 continuity.
/// A lower-degree input is elevated exactly to match the other curve.
/// Coincident endpoints preserve both curve shapes exactly. Within tolerance,
/// only the candidate endpoint is snapped to the source endpoint; the source
/// control points and weights remain unchanged when appending or prepending.
pub fn join_nurbs_curves(source: &NurbsCurve3, other: &NurbsCurve3, tolerance: f64) -> Option<NurbsCurve3> {
    if !tolerance.is_finite() || tolerance < 0.0 || source.is_closed() || other.is_closed() { return None; }
    let degree = source.degree().max(other.degree());
    let source = if source.degree() < degree { source.elevated(degree - source.degree())? } else { source.clone() };
    let other = if other.degree() < degree { other.elevated(degree - other.degree())? } else { other.clone() };
    for curve in [&source,&other] {
        let (a,b)=curve.domain(); let knots=curve.knots();
        if !a.is_finite() || !b.is_finite() || a>=b || !knots[..=degree].iter().all(|v|*v==a)
            || !knots[knots.len()-degree-1..].iter().all(|v|*v==b) { return None; }
    }
    fn reverse(curve: &NurbsCurve3) -> Option<NurbsCurve3> {
        let (a,b)=curve.domain();
        NurbsCurve3::new_strict(curve.degree(),curve.control_points().iter().copied().rev().collect(),
            curve.knots().iter().rev().map(|v|a+b-v).collect(),curve.weights().iter().copied().rev().collect())
    }
    let close=|a:[f64;3],b:[f64;3]|Vec3::from(a).distance(Vec3::from(b))<=tolerance;
    let (first,second,prepend)=if close(source.point_at(1.0),other.point_at(0.0)) {(source.clone(),other.clone(),false)}
        else if close(source.point_at(1.0),other.point_at(1.0)) {(source.clone(),reverse(&other)?,false)}
        else if close(source.point_at(0.0),other.point_at(1.0)) {(other.clone(),source.clone(),true)}
        else if close(source.point_at(0.0),other.point_at(0.0)) {(reverse(&other)?,source.clone(),true)}
        else {return None;};
    let (_,end)=first.domain();let(start,_)=second.domain();
    let mut controls=first.control_points().to_vec();
    if prepend { *controls.last_mut()? = *second.control_points().first()?; }
    controls.extend_from_slice(&second.control_points()[1..]);
    let mut weights = if prepend {
        let ratio=second.weights().first()? / first.weights().last()?;
        first.weights().iter().map(|weight| weight * ratio).collect::<Vec<_>>()
    } else { first.weights().to_vec() };
    if prepend {
        *weights.last_mut()? = *second.weights().first()?;
        weights.extend_from_slice(&second.weights()[1..]);
    } else {
        let ratio=first.weights().last()? / second.weights().first()?;
        weights.extend(second.weights()[1..].iter().map(|weight| weight * ratio));
    }
    let mut knots=first.knots()[..first.knots().len()-1].to_vec();
    knots.extend(second.knots()[degree+1..].iter().map(|v|v-start+end));
    NurbsCurve3::new_strict(degree,controls,knots,weights)
}

/// Span two collinear finite lines, including the gap between them.
/// The output follows the first line's direction. Non-collinear and
/// degenerate inputs are rejected without changing either input.
pub fn join_collinear_lines(source: [[f64; 3]; 2], other: [[f64; 3]; 2], tolerance: f64) -> Option<[[f64; 3]; 2]> {
    if !tolerance.is_finite() || tolerance < 0.0 || source.iter().chain(other.iter()).flatten().any(|v| !v.is_finite()) { return None; }
    let a = Vec3::from(source[0]);
    let delta = Vec3::from(source[1]) - a;
    let length = delta.length();
    if length <= tolerance { return None; }
    let direction = delta.normalize()?;
    let c = Vec3::from(other[0]);
    let d = Vec3::from(other[1]);
    if c.distance(d) <= tolerance || (c-a).cross(direction).length() > tolerance || (d-a).cross(direction).length() > tolerance { return None; }
    let first = (c-a).dot(direction);
    let last = (d-a).dot(direction);
    Some([(a + direction * first.min(last).min(0.0)).to_array(), (a + direction * first.max(last).max(length)).to_array()])
}

/// Extend a counterclockwise angular interval to include another interval.
/// Angles share a circle and angular frame, established by the caller. The
/// source start is retained; a full revolution is returned as start + TAU.
pub fn join_counterclockwise_spans(source: [f64; 2], other: [f64; 2]) -> Option<[f64; 2]> {
    use std::f64::consts::TAU;
    if source.iter().chain(other.iter()).any(|v| !v.is_finite()) { return None; }
    let source_span = (source[1] - source[0]).rem_euclid(TAU);
    let other_span = (other[1] - other[0]).rem_euclid(TAU);
    if source_span == 0.0 || other_span == 0.0 { return None; }
    let offset = (other[0] - source[0]).rem_euclid(TAU);
    let end = source_span.max(offset + other_span).min(TAU);
    Some([source[0], source[0] + end])
}

/// Join arcs represented in the same object-coordinate angular frame.
/// Centers and normals must agree within tolerance, and radii must be positive.
/// Opposite normals are rejected because their angular frames differ.
pub fn join_cocircular_arcs(
    source: ([f64; 3], [f64; 3], f64, [f64; 2]),
    other: ([f64; 3], [f64; 3], f64, [f64; 2]),
    tolerance: f64,
) -> Option<[f64; 2]> {
    if !tolerance.is_finite() || tolerance < 0.0 || !source.2.is_finite() || !other.2.is_finite()
        || source.2 <= 0.0 || other.2 <= 0.0 || (source.2-other.2).abs() > tolerance
        || source.0.iter().chain(source.1.iter()).chain(other.0.iter()).chain(other.1.iter()).any(|v| !v.is_finite()) { return None; }
    if Vec3::from(source.0).distance(Vec3::from(other.0)) > tolerance
        || Vec3::from(source.1).normalize()?.distance(Vec3::from(other.1).normalize()?) > 1e-12 { return None; }
    join_counterclockwise_spans(source.3, other.3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_join_keeps_the_source_direction() {
        assert_eq!(join_collinear_lines([[2.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            [[5.0, 0.0, 0.0], [4.0, 0.0, 0.0]], 1e-9),
            Some([[5.0, 0.0, 0.0], [0.0, 0.0, 0.0]]));
    }

    #[test]
    fn spline_join_elevates_the_lower_degree_without_moving_either_half() {
        let source = line_as_nurbs([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], 1).unwrap();
        let other = line_as_nurbs([[1.0, 0.0, 0.0], [2.0, 1.0, 0.0]], 3).unwrap();
        let joined = join_nurbs_curves(&source, &other, 1e-9).unwrap();
        assert_eq!(joined.degree(), 3);
        assert!(Vec3::from(joined.point_at(0.0)).distance(Vec3::new(0.0, 0.0, 0.0)) < 1e-12);
        assert!(Vec3::from(joined.point_at(0.25)).distance(Vec3::new(0.5, 0.0, 0.0)) < 1e-12);
        assert!(Vec3::from(joined.point_at(0.5)).distance(Vec3::new(1.0, 0.0, 0.0)) < 1e-12);
        assert!(Vec3::from(joined.point_at(0.75)).distance(Vec3::new(1.5, 0.5, 0.0)) < 1e-12);
        assert!(Vec3::from(joined.point_at(1.0)).distance(Vec3::new(2.0, 1.0, 0.0)) < 1e-12);
    }
}
