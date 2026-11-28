//! Pure solver for deferred TAN/PER line ends.
//!
//! No `Scene`, document or UI dependency: every input is a world-space point
//! plus an exact curve description, and every result is a pair of world-space
//! points. Geometry is solved in the curve's own plane (or directly in 3D for
//! straight targets); see `docs/deferred-precision-input-design.md`.

use glam::DVec3;
use std::f64::consts::TAU;

const EPS: f64 = 1e-9;
const SAMPLES: usize = 96;
const BISECTIONS: usize = 48;

/// Which constraint a deferred pick places on the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeferredMode {
    /// The line touches the curve without crossing it.
    Tangent,
    /// The line runs along the curve's normal at the contact point.
    Perpendicular,
}

/// Exact curve description carried by a deferred pick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeferredCurve {
    /// Infinite line through two world-space points.
    Line { p1: DVec3, p2: DVec3 },
    /// Circle in the plane spanned by `axis_x` and `axis_y`.
    Circle {
        center: DVec3,
        axis_x: DVec3,
        axis_y: DVec3,
        radius: f64,
    },
    /// Bounded circular arc. Angles are measured from `axis_x` CCW to `axis_y`.
    Arc {
        center: DVec3,
        axis_x: DVec3,
        axis_y: DVec3,
        radius: f64,
        start: f64,
        end: f64,
    },
    /// Ellipse (or elliptical arc) in the plane of `major_axis` and `normal`.
    Ellipse {
        center: DVec3,
        major_axis: DVec3,
        normal: DVec3,
        minor_axis_ratio: f64,
        start: f64,
        end: f64,
    },
}

/// A TAN/PER pick made before the other end of the line was known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeferredPick {
    pub mode: DeferredMode,
    pub curve: DeferredCurve,
    /// Nearest point on the picked curve under the cursor; a solution hint.
    pub at: DVec3,
}

/// Compatibility view of a deferred curve for commands that only understand
/// the older `TangentObject` handles.
pub fn tangent_object_from_curve(
    curve: &DeferredCurve,
    _at: DVec3,
) -> Option<super::TangentObject> {
    match *curve {
        DeferredCurve::Line { p1, p2 } => Some(super::TangentObject::Line { p1, p2 }),
        DeferredCurve::Circle { center, radius, .. }
        | DeferredCurve::Arc { center, radius, .. } => {
            Some(super::TangentObject::Circle { center, radius })
        }
        DeferredCurve::Ellipse {
            center,
            major_axis,
            normal,
            minor_axis_ratio,
            ..
        } => Some(super::TangentObject::Ellipse {
            center,
            major_axis,
            normal,
            minor_axis_ratio,
        }),
    }
}

/// One end of a line being solved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineEnd {
    Point(DVec3),
    Deferred(DeferredPick),
}

/// A world-space orthonormal frame for a planar curve.
#[derive(Clone, Copy)]
struct Plane {
    origin: DVec3,
    u: DVec3,
    v: DVec3,
}

impl Plane {
    fn project(self, p: DVec3) -> [f64; 2] {
        let d = p - self.origin;
        [d.dot(self.u), d.dot(self.v)]
    }

    fn lift(self, q: [f64; 2]) -> DVec3 {
        self.origin + self.u * q[0] + self.v * q[1]
    }
}

