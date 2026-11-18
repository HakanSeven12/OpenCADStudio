//! Conservative spatial bounds for rejecting separated geometry.
//! Closed analytic surfaces and positive-weight NURBS use their full extents.

use super::geometry::Surface;
use super::topology::{Body, FaceKey};
use ocs_geom::space::Vec3;

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Lowest corner.
    pub min: [f64; 3],
    /// Highest corner.
    pub max: [f64; 3],
}

impl Aabb {
    /// The box around a single point.
    pub fn at(point: [f64; 3]) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// The box around every point given, or `None` for none at all.
    pub fn around(points: impl IntoIterator<Item = [f64; 3]>) -> Option<Self> {
        let mut points = points.into_iter();
        let mut bounds = Self::at(points.next()?);
        for point in points {
            bounds.absorb(point);
        }
        Some(bounds)
    }

    /// Grows to include `point`.
    pub fn absorb(&mut self, point: [f64; 3]) {
        for ((low, high), value) in self.min.iter_mut().zip(self.max.iter_mut()).zip(point) {
            *low = low.min(value);
            *high = high.max(value);
        }
    }

    /// Grows to include another box.
    pub fn merge(&mut self, other: Self) {
        self.absorb(other.min);
        self.absorb(other.max);
    }

    /// Grows by `padding` on every side.
    ///
    /// A prefilter pads by its tolerance so two faces that touch exactly are
    /// still offered to the test that decides.
    pub fn grown(&self, padding: f64) -> Self {
        Self {
            min: [
                self.min[0] - padding,
                self.min[1] - padding,
                self.min[2] - padding,
            ],
            max: [
                self.max[0] + padding,
                self.max[1] + padding,
                self.max[2] + padding,
            ],
        }
    }

    /// Whether two boxes share any space.
    /// A box holding nothing, overlapping no other.
    pub const EMPTY: Self = Self { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] };

    pub fn overlaps(&self, other: &Self) -> bool {
        (0..3).all(|axis| self.min[axis] <= other.max[axis] && other.min[axis] <= self.max[axis])
    }

    /// Whether a point is within.
    pub fn holds(&self, point: [f64; 3]) -> bool {
        (0..3).all(|axis| point[axis] >= self.min[axis] && point[axis] <= self.max[axis])
    }

    /// The middle.
    pub fn centre(&self) -> [f64; 3] {
        // Averaged as `min + half the span` rather than `(min + max) / 2`:
        // the sum overflows for coordinates near the top of the range and
        // loses a bit everywhere else.
        [0, 1, 2].map(|axis| self.min[axis] + (self.max[axis] - self.min[axis]) * 0.5)
    }

    /// Width, depth and height.
    pub fn size(&self) -> [f64; 3] {
        [0, 1, 2].map(|axis| self.max[axis] - self.min[axis])
    }
}

/// How finely a curved edge is sampled when bounding it.
///
/// A chord between two samples cuts the corner, so a bound built from too few
/// would be too small — the one direction a prefilter must never be wrong in.
/// Sixteen keeps a full circle within half a per cent, and the padding a
/// caller applies covers the rest.
const SAMPLES: usize = 16;

