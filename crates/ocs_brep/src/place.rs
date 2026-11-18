//! Moving a solid, and drawing the curves along its edges.
//!
//! The two things every consumer of a body needs that are not about building
//! one. A modeller places each primitive on the plane the user is working in
//! and then turns, mirrors and aligns it; a renderer wants the edges as
//! polylines to draw a wireframe and to click on.
//!
//! # A similarity, and nothing looser
//!
//! Moving a body moves its *surfaces*, not a cloud of points, and an analytic
//! surface only survives a map that preserves shape. Squash a cylinder along
//! one axis and it becomes an elliptic cylinder, which is not a
//! [`Cylinder`](super::geometry::Cylinder) and not any other variant either —
//! so the honest answer is to refuse rather than to store a circle where an
//! ellipse belongs and let every later reader believe it.
//!
//! What is allowed is a rotation, a translation, a uniform scale and a
//! reflection.
//!
//! Reflections preserve curve parameters. Analytic surface pcurves are mapped
//! with their frames; reflected spline surfaces also reverse face sense.

use super::geometry::{Curve3, Surface};
use super::topology::{Body, EdgeKey};
use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};

/// Where a body is put: three axes and an origin.
///
/// The columns of a transform, in the order a caller reading a matrix would
/// write them. `x_axis` is where the body's own x direction ends up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub z_axis: [f64; 3],
    pub origin: [f64; 3],
}

impl Placement {
    /// A placement that moves nothing.
    pub const IDENTITY: Self = Self {
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
        z_axis: [0.0, 0.0, 1.0],
        origin: [0.0, 0.0, 0.0],
    };

    /// Just a move.
    pub fn at(origin: [f64; 3]) -> Self {
        Self {
            origin,
            ..Self::IDENTITY
        }
    }

    /// How much it scales by, when it scales everything alike.
    ///
    /// `None` when the axes are not mutually perpendicular or not all the
    /// same length — the cases that would turn a circle into an ellipse.
    pub fn scale(&self) -> Option<f64> {
        let (x, y, z) = (
            Vec3::from(self.x_axis),
            Vec3::from(self.y_axis),
            Vec3::from(self.z_axis),
        );
        let scale = x.length();
        if scale <= 0.0 || !scale.is_finite() {
            return None;
        }
        let square = scale * scale;
        let alike = (y.length() - scale).abs() <= 1e-9 * scale
            && (z.length() - scale).abs() <= 1e-9 * scale;
        let square_on = x.dot(y).abs() <= 1e-9 * square
            && y.dot(z).abs() <= 1e-9 * square
            && z.dot(x).abs() <= 1e-9 * square;
        (alike && square_on).then_some(scale)
    }

    /// Whether it turns the body over. A reflection has all three axes still
    /// perpendicular but left-handed, which no rotation can produce.
    pub fn reflects(&self) -> bool {
        Vec3::from(self.x_axis)
            .cross(Vec3::from(self.y_axis))
            .dot(Vec3::from(self.z_axis))
            < 0.0
    }

    /// Where a point ends up.
    pub fn point(&self, point: [f64; 3]) -> [f64; 3] {
        let moved = self.vector(point);
        [
            moved[0] + self.origin[0],
            moved[1] + self.origin[1],
            moved[2] + self.origin[2],
        ]
    }

    /// Where a direction ends up — the origin is not added, so a tangent
    /// stays a tangent.
    pub fn vector(&self, vector: [f64; 3]) -> [f64; 3] {
        let (x, y, z) = (
            Vec3::from(self.x_axis),
            Vec3::from(self.y_axis),
            Vec3::from(self.z_axis),
        );
        (x * vector[0] + y * vector[1] + z * vector[2]).to_array()
    }

    /// A frame, moved and brought back to unit length.
    ///
    /// Every frame stays orthonormal, which is what the rest of the kernel
    /// reads them as: a projection onto a scaled frame returns scaled
    /// coordinates, and a radius stored beside it does not scale with them —
    /// so a plane holding a circle would draw it at the wrong size. The
    /// scale belongs in the radii, and in a spline's own coordinates.
    ///
    /// The normal is carried across rather than recomputed from the moved
    /// axes. Under a reflection those differ: the mapped axes cross to the
    /// *negation* of the mapped normal, and taking that would turn every
    /// round surface inside out while leaving the flat ones alone.
    fn frame(&self, plane: &Plane) -> Option<Plane> {
        let normal = Vec3::from(plane.normal()?);
        Plane::orthonormal(
            self.point(plane.origin),
            self.vector(plane.x_axis),
            self.vector(normal.to_array()),
        )
    }

