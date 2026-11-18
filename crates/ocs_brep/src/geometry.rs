//! The shapes a face sits on and an edge runs along.
//!
//! Topology says which face borders which; geometry says where they are. The
//! two are kept apart because they change independently — a boolean rewrites
//! adjacency while leaving every surface exactly as it was, and a face can be
//! moved without any coedge noticing.
//!
//! # Analytic first, spline last
//!
//! A cylinder is stored as a cylinder, not as a spline that happens to look
//! like one. That is not an optimisation: an ACIS file says `cone`, and
//! lowering it back as a B-spline surface loses the fact that it was ever
//! round — every downstream consumer, including the next modeller to open
//! the file, then sees an approximation. The analytic cases are also the ones
//! whose intersections have closed forms, which is the difference between a
//! boolean that lands on the exact circle two cylinders share and one that
//! lands near it.
//!
//! Surfaces this layer does not model yet are not lost either: a face lifted
//! from a file and left alone lowers back as its own record. See
//! [`Provenance`](super::Provenance).

use ocs_geom::geom2d::NurbsCurve;
use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};

/// A surface a face lies on.
///
/// `(u, v)` is the surface's own parameter space, which is where a face's
/// loops are resolved and where a boolean does its cutting.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    /// Flat. `(u, v)` are the plane's own coordinates.
    Plane(Plane),
    /// A right circular cylinder about `axis`. `u` runs round it in radians,
    /// `v` along the axis.
    Cylinder(Cylinder),
    /// A right circular cone, which a cylinder is the zero-angle case of —
    /// kept separate because ACIS stores them as one record with a flag, and
    /// because their intersections behave differently.
    Cone(Cone),
    /// A sphere. `u` is longitude, `v` latitude, both in radians.
    Sphere(Sphere),
    /// A torus. `u` runs the major circle, `v` the minor.
    Torus(Torus),
    /// A tensor-product spline surface in its knot parameters.
    Nurbs(NurbsSurface3),
    /// A spline surface pushed along its own normal by a distance, kept as
    /// such because ACIS stores it so (`offsur`): written back as a plain
    /// spline it would lose what it was. Its parameters are the
    /// approximation's, which every evaluation, mesh and intersection uses.
    Offset(OffsetSurface),
}

/// A spline surface offset along its normal (`u × v`).
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetSurface {
    /// The surface offset.
    pub base: NurbsSurface3,
    /// The signed distance along the base's normal.
    pub distance: f64,
    /// A spline through the offset, in the base's parameters.
    pub approximation: NurbsSurface3,
}

/// A right circular cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    /// A point on the axis, and the frame `u` is measured from: `x_axis` is
    /// where `u = 0` points, and the axis itself is the frame's normal.
    pub base: Plane,
    /// Distance from the axis.
    pub radius: f64,
}

/// A right circular cone.
///
/// Both nappes, as ACIS stores it: past the apex the radius passes through
/// zero and grows again on the mirrored half, and a single record covers the
/// pair. A ray up the side of one therefore meets the other as well, and a
/// caller wanting only one nappe bounds it with the face's own extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    /// The frame at the reference circle, as for a cylinder.
    pub base: Plane,
    /// Radius of that circle. This is the radius *at the base*, not at the
    /// apex — reading it as the apex radius puts every generator on the wrong
    /// slope.
    pub radius: f64,
    /// Half-angle at the apex, positive when the cone narrows along the axis.
    pub half_angle: f64,
}

/// A sphere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    /// Centre, and the frame longitude and latitude are measured in.
    pub frame: Plane,
    /// Radius.
    pub radius: f64,
}

/// A torus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus {
    /// Centre, and the frame the major circle lies in.
    pub frame: Plane,
    /// Distance from the centre to the middle of the tube.
    pub major_radius: f64,
    /// Radius of the tube.
    pub minor_radius: f64,
}

impl Surface {
    /// The spline a spline surface is evaluated through: its own, or an
    /// offset's approximation.
    pub fn spline(&self) -> Option<&NurbsSurface3> {
        match self {
            Self::Nurbs(surface) | Self::Offset(OffsetSurface { approximation: surface, .. }) => {
                Some(surface)
            }
            _ => None,
        }
    }

    /// Whether the surface is a spline (an offset is one too).
    pub fn is_spline(&self) -> bool {
        self.spline().is_some()
    }

    /// The point at surface parameters `(u, v)`.
    pub fn point_at(&self, u: f64, v: f64) -> [f64; 3] {
        match self {
            Self::Plane(plane) => plane.point_at([u, v]),
            Self::Cylinder(cylinder) => {
                let around = cylinder
                    .base
                    .point_at([cylinder.radius * u.cos(), cylinder.radius * u.sin()]);
                offset_along_normal(&cylinder.base, around, v)
            }
            Self::Cone(cone) => {
                // The radius shrinks along the axis at the tangent of the
                // half-angle, from the *base* circle.
                let radius = cone.radius - v * cone.half_angle.tan();
                let around = cone.base.point_at([radius * u.cos(), radius * u.sin()]);
                offset_along_normal(&cone.base, around, v)
            }
            Self::Sphere(sphere) => {
                let ring = sphere.radius * v.cos();
                let around = sphere.frame.point_at([ring * u.cos(), ring * u.sin()]);
                offset_along_normal(&sphere.frame, around, sphere.radius * v.sin())
            }
            Self::Torus(torus) => {
                let ring = torus.major_radius + torus.minor_radius * v.cos();
                let around = torus.frame.point_at([ring * u.cos(), ring * u.sin()]);
                offset_along_normal(&torus.frame, around, torus.minor_radius * v.sin())
            }
            Self::Nurbs(surface) | Self::Offset(OffsetSurface { approximation: surface, .. }) => surface.point_at_knot(u, v),
        }
    }

