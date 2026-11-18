//! Curve arithmetic in space, for the shapes that do not lie in a plane.
//!
//! [`geom2d`](crate::geom2d) is the kernel's curve layer and it is
//! two-dimensional on purpose. Some of a drawing is not: a 3D polyline, a
//! spline through points in space, the blend between two of them, and — once
//! there is a B-rep — every edge, which lives in space and is only projected
//! into a face's parameter space when a face needs it.
//!
//! This is the small part of that those callers actually need. It is not a
//! third curve layer; it is the arithmetic that has no planar counterpart to
//! delegate to.

use super::vec::Vec3;

/// The point at `t` on a Bézier curve through `control`, by de Casteljau.
///
/// Repeated interpolation rather than the Bernstein sum. The sum needs
/// binomial coefficients, which invites a lookup table for the degrees a
/// caller happens to use today and silently wrong answers for the rest, and
/// it is less stable besides — each term is a large coefficient multiplied by
/// a small power, and they cancel. De Casteljau only ever averages two
/// neighbouring points, so nothing grows.
///
/// `t` outside `0..=1` extrapolates along the curve's own continuation, which
/// is what a caller reaching past an end wants.
pub fn bezier_point(control: &[[f64; 3]], t: f64) -> [f64; 3] {
    match control.len() {
        0 => return [0.0, 0.0, 0.0],
        1 => return control[0],
        _ => {}
    }
    let mut working: Vec<Vec3> = control.iter().copied().map(Vec3::from).collect();
    for round in 1..working.len() {
        for index in 0..working.len() - round {
            working[index] = working[index].lerp(working[index + 1], t);
        }
    }
    working[0].to_array()
}

/// The Bézier sampled at `segments + 1` evenly spaced parameters, both ends
/// included.
pub fn bezier_points(control: &[[f64; 3]], segments: usize) -> Vec<[f64; 3]> {
    let segments = segments.max(1);
    (0..=segments)
        .map(|step| bezier_point(control, step as f64 / segments as f64))
        .collect()
}

/// Whether two non-degenerate 3D segments share a collinear stretch.
pub fn segments_overlap_collinearly(
    first_start: [f64; 3],
    first_end: [f64; 3],
    second_start: [f64; 3],
    second_end: [f64; 3],
    linear_tolerance: f64,
) -> bool {
    if !linear_tolerance.is_finite() || linear_tolerance <= 0.0 {
        return false;
    }
    let first_start = Vec3::from(first_start);
    let first_end = Vec3::from(first_end);
    let second_start = Vec3::from(second_start);
    let second_end = Vec3::from(second_end);
    if ![first_start, first_end, second_start, second_end]
        .into_iter()
        .all(Vec3::is_finite)
    {
        return false;
    }

    let first = first_end - first_start;
    let second = second_end - second_start;
    let first_length = first.length();
    let second_length = second.length();
    if first_length <= linear_tolerance || second_length <= linear_tolerance {
        return false;
    }

    let coordinate_scale = [first_start, first_end, second_start, second_end]
        .into_iter()
        .fold(1.0_f64, |scale, point| {
            scale.max(point.x.abs()).max(point.y.abs()).max(point.z.abs())
        });
    let tolerance = linear_tolerance.max(coordinate_scale * f64::EPSILON * 32.0);
    if first.cross(second).length() > 1.0e-12 * first_length * second_length {
        return false;
    }
    if (second_start - first_start).cross(first).length() > tolerance * first_length {
        return false;
    }

    let direction = first / first_length;
    let second_a = (second_start - first_start).dot(direction);
    let second_b = (second_end - first_start).dot(direction);
    second_a.max(second_b).min(first_length) - second_a.min(second_b).max(0.0) > tolerance
}

