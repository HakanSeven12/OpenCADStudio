//! Measuring a ring of points in space.
//!
//! A closed loop in space encloses an area even though it does not lie in any
//! coordinate plane, and the loop a drawing hands over — a 3D face's boundary,
//! a picked sequence of points — routinely does not. Projecting it to XY first
//! would report the shadow's area rather than the loop's.
//!
//! # Newell's method
//!
//! The signed area *vector* of a polygon is `½ Σ pᵢ × pᵢ₊₁`. Its magnitude is
//! the area and its direction is the plane's normal, so both come out of one
//! pass — which is also why a loop that is not quite planar still gets a
//! sensible answer rather than an error: the sum is the best-fit plane's
//! projection of it.
//!
//! Summed about the ring's own first point. At survey coordinates the terms of
//! the origin form are around 10¹² and cancel to something small, spending
//! most of the significant digits before the answer appears.

use super::vec::Vec3;

/// The signed area vector of a closed ring: `½ Σ pᵢ × pᵢ₊₁`.
///
/// Its length is the area the ring encloses and its direction is the normal
/// of the plane it best fits, right-handed about the direction the ring runs.
///
/// The ring is treated as closed whether or not its last point repeats the
/// first, since a repeated point contributes nothing.
pub fn area_vector(ring: &[[f64; 3]]) -> [f64; 3] {
    if ring.len() < 3 {
        return [0.0, 0.0, 0.0];
    }
    let origin = Vec3::from(ring[0]);
    let mut total = Vec3::ZERO;
    for index in 0..ring.len() {
        let a = Vec3::from(ring[index]) - origin;
        let b = Vec3::from(ring[(index + 1) % ring.len()]) - origin;
        total = total + a.cross(b);
    }
    (total * 0.5).to_array()
}

/// The area a closed ring of points in space encloses.
///
/// Unsigned — a loop in space has no side to be on until a normal is chosen,
/// and [`area_vector`] is what carries that.
pub fn area(ring: &[[f64; 3]]) -> f64 {
    Vec3::from(area_vector(ring)).length()
}

/// The unit normal of the plane a ring best fits, or `None` for a ring that
/// encloses nothing — fewer than three points, or all of them in a line.
pub fn normal(ring: &[[f64; 3]]) -> Option<[f64; 3]> {
    Vec3::from(area_vector(ring)).normalize().map(Vec3::to_array)
}

/// The length once round a ring, including the closing edge.
pub fn perimeter(ring: &[[f64; 3]]) -> f64 {
    if ring.len() < 2 {
        return 0.0;
    }
    (0..ring.len())
        .map(|index| {
            Vec3::from(ring[index]).distance(Vec3::from(ring[(index + 1) % ring.len()]))
        })
        .sum()
}

/// The length along an open chain of points, without a closing edge.
pub fn chain_length(points: &[[f64; 3]]) -> f64 {
    points
        .windows(2)
        .map(|pair| Vec3::from(pair[0]).distance(Vec3::from(pair[1])))
        .sum()
}

/// Triangulate a simple planar ring into a triangle soup.
#[cfg(feature = "geom2d")]
pub fn triangulate(
    input: &[[f64; 3]],
    tolerance: crate::geom2d::Tolerance,
) -> Vec<[f64; 3]> {
    let mut ring = Vec::with_capacity(input.len());
    for &point in input {
        if !point.iter().all(|value| value.is_finite()) {
            return Vec::new();
        }
        if ring.last().is_none_or(|previous| {
            Vec3::from(*previous).distance(Vec3::from(point)) > tolerance.linear()
        }) {
            ring.push(point);
        }
    }
    if ring.len() > 1
        && Vec3::from(ring[0]).distance(Vec3::from(*ring.last().unwrap()))
            <= tolerance.linear()
    {
        ring.pop();
    }
    if ring.len() < 3 {
        return Vec::new();
    }
    let Some(normal) = normal(&ring) else {
        return Vec::new();
    };
    let Some(axis) = ring
        .iter()
        .skip(1)
        .map(|point| Vec3::from(*point) - Vec3::from(ring[0]))
        .find(|axis| axis.length() > tolerance.linear())
    else {
        return Vec::new();
    };
    let Some(plane) = super::Plane::orthonormal(ring[0], axis.to_array(), normal) else {
        return Vec::new();
    };
    if ring
        .iter()
        .any(|point| !plane.contains(*point, tolerance.linear()))
    {
        return Vec::new();
    }
    let Some(projected) = ring
        .iter()
        .map(|point| plane.project(*point))
        .collect::<Option<Vec<_>>>()
    else {
        return Vec::new();
    };
    let (points, triangles) = crate::geom2d::triangulate(&projected, &[]);
    triangles
        .into_iter()
        .flat_map(|triangle| triangle.map(|index| plane.point_at(points[index])))
        .collect()
}

