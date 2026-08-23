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

// The plane geometry these commands run on lives in cadkernel; `geom` adapts
// its call shapes to the loose scalars and f32 render vertices used here.
use super::geom;
use super::geom::{
    arc_parameter as arc_t, arc_points as arc_pts, ellipse_points as ellipse_pts,
    lerp as lerp2, normalize_angle as norm,
};

use crate::modules::draw::fence::{crossing_box_preview, FencePick};

use acadrust::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline,
    LwVertex, Ray as RayEnt, Spline as SplineEnt, XLine as XLineEnt,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use cadkernel::geom2d::nurbs::clamped_uniform_knots;
use cadkernel::geom2d::{
    intersect as kernel_intersect, trim_spans as kernel_trim_spans, Arc as KernelArc,
    Circle as KernelCircle, Curve, Extent as KernelExtent,
    Ellipse as KernelEllipse, EllipseArc as KernelEllipseArc, Line as KernelLine,
    NurbsCurve, Ray as KernelRay, Tolerance as KernelTolerance, XLine as KernelXLine,
};

use crate::entities::curve::{entity_curve_xy, entity_with_lwpolyline_world_xy};

use crate::command::{CadCommand, CmdResult};
use crate::modules::draw::modify::spline_ops::{
    spline_cut, spline_nearest_t, spline_pts_wire, spline_range, spline_to_nurbs, t_to_rel,
};
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
use crate::t;

use super::entity_index::ModifyEntityIndex;

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

// ── Boundary geometry ─────────────────────────────────────────────────────

/// Virtual extent used to represent infinite ends of Ray / XLine.
const TRIM_EXTENT: f64 = 1_000_000.0;

/// Sampling density for the plan-view point lists the fence and preview
/// passes walk. The renderer's own figure, so a preview cut lands where the
/// drawn geometry is rather than a chord away from it.
const SAMPLE_SEGMENTS_PER_RADIAN: f64 = cadkernel::geom2d::DEFAULT_SEGMENTS_PER_RADIAN;
/// If a trim interval endpoint is beyond this threshold it is treated as "infinite".

#[derive(Clone)]
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
    /// A NURBS boundary, carried exactly.
    Spline {
        handle: Handle,
        curve: Box<NurbsCurve>,
    },
}

impl Geo {
    fn handle(&self) -> Handle {
        match self {
            Self::Line { handle, .. }
            | Self::Arc { handle, .. }
            | Self::Circle { handle, .. }
            | Self::Ray { handle, .. }
            | Self::InfLine { handle, .. }
            | Self::Ellipse { handle, .. }
            | Self::Spline { handle, .. } => *handle,
        }
    }
}

