//! The exact NURBS form of every curve and surface an edge or face can lie on.
//!
//! The scheme is Solidcore's (`eval/form.rs`): angles run on rational
//! quadratic quarter arcs laid on their frame's quarter grid. Quarter `q`
//! covers `q·π/2 ..= (q+1)·π/2` from the frame's x axis, and an angle `θ` in
//! it has form parameter `q + u`, `u` the arc's own parameter there
//! ([`quarter_parameter`]). Every form on one frame therefore has the same
//! parameter at the same angle, whatever range it spans: a circle edge's form
//! and its cylinder's form agree along it.
//!
//! A circle or ellipse is the arcs (an ellipse scaled by its radii, which
//! keeps it exact: its parameter is the eccentric angle). A cylinder or cone
//! is the arcs times a degree-1 line along the axis, a sphere the arcs times
//! arcs of latitude, a torus the arcs times the arcs of its tube; a plane is
//! bilinear and a line degree 1, both on their own parameters. Each is cut
//! to the range asked.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use super::geometry::{Curve3, OffsetSurface, Surface};
use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Vec3};

/// The form parameter of `angle` (radians) on the quarter grid: `q + u`, `q`
/// its quarter and `u` the parameter of the quarter's rational arc there.
pub fn quarter_parameter(angle: f64) -> f64 {
    let quarter = (angle / FRAC_PI_2).floor();
    let local = angle - quarter * FRAC_PI_2;
    // On the arc with weights (1, cos α, 1) over 2α, tan(φ/2) is
    // u sin α / ((1 - u) + u cos α); solved for u.
    let t = (local / 2.0).tan();
    quarter + t / (FRAC_PI_4.sin() + t * (1.0 - FRAC_PI_4.cos()))
}

/// The unit circle over `angles` (radians, increasing), its control points'
/// `(cos, sin)` in the first two coordinates.
fn unit_arc([from, to]: [f64; 2]) -> Option<NurbsCurve3> {
    if !(from.is_finite() && to.is_finite() && to > from) || to - from > 128.0 * FRAC_PI_2 {
        return None;
    }
    let first = (from / FRAC_PI_2).floor();
    let last = (to / FRAC_PI_2).ceil().max(first + 1.0);
    let quarters = (last - first) as usize;
    let middle = FRAC_PI_4.cos();
    let mut points = Vec::with_capacity(2 * quarters + 1);
    let mut weights = Vec::with_capacity(2 * quarters + 1);
    let mut knots = vec![first; 3];
    for k in 0..quarters {
        let start = (first + k as f64) * FRAC_PI_2;
        points.push([start.cos(), start.sin(), 0.0]);
        weights.push(1.0);
        let mid = start + FRAC_PI_4;
        points.push([mid.cos() / middle, mid.sin() / middle, 0.0]);
        weights.push(middle);
        knots.extend([first + k as f64 + 1.0; 2]);
    }
    let end = last * FRAC_PI_2;
    points.push([end.cos(), end.sin(), 0.0]);
    weights.push(1.0);
    *knots.last_mut()? = last;
    knots.push(last);
    let whole = NurbsCurve3::new_strict(2, points, knots, weights)?;
    whole.segment(quarter_parameter(from), quarter_parameter(to))
}

/// The degree-1 curve from `a` at `from` to `b` at `to`.
fn line(a: [f64; 3], b: [f64; 3], [from, to]: [f64; 2]) -> Option<NurbsCurve3> {
    NurbsCurve3::new_strict(1, vec![a, b], vec![from, from, to, to], vec![1.0; 2])
}

/// The tensor product of two rational curves through a map bilinear in
/// their homogeneous coordinates: control point `(i, j)` is
/// `map(P_i, Q_j)`, weight `w_i w_j`.
fn product(
    u: &NurbsCurve3,
    v: &NurbsCurve3,
    map: impl Fn([f64; 3], [f64; 3]) -> [f64; 3],
) -> Option<NurbsSurface3> {
    let net = u
        .control_points()
        .iter()
        .map(|&p| v.control_points().iter().map(|&q| map(p, q)).collect())
        .collect();
    let weights = u
        .weights()
        .iter()
        .map(|&a| v.weights().iter().map(|&b| a * b).collect())
        .collect();
    NurbsSurface3::new_strict(
        u.degree(),
        v.degree(),
        net,
        u.knots().to_vec(),
        v.knots().to_vec(),
        weights,
    )
}