fn finite(p: DVec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

fn ok_line(p: DVec3, q: DVec3) -> bool {
    finite(p) && finite(q) && (p - q).length_squared() > EPS * EPS
}

/// Curved kinds share one plane; a straight target has none of its own.
fn curve_plane(curve: &DeferredCurve) -> Option<Plane> {
    let (center, u_raw, v_raw) = match *curve {
        DeferredCurve::Line { .. } => return None,
        DeferredCurve::Circle {
            center,
            axis_x,
            axis_y,
            ..
        }
        | DeferredCurve::Arc {
            center,
            axis_x,
            axis_y,
            ..
        } => (center, axis_x, axis_y),
        DeferredCurve::Ellipse {
            center,
            major_axis,
            normal,
            ..
        } => (center, major_axis, normal.cross(major_axis)),
    };
    let u = u_raw.normalize_or_zero();
    if u.length_squared() < 0.5 {
        return None;
    }
    let n = u.cross(v_raw).normalize_or_zero();
    if n.length_squared() < 0.5 {
        return None;
    }
    let v = n.cross(u).normalize_or_zero();
    (v.length_squared() > 0.5).then_some(Plane {
        origin: center,
        u,
        v,
    })
}

/// Semi-axes `(a, b)` of the curve in its own plane, with the centre removed.
fn curve_axes(curve: &DeferredCurve) -> Option<(f64, f64)> {
    match *curve {
        DeferredCurve::Line { .. } => None,
        DeferredCurve::Circle { radius, .. } | DeferredCurve::Arc { radius, .. } => {
            let r = radius.abs();
            (r > EPS).then_some((r, r))
        }
        DeferredCurve::Ellipse {
            major_axis,
            minor_axis_ratio,
            ..
        } => {
            let a = major_axis.length();
            let b = a * minor_axis_ratio.abs();
            (a > EPS && b > EPS).then_some((a, b))
        }
    }
}

/// Whether a solved point lies on the drawn span (arcs may exclude solutions
/// that sit on their full circle/ellipse but off the arc itself).
fn on_curve(curve: &DeferredCurve, plane: Plane, q: [f64; 2], _p: DVec3) -> bool {
    let center = circle_center(curve).unwrap_or(plane.origin);
    match *curve {
        DeferredCurve::Arc { start, end, .. } => {
            let c = plane.project(center);
            let angle = (q[1] - c[1]).atan2(q[0] - c[0]);
            angle_on_arc(angle, start, end)
        }
        DeferredCurve::Ellipse { start, end, .. } => {
            let span = (end - start).abs();
            if span < 1e-9 || span >= TAU - 1e-9 {
                return true;
            }
            let a = plane.project(center);
            let (ra, rb) = match curve_axes(curve) {
                Some(v) => v,
                None => return false,
            };
            let t = ((q[0] - a[0]) / ra).atan2((q[1] - a[1]) / rb);
            angle_on_arc(t, start, end)
        }
        _ => true,
    }
}

fn angle_on_arc(t: f64, start: f64, end: f64) -> bool {
    let span = (end - start).abs();
    if span < 1e-9 || span >= TAU - 1e-9 {
        // A zero sweep is how a full circle is stored.
        return true;
    }
    // Sweep CCW from `start` to `end`; a clockwise arc has end < start.
    let sweep = (end - start).rem_euclid(TAU);
    let along = (t - start).rem_euclid(TAU);
    along <= sweep + 1e-6 || along >= TAU - 1e-6
}

fn line_direction(curve: &DeferredCurve) -> Option<DVec3> {
    match *curve {
        DeferredCurve::Line { p1, p2 } => {
            let d = p2 - p1;
            (d.length_squared() > EPS * EPS).then(|| d.normalize())
        }
        _ => None,
    }
}

fn line_foot(curve: &DeferredCurve, p: DVec3) -> Option<DVec3> {
    match *curve {
        DeferredCurve::Line { p1, .. } => {
            let d = line_direction(curve)?;
            Some(p1 + d * (p - p1).dot(d))
        }
        _ => None,
    }
}

fn circle_center(curve: &DeferredCurve) -> Option<DVec3> {
    match *curve {
        DeferredCurve::Circle { center, .. }
        | DeferredCurve::Arc { center, .. }
        | DeferredCurve::Ellipse { center, .. } => Some(center),
        DeferredCurve::Line { .. } => None,
    }
}

/// Points on `curve` from which the line to `p` is tangent.
pub fn tangent_candidates(curve: &DeferredCurve, p: DVec3) -> Vec<DVec3> {
    let Some(plane) = curve_plane(curve) else {
        return Vec::new(); // a line has no tangent point to draw from
    };
    let Some((a, b)) = curve_axes(curve) else {
        return Vec::new();
    };
    let c = plane.project(circle_center(curve).unwrap_or(plane.origin));
    let target = plane.project(p);
    let (x, y) = (target[0] - c[0], target[1] - c[1]);

    let params: Vec<f64> = if matches!(
        curve,
        DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
    ) {
        let d = (x * x + y * y).sqrt();
        if d <= a + EPS {
            return Vec::new();
        }
        let base = y.atan2(x);
        let off = (a / d).acos();
        vec![base + off, base - off]
    } else {
        // cross((p-q), tangent(t)) = 0  ->  b*x*cos t + a*y*sin t = a*b
        let r = ((b * x).powi(2) + (a * y).powi(2)).sqrt();
        if r <= a * b + EPS {
            return Vec::new();
        }
        let base = (a * y).atan2(b * x);
        let off = (a * b / r).acos();
        vec![base + off, base - off]
    };

    params
        .into_iter()
        .map(|t| {
            let q = if matches!(
                curve,
                DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
            ) {
                [c[0] + a * t.cos(), c[1] + a * t.sin()]
            } else {
                [c[0] + a * t.cos(), c[1] + b * t.sin()]
            };
            let world = plane.lift(q);
            on_curve(curve, plane, q, world).then_some(world)
        })
        .flatten()
        .collect()
}

/// Perpendicular feet on `curve` seen from `p`. The picked point is used when
/// `p` is the centre of a circle, where every radius is perpendicular.
pub fn perpendicular_candidates(curve: &DeferredCurve, p: DVec3, hint: DVec3) -> Vec<DVec3> {
    if let Some(foot) = line_foot(curve, p) {
        return vec![foot];
    }
    let Some(plane) = curve_plane(curve) else {
        return Vec::new();
    };
    let Some((a, b)) = curve_axes(curve) else {
        return Vec::new();
    };
    let c = plane.project(circle_center(curve).unwrap_or(plane.origin));
    let target = plane.project(p);
    let picked = plane.project(hint);

    let params: Vec<f64> = if matches!(
        curve,
        DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
    ) {
        let (ux, uy) = (target[0] - c[0], target[1] - c[1]);
        let len = (ux * ux + uy * uy).sqrt();
        if len <= EPS {
            let (hx, hy) = (picked[0] - c[0], picked[1] - c[1]);
            let hl = (hx * hx + hy * hy).sqrt();
            if hl <= EPS {
                return Vec::new();
            }
            let t = (hy / hl).atan2(hx / hl);
            vec![t, t + std::f64::consts::PI]
        } else {
            let t = (uy / len).atan2(ux / len);
            vec![t, t + std::f64::consts::PI]
        }
    } else {
        // stationary distance: (a²-b²) sin t cos t - a*x sin t + b*y cos t = 0
        let (x, y) = (target[0] - c[0], target[1] - c[1]);
        let f = |t: f64| (a * a - b * b) * t.sin() * t.cos() - a * x * t.sin() + b * y * t.cos();
        let mut roots = Vec::new();
        let mut prev = f(0.0);
        if prev.abs() < 1e-7 {
            roots.push(0.0);
        }
        for i in 1..=SAMPLES {
            let t = TAU * i as f64 / SAMPLES as f64;
            let cur = f(t);
            if (prev < 0.0) != (cur < 0.0) {
                let (mut lo, mut hi) = (TAU * (i - 1) as f64 / SAMPLES as f64, t);
                let mut f_lo = prev;
                for _ in 0..BISECTIONS {
                    let mid = 0.5 * (lo + hi);
                    let fm = f(mid);
                    if (fm < 0.0) == (f_lo < 0.0) {
                        lo = mid;
                        f_lo = fm;
                    } else {
                        hi = mid;
                    }
                }
                roots.push(0.5 * (lo + hi));
            }
            prev = cur;
        }
        roots
    };

    params
        .into_iter()
        .map(|t| {
            let q = if matches!(
                curve,
                DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
            ) {
                [c[0] + a * t.cos(), c[1] + a * t.sin()]
            } else {
                [c[0] + a * t.cos(), c[1] + b * t.sin()]
            };
            let world = plane.lift(q);
            on_curve(curve, plane, q, world).then_some(world)
        })
        .flatten()
        .collect()
}

fn nearest_to_hint(candidates: Vec<DVec3>, hint: DVec3) -> Option<DVec3> {
    candidates
        .into_iter()
        .filter(|p| finite(*p))
        .min_by(|a, b| {
            let da = (*a - hint).length_squared();
            let db = (*b - hint).length_squared();
            da.total_cmp(&db)
        })
}

fn pick_best(
    candidates: impl Iterator<Item = (DVec3, DVec3)>,
    at1: DVec3,
    at2: Option<DVec3>,
) -> Option<(DVec3, DVec3)> {
    candidates
        .filter(|(p, q)| ok_line(*p, *q))
        .map(|(p, q)| {
            let cost = (p - at1).length_squared() + at2.map_or(0.0, |a| (q - a).length_squared());
            (cost, p, q)
        })
        .filter(|(cost, _, _)| cost.is_finite())
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p, q)| (p, q))
}