/// Triangulate coplanar rings with even-odd nesting.
#[cfg(feature = "geom2d")]
pub fn triangulate_rings(
    input: &[Vec<[f64; 3]>],
    tolerance: crate::geom2d::Tolerance,
) -> Vec<[f64; 3]> {
    let mut rings = Vec::new();
    for input_ring in input {
        let mut ring = Vec::with_capacity(input_ring.len());
        for &point in input_ring {
            if !point.iter().all(|value| value.is_finite()) {
                return Vec::new();
            }
            if ring.last().is_none_or(|previous| {
                Vec3::from(*previous).distance(Vec3::from(point)) > tolerance.linear()
            }) {
                ring.push(point);
            }
        }
        if ring.len() > 1
            && Vec3::from(ring[0]).distance(Vec3::from(*ring.last().unwrap()))
                <= tolerance.linear()
        {
            ring.pop();
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    let Some(first) = rings.first() else {
        return Vec::new();
    };
    let Some(normal) = normal(first) else {
        return Vec::new();
    };
    let Some(axis) = first
        .iter()
        .skip(1)
        .map(|point| Vec3::from(*point) - Vec3::from(first[0]))
        .find(|axis| axis.length() > tolerance.linear())
    else {
        return Vec::new();
    };
    let Some(plane) = super::Plane::orthonormal(first[0], axis.to_array(), normal) else {
        return Vec::new();
    };
    let mut projected = Vec::with_capacity(rings.len());
    for ring in &rings {
        if ring
            .iter()
            .any(|point| !plane.contains(*point, tolerance.linear()))
        {
            return Vec::new();
        }
        let Some(ring) = ring
            .iter()
            .map(|point| plane.project(*point))
            .collect::<Option<Vec<_>>>()
        else {
            return Vec::new();
        };
        projected.push(ring);
    }
    let (points, triangles) = crate::geom2d::triangulate_rings(&projected);
    triangles
        .into_iter()
        .flat_map(|triangle| triangle.map(|index| plane.point_at(points[index])))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    fn unit_square_xy() -> Vec<[f64; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
        ]
    }

    #[test]
    fn a_flat_square_measures_its_side_squared() {
        assert!((area(&unit_square_xy()) - 100.0).abs() < 1e-9);
        assert!((perimeter(&unit_square_xy()) - 40.0).abs() < 1e-9);
    }

    #[test]
    fn a_repeated_closing_point_changes_nothing() {
        let mut ring = unit_square_xy();
        ring.push(ring[0]);
        assert!((area(&ring) - 100.0).abs() < 1e-9);
        assert!((perimeter(&ring) - 40.0).abs() < 1e-9);
    }

    #[test]
    fn a_tilted_square_keeps_its_own_area_rather_than_its_shadow() {
        // The reason this is not done by projecting to XY: this square is
        // still 100 across, but its shadow on the ground is 70.7.
        let ring = vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0 * FRAC_1_SQRT_2, 10.0 * FRAC_1_SQRT_2],
            [0.0, 10.0 * FRAC_1_SQRT_2, 10.0 * FRAC_1_SQRT_2],
        ];
        assert!((area(&ring) - 100.0).abs() < 1e-9, "{}", area(&ring));
        let normal = normal(&ring).unwrap();
        assert!((normal[1] + FRAC_1_SQRT_2).abs() < 1e-9, "{normal:?}");
        assert!((normal[2] - FRAC_1_SQRT_2).abs() < 1e-9, "{normal:?}");
    }

    #[test]
    fn the_normal_follows_the_direction_the_ring_runs() {
        let mut reversed = unit_square_xy();
        reversed.reverse();
        assert_eq!(normal(&unit_square_xy()), Some([0.0, 0.0, 1.0]));
        assert_eq!(normal(&reversed), Some([0.0, 0.0, -1.0]));
    }

    #[test]
    fn a_ring_with_nothing_to_enclose_reports_nothing() {
        assert_eq!(area(&[]), 0.0);
        assert_eq!(area(&[[0.0; 3], [1.0, 0.0, 0.0]]), 0.0);
        // Three collinear points span no plane.
        let collinear = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        assert_eq!(area(&collinear), 0.0);
        assert!(normal(&collinear).is_none());
    }

    #[test]
    fn a_chain_is_measured_without_closing_it() {
        let chain = [[0.0, 0.0, 0.0], [3.0, 0.0, 0.0], [3.0, 4.0, 0.0]];
        assert!((chain_length(&chain) - 7.0).abs() < 1e-12);
        // Once round, the closing edge adds the hypotenuse.
        assert!((perimeter(&chain) - 12.0).abs() < 1e-12);
    }

    #[test]
    fn survey_coordinates_measure_the_same_area() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let shifted: Vec<[f64; 3]> = unit_square_xy()
            .iter()
            .map(|p| [origin[0] + p[0], origin[1] + p[1], origin[2] + p[2]])
            .collect();
        assert!((area(&shifted) - 100.0).abs() < 1e-6, "{}", area(&shifted));
    }
}