    /// Tangents along the surface's `u` and `v` parameters.
    pub fn tangents_at(&self, u: f64, v: f64) -> Option<([f64; 3], [f64; 3])> {
        match self {
            Self::Plane(plane) => Some((plane.x_axis, plane.y_axis)),
            Self::Cylinder(cylinder) => Some((
                cylinder
                    .base
                    .vector_at([-cylinder.radius * u.sin(), cylinder.radius * u.cos()]),
                cylinder.base.normal()?,
            )),
            Self::Cone(cone) => {
                let radius = cone.radius - v * cone.half_angle.tan();
                let radial = cone.base.vector_at([u.cos(), u.sin()]);
                let along =
                    Vec3::from(cone.base.normal()?) - Vec3::from(radial) * cone.half_angle.tan();
                Some((
                    cone.base.vector_at([-radius * u.sin(), radius * u.cos()]),
                    along.to_array(),
                ))
            }
            Self::Sphere(sphere) => {
                let around = sphere.frame.vector_at([-u.sin(), u.cos()]);
                let radial = sphere.frame.vector_at([u.cos(), u.sin()]);
                let normal = Vec3::from(sphere.frame.normal()?);
                Some((
                    (Vec3::from(around) * (sphere.radius * v.cos())).to_array(),
                    (Vec3::from(radial) * (-sphere.radius * v.sin())
                        + normal * (sphere.radius * v.cos()))
                    .to_array(),
                ))
            }
            Self::Torus(torus) => {
                let around = torus.frame.vector_at([-u.sin(), u.cos()]);
                let radial = torus.frame.vector_at([u.cos(), u.sin()]);
                let normal = Vec3::from(torus.frame.normal()?);
                let ring = torus.major_radius + torus.minor_radius * v.cos();
                Some((
                    (Vec3::from(around) * ring).to_array(),
                    (Vec3::from(radial) * (-torus.minor_radius * v.sin())
                        + normal * (torus.minor_radius * v.cos()))
                    .to_array(),
                ))
            }
            Self::Nurbs(surface) | Self::Offset(OffsetSurface { approximation: surface, .. }) => surface.tangents_at_knot(u, v),
        }
    }

    /// Natural unit normal at `(u, v)`.
    pub fn normal_at(&self, u: f64, v: f64) -> Option<[f64; 3]> {
        match self {
            Self::Plane(plane) => plane.normal(),
            Self::Cylinder(cylinder) => Vec3::from(cylinder.base.vector_at([u.cos(), u.sin()]))
                .normalize()
                .map(Vec3::to_array),
            Self::Sphere(sphere) => (Vec3::from(self.point_at(u, v))
                - Vec3::from(sphere.frame.origin))
            .normalize()
            .map(Vec3::to_array),
            Self::Torus(torus) => {
                let radial = Vec3::from(torus.frame.vector_at([u.cos(), u.sin()]));
                let normal = Vec3::from(torus.frame.normal()?);
                ((radial * v.cos() + normal * v.sin()) * torus.minor_radius.signum())
                    .normalize()
                    .map(Vec3::to_array)
            }
            Self::Cone(cone) => {
                let radial = Vec3::from(cone.base.vector_at([u.cos(), u.sin()]));
                let axis = Vec3::from(cone.base.normal()?);
                (radial + axis * cone.half_angle.tan())
                    .normalize()
                    .map(Vec3::to_array)
            }
            Self::Nurbs(_) | Self::Offset(_) => {
                let (along_u, along_v) = self.tangents_at(u, v)?;
                Vec3::from(along_u)
                    .cross(Vec3::from(along_v))
                    .normalize()
                    .map(Vec3::to_array)
            }
        }
    }

    /// The frame a surface's parameters are measured in, for the kinds that
    /// have one. Every variant here does; the accessor exists so a caller can
    /// move a surface without matching on its kind.
    pub fn frame(&self) -> Option<&Plane> {
        Some(match self {
            Self::Plane(plane) => plane,
            Self::Cylinder(cylinder) => &cylinder.base,
            Self::Cone(cone) => &cone.base,
            Self::Sphere(sphere) => &sphere.frame,
            Self::Torus(torus) => &torus.frame,
            Self::Nurbs(_) | Self::Offset(_) => return None,
        })
    }