/// Solve a line whose ends may be deferred TAN/PER picks.
pub fn resolve_deferred_line(a: LineEnd, b: LineEnd) -> Option<(DVec3, DVec3)> {
    match (a, b) {
        (LineEnd::Point(p), LineEnd::Point(q)) => ok_line(p, q).then_some((p, q)),
        (LineEnd::Point(p), LineEnd::Deferred(d)) => {
            resolve_deferred_line(LineEnd::Deferred(d), LineEnd::Point(p)).map(|(q, p)| (p, q))
        }
        (LineEnd::Deferred(d), LineEnd::Point(p)) => {
            if !finite(p) {
                return None;
            }
            let solutions = match d.mode {
                DeferredMode::Tangent => tangent_candidates(&d.curve, p),
                DeferredMode::Perpendicular => perpendicular_candidates(&d.curve, p, d.at),
            };
            pick_best(solutions.into_iter().map(|q| (q, p)), d.at, None)
        }
        (LineEnd::Deferred(d1), LineEnd::Deferred(d2)) => {
            let pairs = deferred_pairs(&d1, &d2).or_else(|| {
                deferred_pairs(&d2, &d1).map(|v| v.into_iter().map(|(q, p)| (p, q)).collect())
            })?;
            pick_best(pairs.into_iter(), d1.at, Some(d2.at))
        }
    }
}