/// A conservative face box, including the full supporting surface when needed.
///
/// `None` means "cannot exclude this face", not "this face is empty".
pub fn face_bounds(body: &Body, face: FaceKey) -> Option<Aabb> {
    let node = body.faces.get(face)?;
    let surface = body.surfaces.get(node.surface)?;
    match surface {
        Surface::Sphere(sphere) => {
            let normal = sphere.frame.normal()?;
            let extent = std::array::from_fn(|axis| sphere.radius.abs()
                * sphere.frame.x_axis[axis].hypot(sphere.frame.y_axis[axis]).hypot(normal[axis]));
            return centred_bounds(sphere.frame.origin, extent);
        }
        Surface::Torus(torus) => {
            let normal = torus.frame.normal()?;
            let extent = std::array::from_fn(|axis| {
                let radial = torus.frame.x_axis[axis].hypot(torus.frame.y_axis[axis]);
                torus.major_radius.abs() * radial + torus.minor_radius.abs() * radial.hypot(normal[axis])
            });
            return centred_bounds(torus.frame.origin, extent);
        }
        Surface::Nurbs(surface) | Surface::Offset(crate::OffsetSurface { approximation: surface, .. }) => {
            // Positive rational weights keep the surface inside its control hull.
            if surface.weights().iter().flatten().any(|w| !w.is_finite() || *w <= 0.0)
                || surface.control_points().iter().flatten().flatten().any(|v| !v.is_finite()) {
                return None;
            }
            return Aabb::around(surface.control_points().iter().flatten().copied());
        }
        _ => {}
    }
    let mut bounds: Option<Aabb> = None;
    for coedge in body.face_coedges(face) {
        let edge = body.edges.get(body.coedges.get(coedge)?.edge)?;
        let curve = body.curves.get(edge.curve)?;
        let span = edge.end_parameter - edge.start_parameter;
        let mut points: Vec<[f64; 3]> = (0..=SAMPLES)
            .map(|step| curve.point_at(edge.start_parameter + span * step as f64 / SAMPLES as f64))
            .collect();
        // A circle or ellipse reaches furthest along each axis between its
        // samples; those turns go in exactly, where they lie on the edge.
        let conic = match curve {
            super::Curve3::Circle(circle) => Some((circle.plane, circle.radius, circle.radius)),
            super::Curve3::Ellipse(ellipse) => {
                Some((ellipse.plane, ellipse.major_radius, ellipse.minor_radius))
            }
            _ => None,
        };
        if let Some((plane, across, along)) = conic {
            let (low, high) = (
                edge.start_parameter.min(edge.end_parameter),
                edge.start_parameter.max(edge.end_parameter),
            );
            for axis in 0..3 {
                let turn = (along * plane.y_axis[axis]).atan2(across * plane.x_axis[axis]);
                for base in [turn, turn + std::f64::consts::PI] {
                    let period = std::f64::consts::TAU;
                    let at = base + period * ((low - base) / period).ceil();
                    if at <= high {
                        points.push(curve.point_at(at));
                    }
                }
            }
        }
        // A spline bulges past the chords between its samples by an amount
        // no sample shows: a twentieth of the chord, as a circle would.
        let chord = points
            .windows(2)
            .map(|pair| {
                (0..3).map(|axis| (pair[1][axis] - pair[0][axis]).powi(2)).sum::<f64>().sqrt()
            })
            .fold(0.0, f64::max);
        let pad = if matches!(curve, super::Curve3::Nurbs(_) | super::Curve3::PlanarSpline { .. })
        {
            chord * 0.06
        } else {
            0.0
        };
        let Some(around) = Aabb::around(points) else {
            continue;
        };
        let around = around.grown(pad);
        match &mut bounds {
            Some(box_) => box_.merge(around),
            None => bounds = Some(around),
        }
    }
    // A cone face running up to its apex ends there in a loop with no edge,
    // so the edges alone leave the point out — and the box with it misses
    // every face the cone's tip reaches.
    if let Surface::Cone(cone) = surface {
        let singular = node
            .loops
            .iter()
            .any(|ring| body.loops.get(*ring).is_some_and(|ring| ring.coedges.is_empty()));
        let height = cone.radius / cone.half_angle.tan();
        if singular && height.is_finite() {
            let normal = cone.base.normal()?;
            let apex = std::array::from_fn(|axis| cone.base.origin[axis] + normal[axis] * height);
            match &mut bounds {
                Some(box_) => box_.absorb(apex),
                None => bounds = Some(Aabb::at(apex)),
            }
        }
    }
    bounds
}

/// The box around a whole body, or `None` if any of its faces cannot be
/// bounded.
pub fn body_bounds(body: &Body) -> Option<Aabb> {
    let mut bounds: Option<Aabb> = None;
    for face in body.face_keys() {
        let face = face_bounds(body, face)?;
        match &mut bounds {
            Some(box_) => box_.merge(face),
            None => bounds = Some(face),
        }
    }
    bounds
}

/// Scale-aware tolerance for operations spanning one or more bodies.
pub fn operation_tolerance(bodies: &[&Body]) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut coordinate_scale = 1.0_f64;
    for point in bodies
        .iter()
        .flat_map(|body| body.vertices.iter().map(|(_, vertex)| vertex.point))
    {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
            coordinate_scale = coordinate_scale.max(point[axis].abs());
        }
    }
    let extent = (0..3)
        .map(|axis| high[axis] - low[axis])
        .filter(|value| value.is_finite())
        .fold(1.0_f64, f64::max);
    extent * 1e-8 + f64::EPSILON * coordinate_scale * 64.0
}