fn build_geos(entities: &[EntityType]) -> Vec<Geo> {
    let mut out = Vec::new();
    for e in entities {
        let h = e.common().handle;
        match e {
            // A polyline acts as a boundary through its constituent edges, so
            // a Line/Arc/… can be trimmed against it. Explode into Line + Arc
            // segments and tag each with the polyline's own handle (so trim
            // still excludes it as the click target).
            EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_) => {
                for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
                    if let Some(g) = geo_from_entity(h, &seg) {
                        out.push(g);
                    }
                }
            }
            _ => {
                if let Some(g) = geo_from_entity(h, e) {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// Convert a simple boundary entity (Line / Arc / Circle / Ray / XLine /
/// Ellipse / Spline) into a `Geo`, tagged with `h`. Returns `None` for types
/// that do not act as trim boundaries.
fn geo_from_entity(h: Handle, e: &EntityType) -> Option<Geo> {
    // The geometry itself comes from the one converter, in plan-view
    // coordinates. What is left here is only the shape of the boundary
    // record, which the edit options need to mutate — `imply_edge_geos` turns
    // a segment into an infinite line and opens an arc into a full circle,
    // neither of which a bare curve says.
    match entity_curve_xy(e)? {
        Curve::Line(line) => Some(Geo::Line {
            handle: h,
            p1: line.start,
            p2: line.end,
        }),
        Curve::Arc(arc) => Some(Geo::Arc {
            handle: h,
            cx: arc.centre[0],
            cy: arc.centre[1],
            r: arc.radius,
            a0: arc.start_angle,
            a1: arc.end_angle,
        }),
        Curve::Circle(circle) => Some(Geo::Circle {
            handle: h,
            cx: circle.centre[0],
            cy: circle.centre[1],
            r: circle.radius,
        }),
        Curve::Ray(ray) => Some(Geo::Ray {
            handle: h,
            bx: ray.origin[0],
            by: ray.origin[1],
            dx: ray.direction[0],
            dy: ray.direction[1],
        }),
        Curve::XLine(line) => Some(Geo::InfLine {
            handle: h,
            bx: line.base[0],
            by: line.base[1],
            dx: line.direction[0],
            dy: line.direction[1],
        }),
        Curve::Ellipse(arc) => {
            let ellipse = arc.ellipse;
            if ellipse.major_radius < 1e-9 {
                return None;
            }
            let t0 = arc.start_parameter;
            let mut t1 = arc.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
            Some(Geo::Ellipse {
                handle: h,
                cx: ellipse.centre[0],
                cy: ellipse.centre[1],
                a: ellipse.major_radius,
                b: ellipse.minor_radius,
                nx: ellipse.major_axis[0],
                ny: ellipse.major_axis[1],
                t0,
                t1,
            })
        }
        Curve::Nurbs(curve) => Some(Geo::Spline {
            handle: h,
            curve: Box::new(curve),
        }),
        // A polyline reaches here already taken apart into its segments.
        Curve::Polyline(_) => None,
    }
}

// ── Intersection helpers ──────────────────────────────────────────────────

// ── Boundary crossings ────────────────────────────────────────────────────
//
// Every cut a trim or extend makes comes from one question: where does the
// entity being edited meet the boundaries picked as cutting edges. The kernel
// answers it for any pair of curves, so these four just describe the target as
// a `Curve` and hand it over.

/// The entity a boundary came from, so a curve is never cut by itself.
fn geo_handle(geo: &Geo) -> Handle {
    match geo {
        Geo::Line { handle, .. }
        | Geo::Arc { handle, .. }
        | Geo::Circle { handle, .. }
        | Geo::Ray { handle, .. }
        | Geo::InfLine { handle, .. }
        | Geo::Ellipse { handle, .. }
        | Geo::Spline { handle, .. } => *handle,
    }
}

/// A boundary as a kernel curve.
fn geo_to_curve(geo: &Geo) -> Option<Curve> {
    Some(match geo {
        Geo::Line { p1, p2, .. } => Curve::Line(KernelLine {
            start: *p1,
            end: *p2,
        }),
        Geo::Arc { cx, cy, r, a0, a1, .. } => Curve::Arc(KernelArc {
            centre: [*cx, *cy],
            radius: *r,
            start_angle: *a0,
            end_angle: *a1,
        }),
        Geo::Circle { cx, cy, r, .. } => Curve::Circle(KernelCircle {
            centre: [*cx, *cy],
            radius: *r,
        }),
        Geo::Ray { bx, by, dx, dy, .. } => Curve::Ray(KernelRay {
            origin: [*bx, *by],
            direction: [*dx, *dy],
        }),
        Geo::InfLine { bx, by, dx, dy, .. } => Curve::XLine(KernelXLine {
            base: [*bx, *by],
            direction: [*dx, *dy],
        }),
        Geo::Ellipse {
            cx, cy, a, b, nx, ny, t0, t1, ..
        } => Curve::Ellipse(KernelEllipseArc {
            ellipse: KernelEllipse {
                centre: [*cx, *cy],
                major_radius: *a,
                minor_radius: *b,
                major_axis: [*nx, *ny],
            },
            start_parameter: *t0,
            end_parameter: *t1,
        }),
        // Exactly, not through its sampling. The kernel intersects a NURBS by
        // subdivision against a bounding box, so a boundary spline cuts where
        // it actually runs rather than where a 64-chord approximation of it
        // does.
        Geo::Spline { curve, .. } => Curve::Nurbs((**curve).clone()),
    })
}

/// Where `target` is cut by every boundary except its own entity, as
/// parameters along it in `0..=1`.
fn cut_params(target: &Curve, handle: Handle, geos: &[Geo]) -> Vec<f64> {
    let tolerance = KernelTolerance::new(CUT_TOLERANCE);
    // A bounded target's parameters are pinned to its own span, as every
    // caller here expects. An unbounded one — a ray shot out to find the
    // nearest boundary — has to keep whatever it comes back with, since the
    // whole point is how far away the crossing is.
    let bounded = matches!(target.extent(), KernelExtent::Bounded);
    let mut ts: Vec<f64> = geos
        .iter()
        .filter(|geo| geo_handle(geo) != handle)
        .filter_map(geo_to_curve)
        .flat_map(|boundary| {
            kernel_intersect(target, &boundary, tolerance)
                .into_iter()
                .map(|hit| if bounded { hit.t_a.clamp(0.0, 1.0) } else { hit.t_a })
                .collect::<Vec<_>>()
        })
        .collect();
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    ts
}

/// A Ray entity as the kernel curve it is, so its parameter is the ray's own
/// rather than a fraction of some invented length.
fn ray_curve(r: &RayEnt) -> Curve {
    Curve::Ray(KernelRay {
        origin: [r.base_point.x, r.base_point.y],
        direction: [r.direction.x, r.direction.y],
    })
}

/// An XLine entity as the kernel curve it is.
fn xline_curve(x: &XLineEnt) -> Curve {
    Curve::XLine(KernelXLine {
        base: [x.base_point.x, x.base_point.y],
        direction: [x.direction.x, x.direction.y],
    })
}

/// How close two points have to be to count as a crossing.
///
/// Loose enough that a boundary drawn to meet an entity still registers when
/// its endpoint is a rounding step away, which is the usual case in a drawing
/// that was snapped together rather than computed.
const CUT_TOLERANCE: f64 = 1e-7;

fn line_seg_ts(ax: f64, ay: f64, bx: f64, by: f64, target: Handle, geos: &[Geo]) -> Vec<f64> {
    cut_params(
        &Curve::Line(KernelLine {
            start: [ax, ay],
            end: [bx, by],
        }),
        target,
        geos,
    )
}

fn arc_seg_ts(
    cx: f64,
    cy: f64,
    r: f64,
    a0: f64,
    a1: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    cut_params(
        &Curve::Arc(KernelArc {
            centre: [cx, cy],
            radius: r,
            start_angle: a0,
            end_angle: a1,
        }),
        target,
        geos,
    )
}

#[allow(clippy::too_many_arguments)]
fn ellipse_seg_ts(
    cx: f64,
    cy: f64,
    a: f64,
    b: f64,
    nx: f64,
    ny: f64,
    t0: f64,
    t1: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    cut_params(
        &Curve::Ellipse(KernelEllipseArc {
            ellipse: KernelEllipse {
                centre: [cx, cy],
                major_radius: a,
                minor_radius: b,
                major_axis: [nx, ny],
            },
            start_parameter: t0,
            end_parameter: t1,
        }),
        target,
        geos,
    )
}

/// Cuts on a spline, solved against the true curve rather than a sampling of
/// it. The kernel subdivides and converges, so a cut lands where the boundary
/// actually crosses instead of where a fixed number of samples said it did.
fn spline_seg_ts(spl: &SplineEnt, target: Handle, geos: &[Geo]) -> Vec<f64> {
    let Some(curve) = spline_to_nurbs(spl) else {
        return vec![];
    };
    cut_params(&Curve::Nurbs(curve), target, geos)
}

/// Trim an Ellipse entity. Returns the surviving ellipse-arc segments.
fn trim_ellipse(orig: &EllipseEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let t0 = orig.start_parameter;
    let mut t1 = orig.end_parameter;
    if t1 <= t0 {
        t1 += TAU;
    }
    let span = t1 - t0;
    let angle_at = |t: f64| t0 + span * t;

    // A closed ellipse has no ends for `trim_intervals` to anchor on. Handing
    // it one makes it invent cuts at the parameter seam, so the survivor comes
    // back as two arcs either side of the seam instead of one joined across
    // it — and if the click lands in the wrapping piece, the wrong side goes.
    // Circles avoid this by treating the cuts cyclically; see `trim_circle`.
    if (span - TAU).abs() < 1e-9 {
        if ts.len() < 2 {
            return vec![];
        }
        let click = t_click.rem_euclid(1.0);
        // The gap holding the click, with the last one wrapping past 1.0 back
        // to the first cut.
        let gap = (0..ts.len()).find_map(|i| {
            let from = ts[i];
            let to = if i + 1 < ts.len() {
                ts[i + 1]
            } else {
                ts[0] + 1.0
            };
            let holds = |t: f64| t >= from - 1e-9 && t <= to + 1e-9;
            (holds(click) || holds(click + 1.0)).then_some((from, to))
        });
        let Some((from, to)) = gap else {
            return vec![];
        };
        // The survivor runs from the far edge of the removed gap all the way
        // round to its near edge. Leaving the end below the start is what
        // signals the wrap to everything downstream.
        let mut e = orig.clone();
        e.common.handle = Handle::NULL;
        e.start_parameter = angle_at(to % 1.0);
        e.end_parameter = angle_at(from % 1.0);
        return vec![EntityType::Ellipse(e)];
    }

    let major_radius = orig.major_axis.x.hypot(orig.major_axis.y);
    if major_radius < 1e-9 {
        return vec![];
    }
    let curve = Curve::Ellipse(KernelEllipseArc {
        ellipse: KernelEllipse {
            centre: [orig.center.x, orig.center.y],
            major_radius,
            minor_radius: major_radius * orig.minor_axis_ratio,
            major_axis: [
                orig.major_axis.x / major_radius,
                orig.major_axis.y / major_radius,
            ],
        },
        start_parameter: t0,
        end_parameter: t1,
    });
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            if (tb - ta).abs() < 1e-6 {
                return None;
            }
            let mut e = orig.clone();
            e.common.handle = Handle::NULL;
            e.start_parameter = angle_at(ta);
            e.end_parameter = angle_at(tb);
            Some(EntityType::Ellipse(e))
        })
        .collect()
}

/// Extend an Ellipse arc to the nearest boundary (along the arc direction).
fn extend_ellipse(orig: &EllipseEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let t0 = orig.start_parameter;
    let mut t1 = orig.end_parameter;
    if t1 <= t0 {
        t1 += TAU;
    }
    let span = t1 - t0;
    let a = (orig.major_axis.x.powi(2) + orig.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 {
        return None;
    }
    let b = a * orig.minor_axis_ratio;
    let (nx, ny) = (orig.major_axis.x / a, orig.major_axis.y / a);
    let cx = orig.center.x;
    let cy = orig.center.y;
    let ts = ellipse_seg_ts(cx, cy, a, b, nx, ny, t0, t1, orig.common.handle, geos);
    let extend_end = t_click >= 0.5;

    let best = if extend_end {
        ts.into_iter()
            .filter(|&t| t > 1.0 + 1e-6)
            .min_by(|x, y| x.partial_cmp(y).unwrap())
    } else {
        ts.into_iter()
            .filter(|&t| t < -1e-6)
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

// ── Spline trim / extend ──────────────────────────────────────────────────

/// Trim a Spline entity. Returns surviving spline pieces (one or two).
fn trim_spline(spl: &SplineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let Some(curve) = spline_to_nurbs(spl) else {
        return vec![];
    };
    let (t0, t1) = curve.domain();
    let curve = Curve::Nurbs(curve);

    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let t_lo = t0 + ta * (t1 - t0);
            let t_hi = t0 + tb * (t1 - t0);
            if t_hi - t_lo < 1e-9 {
                return None;
            }
            // Cut away everything before the interval, then everything after
            // it, leaving [t_lo, t_hi]. Either cut is a no-op when the
            // interval already reaches that end of the curve — dropping the
            // piece there would delete the half the user meant to keep.
            let after_low = match spline_cut(spl, t_lo) {
                Some((_, right)) => right,
                None => spl.clone(),
            };
            let kept = match spline_cut(&after_low, t_hi) {
                Some((middle, _)) => middle,
                // The interval reaches the far end, so nothing is left to cut.
                None => after_low,
            };
            Some(EntityType::Spline(kept))
        })
        .collect()
}

/// Extend a Spline toward the nearest boundary (nearest endpoint to pick).
fn extend_spline(spl: &SplineEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    // Sample spline and treat it like a polyline; look for intersections beyond
    // the current start (t<0 virtual) or end (t>1 virtual).
    // For splines we simply find whether the start (t=0) or end (t=1) is closer
    // to the click, then walk along that tangent direction to the nearest boundary.
    let curve = spline_to_nurbs(spl)?;
    let (t0, t1) = curve.domain();
    let extend_end = t_click >= 0.5;

    // Tangent at the endpoint (numerical, Δ = 1e-4 of range)
    let delta = (t1 - t0) * 1e-4;
    let (ep_t, tang_dir) = if extend_end {
        let p0 = curve.point_at_knot(t1 - delta);
        let p1 = curve.point_at_knot(t1);
        (t1, [p1[0] - p0[0], p1[1] - p0[1]])
    } else {
        let p0 = curve.point_at_knot(t0);
        let p1 = curve.point_at_knot(t0 + delta);
        (t0, [p0[0] - p1[0], p0[1] - p1[1]]) // reverse for "before start"
    };
    let ep = curve.point_at_knot(ep_t);
    let (dx, dy) = (tang_dir[0], tang_dir[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return None;
    }
    let (dx, dy) = (dx / len, dy / len);

    // Shoot a real ray from the endpoint along the tangent. The direction is a
    // unit vector, so the parameter that comes back is the distance to the
    // boundary — no stand-in length to pick, and nothing beyond it to miss.
    let shot = Curve::Ray(KernelRay {
        origin: ep,
        direction: [dx, dy],
    });
    let best_t = cut_params(&shot, spl.common.handle, geos)
        .into_iter()
        .filter(|&t| t > 1e-6)
        .reduce(f64::min)?;

    let hit_x = ep[0] + best_t * dx;
    let hit_y = ep[1] + best_t * dy;

    // Add a new control point at the hit location by appending/prepending.
    let z = spl.control_points.first().map(|v| v.z).unwrap_or(0.0);
    let mut new_spl = spl.clone();
    new_spl.common.handle = Handle::NULL;
    new_spl.fit_points.clear();
    if extend_end {
        new_spl
            .control_points
            .push(acadrust::types::Vector3::new(hit_x, hit_y, z));
    } else {
        new_spl
            .control_points
            .insert(0, acadrust::types::Vector3::new(hit_x, hit_y, z));
    }
    // Rebuild knots (uniform) for the extended control polygon.
    let degree = new_spl.degree as usize;
    let n = new_spl.control_points.len();
    new_spl.knots = clamped_uniform_knots(degree, n);
    Some(EntityType::Spline(new_spl))
}

// ── Trim helpers ──────────────────────────────────────────────────────────

fn trim_intervals(curve: &Curve, ts: &[f64], t_click: f64) -> Vec<(f64, f64)> {
    kernel_trim_spans(
        curve,
        ts,
        t_click,
        KernelTolerance::new(CUT_TOLERANCE),
    )
    .into_iter()
    .map(|span| (span[0], span[1]))
    .collect()
}

/// Trim a Line entity. Returns the surviving line segments.
fn trim_line(orig: &LineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let p1 = [orig.start.x, orig.start.y];
    let p2 = [orig.end.x, orig.end.y];
    let z = orig.start.z;
    let curve = Curve::Line(KernelLine { start: p1, end: p2 });
    trim_intervals(&curve, ts, t_click)
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

/// Extend a clicked Arc end along its circle to the nearest boundary
/// crossing (#409). `t_click` ∈ [0,1] within the current span picks the end:
/// ≥ 0.5 extends the end angle CCW, < 0.5 extends the start angle CW.
fn extend_arc(orig: &ArcEnt, t_click: f64, geos: &[Geo]) -> Option<EntityType> {
    let cx = orig.center.x;
    let cy = orig.center.y;
    let r = orig.radius;
    if r < 1e-9 {
        return None;
    }
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
    if span >= TAU - 1e-9 {
        return None; // already a full circle
    }
    // Boundary crossings around the FULL circle, then re-expressed relative
    // to the arc's own span: t ∈ [0,1] lies on the arc, t > 1 walks CCW past
    // the end and wraps around toward the start.
    let ts: Vec<f64> = arc_seg_ts(cx, cy, r, a0, a0 + TAU, orig.common.handle, geos)
        .into_iter()
        .map(|t| t * TAU / span)
        .collect();
    let extend_end = t_click >= 0.5;
    let best = if extend_end {
        // First crossing CCW past the end.
        ts.iter()
            .copied()
            .filter(|&t| t > 1.0 + 1e-6)
            .min_by(|x, y| x.partial_cmp(y).unwrap())
    } else {
        // First crossing CW before the start — in wrapped span-relative
        // coordinates that is the LARGEST t outside the arc.
        ts.iter()
            .copied()
            .filter(|&t| t > 1.0 + 1e-6)
            .max_by(|x, y| x.partial_cmp(y).unwrap())
    }?;
    let ang = norm(a0) + span * best;
    let mut a = orig.clone();
    a.common.handle = Handle::NULL;
    if extend_end {
        a.end_angle = ang;
    } else {
        a.start_angle = ang;
    }
    Some(EntityType::Arc(a))
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
    let curve = Curve::Arc(KernelArc {
        centre: [orig.center.x, orig.center.y],
        radius: orig.radius,
        start_angle: a0,
        end_angle: a1,
    });

    trim_intervals(&curve, ts, t_click)
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

/// Trim a clicked Circle. A full circle has no endpoints, so it needs ≥2
/// boundary crossings: the arc segment containing the click is removed and the
/// circle becomes a single Arc spanning the rest. With fewer than two crossings
/// there is nothing to cut, so an empty result leaves the circle unchanged.
///
/// `ts` are the cut parameters in [0,1) around the circle (angle / TAU), sorted.
fn trim_circle(orig: &CircleEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    if ts.len() < 2 {
        return vec![];
    }
    let tc = t_click.rem_euclid(1.0);
    // Find the cyclic gap (ta, tb) between adjacent cuts that holds the click;
    // the last gap wraps past 1.0 back to the first cut.
    let n = ts.len();
    let mut removed: Option<(f64, f64)> = None;
    for i in 0..n {
        let ta = ts[i];
        let tb = if i + 1 < n { ts[i + 1] } else { ts[0] + 1.0 };
        if (tc >= ta - 1e-9 && tc <= tb + 1e-9)
            || (tc + 1.0 >= ta - 1e-9 && tc + 1.0 <= tb + 1e-9)
        {
            removed = Some((ta, tb));
            break;
        }
    }
    let (ta, tb) = match removed {
        Some(g) => g,
        None => return vec![],
    };

    // Surviving arc runs CCW from the far edge of the removed gap all the way
    // around to its near edge.
    let mut arc = ArcEnt::new();
    arc.common = orig.common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = orig.center;
    arc.radius = orig.radius;
    arc.thickness = orig.thickness;
    arc.normal = orig.normal;
    arc.start_angle = (tb % 1.0) * TAU;
    arc.end_angle = (ta % 1.0) * TAU;
    vec![EntityType::Arc(arc)]
}

/// Trim a clicked LwPolyline: remove the portion containing the click, bounded
/// by the nearest boundary intersections on each side. A closed polyline needs
/// ≥2 cuts and becomes an open polyline (the surviving arc); an open one yields
/// the surviving piece(s). Bulges on fully-surviving segments are kept; the
/// partial end segments at a cut become straight (issue #65).
fn trim_lwpolyline(poly: &LwPolyline, cx: f64, cy: f64, geos: &[Geo]) -> Option<Vec<EntityType>> {
    let handle = poly.common.handle;
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    let total = seg_count as f64;

    let vx = |i: usize| -> (f64, f64) {
        let v = &poly.vertices[i % n];
        (v.location.x, v.location.y)
    };
    let point_at = |t: f64| -> (f64, f64) {
        let tt = if closed { t.rem_euclid(total) } else { t.clamp(0.0, total) };
        let i = (tt.floor() as usize).min(seg_count.saturating_sub(1));
        let u = tt - i as f64;
        let (ax, ay) = vx(i);
        let (bx, by) = vx(i + 1);
        (ax + u * (bx - ax), ay + u * (by - ay))
    };

    // Boundary cuts as global params (segment index + local u).
    let mut cuts: Vec<f64> = Vec::new();
    for i in 0..seg_count {
        let (ax, ay) = vx(i);
        let (bx, by) = vx(i + 1);
        for u in line_seg_ts(ax, ay, bx, by, handle, geos) {
            cuts.push(i as f64 + u.clamp(0.0, 1.0));
        }
    }
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    if cuts.is_empty() {
        return None;
    }

    // Click param: nearest point on the polyline.
    let mut best = (f64::INFINITY, 0.0_f64);
    for i in 0..seg_count {
        let (ax, ay) = vx(i);
        let (bx, by) = vx(i + 1);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let u = if len2 > 1e-12 {
            (((cx - ax) * dx + (cy - ay) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (px, py) = (ax + u * dx, ay + u * dy);
        let d = (px - cx).powi(2) + (py - cy).powi(2);
        if d < best.0 {
            best = (d, i as f64 + u);
        }
    }
    let t_click = best.1;

    // Emit the surviving sub-polyline from param `s0` to `s1` (s1 > s0), with
    // both ends treated as cut points (straight) and interior vertices keeping
    // their original bulge.
    let emit = |s0: f64, s1: f64, start_cut: bool, end_cut: bool| -> Vec<(f64, f64, f64)> {
        let mut o: Vec<(f64, f64, f64)> = Vec::new();
        let (sx, sy) = point_at(s0);
        let s_idx = (s0.floor() as usize) % seg_count;
        let s_bulge = if start_cut { 0.0 } else { poly.vertices[s_idx % n].bulge };
        o.push((sx, sy, s_bulge));
        let mut k = s0.floor() as i64 + 1;
        while (k as f64) < s1 - 1e-9 {
            let idx = (k as usize) % n;
            let seg = (k as usize) % seg_count;
            o.push((vx(idx).0, vx(idx).1, poly.vertices[seg].bulge));
            k += 1;
        }
        if end_cut {
            if let Some(l) = o.last_mut() {
                l.2 = 0.0; // outgoing toward the cut is partial → straight
            }
        }
        let (ex, ey) = point_at(s1);
        o.push((ex, ey, 0.0));
        o
    };

    let mut pieces: Vec<Vec<(f64, f64, f64)>> = Vec::new();
    if closed {
        if cuts.len() < 2 {
            return None;
        }
        let hi = cuts
            .iter()
            .cloned()
            .find(|&c| c > t_click + 1e-9)
            .unwrap_or(cuts[0] + total);
        let lo = cuts
            .iter()
            .cloned()
            .rev()
            .find(|&c| c < t_click - 1e-9)
            .unwrap_or(cuts[cuts.len() - 1] - total);
        let mut s1 = lo;
        while s1 <= hi {
            s1 += total;
        }
        pieces.push(emit(hi, s1, true, true));
    } else {
        let lo = cuts.iter().cloned().rev().find(|&c| c < t_click - 1e-9);
        let hi = cuts.iter().cloned().find(|&c| c > t_click + 1e-9);
        if let Some(lo) = lo {
            pieces.push(emit(0.0, lo, false, true));
        }
        if let Some(hi) = hi {
            pieces.push(emit(hi, total, true, false));
        }
    }

    let mut out: Vec<EntityType> = Vec::new();
    for verts in pieces {
        if verts.len() < 2 {
            continue;
        }
        let mut np = poly.clone();
        np.common.handle = Handle::NULL;
        np.is_closed = false;
        np.vertices = verts
            .into_iter()
            .map(|(x, y, b)| {
                let mut v = LwVertex::from_coords(x, y);
                v.bulge = b;
                v
            })
            .collect();
        out.push(EntityType::LwPolyline(np));
    }
    Some(out)
}

// ── Extend helpers ────────────────────────────────────────────────────────

/// Extend the first or last segment of an LwPolyline to the nearest boundary.
/// Click point (DXF XY) determines which end to extend.
/// The nearest boundary crossing past one end of a segment.
///
/// `t` runs on the infinite line through `from` → `to`: zero at `from`, one
/// at `to`. Extending the far end wants the smallest `t` above one; extending
/// the near end wants the largest `t` below zero. `None` when nothing lies
/// that way.
///
/// The crossings come from `cut_params`, which is the same kernel call every
/// other cut in this file uses. What was here instead was a match over every
/// `Geo` variant with its own line-line, line-circle and line-ellipse test —
/// twice, once for a line and once for a polyline's terminal segment — and a
/// boundary spline was walked as sixty-four chords rather than intersected.
fn extension_hit(
    from: [f64; 2],
    to: [f64; 2],
    handle: Handle,
    geos: &[Geo],
    beyond_end: bool,
) -> Option<f64> {
    let direction = [to[0] - from[0], to[1] - from[1]];
    if direction[0].hypot(direction[1]) < 1e-9 {
        return None;
    }
    // An infinite line rather than the segment: the crossing being looked for
    // is by definition off the end of the drawn part.
    let ray = Curve::XLine(KernelXLine {
        base: from,
        direction,
    });
    let hits = cut_params(&ray, handle, geos);
    if beyond_end {
        hits.into_iter()
            .filter(|t| *t > 1.0 + 1e-6)
            .min_by(f64::total_cmp)
    } else {
        hits.into_iter()
            .filter(|t| *t < -1e-6)
            .max_by(f64::total_cmp)
    }
}

fn extend_lwpoly(
    poly: &LwPolyline,
    click_x: f64,
    click_y: f64,
    geos: &[Geo],
) -> Option<EntityType> {
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }

    let first = &poly.vertices[0];
    let second = &poly.vertices[1];
    let last = &poly.vertices[n - 1];
    let prev = &poly.vertices[n - 2];

    let d_first = (first.location.x - click_x).hypot(first.location.y - click_y);
    let d_last = (last.location.x - click_x).hypot(last.location.y - click_y);
    let extend_end = d_last <= d_first;

    // Extract the terminal segment as a virtual line.
    let (ax, ay, bx, by) = if extend_end {
        (
            prev.location.x,
            prev.location.y,
            last.location.x,
            last.location.y,
        )
    } else {
        (
            second.location.x,
            second.location.y,
            first.location.x,
            first.location.y,
        )
    };

    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return None;
    }

    let best_t = extension_hit([ax, ay], [bx, by], poly.common.handle, geos, true)?;

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
    let extend_end = t_click >= 0.5;
    let best_t = extension_hit([ax, ay], [bx, by], orig.common.handle, geos, extend_end)?;

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
///
/// The parameter is the ray's own: `t = 0` at the base and advancing by one
/// `direction` per unit, unbounded above. A surviving piece that reaches
/// infinity stays a Ray; one with two ends becomes a Line.
fn trim_ray(orig: &RayEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bx = orig.base_point.x;
    let by = orig.base_point.y;
    let bz = orig.base_point.z;
    let dx = orig.direction.x;
    let dy = orig.direction.y;
    let dz = orig.direction.z;
    let pt = |t: f64| [bx + t * dx, by + t * dy, bz + t * dz];

    let curve = ray_curve(orig);
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let pa = pt(ta);
            let pb = pt(tb);
            if tb.is_finite() && (pb[0] - pa[0]).hypot(pb[1] - pa[1]) < 1e-6 {
                return None;
            }

            if tb.is_infinite() {
                // Still extends to infinity → remains a Ray with new base
                let r = RayEnt::new(Vector3::new(pa[0], pa[1], pa[2]), Vector3::new(dx, dy, dz));
                let mut r = r;
                r.common = orig.common.clone();
                r.common.handle = Handle::NULL;
                Some(EntityType::Ray(r))
            } else {
                // Finite segment → Line
                let mut l = LineEnt {
                    common: orig.common.clone(),
                    ..LineEnt::new()
                };
                l.common.handle = Handle::NULL;
                l.start = Vector3::new(pa[0], pa[1], pa[2]);
                l.end = Vector3::new(pb[0], pb[1], pb[2]);
                Some(EntityType::Line(l))
            }
        })
        .collect()
}

/// Trim an XLine entity.
///
/// The parameter is the line's own: `t = 0` at the base point, advancing by
/// one `direction` per unit and running both ways without limit. A surviving
/// piece keeps whichever ends reach infinity — both, and it is still an XLine;
/// one, and it is a Ray; neither, and it is a Line.
fn trim_xline(orig: &XLineEnt, ts: &[f64], t_click: f64) -> Vec<EntityType> {
    let bx = orig.base_point.x;
    let by = orig.base_point.y;
    let bz = orig.base_point.z;
    let dx = orig.direction.x;
    let dy = orig.direction.y;
    let dz = orig.direction.z;
    let pt = |t: f64| [bx + t * dx, by + t * dy, bz + t * dz];

    let curve = xline_curve(orig);
    trim_intervals(&curve, ts, t_click)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let pa = pt(ta);
            let pb = pt(tb);
            let ext_neg = ta.is_infinite();
            let ext_pos = tb.is_infinite();

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
                    let r =
                        RayEnt::new(Vector3::new(pa[0], pa[1], pa[2]), Vector3::new(dx, dy, dz));
                    let mut r = r;
                    r.common = orig.common.clone();
                    r.common.handle = Handle::NULL;
                    Some(EntityType::Ray(r))
                }
                (false, false) => {
                    // Finite segment
                    let mut l = LineEnt {
                        common: orig.common.clone(),
                        ..LineEnt::new()
                    };
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
    geom::line_points(
        [l.start.x, l.start.y, l.start.z],
        [l.end.x, l.end.y, l.end.z],
    )
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
            a.center.z,
        ),
        EntityType::Ellipse(e) => {
            let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
            if a < 1e-9 {
                return vec![];
            }
            let b = a * e.minor_axis_ratio;
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
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
                pts.push([v0.location.x as f32, v0.location.y as f32, elev]);
                pts.push([v1.location.x as f32, v1.location.y as f32, elev]);
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
            vec![
                [bx as f32, bz as f32, by as f32],
                [far_x as f32, far_z as f32, far_y as f32],
            ]
        }
        _ => vec![],
    }
}

// ══════════════════════════════════════════════════════════════════════════
// TrimCommand
// ══════════════════════════════════════════════════════════════════════════


// ── TRIM / EXTEND option machinery (#336) ─────────────────────────────────

/// Sub-mode of the TRIM / EXTEND commands (#336). `Pick` is the quick mode;
/// the others are entered through the option keywords.
enum TrimMode {
    Pick,
    /// Collecting cutting/boundary edges; Enter returns to Pick.
    SelectEdges,
    /// Collecting fence points; Enter runs the fence pass.
    Fence(FencePick),
    /// Crossing: waiting for the first rectangle corner.
    CrossFirst,
    /// Crossing: first corner picked, waiting for the second.
    CrossSecond([f64; 2]),
    /// Erase mode: each pick deletes the object; Enter returns to Pick.
    Erase,
}

#[derive(Clone, Copy)]
struct CrossingWindow {
    min: [f64; 2],
    max: [f64; 2],
    /// The first corner is the trim-side hint, matching the side from which
    /// the crossing window was dragged.
    pick: [f64; 2],
}

fn segment_window_range(
    p1: [f64; 2],
    p2: [f64; 2],
    window: CrossingWindow,
) -> Option<(f64, f64)> {
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for axis in 0..2 {
        let d = p2[axis] - p1[axis];
        if d.abs() < 1e-12 {
            if p1[axis] < window.min[axis] - 1e-9
                || p1[axis] > window.max[axis] + 1e-9
            {
                return None;
            }
            continue;
        }
        let mut a = (window.min[axis] - p1[axis]) / d;
        let mut b = (window.max[axis] - p1[axis]) / d;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        lo = lo.max(a);
        hi = hi.min(b);
        if lo > hi + 1e-9 {
            return None;
        }
    }
    Some((lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0)))
}

/// Crossing selection trims LwPolyline segments independently. A segment that
/// lies inside the selection rectangle but never meets a cutting edge must
/// survive; otherwise a rectangle loses its far vertical edge (#588).
fn crossing_trim_lwpolyline(
    poly: &LwPolyline,
    geos: &[Geo],
    window: CrossingWindow,
) -> Option<Vec<EntityType>> {
    let handle = poly.common.handle;
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    let total = seg_count as f64;
    let vertex_xy = |i: usize| {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };

    let mut removed = Vec::<(f64, f64)>::new();
    for i in 0..seg_count {
        let a = vertex_xy(i);
        let b = vertex_xy(i + 1);
        let Some((inside_lo, inside_hi)) = segment_window_range(a, b, window) else {
            continue;
        };
        let cuts = line_seg_ts(a[0], a[1], b[0], b[1], handle, geos);
        if cuts.is_empty() {
            continue;
        }

        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let len2 = dx * dx + dy * dy;
        let projected = if len2 > 1e-12 {
            ((window.pick[0] - a[0]) * dx + (window.pick[1] - a[1]) * dy) / len2
        } else {
            (inside_lo + inside_hi) * 0.5
        };
        let pick_t = projected.clamp(inside_lo, inside_hi);

        let mut bounds = vec![0.0];
        bounds.extend(cuts);
        bounds.push(1.0);
        bounds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        if let Some(span) = bounds
            .windows(2)
            .find(|span| pick_t >= span[0] - 1e-6 && pick_t <= span[1] + 1e-6)
        {
            if span[1] - span[0] > 1e-6 {
                removed.push((i as f64 + span[0], i as f64 + span[1]));
            }
        }
    }
    if removed.is_empty() {
        return None;
    }

    removed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged = Vec::<(f64, f64)>::new();
    for span in removed {
        if let Some(last) = merged.last_mut() {
            if span.0 <= last.1 + 1e-6 {
                last.1 = last.1.max(span.1);
                continue;
            }
        }
        merged.push(span);
    }

    let point_at = |t: f64| {
        let tt = if closed {
            t.rem_euclid(total)
        } else {
            t.clamp(0.0, total)
        };
        let i = (tt.floor() as usize).min(seg_count.saturating_sub(1));
        let u = tt - i as f64;
        let a = vertex_xy(i);
        let b = vertex_xy(i + 1);
        [a[0] + u * (b[0] - a[0]), a[1] + u * (b[1] - a[1])]
    };
    let emit = |s0: f64, s1: f64| -> Option<EntityType> {
        if s1 - s0 <= 1e-6 {
            return None;
        }
        let mut vertices = Vec::<LwVertex>::new();
        let start = point_at(s0);
        vertices.push(LwVertex::from_coords(start[0], start[1]));
        let mut k = s0.floor() as i64 + 1;
        while (k as f64) < s1 - 1e-9 {
            vertices.push(poly.vertices[(k as usize) % n]);
            k += 1;
        }
        if let Some(last) = vertices.last_mut() {
            last.bulge = 0.0;
        }
        let end = point_at(s1);
        if vertices.last().is_none_or(|v| {
            (v.location.x - end[0]).hypot(v.location.y - end[1]) > 1e-6
        }) {
            vertices.push(LwVertex::from_coords(end[0], end[1]));
        }
        if vertices.len() < 2 {
            return None;
        }
        let mut piece = poly.clone();
        piece.common.handle = Handle::NULL;
        piece.is_closed = false;
        piece.vertices = vertices;
        Some(EntityType::LwPolyline(piece))
    };

    let mut kept = Vec::<(f64, f64)>::new();
    if closed {
        for i in 0..merged.len() {
            let start = merged[i].1;
            let mut end = merged[(i + 1) % merged.len()].0;
            if i + 1 == merged.len() {
                end += total;
            }
            if end - start > 1e-6 {
                kept.push((start, end));
            }
        }
    } else {
        let mut cursor = 0.0;
        for &(start, end) in &merged {
            if start - cursor > 1e-6 {
                kept.push((cursor, start));
            }
            cursor = cursor.max(end);
        }
        if total - cursor > 1e-6 {
            kept.push((cursor, total));
        }
    }

    Some(
        kept.into_iter()
            .filter_map(|(start, end)| emit(start, end))
            .collect(),
    )
}

/// Quick-mode trim at a click/crossing point: the surviving pieces, or `None`
/// when the pick doesn't intersect any boundary (or the type is unsupported).
fn pick_trim_at(
    all: &[EntityType],
    geos: &[Geo],
    handle: Handle,
    px: f64,
    py: f64,
) -> Option<Vec<EntityType>> {
    let entity = all.iter().find(|e| e.common().handle == handle);
    let result: Option<Vec<EntityType>> = match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let ts = line_seg_ts(ax, ay, bx, by, handle, geos);
                if ts.is_empty() {
                    return None;
                }
                let dx = bx - ax;
                let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((px - ax) * dx + (py - ay) * dy) / len2
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
                let ts = arc_seg_ts(cx, cy, a.radius, a0, a1, handle, geos);
                if ts.is_empty() {
                    return None;
                }
                let click_angle = (py - cy).atan2(px - cx);
                let t_click = arc_t(click_angle, a0, a1);
                Some(trim_arc(a, &ts, t_click))
            }
            Some(EntityType::Circle(c)) => {
                let cx = c.center.x;
                let cy = c.center.y;
                let ts = arc_seg_ts(cx, cy, c.radius, 0.0, TAU, handle, geos);
                if ts.len() < 2 {
                    return None;
                }
                let click_angle = (py - cy).atan2(px - cx);
                let t_click = arc_t(click_angle, 0.0, TAU);
                let survivors = trim_circle(c, &ts, t_click);
                if survivors.is_empty() {
                    return None;
                }
                Some(survivors)
            }
            Some(EntityType::Ray(r)) => {
                let curve = ray_curve(r);
                let ts = cut_params(&curve, handle, geos);
                if ts.is_empty() {
                    return None;
                }
                let t_click = curve.parameter_at([px, py]);
                Some(trim_ray(r, &ts, t_click))
            }
            Some(EntityType::XLine(x)) => {
                let curve = xline_curve(x);
                let ts = cut_params(&curve, handle, geos);
                if ts.is_empty() {
                    return None;
                }
                let t_click = curve.parameter_at([px, py]);
                Some(trim_xline(x, &ts, t_click))
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 {
                    return None;
                }
                let b = a * e.minor_axis_ratio;
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 {
                    t1 += TAU;
                }
                let ts = ellipse_seg_ts(
                    e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, geos,
                );
                if ts.is_empty() {
                    return None;
                }
                // t_click: project mouse onto ellipse local param
                let rx = px - e.center.x;
                let ry = py - e.center.y;
                let xl = rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_ell = yl.atan2(xl);
                let t_click = arc_t(t_ell, t0, t1);
                Some(trim_ellipse(e, &ts, t_click))
            }
            Some(EntityType::Spline(s)) => {
                let ts = spline_seg_ts(s, handle, geos);
                if ts.is_empty() {
                    return None;
                }
                let t_click = spline_nearest_t(s, px, py)
                    .and_then(|t_actual| {
                        let (t0, t1) = spline_range(s)?;
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                Some(trim_spline(s, &ts, t_click))
            }
            Some(EntityType::LwPolyline(p)) => trim_lwpolyline(p, px, py, geos),
            _ => None,
        };
    result
}

/// Quick-mode extend at a click/crossing point: the lengthened entity, or
/// `None` when no boundary lies beyond that end (or the type is unsupported).
fn pick_extend_at(
    all: &[EntityType],
    geos: &[Geo],
    handle: Handle,
    px: f64,
    py: f64,
) -> Option<EntityType> {
    let entity = all.iter().find(|e| e.common().handle == handle);
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
                    ((px - ax) * dx + (py - ay) * dy) / len2
                } else {
                    0.5
                };
                extend_line(l, t_click, geos)
            }
            Some(EntityType::Arc(a)) => {
                let ang = (py - a.center.y).atan2(px - a.center.x);
                let t_click = arc_t(ang, a.start_angle, a.end_angle);
                extend_arc(a, t_click, geos)
            }
            Some(EntityType::Ellipse(e)) => {
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 {
                    t1 += TAU;
                }
                let span = t1 - t0;
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 {
                    return None;
                }
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let rx = px - e.center.x;
                let ry = py - e.center.y;
                let xl = rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_click = arc_t(yl.atan2(xl), t0, t1);
                let _ = span;
                extend_ellipse(e, t_click, geos)
            }
            Some(EntityType::LwPolyline(p)) => {
                extend_lwpoly(p, px, py, geos)
            }
            Some(EntityType::Spline(s)) => {
                let t_click = spline_nearest_t(s, px, py)
                    .and_then(|t_actual| {
                        let (t0, t1) = spline_range(s)?;
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                extend_spline(s, t_click, geos)
            }
            _ => None,
        };
    result
}

/// Edge option (#336): treat boundary edges as implied — straight edges run
/// on past their endpoints and arcs close to full circles, so objects trim /
/// extend to the boundary's extrapolation, not only its drawn extent.
fn imply_edge_geos(geos: &mut [Geo]) {
    for g in geos.iter_mut() {
        match g {
            Geo::Line { handle, p1, p2 } => {
                let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
                if dx.hypot(dy) > 1e-9 {
                    // An implied edge cuts wherever the boundary's line would
                    // reach, which is what an infinite line is. Stretching the
                    // segment to a stand-in length instead both invents a
                    // limit and, at survey coordinates, is no larger than the
                    // coordinates themselves.
                    *g = Geo::InfLine {
                        handle: *handle,
                        bx: p1[0],
                        by: p1[1],
                        dx,
                        dy,
                    };
                }
            }
            Geo::Arc { a0, a1, .. } => {
                *a0 = 0.0;
                *a1 = TAU;
            }
            _ => {}
        }
    }
}

/// Dense XY sampling for the fence pass — covers the types the quick pick
/// supports (adds Circle / Ray / XLine over `sample_entity_xy`).
fn fence_sample_xy(e: &EntityType) -> Vec<[f64; 2]> {
    match e {
        // The two unbounded kinds, which `sample_entity_xy` declines to
        // guess a length for. Here there is one: as far as a trim reaches.
        EntityType::Ray(r) => vec![
            [r.base_point.x, r.base_point.y],
            [
                r.base_point.x + r.direction.x * TRIM_EXTENT,
                r.base_point.y + r.direction.y * TRIM_EXTENT,
            ],
        ],
        EntityType::XLine(x) => vec![
            [
                x.base_point.x - x.direction.x * TRIM_EXTENT,
                x.base_point.y - x.direction.y * TRIM_EXTENT,
            ],
            [
                x.base_point.x + x.direction.x * TRIM_EXTENT,
                x.base_point.y + x.direction.y * TRIM_EXTENT,
            ],
        ],
        _ => sample_entity_xy(e),
    }
}

/// Every crossing point of `e` with the fence polyline, in traversal order.
/// Each drives the same per-type pick as a direct click, so the actual cuts
/// happen at the exact boundary intersections.
fn fence_cross_points(e: &EntityType, fence_geos: &[Geo]) -> Vec<[f64; 2]> {
    let pts = fence_sample_xy(e);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        // NOTE: the exclusion handle must differ from the fence geos' own —
        // passing NULL against NULL-handle geos excluded the whole fence.
        let mut ts = line_seg_ts(w[0][0], w[0][1], w[1][0], w[1][1], Handle::new(FENCE_PROBE), fence_geos);
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for t in ts {
            out.push(lerp2(w[0], w[1], t));
        }
    }
    out
}