/// The point on the curve whose tangent/normal runs along `dir`, nearest `hint`.
/// Used when the free end of a deferred line is typed as `@distance<angle`.
pub fn point_in_direction(
    curve: &DeferredCurve,
    mode: DeferredMode,
    dir: DVec3,
    hint: DVec3,
) -> Option<DVec3> {
    let dir = dir.normalize_or_zero();
    if dir.length_squared() < 0.5 {
        return None;
    }
    if let Some(line_dir) = line_direction(curve) {
        return match mode {
            DeferredMode::Tangent => None,
            DeferredMode::Perpendicular => {
                if line_dir.dot(dir).abs() > 1e-6 {
                    return None;
                }
                line_foot(curve, hint)
            }
        };
    }
    let plane = curve_plane(curve)?;
    let (a, b) = curve_axes(curve)?;
    let c = plane.project(circle_center(curve)?);
    let d2 = plane.project(plane.origin + dir);
    let dl = (d2[0] * d2[0] + d2[1] * d2[1]).sqrt();
    if dl <= EPS {
        return None;
    }
    let (dx, dy) = (d2[0] / dl, d2[1] / dl);
    let is_circle = matches!(
        curve,
        DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
    );
    let params = if is_circle {
        match mode {
            DeferredMode::Tangent => {
                let n = dy.atan2(dx) + std::f64::consts::FRAC_PI_2;
                vec![n, n + std::f64::consts::PI]
            }
            DeferredMode::Perpendicular => {
                let n = dy.atan2(dx);
                vec![n, n + std::f64::consts::PI]
            }
        }
    } else {
        match mode {
            // tangent(t) = (-a sin t, b cos t) parallel to dir
            DeferredMode::Tangent => {
                let t = (-dx / a).atan2(dy / b);
                vec![t, t + std::f64::consts::PI]
            }
            // normal(t) = (b cos t, a sin t) parallel to dir
            DeferredMode::Perpendicular => {
                let t = (dy / a).atan2(dx / b);
                vec![t, t + std::f64::consts::PI]
            }
        }
    };
    let candidates: Vec<DVec3> = params
        .into_iter()
        .map(|t| {
            let q = if is_circle {
                [c[0] + a * t.cos(), c[1] + a * t.sin()]
            } else {
                [c[0] + a * t.cos(), c[1] + b * t.sin()]
            };
            let world = plane.lift(q);
            on_curve(curve, plane, q, world).then_some(world)
        })
        .flatten()
        .collect();
    nearest_to_hint(candidates, hint)
}

/// Support function of a circle/ellipse in plane direction `n` (unit).
fn support_radius((a, b): (f64, f64), n: [f64; 2]) -> f64 {
    ((a * n[0]).powi(2) + (b * n[1]).powi(2)).sqrt()
}

fn support_offset((a, b): (f64, f64), n: [f64; 2]) -> [f64; 2] {
    let r = support_radius((a, b), n);
    if r <= EPS {
        return [0.0, 0.0];
    }
    [a * a * n[0] / r, b * b * n[1] / r]
}