fn centred_bounds(centre: [f64; 3], extent: [f64; 3]) -> Option<Aabb> {
    let bounds = Aabb {
        min: std::array::from_fn(|axis| centre[axis] - extent[axis]),
        max: std::array::from_fn(|axis| centre[axis] + extent[axis]),
    };
    bounds.min.iter().chain(&bounds.max).all(|v| v.is_finite()).then_some(bounds)
}

/// The box around a point set, for a caller that already has the points.
pub fn around_points(points: &[[f64; 3]]) -> Option<Aabb> {
    Aabb::around(points.iter().copied())
}

/// The distance between two boxes, zero when they touch or overlap.
///
/// Cheaper than any surface test and enough to order a boolean's work: the
/// pairs likeliest to matter are the ones already touching.
pub fn separation(one: &Aabb, other: &Aabb) -> f64 {
    let gap = |axis: usize| {
        (other.min[axis] - one.max[axis])
            .max(one.min[axis] - other.max[axis])
            .max(0.0)
    };
    Vec3::new(gap(0), gap(1), gap(2)).length()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make::cuboid;

    #[test]
    fn a_box_around_points_holds_every_one_of_them() {
        let points = [[1.0, -2.0, 3.0], [-4.0, 5.0, -6.0], [0.0, 0.0, 0.0]];
        let bounds = around_points(&points).unwrap();
        assert_eq!(bounds.min, [-4.0, -2.0, -6.0]);
        assert_eq!(bounds.max, [1.0, 5.0, 3.0]);
        for point in points {
            assert!(bounds.holds(point));
        }
        assert!(around_points(&[]).is_none());
    }

    #[test]
    fn boxes_that_touch_overlap_and_boxes_apart_do_not() {
        let one = Aabb {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let touching = Aabb {
            min: [1.0, 0.0, 0.0],
            max: [2.0, 1.0, 1.0],
        };
        let apart = Aabb {
            min: [2.0, 0.0, 0.0],
            max: [3.0, 1.0, 1.0],
        };
        assert!(one.overlaps(&touching));
        assert!(!one.overlaps(&apart));
        assert_eq!(separation(&one, &touching), 0.0);
        assert!((separation(&one, &apart) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn padding_lets_a_touching_pair_through() {
        // Two faces that meet exactly would be rejected by a box test that
        // compared them as they are, if either had rounded outwards.
        let one = Aabb {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let just_past = Aabb {
            min: [1.000_000_1, 0.0, 0.0],
            max: [2.0, 1.0, 1.0],
        };
        assert!(!one.overlaps(&just_past));
        assert!(one.grown(1e-6).overlaps(&just_past));
    }

    #[test]
    fn a_box_bounds_its_own_faces() {
        let body = cuboid([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]).unwrap();
        let bounds = body_bounds(&body).unwrap();
        assert_eq!(bounds.min, [1.0, 2.0, 3.0]);
        assert_eq!(bounds.max, [5.0, 7.0, 9.0]);
        assert_eq!(bounds.size(), [4.0, 5.0, 6.0]);
        assert_eq!(bounds.centre(), [3.0, 4.5, 6.0]);
    }

    #[test]
    fn each_face_of_a_box_is_flat_in_one_direction() {
        let body = cuboid([0.0; 3], [2.0, 4.0, 6.0]).unwrap();
        for face in body.face_keys() {
            let bounds = face_bounds(&body, face).expect("a planar face is bounded by its edges");
            let flat = bounds
                .size()
                .iter()
                .filter(|extent| **extent < 1e-12)
                .count();
            assert_eq!(flat, 1, "{bounds:?}");
        }
    }

    #[test]
    fn closed_surfaces_and_nurbs_are_bounded_by_their_full_extent() {
        use crate::{Sphere, Torus};
        use ocs_geom::space::{Plane, NurbsSurface3};
        let frame = Plane::orthonormal([7.0, -9.0, 12.0], [1.0, 1.0, 0.0], [1.0, -1.0, 2.0]).unwrap();
        let surfaces = [
            Surface::Sphere(Sphere { frame, radius: 3.0 }),
            Surface::Torus(Torus { frame, major_radius: 5.0, minor_radius: 2.0 }),
            Surface::Nurbs(NurbsSurface3::new(1, 1,
                vec![vec![[0.0; 3], [1.0, 2.0, 3.0]], vec![[4.0, 5.0, 1.0], [6.0, 7.0, 2.0]]],
                vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], Some(vec![vec![1.0, 2.0], vec![3.0, 1.0]])).unwrap()),
        ];
        for surface in surfaces {
            let mut body = cuboid([0.0; 3], [1.0; 3]).unwrap();
            let face = body.face_keys().next().unwrap();
            let key = body.faces.get(face).unwrap().surface;
            *body.surfaces.get_mut(key).unwrap() = surface.clone();
            let bounds = face_bounds(&body, face).unwrap().grown(1e-12);
            for i in 0..=32 {
                for j in 0..=32 {
                    let (u, v) = if surface.is_spline() {
                        (i as f64 / 32.0, j as f64 / 32.0)
                    } else {
                        (i as f64 * std::f64::consts::TAU / 32.0, j as f64 * std::f64::consts::TAU / 32.0)
                    };
                    assert!(bounds.holds(surface.point_at(u, v)));
                }
            }
            assert!(body_bounds(&body).is_some());
        }
        let sphere = crate::make::sphere([2.0; 3], 3.0).unwrap();
        let bounds = body_bounds(&sphere).unwrap();
        assert_eq!(bounds.min, [-1.0; 3]);
        assert_eq!(bounds.max, [5.0; 3]);
    }

    #[test]
    fn a_curved_edge_is_sampled_rather_than_cornered() {
        // A face bounded by an arc reaches further than the arc's ends do.
        // Bounding it by its vertices alone would cut the corner.
        let mut body = cuboid([0.0; 3], [2.0, 2.0, 2.0]).unwrap();
        let edge = body.edges.keys().next().unwrap();
        let curve = body.edges.get(edge).unwrap().curve;
        let (start, end) = body.edge_endpoints(edge).unwrap();
        // Replace it with an arc bulging away from the chord.
        let middle = ocs_geom::space::Vec3::from(start).lerp(ocs_geom::space::Vec3::from(end), 0.5);
        let plane = ocs_geom::space::Plane::orthonormal(
            (middle - ocs_geom::space::Vec3::new(0.0, 0.0, 1.0)).to_array(),
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        )
        .unwrap();
        *body.curves.get_mut(curve).unwrap() =
            crate::Curve3::Circle(crate::Circle3 { plane, radius: 1.5 });
        let node = body.edges.get_mut(edge).unwrap();
        node.start_parameter = 0.0;
        node.end_parameter = std::f64::consts::PI;
        let face = body
            .face_keys()
            .find(|f| {
                body.face_coedges(*f)
                    .iter()
                    .any(|c| body.coedges.get(*c).unwrap().edge == edge)
            })
            .unwrap();
        let bounds = face_bounds(&body, face).unwrap();
        // Compared against the box the face's corners alone would give: the
        // arc leaves it, so bounding by vertices would cut the corner.
        let corners: Vec<[f64; 3]> = body
            .face_coedges(face)
            .iter()
            .filter_map(|c| body.coedge_vertices(*c))
            .filter_map(|(from, _)| Some(body.vertices.get(from)?.point))
            .collect();
        let by_corners = around_points(&corners).unwrap();
        assert!(
            (0..3).any(|axis| {
                bounds.min[axis] < by_corners.min[axis] - 1e-9
                    || bounds.max[axis] > by_corners.max[axis] + 1e-9
            }),
            "the arc stayed inside its corners: {bounds:?} vs {by_corners:?}"
        );
    }

    #[test]
    fn survey_coordinates_bound_without_losing_the_size() {
        let origin = [512_345.678, 4_512_345.678, 91.5];
        let body = cuboid(origin, [0.001, 0.002, 0.003]).unwrap();
        let bounds = body_bounds(&body).unwrap();
        let size = bounds.size();
        assert!((size[0] - 0.001).abs() < 1e-9, "{size:?}");
        assert!((size[2] - 0.003).abs() < 1e-9, "{size:?}");
    }

    #[test]
    fn the_centre_of_a_distant_box_is_between_its_corners() {
        // Averaged as min plus half the span; summing the two corners would
        // lose a bit at this magnitude and can overflow near the top of the
        // range.
        let bounds = Aabb {
            min: [1e308, 0.0, 0.0],
            max: [1.5e308, 0.0, 0.0],
        };
        assert!(bounds.centre()[0].is_finite());
        assert!(bounds.holds(bounds.centre()));
    }
}
