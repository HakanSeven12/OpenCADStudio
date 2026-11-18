//! Whether a point is inside a solid.
//!
//! The question a boolean asks of every piece it has cut: this fragment of a
//! face — does it lie inside the other solid, outside it, or on its surface?
//! Union keeps what is outside, intersection what is inside, difference one
//! of each. Everything else about a boolean is bookkeeping around that
//! answer.
//!
//! # By counting crossings
//!
//! A ray from the point crosses a closed surface an odd number of times if it
//! started inside and an even number if it started outside. That is the whole
//! method, and its accuracy is entirely a matter of counting honestly.
//!
//! # Where counting is not honest
//!
//! A ray that grazes an edge, passes through a vertex, or runs along a face
//! crosses at a place where "how many times" has no answer — one face's hit
//! and its neighbour's are the same point, and counting them as two flips the
//! result. Rather than perturb the geometry, the count is attempted along
//! several directions and the first one that lands cleanly is taken. A point
//! that defeats all of them is reported as [`Containment::Unknown`], which a
//! boolean can refuse; guessing would put a hole in a solid.
//!
//! The same shape as [`geom2d::contains`](ocs_geom::geom2d::contains), for the
//! same reasons.

use super::pcurve;
use super::topology::{Body, FaceKey};
use ocs_geom::geom2d::Tolerance;
use ocs_geom::space::Vec3;

/// Where a point stands relative to a solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Containment {
    /// Within it.
    Inside,
    /// Beyond it.
    Outside,
    /// On its surface, to within the tolerance asked.
    OnBoundary,
    /// Not decidable: every ray tried met the solid where a crossing cannot
    /// be counted, or met a surface this kernel cannot cast against.
    Unknown,
}

/// Directions to cast along.
///
/// Deliberately unrelated to each other and to the axes: a solid whose faces
/// are axis-aligned — which most are — defeats an axis-aligned ray at every
/// edge, and two rays that differ only slightly fail together. Each is a
/// small integer triple normalised, so none lands on a ratio a box is likely
/// to share.
const CASTS: [[f64; 3]; 4] = [
    [0.577_350_269_189_626, 0.577_350_269_189_626, 0.577_350_269_189_626],
    [-0.801_783_725_737_273, 0.267_261_241_912_424, 0.534_522_483_824_849],
    [0.358_057_437_019_716, -0.501_280_411_827_603, 0.787_726_361_443_376],
    [0.132_453_235_530_744, 0.794_719_413_184_463, -0.592_363_112_251_313],
];

/// Where `point` stands relative to `body`.
pub fn contains_point(body: &Body, point: [f64; 3], tolerance: f64) -> Containment {
    // On the surface takes priority: a point on a face is neither in nor out,
    // and a ray from it crosses at zero distance in a way no count survives.
    for face in body.face_keys() {
        if face_distance(body, face, point, tolerance).is_some_and(|gap| gap <= tolerance) {
            return Containment::OnBoundary;
        }
    }
    for direction in CASTS {
        if let Some(crossings) = count_crossings(body, point, direction, tolerance) {
            return if crossings % 2 == 1 {
                Containment::Inside
            } else {
                Containment::Outside
            };
        }
    }
    Containment::Unknown
}

/// How many of `body`'s faces a ray from `point` crosses ahead of it.
///
/// `None` when the count cannot be trusted: two hits at the same place, a hit
/// at zero distance, or a face this kernel cannot cast against.
fn count_crossings(
    body: &Body,
    point: [f64; 3],
    direction: [f64; 3],
    tolerance: f64,
) -> Option<usize> {
    let mut distances: Vec<f64> = Vec::new();
    for face in body.face_keys() {
        for distance in face_hits(body, face, point, direction, tolerance)? {
            // A hit at the origin means the point is on the face, which the
            // caller has already ruled out — so seeing one here means the
            // ray is running along a face and the count is meaningless.
            if distance <= tolerance {
                if distance >= -tolerance {
                    return None;
                }
                continue;
            }
            distances.push(distance);
        }
    }
    distances.sort_by(f64::total_cmp);
    // Two faces met at the same distance is the ray passing through an edge
    // or a vertex: one crossing counted twice, which flips the answer.
    if distances
        .windows(2)
        .any(|pair| (pair[1] - pair[0]).abs() <= tolerance)
    {
        return None;
    }
    Some(distances.len())
}