    /// The `(u, v)` of a point on the surface — the inverse of
    /// [`point_at`](Self::point_at).
    ///
    /// A point off the surface is answered for the nearest point on it, so a
    /// caller holding an intersection result does not have to be exact
    /// first. `None` only where the frame is degenerate, or at a place with
    /// no single answer — a sphere's pole, where every longitude meets.
    pub fn parameters_at(&self, point: [f64; 3]) -> Option<(f64, f64)> {
        match self {
            Self::Plane(plane) => {
                let local = plane.project(point)?;
                Some((local[0], local[1]))
            }
            Self::Cylinder(cylinder) => {
                let local = cylinder.base.project(point)?;
                let height = height_above(&cylinder.base, point)?;
                Some((local[1].atan2(local[0]), height))
            }
            Self::Cone(cone) => {
                let local = cone.base.project(point)?;
                let height = height_above(&cone.base, point)?;
                Some((local[1].atan2(local[0]), height))
            }
            Self::Sphere(sphere) => {
                let local = sphere.frame.project(point)?;
                let height = height_above(&sphere.frame, point)?;
                let ring = local[0].hypot(local[1]);
                if ring <= 0.0 {
                    // A pole. Every longitude passes through it, so there is
                    // no `u` to report rather than an arbitrary one.
                    return None;
                }
                Some((local[1].atan2(local[0]), height.atan2(ring)))
            }
            Self::Torus(torus) => {
                if torus.minor_radius.abs() <= f64::EPSILON {
                    return None;
                }
                let local = torus.frame.project(point)?;
                let height = height_above(&torus.frame, point)?;
                let ring = local[0].hypot(local[1]);
                let around = local[1].atan2(local[0]);
                let minor_sign = torus.minor_radius.signum();
                let outer = (
                    around,
                    (height * minor_sign).atan2((ring - torus.major_radius) * minor_sign),
                );
                // A horn or spindle torus has a second sheet whose signed
                // ring is negative. In space that reverses the longitude by
                // half a turn; `hypot` alone folds it onto the other sheet.
                // Try both inverses and retain the one which actually maps
                // back nearest to the supplied point.
                let inner = (
                    around + std::f64::consts::PI,
                    (height * minor_sign).atan2((-ring - torus.major_radius) * minor_sign),
                );
                let error = |parameters: (f64, f64)| {
                    Vec3::from(self.point_at(parameters.0, parameters.1))
                        .distance(Vec3::from(point))
                };
                if error(inner) < error(outer) {
                    Some(inner)
                } else {
                    Some(outer)
                }
            }
            Self::Nurbs(surface) | Self::Offset(OffsetSurface { approximation: surface, .. }) => surface.parameters_at(point),
        }
    }

    /// Where a ray meets the surface, as distances along `direction`.
    ///
    /// In order, and including negative ones — behind the origin is still on
    /// the surface, and a caller counting crossings ahead filters for itself.
    ///
    /// `None` where a spline patch's crossing will not settle. As everywhere
    /// else here, that is said rather than answered with an empty list a
    /// caller would read as "no hits".
    pub fn ray_hits(&self, origin: [f64; 3], direction: [f64; 3]) -> Option<Vec<f64>> {
        let start = Vec3::from(origin);
        let along = Vec3::from(direction);
        match self {
            Self::Plane(plane) => {
                let normal = Vec3::from(plane.normal()?);
                let slope = along.dot(normal);
                if slope == 0.0 {
                    // Parallel: either no hit, or the ray lies in the plane
                    // and every point is one. Neither is a crossing.
                    return Some(Vec::new());
                }
                Some(vec![-plane.distance_to(origin)? / slope])
            }
            Self::Sphere(sphere) => {
                let offset = start - Vec3::from(sphere.frame.origin);
                Some(roots(
                    along.length_squared(),
                    2.0 * offset.dot(along),
                    offset.length_squared() - sphere.radius * sphere.radius,
                ))
            }
            Self::Cylinder(cylinder) => {
                let axis = Vec3::from(cylinder.base.normal()?);
                let offset = start - Vec3::from(cylinder.base.origin);
                let (across_offset, across_along) =
                    (perpendicular(offset, axis), perpendicular(along, axis));
                Some(roots(
                    across_along.length_squared(),
                    2.0 * across_offset.dot(across_along),
                    across_offset.length_squared() - cylinder.radius * cylinder.radius,
                ))
            }
            Self::Cone(cone) => {
                let axis = Vec3::from(cone.base.normal()?);
                let offset = start - Vec3::from(cone.base.origin);
                let (across_offset, across_along) =
                    (perpendicular(offset, axis), perpendicular(along, axis));
                // The radius shrinks along the axis, so the condition is
                // |q perpendicular| = radius at that height rather than a
                // constant.
                let slope = cone.half_angle.tan();
                let (up_offset, up_along) = (offset.dot(axis), along.dot(axis));
                let at_start = cone.radius - slope * up_offset;
                let shrink = slope * up_along;
                Some(roots(
                    across_along.length_squared() - shrink * shrink,
                    2.0 * (across_offset.dot(across_along) + at_start * shrink),
                    across_offset.length_squared() - at_start * at_start,
                ))
            }
            Self::Torus(torus) => {
                // The section is a quartic in the ray parameter, solved in
                // closed form. It is set up on the unit direction from the
                // line's point nearest the centre, in units of the torus's
                // size, so its coefficients are near one whatever the model's
                // scale and the ray's start. A graze (a double root) is no
                // crossing and is left out, keeping a crossing count's parity.
                let axis = Vec3::from(torus.frame.normal()?);
                let (major, minor) = (torus.major_radius.abs(), torus.minor_radius.abs());
                let (length, size) = (along.length(), major + minor);
                if !(length > 0.0 && size > 0.0) {
                    return Some(Vec::new());
                }
                let unit = along / length;
                let offset = start - Vec3::from(torus.frame.origin);
                let shift = -offset.dot(unit);
                let near = (offset + unit * shift) / size;
                let (big, small) = (major / size, minor / size);
                let (h0, h1) = (near.dot(axis), unit.dot(axis));
                let squared = near.length_squared();
                let c = squared + big * big - small * small;
                let four = 4.0 * big * big;
                // |q|^4-style expansion with `near` perpendicular to `unit`.
                let coefficients = [
                    c * c - four * (squared - h0 * h0),
                    2.0 * four * h0 * h1,
                    2.0 * c - four * (1.0 - h1 * h1),
                    0.0,
                    1.0,
                ];
                let quartic = |s: f64| {
                    coefficients.iter().rev().fold(0.0, |value, &k| value * s + k)
                };
                let slope = |s: f64| {
                    4.0 * s * s * s
                        + 3.0 * coefficients[3] * s * s
                        + 2.0 * coefficients[2] * s
                        + coefficients[1]
                };
                const STEP: f64 = 1e-9;
                let mut hits: Vec<f64> =
                    nlib::geom::polynomial::solve_quartic(&coefficients, 1e-12)
                        .iter()
                        .filter_map(|&root| {
                            // Newton polishes what the closed form rounds.
                            let mut s = root;
                            for _ in 0..3 {
                                let d = slope(s);
                                if d == 0.0 {
                                    break;
                                }
                                let next = s - quartic(s) / d;
                                if !next.is_finite() {
                                    break;
                                }
                                s = next;
                            }
                            let crosses = (quartic(s - STEP) > 0.0) != (quartic(s + STEP) > 0.0);
                            crosses.then_some((shift + s * size) / length)
                        })
                        .collect();
                hits.sort_by(f64::total_cmp);
                hits.dedup_by(|a, b| (*a - *b).abs() <= STEP * size / length);
                Some(hits)
            }
            Self::Nurbs(surface) | Self::Offset(OffsetSurface { approximation: surface, .. }) => nurbs_ray_hits(surface, start, along),
        }
    }

