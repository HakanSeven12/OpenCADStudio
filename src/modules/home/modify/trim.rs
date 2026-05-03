// Trim / Extend — ribbon definitions + full command implementations.
//
// TRIM  (TR): Click the segment you want to remove. The command finds all
//             intersections of that entity with every other entity and trims
//             out the clicked interval. Stays active — click more segments,
//             press Enter to finish.
//
// EXTEND (EX): Click near one end of an entity.  The command extends that
//              endpoint to the nearest intersecting boundary. Stays active.

use std::f64::consts::TAU;

use acadrust::entities::{Arc as ArcEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline, Ray as RayEnt, Spline as SplineEnt, XLine as XLineEnt};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::Vec3;
use truck_modeling::base::{BoundedCurve, Cut, ParametricCurve};

use crate::command::{CadCommand, CmdResult};
use crate::modules::IconKind;
use crate::modules::home::modify::spline_ops::{
    bspline_to_spline, spline_nearest_t, spline_sample_xy, spline_to_bspline, spline_pts_wire, t_to_rel,
};
use crate::scene::wire_model::WireModel;

// ── Dropdown constants ─────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "trim_extend";
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/trim.svg"));

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    (
        "TRIM",
        "Trim",
        IconKind::Svg(include_bytes!("../../../../assets/icons/trim.svg")),
    ),
    (
        "EXTEND",
        "Extend",
        IconKind::Svg(include_bytes!("../../../../assets/icons/extend.svg")),
    ),
];

// ══════════════════════════════════════════════════════════════════════════
// Geometry helpers
// ══════════════════════════════════════════════════════════════════════════

/// Normalize angle to [0, 2π).
fn norm(a: f64) -> f64 {
    ((a % TAU) + TAU) % TAU
}

/// Is angle `a` within the arc from `s` to `e` (CCW, radians)?
fn in_arc(a: f64, s: f64, e: f64) -> bool {
    let (a, s, e) = (norm(a), norm(s), norm(e));
    if (e - s).abs() < 1e-9 || (e - s - TAU).abs() < 1e-9 {
        return true;
    }
    if s <= e {
        a >= s - 1e-9 && a <= e + 1e-9
    } else {
        a >= s - 1e-9 || a <= e + 1e-9
    }
}

/// Parametric t ∈ [0,1] on arc (a0→a1 CCW) for angle `a`.
fn arc_t(a: f64, a0: f64, a1: f64) -> f64 {
    let span = {
        let s = norm(a1) - norm(a0);
        if s <= 0.0 {
            s + TAU
        } else {
            s
        }
    };
    let da = {
        let d = norm(a) - norm(a0);
        if d < 0.0 {
            d + TAU
        } else {
            d
        }
    };
    (da / span).clamp(0.0, 1.0)
}

/// Intersect infinite lines (p+t·d) and (q+u·e). Returns (t, u).
fn ll(
    px: f64,
    py: f64,
    dx: f64,
    dy: f64,
    qx: f64,
    qy: f64,
    ex: f64,
    ey: f64,
) -> Option<(f64, f64)> {
    let det = dx * ey - dy * ex;
    if det.abs() < 1e-10 {
        return None;
    }
    let t = ((qx - px) * ey - (qy - py) * ex) / det;
    let u = ((qx - px) * dy - (qy - py) * dx) / det;
    Some((t, u))
}

/// Intersect infinite line (p+t·d) with circle (cx,cy,r). Returns t values.
fn lc(px: f64, py: f64, dx: f64, dy: f64, cx: f64, cy: f64, r: f64) -> Vec<f64> {
    let fx = px - cx;
    let fy = py - cy;
    let a = dx * dx + dy * dy;
    let b = 2.0 * (fx * dx + fy * dy);
    let c = fx * fx + fy * fy - r * r;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return vec![];
    }
    let sq = disc.sqrt();
    if disc < 1e-14 {
        vec![(-b) / (2.0 * a)]
    } else {
        vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
    }
}

/// Circle-circle intersection: angles on circle 1 where they meet.
fn cc_angles(cx1: f64, cy1: f64, r1: f64, cx2: f64, cy2: f64, r2: f64) -> Vec<f64> {
    let d = ((cx2 - cx1).powi(2) + (cy2 - cy1).powi(2)).sqrt();
    if d < 1e-9 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
        return vec![];
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h2 = r1 * r1 - a * a;
    if h2 < 0.0 {
        return vec![];
    }
    let h = h2.sqrt();
    let mx = cx1 + a * (cx2 - cx1) / d;
    let my = cy1 + a * (cy2 - cy1) / d;
    let px = h * (cy2 - cy1) / d;
    let py = -h * (cx2 - cx1) / d;
    let a1 = ((my + py) - cy1).atan2((mx + px) - cx1);
    let a2 = ((my - py) - cy1).atan2((mx - px) - cx1);
    if h < 1e-9 {
        vec![a1]
    } else {
        vec![a1, a2]
    }
}

/// Line (px+s·d) vs ellipse (cx,cy,a,b,nx,ny). Returns (s_on_line, t_on_ellipse) pairs.
/// nx,ny: unit major-axis; perp = (-ny, nx).  Parametric ellipse: P(t) = center + a·cos(t)·n + b·sin(t)·v.
fn le(px: f64, py: f64, dpx: f64, dpy: f64, cx: f64, cy: f64, a: f64, b: f64, nx: f64, ny: f64) -> Vec<(f64, f64)> {
    // Transform line origin to ellipse local frame
    let rx = px - cx;
    let ry = py - cy;
    // Project onto major/minor axes
    let xl0 =  rx * nx + ry * ny;
    let yl0 = -rx * ny + ry * nx;
    let dxl =  dpx * nx + dpy * ny;
    let dyl = -dpx * ny + dpy * nx;
    // Scale by 1/a, 1/b → circle equation
    let xa = xl0 / a;  let xda = dxl / a;
    let yb = yl0 / b;  let ydb = dyl / b;
    let big_a = xda * xda + ydb * ydb;
    if big_a < 1e-20 { return vec![]; }
    let big_b = 2.0 * (xa * xda + yb * ydb);
    let big_c = xa * xa + yb * yb - 1.0;
    let disc = big_b * big_b - 4.0 * big_a * big_c;
    if disc < 0.0 { return vec![]; }
    let sq = disc.sqrt();
    let s_vals: Vec<f64> = if disc < 1e-14 {
        vec![(-big_b) / (2.0 * big_a)]
    } else {
        vec![(-big_b - sq) / (2.0 * big_a), (-big_b + sq) / (2.0 * big_a)]
    };
    s_vals.into_iter().map(|s| {
        let xl = xl0 + s * dxl;
        let yl = yl0 + s * dyl;
        let t = yl.atan2(xl); // ≡ atan2(yl/b, xl/a) but faster since sign is preserved
        (s, t)
    }).collect()
}

// ── Boundary geometry ─────────────────────────────────────────────────────

/// Virtual extent used to represent infinite ends of Ray / XLine.
const TRIM_EXTENT: f64 = 1_000_000.0;
/// If a trim interval endpoint is beyond this threshold it is treated as "infinite".
const INF_T: f64 = 0.9999;

enum Geo {
    Line {
        handle: Handle,
        p1: [f64; 2],
        p2: [f64; 2],
    },
    Arc {
        handle: Handle,
        cx: f64,
        cy: f64,
        r: f64,
        a0: f64,
        a1: f64,
    },
    Circle {
        handle: Handle,
        cx: f64,
        cy: f64,
        r: f64,
    },
    /// Semi-infinite line from base in +direction.
    Ray {
        handle: Handle,
        bx: f64,
        by: f64,
        dx: f64,
        dy: f64,
    },
    /// Fully-infinite line through base along direction.
    InfLine {
        handle: Handle,
        bx: f64,
        by: f64,
        dx: f64,
        dy: f64,
    },
    /// Ellipse arc: center, semi-axes, unit major-axis direction, parameter range [t0,t1].
    Ellipse {
        handle: Handle,
        cx: f64,
        cy: f64,
        a: f64,  // semi-major
        b: f64,  // semi-minor
        nx: f64, // unit major-axis X
        ny: f64, // unit major-axis Y
        t0: f64, // start parameter
        t1: f64, // end parameter (may be > 2π if wrapped)
    },
    /// Spline represented as sampled polyline segments (DXF XY).
    Spline {
        handle: Handle,
        segs: Vec<([f64; 2], [f64; 2])>,
    },
}