/// Where a ray meets one face, as distances along `direction`.
///
/// `None` when the face's surface cannot be cast against, or its boundary
/// cannot be expressed in the surface's parameter space — both of which make
/// the whole count unusable rather than merely this face's contribution.
fn face_hits(
    body: &Body,
    face: FaceKey,
    origin: [f64; 3],
    direction: [f64; 3],
    tolerance: f64,
) -> Option<Vec<f64>> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    if whole_closed_face(body, face) {
        return surface.ray_hits(origin, direction);
    }
    let boundary = pcurve::face_boundary(body, face, tolerance)?;
    let mut out = Vec::new();
    for distance in surface.ray_hits(origin, direction)? {
        let point = (Vec3::from(origin) + Vec3::from(direction) * distance).to_array();
        let Some((u, v)) = surface.parameters_at(point) else {
            // A pole or a degenerate frame: the hit is real but cannot be
            // placed within the face's boundary, so the count is unusable.
            return None;
        };
        // A planar edge hit can survive only one adjacent face's trim test.
        // Retry the ray before rounding makes that single hit a crossing.
        if distance >= -tolerance && matches!(surface, super::Surface::Plane(_))
            && boundary.iter().any(|edge| {
                let closest = ocs_geom::geom2d::closest_point(edge, [u, v]).point;
                Vec3::from(surface.point_at(closest[0], closest[1]))
                    .distance(Vec3::from(point)) <= tolerance
            })
        {
            return None;
        }
        if pcurve::contains_parameter_facing(
            surface,
            &boundary,
            [u, v],
            Tolerance::new(tolerance),
            node.forward,
        ) {
            out.push(distance);
        }
    }
    Some(out)
}

/// Whether a sphere or torus face is the whole surface: no loops, or only
/// slits — one edge walked there and back, enclosing nothing. A ball held in
/// its pocket carries one slit per contact line; tested against those, every
/// ray hit but the ones beside them fell outside, and a ray through the ball
/// counted its way in but not out (#1563).
pub(super) fn whole_closed_face(body: &Body, face: FaceKey) -> bool {
    let Some(node) = body.faces.get(face) else {
        return false;
    };
    if !matches!(
        body.surfaces.get(node.surface),
        Some(super::Surface::Sphere(_) | super::Surface::Torus(_))
    ) {
        return false;
    }
    node.loops.iter().all(|ring| {
        body.loops.get(*ring).is_some_and(|ring| {
            !ring.coedges.is_empty()
                && ring.coedges.iter().all(|key| {
                    let Some(coedge) = body.coedges.get(*key) else {
                        return false;
                    };
                    coedge.pcurve.is_none()
                        && ring.coedges.iter().any(|other| {
                            body.coedges.get(*other).is_some_and(|other| {
                                other.edge == coedge.edge && other.forward != coedge.forward
                            })
                        })
                })
        })
    })
}