    /// Whether `point` lies on the surface, to within `tolerance`.
    ///
    /// The check a lift wants: a file's topology says a face sits on a
    /// surface, and a vertex that does not is a parse gone wrong rather than
    /// a shape.
    pub fn contains(&self, point: [f64; 3], tolerance: f64) -> bool {
        self.distance_to(point).abs() <= tolerance
    }

    /// Signed distance from `point` to the surface, positive outside.
    ///
    /// Exact for every analytic kind here — no search, no sampling.
    pub fn distance_to(&self, point: [f64; 3]) -> f64 {
        match self {
            Self::Plane(plane) => plane.distance_to(point).unwrap_or(f64::INFINITY),
            Self::Cylinder(cylinder) => axial_distance(&cylinder.base, point).1 - cylinder.radius,
            Self::Cone(cone) => {
                let (along, across) = axial_distance(&cone.base, point);
                // In the (across, along) half plane the cone's profile is not
                // one line but two, meeting at the apex — the record covers
                // both nappes, so past the apex the radius grows again on the
                // mirrored half. Taking the magnitude is what folds the
                // second one in; reading the first line's own extension
                // instead put every point on the far nappe several units off
                // a surface it lies exactly on, and a seam running up to the
                // apex was refused as not being on its own cone.
                let cos = cone.half_angle.cos();
                (across - (cone.radius - along * cone.half_angle.tan()).abs()) * cos
            }
            Self::Sphere(sphere) => {
                Vec3::from(point).distance(Vec3::from(sphere.frame.origin)) - sphere.radius
            }
            Self::Torus(torus) => {
                let (along, across) = axial_distance(&torus.frame, point);
                let minor_radius = torus.minor_radius.abs();
                let outer = (across - torus.major_radius).hypot(along) - minor_radius;
                let inner = (across + torus.major_radius).hypot(along) - minor_radius;
                // For a self-intersecting torus neither signed half-plane
                // circle contains the other parametrised sheet. The nearest
                // sheet is the one with the smaller absolute residual.
                if inner.abs() < outer.abs() {
                    inner
                } else {
                    outer
                }
            }
            // Unsigned off the patch's side, where the nearest point is on
            // its edge and the normal says nothing about the side.
            Self::Nurbs(surface) | Self::Offset(OffsetSurface { approximation: surface, .. }) => {
                let Some((u, v)) = surface.parameters_at(point) else {
                    return f64::INFINITY;
                };
                let offset = Vec3::from(point) - Vec3::from(surface.point_at_knot(u, v));
                let side = self
                    .normal_at(u, v)
                    .map_or(1.0, |normal| offset.dot(Vec3::from(normal)).signum());
                offset.length() * if side < 0.0 { -1.0 } else { 1.0 }
            }
        }
    }
}

