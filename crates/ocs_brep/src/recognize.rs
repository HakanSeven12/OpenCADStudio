//! Spline geometry that is exactly a simpler shape, recognised as that shape.
//!
//! Exchange formats routinely write every curve and surface as a NURBS — a
//! box's sides as bilinear patches, a hole as a rational quadratic — and the
//! analytic forms carry what the spline only implies: a plane's normal, a
//! circle's centre, a sphere's poles. Recognising them gives a body that
//! meshes, measures and edits as if it had been modelled here.
//!
//! Recognition is by measurement, not by inspecting the control net's
//! layout: the spline is sampled and the candidate shape kept only when every
//! sample lies on it within a tolerance relative to the spline's own size.
//! A spline that only approximates the shape is left alone.

use super::geometry::{Circle3, Cone, Curve3, Cylinder, Line3, Sphere, Surface, Torus};
use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};

/// Samples along each parameter direction.
const SAMPLES: usize = 9;

/// The tolerance a shape must fit within: a ten-millionth of the extent of
/// the points it is fitted to.
fn tolerance(points: &[Vec3]) -> f64 {
    let mut low = Vec3::from([f64::INFINITY; 3]);
    let mut high = Vec3::from([f64::NEG_INFINITY; 3]);
    for p in points {
        low = Vec3::from([low.x.min(p.x), low.y.min(p.y), low.z.min(p.z)]);
        high = Vec3::from([high.x.max(p.x), high.y.max(p.y), high.z.max(p.z)]);
    }
    1e-7 * (high - low).length().max(1e-3)
}

fn steps(from: f64, to: f64) -> impl Iterator<Item = f64> {
    (0..SAMPLES).map(move |k| from + (to - from) * k as f64 / (SAMPLES - 1) as f64)
}

/// Solves the 3×3 system `m·x = b`.
fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-300 {
        return None;
    }
    let mut x = [0.0; 3];
    for (column, value) in x.iter_mut().enumerate() {
        let mut replaced = m;
        for row in 0..3 {
            replaced[row][column] = b[row];
        }
        *value = det(replaced) / d;
    }
    Some(x)
}