/// Trim/extend one entity at EVERY fence crossing: the pick is re-applied to
/// the surviving pieces until no crossing bites (a fence across both
/// overhanging ends of one line clears both). Returns `None` when nothing
/// changed. Pieces carry NULL handles for the ReplaceMany placeholder flow.
fn fence_pieces(
    e: &EntityType,
    geos: &[Geo],
    fence_geos: &[Geo],
    window: Option<CrossingWindow>,
    extend: bool,
) -> Option<Vec<EntityType>> {
    if !extend {
        if let (EntityType::LwPolyline(poly), Some(window)) = (e, window) {
            return crossing_trim_lwpolyline(poly, geos, window);
        }
    }

    let target = e.common().handle;
    let mut pieces = vec![e.clone()];
    let mut changed = false;
    for _round in 0..8 {
        let mut next: Vec<EntityType> = Vec::new();
        let mut any = false;
        for piece in &pieces {
            let mut tmp = piece.clone();
            tmp.as_entity_mut().set_handle(target);
            let tmp_all = [tmp];
            let mut consumed = false;
            let mut cps = fence_cross_points(&tmp_all[0], fence_geos);
            // Crossing works like a crossing SELECTION: an object wholly
            // inside the window is picked too — synthesize the click at a
            // sampled point inside it.
            if cps.is_empty() {
                if let Some(window) = window {
                    if let Some(p) = fence_sample_xy(&tmp_all[0]).into_iter().find(|p| {
                        p[0] >= window.min[0]
                            && p[0] <= window.max[0]
                            && p[1] >= window.min[1]
                            && p[1] <= window.max[1]
                    }) {
                        cps.push(p);
                    }
                }
            }
            for cp in cps {
                let res = if extend {
                    pick_extend_at(&tmp_all, geos, target, cp[0], cp[1])
                        .map(|x| vec![x])
                } else {
                    pick_trim_at(&tmp_all, geos, target, cp[0], cp[1])
                };
                if let Some(mut sub) = res {
                    for sp in &mut sub {
                        sp.as_entity_mut().set_handle(Handle::NULL);
                    }
                    next.extend(sub);
                    any = true;
                    changed = true;
                    consumed = true;
                    break;
                }
            }
            if !consumed {
                next.push(piece.clone());
            }
        }
        pieces = next;
        if !any || pieces.len() > 64 {
            break;
        }
    }
    if changed { Some(pieces) } else { None }
}