fn build_geos(entities: &[EntityType]) -> Vec<Geo> {
    entities
        .iter()
        .filter_map(|e| {
            let h = e.common().handle;
            match e {
                EntityType::Line(l) => Some(Geo::Line {
                    handle: h,
                    p1: [l.start.x, l.start.y],
                    p2: [l.end.x, l.end.y],
                }),
                EntityType::Arc(a) => Some(Geo::Arc {
                    handle: h,
                    cx: a.center.x,
                    cy: a.center.y,
                    r: a.radius,
                    a0: a.start_angle,
                    a1: a.end_angle,
                }),
                EntityType::Circle(c) => Some(Geo::Circle {
                    handle: h,
                    cx: c.center.x,
                    cy: c.center.y,
                    r: c.radius,
                }),
                EntityType::Ray(r) => Some(Geo::Ray {
                    handle: h,
                    bx: r.base_point.x,
                    by: r.base_point.y,
                    dx: r.direction.x,
                    dy: r.direction.y,
                }),
                EntityType::XLine(x) => Some(Geo::InfLine {
                    handle: h,
                    bx: x.base_point.x,
                    by: x.base_point.y,
                    dx: x.direction.x,
                    dy: x.direction.y,
                }),
                EntityType::Ellipse(e) => {
                    let mx = e.major_axis.x;
                    let my = e.major_axis.y;
                    let a = (mx * mx + my * my).sqrt();
                    if a < 1e-9 { return None; }
                    let (nx, ny) = (mx / a, my / a);
                    let b = a * e.minor_axis_ratio;
                    let t0 = e.start_parameter;
                    let mut t1 = e.end_parameter;
                    if t1 <= t0 { t1 += TAU; }
                    Some(Geo::Ellipse { handle: h, cx: e.center.x, cy: e.center.y, a, b, nx, ny, t0, t1 })
                }
                EntityType::Spline(s) => {
                    let (_, pts) = spline_sample_xy(s, 64);
                    if pts.len() < 2 { return None; }
                    let segs = pts.windows(2)
                        .map(|w| ([w[0][0], w[0][1]], [w[1][0], w[1][1]]))
                        .collect();
                    Some(Geo::Spline { handle: h, segs })
                }
                _ => None,
            }
        })
        .collect()
}

// ── Intersection helpers ──────────────────────────────────────────────────