/// The centre of the circle through three points.
fn circumcentre(a: Vec3, b: Vec3, c: Vec3) -> Option<Vec3> {
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    let squared = normal.length_squared();
    if squared < 1e-300 {
        return None;
    }
    let offset = (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
        / (2.0 * squared);
    Some(a + offset)
}

/// A spline curve that is exactly a straight line or a circular arc, as one.
pub fn analytic_curve(curve: &NurbsCurve3) -> Option<Curve3> {
    let (from, to) = curve.domain();
    let samples: Vec<Vec3> = steps(from, to)
        .map(|t| Vec3::from(curve.point_at_knot(t)))
        .collect();
    let control: Vec<Vec3> = curve.control_points().iter().copied().map(Vec3::from).collect();
    let tolerance = tolerance(&control);
    let (first, last) = (samples[0], samples[SAMPLES - 1]);

    // A line: every control point on the chord.
    if let Some(direction) = (last - first).normalize() {
        let off = |p: &Vec3| {
            let d = *p - first;
            (d - direction * d.dot(direction)).length()
        };
        if control.iter().all(|p| off(p) <= tolerance) {
            return Some(Curve3::Line(Line3 {
                origin: first.to_array(),
                direction: (last - first).to_array(),
            }));
        }
    }

    // A circle: a centre all samples keep their distance from, in one
    // plane. A closed curve's ends coincide, so the third point is taken a
    // third of the way along instead.
    let middle = Vec3::from(curve.point_at_knot(from + (to - from) / 3.0));
    let other = Vec3::from(curve.point_at_knot(from + 2.0 * (to - from) / 3.0));
    let centre = circumcentre(first, middle, other)?;
    let radius = (first - centre).length();
    let normal = (first - centre).cross(middle - centre).normalize()?;
    let on_circle = |p: &Vec3| {
        ((*p - centre).length() - radius).abs() <= tolerance
            && (*p - centre).dot(normal).abs() <= tolerance
    };
    if !samples.iter().all(on_circle) {
        return None;
    }
    // Angles run the way the curve does.
    let tangent = Vec3::from(curve.tangent_at_knot(from + (to - from) * 0.01));
    let normal = if (first - centre).cross(tangent).dot(normal) < 0.0 { -normal } else { normal };
    let plane = Plane::orthonormal(centre.to_array(), (first - centre).to_array(), normal.to_array())?;
    Some(Curve3::Circle(Circle3 { plane, radius }))
}

/// A spline surface that is exactly a plane, a sphere, a cylinder, a cone or
/// a torus, as one, with whether the analytic surface's normal agrees with
/// the spline's.
pub fn analytic_surface(surface: &NurbsSurface3) -> Option<(Surface, bool)> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let point = |u: f64, v: f64| Vec3::from(surface.point_at_knot(u, v));
    let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
    // Ends where the surface pinches to a point: a sphere's poles, a cone's
    // apex. The normal is undefined there and is not sampled.
    let pinched = |a: Vec3, b: Vec3| (a - b).length() <= 1e-9 * (1.0 + a.length());
    let [pinch_u0, pinch_u1] = [u0, u1].map(|u| pinched(point(u, v0), point(u, vm)));
    let [pinch_v0, pinch_v1] = [v0, v1].map(|v| pinched(point(u0, v), point(um, v)));
    let mut points = Vec::new();
    let mut normals = Vec::new();
    for u in steps(u0, u1) {
        for v in steps(v0, v1) {
            let p = point(u, v);
            points.push(p);
            let at_pinch = (pinch_u0 && u == u0)
                || (pinch_u1 && u == u1)
                || (pinch_v0 && v == v0)
                || (pinch_v1 && v == v1);
            if let Some(n) = surface.normal_at_knot(u, v).filter(|_| !at_pinch) {
                normals.push((p, Vec3::from(n)));
            }
        }
    }
    let control: Vec<Vec3> = surface
        .control_points()
        .iter()
        .flatten()
        .copied()
        .map(Vec3::from)
        .collect();
    let tolerance = tolerance(&control);
    // A normal where the surface is regular, to tell the sense by.
    let (at, normal) = *normals.get(normals.len() / 2)?;
    // Where a closed surface meets itself, and where it pinches to a pole:
    // the analytic frame starts its angle at the seam and runs its axis
    // through the poles, as this kernel's own spheres and cylinders do, so
    // the face's seam edge falls where the surface's parameters wrap.
    let (pinched_u, pinched_v) = (pinch_u0 || pinch_u1, pinch_v0 || pinch_v1);
    let [closed_u, closed_v] = surface.periodicity();
    let seam = if closed_v || pinched_u {
        point(um, v0)
    } else if closed_u || pinched_v {
        point(u0, vm)
    } else {
        at
    };
    let poles = if pinch_u0 && pinch_u1 {
        Some(point(u1, v0) - point(u0, v0))
    } else if pinch_v0 && pinch_v1 {
        Some(point(u0, v1) - point(u0, v0))
    } else {
        None
    };

    // A plane: every control point on the tangent plane at the middle.
    if control.iter().all(|p| (*p - at).dot(normal).abs() <= tolerance) {
        let helper = if normal.x.abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let plane = Plane::orthonormal(at.to_array(), helper, normal.to_array())?;
        return Some((Surface::Plane(plane), true));
    }

    // A sphere: x² + y² + z² = 2c·p + k, fitted by least squares.
    let mut m = [[0.0; 4]; 4];
    let mut b = [0.0; 4];
    for p in &points {
        let row = [2.0 * p.x, 2.0 * p.y, 2.0 * p.z, 1.0];
        let rhs = p.length_squared();
        for i in 0..4 {
            for j in 0..4 {
                m[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * rhs;
        }
    }
    if let Some(centre) = solve4(m, b) {
        let centre = Vec3::from([centre[0], centre[1], centre[2]]);
        let radius = (points[0] - centre).length();
        if radius > tolerance
            && points
                .iter()
                .all(|p| ((*p - centre).length() - radius).abs() <= tolerance)
        {
            let axis = poles.and_then(Vec3::normalize).unwrap_or(Vec3::from([0.0, 0.0, 1.0]));
            let frame = Plane::orthonormal(centre.to_array(), (seam - centre).to_array(), axis.to_array())
                .or_else(|| Plane::orthonormal(centre.to_array(), [1.0, 0.0, 0.0], axis.to_array()))
                .or_else(|| Plane::orthonormal(centre.to_array(), [0.0, 1.0, 0.0], axis.to_array()))?;
            let agrees = (at - centre).dot(normal) > 0.0;
            return Some((Surface::Sphere(Sphere { frame, radius }), agrees));
        }
    }

    let corner = point(u0, v0);
    if let Some(found) = axial(&points, &normals, at, normal, seam, tolerance) {
        return Some(found);
    }
    toroidal(&points, &normals, at, normal, corner, tolerance)
}

/// A cylinder or a cone: every normal at one angle to an axis, every normal
/// line through it, and the distance from it changing linearly along it —
/// not at all for a cylinder.
fn axial(
    points: &[Vec3],
    normals: &[(Vec3, Vec3)],
    at: Vec3,
    normal: Vec3,
    seam: Vec3,
    tolerance: f64,
) -> Option<(Surface, bool)> {
    let differences: Vec<Vec3> = normals.iter().map(|(_, n)| *n - normals[0].1).collect();
    let longest = *differences
        .iter()
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?;
    let axis = differences
        .iter()
        .map(|d| longest.cross(*d))
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))?
        .normalize()?;
    if differences.iter().any(|d| d.dot(axis).abs() > 1e-7) {
        return None;
    }
    // Where the normal lines meet the axis, in a plane square to it.
    let section = Plane::orthonormal([0.0; 3], normal.cross(axis).to_array(), axis.to_array())?;
    let mut m = [[0.0; 2]; 2];
    let mut b = [0.0; 2];
    for (p, n) in normals {
        let (Some([px, py]), Some([tx, ty])) = (section.project(p.to_array()), section.project_vector(n.to_array()))
        else {
            continue;
        };
        let length = tx.hypot(ty);
        if length < 1e-9 {
            continue;
        }
        let (tx, ty) = (tx / length, ty / length);
        let across = [[1.0 - tx * tx, -tx * ty], [-tx * ty, 1.0 - ty * ty]];
        for i in 0..2 {
            for j in 0..2 {
                m[i][j] += across[i][j];
            }
            b[i] += across[i][0] * px + across[i][1] * py;
        }
    }
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det.abs() < 1e-12 {
        return None;
    }
    let qx = (b[0] * m[1][1] - b[1] * m[0][1]) / det;
    let qy = (m[0][0] * b[1] - m[1][0] * b[0]) / det;
    let on_axis = Vec3::from(section.point_at([qx, qy]));
    // Distance from the axis against position along it: r = r0 + k·h.
    let measured: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            let h = (*p - on_axis).dot(axis);
            (h, ((*p - on_axis) - axis * h).length())
        })
        .collect();
    let count = measured.len() as f64;
    let (mean_h, mean_r) = (
        measured.iter().map(|(h, _)| h).sum::<f64>() / count,
        measured.iter().map(|(_, r)| r).sum::<f64>() / count,
    );
    let spread: f64 = measured.iter().map(|(h, _)| (h - mean_h).powi(2)).sum();
    let slope = if spread > 0.0 {
        measured
            .iter()
            .map(|(h, r)| (h - mean_h) * (r - mean_r))
            .sum::<f64>()
            / spread
    } else {
        0.0
    };
    let cylinder = slope.abs() <= 1e-9;
    let slope = if cylinder { 0.0 } else { slope };
    if mean_r <= tolerance
        || measured
            .iter()
            .any(|(h, r)| (mean_r + slope * (h - mean_h) - r).abs() > tolerance)
    {
        return None;
    }
    let centre = on_axis + axis * mean_h;
    let base = Plane::orthonormal(centre.to_array(), (seam - centre).to_array(), axis.to_array())?;
    let radial = (at - on_axis) - axis * (at - on_axis).dot(axis);
    let agrees = radial.dot(normal) > 0.0;
    Some((
        if cylinder {
            Surface::Cylinder(Cylinder {
                base,
                radius: mean_r,
            })
        } else {
            // The kernel's half-angle is positive where the cone narrows
            // along its axis.
            Surface::Cone(Cone {
                base,
                radius: mean_r,
                half_angle: -slope.atan(),
            })
        },
        agrees,
    ))
}