/// Where a ray meets a spline patch: the crossings of a grid of the patch's
/// own points, each then refined onto the patch itself by Newton's method in
/// `(u, v, t)`. `None` when a crossing will not settle — a ray grazing the
/// patch, whose count of crossings is not to be trusted either way.
fn nurbs_ray_hits(surface: &NurbsSurface3, origin: Vec3, direction: Vec3) -> Option<Vec<f64>> {
    // The patch lies within its control net's box (positive weights); a
    // line passing the box meets none of it, and the grid below costs
    // thousands of evaluations a ray.
    if surface.weights().iter().flatten().all(|weight| *weight > 0.0) {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for point in surface.control_points().iter().flatten() {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        let (start, along) = (origin.to_array(), direction.to_array());
        let reach = (0..3).map(|axis| high[axis] - low[axis]).fold(0.0, f64::max) * 1e-9;
        let (mut enter, mut leave) = (f64::NEG_INFINITY, f64::INFINITY);
        for axis in 0..3 {
            let (from, to) = (low[axis] - reach, high[axis] + reach);
            if along[axis].abs() <= f64::EPSILON {
                if start[axis] < from || start[axis] > to {
                    return Some(Vec::new());
                }
                continue;
            }
            let (a, b) = ((from - start[axis]) / along[axis], (to - start[axis]) / along[axis]);
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
        }
        if enter > leave {
            return Some(Vec::new());
        }
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (u_knots, v_knots) = surface.knots();
    // ponytail: four cells a knot span; a patch folding tighter than that can
    // hide a pair of crossings, which leaves the parity intact.
    let cells = |knots: &[f64], low: f64, high: f64| {
        let spans = knots
            .windows(2)
            .filter(|pair| pair[1] > pair[0] && pair[0] >= low && pair[1] <= high)
            .count();
        (spans * 4).clamp(8, 96)
    };
    let (columns, rows) = (cells(u_knots, u0, u1), cells(v_knots, v0, v1));
    let at = |i: usize, j: usize| {
        (
            u0 + (u1 - u0) * i as f64 / columns as f64,
            v0 + (v1 - v0) * j as f64 / rows as f64,
        )
    };
    let grid: Vec<Vec<Vec3>> = (0..=columns)
        .map(|i| {
            (0..=rows)
                .map(|j| {
                    let (u, v) = at(i, j);
                    Vec3::from(surface.point_at_knot(u, v))
                })
                .collect()
        })
        .collect();
    let mut hits: Vec<f64> = Vec::new();
    for i in 0..columns {
        for j in 0..rows {
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            for triangle in [[0, 1, 2], [0, 2, 3]] {
                let [a, b, c] = triangle.map(|k| corners[k]);
                let corners = [grid[a.0][a.1], grid[b.0][b.1], grid[c.0][c.1]];
                let Some((t, beta, gamma)) = ray_triangle(origin, direction, corners) else {
                    continue;
                };
                let ((ua, va), (ub, vb), (uc, vc)) = (at(a.0, a.1), at(b.0, b.1), at(c.0, c.1));
                let alpha = 1.0 - beta - gamma;
                let guess = (
                    alpha * ua + beta * ub + gamma * uc,
                    alpha * va + beta * vb + gamma * vc,
                );
                let t = refine_ray_hit(surface, origin, direction, guess, t)?;
                if !hits.iter().any(|hit| (hit - t).abs() <= 1e-9 * (1.0 + t.abs())) {
                    hits.push(t);
                }
            }
        }
    }
    hits.sort_by(f64::total_cmp);
    Some(hits)
}

/// Where a ray crosses a triangle: its distance and the crossing's second
/// and third barycentric weights. The edges count as inside — a crossing on
/// one is found from both its triangles and kept once.
fn ray_triangle(origin: Vec3, direction: Vec3, [a, b, c]: [Vec3; 3]) -> Option<(f64, f64, f64)> {
    let (ab, ac) = (b - a, c - a);
    let cross = direction.cross(ac);
    let determinant = ab.dot(cross);
    if determinant.abs() <= f64::EPSILON * ab.length() * ac.length() {
        return None;
    }
    let from = origin - a;
    let beta = from.dot(cross) / determinant;
    let back = from.cross(ab);
    let gamma = direction.dot(back) / determinant;
    const SLACK: f64 = 1e-9;
    (beta >= -SLACK && gamma >= -SLACK && beta + gamma <= 1.0 + SLACK)
        .then(|| (ac.dot(back) / determinant, beta, gamma))
}

/// A grid crossing pulled onto the patch: `S(u, v) = origin + t·direction`
/// solved from the grid's guess.
fn refine_ray_hit(
    surface: &NurbsSurface3,
    origin: Vec3,
    direction: Vec3,
    (mut u, mut v): (f64, f64),
    mut t: f64,
) -> Option<f64> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let scale = 1.0 + origin.length() + t.abs() * direction.length();
    for _ in 0..32 {
        let residual = Vec3::from(surface.point_at_knot(u, v)) - (origin + direction * t);
        if residual.length() <= 1e-10 * scale {
            return Some(t);
        }
        let (along_u, along_v) = surface.tangents_at_knot(u, v)?;
        let (along_u, along_v, back) = (Vec3::from(along_u), Vec3::from(along_v), direction * -1.0);
        let determinant = along_u.dot(along_v.cross(back));
        if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
            return None;
        }
        // Cramer's rule for J·step = -residual, J = [S_u S_v -d].
        let target = residual * -1.0;
        u += target.dot(along_v.cross(back)) / determinant;
        v += along_u.dot(target.cross(back)) / determinant;
        t += along_u.dot(along_v.cross(target)) / determinant;
        u = u.clamp(u0, u1);
        v = v.clamp(v0, v1);
    }
    let residual = Vec3::from(surface.point_at_knot(u, v)) - (origin + direction * t);
    (residual.length() <= 1e-7 * scale).then_some(t)
}

/// Moves `point` by `distance` along `frame`'s normal.
fn offset_along_normal(frame: &Plane, point: [f64; 3], distance: f64) -> [f64; 3] {
    match frame.normal() {
        Some(normal) => (Vec3::from(point) + Vec3::from(normal) * distance).to_array(),
        None => point,
    }
}

/// How far `point` sits along a frame's normal.
fn height_above(frame: &Plane, point: [f64; 3]) -> Option<f64> {
    frame.distance_to(point)
}

/// The part of `vector` that is not along `axis`, which must be unit.
fn perpendicular(vector: Vec3, axis: Vec3) -> Vec3 {
    vector - axis * vector.dot(axis)
}

/// Real roots of `a·t² + b·t + c`, in order.
///
/// Falls back to the linear case when the quadratic term vanishes, which is
/// how a ray parallel to a cone's own slope is answered rather than divided
/// by nothing.
fn roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() <= f64::MIN_POSITIVE {
        if b.abs() <= f64::MIN_POSITIVE {
            return Vec::new();
        }
        return vec![-c / b];
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return Vec::new();
    }
    let root = discriminant.sqrt();
    // The stable pairing: computing both roots from the subtraction loses
    // the small one entirely when b dwarfs the discriminant.
    let stable = -0.5 * (b + b.signum() * root);
    let mut out = if stable == 0.0 {
        vec![0.0]
    } else {
        vec![stable / a, c / stable]
    };
    out.sort_by(f64::total_cmp);
    out.dedup_by(|x, y| x == y);
    out
}