/// Sorted, deduped t-params ∈ [0,1] where LINE segment (ax,ay)→(bx,by) intersects boundaries.
fn line_seg_ts(ax: f64, ay: f64, bx: f64, by: f64, target: Handle, geos: &[Geo]) -> Vec<f64> {
    let (dx, dy) = (bx - ax, by - ay);
    let mut ts = vec![];
    for geo in geos {
        match geo {
            Geo::Line { handle, p1, p2 } => {
                if *handle == target {
                    continue;
                }
                let (ex, ey) = (p2[0] - p1[0], p2[1] - p1[1]);
                if let Some((t, u)) = ll(ax, ay, dx, dy, p1[0], p1[1], ex, ey) {
                    if (-1e-9..=1.0 + 1e-9).contains(&u) && (-1e-9..=1.0 + 1e-9).contains(&t) {
                        ts.push(t.clamp(0.0, 1.0));
                    }
                }
            }
            Geo::Arc {
                handle,
                cx,
                cy,
                r,
                a0,
                a1,
            } => {
                if *handle == target {
                    continue;
                }
                for t in lc(ax, ay, dx, dy, *cx, *cy, *r) {
                    if !(-1e-9..=1.0 + 1e-9).contains(&t) {
                        continue;
                    }
                    let ix = ax + t * dx;
                    let iy = ay + t * dy;
                    if in_arc((iy - cy).atan2(ix - cx), *a0, *a1) {
                        ts.push(t.clamp(0.0, 1.0));
                    }
                }
            }
            Geo::Circle { handle, cx, cy, r } => {
                if *handle == target {
                    continue;
                }
                for t in lc(ax, ay, dx, dy, *cx, *cy, *r) {
                    if (-1e-9..=1.0 + 1e-9).contains(&t) {
                        ts.push(t.clamp(0.0, 1.0));
                    }
                }
            }
            Geo::Ray { handle, bx: rbx, by: rby, dx: rdx, dy: rdy } => {
                if *handle == target { continue; }
                if let Some((t, u)) = ll(ax, ay, dx, dy, *rbx, *rby, *rdx, *rdy) {
                    // Ray: u >= 0 (semi-infinite)
                    if u >= -1e-9 && (-1e-9..=1.0 + 1e-9).contains(&t) {
                        ts.push(t.clamp(0.0, 1.0));
                    }
                }
            }
            Geo::InfLine { handle, bx: ibx, by: iby, dx: idx, dy: idy } => {
                if *handle == target { continue; }
                if let Some((t, _u)) = ll(ax, ay, dx, dy, *ibx, *iby, *idx, *idy) {
                    // XLine: any u accepted
                    if (-1e-9..=1.0 + 1e-9).contains(&t) {
                        ts.push(t.clamp(0.0, 1.0));
                    }
                }
            }
            Geo::Ellipse { handle, cx, cy, a, b, nx, ny, t0, t1 } => {
                if *handle == target { continue; }
                for (s, t_ell) in le(ax, ay, dx, dy, *cx, *cy, *a, *b, *nx, *ny) {
                    if !(-1e-9..=1.0 + 1e-9).contains(&s) { continue; }
                    if in_arc(t_ell, *t0, *t1) {
                        ts.push(s.clamp(0.0, 1.0));
                    }
                }
            }
            Geo::Spline { handle, segs } => {
                if *handle == target { continue; }
                for (p1, p2) in segs {
                    let ex = p2[0] - p1[0];
                    let ey = p2[1] - p1[1];
                    if let Some((t, u)) = ll(ax, ay, dx, dy, p1[0], p1[1], ex, ey) {
                        if (-1e-9..=1.0 + 1e-9).contains(&u) && (-1e-9..=1.0 + 1e-9).contains(&t) {
                            ts.push(t.clamp(0.0, 1.0));
                        }
                    }
                }
            }
        }
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    ts
}

/// Sorted, deduped t-params ∈ [0,1] where ARC (cx,cy,r,a0→a1) intersects boundaries.
fn arc_seg_ts(
    cx: f64,
    cy: f64,
    r: f64,
    a0: f64,
    a1: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    let mut ts = vec![];
    for geo in geos {
        let angles: Vec<f64> = match geo {
            Geo::Line { handle, p1, p2 } => {
                if *handle == target {
                    continue;
                }
                let (ldx, ldy) = (p2[0] - p1[0], p2[1] - p1[1]);
                lc(p1[0], p1[1], ldx, ldy, cx, cy, r)
                    .into_iter()
                    .filter(|&u| (-1e-9..=1.0 + 1e-9).contains(&u))
                    .map(|u| (p1[1] + u * ldy - cy).atan2(p1[0] + u * ldx - cx))
                    .collect()
            }
            Geo::Arc {
                handle,
                cx: cx2,
                cy: cy2,
                r: r2,
                a0: a02,
                a1: a12,
            } => {
                if *handle == target {
                    continue;
                }
                cc_angles(cx, cy, r, *cx2, *cy2, *r2)
                    .into_iter()
                    .filter(|&a| in_arc(a, *a02, *a12))
                    .collect()
            }
            Geo::Circle {
                handle,
                cx: cx2,
                cy: cy2,
                r: r2,
            } => {
                if *handle == target {
                    continue;
                }
                cc_angles(cx, cy, r, *cx2, *cy2, *r2)
            }
            Geo::Ray { handle, bx: rbx, by: rby, dx: rdx, dy: rdy } => {
                if *handle == target { continue; }
                // Intersect arc circle with the Ray direction
                lc(*rbx, *rby, *rdx, *rdy, cx, cy, r)
                    .into_iter()
                    .filter(|&u| u >= -1e-9) // Ray: u >= 0
                    .map(|u| (rby + u * rdy - cy).atan2(rbx + u * rdx - cx))
                    .collect()
            }
            Geo::InfLine { handle, bx: ibx, by: iby, dx: idx, dy: idy } => {
                if *handle == target { continue; }
                // XLine: any u accepted
                lc(*ibx, *iby, *idx, *idy, cx, cy, r)
                    .into_iter()
                    .map(|u| (iby + u * idy - cy).atan2(ibx + u * idx - cx))
                    .collect()
            }
            Geo::Ellipse { handle, cx: ecx, cy: ecy, a: ea, b: eb, nx, ny, t0: et0, t1: et1 } => {
                if *handle == target { continue; }
                // Sample the arc and find where it crosses the ellipse boundary.
                ellipse_boundary_angles_for_arc(cx, cy, r, a0, a1, *ecx, *ecy, *ea, *eb, *nx, *ny, *et0, *et1)
            }
            Geo::Spline { handle, segs } => {
                if *handle == target { continue; }
                // Intersect arc circle with each spline segment.
                let mut hit_angles = vec![];
                for (p1, p2) in segs {
                    let ldx = p2[0] - p1[0];
                    let ldy = p2[1] - p1[1];
                    for u in lc(p1[0], p1[1], ldx, ldy, cx, cy, r) {
                        if !(-1e-9..=1.0 + 1e-9).contains(&u) { continue; }
                        let ix = p1[0] + u * ldx;
                        let iy = p1[1] + u * ldy;
                        hit_angles.push((iy - cy).atan2(ix - cx));
                    }
                }
                hit_angles
            }
        };
        for a in angles {
            if in_arc(a, a0, a1) {
                ts.push(arc_t(a, a0, a1));
            }
        }
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    ts
}

/// Find angles on a circular arc where it crosses an ellipse-arc boundary.
/// Uses 64-sample sign-change detection + bisection.
fn ellipse_boundary_angles_for_arc(
    cx: f64, cy: f64, r: f64, a0: f64, a1: f64,
    ecx: f64, ecy: f64, ea: f64, eb: f64, nx: f64, ny: f64,
    et0: f64, et1: f64,
) -> Vec<f64> {
    // f(α) = (x_local/ea)² + (y_local/eb)² – 1  where (x_local, y_local) is the arc
    // point projected onto ellipse local axes.
    let f = |alpha: f64| {
        let px = cx + r * alpha.cos() - ecx;
        let py = cy + r * alpha.sin() - ecy;
        let xl =  px * nx + py * ny;
        let yl = -px * ny + py * nx;
        (xl / ea).powi(2) + (yl / eb).powi(2) - 1.0
    };
    let span = { let s = norm(a1) - norm(a0); if s <= 0.0 { s + TAU } else { s } };
    let n = 128usize;
    let mut hits = vec![];
    let mut prev = f(norm(a0));
    for i in 1..=n {
        let alpha = norm(a0) + span * (i as f64 / n as f64);
        let cur = f(alpha);
        if prev * cur <= 0.0 {
            // Bisect
            let alpha_lo = norm(a0) + span * ((i - 1) as f64 / n as f64);
            let alpha_hi = alpha;
            let mut lo = alpha_lo;
            let mut hi = alpha_hi;
            let mut flo = prev;
            for _ in 0..32 {
                let mid = (lo + hi) * 0.5;
                let fm = f(mid);
                if flo * fm <= 0.0 { hi = mid; } else { lo = mid; flo = fm; }
            }
            let alpha_hit = (lo + hi) * 0.5;
            // Check that the intersection point is on the ellipse ARC (not outside t0..t1)
            let px = cx + r * alpha_hit.cos() - ecx;
            let py = cy + r * alpha_hit.sin() - ecy;
            let xl =  px * nx + py * ny;
            let yl = -px * ny + py * nx;
            let t_ell = yl.atan2(xl);
            if in_arc(t_ell, et0, et1) {
                hits.push(alpha_hit);
            }
        }
        prev = cur;
    }
    hits
}

/// Sorted t-params ∈ [0,1] where an ELLIPSE arc intersects boundary geometries.
/// t is the normalised eccentric-anomaly parameter along [t0, t1].
fn ellipse_seg_ts(
    cx: f64, cy: f64, a: f64, b: f64, nx: f64, ny: f64,
    t0: f64, t1: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    let span = t1 - t0; // always positive (build_geos ensures t1 > t0)
    let ellipse_pt = |t: f64| -> [f64; 2] {
        [cx + a * t.cos() * nx - b * t.sin() * ny,
         cy + a * t.cos() * ny + b * t.sin() * nx]
    };
    // f_boundary(t) > 0 means "outside this boundary segment"
    let mut ts = vec![];

    for geo in geos {
        match geo {
            Geo::Line { handle, p1, p2 } => {
                if *handle == target { continue; }
                // Find t values where ellipse crosses the infinite line p1→p2,
                // then filter to the finite segment [p1,p2].
                let ldx = p2[0] - p1[0];
                let ldy = p2[1] - p1[1];
                for (s, t_ell) in le(p1[0], p1[1], ldx, ldy, cx, cy, a, b, nx, ny) {
                    if !(-1e-9..=1.0 + 1e-9).contains(&s) { continue; }
                    if in_arc(t_ell, t0, t1) {
                        let t_norm = arc_t(t_ell, t0, t0 + span);
                        ts.push(t_norm);
                    }
                }
            }
            Geo::Arc { handle, cx: acx, cy: acy, r, a0: aa0, a1: aa1 } => {
                if *handle == target { continue; }
                // 64-sample sign-change on (dist_to_arc_circle - r)
                let n = 64usize;
                let mut prev_sign = {
                    let [px, py] = ellipse_pt(t0);
                    (px - acx).hypot(py - acy) - r
                };
                for i in 1..=n {
                    let t_ell = t0 + span * (i as f64 / n as f64);
                    let [px, py] = ellipse_pt(t_ell);
                    let cur_sign = (px - acx).hypot(py - acy) - r;
                    if prev_sign * cur_sign <= 0.0 {
                        let t_lo = t0 + span * ((i - 1) as f64 / n as f64);
                        let t_hi = t_ell;
                        let mut lo = t_lo; let mut hi = t_hi;
                        let mut flo = prev_sign;
                        for _ in 0..32 {
                            let mid = (lo + hi) * 0.5;
                            let [px2, py2] = ellipse_pt(mid);
                            let fm = (px2 - acx).hypot(py2 - acy) - r;
                            if flo * fm <= 0.0 { hi = mid; } else { lo = mid; flo = fm; }
                        }
                        let t_hit = (lo + hi) * 0.5;
                        let [phx, phy] = ellipse_pt(t_hit);
                        let ang = (phy - acy).atan2(phx - acx);
                        if in_arc(ang, *aa0, *aa1) {
                            ts.push(arc_t(t_hit, t0, t0 + span));
                        }
                    }
                    prev_sign = cur_sign;
                }
            }
            Geo::Circle { handle, cx: acx, cy: acy, r } => {
                if *handle == target { continue; }
                let n = 64usize;
                let mut prev_sign = {
                    let [px, py] = ellipse_pt(t0);
                    (px - acx).hypot(py - acy) - r
                };
                for i in 1..=n {
                    let t_ell = t0 + span * (i as f64 / n as f64);
                    let [px, py] = ellipse_pt(t_ell);
                    let cur_sign = (px - acx).hypot(py - acy) - r;
                    if prev_sign * cur_sign <= 0.0 {
                        let t_lo = t0 + span * ((i - 1) as f64 / n as f64);
                        let t_hi = t_ell;
                        let mut lo = t_lo; let mut hi = t_hi;
                        let mut flo = prev_sign;
                        for _ in 0..32 {
                            let mid = (lo + hi) * 0.5;
                            let [px2, py2] = ellipse_pt(mid);
                            let fm = (px2 - acx).hypot(py2 - acy) - r;
                            if flo * fm <= 0.0 { hi = mid; } else { lo = mid; flo = fm; }
                        }
                        ts.push(arc_t((lo + hi) * 0.5, t0, t0 + span));
                    }
                    prev_sign = cur_sign;
                }
            }
            Geo::Ray { handle, bx: rbx, by: rby, dx: rdx, dy: rdy } => {
                if *handle == target { continue; }
                for (s, t_ell) in le(*rbx, *rby, *rdx, *rdy, cx, cy, a, b, nx, ny) {
                    if s >= -1e-9 && in_arc(t_ell, t0, t1) {
                        ts.push(arc_t(t_ell, t0, t0 + span));
                    }
                }
            }
            Geo::InfLine { handle, bx: ibx, by: iby, dx: idx, dy: idy } => {
                if *handle == target { continue; }
                for (_s, t_ell) in le(*ibx, *iby, *idx, *idy, cx, cy, a, b, nx, ny) {
                    if in_arc(t_ell, t0, t1) {
                        ts.push(arc_t(t_ell, t0, t0 + span));
                    }
                }
            }
            Geo::Ellipse { handle, .. } => {
                if *handle == target { continue; }
                // Ellipse-ellipse: numerical 64-sample
                if let Geo::Ellipse { cx: ecx2, cy: ecy2, a: ea2, b: eb2, nx: nx2, ny: ny2, t0: et02, t1: et12, .. } = geo {
                    let n = 64usize;
                    let f = |t: f64| -> f64 {
                        let [px, py] = ellipse_pt(t);
                        let xl =  (px - ecx2) * nx2 + (py - ecy2) * ny2;
                        let yl = -(px - ecx2) * ny2 + (py - ecy2) * nx2;
                        (xl / ea2).powi(2) + (yl / eb2).powi(2) - 1.0
                    };
                    let mut prev_f = f(t0);
                    for i in 1..=n {
                        let t_ell = t0 + span * (i as f64 / n as f64);
                        let cur_f = f(t_ell);
                        if prev_f * cur_f <= 0.0 {
                            let t_lo = t0 + span * ((i - 1) as f64 / n as f64);
                            let mut lo = t_lo; let mut hi = t_ell; let mut flo = prev_f;
                            for _ in 0..32 {
                                let mid = (lo + hi) * 0.5;
                                let fm = f(mid);
                                if flo * fm <= 0.0 { hi = mid; } else { lo = mid; flo = fm; }
                            }
                            let t_hit = (lo + hi) * 0.5;
                            let [phx, phy] = ellipse_pt(t_hit);
                            let xl =  (phx - ecx2) * nx2 + (phy - ecy2) * ny2;
                            let yl = -(phx - ecx2) * ny2 + (phy - ecy2) * nx2;
                            let t_ell2 = yl.atan2(xl);
                            if in_arc(t_ell2, *et02, *et12) {
                                ts.push(arc_t(t_hit, t0, t0 + span));
                            }
                        }
                        prev_f = cur_f;
                    }
                }
            }
            Geo::Spline { handle, segs } => {
                if *handle == target { continue; }
                // Ellipse × Spline: sign-change detection on each spline segment
                for (p1, p2) in segs {
                    let ldx = p2[0] - p1[0];
                    let ldy = p2[1] - p1[1];
                    for (s, t_ell) in le(p1[0], p1[1], ldx, ldy, cx, cy, a, b, nx, ny) {
                        if !(-1e-9..=1.0 + 1e-9).contains(&s) { continue; }
                        if in_arc(t_ell, t0, t1) {
                            ts.push(arc_t(t_ell, t0, t0 + span));
                        }
                    }
                }
            }
        }
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    ts
}

/// Trim an Ellipse entity. Returns the surviving ellipse-arc segments.
fn trim_ellipse(orig: &EllipseEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let t0 = orig.start_parameter;
    let mut t1 = orig.end_parameter;
    if t1 <= t0 { t1 += TAU; }
    let span = t1 - t0;
    let angle_at = |t: f64| t0 + span * t;

    trim_intervals(ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            if (tb - ta).abs() < 1e-6 { return None; }
            let mut e = orig.clone();
            e.common.handle = Handle::NULL;
            e.start_parameter = angle_at(ta);
            e.end_parameter   = angle_at(tb);
            Some(EntityType::Ellipse(e))
        })
        .collect()
}

/// Extend an Ellipse arc to the nearest boundary (along the arc direction).
fn extend_ellipse(orig: &EllipseEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let t0 = orig.start_parameter;
    let mut t1 = orig.end_parameter;
    if t1 <= t0 { t1 += TAU; }
    let span = t1 - t0;
    let a = (orig.major_axis.x.powi(2) + orig.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 { return None; }
    let b = a * orig.minor_axis_ratio;
    let (nx, ny) = (orig.major_axis.x / a, orig.major_axis.y / a);
    let cx = orig.center.x;
    let cy = orig.center.y;
    let ts = ellipse_seg_ts(cx, cy, a, b, nx, ny, t0, t1, orig.common.handle, geos);
    let extend_end = t_click >= 0.5;

    let best = if extend_end {
        ts.into_iter().filter(|&t| t > 1.0 + 1e-6)
            .min_by(|x, y| x.partial_cmp(y).unwrap())
    } else {
        ts.into_iter().filter(|&t| t < -1e-6)
            .max_by(|x, y| x.partial_cmp(y).unwrap())
    };

    let best_t = best?;
    let new_param = t0 + span * best_t;
    let mut e = orig.clone();
    e.common.handle = Handle::NULL;
    if extend_end {
        e.end_parameter = new_param;
    } else {
        e.start_parameter = new_param;
    }
    Some(EntityType::Ellipse(e))
}

/// Generate preview points for an ellipse arc.
fn ellipse_pts(cx: f64, cy: f64, a: f64, b: f64, nx: f64, ny: f64, t0: f64, t1: f64, z: f64) -> Vec<[f32; 3]> {
    let span = t1 - t0;
    let steps = (span.abs() * 20.0).ceil().max(4.0) as usize;
    (0..=steps).map(|i| {
        let t = t0 + span * (i as f64 / steps as f64);
        let lx = a * t.cos();
        let ly = b * t.sin();
        [(cx + lx * nx - ly * ny) as f32,
         z as f32,
         (cy + lx * ny + ly * nx) as f32]
    }).collect()
}

// ── Spline trim / extend ──────────────────────────────────────────────────

/// Find normalised t-params ∈ [0,1] where a Spline intersects boundary geos.
/// Uses sampled polyline segments for intersection detection.
fn spline_seg_ts(spl: &SplineEnt, target: Handle, geos: &[Geo]) -> Vec<f64> {
    let bs = match spline_to_bspline(spl) { Some(b) => b, None => return vec![] };
    let (t0, t1) = bs.range_tuple();
    let range = t1 - t0;
    if range < 1e-12 { return vec![]; }

    let (ts_spl, pts) = spline_sample_xy(spl, 64);
    let mut out = vec![];
    for i in 0..pts.len().saturating_sub(1) {
        let ax = pts[i][0];
        let ay = pts[i][1];
        let bx = pts[i + 1][0];
        let by = pts[i + 1][1];
        let seg_ts = line_seg_ts(ax, ay, bx, by, target, geos);
        for u in seg_ts {
            // u is a t-param on this polyline segment; map to spline knot range, then normalise.
            let t_spline = ts_spl[i] + u * (ts_spl[i + 1] - ts_spl[i]);
            out.push(t_to_rel(t_spline, t0, t1));
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    out
}

/// Trim a Spline entity. Returns surviving spline pieces (one or two).
fn trim_spline(spl: &SplineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bs = match spline_to_bspline(spl) { Some(b) => b, None => return vec![] };
    let (t0, t1) = bs.range_tuple();

    trim_intervals(ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let t_lo = t0 + ta * (t1 - t0);
            let t_hi = t0 + tb * (t1 - t0);
            if t_hi - t_lo < 1e-9 { return None; }
            let mut piece = bs.clone();
            let right = piece.cut(t_lo);   // piece = [t0..t_lo] (discarded), right = [t_lo..t1]
            let mut right = right;
            let _tail = right.cut(t_hi);   // right = [t_lo..t_hi], _tail discarded
            Some(EntityType::Spline(bspline_to_spline(&right, spl)))
        })
        .collect()
}

/// Extend a Spline toward the nearest boundary (nearest endpoint to pick).
fn extend_spline(spl: &SplineEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    // Sample spline and treat it like a polyline; look for intersections beyond
    // the current start (t<0 virtual) or end (t>1 virtual).
    // For splines we simply find whether the start (t=0) or end (t=1) is closer
    // to the click, then walk along that tangent direction to the nearest boundary.
    let bs = spline_to_bspline(spl)?;
    let (t0, t1) = bs.range_tuple();
    let extend_end = t_click >= 0.5;

    // Tangent at the endpoint (numerical, Δ = 1e-4 of range)
    let delta = (t1 - t0) * 1e-4;
    let (ep_t, tang_dir) = if extend_end {
        let p0 = bs.subs(t1 - delta);
        let p1 = bs.subs(t1);
        (t1, [p1.x - p0.x, p1.y - p0.y])
    } else {
        let p0 = bs.subs(t0);
        let p1 = bs.subs(t0 + delta);
        (t0, [p0.x - p1.x, p0.y - p1.y]) // reverse for "before start"
    };
    let ep = bs.subs(ep_t);
    let (dx, dy) = (tang_dir[0], tang_dir[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 { return None; }
    let (dx, dy) = (dx / len, dy / len);

    // Shoot a ray from the endpoint along the tangent and find nearest boundary.
    let ray_end_x = ep.x + dx * TRIM_EXTENT;
    let ray_end_y = ep.y + dy * TRIM_EXTENT;
    let seg_ts = line_seg_ts(ep.x, ep.y, ray_end_x, ray_end_y, spl.common.handle, geos);

    let best_t = seg_ts.into_iter()
        .filter(|&t| t > 1e-6)
        .reduce(f64::min)?;

    let hit_x = ep.x + best_t * (ray_end_x - ep.x) * TRIM_EXTENT;
    let hit_y = ep.y + best_t * (ray_end_y - ep.y) * TRIM_EXTENT;

    // Add a new control point at the hit location by appending/prepending.
    let z = spl.control_points.first().map(|v| v.z).unwrap_or(0.0);
    let mut new_spl = spl.clone();
    new_spl.common.handle = Handle::NULL;
    new_spl.fit_points.clear();
    if extend_end {
        new_spl.control_points.push(acadrust::types::Vector3::new(hit_x, hit_y, z));
    } else {
        new_spl.control_points.insert(0, acadrust::types::Vector3::new(hit_x, hit_y, z));
    }
    // Rebuild knots (uniform) for the extended control polygon.
    let degree = new_spl.degree as usize;
    let n = new_spl.control_points.len();
    let kv = truck_modeling::KnotVec::uniform_knot(degree, n - 1);
    new_spl.knots = kv.iter().copied().collect();
    Some(EntityType::Spline(new_spl))
}

// ── Trim helpers ──────────────────────────────────────────────────────────

/// Remove the t-interval containing `t_click` from sorted ts.  Returns surviving pieces.
fn trim_intervals(ts: &[f64], t_click: f64) -> Vec<(f64, f64)> {
    let mut bounds = vec![0.0f64];
    bounds.extend_from_slice(ts);
    bounds.push(1.0);
    bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

    let remove = bounds
        .windows(2)
        .position(|w| t_click >= w[0] - 1e-6 && t_click <= w[1] + 1e-6);

    bounds
        .windows(2)
        .enumerate()
        .filter(|(idx, _)| Some(*idx) != remove)
        .filter(|(_, w)| (w[1] - w[0]) > 1e-6)
        .map(|(_, w)| (w[0], w[1]))
        .collect()
}

fn lerp2(p1: [f64; 2], p2: [f64; 2], t: f64) -> [f64; 2] {
    [p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1])]
}

/// Trim a Line entity. Returns the surviving line segments.
fn trim_line(orig: &LineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let p1 = [orig.start.x, orig.start.y];
    let p2 = [orig.end.x, orig.end.y];
    let z = orig.start.z;
    trim_intervals(ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let a = lerp2(p1, p2, ta);
            let b = lerp2(p1, p2, tb);
            if (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-6 {
                return None;
            }
            let mut l = orig.clone();
            l.common.handle = Handle::NULL;
            l.start = Vector3::new(a[0], a[1], z);
            l.end = Vector3::new(b[0], b[1], z);
            Some(EntityType::Line(l))
        })
        .collect()
}

/// Trim an Arc entity. Returns the surviving arc segments.
fn trim_arc(orig: &ArcEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let a0 = orig.start_angle;
    let a1 = orig.end_angle;
    let span = {
        let s = norm(a1) - norm(a0);
        if s <= 0.0 {
            s + TAU
        } else {
            s
        }
    };
    let angle_at = |t: f64| norm(a0) + span * t;

    trim_intervals(ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            if (tb - ta).abs() < 1e-6 {
                return None;
            }
            let mut a = orig.clone();
            a.common.handle = Handle::NULL;
            a.start_angle = angle_at(ta);
            a.end_angle = angle_at(tb);
            Some(EntityType::Arc(a))
        })
        .collect()
}

// ── Extend helpers ────────────────────────────────────────────────────────

/// Extend the first or last segment of an LwPolyline to the nearest boundary.
/// Click point (DXF XY) determines which end to extend.
fn extend_lwpoly(poly: &LwPolyline, click_x: f64, click_y: f64, geos: &[Geo]) -> Option<EntityType> {
    let n = poly.vertices.len();
    if n < 2 { return None; }

    let first = &poly.vertices[0];
    let second = &poly.vertices[1];
    let last   = &poly.vertices[n - 1];
    let prev   = &poly.vertices[n - 2];

    let d_first = (first.location.x - click_x).hypot(first.location.y - click_y);
    let d_last  = (last.location.x  - click_x).hypot(last.location.y  - click_y);
    let extend_end = d_last <= d_first;

    // Extract the terminal segment as a virtual line.
    let (ax, ay, bx, by) = if extend_end {
        (prev.location.x, prev.location.y, last.location.x, last.location.y)
    } else {
        (second.location.x, second.location.y, first.location.x, first.location.y)
    };

    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 { return None; }

    // t_click on the segment: 0 = ax/ay, 1 = bx/by. We're extending beyond t=1.
    let target = poly.common.handle;
    let mut best_t = f64::INFINITY;

    for geo in geos {
        match geo {
            Geo::Line { handle, p1, p2 } => {
                if *handle == target { continue; }
                let (ex, ey) = (p2[0] - p1[0], p2[1] - p1[1]);
                if let Some((t, u)) = ll(ax, ay, dx, dy, p1[0], p1[1], ex, ey) {
                    if (-1e-9..=1.0 + 1e-9).contains(&u) && t > 1.0 + 1e-6 && t < best_t {
                        best_t = t;
                    }
                }
            }
            Geo::Arc { handle, cx, cy, r, a0, a1 } => {
                if *handle == target { continue; }
                for t in lc(ax, ay, dx, dy, *cx, *cy, *r) {
                    let ix = ax + t * dx;
                    let iy = ay + t * dy;
                    if in_arc((iy - cy).atan2(ix - cx), *a0, *a1) && t > 1.0 + 1e-6 && t < best_t {
                        best_t = t;
                    }
                }
            }
            Geo::Circle { handle, cx, cy, r } => {
                if *handle == target { continue; }
                for t in lc(ax, ay, dx, dy, *cx, *cy, *r) {
                    if t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                }
            }
            Geo::Ray { handle, bx: rbx, by: rby, dx: rdx, dy: rdy } => {
                if *handle == target { continue; }
                if let Some((t, u)) = ll(ax, ay, dx, dy, *rbx, *rby, *rdx, *rdy) {
                    if u >= -1e-9 && t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                }
            }
            Geo::InfLine { handle, bx: ibx, by: iby, dx: idx, dy: idy } => {
                if *handle == target { continue; }
                if let Some((t, _)) = ll(ax, ay, dx, dy, *ibx, *iby, *idx, *idy) {
                    if t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                }
            }
            Geo::Ellipse { handle, cx, cy, a, b, nx, ny, t0, t1 } => {
                if *handle == target { continue; }
                for (t, t_ell) in le(ax, ay, dx, dy, *cx, *cy, *a, *b, *nx, *ny) {
                    if in_arc(t_ell, *t0, *t1) && t > 1.0 + 1e-6 && t < best_t {
                        best_t = t;
                    }
                }
            }
            Geo::Spline { handle, segs } => {
                if *handle == target { continue; }
                for (p1, p2) in segs {
                    let ex = p2[0] - p1[0]; let ey = p2[1] - p1[1];
                    if let Some((t, u)) = ll(ax, ay, dx, dy, p1[0], p1[1], ex, ey) {
                        if (-1e-9..=1.0+1e-9).contains(&u) && t > 1.0 + 1e-6 && t < best_t {
                            best_t = t;
                        }
                    }
                }
            }
        }
    }

    if !best_t.is_finite() { return None; }

    let new_x = ax + best_t * dx;
    let new_y = ay + best_t * dy;
    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    if extend_end {
        let last_v = new_poly.vertices.last_mut()?;
        last_v.location.x = new_x;
        last_v.location.y = new_y;
    } else {
        let first_v = new_poly.vertices.first_mut()?;
        first_v.location.x = new_x;
        first_v.location.y = new_y;
    }
    Some(EntityType::LwPolyline(new_poly))
}

/// Extend a Line to the nearest boundary on the extended side.
/// t_click < 0.5 → extend start (look for t < 0); t_click ≥ 0.5 → extend end (t > 1).
fn extend_line(orig: &LineEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let ax = orig.start.x;
    let ay = orig.start.y;
    let bx = orig.end.x;
    let by = orig.end.y;
    let (dx, dy) = (bx - ax, by - ay);
    let target = orig.common.handle;
    let extend_end = t_click >= 0.5;

    let mut best_t = if extend_end {
        f64::INFINITY
    } else {
        f64::NEG_INFINITY
    };

    for geo in geos {
        match geo {
            Geo::Line { handle, p1, p2 } => {
                if *handle == target {
                    continue;
                }
                let (ex, ey) = (p2[0] - p1[0], p2[1] - p1[1]);
                if let Some((t, u)) = ll(ax, ay, dx, dy, p1[0], p1[1], ex, ey) {
                    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
                        continue;
                    }
                    if extend_end && t > 1.0 + 1e-6 && t < best_t {
                        best_t = t;
                    }
                    if !extend_end && t < -1e-6 && t > best_t {
                        best_t = t;
                    }
                }
            }
            Geo::Arc {
                handle,
                cx,
                cy,
                r,
                a0,
                a1,
            } => {
                if *handle == target {
                    continue;
                }
                for t in lc(ax, ay, dx, dy, *cx, *cy, *r) {
                    let ix = ax + t * dx;
                    let iy = ay + t * dy;
                    if !in_arc((iy - cy).atan2(ix - cx), *a0, *a1) {
                        continue;
                    }
                    if extend_end && t > 1.0 + 1e-6 && t < best_t {
                        best_t = t;
                    }
                    if !extend_end && t < -1e-6 && t > best_t {
                        best_t = t;
                    }
                }
            }
            Geo::Circle { handle, cx, cy, r } => {
                if *handle == target {
                    continue;
                }
                for t in lc(ax, ay, dx, dy, *cx, *cy, *r) {
                    if extend_end && t > 1.0 + 1e-6 && t < best_t {
                        best_t = t;
                    }
                    if !extend_end && t < -1e-6 && t > best_t {
                        best_t = t;
                    }
                }
            }
            Geo::Ray { handle, bx: rbx, by: rby, dx: rdx, dy: rdy } => {
                if *handle == target { continue; }
                if let Some((t, u)) = ll(ax, ay, dx, dy, *rbx, *rby, *rdx, *rdy) {
                    if u >= -1e-9 { // only forward along the Ray
                        if extend_end && t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                        if !extend_end && t < -1e-6 && t > best_t { best_t = t; }
                    }
                }
            }
            Geo::InfLine { handle, bx: ibx, by: iby, dx: idx, dy: idy } => {
                if *handle == target { continue; }
                if let Some((t, _u)) = ll(ax, ay, dx, dy, *ibx, *iby, *idx, *idy) {
                    if extend_end && t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                    if !extend_end && t < -1e-6 && t > best_t { best_t = t; }
                }
            }
            Geo::Ellipse { handle, cx: ecx, cy: ecy, a, b, nx, ny, t0: et0, t1: et1 } => {
                if *handle == target { continue; }
                for (t, t_ell) in le(ax, ay, dx, dy, *ecx, *ecy, *a, *b, *nx, *ny) {
                    if !in_arc(t_ell, *et0, *et1) { continue; }
                    if extend_end && t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                    if !extend_end && t < -1e-6 && t > best_t { best_t = t; }
                }
            }
            Geo::Spline { handle, segs } => {
                if *handle == target { continue; }
                for (p1, p2) in segs {
                    let ex = p2[0] - p1[0];
                    let ey = p2[1] - p1[1];
                    if let Some((t, u)) = ll(ax, ay, dx, dy, p1[0], p1[1], ex, ey) {
                        if !(-1e-9..=1.0 + 1e-9).contains(&u) { continue; }
                        if extend_end && t > 1.0 + 1e-6 && t < best_t { best_t = t; }
                        if !extend_end && t < -1e-6 && t > best_t { best_t = t; }
                    }
                }
            }
        }
    }

    if !best_t.is_finite() {
        return None;
    }
    let mut line = orig.clone();
    line.common.handle = Handle::NULL;
    let new_x = ax + best_t * dx;
    let new_y = ay + best_t * dy;
    if extend_end {
        line.end = Vector3::new(new_x, new_y, orig.end.z);
    } else {
        line.start = Vector3::new(new_x, new_y, orig.start.z);
    }
    Some(EntityType::Line(line))
}

/// Trim a Ray entity.
/// Virtual t ∈ [0,1]: t=0 → base_point, t=1 → base + TRIM_EXTENT * dir.
/// Surviving pieces become Lines (finite) or Rays (still semi-infinite).
fn trim_ray(orig: &RayEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bx = orig.base_point.x;
    let by = orig.base_point.y;
    let bz = orig.base_point.z;
    let dx = orig.direction.x;
    let dy = orig.direction.y;
    let dz = orig.direction.z;
    let pt = |t: f64| [bx + t * dx * TRIM_EXTENT, by + t * dy * TRIM_EXTENT, bz + t * dz * TRIM_EXTENT];

    trim_intervals(ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let pa = pt(ta);
            let pb = pt(tb);
            if (pb[0] - pa[0]).hypot(pb[1] - pa[1]) < 1e-6 { return None; }

            if tb > INF_T {
                // Still extends to infinity → remains a Ray with new base
                let r = RayEnt::new(
                    Vector3::new(pa[0], pa[1], pa[2]),
                    Vector3::new(dx, dy, dz),
                );
                let mut r = r;
                r.common = orig.common.clone();
                r.common.handle = Handle::NULL;
                Some(EntityType::Ray(r))
            } else {
                // Finite segment → Line
                let mut l = LineEnt { common: orig.common.clone(), ..LineEnt::new() };
                l.common.handle = Handle::NULL;
                l.start = Vector3::new(pa[0], pa[1], pa[2]);
                l.end = Vector3::new(pb[0], pb[1], pb[2]);
                Some(EntityType::Line(l))
            }
        })
        .collect()
}

/// Trim an XLine entity.
/// Virtual t ∈ [0,1]: t=0 → base - dir*TRIM_EXTENT, t=0.5 → base, t=1 → base + dir*TRIM_EXTENT.
/// Surviving pieces become Lines (finite), Rays (one infinite end), or the original XLine (both ends).
fn trim_xline(orig: &XLineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bx = orig.base_point.x;
    let by = orig.base_point.y;
    let bz = orig.base_point.z;
    let dx = orig.direction.x;
    let dy = orig.direction.y;
    let dz = orig.direction.z;
    // Point at virtual t: scale factor s = 2t - 1 ∈ [-1, +1]
    let pt = |t: f64| {
        let s = 2.0 * t - 1.0;
        [bx + s * dx * TRIM_EXTENT, by + s * dy * TRIM_EXTENT, bz + s * dz * TRIM_EXTENT]
    };

    trim_intervals(ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let pa = pt(ta);
            let pb = pt(tb);
            let ext_neg = ta < 1.0 - INF_T; // extends toward -infinity
            let ext_pos = tb > INF_T;        // extends toward +infinity

            match (ext_neg, ext_pos) {
                (true, true) => {
                    // Whole XLine survived (shouldn't happen after a real trim)
                    let mut x = orig.clone();
                    x.common.handle = Handle::NULL;
                    Some(EntityType::XLine(x))
                }
                (true, false) => {
                    // Extends toward -infinity: Ray at pb pointing in -dir
                    let r = RayEnt::new(
                        Vector3::new(pb[0], pb[1], pb[2]),
                        Vector3::new(-dx, -dy, -dz),
                    );
                    let mut r = r;
                    r.common = orig.common.clone();
                    r.common.handle = Handle::NULL;
                    Some(EntityType::Ray(r))
                }
                (false, true) => {
                    // Extends toward +infinity: Ray at pa pointing in +dir
                    let r = RayEnt::new(
                        Vector3::new(pa[0], pa[1], pa[2]),
                        Vector3::new(dx, dy, dz),
                    );
                    let mut r = r;
                    r.common = orig.common.clone();
                    r.common.handle = Handle::NULL;
                    Some(EntityType::Ray(r))
                }
                (false, false) => {
                    // Finite segment
                    let mut l = LineEnt { common: orig.common.clone(), ..LineEnt::new() };
                    l.common.handle = Handle::NULL;
                    l.start = Vector3::new(pa[0], pa[1], pa[2]);
                    l.end = Vector3::new(pb[0], pb[1], pb[2]);
                    Some(EntityType::Line(l))
                }
            }
        })
        .collect()
}