/// A torus: every normal line through one axis, and the points, seen in a
/// plane through that axis, on one circle off it.
fn toroidal(
    points: &[Vec3],
    normals: &[(Vec3, Vec3)],
    at: Vec3,
    normal: Vec3,
    corner: Vec3,
    tolerance: f64,
) -> Option<(Surface, bool)> {
    // The axis as the line every normal line meets: in Plücker coordinates
    // (d, m) it has d·mᵢ + m·dᵢ = 0 for each normal line (dᵢ, mᵢ), so it is
    // the direction that system least resists.
    // Taken about the points' middle, so the moments stay the size of the
    // directions.
    let middle = points.iter().fold(Vec3::from([0.0; 3]), |sum, p| sum + *p) / points.len() as f64;
    let mut a = [[0.0; 6]; 6];
    for (p, n) in normals {
        let m = (*p - middle).cross(*n);
        let row = [m.x, m.y, m.z, n.x, n.y, n.z];
        for i in 0..6 {
            for j in 0..6 {
                a[i][j] += row[i] * row[j];
            }
        }
    }
    let line = least_eigenvector6(a);
    let d = Vec3::from([line[0], line[1], line[2]]);
    let m = Vec3::from([line[3], line[4], line[5]]);
    let length = d.length();
    if length < 1e-9 {
        return None;
    }
    let axis = d / length;
    let on_axis = middle + d.cross(m) / (length * length);
    // The meridian: (distance from the axis, height along it) on a circle.
    let meridian: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            let h = (*p - on_axis).dot(axis);
            (((*p - on_axis) - axis * h).length(), h)
        })
        .collect();
    let mut mm = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for (r, h) in &meridian {
        let row = [2.0 * r, 2.0 * h, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                mm[i][j] += row[i] * row[j];
            }
            b[i] += row[i] * (r * r + h * h);
        }
    }
    let [major, height, _] = solve3(mm, b)?;
    let (r0, h0) = meridian[0];
    let minor = (r0 - major).hypot(h0 - height);
    if major <= tolerance
        || minor <= tolerance
        || meridian
            .iter()
            .any(|(r, h)| ((r - major).hypot(h - height) - minor).abs() > tolerance)
    {
        return None;
    }
    let centre = on_axis + axis * height;
    // Angles start where the face's seams cross.
    let towards = (corner - centre) - axis * (corner - centre).dot(axis);
    let frame = Plane::orthonormal(centre.to_array(), towards.to_array(), axis.to_array())?;
    let radial = ((at - centre) - axis * (at - centre).dot(axis)).normalize()?;
    let tube = centre + radial * major;
    let agrees = (at - tube).dot(normal) > 0.0;
    Some((
        Surface::Torus(Torus {
            frame,
            major_radius: major,
            minor_radius: minor,
        }),
        agrees,
    ))
}