/// Common tangents of two curves, returned as (point on d1, point on d2).
fn common_tangents(d1: &DeferredPick, d2: &DeferredPick) -> Option<Vec<(DVec3, DVec3)>> {
    let plane1 = curve_plane(&d1.curve)?;
    let plane2 = curve_plane(&d2.curve)?;
    if plane1.u.dot(plane2.u).abs() < 1.0 - 1e-8 || plane1.v.dot(plane2.v).abs() < 1.0 - 1e-8 {
        return None;
    }
    let axes1 = curve_axes(&d1.curve)?;
    let axes2 = curve_axes(&d2.curve)?;
    let c1 = plane1.project(circle_center(&d1.curve)?);
    let c2 = plane1.project(circle_center(&d2.curve)?);
    let delta = [c2[0] - c1[0], c2[1] - c1[1]];
    if delta[0] * delta[0] + delta[1] * delta[1] <= EPS * EPS {
        return None;
    }

    let eval = |angle: f64, internal: bool| {
        let n = [angle.cos(), angle.sin()];
        let r1 = support_radius(axes1, n);
        let r2 = support_radius(axes2, n);
        let f = if internal {
            n[0] * delta[0] + n[1] * delta[1] - r2 - r1
        } else {
            n[0] * delta[0] + n[1] * delta[1] + r2 - r1
        };
        let o1 = support_offset(axes1, n);
        let o2 = support_offset(axes2, n);
        let t1 = [c1[0] + o1[0], c1[1] + o1[1]];
        let t2 = if internal {
            [c2[0] - o2[0], c2[1] - o2[1]]
        } else {
            [c2[0] + o2[0], c2[1] + o2[1]]
        };
        (f, t1, t2)
    };

    let mut out = Vec::new();
    for &internal in &[false, true] {
        let mut prev = eval(0.0, internal).0;
        if prev.abs() < 1e-10 {
            let (_, t1, t2) = eval(0.0, internal);
            out.push((plane1.lift(t1), plane1.lift(t2)));
        }
        for i in 1..=SAMPLES {
            let a0 = TAU * (i - 1) as f64 / SAMPLES as f64;
            let a1 = TAU * i as f64 / SAMPLES as f64;
            let cur = eval(a1, internal).0;
            if (prev < 0.0) != (cur < 0.0) {
                let (mut lo, mut hi) = (a0, a1);
                let mut f_lo = prev;
                for _ in 0..BISECTIONS {
                    let mid = 0.5 * (lo + hi);
                    let fm = eval(mid, internal).0;
                    if (fm < 0.0) == (f_lo < 0.0) {
                        lo = mid;
                        f_lo = fm;
                    } else {
                        hi = mid;
                    }
                }
                let (f, t1, t2) = eval(0.5 * (lo + hi), internal);
                if f.abs() < 1e-5 {
                    out.push((plane1.lift(t1), plane1.lift(t2)));
                }
            }
            prev = cur;
        }
    }
    let mut unique: Vec<(DVec3, DVec3)> = Vec::new();
    for pair in out {
        let p = pair.0;
        let q = pair.1;
        if !on_curve(&d1.curve, plane1, plane1.project(p), p)
            || !on_curve(&d2.curve, plane2, plane2.project(q), q)
        {
            continue;
        }
        if !unique
            .iter()
            .any(|(a, b)| (*a - p).length_squared() < 1e-8 && (*b - q).length_squared() < 1e-8)
        {
            unique.push(pair);
        }
    }
    Some(unique)
}

/// Points on a curved target whose normal runs along `dir`.
fn normal_points(curve: &DeferredCurve, dir: DVec3) -> Vec<DVec3> {
    let Some(plane) = curve_plane(curve) else {
        return Vec::new();
    };
    let Some((a, b)) = curve_axes(curve) else {
        return Vec::new();
    };
    let Some(center) = circle_center(curve) else {
        return Vec::new();
    };
    let c = plane.project(center);
    let d = plane.project(plane.origin + dir);
    let dl = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if dl <= EPS {
        return Vec::new();
    }
    let (dx, dy) = (d[0] / dl, d[1] / dl);
    let is_circle = matches!(
        curve,
        DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
    );
    let t = if is_circle {
        (dy).atan2(dx)
    } else {
        (dy / a).atan2(dx / b)
    };
    [t, t + std::f64::consts::PI]
        .into_iter()
        .filter_map(|t| {
            let q = if is_circle {
                [c[0] + a * t.cos(), c[1] + a * t.sin()]
            } else {
                [c[0] + a * t.cos(), c[1] + b * t.sin()]
            };
            let world = plane.lift(q);
            on_curve(curve, plane, q, world).then_some(world)
        })
        .collect()
}