// ── Point-generation helpers ──────────────────────────────────────────────

const DIM_RED: [f32; 4] = [1.0, 0.3, 0.3, 0.6];

fn line_pts(l: &LineEnt) -> Vec<[f32; 3]> {
    vec![
        [l.start.x as f32, l.start.y as f32, l.start.z as f32],
        [l.end.x as f32, l.end.y as f32, l.end.z as f32],
    ]
}

fn arc_pts(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, y: f64) -> Vec<[f32; 3]> {
    let span = {
        let s = norm(a1) - norm(a0);
        if s <= 0.0 {
            s + TAU
        } else {
            s
        }
    };
    let steps = (span.abs() * 20.0).ceil().max(4.0) as usize;
    (0..=steps)
        .map(|i| {
            let ang = norm(a0) + span * (i as f64 / steps as f64);
            [
                (cx + r * ang.cos()) as f32,
                y as f32,
                (cy + r * ang.sin()) as f32,
            ]
        })
        .collect()
}

fn entity_pts(e: &EntityType) -> Vec<[f32; 3]> {
    match e {
        EntityType::Line(l) => line_pts(l),
        EntityType::Arc(a) => arc_pts(
            a.center.x,
            a.center.y,
            a.radius,
            a.start_angle,
            a.end_angle,
            a.center.y,
        ),
        EntityType::Ellipse(e) => {
            let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
            if a < 1e-9 { return vec![]; }
            let b = a * e.minor_axis_ratio;
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 { t1 += TAU; }
            ellipse_pts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, e.center.z)
        }
        EntityType::Spline(s) => spline_pts_wire(s),
        EntityType::LwPolyline(p) => {
            let elev = p.elevation as f32;
            let n = p.vertices.len();
            let seg_count = if p.is_closed { n } else { n.saturating_sub(1) };
            let mut pts = Vec::with_capacity(seg_count * 2);
            for i in 0..seg_count {
                let v0 = &p.vertices[i];
                let v1 = &p.vertices[(i + 1) % n];
                pts.push([v0.location.x as f32, elev, v0.location.y as f32]);
                pts.push([v1.location.x as f32, elev, v1.location.y as f32]);
            }
            pts
        }
        // For preview, show a 20-unit section of semi-infinite results
        EntityType::Ray(r) => {
            let bx = r.base_point.x;
            let by = r.base_point.y;
            let bz = r.base_point.z;
            let far_x = bx + r.direction.x * 20.0;
            let far_y = by + r.direction.y * 20.0;
            let far_z = bz + r.direction.z * 20.0;
            vec![[bx as f32, bz as f32, by as f32], [far_x as f32, far_z as f32, far_y as f32]]
        }
        _ => vec![],
    }
}