    fn curve_frame(&self, plane: &Plane) -> Option<Plane> {
        let x = Vec3::from(self.vector(plane.x_axis)).normalize()?;
        let y = Vec3::from(self.vector(plane.y_axis)).normalize()?;
        Some(Plane::from_axes(
            self.point(plane.origin),
            x.to_array(),
            y.to_array(),
        ))
    }
}

/// Moves a whole body.
///
/// `None` for a map that is not a similarity — see the module note — and for
/// one that introduces a topology flaw.
pub fn transform(body: &Body, place: &Placement) -> Option<Body> {
    let scale = place.scale()?;
    let original_flaws = body.validate();
    let mut out = body.clone();

    for vertex in out.vertices.values_mut() {
        vertex.point = place.point(vertex.point);
    }
    for curve in out.curves.values_mut() {
        *curve = move_curve(curve, place, scale)?;
    }
    for surface in out.surfaces.values_mut() {
        *surface = move_surface(surface, place, scale)?;
    }

    let rings: Vec<_> = out.loops.keys().collect();
    for ring in rings {
        let coedges = out.loops.get(ring)?.coedges.clone();
        for coedge in &coedges {
            let surface = {
                let node = out.coedges.get(*coedge)?;
                let face = out.faces.get(out.loops.get(node.owner)?.owner)?;
                out.surfaces.get(face.surface)?.clone()
            };
            let node = out.coedges.get_mut(*coedge)?;
            node.pcurve = node
                .pcurve
                .take()
                .and_then(|curve| moved_pcurve(curve, &surface, scale, place.reflects()));
        }
    }
    if place.reflects() {
        for face in out.faces.values_mut() {
            if out.surfaces.get(face.surface).is_some_and(Surface::is_spline) {
                face.forward = !face.forward;
            }
        }
    }
    (out.validate() == original_flaws).then_some(out)
}

fn moved_pcurve(
    curve: ocs_geom::geom2d::Curve,
    surface: &Surface,
    scale: f64,
    reflects: bool,
) -> Option<ocs_geom::geom2d::Curve> {
    let handed = if reflects { -1.0 } else { 1.0 };
    let factors = match surface {
        Surface::Plane(_) => [scale, handed * scale],
        Surface::Cylinder(_) | Surface::Cone(_) => [handed, scale],
        Surface::Sphere(_) | Surface::Torus(_) => [handed, 1.0],
        Surface::Nurbs(_) | Surface::Offset(_) => [1.0, 1.0],
    };
    curve.transformed(&ocs_geom::geom2d::Transform::scale(factors[0], factors[1]))
}

pub(super) fn move_curve(curve: &Curve3, place: &Placement, scale: f64) -> Option<Curve3> {
    Some(match curve {
        Curve3::Line(line) => Curve3::Line(super::geometry::Line3 {
            origin: place.point(line.origin),
            direction: place.vector(line.direction),
        }),
        Curve3::Circle(circle) => Curve3::Circle(super::geometry::Circle3 {
            plane: place.curve_frame(&circle.plane)?,
            radius: circle.radius * scale,
        }),
        Curve3::Ellipse(ellipse) => Curve3::Ellipse(super::geometry::Ellipse3 {
            plane: place.curve_frame(&ellipse.plane)?,
            major_radius: ellipse.major_radius * scale,
            minor_radius: ellipse.minor_radius * scale,
        }),
        // The frame stays unit, so the scale has to go into the curve's own
        // coordinates instead — otherwise a scaled body keeps splines at
        // their old size while everything around them grows.
        Curve3::PlanarSpline { plane, curve } => {
            let grown = ocs_geom::geom2d::Curve::Nurbs(curve.clone())
                .transformed(&ocs_geom::geom2d::Transform::scale(scale, scale))?;
            let ocs_geom::geom2d::Curve::Nurbs(grown) = grown else {
                return None;
            };
            Curve3::PlanarSpline {
                plane: place.curve_frame(plane)?,
                curve: grown,
            }
        }
        Curve3::Nurbs(curve) => Curve3::Nurbs(move_nurbs_curve(curve, place)?),
    })
}