/// Candidate line pairs for two deferred picks. `None` means this order is not
/// handled; the caller retries with the ends swapped.
fn deferred_pairs(d1: &DeferredPick, d2: &DeferredPick) -> Option<Vec<(DVec3, DVec3)>> {
    let straight1 = matches!(d1.curve, DeferredCurve::Line { .. });
    let straight2 = matches!(d2.curve, DeferredCurve::Line { .. });

    if !straight1
        && !straight2
        && d1.mode == DeferredMode::Tangent
        && d2.mode == DeferredMode::Tangent
    {
        return common_tangents(d1, d2);
    }

    // PER on a straight target: the contact point is the other end projected
    // onto that line.
    if d2.mode == DeferredMode::Perpendicular && straight2 {
        let dir = line_direction(&d2.curve)?;
        let normal = DVec3::new(-dir.y, dir.x, 0.0);
        let curve_plane = curve_plane(&d1.curve);
        let plane_normal = curve_plane.map(|p| p.u.cross(p.v));
        let normal = if let (Some(p), Some(n)) = (curve_plane, plane_normal) {
            if n.length_squared() > EPS && n.normalize().dot(normal).abs() < 0.999 {
                // Project the line's normal into the curve's plane.
                let d = p.project(p.origin + normal);
                let dl = (d[0] * d[0] + d[1] * d[1]).sqrt();
                if dl > EPS {
                    p.lift([d[0] / dl, d[1] / dl]) - p.origin
                } else {
                    normal
                }
            } else {
                normal
            }
        } else {
            normal
        };
        // Tangent ∥ line normal  <=>  normal ∥ line direction.
        let points = match d1.mode {
            DeferredMode::Tangent => normal_points(&d1.curve, dir),
            DeferredMode::Perpendicular => normal_points(&d1.curve, normal),
        };
        let mut pairs = Vec::new();
        for q in points {
            if let Some(foot) = line_foot(&d2.curve, q) {
                pairs.push((q, foot));
            }
        }
        return Some(pairs);
    }

    // PER of a circle/ellipse goes through its centre; only the circle form
    // has that closed property, so ellipse–ellipse normal pairs are not offered.
    if !straight1
        && !straight2
        && d1.mode == DeferredMode::Perpendicular
        && matches!(
            d2.curve,
            DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
        )
    {
        let k1 = circle_center(&d1.curve)?;
        let k2 = circle_center(&d2.curve)?;
        let Some((_, r2)) = curve_axes(&d2.curve) else {
            return Some(Vec::new());
        };
        let u = (k2 - k1).normalize_or_zero();
        if u.length_squared() < 0.5 {
            return Some(Vec::new());
        }
        let q = nearest_to_hint(normal_points(&d1.curve, u), d1.at).unwrap_or(k1);
        return Some(vec![(q, k2 + u * r2), (q, k2 - u * r2)]);
    }

    // TAN from a circle/ellipse with a PER circle: the line passes through the
    // circle's centre, so use the tangents from that centre.
    if !straight1
        && d1.mode == DeferredMode::Tangent
        && !straight2
        && d2.mode == DeferredMode::Perpendicular
        && matches!(
            d2.curve,
            DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. }
        )
    {
        let k2 = circle_center(&d2.curve)?;
        let Some((_, r2)) = curve_axes(&d2.curve) else {
            return Some(Vec::new());
        };
        let mut pairs = Vec::new();
        for q in tangent_candidates(&d1.curve, k2) {
            let u = (q - k2).normalize_or_zero();
            if u.length_squared() > 0.5 {
                pairs.push((q, k2 + u * r2));
                pairs.push((q, k2 - u * r2));
            }
        }
        return Some(pairs);
    }

    // Two parallel straight targets share a perpendicular.
    if straight1
        && straight2
        && d1.mode == DeferredMode::Perpendicular
        && d2.mode == DeferredMode::Perpendicular
    {
        let v1 = line_direction(&d1.curve)?;
        let v2 = line_direction(&d2.curve)?;
        if v1.cross(v2).length() > 1e-8 {
            return Some(Vec::new());
        }
        let p = line_foot(&d1.curve, d1.at)?;
        let q = line_foot(&d2.curve, p)?;
        return Some(vec![(p, q)]);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f64, y: f64) -> DVec3 {
        DVec3::new(x, y, 0.0)
    }
    fn line(a: [f64; 2], b: [f64; 2]) -> DeferredCurve {
        DeferredCurve::Line {
            p1: pt(a[0], a[1]),
            p2: pt(b[0], b[1]),
        }
    }
    fn circle(c: [f64; 2], r: f64) -> DeferredCurve {
        DeferredCurve::Circle {
            center: pt(c[0], c[1]),
            axis_x: DVec3::X,
            axis_y: DVec3::Y,
            radius: r,
        }
    }
    fn arc(c: [f64; 2], r: f64, start: f64, end: f64) -> DeferredCurve {
        DeferredCurve::Arc {
            center: pt(c[0], c[1]),
            axis_x: DVec3::X,
            axis_y: DVec3::Y,
            radius: r,
            start,
            end,
        }
    }
    fn ellipse(c: [f64; 2], a: f64, ratio: f64) -> DeferredCurve {
        DeferredCurve::Ellipse {
            center: pt(c[0], c[1]),
            major_axis: DVec3::X * a,
            normal: DVec3::Z,
            minor_axis_ratio: ratio,
            start: 0.0,
            end: 0.0,
        }
    }
    fn tangent(curve: DeferredCurve, at: DVec3) -> LineEnd {
        LineEnd::Deferred(DeferredPick {
            mode: DeferredMode::Tangent,
            curve,
            at,
        })
    }
    fn perpendicular(curve: DeferredCurve, at: DVec3) -> LineEnd {
        LineEnd::Deferred(DeferredPick {
            mode: DeferredMode::Perpendicular,
            curve,
            at,
        })
    }
    fn dot_tangent(curve: &DeferredCurve, q: DVec3, dir: DVec3) -> f64 {
        let Some(plane) = curve_plane(curve) else {
            return 0.0;
        };
        let c = plane.project(circle_center(curve).unwrap_or(plane.origin));
        let qq = plane.project(q);
        let (x, y) = (qq[0] - c[0], qq[1] - c[1]);
        let (dx, dy) = {
            let d = plane.project(plane.origin + dir);
            (d[0], d[1])
        };
        match curve {
            DeferredCurve::Circle { .. } | DeferredCurve::Arc { .. } => x * dx + y * dy,
            DeferredCurve::Ellipse {
                minor_axis_ratio, ..
            } => {
                let (a, _) = curve_axes(curve).unwrap();
                let b = a * minor_axis_ratio;
                (x / (a * a)) * dx + (y / (b * b)) * dy
            }
            DeferredCurve::Line { .. } => 0.0,
        }
    }

    #[test]
    fn tangent_tangent_circles_have_common_tangents_nearest_the_picks() {
        let c1 = circle([0.0, 0.0], 2.0);
        let c2 = circle([10.0, 0.0], 2.0);
        let (p, q) = resolve_deferred_line(tangent(c1, pt(0.0, 2.0)), tangent(c2, pt(10.0, 2.0)))
            .expect("a common tangent");
        // The top external tangent is the one nearest both picks.
        assert!((p.y - 2.0).abs() < 1e-4, "{p:?}");
        assert!((q.y - 2.0).abs() < 1e-4, "{q:?}");
        assert!((dot_tangent(&circle([0.0, 0.0], 2.0), p, q - p)).abs() < 1e-3);
    }

    #[test]
    fn tangent_tangent_circle_ellipse_is_supported() {
        let e = ellipse([-20.0, 0.0], 10.0, 0.5);
        let c = circle([20.0, 0.0], 5.0);
        let solved = resolve_deferred_line(tangent(e, pt(-20.0, 5.0)), tangent(c, pt(20.0, 5.0)));
        assert!(
            solved.is_some(),
            "an ellipse and a circle have common tangents"
        );
    }

    #[test]
    fn tangent_perpendicular_circle_and_line() {
        let (p, q) = resolve_deferred_line(
            tangent(circle([0.0, 0.0], 5.0), pt(0.0, 5.0)),
            perpendicular(line([0.0, 20.0], [10.0, 20.0]), pt(0.0, 20.0)),
        )
        .expect("a tangent to a circle and perpendicular to a line");
        assert!((p.length() - 5.0).abs() < 1e-8, "off circle: {p:?}");
        assert!((q.y - 20.0).abs() < 1e-8, "off line: {q:?}");
        assert!((dot_tangent(&circle([0.0, 0.0], 5.0), p, q - p)).abs() < 1e-6);
    }

    #[test]
    fn tangent_perpendicular_ellipse_and_line() {
        let e = ellipse([0.0, 0.0], 10.0, 0.5);
        let solved = resolve_deferred_line(
            tangent(e, pt(0.0, 5.0)),
            perpendicular(line([20.0, -20.0], [20.0, 20.0]), pt(20.0, 0.0)),
        );
        assert!(
            solved.is_some(),
            "ellipse tangent + line perpendicular has a solution"
        );
    }

    #[test]
    fn perpendicular_perpendicular_circles_run_through_both_centres() {
        let (p, q) = resolve_deferred_line(
            perpendicular(circle([0.0, 0.0], 3.0), pt(3.0, 0.0)),
            perpendicular(circle([20.0, 0.0], 4.0), pt(24.0, 0.0)),
        )
        .expect("a line through both centres");
        assert!(
            (p.x - 3.0).abs() < 1e-8 && (q.x - 24.0).abs() < 1e-8,
            "{p:?} {q:?}"
        );
    }

    #[test]
    fn perpendicular_perpendicular_ellipse_and_line() {
        let solved = resolve_deferred_line(
            perpendicular(ellipse([0.0, 0.0], 10.0, 0.5), pt(10.0, 0.0)),
            perpendicular(line([20.0, -20.0], [20.0, 20.0]), pt(20.0, 0.0)),
        );
        assert!(solved.is_some());
    }

    #[test]
    fn arc_solutions_outside_the_sweep_are_filtered() {
        let upper = arc([0.0, 0.0], 5.0, 0.0, std::f64::consts::PI);
        let (p, _) = resolve_deferred_line(
            perpendicular(upper, pt(5.0, 0.0)),
            LineEnd::Point(pt(0.0, -20.0)),
        )
        .expect("a perpendicular from below");
        assert!(
            p.y >= -1e-6,
            "a solution in the lower half is off the arc: {p:?}"
        );
    }

    #[test]
    fn nearest_solution_is_selected() {
        let c = circle([0.0, 0.0], 5.0);
        let (p, _) =
            resolve_deferred_line(tangent(c, pt(-5.0, 0.0)), LineEnd::Point(pt(20.0, 5.0)))
                .expect("a tangent from (20,5)");
        let (other, _) =
            resolve_deferred_line(tangent(c, pt(5.0, 0.0)), LineEnd::Point(pt(20.0, 5.0)))
                .expect("the same line with a different hint");
        assert!(
            p.y > 0.0 && other.y < 0.0,
            "hints must choose opposite sides"
        );
    }

    #[test]
    fn degenerate_and_parallel_cases_return_none() {
        // A point inside a circle has no tangent.
        assert!(resolve_deferred_line(
            tangent(circle([0.0, 0.0], 5.0), pt(0.0, 5.0)),
            LineEnd::Point(pt(1.0, 1.0))
        )
        .is_none());
        // Non-parallel lines have no common perpendicular of this form.
        assert!(resolve_deferred_line(
            perpendicular(line([0.0, 0.0], [10.0, 0.0]), pt(1.0, 0.0)),
            perpendicular(line([0.0, 0.0], [0.0, 10.0]), pt(0.0, 1.0))
        )
        .is_none());
        // Zero-length result.
        assert!(
            resolve_deferred_line(LineEnd::Point(pt(1.0, 1.0)), LineEnd::Point(pt(1.0, 1.0)))
                .is_none()
        );
        // Concentric circles have no common tangent.
        assert!(resolve_deferred_line(
            tangent(circle([0.0, 0.0], 5.0), pt(0.0, 5.0)),
            tangent(circle([0.0, 0.0], 2.0), pt(0.0, 2.0))
        )
        .is_none());
    }

    #[test]
    fn point_in_direction_resolves_typed_polar_entry() {
        let c = circle([0.0, 0.0], 5.0);
        let p = point_in_direction(&c, DeferredMode::Tangent, DVec3::X, pt(0.0, 5.0))
            .expect("a tangent point in the +X direction");
        assert!((p.y - 5.0).abs() < 1e-9 && p.x.abs() < 1e-9, "{p:?}");

        let l = line([0.0, 0.0], [10.0, 0.0]);
        assert!(
            point_in_direction(&l, DeferredMode::Perpendicular, DVec3::X, pt(5.0, 0.0)).is_none()
        );
        let p = point_in_direction(&l, DeferredMode::Perpendicular, DVec3::Y, pt(5.0, 0.0))
            .expect("the +Y normal");
        assert!((p.x - 5.0).abs() < 1e-9);
    }
}