// ══════════════════════════════════════════════════════════════════════════
// TrimCommand
// ══════════════════════════════════════════════════════════════════════════

pub struct TrimCommand {
    all_entities: Vec<EntityType>,
    geos: Vec<Geo>,
}

impl TrimCommand {
    pub fn new(all_entities: Vec<EntityType>) -> Self {
        let geos = build_geos(&all_entities);
        Self { all_entities, geos }
    }
}

impl CadCommand for TrimCommand {
    fn name(&self) -> &'static str {
        "TRIM"
    }

    fn prompt(&self) -> String {
        "TRIM  Click segment to remove  [Enter=done]:".into()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: Vec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }

        let entity = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle);

        let result: Option<Vec<EntityType>> = match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let ts = line_seg_ts(ax, ay, bx, by, handle, &self.geos);
                if ts.is_empty() {
                    return CmdResult::NeedPoint;
                }
                let dx = bx - ax;
                let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - ax) * dx + (pt.y as f64 - ay) * dy) / len2
                } else {
                    0.5
                };
                Some(trim_line(l, &ts, t_click))
            }
            Some(EntityType::Arc(a)) => {
                let cx = a.center.x;
                let cy = a.center.y;
                let a0 = a.start_angle;
                let a1 = a.end_angle;
                let ts = arc_seg_ts(cx, cy, a.radius, a0, a1, handle, &self.geos);
                if ts.is_empty() {
                    return CmdResult::NeedPoint;
                }
                let click_angle = (pt.y as f64 - cy).atan2(pt.x as f64 - cx);
                let t_click = arc_t(click_angle, a0, a1);
                Some(trim_arc(a, &ts, t_click))
            }
            Some(EntityType::Ray(r)) => {
                // Virtual segment: base → base + dir * TRIM_EXTENT (t ∈ [0,1])
                let bx = r.base_point.x;
                let by = r.base_point.y;
                let ex = bx + r.direction.x * TRIM_EXTENT;
                let ey = by + r.direction.y * TRIM_EXTENT;
                let ts = line_seg_ts(bx, by, ex, ey, handle, &self.geos);
                if ts.is_empty() { return CmdResult::NeedPoint; }
                let dx = r.direction.x * TRIM_EXTENT;
                let dy = r.direction.y * TRIM_EXTENT;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - bx) * dx + (pt.y as f64 - by) * dy) / len2
                } else { 0.5 };
                Some(trim_ray(r, &ts, t_click))
            }
            Some(EntityType::XLine(x)) => {
                // Virtual segment: base - dir*TRIM_EXTENT → base + dir*TRIM_EXTENT
                let bx = x.base_point.x - x.direction.x * TRIM_EXTENT;
                let by = x.base_point.y - x.direction.y * TRIM_EXTENT;
                let ex = x.base_point.x + x.direction.x * TRIM_EXTENT;
                let ey = x.base_point.y + x.direction.y * TRIM_EXTENT;
                let ts = line_seg_ts(bx, by, ex, ey, handle, &self.geos);
                if ts.is_empty() { return CmdResult::NeedPoint; }
                let dx = ex - bx;
                let dy = ey - by;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - bx) * dx + (pt.y as f64 - by) * dy) / len2
                } else { 0.5 };
                Some(trim_xline(x, &ts, t_click))
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 { return CmdResult::NeedPoint; }
                let b = a * e.minor_axis_ratio;
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 { t1 += TAU; }
                let ts = ellipse_seg_ts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, &self.geos);
                if ts.is_empty() { return CmdResult::NeedPoint; }
                // t_click: project mouse onto ellipse local param
                let rx = pt.x as f64 - e.center.x;
                let ry = pt.y as f64 - e.center.y;
                let xl =  rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_ell = yl.atan2(xl);
                let t_click = arc_t(t_ell, t0, t1);
                Some(trim_ellipse(e, &ts, t_click))
            }
            Some(EntityType::Spline(s)) => {
                let ts = spline_seg_ts(s, handle, &self.geos);
                if ts.is_empty() { return CmdResult::NeedPoint; }
                let t_click = spline_nearest_t(s, pt.x as f64, pt.y as f64)
                    .and_then(|t_actual| {
                        let bs = spline_to_bspline(s)?;
                        let (t0, t1) = bs.range_tuple();
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                Some(trim_spline(s, &ts, t_click))
            }
            _ => None,
        };

        if let Some(new_entities) = result {
            // Snapshot is updated in on_entity_replaced once we know the real handles.
            // Pre-stage: remove old entry now so geos exclude it immediately.
            if let Some(pos) = self
                .all_entities
                .iter()
                .position(|e| e.common().handle == handle)
            {
                self.all_entities.remove(pos);
                // Add pieces with NULL handles as geometry-only placeholders.
                self.all_entities.extend(new_entities.clone());
                self.geos = build_geos(&self.all_entities);
            }
            CmdResult::ReplaceEntity(handle, new_entities)
        } else {
            self.command_line_hint();
            CmdResult::NeedPoint
        }
    }

    fn on_entity_replaced(&mut self, _old: Handle, new_handles: &[acadrust::Handle]) {
        // The last new_handles.len() entries in all_entities are the trimmed pieces
        // that were appended with NULL handles. Assign their real document handles.
        let start = self.all_entities.len().saturating_sub(new_handles.len());
        for (e, &h) in self.all_entities[start..]
            .iter_mut()
            .zip(new_handles.iter())
        {
            match e {
                EntityType::Line(l) => l.common.handle = h,
                EntityType::Arc(a) => a.common.handle = h,
                EntityType::Ray(r) => r.common.handle = h,
                EntityType::XLine(x) => x.common.handle = h,
                EntityType::Ellipse(e) => e.common.handle = h,
                EntityType::Spline(s) => s.common.handle = h,
                _ => {}
            }
        }
        self.geos = build_geos(&self.all_entities);
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: Vec3) -> Vec<WireModel> {
        if handle.is_null() {
            return vec![];
        }

        let entity = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle);

        match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let ts = line_seg_ts(ax, ay, bx, by, handle, &self.geos);
                if ts.is_empty() {
                    return vec![];
                }
                let dx = bx - ax;
                let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - ax) * dx + (pt.y as f64 - ay) * dy) / len2
                } else {
                    0.5
                };
                let survivors = trim_line(l, &ts, t_click);
                let p1 = [l.start.x as f32, l.start.y as f32, l.start.y as f32];
                let p2 = [l.end.x as f32, l.end.y as f32, l.end.y as f32];
                let removed = WireModel::solid("trim_rm".into(), vec![p1, p2], DIM_RED, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Arc(a)) => {
                let cx = a.center.x;
                let cy = a.center.y;
                let a0 = a.start_angle;
                let a1 = a.end_angle;
                let ts = arc_seg_ts(cx, cy, a.radius, a0, a1, handle, &self.geos);
                if ts.is_empty() {
                    return vec![];
                }
                let click_angle = (pt.y as f64 - cy).atan2(pt.x as f64 - cx);
                let t_click = arc_t(click_angle, a0, a1);
                let survivors = trim_arc(a, &ts, t_click);
                let orig_pts = arc_pts(cx, cy, a.radius, a0, a1, a.center.y);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, DIM_RED, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Ray(r)) => {
                let bx = r.base_point.x;
                let by = r.base_point.y;
                let ex = bx + r.direction.x * TRIM_EXTENT;
                let ey = by + r.direction.y * TRIM_EXTENT;
                let ts = line_seg_ts(bx, by, ex, ey, handle, &self.geos);
                if ts.is_empty() { return vec![]; }
                let dx = r.direction.x * TRIM_EXTENT;
                let dy = r.direction.y * TRIM_EXTENT;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - bx) * dx + (pt.y as f64 - by) * dy) / len2
                } else { 0.5 };
                let survivors = trim_ray(r, &ts, t_click);
                // Show a finite preview section (20 units) for the original ray
                let far = [(bx + r.direction.x * 20.0) as f32, (by + r.direction.y * 20.0) as f32, r.base_point.z as f32];
                let base = [bx as f32, by as f32, r.base_point.z as f32];
                let removed = WireModel::solid("trim_rm".into(), vec![base, far], DIM_RED, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(format!("trim_keep_{i}"), pts, WireModel::CYAN, false));
                }
                out
            }
            Some(EntityType::XLine(x)) => {
                let bx = x.base_point.x;
                let by = x.base_point.y;
                let ex_start = bx - x.direction.x * TRIM_EXTENT;
                let ey_start = by - x.direction.y * TRIM_EXTENT;
                let ex_end = bx + x.direction.x * TRIM_EXTENT;
                let ey_end = by + x.direction.y * TRIM_EXTENT;
                let ts = line_seg_ts(ex_start, ey_start, ex_end, ey_end, handle, &self.geos);
                if ts.is_empty() { return vec![]; }
                let dx = ex_end - ex_start;
                let dy = ey_end - ey_start;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - ex_start) * dx + (pt.y as f64 - ey_start) * dy) / len2
                } else { 0.5 };
                let survivors = trim_xline(x, &ts, t_click);
                let neg = [(bx - x.direction.x * 20.0) as f32, (by - x.direction.y * 20.0) as f32, x.base_point.z as f32];
                let pos_pt = [(bx + x.direction.x * 20.0) as f32, (by + x.direction.y * 20.0) as f32, x.base_point.z as f32];
                let removed = WireModel::solid("trim_rm".into(), vec![neg, pos_pt], DIM_RED, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(format!("trim_keep_{i}"), pts, WireModel::CYAN, false));
                }
                out
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 { return vec![]; }
                let b = a * e.minor_axis_ratio;
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 { t1 += TAU; }
                let ts = ellipse_seg_ts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, &self.geos);
                if ts.is_empty() { return vec![]; }
                let rx = pt.x as f64 - e.center.x;
                let ry = pt.y as f64 - e.center.y;
                let xl =  rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_click = arc_t(yl.atan2(xl), t0, t1);
                let survivors = trim_ellipse(e, &ts, t_click);
                let orig_pts = ellipse_pts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, e.center.z);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, DIM_RED, false);
                let mut out = vec![removed];
                for (i, ent) in survivors.iter().enumerate() {
                    let pts = entity_pts(ent);
                    out.push(WireModel::solid(format!("trim_keep_{i}"), pts, WireModel::CYAN, false));
                }
                out
            }
            Some(EntityType::Spline(s)) => {
                let ts = spline_seg_ts(s, handle, &self.geos);
                if ts.is_empty() { return vec![]; }
                let t_click = spline_nearest_t(s, pt.x as f64, pt.y as f64)
                    .and_then(|t_actual| {
                        let bs = spline_to_bspline(s)?;
                        let (t0, t1) = bs.range_tuple();
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                let orig_pts = spline_pts_wire(s);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, DIM_RED, false);
                let survivors = trim_spline(s, &ts, t_click);
                let mut out = vec![removed];
                for (i, ent) in survivors.iter().enumerate() {
                    let pts = entity_pts(ent);
                    out.push(WireModel::solid(format!("trim_keep_{i}"), pts, WireModel::CYAN, false));
                }
                out
            }
            _ => vec![],
        }
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