/// The exact NURBS form of `surface` over `u` and `v`, its own parameter
/// ranges (angles in radians). Its parameters are the forms': a plane's,
/// cylinder's and cone's `v` and a plane's `u` are kept, an angle becomes its
/// [`quarter_parameter`]. A spline is returned as it is.
pub fn nurbs_form(surface: &Surface, u: [f64; 2], v: [f64; 2]) -> Option<NurbsSurface3> {
    let up = |plane: &ocs_geom::space::Plane| plane.normal().map(Vec3::from);
    match surface {
        Surface::Plane(plane) => {
            let along_u = line([u[0], 0.0, 0.0], [u[1], 0.0, 0.0], u)?;
            let along_v = line([v[0], 0.0, 0.0], [v[1], 0.0, 0.0], v)?;
            product(&along_u, &along_v, |a, b| plane.point_at([a[0], b[0]]))
        }
        Surface::Cylinder(cylinder) => {
            let normal = up(&cylinder.base)?;
            let r = cylinder.radius;
            product(&unit_arc(u)?, &line([v[0], 0.0, 0.0], [v[1], 0.0, 0.0], v)?, |a, b| {
                (Vec3::from(cylinder.base.point_at([r * a[0], r * a[1]])) + normal * b[0])
                    .to_array()
            })
        }
        Surface::Cone(cone) => {
            let normal = up(&cone.base)?;
            let slope = cone.half_angle.tan();
            product(&unit_arc(u)?, &line([v[0], 0.0, 0.0], [v[1], 0.0, 0.0], v)?, |a, b| {
                let r = cone.radius - b[0] * slope;
                (Vec3::from(cone.base.point_at([r * a[0], r * a[1]])) + normal * b[0]).to_array()
            })
        }
        Surface::Sphere(sphere) => {
            let normal = up(&sphere.frame)?;
            let r = sphere.radius;
            product(&unit_arc(u)?, &unit_arc(v)?, |a, b| {
                (Vec3::from(sphere.frame.point_at([r * b[0] * a[0], r * b[0] * a[1]]))
                    + normal * (r * b[1]))
                    .to_array()
            })
        }
        Surface::Torus(torus) => {
            let normal = up(&torus.frame)?;
            let (big, small) = (torus.major_radius, torus.minor_radius);
            product(&unit_arc(u)?, &unit_arc(v)?, |a, b| {
                let ring = big + small * b[0];
                (Vec3::from(torus.frame.point_at([ring * a[0], ring * a[1]]))
                    + normal * (small * b[1]))
                    .to_array()
            })
        }
        Surface::Nurbs(spline) | Surface::Offset(OffsetSurface { approximation: spline, .. }) => {
            Some(spline.clone())
        }
    }
}