/// Fence / Crossing pass (#336): every object crossing the fence polyline is
/// trimmed (or extended) at each of its crossing points; the cuts land on the
/// boundary-edge intersections, the fence only selects.
/// Boundary/edge-selection highlight colour (the EXTRIM boundary yellow).
const OPT_YELLOW: [f32; 4] = [1.0, 0.90, 0.15, 1.0];

/// Sentinel handles for the fence pass: the fence's own geos and the probe
/// handle used against them must never collide with a real entity handle (or
/// with each other — `line_seg_ts` drops geos matching the probe handle).
const FENCE_GEO: u64 = u64::MAX - 8;
const FENCE_PROBE: u64 = u64::MAX - 9;

fn build_fence_geos(fence: &[[f64; 2]]) -> Vec<Geo> {
    fence
        .windows(2)
        .map(|w| Geo::Line {
            handle: Handle::new(FENCE_GEO),
            p1: w[0],
            p2: w[1],
        })
        .collect()
}

/// Live result preview of a fence / crossing pass: each affected original in
/// red, its surviving pieces in cyan — the same convention as the pick hover.
fn fence_result_preview(
    all: &[EntityType],
    geos: &[Geo],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    extend: bool,
    implied_edges: bool,
) -> Vec<WireModel> {
    let fence_geos = build_fence_geos(fence);
    let mut out = Vec::new();
    for e in all {
        if e.common().handle.is_null() {
            continue;
        }
        if let Some(pieces) = fence_pieces(e, geos, &fence_geos, window, extend) {
            out.push(WireModel::solid(
                "trim_rm".into(),
                entity_pts(e),
                DIM_RED,
                false,
            ));
            for (i, pe) in pieces.iter().enumerate() {
                out.push(WireModel::solid(
                    format!("trim_keep_{i}"),
                    entity_pts(pe),
                    WireModel::CYAN,
                    false,
                ));
                if extend {
                    if let Some(t) = extend_tail_preview(e, pe, "extend_tail") {
                        out.push(t);
                    }
                }
            }
            if implied_edges {
                let cuts = piece_cut_points(e, &pieces);
                out.extend(implied_cut_guides(all, e.common().handle, &cuts));
            }
        }
    }
    out
}

fn fence_pass(
    all: &[EntityType],
    geos: &[Geo],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    extend: bool,
) -> Vec<(Handle, Vec<EntityType>)> {
    let fence_geos: Vec<Geo> = build_fence_geos(fence);
    let mut out = Vec::new();
    for e in all {
        let h = e.common().handle;
        if h.is_null() {
            continue;
        }
        if let Some(pieces) = fence_pieces(e, geos, &fence_geos, window, extend) {
            out.push((h, pieces));
        }
    }
    out
}

/// The ADDED tail of an extend result, dashed — the slice of the extended
/// curve beyond the original endpoint, so the preview shows the extension
/// itself rather than only a recolored whole (#336). Generic over entity
/// type via the sampled points.
fn extend_tail_preview(orig: &EntityType, ext: &EntityType, name: &str) -> Option<WireModel> {
    let op = entity_pts(orig);
    let ep = entity_pts(ext);
    if op.len() < 2 || ep.len() < 2 {
        return None;
    }
    let d2 = |a: &[f32; 3], b: &[f32; 3]| {
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        dx * dx + dy * dy
    };
    let nearest_idx = |pts: &[[f32; 3]], q: &[f32; 3]| {
        let mut best = 0usize;
        let mut bd = f32::MAX;
        for (i, p) in pts.iter().enumerate() {
            let d = d2(p, q);
            if d < bd {
                bd = d;
                best = i;
            }
        }
        best
    };
    let of = op.first().unwrap();
    let ol = op.last().unwrap();
    let ef = ep.first().unwrap();
    let el = ep.last().unwrap();
    let tail: Vec<[f32; 3]> = if d2(ol, el) > 1e-10 {
        let i = nearest_idx(&ep, ol);
        ep[i..].to_vec()
    } else if d2(of, ef) > 1e-10 {
        let i = nearest_idx(&ep, of);
        ep[..=i].to_vec()
    } else {
        return None;
    };
    if tail.len() < 2 {
        return None;
    }
    let mut w = WireModel::solid(name.into(), tail, [0.3, 1.0, 1.0, 1.0], false);
    w.pattern_length = 0.8;
    w.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    w.line_weight_px = 1.6;
    Some(w)
}