impl TrimCommand {
    fn command_line_hint(&self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// ExtendCommand
// ══════════════════════════════════════════════════════════════════════════

pub struct ExtendCommand {
    all_entities: Vec<EntityType>,
    geos: Vec<Geo>,
    /// (old_handle, new_entity_with_updated_geometry) — set in on_entity_pick,
    /// consumed in on_entity_replaced to patch the snapshot with both new handle + geometry.
    pending_replace: Option<(Handle, EntityType)>,
}

impl ExtendCommand {
    pub fn new(all_entities: Vec<EntityType>) -> Self {
        let geos = build_geos(&all_entities);
        Self {
            all_entities,
            geos,
            pending_replace: None,
        }
    }
}

impl CadCommand for ExtendCommand {
    fn name(&self) -> &'static str {
        "EXTEND"
    }

    fn prompt(&self) -> String {
        "EXTEND  Click near end of object to extend  [Enter=done]:".into()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: Vec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }

        let entity = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle);

        let result: Option<EntityType> = match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let dx = bx - ax;
                let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - ax) * dx + (pt.y as f64 - ay) * dy) / len2
                } else {
                    0.5
                };
                extend_line(l, t_click, &self.geos)
            }
            Some(EntityType::Ellipse(e)) => {
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 { t1 += TAU; }
                let span = t1 - t0;
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 { return CmdResult::NeedPoint; }
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let rx = pt.x as f64 - e.center.x;
                let ry = pt.y as f64 - e.center.y;
                let xl =  rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_click = arc_t(yl.atan2(xl), t0, t1);
                let _ = span;
                extend_ellipse(e, t_click, &self.geos)
            }
            Some(EntityType::LwPolyline(p)) => {
                extend_lwpoly(p, pt.x as f64, pt.y as f64, &self.geos)
            }
            Some(EntityType::Spline(s)) => {
                let t_click = spline_nearest_t(s, pt.x as f64, pt.y as f64)
                    .and_then(|t_actual| {
                        let bs = spline_to_bspline(s)?;
                        let (t0, t1) = bs.range_tuple();
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                extend_spline(s, t_click, &self.geos)
            }
            _ => None,
        };

        if let Some(new_entity) = result {
            // Save the extended entity so on_entity_replaced can patch the snapshot
            // with both the new geometry and the real document handle.
            self.pending_replace = Some((handle, new_entity.clone()));
            CmdResult::ReplaceEntity(handle, vec![new_entity])
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[acadrust::Handle]) {
        if let (Some(&new_handle), Some((pending_old, mut new_entity))) =
            (new_handles.first(), self.pending_replace.take())
        {
            if pending_old == old {
                // Update the snapshot entry: replace geometry + assign real handle.
                match &mut new_entity {
                    EntityType::Line(l) => l.common.handle = new_handle,
                    EntityType::Ellipse(e) => e.common.handle = new_handle,
                    EntityType::Spline(s) => s.common.handle = new_handle,
                    EntityType::LwPolyline(p) => p.common.handle = new_handle,
                    _ => {}
                }
                if let Some(pos) = self
                    .all_entities
                    .iter()
                    .position(|e| e.common().handle == old)
                {
                    self.all_entities[pos] = new_entity;
                }
                self.geos = build_geos(&self.all_entities);
            }
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: Vec3) -> Vec<WireModel> {
        if handle.is_null() {
            return vec![];
        }

        let entity = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle);
        match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x; let ay = l.start.y;
                let bx = l.end.x;   let by = l.end.y;
                let dx = bx - ax;   let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x as f64 - ax) * dx + (pt.y as f64 - ay) * dy) / len2
                } else { 0.5 };
                if let Some(ext) = extend_line(l, t_click, &self.geos) {
                    return vec![WireModel::solid("extend_prev".into(), entity_pts(&ext), WireModel::CYAN, false)];
                }
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a >= 1e-9 {
                    let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                    let t0 = e.start_parameter;
                    let mut t1 = e.end_parameter;
                    if t1 <= t0 { t1 += TAU; }
                    let rx = pt.x as f64 - e.center.x;
                    let ry = pt.y as f64 - e.center.y;
                    let xl =  rx * nx + ry * ny;
                    let yl = -rx * ny + ry * nx;
                    let t_click = arc_t(yl.atan2(xl), t0, t1);
                    if let Some(ext) = extend_ellipse(e, t_click, &self.geos) {
                        return vec![WireModel::solid("extend_prev".into(), entity_pts(&ext), WireModel::CYAN, false)];
                    }
                }
            }
            Some(EntityType::LwPolyline(p)) => {
                if let Some(ext) = extend_lwpoly(p, pt.x as f64, pt.y as f64, &self.geos) {
                    return vec![WireModel::solid("extend_prev".into(), entity_pts(&ext), WireModel::CYAN, false)];
                }
            }
            Some(EntityType::Spline(s)) => {
                let t_click = spline_nearest_t(s, pt.x as f64, pt.y as f64)
                    .and_then(|t_actual| {
                        let bs = spline_to_bspline(s)?;
                        let (t0, t1) = bs.range_tuple();
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                if let Some(ext) = extend_spline(s, t_click, &self.geos) {
                    return vec![WireModel::solid("extend_prev".into(), entity_pts(&ext), WireModel::CYAN, false)];
                }
            }
            _ => {}
        }
        vec![]
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