/// The curvature vector at `point` of the circle through the three points.
///
/// Points from `point` towards that circle's centre with magnitude `1/r`,
/// which is the form a blend wants: it is what has to be matched at an end
/// for the join to be curvature-continuous, and both the direction and the
/// tightness are in the one vector.
///
/// Zero when the three are collinear or two of them coincide — there is no
/// circle through them, and a straight run has no curvature to report.
pub fn curvature_through(point: [f64; 3], next: [f64; 3], third: [f64; 3]) -> [f64; 3] {
    let (point, next, third) = (Vec3::from(point), Vec3::from(next), Vec3::from(third));
    let u = next - point;
    let v = third - point;
    let across = u.cross(v);
    let denominator = 2.0 * across.length_squared();
    if denominator <= f64::MIN_POSITIVE {
        return [0.0, 0.0, 0.0];
    }
    // The circumcentre relative to `point`, by the standard identity.
    let centre = (v.cross(across) * u.length_squared() + across.cross(u) * v.length_squared())
        / denominator;
    let radius_squared = centre.length_squared();
    if radius_squared <= f64::MIN_POSITIVE {
        return [0.0, 0.0, 0.0];
    }
    // centre/|centre|² has length 1/|centre| = 1/r and points at the centre.
    (centre / radius_squared).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bezier_starts_and_ends_on_its_outer_control_points() {
        let control = [[0.0, 0.0, 0.0], [1.0, 5.0, 0.0], [4.0, 5.0, 2.0], [5.0, 0.0, 2.0]];
        assert_eq!(bezier_point(&control, 0.0), control[0]);
        assert_eq!(bezier_point(&control, 1.0), control[3]);
    }

    #[test]
    fn a_degree_one_bezier_is_the_segment_between_its_points() {
        let control = [[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]];
        assert_eq!(bezier_point(&control, 0.5), [5.0, 10.0, 15.0]);
    }

    #[test]
    fn de_casteljau_agrees_with_the_bernstein_sum_at_every_degree() {
        // The property the lookup-table version could only promise for the
        // two degrees it had entries for.
        fn bernstein(control: &[[f64; 3]], t: f64) -> [f64; 3] {
            let degree = control.len() - 1;
            let mut binomial = 1.0f64;
            let mut point = Vec3::ZERO;
            for (index, value) in control.iter().enumerate() {
                let weight = binomial
                    * t.powi(index as i32)
                    * (1.0 - t).powi((degree - index) as i32);
                point = point + Vec3::from(*value) * weight;
                binomial = binomial * (degree - index) as f64 / (index + 1) as f64;
            }
            point.to_array()
        }

        for degree in 1..=7usize {
            let control: Vec<[f64; 3]> = (0..=degree)
                .map(|i| {
                    let x = i as f64;
                    [x, (x * 1.7).sin() * 4.0, (x * 0.6).cos() * 3.0]
                })
                .collect();
            for step in 0..=10 {
                let t = step as f64 / 10.0;
                let a = Vec3::from(bezier_point(&control, t));
                let b = Vec3::from(bernstein(&control, t));
                assert!(
                    a.distance(b) < 1e-9,
                    "degree {degree} at t={t}: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn a_bezier_leaves_its_first_end_along_the_first_control_edge() {
        // What the blend relies on: the tangent at the start runs towards the
        // second control point, which is how a tangent-continuous join is
        // arranged.
        let control = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 1.0, 0.0], [3.0, 1.0, 0.0]];
        let step = Vec3::from(bezier_point(&control, 1e-6)) - Vec3::from(control[0]);
        let along = step.normalize().unwrap();
        assert!((along.x - 1.0).abs() < 1e-5, "{along:?}");
    }

    #[test]
    fn sampling_includes_both_ends() {
        let control = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 0.0, 0.0]];
        let points = bezier_points(&control, 8);
        assert_eq!(points.len(), 9);
        assert_eq!(points[0], control[0]);
        assert_eq!(points[8], control[2]);
    }

    #[test]
    fn the_curvature_of_a_circle_is_one_over_its_radius() {
        // Three points on a circle of radius 5 in the XY plane.
        let radius = 5.0;
        let at = |angle: f64| [radius * angle.cos(), radius * angle.sin(), 0.0];
        let curvature = Vec3::from(curvature_through(at(0.0), at(0.4), at(0.8)));
        assert!((curvature.length() - 1.0 / radius).abs() < 1e-9);
        // And it points at the centre, which from (5, 0) is the −X direction.
        let towards = curvature.normalize().unwrap();
        assert!((towards.x + 1.0).abs() < 1e-9, "{towards:?}");
    }

    #[test]
    fn a_circle_out_of_the_ground_plane_curves_in_its_own() {
        let radius = 2.0;
        let at = |angle: f64| [radius * angle.cos(), 0.0, radius * angle.sin()];
        let curvature = Vec3::from(curvature_through(at(0.0), at(0.5), at(1.0)));
        assert!((curvature.length() - 0.5).abs() < 1e-9);
        assert!(curvature.y.abs() < 1e-9, "should stay in the XZ plane");
    }

    #[test]
    fn collinear_points_have_no_curvature() {
        assert_eq!(
            curvature_through([0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]),
            [0.0, 0.0, 0.0]
        );
        assert_eq!(
            curvature_through([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]),
            [0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn survey_coordinates_report_the_same_curvature() {
        let origin = Vec3::new(512_345.678, 4_512_345.678, 91.5);
        let radius = 5.0;
        let at = |angle: f64| {
            (origin + Vec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)).to_array()
        };
        let curvature = Vec3::from(curvature_through(at(0.0), at(0.4), at(0.8)));
        assert!(
            (curvature.length() - 1.0 / radius).abs() < 1e-6,
            "{}",
            curvature.length()
        );
    }

    #[test]
    fn spatial_angle_bisector_handles_regular_and_nearly_opposite_rays() {
        let diagonal = Vec3::from(
            angle_bisector([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
                .unwrap(),
        );
        let expected = std::f64::consts::FRAC_1_SQRT_2;
        assert!((diagonal.x - expected).abs() < 1e-12);
        assert!((diagonal.y - expected).abs() < 1e-12);

        let near_opposite = Vec3::from(
            angle_bisector(
                [1.0, 1e-9, 0.0],
                [-1.0, 1e-9, 0.0],
                [0.0, 0.0, 1.0],
            )
            .unwrap(),
        );
        assert!(near_opposite.x.abs() < 1e-12, "{near_opposite:?}");
        assert!((near_opposite.y - 1.0).abs() < 1e-12, "{near_opposite:?}");
    }

    #[test]
    fn antiparallel_bisector_uses_the_oriented_plane_normal() {
        assert_eq!(
            angle_bisector([1.0, 0.0, 0.0], [-2.0, 0.0, 0.0], [0.0, 0.0, 4.0]),
            Some([0.0, 1.0, 0.0])
        );
        assert_eq!(
            angle_bisector([-2.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 4.0]),
            Some([0.0, -1.0, 0.0])
        );
        assert_eq!(
            angle_bisector([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
            None
        );
    }

    #[test]
    fn spatial_angle_bisector_rejects_invalid_rays_without_overflowing() {
        assert_eq!(angle_bisector([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), None);
        assert_eq!(
            angle_bisector([f64::INFINITY, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            None
        );
        let huge = Vec3::from(
            angle_bisector(
                [f64::MAX, f64::MAX, 0.0],
                [f64::MAX, f64::MAX, 0.0],
                [0.0, 0.0, 1.0],
            )
            .unwrap(),
        );
        assert!(huge.is_finite());
        assert!((huge.length() - 1.0).abs() < 1e-12);
    }
}

/// Unit bisector of two spatial rays. Opposite rays use the oriented normal
/// to select the perpendicular direction (normal cross first ray).
/// A zero ray, nonfinite input or an indeterminate antiparallel plane fails.
/// Nearly opposite but distinct rays retain their actual bisector.
pub fn angle_bisector(first: [f64; 3], second: [f64; 3], normal: [f64; 3]) -> Option<[f64; 3]> {
    if first.iter().chain(second.iter()).chain(normal.iter()).any(|v| !v.is_finite()) { return None; }
    fn unit(value: Vec3) -> Option<Vec3> {
        let scale = value.x.abs().max(value.y.abs()).max(value.z.abs());
        if scale == 0.0 || !scale.is_finite() { return None; }
        (value / scale).normalize()
    }
    let a = unit(Vec3::from(first))?;
    let b = unit(Vec3::from(second))?;
    let direction = unit(a + b).or_else(|| unit(unit(Vec3::from(normal))?.cross(a)))?;
    Some(direction.to_array())
}