/// Distance from `q` to the drawn (sampled) body of `e`.
fn dist_to_drawn(e: &EntityType, q: [f64; 2]) -> f64 {
    let pts = fence_sample_xy(e);
    let mut best = f64::MAX;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 1e-12 {
            (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let px = a[0] + dx * t;
        let py = a[1] + dy * t;
        best = best.min((q[0] - px).hypot(q[1] - py));
    }
    best
}

/// Endpoints of the surviving pieces that are NOT endpoints of the original —
/// i.e. the actual cut points of a trim result.
fn piece_cut_points(orig: &EntityType, pieces: &[EntityType]) -> Vec<[f64; 2]> {
    let op = entity_pts(orig);
    if op.len() < 2 {
        return vec![];
    }
    let f = op.first().unwrap();
    let l = op.last().unwrap();
    let closed = (f[0] - l[0]).abs() < 1e-6 && (f[1] - l[1]).abs() < 1e-6;
    let ends: Vec<[f32; 3]> = if closed { vec![] } else { vec![*f, *l] };
    let mut out = Vec::new();
    for pe in pieces {
        let pp = entity_pts(pe);
        if pp.len() < 2 {
            continue;
        }
        for q in [pp.first().unwrap(), pp.last().unwrap()] {
            let is_orig_end = ends.iter().any(|e| {
                let dx = e[0] - q[0];
                let dy = e[1] - q[1];
                dx * dx + dy * dy < 1e-6
            });
            if !is_orig_end {
                out.push([q[0] as f64, q[1] as f64]);
            }
        }
    }
    out
}

/// Edge: Extend guides (#336): for every cut point that does NOT lie on a
/// boundary's drawn body, draw a dashed guide along the boundary's implied
/// extension — from its drawn end to the cut — so the user sees WHICH edge
/// causes the cut there. Lines get a straight guide, arcs follow the circle.
fn implied_cut_guides(
    all: &[EntityType],
    target: Handle,
    cuts: &[[f64; 2]],
) -> Vec<WireModel> {
    let mut out = Vec::new();
    'cuts: for (ci, cp) in cuts.iter().enumerate() {
        let tol = 1e-6 * (1.0 + cp[0].abs() + cp[1].abs());
        // Attributed to a drawn body → nothing implied to explain.
        for e in all {
            let h = e.common().handle;
            if h.is_null() || h == target {
                continue;
            }
            if dist_to_drawn(e, *cp) < tol {
                continue 'cuts;
            }
        }
        // Find the boundary whose extrapolation passes through the cut.
        for e in all {
            let h = e.common().handle;
            if h.is_null() || h == target {
                continue;
            }
            match e {
                EntityType::Line(l) => {
                    let (ax, ay) = (l.start.x, l.start.y);
                    let (dx, dy) = (l.end.x - ax, l.end.y - ay);
                    let len = dx.hypot(dy);
                    if len < 1e-9 {
                        continue;
                    }
                    // Perpendicular distance to the infinite line.
                    let d = ((cp[0] - ax) * dy - (cp[1] - ay) * dx).abs() / len;
                    if d < tol {
                        let d_start = (cp[0] - ax).hypot(cp[1] - ay);
                        let d_end = (cp[0] - l.end.x).hypot(cp[1] - l.end.y);
                        let from = if d_start < d_end {
                            [ax as f32, ay as f32, 0.0]
                        } else {
                            [l.end.x as f32, l.end.y as f32, 0.0]
                        };
                        let mut w = WireModel::solid(
                            format!("edge_guide_{ci}"),
                            vec![from, [cp[0] as f32, cp[1] as f32, 0.0]],
                            OPT_YELLOW,
                            false,
                        );
                        w.pattern_length = 0.8;
                        w.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                        out.push(w);
                        continue 'cuts;
                    }
                }
                EntityType::Arc(a) => {
                    let r = (cp[0] - a.center.x).hypot(cp[1] - a.center.y);
                    if (r - a.radius).abs() < tol {
                        // Follow the circle from the nearer drawn end to the
                        // cut, going the short way outside the drawn span.
                        let theta = (cp[1] - a.center.y).atan2(cp[0] - a.center.x);
                        let a0 = norm(a.start_angle);
                        let a1 = norm(a.end_angle);
                        let ccw_from_end = (theta - a1).rem_euclid(TAU);
                        let cw_from_start = (a0 - theta).rem_euclid(TAU);
                        let (base, sweep) = if ccw_from_end <= cw_from_start {
                            (a1, ccw_from_end)
                        } else {
                            (a0, -cw_from_start)
                        };
                        let steps = ((sweep.abs() * 16.0).ceil() as usize).max(2);
                        let pts: Vec<[f32; 3]> = (0..=steps)
                            .map(|i| {
                                let ang = base + sweep * (i as f64 / steps as f64);
                                [
                                    (a.center.x + a.radius * ang.cos()) as f32,
                                    (a.center.y + a.radius * ang.sin()) as f32,
                                    0.0,
                                ]
                            })
                            .collect();
                        let mut w = WireModel::solid(
                            format!("edge_guide_{ci}"),
                            pts,
                            OPT_YELLOW,
                            false,
                        );
                        w.pattern_length = 0.8;
                        w.pattern = [0.5, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
                        out.push(w);
                        continue 'cuts;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Extend hover preview: the whole result in cyan, the added tail dashed and
/// — with Edge: Extend on — a guide along the implied boundary that the new
/// endpoint lands on.
fn extend_hover_wires(
    orig: &EntityType,
    ext: &EntityType,
    all: &[EntityType],
    target: Handle,
    implied_edges: bool,
) -> Vec<WireModel> {
    let mut out = vec![WireModel::solid(
        "extend_prev".into(),
        entity_pts(ext),
        WireModel::CYAN,
        false,
    )];
    if let Some(t) = extend_tail_preview(orig, ext, "extend_tail") {
        out.push(t);
    }
    if implied_edges {
        // The moved endpoint is the cut against the (implied) boundary.
        let cuts = piece_cut_points(orig, std::slice::from_ref(ext));
        out.extend(implied_cut_guides(all, target, &cuts));
    }
    out
}

pub struct TrimCommand {
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    geos: Vec<Geo>,
    mode: TrimMode,
    /// Cutting-edge selection; empty = every object cuts (quick mode).
    edge_set: Vec<Handle>,
    /// Edge option: boundaries extrapolate past their drawn extent.
    implied_edges: bool,
    /// Live Shift state — Shift+click extends instead of trims (#336).
    shift: bool,
}

impl TrimCommand {
    pub fn new(all_entities: Vec<EntityType>) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .iter()
            .map(entity_with_lwpolyline_world_xy)
            .collect();
        let entity_index = ModifyEntityIndex::build(&all_entities);
        let geos = build_geos(&all_entities);
        Self {
            all_entities,
            entity_index,
            geos,
            mode: TrimMode::Pick,
            edge_set: Vec::new(),
            implied_edges: false,
            shift: false,
        }
    }

    /// Boundary geometry from the edge selection (or everything when none),
    /// with the Edge option's implied extrapolation applied on top.
    fn rebuild_geos(&mut self) {
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.geos = if self.edge_set.is_empty() {
            build_geos(&self.all_entities)
        } else {
            let picked: Vec<EntityType> = self
                .all_entities
                .iter()
                .filter(|e| self.edge_set.contains(&e.common().handle))
                .cloned()
                .collect();
            build_geos(&picked)
        };
        if self.implied_edges {
            imply_edge_geos(&mut self.geos);
        }
    }

    /// Exact analytic boundaries whose boxes overlap the picked entity. The
    /// host already narrowed the cursor pick; this second broad phase keeps
    /// TRIM preview/intersection work local on dense drawings.
    fn nearby_geos(&self, handle: Handle) -> Vec<Geo> {
        if self.implied_edges {
            return self.geos.clone();
        }
        let Some(handles) = self.entity_index.nearby_handles(&self.all_entities, handle) else {
            return self.geos.clone();
        };
        self.geos
            .iter()
            .filter(|geo| handles.contains(&geo.handle()))
            .cloned()
            .collect()
    }

    fn fence_run(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<CrossingWindow>,
    ) -> CmdResult {
        let repl = fence_pass(&self.all_entities, &self.geos, fence, window, self.shift);
        if repl.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::ReplaceMany(repl, Vec::new())
    }

    fn stage_replacements(&mut self, replacements: &[(Handle, Vec<EntityType>)]) {
        for (handle, _) in replacements {
            if let Some(index) = self
                .all_entities
                .iter()
                .position(|entity| entity.common().handle == *handle)
            {
                self.all_entities.remove(index);
            }
            self.edge_set.retain(|edge| edge != handle);
        }
        for (_, entities) in replacements {
            self.all_entities.extend(entities.iter().cloned());
        }
        self.rebuild_geos();
    }
}

impl CadCommand for TrimCommand {
    fn name(&self) -> &'static str {
        "TRIM"
    }

    fn prompt(&self) -> String {
        let edge = if self.implied_edges {
            t!(" [Edge: Extend]")
        } else {
            std::borrow::Cow::Borrowed("")
        };
        match &self.mode {
            TrimMode::Pick => {
                crate::tf!("TRIM{edge}  Click segment to remove (Shift+click extends):")
                    .into_owned()
            }
            TrimMode::SelectEdges => crate::tf!(
                "TRIM  Select cutting edges [{} picked, Enter = done]:",
                self.edge_set.len()
            )
            .into_owned(),
            TrimMode::Fence(pick) => crate::tf!(
                "TRIM{edge}  Fence: pick points [{} placed, Enter = trim crossed]:",
                pick.len()
            )
            .into_owned(),
            TrimMode::CrossFirst => crate::tf!("TRIM{edge}  Crossing: first corner:").into_owned(),
            TrimMode::CrossSecond(_) => {
                crate::tf!("TRIM{edge}  Crossing: opposite corner:").into_owned()
            }
            TrimMode::Erase => {
                t!("TRIM  Erase: click objects to delete [Enter = done]:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.mode {
            TrimMode::Pick => vec![
                CmdOption::new("Cutting edges", "T"),
                CmdOption::new("Fence", "F"),
                CmdOption::new("Crossing", "C"),
                CmdOption::new(
                    if self.implied_edges { "Edge: Extend" } else { "Edge: No extend" },
                    "E",
                ),
                CmdOption::new("Erase", "R"),
                CmdOption::enter("Done"),
            ],
            _ => vec![CmdOption::enter("Done")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        // Consumed inputs return Some(NeedPoint) — None would be offered to
        // the command a second time by the driver.
        match text.trim().to_uppercase().as_str() {
            "T" | "CUTTING" | "B" | "BOUNDARY" => {
                self.edge_set.clear();
                self.mode = TrimMode::SelectEdges;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FENCE" => {
                self.mode = TrimMode::Fence(FencePick::fence());
                Some(CmdResult::NeedPoint)
            }
            "C" | "CROSSING" => {
                self.mode = TrimMode::CrossFirst;
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" => {
                self.implied_edges = !self.implied_edges;
                self.rebuild_geos();
                Some(CmdResult::NeedPoint)
            }
            "R" | "ERASE" => {
                self.mode = TrimMode::Erase;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.mode,
            TrimMode::Pick | TrimMode::SelectEdges | TrimMode::Erase
        )
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            TrimMode::SelectEdges => {
                // Toggle membership; the boundary rebuilds on Enter.
                if let Some(pos) = self.edge_set.iter().position(|h| *h == handle) {
                    self.edge_set.remove(pos);
                } else {
                    self.edge_set.push(handle);
                }
                CmdResult::NeedPoint
            }
            TrimMode::Erase => {
                // Erase option: delete without needing an intersection.
                if let Some(pos) = self
                    .all_entities
                    .iter()
                    .position(|e| e.common().handle == handle)
                {
                    self.all_entities.remove(pos);
                }
                self.edge_set.retain(|h| *h != handle);
                self.rebuild_geos();
                CmdResult::ReplaceEntity(handle, vec![])
            }
            _ => {
                let (px, py) = (pt.x, pt.y);
                let new_entities = if self.shift {
                    // Shift+click swaps to Extend for this pick (#336).
                    pick_extend_at(&self.all_entities, &self.geos, handle, px, py)
                        .map(|e| vec![e])
                } else {
                    let geos = self.nearby_geos(handle);
                    pick_trim_at(&self.all_entities, &geos, handle, px, py)
                };
                if let Some(new_entities) = new_entities {
                    // Snapshot is updated in on_entity_replaced once we know
                    // the real handles. Pre-stage: remove the old entry now so
                    // geos exclude it immediately.
                    if let Some(pos) = self
                        .all_entities
                        .iter()
                        .position(|e| e.common().handle == handle)
                    {
                        self.all_entities.remove(pos);
                        // Pieces join with NULL handles as geometry-only placeholders.
                        self.all_entities.extend(new_entities.clone());
                        self.edge_set.retain(|h| *h != handle);
                        self.rebuild_geos();
                    }
                    CmdResult::ReplaceEntity(handle, new_entities)
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_entity_replaced(&mut self, _old: Handle, new_handles: &[acadrust::Handle]) {
        // Batch gestures stage several NULL-handle replacement groups before
        // the document assigns real handles. The host applies them in the same
        // order, so fill the first remaining placeholders on each callback.
        let mut handles = new_handles.iter().copied();
        for entity in self
            .all_entities
            .iter_mut()
            .filter(|entity| entity.common().handle.is_null())
        {
            let Some(handle) = handles.next() else {
                break;
            };
            entity.as_entity_mut().set_handle(handle);
        }
        self.rebuild_geos();
    }

    fn on_drag_selection(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
    ) -> Option<CmdResult> {
        if !matches!(self.mode, TrimMode::Pick) || fence.len() < 2 {
            return None;
        }
        let window = window.map(|(min, max)| CrossingWindow {
            min,
            max,
            pick: fence[0],
        });
        let replacements = fence_pass(
            &self.all_entities,
            &self.geos,
            fence,
            window,
            self.shift,
        );
        if replacements.is_empty() {
            return Some(CmdResult::NeedPoint);
        }
        self.stage_replacements(&replacements);
        Some(CmdResult::ReplaceManyContinue(replacements))
    }

    fn accepts_drag_selection(&self) -> bool {
        matches!(self.mode, TrimMode::Pick)
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        match self.mode {
            TrimMode::SelectEdges => {
                // Picked cutting edges stay highlighted; the hovered candidate
                // joins them at half strength.
                let mut out: Vec<WireModel> = self
                    .edge_set
                    .iter()
                    .filter_map(|h| {
                        self.all_entities.iter().find(|e| e.common().handle == *h)
                    })
                    .map(|e| {
                        WireModel::solid("edge_sel".into(), entity_pts(e), OPT_YELLOW, false)
                    })
                    .collect();
                if !handle.is_null() && !self.edge_set.contains(&handle) {
                    if let Some(e) = self
                        .all_entities
                        .iter()
                        .find(|e| e.common().handle == handle)
                    {
                        let mut c = OPT_YELLOW;
                        c[3] = 0.45;
                        out.push(WireModel::solid(
                            "edge_cand".into(),
                            entity_pts(e),
                            c,
                            false,
                        ));
                    }
                }
                return out;
            }
            TrimMode::Erase => {
                // Erase mode: the hovered object previews fully red.
                if let Some(e) = self
                    .all_entities
                    .iter()
                    .find(|e| e.common().handle == handle)
                {
                    return vec![WireModel::solid(
                        "erase_prev".into(),
                        entity_pts(e),
                        DIM_RED,
                        false,
                    )];
                }
                return vec![];
            }
            TrimMode::Pick => {}
            _ => return vec![],
        }
        if self.shift {
            // Shift held: preview the extend result instead.
            if let Some(ext) =
                pick_extend_at(&self.all_entities, &self.geos, handle, pt.x, pt.y)
            {
                if let Some(orig) = self
                    .all_entities
                    .iter()
                    .find(|e| e.common().handle == handle)
                {
                    return extend_hover_wires(
                        orig,
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
                return vec![WireModel::solid(
                    "extend_prev".into(),
                    entity_pts(&ext),
                    WireModel::CYAN,
                    false,
                )];
            }
            return vec![];
        }
        if handle.is_null() {
            return vec![];
        }

        let nearby_geos = self.nearby_geos(handle);
        let geos = nearby_geos.as_slice();
        let entity = self
            .entity_index.get(&self.all_entities, handle);

        let mut hover_wires = match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let ts = line_seg_ts(ax, ay, bx, by, handle, geos);
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
                let ts = arc_seg_ts(cx, cy, a.radius, a0, a1, handle, geos);
                if ts.is_empty() {
                    return vec![];
                }
                let click_angle = (pt.y as f64 - cy).atan2(pt.x as f64 - cx);
                let t_click = arc_t(click_angle, a0, a1);
                let survivors = trim_arc(a, &ts, t_click);
                let orig_pts = arc_pts(cx, cy, a.radius, a0, a1, a.center.z);
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
            Some(EntityType::Circle(c)) => {
                let cx = c.center.x;
                let cy = c.center.y;
                let ts = arc_seg_ts(cx, cy, c.radius, 0.0, TAU, handle, geos);
                if ts.len() < 2 {
                    return vec![];
                }
                let click_angle = (pt.y as f64 - cy).atan2(pt.x as f64 - cx);
                let t_click = arc_t(click_angle, 0.0, TAU);
                let survivors = trim_circle(c, &ts, t_click);
                if survivors.is_empty() {
                    return vec![];
                }
                let orig_pts = arc_pts(cx, cy, c.radius, 0.0, TAU, c.center.z);
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
                let curve = ray_curve(r);
                let ts = cut_params(&curve, handle, geos);
                if ts.is_empty() {
                    return vec![];
                }
                let t_click = curve.parameter_at([pt.x as f64, pt.y as f64]);
                let survivors = trim_ray(r, &ts, t_click);
                // Show a finite preview section (20 units) for the original ray
                let far = [
                    (bx + r.direction.x * 20.0) as f32,
                    (by + r.direction.y * 20.0) as f32,
                    r.base_point.z as f32,
                ];
                let base = [bx as f32, by as f32, r.base_point.z as f32];
                let removed = WireModel::solid("trim_rm".into(), vec![base, far], DIM_RED, false);
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
            Some(EntityType::XLine(x)) => {
                let bx = x.base_point.x;
                let by = x.base_point.y;
                let curve = xline_curve(x);
                let ts = cut_params(&curve, handle, geos);
                if ts.is_empty() {
                    return vec![];
                }
                let t_click = curve.parameter_at([pt.x as f64, pt.y as f64]);
                let survivors = trim_xline(x, &ts, t_click);
                let neg = [
                    (bx - x.direction.x * 20.0) as f32,
                    (by - x.direction.y * 20.0) as f32,
                    x.base_point.z as f32,
                ];
                let pos_pt = [
                    (bx + x.direction.x * 20.0) as f32,
                    (by + x.direction.y * 20.0) as f32,
                    x.base_point.z as f32,
                ];
                let removed = WireModel::solid("trim_rm".into(), vec![neg, pos_pt], DIM_RED, false);
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
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 {
                    return vec![];
                }
                let b = a * e.minor_axis_ratio;
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 {
                    t1 += TAU;
                }
                let ts = ellipse_seg_ts(
                    e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, geos,
                );
                if ts.is_empty() {
                    return vec![];
                }
                let rx = pt.x as f64 - e.center.x;
                let ry = pt.y as f64 - e.center.y;
                let xl = rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_click = arc_t(yl.atan2(xl), t0, t1);
                let survivors = trim_ellipse(e, &ts, t_click);
                let orig_pts =
                    ellipse_pts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, e.center.z);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, DIM_RED, false);
                let mut out = vec![removed];
                for (i, ent) in survivors.iter().enumerate() {
                    let pts = entity_pts(ent);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Spline(s)) => {
                let ts = spline_seg_ts(s, handle, geos);
                if ts.is_empty() {
                    return vec![];
                }
                let t_click = spline_nearest_t(s, pt.x as f64, pt.y as f64)
                    .and_then(|t_actual| {
                        let (t0, t1) = spline_range(s)?;
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                let orig_pts = spline_pts_wire(s);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, DIM_RED, false);
                let survivors = trim_spline(s, &ts, t_click);
                let mut out = vec![removed];
                for (i, ent) in survivors.iter().enumerate() {
                    let pts = entity_pts(ent);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::LwPolyline(p)) => {
                let Some(survivors) = trim_lwpolyline(p, pt.x as f64, pt.y as f64, geos)
                else {
                    return vec![];
                };
                let orig = WireModel::solid("trim_rm".into(), entity_pts(entity.unwrap()), DIM_RED, false);
                let mut out = vec![orig];
                for (i, ent) in survivors.iter().enumerate() {
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        entity_pts(ent),
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            _ => vec![],
        };
        // Edge: Extend — explain cuts landing on an IMPLIED boundary with a
        // dashed guide from that boundary's drawn end to the cut point (#336).
        if self.implied_edges && !hover_wires.is_empty() {
            if let (Some(orig), Some(pieces)) = (
                entity,
                pick_trim_at(&self.all_entities, geos, handle, pt.x, pt.y),
            ) {
                let cuts = piece_cut_points(orig, &pieces);
                hover_wires.extend(implied_cut_guides(&self.all_entities, handle, &cuts));
            }
        }
        hover_wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &mut self.mode {
            TrimMode::Fence(pick) => {
                pick.push([pt.x, pt.y]);
                CmdResult::NeedPoint
            }
            TrimMode::CrossFirst => {
                self.mode = TrimMode::CrossSecond([pt.x, pt.y]);
                CmdResult::NeedPoint
            }
            TrimMode::CrossSecond(p1) => {
                // The rectangle's edges act as a 4-segment fence, and objects
                // wholly inside the window are picked too (crossing-selection
                // semantics).
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                self.mode = TrimMode::Pick;
                let rect = [p1, [p1[0], p2[1]], p2, [p2[0], p1[1]], p1];
                let window = CrossingWindow {
                    min: [p1[0].min(p2[0]), p1[1].min(p2[1])],
                    max: [p1[0].max(p2[0]), p1[1].max(p2[1])],
                    pick: p1,
                };
                self.fence_run(&rect, Some(window))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        match &self.mode {
            TrimMode::Fence(pick) if !pick.is_empty() => {
                let mut out = vec![pick.preview([pt.x, pt.y], "trim_fence")];
                let mut fpts = pick.path();
                fpts.push([pt.x, pt.y]);
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.geos,
                    &fpts,
                    None,
                    self.shift,
                    self.implied_edges,
                ));
                out
            }
            TrimMode::CrossSecond(p1) => {
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                let mut out = vec![crossing_box_preview(p1, p2, "trim_cross")];
                let rect = [p1, [p1[0], p2[1]], p2, [p2[0], p1[1]], p1];
                let window = CrossingWindow {
                    min: [p1[0].min(p2[0]), p1[1].min(p2[1])],
                    max: [p1[0].max(p2[0]), p1[1].max(p2[1])],
                    pick: p1,
                };
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.geos,
                    &rect,
                    Some(window),
                    self.shift,
                    self.implied_edges,
                ));
                out
            }
            _ => Vec::new(),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.mode, TrimMode::Pick) {
            TrimMode::SelectEdges => {
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            TrimMode::Fence(pick) if pick.is_usable() => self.fence_run(&pick.path(), None),
            TrimMode::Fence(_)
            | TrimMode::CrossFirst
            | TrimMode::CrossSecond(_)
            | TrimMode::Erase => CmdResult::NeedPoint,
            TrimMode::Pick => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        // Esc leaves a sub-mode first; a second Esc ends the command.
        if !matches!(self.mode, TrimMode::Pick) {
            self.mode = TrimMode::Pick;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ExtendCommand
// ══════════════════════════════════════════════════════════════════════════

pub struct ExtendCommand {
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    geos: Vec<Geo>,
    mode: TrimMode,
    /// Boundary-edge selection; empty = every object is a boundary.
    edge_set: Vec<Handle>,
    /// Edge option: boundaries extrapolate past their drawn extent.
    implied_edges: bool,
    /// Live Shift state — Shift+click trims instead of extends (#336).
    shift: bool,
}

impl ExtendCommand {
    pub fn new(all_entities: Vec<EntityType>) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .iter()
            .map(entity_with_lwpolyline_world_xy)
            .collect();
        let entity_index = ModifyEntityIndex::build(&all_entities);
        let geos = build_geos(&all_entities);
        Self {
            all_entities,
            entity_index,
            geos,
            mode: TrimMode::Pick,
            edge_set: Vec::new(),
            implied_edges: false,
            shift: false,
        }
    }

    fn rebuild_geos(&mut self) {
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.geos = if self.edge_set.is_empty() {
            build_geos(&self.all_entities)
        } else {
            let picked: Vec<EntityType> = self
                .all_entities
                .iter()
                .filter(|e| self.edge_set.contains(&e.common().handle))
                .cloned()
                .collect();
            build_geos(&picked)
        };
        if self.implied_edges {
            imply_edge_geos(&mut self.geos);
        }
    }

    fn fence_run(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<CrossingWindow>,
    ) -> CmdResult {
        // EXTEND's fence extends; Shift held at Enter swaps it to trim.
        let repl = fence_pass(&self.all_entities, &self.geos, fence, window, !self.shift);
        if repl.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::ReplaceMany(repl, Vec::new())
    }
}

impl CadCommand for ExtendCommand {
    fn name(&self) -> &'static str {
        "EXTEND"
    }

    fn prompt(&self) -> String {
        let edge = if self.implied_edges {
            t!(" [Edge: Extend]")
        } else {
            std::borrow::Cow::Borrowed("")
        };
        match &self.mode {
            TrimMode::Pick => crate::tf!(
                "EXTEND{edge}  Click near end of object to extend (Shift+click trims):"
            )
            .into_owned(),
            TrimMode::SelectEdges => crate::tf!(
                "EXTEND  Select boundary edges [{} picked, Enter = done]:",
                self.edge_set.len()
            )
            .into_owned(),
            TrimMode::Fence(pick) => crate::tf!(
                "EXTEND{edge}  Fence: pick points [{} placed, Enter = extend crossed]:",
                pick.len()
            )
            .into_owned(),
            TrimMode::CrossFirst => {
                crate::tf!("EXTEND{edge}  Crossing: first corner:").into_owned()
            }
            TrimMode::CrossSecond(_) => {
                crate::tf!("EXTEND{edge}  Crossing: opposite corner:").into_owned()
            }
            TrimMode::Erase => t!("EXTEND  [Enter = done]:").into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.mode {
            TrimMode::Pick => vec![
                CmdOption::new("Boundary edges", "B"),
                CmdOption::new("Fence", "F"),
                CmdOption::new("Crossing", "C"),
                CmdOption::new(
                    if self.implied_edges { "Edge: Extend" } else { "Edge: No extend" },
                    "E",
                ),
                CmdOption::enter("Done"),
            ],
            _ => vec![CmdOption::enter("Done")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "B" | "BOUNDARY" | "T" | "CUTTING" => {
                self.edge_set.clear();
                self.mode = TrimMode::SelectEdges;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FENCE" => {
                self.mode = TrimMode::Fence(FencePick::fence());
                Some(CmdResult::NeedPoint)
            }
            "C" | "CROSSING" => {
                self.mode = TrimMode::CrossFirst;
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" => {
                self.implied_edges = !self.implied_edges;
                self.rebuild_geos();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, TrimMode::Pick | TrimMode::SelectEdges)
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            TrimMode::SelectEdges => {
                if let Some(pos) = self.edge_set.iter().position(|h| *h == handle) {
                    self.edge_set.remove(pos);
                } else {
                    self.edge_set.push(handle);
                }
                CmdResult::NeedPoint
            }
            _ => {
                let (px, py) = (pt.x, pt.y);
                let new_entities = if self.shift {
                    // Shift+click swaps to Trim for this pick (#336).
                    pick_trim_at(&self.all_entities, &self.geos, handle, px, py)
                } else {
                    pick_extend_at(&self.all_entities, &self.geos, handle, px, py)
                        .map(|e| vec![e])
                };
                if let Some(new_entities) = new_entities {
                    // Same snapshot bookkeeping as TRIM: drop the old entry,
                    // append the pieces as NULL-handle placeholders, and let
                    // on_entity_replaced assign the real handles.
                    if let Some(pos) = self
                        .all_entities
                        .iter()
                        .position(|e| e.common().handle == handle)
                    {
                        self.all_entities.remove(pos);
                        self.all_entities.extend(new_entities.clone());
                        self.edge_set.retain(|h| *h != handle);
                        self.rebuild_geos();
                    }
                    CmdResult::ReplaceEntity(handle, new_entities)
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_entity_replaced(&mut self, _old: Handle, new_handles: &[acadrust::Handle]) {
        // The last new_handles.len() entries are the pieces appended with NULL
        // handles in on_entity_pick — assign their real document handles.
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
                EntityType::LwPolyline(p) => p.common.handle = h,
                _ => {}
            }
        }
        self.rebuild_geos();
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        match self.mode {
            TrimMode::SelectEdges => {
                let mut out: Vec<WireModel> = self
                    .edge_set
                    .iter()
                    .filter_map(|h| self.entity_index.get(&self.all_entities, *h))
                    .map(|e| {
                        WireModel::solid("edge_sel".into(), entity_pts(e), OPT_YELLOW, false)
                    })
                    .collect();
                if !handle.is_null() && !self.edge_set.contains(&handle) {
                    if let Some(e) = self
                        .entity_index.get(&self.all_entities, handle)
                    {
                        let mut c = OPT_YELLOW;
                        c[3] = 0.45;
                        out.push(WireModel::solid(
                            "edge_cand".into(),
                            entity_pts(e),
                            c,
                            false,
                        ));
                    }
                }
                return out;
            }
            TrimMode::Pick => {}
            _ => return vec![],
        }
        if self.shift {
            // Shift held: preview the trim result instead.
            if let Some(pieces) =
                pick_trim_at(&self.all_entities, &self.geos, handle, pt.x, pt.y)
            {
                let mut out = Vec::new();
                for (i, e) in pieces.iter().enumerate() {
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        entity_pts(e),
                        WireModel::CYAN,
                        false,
                    ));
                }
                return out;
            }
            return vec![];
        }
        if handle.is_null() {
            return vec![];
        }

        let entity = self
            .entity_index.get(&self.all_entities, handle);
        match entity {
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
                if let Some(ext) = extend_line(l, t_click, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::Line(l.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            Some(EntityType::Arc(a)) => {
                let ang = (pt.y as f64 - a.center.y).atan2(pt.x as f64 - a.center.x);
                let t_click = arc_t(ang, a.start_angle, a.end_angle);
                if let Some(ext) = extend_arc(a, t_click, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::Arc(a.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a >= 1e-9 {
                    let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                    let t0 = e.start_parameter;
                    let mut t1 = e.end_parameter;
                    if t1 <= t0 {
                        t1 += TAU;
                    }
                    let rx = pt.x as f64 - e.center.x;
                    let ry = pt.y as f64 - e.center.y;
                    let xl = rx * nx + ry * ny;
                    let yl = -rx * ny + ry * nx;
                    let t_click = arc_t(yl.atan2(xl), t0, t1);
                    if let Some(ext) = extend_ellipse(e, t_click, &self.geos) {
                        return extend_hover_wires(
                        &EntityType::Ellipse(e.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                    }
                }
            }
            Some(EntityType::LwPolyline(p)) => {
                if let Some(ext) = extend_lwpoly(p, pt.x as f64, pt.y as f64, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::LwPolyline(p.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            Some(EntityType::Spline(s)) => {
                let t_click = spline_nearest_t(s, pt.x as f64, pt.y as f64)
                    .and_then(|t_actual| {
                        let (t0, t1) = spline_range(s)?;
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                if let Some(ext) = extend_spline(s, t_click, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::Spline(s.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            _ => {}
        }
        vec![]
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &mut self.mode {
            TrimMode::Fence(pick) => {
                pick.push([pt.x, pt.y]);
                CmdResult::NeedPoint
            }
            TrimMode::CrossFirst => {
                self.mode = TrimMode::CrossSecond([pt.x, pt.y]);
                CmdResult::NeedPoint
            }
            TrimMode::CrossSecond(p1) => {
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                self.mode = TrimMode::Pick;
                let rect = [p1, [p1[0], p2[1]], p2, [p2[0], p1[1]], p1];
                let window = CrossingWindow {
                    min: [p1[0].min(p2[0]), p1[1].min(p2[1])],
                    max: [p1[0].max(p2[0]), p1[1].max(p2[1])],
                    pick: p1,
                };
                self.fence_run(&rect, Some(window))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        match &self.mode {
            TrimMode::Fence(pick) if !pick.is_empty() => {
                let mut out = vec![pick.preview([pt.x, pt.y], "extend_fence")];
                let mut fpts = pick.path();
                fpts.push([pt.x, pt.y]);
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.geos,
                    &fpts,
                    None,
                    !self.shift,
                    self.implied_edges,
                ));
                out
            }
            TrimMode::CrossSecond(p1) => {
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                let mut out = vec![crossing_box_preview(p1, p2, "extend_cross")];
                let rect = [p1, [p1[0], p2[1]], p2, [p2[0], p1[1]], p1];
                let window = CrossingWindow {
                    min: [p1[0].min(p2[0]), p1[1].min(p2[1])],
                    max: [p1[0].max(p2[0]), p1[1].max(p2[1])],
                    pick: p1,
                };
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.geos,
                    &rect,
                    Some(window),
                    !self.shift,
                    self.implied_edges,
                ));
                out
            }
            _ => Vec::new(),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.mode, TrimMode::Pick) {
            TrimMode::SelectEdges => {
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            TrimMode::Fence(pick) if pick.is_usable() => self.fence_run(&pick.path(), None),
            TrimMode::Fence(_)
            | TrimMode::CrossFirst
            | TrimMode::CrossSecond(_)
            | TrimMode::Erase => CmdResult::NeedPoint,
            TrimMode::Pick => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        if !matches!(self.mode, TrimMode::Pick) {
            self.mode = TrimMode::Pick;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["EXTEND"] });  // ExtendCommand
inventory::submit!(crate::command::CommandRegistration { names: &["TRIM"] });  // TrimCommand

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn xline_at(x1: f64, y1: f64, x2: f64, y2: f64, h: u64) -> EntityType {
        let mut l = LineEnt::new();
        l.start = Vector3::new(x1, y1, 0.0);
        l.end = Vector3::new(x2, y2, 0.0);
        l.common.handle = Handle::new(h);
        EntityType::Line(l)
    }

    /// Edge: Extend — the target must lengthen to the BOUNDARY'S extrapolation
    /// when the boundary doesn't physically cross the target's path (#336).
    #[test]
    fn edge_extend_reaches_implied_boundary() {
        let target = xline_at(0.0, 0.0, 2.0, 0.0, 1);
        let boundary = xline_at(5.0, 2.0, 5.0, 8.0, 2);
        let all = vec![target.clone(), boundary];
        // Edge off: the boundary's drawn extent never meets y=0 — no extend.
        let geos = build_geos(&all);
        assert!(pick_extend_at(&all, &geos, Handle::new(1), 1.9, 0.0).is_none());
        // Edge on: the implied boundary crosses y=0 at x=5 — extend to (5,0).
        let mut implied = build_geos(&all);
        imply_edge_geos(&mut implied);
        match pick_extend_at(&all, &implied, Handle::new(1), 1.9, 0.0) {
            Some(EntityType::Line(l)) => {
                assert!((l.end.x - 5.0).abs() < 1e-6 && l.end.y.abs() < 1e-6,
                    "extends to the implied boundary, got ({}, {})", l.end.x, l.end.y);
            }
            other => panic!("expected an extended Line, got {other:?}"),
        }
    }

    /// #336 repro: two lines forming an X; a fence across one overhanging arm
    /// must trim that arm back to the intersection.
    #[test]
    fn fence_trims_x_arm() {
        let all = vec![
            xline_at(0.0, 0.0, 10.0, 10.0, 1),
            xline_at(0.0, 10.0, 10.0, 0.0, 2),
        ];
        let geos = build_geos(&all);
        let fence = [[8.0, 10.0], [10.0, 8.0]];
        let repl = fence_pass(&all, &geos, &fence, None, false);
        assert_eq!(repl.len(), 1, "exactly the crossed line is trimmed: {repl:?}");
        assert_eq!(repl[0].0, Handle::new(1));
        assert_eq!(repl[0].1.len(), 1);
        match &repl[0].1[0] {
            EntityType::Line(l) => {
                assert!((l.end.x - 5.0).abs() < 1e-6 && (l.end.y - 5.0).abs() < 1e-6,
                    "arm cut back to the intersection, got end ({}, {})", l.end.x, l.end.y);
            }
            other => panic!("expected a Line, got {other:?}"),
        }
    }

    fn circle(r: f64) -> CircleEnt {
        let mut c = CircleEnt::new();
        c.center = Vector3::new(0.0, 0.0, 0.0);
        c.radius = r;
        c
    }

    /// A circle crossed by a horizontal cutter (cuts at angle 0 and π, i.e.
    /// t = 0.0 and 0.5) becomes a single Arc; clicking the top half removes it
    /// and leaves the bottom half (π → 0 CCW), clicking the bottom does the
    /// reverse.
    #[test]
    fn trims_circle_into_arc_on_clicked_half() {
        let c = circle(10.0);
        let ts = [0.0, 0.5];

        // Click top (t = 0.25) → removes top, keeps bottom half (start π, end 0).
        let top = trim_circle(&c, &ts, 0.25);
        assert_eq!(top.len(), 1, "circle should trim to exactly one arc");
        match &top[0] {
            EntityType::Arc(a) => {
                assert!((norm(a.start_angle) - PI).abs() < 1e-9);
                assert!(norm(a.end_angle).abs() < 1e-9);
                assert_eq!(a.radius, 10.0);
                assert!(a.common.handle.is_null());
            }
            _ => panic!("expected an Arc"),
        }

        // Click bottom (t = 0.75) → keeps top half (start 0, end π).
        let bottom = trim_circle(&c, &ts, 0.75);
        match &bottom[0] {
            EntityType::Arc(a) => {
                assert!(norm(a.start_angle).abs() < 1e-9);
                assert!((norm(a.end_angle) - PI).abs() < 1e-9);
            }
            _ => panic!("expected an Arc"),
        }
    }

    /// Fewer than two crossings can't cut a closed circle, so it is left as-is.
    #[test]
    fn circle_with_one_or_zero_cuts_is_left_unchanged() {
        let c = circle(5.0);
        assert!(trim_circle(&c, &[], 0.3).is_empty());
        assert!(trim_circle(&c, &[0.4], 0.3).is_empty());
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ExtrimCommand — EXTRIM (Express-Tools cookie-cutter trim). #253
//
// Pick one boundary, then a side: every object crossing the boundary is trimmed
// on the picked side, and objects lying wholly on that side are erased. The side
// test is a parity count — a segment from a candidate point to the pick point
// that crosses the boundary an even number of times lands on the pick side.
// ══════════════════════════════════════════════════════════════════════════

/// Extend `Geo::Line` boundary edges so a line boundary cuts across the whole
/// drawing (EXTRIM treats a line boundary as infinite).
fn extend_line_geos(geos: &mut [Geo]) {
    for g in geos.iter_mut() {
        if let Geo::Line { handle, p1, p2 } = g {
            let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
            if dx.hypot(dy) > 1e-9 {
                // Same as `imply_edge_geos`: the boundary reaches as far as its
                // line does, so say so rather than picking a length.
                *g = Geo::InfLine {
                    handle: *handle,
                    bx: p1[0],
                    by: p1[1],
                    dx,
                    dy,
                };
            }
        }
    }
}

/// Kept parametric intervals — those whose midpoint is NOT on the pick side.
fn extrim_keep(
    ts: &[f64],
    point_at: &dyn Fn(f64) -> [f64; 2],
    on_pick_side: &dyn Fn([f64; 2]) -> bool,
) -> Vec<(f64, f64)> {
    let mut bounds = vec![0.0f64];
    bounds.extend_from_slice(ts);
    bounds.push(1.0);
    bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    bounds
        .windows(2)
        .filter_map(|w| {
            if w[1] - w[0] <= 1e-6 {
                return None;
            }
            let mid = point_at((w[0] + w[1]) * 0.5);
            if on_pick_side(mid) {
                None
            } else {
                Some((w[0], w[1]))
            }
        })
        .collect()
}

fn extrim_line(orig: &LineEnt, ts: &[f64], side: &dyn Fn([f64; 2]) -> bool) -> Vec<EntityType> {
    let p1 = [orig.start.x, orig.start.y];
    let p2 = [orig.end.x, orig.end.y];
    let z = orig.start.z;
    let pa = |t: f64| lerp2(p1, p2, t);
    extrim_keep(ts, &pa, side)
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

fn extrim_arc(orig: &ArcEnt, ts: &[f64], side: &dyn Fn([f64; 2]) -> bool) -> Vec<EntityType> {
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
    let (cx, cy, r) = (orig.center.x, orig.center.y, orig.radius);
    let pt = |t: f64| {
        let a = angle_at(t);
        [cx + r * a.cos(), cy + r * a.sin()]
    };
    extrim_keep(ts, &pt, side)
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

fn extrim_circle(orig: &CircleEnt, ts: &[f64], side: &dyn Fn([f64; 2]) -> bool) -> Vec<EntityType> {
    if ts.len() < 2 {
        return vec![];
    }
    let (cx, cy, r) = (orig.center.x, orig.center.y, orig.radius);
    let mut s = ts.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    let mut out = Vec::new();
    for i in 0..n {
        let ta = s[i];
        let tb = if i + 1 < n { s[i + 1] } else { s[0] + 1.0 };
        let mt = ((ta + tb) * 0.5).rem_euclid(1.0) * TAU;
        let mid = [cx + r * mt.cos(), cy + r * mt.sin()];
        if side(mid) {
            continue; // removed side
        }
        let mut arc = ArcEnt::new();
        arc.common = orig.common.clone();
        arc.common.handle = Handle::NULL;
        arc.center = orig.center;
        arc.radius = orig.radius;
        arc.thickness = orig.thickness;
        arc.normal = orig.normal;
        arc.start_angle = ta.rem_euclid(1.0) * TAU;
        arc.end_angle = tb.rem_euclid(1.0) * TAU;
        out.push(EntityType::Arc(arc));
    }
    out
}

/// Sample any entity to a dense XY polyline for the sampled trim path.
/// Plan-view sampling of any entity, at the density the editing previews
/// have always used.
///
/// One tessellation rather than a case per type. The per-type version this
/// replaced had drifted from the drawing's own: it sampled an arc at 16
/// segments per radian against the renderer's 20, sampled a spline at a
/// fixed 96 points however long it was, and read an arc's centre as world
/// coordinates when the entity stores it in its OCS.
///
/// Unbounded curves come back empty — a ray has no finite point list, and
/// how far to run it is the caller's decision. [`fence_sample_xy`] makes it.
fn sample_entity_xy(e: &EntityType) -> Vec<[f64; 2]> {
    // A polyline is sampled through its exploded segments so that a caller
    // walking the result sees the same seams the rest of TRIM does.
    if matches!(
        e,
        EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
    ) {
        let mut pts: Vec<[f64; 2]> = Vec::new();
        for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
            let sp = sample_entity_xy(&seg);
            if pts.last() == sp.first() {
                pts.extend_from_slice(&sp[1..]);
            } else {
                pts.extend(sp);
            }
        }
        return pts;
    }
    let Some(curve) = entity_curve_xy(e) else {
        return vec![];
    };
    if curve.extent() != KernelExtent::Bounded {
        return vec![];
    }
    curve.tessellate(SAMPLE_SEGMENTS_PER_RADIAN)
}

/// Dense XY sampling of any entity for the removal preview (lines are
/// subdivided and circles closed so the preview cut follows the boundary).
fn preview_sample_xy(e: &EntityType) -> Vec<[f64; 2]> {
    match e {
        EntityType::Line(l) => {
            let p1 = [l.start.x, l.start.y];
            let p2 = [l.end.x, l.end.y];
            (0..=24).map(|i| lerp2(p1, p2, i as f64 / 24.0)).collect()
        }
        EntityType::Circle(c) => {
            let steps = 64usize;
            (0..=steps)
                .map(|i| {
                    let a = TAU * (i as f64 / steps as f64);
                    [c.center.x + c.radius * a.cos(), c.center.y + c.radius * a.sin()]
                })
                .collect()
        }
        // Polylines sample only their vertices — subdivide the straight
        // segments so the preview cut follows the boundary instead of
        // jumping at the nearest vertex (#340).
        EntityType::LwPolyline(_)
        | EntityType::Polyline(_)
        | EntityType::Polyline2D(_)
        | EntityType::Polyline3D(_) => {
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
                let sp: Vec<[f64; 2]> = match &seg {
                    EntityType::Line(l) => {
                        let p1 = [l.start.x, l.start.y];
                        let p2 = [l.end.x, l.end.y];
                        (0..=24).map(|i| lerp2(p1, p2, i as f64 / 24.0)).collect()
                    }
                    _ => sample_entity_xy(&seg),
                };
                if pts.last() == sp.first() {
                    pts.extend_from_slice(&sp[1..]);
                } else {
                    pts.extend(sp);
                }
            }
            pts
        }
        _ => sample_entity_xy(e),
    }
}

/// Append the sub-runs of `pts` for which `take` holds to `out` as one wire's
/// point list, separated from earlier content by a NaN pen-up.
fn collect_runs(pts: &[[f64; 2]], take: &dyn Fn([f64; 2]) -> bool, out: &mut Vec<[f32; 3]>) {
    let mut run: Vec<[f64; 2]> = Vec::new();
    let flush = |run: &mut Vec<[f64; 2]>, out: &mut Vec<[f32; 3]>| {
        if run.len() >= 2 {
            if !out.is_empty() {
                out.push([f32::NAN, f32::NAN, f32::NAN]);
            }
            out.extend(run.iter().map(|p| [p[0] as f32, p[1] as f32, 0.0]));
        }
        run.clear();
    };
    for &p in pts {
        if take(p) {
            run.push(p);
        } else {
            flush(&mut run, out);
        }
    }
    flush(&mut run, out);
}

/// Build a preview wire from a NaN-break point list.
fn preview_wire(points: Vec<[f32; 3]>, color: [f32; 4], name: &str) -> WireModel {
    WireModel {
        point_marker: None,
        taper_widths: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        render_instance: None,
        pick_tris: Vec::new(),
        pick_tris_low: Vec::new(),
        dash_from_start: false,
        dash_align_end: None,
        text_verts: Vec::new(),
        name: name.into(),
        points,
        points_low: Vec::new(),
        color,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        snap_pts: vec![],
        tangent_geoms: vec![],
        aci: 0,
        key_vertices: vec![],
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        fill_tris: vec![],
        fill_tris_low: Vec::new(),
    }
}

/// Insert the exact boundary crossings between consecutive samples so a
/// sampled cut lands ON the boundary instead of at the nearest sample. A
/// polyline samples only its vertices, so without this a straight segment
/// crossing the boundary was cut at the wrong place — or not at all when
/// both endpoints were on the kept side (#340).
fn insert_boundary_crossings(pts: &[[f64; 2]], geos: &[Geo]) -> Vec<[f64; 2]> {
    let Some(last) = pts.last() else {
        return Vec::new();
    };
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        out.push(a);
        let mut ts = line_seg_ts(a[0], a[1], b[0], b[1], Handle::NULL, geos);
        ts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        for t in ts {
            if t > 1e-6 && t < 1.0 - 1e-6 {
                out.push(lerp2(a, b, t));
            }
        }
    }
    out.push(*last);
    out
}

/// Sampled trim: `None` leaves the entity unchanged, `Some(vec![])` erases it,
/// `Some(runs)` replaces it with the surviving pieces as LwPolylines.
/// Classification is per SEGMENT midpoint (samples + exact crossings), so a
/// vertex sitting exactly on the boundary can't flip the parity test.
fn extrim_sampled(
    pts: &[[f64; 2]],
    geos: &[Geo],
    side: &dyn Fn([f64; 2]) -> bool,
) -> Option<Vec<EntityType>> {
    if pts.len() < 2 {
        return None;
    }
    let dense = insert_boundary_crossings(pts, geos);
    let seg_kept: Vec<bool> = dense
        .windows(2)
        .map(|w| !side([(w[0][0] + w[1][0]) * 0.5, (w[0][1] + w[1][1]) * 0.5]))
        .collect();
    if seg_kept.iter().all(|&k| k) {
        return None; // wholly on the kept side — untouched
    }
    if seg_kept.iter().all(|&k| !k) {
        return Some(vec![]); // wholly on the pick side — erased
    }
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for (i, &kept) in seg_kept.iter().enumerate() {
        if kept {
            if cur.is_empty() {
                cur.push(dense[i]);
            }
            cur.push(dense[i + 1]);
        } else if cur.len() >= 2 {
            runs.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        runs.push(cur);
    }
    Some(
        runs.into_iter()
            .map(|run| {
                let mut pl = LwPolyline::new();
                pl.common.handle = Handle::NULL;
                pl.is_closed = false;
                pl.vertices = run
                    .into_iter()
                    .map(|p| LwVertex::from_coords(p[0], p[1]))
                    .collect();
                EntityType::LwPolyline(pl)
            })
            .collect(),
    )
}

pub struct ExtrimCommand {
    all: Vec<(Handle, EntityType)>,
    boundary: Option<Handle>,
    geos: Vec<Geo>,
}

impl ExtrimCommand {
    pub fn new(all: Vec<(Handle, EntityType)>) -> Self {
        let all = all
            .into_iter()
            .map(|(handle, entity)| (handle, entity_with_lwpolyline_world_xy(&entity)))
            .collect();
        Self { all, boundary: None, geos: Vec::new() }
    }
}

impl CadCommand for ExtrimCommand {
    fn name(&self) -> &'static str {
        "EXTRIM"
    }

    fn prompt(&self) -> String {
        if self.boundary.is_none() {
            crate::t!("EXTRIM  Select cutting boundary:").into_owned()
        } else {
            crate::t!("EXTRIM  Click the side to trim away:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.boundary.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some((_, e)) = self.all.iter().find(|(h, _)| *h == handle) else {
            return CmdResult::NeedPoint;
        };
        let mut geos = build_geos(std::slice::from_ref(e));
        // Only a picked LINE boundary is treated as infinite. A polyline
        // explodes into per-segment Line geos — extending those turns a
        // closed boundary into a grid of infinite lines and the parity
        // side-test breaks (#340).
        if matches!(e, EntityType::Line(_)) {
            extend_line_geos(&mut geos);
        }
        if geos.is_empty() {
            return CmdResult::NeedPoint; // not a usable boundary; keep asking
        }
        self.boundary = Some(handle);
        self.geos = geos;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(bh) = self.boundary else {
            return CmdResult::NeedPoint;
        };
        let q = [pt.x, pt.y];
        let geos = self.geos.clone();
        let side =
            |m: [f64; 2]| line_seg_ts(m[0], m[1], q[0], q[1], Handle::NULL, &geos).len() % 2 == 0;
        let mut repl: Vec<(Handle, Vec<EntityType>)> = Vec::new();
        for (h, e) in &self.all {
            if *h == bh {
                continue;
            }
            match e {
                EntityType::Line(l) => {
                    let ts = line_seg_ts(l.start.x, l.start.y, l.end.x, l.end.y, *h, &geos);
                    if ts.is_empty() {
                        let mid = [(l.start.x + l.end.x) * 0.5, (l.start.y + l.end.y) * 0.5];
                        if side(mid) {
                            repl.push((*h, vec![]));
                        }
                    } else {
                        repl.push((*h, extrim_line(l, &ts, &side)));
                    }
                }
                EntityType::Arc(a) => {
                    let ts = arc_seg_ts(
                        a.center.x,
                        a.center.y,
                        a.radius,
                        a.start_angle,
                        a.end_angle,
                        *h,
                        &geos,
                    );
                    if ts.is_empty() {
                        let am = norm(a.start_angle);
                        let mid =
                            [a.center.x + a.radius * am.cos(), a.center.y + a.radius * am.sin()];
                        if side(mid) {
                            repl.push((*h, vec![]));
                        }
                    } else {
                        repl.push((*h, extrim_arc(a, &ts, &side)));
                    }
                }
                EntityType::Circle(c) => {
                    let ts = arc_seg_ts(c.center.x, c.center.y, c.radius, 0.0, TAU, *h, &geos);
                    if ts.len() < 2 {
                        if side([c.center.x, c.center.y]) {
                            repl.push((*h, vec![]));
                        }
                    } else {
                        repl.push((*h, extrim_circle(c, &ts, &side)));
                    }
                }
                EntityType::LwPolyline(_)
                | EntityType::Polyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Polyline3D(_)
                | EntityType::Ellipse(_)
                | EntityType::Spline(_) => {
                    let pts = sample_entity_xy(e);
                    if let Some(res) = extrim_sampled(&pts, &geos, &side) {
                        repl.push((*h, res));
                    }
                }
                _ => {}
            }
        }
        if repl.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::ReplaceMany(repl, Vec::new())
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let Some(bh) = self.boundary else {
            return Vec::new();
        };
        if self.geos.is_empty() {
            return Vec::new();
        }
        let q = [pt.x, pt.y];
        let geos = &self.geos;
        let side =
            |m: [f64; 2]| line_seg_ts(m[0], m[1], q[0], q[1], Handle::NULL, geos).len() % 2 == 0;
        // Removed (pick side) → red, surviving → blue; the boundary → yellow.
        let mut removed: Vec<[f32; 3]> = Vec::new();
        let mut kept: Vec<[f32; 3]> = Vec::new();
        for (h, e) in &self.all {
            if *h == bh {
                continue;
            }
            let pts = preview_sample_xy(e);
            if pts.len() < 2 {
                continue;
            }
            collect_runs(&pts, &side, &mut removed);
            collect_runs(&pts, &|p| !side(p), &mut kept);
        }
        let mut boundary_pts: Vec<[f32; 3]> = Vec::new();
        if let Some((_, be)) = self.all.iter().find(|(h, _)| *h == bh) {
            boundary_pts = preview_sample_xy(be)
                .into_iter()
                .map(|p| [p[0] as f32, p[1] as f32, 0.0])
                .collect();
        }

        const YELLOW: [f32; 4] = [1.0, 0.90, 0.15, 1.0];
        const REMOVE_RED: [f32; 4] = [0.95, 0.30, 0.30, 1.0];
        let mut out = Vec::new();
        if boundary_pts.len() >= 2 {
            out.push(preview_wire(boundary_pts, YELLOW, "extrim_boundary"));
        }
        if kept.len() >= 2 {
            out.push(preview_wire(kept, WireModel::SELECTED, "extrim_keep"));
        }
        if removed.len() >= 2 {
            out.push(preview_wire(removed, REMOVE_RED, "extrim_remove"));
        }
        out
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["EXTRIM"]
}); // ExtrimCommand