/// How far `point` is along a frame's axis, and how far from it.
fn axial_distance(frame: &Plane, point: [f64; 3]) -> (f64, f64) {
    let offset = Vec3::from(point) - Vec3::from(frame.origin);
    match frame.normal() {
        Some(normal) => {
            let along = offset.dot(Vec3::from(normal));
            let across = (offset - Vec3::from(normal) * along).length();
            (along, across)
        }
        None => (0.0, offset.length()),
    }
}

/// A curve an edge runs along, in space.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve3 {
    /// A straight line. `t` advances by `direction` per unit, so a segment's
    /// two vertices sit at whatever parameters the edge records.
    Line(Line3),
    /// A circle or a circular arc, lying in `plane`. `t` is the angle in
    /// radians measured from the plane's x axis.
    Circle(Circle3),
    /// An ellipse, lying in `plane`. `t` is the ellipse's own parameter, not
    /// an angle — the two differ everywhere except on the axes.
    ///
    /// Where a plane cuts a cylinder at a slant this is what they share
    /// exactly; approximating it with a circle or a spline would put the
    /// seam of every such cut slightly off.
    Ellipse(Ellipse3),
    /// A spline curve lying in a plane.
    PlanarSpline {
        /// The plane it lies in.
        plane: Plane,
        /// The curve, in that plane's coordinates.
        curve: NurbsCurve,
    },
    /// A spline curve in space, evaluated in its knot parameter.
    Nurbs(NurbsCurve3),
}

/// A straight line in space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line3 {
    /// A point on it, where `t` reads zero.
    pub origin: [f64; 3],
    /// One unit of `t`.
    pub direction: [f64; 3],
}

/// A circle in space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle3 {
    /// Centre, and the frame the angle is measured in.
    pub plane: Plane,
    /// Radius.
    pub radius: f64,
}

/// An ellipse in space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse3 {
    /// Centre, and the frame it lies in. Its x axis is the major direction.
    pub plane: Plane,
    /// Half-length along the frame's x axis.
    pub major_radius: f64,
    /// Half-length along its y axis.
    pub minor_radius: f64,
}

impl Curve3 {
    /// The point at parameter `t`.
    pub fn point_at(&self, t: f64) -> [f64; 3] {
        match self {
            Self::Line(line) => {
                (Vec3::from(line.origin) + Vec3::from(line.direction) * t).to_array()
            }
            Self::Circle(circle) => circle
                .plane
                .point_at([circle.radius * t.cos(), circle.radius * t.sin()]),
            Self::Ellipse(ellipse) => ellipse.plane.point_at([
                ellipse.major_radius * t.cos(),
                ellipse.minor_radius * t.sin(),
            ]),
            Self::PlanarSpline { plane, curve } => plane.point_at(curve.point_at(t)),
            Self::Nurbs(curve) => curve.point_at_knot(t),
        }
    }

    /// Direction of travel at parameter `t`.
    pub fn tangent_at(&self, t: f64) -> [f64; 3] {
        match self {
            Self::Line(line) => line.direction,
            Self::Circle(circle) => circle
                .plane
                .vector_at([-circle.radius * t.sin(), circle.radius * t.cos()]),
            Self::Ellipse(ellipse) => ellipse.plane.vector_at([
                -ellipse.major_radius * t.sin(),
                ellipse.minor_radius * t.cos(),
            ]),
            Self::PlanarSpline { plane, curve } => plane.vector_at(curve.derivative_at(t)),
            Self::Nurbs(curve) => curve.tangent_at_knot(t),
        }
    }