/// The unit eigenvector of a symmetric 6×6 matrix with the least
/// eigenvalue, by Jacobi rotations.
fn least_eigenvector6(mut a: [[f64; 6]; 6]) -> [f64; 6] {
    let mut v = [[0.0; 6]; 6];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..100 {
        let mut off = 0.0;
        for i in 0..6 {
            for j in i + 1..6 {
                off += a[i][j] * a[i][j];
            }
        }
        if off < 1e-30 {
            break;
        }
        for p in 0..6 {
            for q in p + 1..6 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..6 {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..6 {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for k in 0..6 {
                    let (vkp, vkq) = (v[k][p], v[k][q]);
                    v[k][p] = c * vkp - s * vkq;
                    v[k][q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let least = (0..6)
        .min_by(|&i, &j| a[i][i].total_cmp(&a[j][j]))
        .unwrap_or(0);
    [0, 1, 2, 3, 4, 5].map(|k| v[k][least])
}

/// Solves the 4×4 system `m·x = b` by elimination with partial pivoting.
fn solve4(mut m: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for column in 0..4 {
        let pivot = (column..4).max_by(|&i, &j| m[i][column].abs().total_cmp(&m[j][column].abs()))?;
        if m[pivot][column].abs() < 1e-300 {
            return None;
        }
        m.swap(column, pivot);
        b.swap(column, pivot);
        for row in column + 1..4 {
            let factor = m[row][column] / m[column][column];
            for k in column..4 {
                m[row][k] -= factor * m[column][k];
            }
            b[row] -= factor * b[column];
        }
    }
    let mut x = [0.0; 4];
    for row in (0..4).rev() {
        let sum: f64 = (row + 1..4).map(|k| m[row][k] * x[k]).sum();
        x[row] = (b[row] - sum) / m[row][row];
    }
    Some(x)
}