/// How far `point` is from a face, or `None` if that cannot be measured.
pub(super) fn face_distance(
    body: &Body,
    face: FaceKey,
    point: [f64; 3],
    tolerance: f64,
) -> Option<f64> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    let gap = surface.distance_to(point).abs();
    if gap > tolerance {
        return Some(gap);
    }
    // Close to the surface, so whether it is on the *face* depends on the
    // boundary.
    let boundary = pcurve::face_boundary(body, face, tolerance)?;
    let (u, v) = surface.parameters_at(point)?;
    if pcurve::contains_parameter_facing(
        surface,
        &boundary,
        [u, v],
        Tolerance::new(tolerance),
        node.forward,
    ) {
        Some(gap)
    } else {
        Some(f64::INFINITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make::cuboid;

    const TOL: f64 = 1e-9;

    fn box_body() -> Body {
        cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap()
    }

    #[test]
    fn the_middle_of_a_box_is_inside_it() {
        assert_eq!(
            contains_point(&box_body(), [5.0, 5.0, 5.0], TOL),
            Containment::Inside
        );
    }

    #[test]
    fn a_point_beyond_a_box_is_outside_it() {
        let body = box_body();
        for point in [
            [20.0, 5.0, 5.0],
            [-1.0, 5.0, 5.0],
            [5.0, 5.0, 40.0],
            [-3.0, -3.0, -3.0],
        ] {
            assert_eq!(
                contains_point(&body, point, TOL),
                Containment::Outside,
                "{point:?}"
            );
        }
    }

    #[test]
    fn a_cylinder_uses_its_periodic_face_boundary() {
        let body = crate::make::cylinder([10.0, 5.0, -2.0], 3.0, 14.0).unwrap();
        assert_eq!(contains_point(&body, [10.0, 5.0, 5.0], TOL), Containment::Inside);
        assert_eq!(contains_point(&body, [5.0, 0.0, 5.0], TOL), Containment::Outside);
        assert_eq!(contains_point(&body, [13.0, 5.0, 5.0], TOL), Containment::OnBoundary);
    }

    #[test]
    fn a_point_on_a_face_is_on_the_boundary() {
        let body = box_body();
        for point in [[5.0, 5.0, 0.0], [0.0, 5.0, 5.0], [10.0, 5.0, 5.0]] {
            assert_eq!(
                contains_point(&body, point, 1e-6),
                Containment::OnBoundary,
                "{point:?}"
            );
        }
    }

    #[test]
    fn a_corner_and_an_edge_are_on_the_boundary_too() {
        // The places a single ray cast would trip over. They are settled
        // before any ray is cast, which is why they are answered at all.
        let body = box_body();
        assert_eq!(
            contains_point(&body, [0.0, 0.0, 0.0], 1e-6),
            Containment::OnBoundary
        );
        assert_eq!(
            contains_point(&body, [5.0, 0.0, 0.0], 1e-6),
            Containment::OnBoundary
        );
    }

    #[test]
    fn a_point_just_inside_a_face_is_inside() {
        let body = box_body();
        assert_eq!(
            contains_point(&body, [5.0, 5.0, 1e-3], 1e-9),
            Containment::Inside
        );
        assert_eq!(
            contains_point(&body, [5.0, 5.0, -1e-3], 1e-9),
            Containment::Outside
        );
    }

    #[test]
    fn a_point_aligned_with_every_corner_is_still_decided() {
        // An axis-aligned ray from here would leave along an edge of the box
        // and count a crossing twice. The alternative directions are what
        // make this answerable.
        let body = box_body();
        assert_eq!(
            contains_point(&body, [5.0, 5.0, 5.0], TOL),
            Containment::Inside
        );
        // On the diagonal of the box, where the first cast direction runs
        // straight at a corner.
        assert_eq!(
            contains_point(&body, [2.0, 2.0, 2.0], TOL),
            Containment::Inside
        );
    }

    #[test]
    fn a_box_away_from_the_origin_answers_the_same() {
        let body = cuboid([100.0, 200.0, 300.0], [2.0, 2.0, 2.0]).unwrap();
        assert_eq!(
            contains_point(&body, [101.0, 201.0, 301.0], TOL),
            Containment::Inside
        );
        assert_eq!(
            contains_point(&body, [0.0, 0.0, 0.0], TOL),
            Containment::Outside
        );
    }

    #[test]
    fn survey_coordinates_do_not_change_the_answer() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let body = cuboid(origin, [4.0, 4.0, 4.0]).unwrap();
        let middle = [origin[0] + 2.0, origin[1] + 2.0, origin[2] + 2.0];
        assert_eq!(contains_point(&body, middle, 1e-6), Containment::Inside);
        let beyond = [origin[0] + 20.0, origin[1] + 2.0, origin[2] + 2.0];
        assert_eq!(contains_point(&body, beyond, 1e-6), Containment::Outside);
    }

    #[test]
    fn a_body_with_a_surface_that_cannot_be_cast_says_so() {
        // A spline surface has no ray solver, so the count is refused rather
        // than taken with that face missing — which would report every point
        // inside it as outside.
        let mut body = box_body();
        let face = body.face_keys().next().unwrap();
        let surface = body.faces.get(face).unwrap().surface;
        *body.surfaces.get_mut(surface).unwrap() = crate::Surface::Nurbs(
            ocs_geom::space::NurbsSurface3::new_strict(
                1,
                1,
                vec![
                    vec![[0.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
                    vec![[10.0, 0.0, 0.0], [10.0, 10.0, 0.0]],
                ],
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                vec![vec![1.0; 2]; 2],
            )
            .unwrap(),
        );
        assert_eq!(
            contains_point(&body, [5.0, 5.0, 5.0], TOL),
            Containment::Unknown
        );
    }

    #[test]
    fn a_ray_hits_a_sphere_twice_and_a_tangent_once() {
        let sphere = crate::Surface::Sphere(crate::Sphere {
            frame: ocs_geom::space::Plane::XY,
            radius: 5.0,
        });
        let through = sphere.ray_hits([-20.0, 0.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(through.len(), 2);
        assert!((through[0] - 15.0).abs() < 1e-9 && (through[1] - 25.0).abs() < 1e-9);
        let grazing = sphere.ray_hits([-20.0, 5.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
        assert_eq!(grazing.len(), 1);
        let missing = sphere.ray_hits([-20.0, 9.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn a_ray_hits_a_cylinder_where_the_chord_says() {
        let cylinder = crate::Surface::Cylinder(crate::Cylinder {
            base: ocs_geom::space::Plane::XY,
            radius: 5.0,
        });
        // Offset three from the axis, so the chord is eight across.
        let hits = cylinder
            .ray_hits([-20.0, 3.0, 7.0], [1.0, 0.0, 0.0])
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!((hits[1] - hits[0] - 8.0).abs() < 1e-9, "{hits:?}");
    }

    #[test]
    fn a_ray_up_a_cone_meets_it_where_the_radius_matches() {
        let cone = crate::Surface::Cone(crate::Cone {
            base: ocs_geom::space::Plane::XY,
            radius: 10.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        });
        // Straight up at x = 4. The cone is 4 wide at height 6 — and 4 wide
        // again at height 14, on the mirrored nappe past the apex, because a
        // cone record covers both. A ray up the side meets each once.
        let hits = cone.ray_hits([4.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!((hits[0] - 6.0).abs() < 1e-9, "{hits:?}");
        assert!((hits[1] - 14.0).abs() < 1e-9, "{hits:?}");
    }

    #[test]
    fn the_parameters_of_a_point_invert_its_evaluation() {
        let surfaces = [
            crate::Surface::Plane(ocs_geom::space::Plane::XY),
            crate::Surface::Cylinder(crate::Cylinder {
                base: ocs_geom::space::Plane::XY,
                radius: 3.0,
            }),
            crate::Surface::Cone(crate::Cone {
                base: ocs_geom::space::Plane::XY,
                radius: 8.0,
                half_angle: 0.4,
            }),
            crate::Surface::Sphere(crate::Sphere {
                frame: ocs_geom::space::Plane::XY,
                radius: 6.0,
            }),
        ];
        for surface in surfaces {
            for (u, v) in [(0.3, 1.0), (2.0, -2.0), (-1.5, 0.7)] {
                let point = surface.point_at(u, v);
                let (back_u, back_v) = surface.parameters_at(point).unwrap();
                let again = surface.point_at(back_u, back_v);
                assert!(
                    Vec3::from(point).distance(Vec3::from(again)) < 1e-9,
                    "{surface:?} at ({u}, {v})"
                );
            }
        }
    }
}