    /// The parameter at `point`, the inverse of [`point_at`](Self::point_at).
    ///
    /// A point off the curve is projected onto it, so a caller matching a
    /// file's vertex against the curve its edge names does not have to be
    /// exact first.
    pub fn parameter_at(&self, point: [f64; 3]) -> f64 {
        match self {
            Self::Line(line) => {
                let along = Vec3::from(line.direction);
                let squared = along.length_squared();
                if squared <= 0.0 {
                    return 0.0;
                }
                (Vec3::from(point) - Vec3::from(line.origin)).dot(along) / squared
            }
            Self::Circle(circle) => match circle.plane.project(point) {
                Some(local) => local[1].atan2(local[0]),
                None => 0.0,
            },
            Self::Ellipse(ellipse) => match ellipse.plane.project(point) {
                // Squashed onto the unit circle, where the parameter reads
                // straight off. Taking the angle of the raw point instead
                // would be wrong by up to the eccentricity.
                Some(local) => {
                    (local[1] / ellipse.minor_radius).atan2(local[0] / ellipse.major_radius)
                }
                None => 0.0,
            },
            Self::PlanarSpline { plane, curve } => match plane.project(point) {
                Some(local) => curve.parameter_at(local),
                None => 0.0,
            },
            Self::Nurbs(curve) => curve.parameter_at(point),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

    fn xy() -> Plane {
        Plane::XY
    }

    #[test]
    fn a_plane_surface_is_its_own_coordinates() {
        let surface = Surface::Plane(xy());
        assert_eq!(surface.point_at(3.0, -4.0), [3.0, -4.0, 0.0]);
        assert!(surface.contains([3.0, -4.0, 0.0], 1e-9));
        assert!(!surface.contains([3.0, -4.0, 1.0], 1e-9));
        assert!((surface.distance_to([0.0, 0.0, 2.5]) - 2.5).abs() < 1e-12);
    }

    #[test]
    fn a_cylinder_wraps_round_its_axis_and_runs_along_it() {
        let surface = Surface::Cylinder(Cylinder {
            base: xy(),
            radius: 5.0,
        });
        let point = surface.point_at(0.0, 3.0);
        assert!((point[0] - 5.0).abs() < 1e-12 && point[1].abs() < 1e-12);
        assert!((point[2] - 3.0).abs() < 1e-12, "{point:?}");
        // Everything on it is exactly the radius from the axis, whatever the
        // height.
        for u in [0.0, 1.0, PI, 5.0] {
            for v in [-10.0, 0.0, 7.0] {
                assert!(surface.contains(surface.point_at(u, v), 1e-9));
            }
        }
        assert!((surface.distance_to([8.0, 0.0, 100.0]) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn a_cone_narrows_from_its_base_radius() {
        // Forty-five degrees, so it loses a unit of radius per unit of
        // height. Reading the radius as the apex one instead would put this
        // point a long way off.
        let surface = Surface::Cone(Cone {
            base: xy(),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        });
        let at_base = surface.point_at(0.0, 0.0);
        assert!((at_base[0] - 10.0).abs() < 1e-12);
        let higher = surface.point_at(0.0, 4.0);
        assert!((higher[0] - 6.0).abs() < 1e-12, "{higher:?}");
        for v in [0.0, 3.0, 9.0] {
            assert!(surface.contains(surface.point_at(1.2, v), 1e-9), "v={v}");
        }
    }

    #[test]
    fn a_cone_holds_the_nappe_past_its_own_apex() {
        // One record, both halves — which `ray_hits` has always said, since a
        // ray up one side meets the other. The distance has to agree: past
        // the apex the radius grows again on the mirrored half, and reading
        // the first half's generator extended instead reports a point lying
        // exactly on the cone as units away from it.
        //
        // What that cost was a cone's whole wall. A seam runs from the rim to
        // the apex, the check that a curve lies on its surface samples past
        // both ends, and the sample beyond the apex was refused — so the
        // face's boundary could not be projected and nothing of it was drawn.
        let surface = Surface::Cone(Cone {
            base: xy(),
            radius: 10.0,
            half_angle: FRAC_PI_4,
        });
        // The apex is ten up; twice that is ten out again on the far nappe.
        for (height, radius) in [
            (0.0, 10.0),
            (6.0, 4.0),
            (10.0, 0.0),
            (16.0, 6.0),
            (20.0, 10.0),
        ] {
            let on = [radius, 0.0, height];
            assert!(surface.distance_to(on).abs() < 1e-12, "{on:?}");
        }
        // And it still reads positive outside and negative in, both halves.
        assert!(surface.distance_to([14.0, 0.0, 20.0]) > 0.0);
        assert!(surface.distance_to([6.0, 0.0, 20.0]) < 0.0);
        assert!(surface.distance_to([14.0, 0.0, 0.0]) > 0.0);
        assert!(surface.distance_to([6.0, 0.0, 0.0]) < 0.0);
    }

    #[test]
    fn a_sphere_holds_every_point_at_its_radius() {
        let surface = Surface::Sphere(Sphere {
            frame: Plane::from_axes([1.0, 2.0, 3.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            radius: 4.0,
        });
        for u in [0.0, 1.0, PI] {
            for v in [-FRAC_PI_2 + 0.1, 0.0, FRAC_PI_2 - 0.1] {
                let point = surface.point_at(u, v);
                assert!(surface.contains(point, 1e-9), "u={u} v={v}");
                let radius = Vec3::from(point).distance(Vec3::new(1.0, 2.0, 3.0));
                assert!((radius - 4.0).abs() < 1e-9);
            }
        }
        // The pole sits a full radius up the frame's normal.
        let pole = surface.point_at(0.0, FRAC_PI_2);
        assert!((pole[2] - 7.0).abs() < 1e-9, "{pole:?}");
    }

    #[test]
    fn a_torus_holds_every_point_at_its_tube_radius() {
        let surface = Surface::Torus(Torus {
            frame: xy(),
            major_radius: 10.0,
            minor_radius: 2.0,
        });
        for u in [0.0, 1.0, PI, 5.0] {
            for v in [0.0, 1.0, PI, 5.0] {
                assert!(
                    surface.contains(surface.point_at(u, v), 1e-9),
                    "u={u} v={v}"
                );
            }
        }
        // The outermost point of the ring, and the innermost.
        assert!((surface.point_at(0.0, 0.0)[0] - 12.0).abs() < 1e-12);
        assert!((surface.point_at(0.0, PI)[0] - 8.0).abs() < 1e-12);
        // The centre of the hole is a major radius from the tube.
        assert!((surface.distance_to([0.0, 0.0, 0.0]) - 8.0).abs() < 1e-12);
    }

    /// A torus's ray hits are the quartic's roots in closed form, exact from
    /// any start and scale; a graze is no crossing.
    #[test]
    fn torus_ray_hits_are_exact() {
        let surface = Surface::Torus(Torus { frame: xy(), major_radius: 5.0, minor_radius: 1.0 });
        let check = |origin: [f64; 3], direction: [f64; 3], expected: &[f64]| {
            let hits = surface.ray_hits(origin, direction).unwrap();
            assert_eq!(hits.len(), expected.len(), "{origin:?} {hits:?}");
            for (hit, want) in hits.iter().zip(expected) {
                assert!((hit - want).abs() < 1e-9, "{hit} {want}");
            }
        };
        check([0.0; 3], [1.0, 0.0, 0.0], &[-6.0, -4.0, 4.0, 6.0]);
        check([1000.0, 0.0, 0.0], [-2.0, 0.0, 0.0], &[497.0, 498.0, 502.0, 503.0]);
        let r = 0.75_f64.sqrt();
        check([0.0, -20.0, 0.5], [0.0, 1.0, 0.0], &[20.0 - 5.0 - r, 20.0 - 5.0 + r, 20.0 + 5.0 - r, 20.0 + 5.0 + r]);
        check([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], &[]);
        check([0.0, 0.0, 3.0], [1.0, 0.0, 0.0], &[]);
        // Down through the tube.
        check([5.0, 0.0, 10.0], [0.0, 0.0, -1.0], &[9.0, 11.0]);
        // Large and far: same answers scaled.
        let big = Surface::Torus(Torus { frame: xy(), major_radius: 5e4, minor_radius: 1e4 });
        let hits = big.ray_hits([1e7, 0.0, 0.0], [-1.0, 0.0, 0.0]).unwrap();
        let want = [1e7 - 6e4, 1e7 - 4e4, 1e7 + 4e4, 1e7 + 6e4];
        assert_eq!(hits.len(), 4);
        for (hit, want) in hits.iter().zip(want) {
            assert!((hit - want).abs() < 1e-6, "{hit} {want}");
        }
    }

    #[test]
    fn negative_minor_radius_torus_parameters_round_trip() {
        let surface = Surface::Torus(Torus {
            frame: xy(),
            major_radius: 12.65,
            minor_radius: -2.0,
        });
        for u in [0.0, 0.7, PI, 5.2] {
            for v in [0.0, 0.8, PI, 5.4] {
                let point = surface.point_at(u, v);
                let parameters = surface.parameters_at(point).unwrap();
                let recovered = surface.point_at(parameters.0, parameters.1);
                assert!(
                    Vec3::from(recovered).distance(Vec3::from(point)) < 1e-9,
                    "u={u} v={v} parameters={parameters:?} recovered={recovered:?}"
                );
                assert!(surface.contains(point, 1e-9), "u={u} v={v}");
                let (along_u, along_v) = surface.tangents_at(parameters.0, parameters.1).unwrap();
                let tangent_normal = Vec3::from(along_u)
                    .cross(Vec3::from(along_v))
                    .normalize()
                    .unwrap();
                let reported = Vec3::from(surface.normal_at(parameters.0, parameters.1).unwrap());
                assert!(tangent_normal.dot(reported) > 1.0 - 1e-12);
            }
        }
    }

    #[test]
    fn a_tilted_frame_carries_the_whole_surface_with_it() {
        let upright = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let surface = Surface::Cylinder(Cylinder {
            base: upright,
            radius: 3.0,
        });
        // The axis is now +Y, so running along it moves in Y.
        let point = surface.point_at(0.0, 5.0);
        assert!((point[1] - 5.0).abs() < 1e-9, "{point:?}");
        assert!(surface.contains(point, 1e-9));
    }

    #[test]
    fn a_straight_edge_runs_between_its_parameters() {
        let curve = Curve3::Line(Line3 {
            origin: [1.0, 2.0, 3.0],
            direction: [0.0, 0.0, 4.0],
        });
        assert_eq!(curve.point_at(0.0), [1.0, 2.0, 3.0]);
        assert_eq!(curve.point_at(2.0), [1.0, 2.0, 11.0]);
        assert!((curve.parameter_at([1.0, 2.0, 11.0]) - 2.0).abs() < 1e-12);
        // A point beside the line projects onto it rather than being refused.
        assert!((curve.parameter_at([9.0, 9.0, 7.0]) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_circular_edge_reads_its_parameter_as_an_angle() {
        let curve = Curve3::Circle(Circle3 {
            plane: xy(),
            radius: 6.0,
        });
        assert_eq!(curve.point_at(0.0), [6.0, 0.0, 0.0]);
        let quarter = curve.point_at(FRAC_PI_2);
        assert!(quarter[0].abs() < 1e-12 && (quarter[1] - 6.0).abs() < 1e-12);
        assert!((curve.parameter_at([0.0, 6.0, 0.0]) - FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn a_circular_edge_on_a_tilted_plane_stays_on_it() {
        let plane = Plane::orthonormal([3.0, 4.0, 5.0], [1.0, 1.0, 0.0], [0.0, 1.0, 1.0]).unwrap();
        let curve = Curve3::Circle(Circle3 { plane, radius: 2.0 });
        for i in 0..8 {
            let point = curve.point_at(TAU * i as f64 / 8.0);
            assert!(plane.contains(point, 1e-9), "{point:?}");
            assert!((Vec3::from(point).distance(Vec3::from(plane.origin)) - 2.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_planar_spline_edge_evaluates_through_its_plane() {
        let plane = Plane::orthonormal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).unwrap();
        let curve = Curve3::PlanarSpline {
            plane,
            curve: NurbsCurve::new(
                2,
                vec![[0.0, 0.0], [5.0, 5.0], [10.0, 0.0]],
                Vec::new(),
                None,
            )
            .unwrap(),
        };
        assert_eq!(curve.point_at(0.0), [0.0, 0.0, 0.0]);
        let end = curve.point_at(1.0);
        assert!((end[0] - 10.0).abs() < 1e-9, "{end:?}");
        for i in 0..=8 {
            let t = i as f64 / 8.0;
            assert!(plane.contains(curve.point_at(t), 1e-9));
        }
    }

    #[test]
    fn survey_coordinates_keep_a_surface_where_it_is() {
        let far = Plane::from_axes(
            [512_345.678, 4_512_345.678, 91.5],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        );
        let surface = Surface::Cylinder(Cylinder {
            base: far,
            radius: 0.5,
        });
        for u in [0.0, 1.0, 3.0] {
            assert!(surface.contains(surface.point_at(u, 2.0), 1e-9), "u={u}");
        }
    }
}