pub(super) fn move_surface(surface: &Surface, place: &Placement, scale: f64) -> Option<Surface> {
    Some(match surface {
        Surface::Plane(plane) => Surface::Plane(place.frame(plane)?),
        Surface::Cylinder(cylinder) => Surface::Cylinder(super::geometry::Cylinder {
            base: place.frame(&cylinder.base)?,
            radius: cylinder.radius * scale,
        }),
        Surface::Cone(cone) => Surface::Cone(super::geometry::Cone {
            base: place.frame(&cone.base)?,
            radius: cone.radius * scale,
            // An angle is what a similarity leaves alone.
            half_angle: cone.half_angle,
        }),
        Surface::Sphere(sphere) => Surface::Sphere(super::geometry::Sphere {
            frame: place.frame(&sphere.frame)?,
            radius: sphere.radius * scale,
        }),
        Surface::Torus(torus) => Surface::Torus(super::geometry::Torus {
            frame: place.frame(&torus.frame)?,
            major_radius: torus.major_radius * scale,
            minor_radius: torus.minor_radius * scale,
        }),
        Surface::Nurbs(surface) => Surface::Nurbs(move_nurbs_surface(surface, place)?),
        // A reflection turns the moved base's normal against the moved
        // offset, so the distance changes sign with it.
        Surface::Offset(offset) => Surface::Offset(super::geometry::OffsetSurface {
            base: move_nurbs_surface(&offset.base, place)?,
            distance: offset.distance * scale * if place.reflects() { -1.0 } else { 1.0 },
            approximation: move_nurbs_surface(&offset.approximation, place)?,
        }),
    })
}

fn move_nurbs_curve(curve: &NurbsCurve3, place: &Placement) -> Option<NurbsCurve3> {
    Some(NurbsCurve3::new(
        curve.degree(),
        curve
            .control_points()
            .iter()
            .map(|point| place.point(*point))
            .collect(),
        curve.knots().to_vec(),
        Some(curve.weights().to_vec()),
    )?
    .with_periodicity(curve.periodicity()))
}