/// The exact NURBS form of `curve` over `t`, its own parameter range. A
/// line keeps its parameter, an angle becomes its [`quarter_parameter`], a
/// planar spline's `0..=1` becomes its knot parameter, and a spline in space
/// keeps its knots.
pub fn nurbs_curve_form(curve: &Curve3, t: [f64; 2]) -> Option<NurbsCurve3> {
    let lift = |arc: NurbsCurve3, map: &dyn Fn([f64; 3]) -> [f64; 3]| {
        let points = arc.control_points().iter().map(|&p| map(p)).collect();
        NurbsCurve3::new_strict(2, points, arc.knots().to_vec(), arc.weights().to_vec())
    };
    match curve {
        Curve3::Line(_) => line(curve.point_at(t[0]), curve.point_at(t[1]), t),
        Curve3::Circle(circle) => lift(unit_arc(t)?, &|p| {
            circle.plane.point_at([circle.radius * p[0], circle.radius * p[1]])
        }),
        Curve3::Ellipse(ellipse) => lift(unit_arc(t)?, &|p| {
            ellipse
                .plane
                .point_at([ellipse.major_radius * p[0], ellipse.minor_radius * p[1]])
        }),
        Curve3::PlanarSpline { plane, curve } => {
            let points = curve
                .control_points()
                .iter()
                .map(|&p| plane.point_at(p))
                .collect();
            let lifted = NurbsCurve3::new_strict(
                curve.degree(),
                points,
                curve.knots().to_vec(),
                curve.weights().to_vec(),
            )?;
            let (start, end) = lifted.domain();
            let at = |s: f64| start + s.clamp(0.0, 1.0) * (end - start);
            lifted.segment(at(t[0].min(t[1])), at(t[0].max(t[1])))
        }
        Curve3::Nurbs(spline) => spline.segment(t[0].min(t[1]), t[0].max(t[1])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Cone, Cylinder, Sphere, Torus};
    use ocs_geom::space::Plane;

    fn frame() -> Plane {
        Plane {
            origin: [1.0, -2.0, 0.5],
            x_axis: [0.0, 1.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        }
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        Vec3::from(a).distance(Vec3::from(b)) < 1e-12
    }

    /// Every analytic surface's form lands where the surface does, at the
    /// mapped parameters, across quarters and off the grid.
    #[test]
    fn analytic_forms_are_exact() {
        let surfaces = [
            Surface::Cylinder(Cylinder { base: frame(), radius: 2.0 }),
            Surface::Cone(Cone { base: frame(), radius: 2.0, half_angle: 0.3 }),
            Surface::Sphere(Sphere { frame: frame(), radius: 3.0 }),
            Surface::Torus(Torus { frame: frame(), major_radius: 5.0, minor_radius: 1.5 }),
            Surface::Plane(frame()),
        ];
        for surface in &surfaces {
            let (u, v) = match surface {
                Surface::Sphere(_) => ([-0.4, 4.0], [-1.2, 0.9]),
                Surface::Torus(_) => ([0.3, 6.0], [-3.0, 2.0]),
                _ => ([-0.4, 4.0], [-1.0, 2.5]),
            };
            let form = nurbs_form(surface, u, v).unwrap();
            let angular_v = matches!(surface, Surface::Sphere(_) | Surface::Torus(_));
            let angular_u = !matches!(surface, Surface::Plane(_));
            for i in 0..=6 {
                for j in 0..=6 {
                    let a = u[0] + (u[1] - u[0]) * i as f64 / 6.0;
                    let b = v[0] + (v[1] - v[0]) * j as f64 / 6.0;
                    let s = if angular_u { quarter_parameter(a) } else { a };
                    let t = if angular_v { quarter_parameter(b) } else { b };
                    assert!(
                        close(form.point_at_knot(s, t), surface.point_at(a, b)),
                        "{surface:?} at {a} {b}"
                    );
                }
            }
        }
    }

    /// A full circle's form is closed, of length 2πr, and puts each angle
    /// where the circle does.
    #[test]
    fn circle_form_runs_on_the_quarter_grid() {
        let circle = Curve3::Circle(crate::geometry::Circle3 { plane: frame(), radius: 2.0 });
        let form = nurbs_curve_form(&circle, [0.0, std::f64::consts::TAU]).unwrap();
        assert_eq!(form.domain(), (0.0, 4.0));
        for k in 0..=40 {
            let angle = std::f64::consts::TAU * k as f64 / 40.0;
            assert!(close(form.point_at_knot(quarter_parameter(angle)), circle.point_at(angle)));
        }
        let ellipse = Curve3::Ellipse(crate::geometry::Ellipse3 {
            plane: frame(),
            major_radius: 3.0,
            minor_radius: 1.0,
        });
        let form = nurbs_curve_form(&ellipse, [0.5, 2.0]).unwrap();
        for k in 0..=10 {
            let angle = 0.5 + 1.5 * k as f64 / 10.0;
            assert!(close(form.point_at_knot(quarter_parameter(angle)), ellipse.point_at(angle)));
        }
    }
}

#[cfg(test)]
mod curvature_tests {
    use super::*;
    use crate::geometry::{Sphere, Torus};
    use ocs_geom::space::Plane;

    /// The curvatures of the exact forms are the analytic surfaces': a
    /// sphere of radius 3 has K = 1/9 and |H| = 1/3; a torus's outer equator
    /// has principal curvatures 1/r and 1/(R + r).
    #[test]
    fn exact_forms_have_the_analytic_curvatures() {
        let frame = Plane { origin: [0.0; 3], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] };
        let sphere = nurbs_form(&Surface::Sphere(Sphere { frame, radius: 3.0 }), [0.0, 6.0], [-1.0, 1.0]).unwrap();
        for (a, b) in [(0.4, 0.2), (2.0, -0.7), (5.0, 0.9)] {
            let c = sphere.curvature_at(quarter_parameter(a), quarter_parameter(b)).unwrap();
            assert!((c.gaussian - 1.0 / 9.0).abs() < 1e-10, "{}", c.gaussian);
            assert!((c.mean.abs() - 1.0 / 3.0).abs() < 1e-10, "{}", c.mean);
        }
        let torus = Surface::Torus(Torus { frame, major_radius: 5.0, minor_radius: 1.0 });
        let form = nurbs_form(&torus, [0.0, 6.0], [-3.0, 3.0]).unwrap();
        let c = form.curvature_at(quarter_parameter(1.0), quarter_parameter(0.0)).unwrap();
        let mut principal = [c.principal[0].abs(), c.principal[1].abs()];
        principal.sort_by(f64::total_cmp);
        assert!((principal[0] - 1.0 / 6.0).abs() < 1e-10, "{principal:?}");
        assert!((principal[1] - 1.0).abs() < 1e-10, "{principal:?}");
    }
}