fn move_nurbs_surface(surface: &NurbsSurface3, place: &Placement) -> Option<NurbsSurface3> {
    let (u_degree, v_degree) = surface.degrees();
    let (u_knots, v_knots) = surface.knots();
    Some(NurbsSurface3::new(
        u_degree,
        v_degree,
        surface
            .control_points()
            .iter()
            .map(|row| row.iter().map(|point| place.point(*point)).collect())
            .collect(),
        u_knots.to_vec(),
        v_knots.to_vec(),
        Some(surface.weights().to_vec()),
    )?
    .with_periodicity(surface.periodicity()[0], surface.periodicity()[1])
    .with_v_reversed(surface.v_reversed()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSample {
    pub parameter: f64,
    pub position: [f64; 3],
}

/// The points along one edge, close enough to its curve to draw.
///
/// `sag` is how far the polyline may depart from the curve, the same measure
/// the mesher uses. A straight edge comes back as its two ends; nothing is
/// gained by subdividing a line, and a renderer holding thousands of them
/// notices.
///
/// `None` when the edge or its curve is missing, which is a body that failed
/// to validate rather than a shape with nothing to draw.
pub fn edge_points(body: &Body, edge: EdgeKey, sag: f64) -> Option<Vec<[f64; 3]>> {
    Some(
        edge_samples(body, edge, sag)?
            .into_iter()
            .map(|sample| sample.position)
            .collect(),
    )
}

pub fn edge_samples(body: &Body, edge: EdgeKey, sag: f64) -> Option<Vec<EdgeSample>> {
    let node = body.edges.get(edge)?;
    let curve = body.curves.get(node.curve)?;
    let (from, to) = (node.start_parameter, node.end_parameter);
    let steps = match curve {
        Curve3::Line(_) => 1,
        Curve3::Circle(circle) => turns(circle.radius, to - from, sag),
        Curve3::Ellipse(ellipse) => turns(ellipse.major_radius, to - from, sag),
        Curve3::PlanarSpline { .. } | Curve3::Nurbs(_) => {
            let mut points = vec![EdgeSample {
                parameter: from,
                position: curve.point_at(from),
            }];
            sample_curve(curve, from, to, sag, 0, &mut points);
            points.push(EdgeSample {
                parameter: to,
                position: curve.point_at(to),
            });
            return Some(points);
        }
    };
    Some(
        (0..=steps)
            .map(|step| {
                let parameter = from + (to - from) * step as f64 / steps as f64;
                EdgeSample {
                    parameter,
                    position: curve.point_at(parameter),
                }
            })
            .collect(),
    )
}

fn sample_curve(
    curve: &Curve3,
    from: f64,
    to: f64,
    sag: f64,
    depth: u32,
    points: &mut Vec<EdgeSample>,
) {
    let middle = 0.5 * (from + to);
    let start = Vec3::from(curve.point_at(from));
    let end = Vec3::from(curve.point_at(to));
    let curved = Vec3::from(curve.point_at(middle));
    if depth < 16 && start.lerp(end, 0.5).distance(curved) > sag.max(1e-12) {
        sample_curve(curve, from, middle, sag, depth + 1, points);
        points.push(EdgeSample {
            parameter: middle,
            position: curved.to_array(),
        });
        sample_curve(curve, middle, to, sag, depth + 1, points);
    }
}

/// How many chords a circular sweep needs to stay within `sag` of the curve.
///
/// From the chord-sag relation: a chord subtending `θ` on a circle of radius
/// `r` departs from it by `r(1 − cos(θ/2))` in the middle.
fn turns(radius: f64, sweep: f64, sag: f64) -> usize {
    if radius <= 0.0 || sag <= 0.0 || sag >= radius {
        return 32;
    }
    let step = 2.0 * (1.0 - sag / radius).clamp(-1.0, 1.0).acos();
    if step <= 0.0 {
        return 32;
    }
    ((sweep.abs() / step).ceil() as usize).clamp(2, 256)
}

/// The points along every edge of a body, for a wireframe.
pub fn edge_polylines(body: &Body, sag: f64) -> Vec<Vec<[f64; 3]>> {
    body.edge_keys()
        .filter_map(|edge| edge_points(body, edge, sag))
        .filter(|points| points.len() >= 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make::{cuboid, cylinder, sphere};
    use crate::mesh;

    /// How much a body encloses, measured about its own middle.
    ///
    /// The divergence theorem sums tetrahedra reaching back to the origin,
    /// and at survey coordinates those are enormous and nearly cancel — a
    /// tenth of a cubic millimetre read off a sum of billions. Shifting to
    /// the body's own middle first makes the arithmetic about the body's own
    /// size, which is what is being measured.
    fn volume(body: &Body) -> f64 {
        let mesh = mesh::body(body, ocs_geom::tessellation::DEFAULT_ANGLE, 1e-9);
        if mesh.is_empty() {
            return 0.0;
        }
        // Read off the mesh rather than `body_bounds`, which refuses a face
        // wrapping a closed surface — a sphere is one face and has no box.
        let mut middle = Vec3::new(0.0, 0.0, 0.0);
        for point in &mesh.positions {
            middle = middle + Vec3::from(*point);
        }
        let middle = middle / mesh.positions.len() as f64;
        mesh.triangles
            .iter()
            .map(|triangle| {
                let at = |index: usize| Vec3::from(mesh.positions[triangle[index]]) - middle;
                at(0).cross(at(1)).dot(at(2)) / 6.0
            })
            .sum()
    }

    fn quarter_turn() -> Placement {
        Placement {
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [-1.0, 0.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
            origin: [0.0, 0.0, 0.0],
        }
    }

    #[test]
    fn a_turn_moves_a_body_without_changing_its_size() {
        let body = cuboid([0.0; 3], [10.0, 4.0, 6.0]).unwrap();
        let moved = transform(&body, &quarter_turn()).expect("a turned box");
        assert!(moved.validate().is_empty());
        assert!((volume(&moved) - 240.0).abs() < 1e-9, "{}", volume(&moved));
        let bounds = crate::body_bounds(&moved).unwrap();
        // Ten along x becomes ten along y, and four along y becomes four
        // back along x.
        assert!((bounds.max[1] - 10.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.min[0] + 4.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn a_scale_takes_the_radii_with_it() {
        // The case a point-by-point map gets wrong: a cylinder's radius is a
        // number beside its frame, so scaling only the frame leaves a solid
        // whose surfaces no longer pass through its own vertices.
        let body = cylinder([0.0; 3], 3.0, 6.0).unwrap();
        let doubled = transform(
            &body,
            &Placement {
                x_axis: [2.0, 0.0, 0.0],
                y_axis: [0.0, 2.0, 0.0],
                z_axis: [0.0, 0.0, 2.0],
                origin: [0.0; 3],
            },
        )
        .expect("a bigger cylinder");
        assert!(doubled.validate().is_empty());
        assert!(doubled.worst_vertex_gap() < 1e-9, "vertices left their edges");
        let expected = std::f64::consts::PI * 36.0 * 12.0;
        assert!(volume(&doubled) > 0.98 * expected, "{}", volume(&doubled));
        assert!(volume(&doubled) <= expected, "{}", volume(&doubled));
    }

    #[test]
    fn a_mirror_comes_back_the_right_way_out() {
        // A reflection turns every outward normal inward. Left alone the
        // result validates and meshes and measures a negative volume, which
        // is a solid lit black.
        let body = sphere([2.0, 0.0, 0.0], 3.0).unwrap();
        let flipped = Placement {
            x_axis: [-1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            z_axis: [0.0, 0.0, 1.0],
            origin: [0.0; 3],
        };
        assert!(flipped.reflects());
        let mirrored = transform(&body, &flipped).expect("a mirrored sphere");
        assert!(mirrored.validate().is_empty());
        let measured = volume(&mirrored);
        assert!(measured > 0.0, "inside out: {measured}");
        let expected = 4.0 / 3.0 * std::f64::consts::PI * 27.0;
        assert!(measured > 0.98 * expected, "{measured} vs {expected}");
        // And it really did move: centred two along x it reached five, and
        // mirrored it reaches one the other way. Read off the mesh, since a
        // sphere is one face wrapping its whole surface and has no box.
        let mesh = mesh::body(&mirrored, ocs_geom::tessellation::DEFAULT_ANGLE, 1e-9);
        let furthest = mesh
            .positions
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((furthest - 1.0).abs() < 1e-6, "{furthest}");
    }

    #[test]
    fn a_squash_is_refused_rather_than_rounded_off() {
        // It would turn every circle into an ellipse, and there is no
        // elliptic cylinder to put the answer in.
        let body = cylinder([0.0; 3], 3.0, 6.0).unwrap();
        assert!(transform(
            &body,
            &Placement {
                x_axis: [2.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
                z_axis: [0.0, 0.0, 1.0],
                origin: [0.0; 3],
            }
        )
        .is_none());
        // And so is a shear, which keeps the lengths but loses the angles.
        assert!(transform(
            &body,
            &Placement {
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [1.0, 0.0, 0.0],
                z_axis: [0.0, 0.0, 1.0],
                origin: [0.0; 3],
            }
        )
        .is_none());
    }

    #[test]
    fn a_body_moved_to_survey_coordinates_is_the_same_solid() {
        let body = cuboid([0.0; 3], [0.5; 3]).unwrap();
        let moved = transform(&body, &Placement::at([512_345.678, 4_512_345.678, 91.5]))
            .expect("a box a long way out");
        assert!(moved.validate().is_empty());
        assert!(moved.worst_vertex_gap() < 1e-6);
        assert!((volume(&moved) - 0.125).abs() < 1e-6, "{}", volume(&moved));
    }

    #[test]
    fn a_straight_edge_needs_only_its_ends_and_a_round_one_does_not() {
        let body = cuboid([0.0; 3], [10.0; 3]).unwrap();
        for edge in body.edge_keys() {
            assert_eq!(edge_points(&body, edge, 0.05).unwrap().len(), 2);
        }
        let round = cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let longest = round
            .edge_keys()
            .filter_map(|edge| edge_points(&round, edge, 0.05))
            .map(|points| points.len())
            .max()
            .unwrap();
        assert!(longest > 20, "a rim of radius five needs more than {longest}");
        // And the points really are on the rim rather than near it.
        for polyline in edge_polylines(&round, 0.05) {
            for point in polyline {
                let radius = point[0].hypot(point[1]);
                assert!(
                    radius < 1e-9 || (radius - 5.0).abs() < 1e-9,
                    "{point:?} is off the cylinder"
                );
            }
        }
    }

    #[test]
    fn a_finer_tolerance_asks_for_more_points() {
        let round = cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let count = |sag: f64| {
            round
                .edge_keys()
                .filter_map(|edge| edge_points(&round, edge, sag))
                .map(|points| points.len())
                .max()
                .unwrap()
        };
        assert!(count(0.001) > count(0.1));
    }
}
