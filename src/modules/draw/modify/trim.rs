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

// The plane geometry these commands run on lives in opencadkernel; `geom` adapts
// its call shapes to the loose scalars and f32 render vertices used here.
use super::geom;
use super::geom::{
    arc_parameter as arc_t, arc_points as arc_pts, ellipse_closest_parameter,
    ellipse_points as ellipse_pts, lerp as lerp2, normalize_angle as norm,
};

use crate::modules::draw::fence::{crossing_box_preview, FencePick};

use codec::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline,
    LwVertex, Ray as RayEnt, Spline as SplineEnt, XLine as XLineEnt,
};
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::nurbs::clamped_uniform_knots;
use kernel::geom2d::{
    intersect as kernel_intersect, trim_spans as kernel_trim_spans, Arc as KernelArc, BulgeArc,
    Circle as KernelCircle, Curve, Ellipse as KernelEllipse, EllipseArc as KernelEllipseArc,
    Extent as KernelExtent, Line as KernelLine, NurbsCurve, Ray as KernelRay,
    Tolerance as KernelTolerance, XLine as KernelXLine,
};

use crate::entities::curve::{
    entity_curve_xy, entity_with_lwpolyline_world_xy, entity_with_lwpolyline_world_xy_owned,
};

use crate::command::{CadCommand, CmdResult};
use crate::modules::draw::modify::spline_ops::{
    spline_cut, spline_nearest_t, spline_pts_wire, spline_range, spline_to_nurbs, t_to_rel,
};
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;
use crate::scene::convert::tess::entity_world_aabb_f64;
use crate::t;

use super::entity_index::ModifyEntityIndex;

#[path = "trim_hatch_region.rs"]
mod hatch_region;

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
const SAMPLE_SEGMENTS_PER_RADIAN: f64 = kernel::geom2d::DEFAULT_SEGMENTS_PER_RADIAN;
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

    fn endpoints(&self) -> Vec<[f64; 2]> {
        match self {
            Self::Line { p1, p2, .. } => vec![*p1, *p2],
            Self::Arc {
                cx, cy, r, a0, a1, ..
            } => vec![
                [cx + r * a0.cos(), cy + r * a0.sin()],
                [cx + r * a1.cos(), cy + r * a1.sin()],
            ],
            Self::Ray { bx, by, .. } => vec![[*bx, *by]],
            Self::Ellipse {
                cx,
                cy,
                a,
                b,
                nx,
                ny,
                t0,
                t1,
                ..
            } => {
                if (t1 - t0).abs() < TAU - 1e-6 {
                    let pt_at = |t: f64| {
                        let ct = t.cos();
                        let st = t.sin();
                        [
                            cx + nx * a * ct - ny * b * st,
                            cy + ny * a * ct + nx * b * st,
                        ]
                    };
                    vec![pt_at(*t0), pt_at(*t1)]
                } else {
                    vec![]
                }
            }
            Self::Spline { curve, .. } => vec![curve.point_at(0.0), curve.point_at(1.0)],
            Self::Circle { .. } | Self::InfLine { .. } => vec![],
        }
    }
}

fn geo_overlaps_aabb(geo: &Geo, target: [f64; 4]) -> bool {
    let bounds = match geo {
        Geo::Line { p1, p2, .. } => [
            p1[0].min(p2[0]),
            p1[1].min(p2[1]),
            p1[0].max(p2[0]),
            p1[1].max(p2[1]),
        ],
        Geo::Arc { cx, cy, r, .. } | Geo::Circle { cx, cy, r, .. } => {
            [cx - r, cy - r, cx + r, cy + r]
        }
        Geo::Ellipse { cx, cy, a, b, .. } => {
            let radius = a.abs() + b.abs();
            [cx - radius, cy - radius, cx + radius, cy + radius]
        }
        Geo::Spline { curve, .. } => {
            let samples = (0..=32).map(|i| curve.point_at(i as f64 / 32.0));
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for point in samples {
                bounds[0] = bounds[0].min(point[0]);
                bounds[1] = bounds[1].min(point[1]);
                bounds[2] = bounds[2].max(point[0]);
                bounds[3] = bounds[3].max(point[1]);
            }
            bounds
        }
        // Unbounded boundaries are always retained in the nearby set.
        Geo::Ray { .. } | Geo::InfLine { .. } => return true,
    };
    bounds[0] <= target[2]
        && bounds[2] >= target[0]
        && bounds[1] <= target[3]
        && bounds[3] >= target[1]
}

pub fn is_trim_boundary_entity(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Line(_)
            | EntityType::Arc(_)
            | EntityType::Circle(_)
            | EntityType::Ellipse(_)
            | EntityType::Ray(_)
            | EntityType::XLine(_)
            | EntityType::Spline(_)
            | EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
    )
}
fn build_geos(entities: &[EntityType]) -> Vec<Geo> {
    build_geos_with_handle(entities, None)
}

/// Build boundary geometry while optionally attributing every segment to the
/// owning INSERT. Block contents are virtual TRIM boundaries; the INSERT stays
/// the real drawing entity and therefore remains untouched by the command.
fn build_geos_with_handle(entities: &[EntityType], owner: Option<Handle>) -> Vec<Geo> {
    let mut out = Vec::new();
    for e in entities {
        let h = owner.unwrap_or_else(|| e.common().handle);
        match e {
            // A polyline acts as a boundary through its constituent edges, so
            // a Line/Arc/… can be trimmed against it. Explode into Line + Arc
            // segments and tag each with the polyline's own handle (so trim
            // still excludes it as the click target).
            EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_) => {
                let before = out.len();
                for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
                    if let Some(g) = geo_from_entity(h, &seg) {
                        out.push(g);
                    }
                }
                // EXPLODE's LWPOLYLINE helper only materializes polylines
                // whose OCS plane is parallel to world XY. Imported files
                // can carry a tilted extrusion normal even when the entity is
                // intended to be used as a plan-view boundary. If that path
                // yields no usable edges, fall back to the shared curve
                // projection and use its tessellated segments as boundaries.
                if out.len() == before {
                    let points = projected_polyline_points_xy(e);
                    for pair in points.windows(2) {
                        if (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]) > 1e-12 {
                            out.push(Geo::Line {
                                handle: h,
                                p1: pair[0],
                                p2: pair[1],
                            });
                        }
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
        Geo::Arc {
            cx, cy, r, a0, a1, ..
        } => Curve::Arc(KernelArc {
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
            cx,
            cy,
            a,
            b,
            nx,
            ny,
            t0,
            t1,
            ..
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
                .map(|hit| {
                    if bounded {
                        hit.t_a.clamp(0.0, 1.0)
                    } else {
                        hit.t_a
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // ── Endpoint contact detection ──
    // When a boundary entity terminates on (or within snap tolerance of) the
    // target curve, register that contact point as a cut parameter.
    const ENDPOINT_TOLERANCE: f64 = CUT_TOLERANCE;
    let mut contacts = Vec::new();

    for geo in geos.iter().filter(|g| geo_handle(g) != handle) {
        // 1. Boundary endpoints touching target
        for pt in geo.endpoints() {
            let t = target.parameter_at(pt);
            let valid_t = if bounded {
                let slack = CUT_TOLERANCE;
                if (-slack..=1.0 + slack).contains(&t) {
                    Some(t.clamp(0.0, 1.0))
                } else {
                    None
                }
            } else {
                Some(t)
            };

            if let Some(t_cand) = valid_t {
                let proj = target.point_at(t_cand);
                let dx = pt[0] - proj[0];
                let dy = pt[1] - proj[1];
                if (dx * dx + dy * dy) <= ENDPOINT_TOLERANCE * ENDPOINT_TOLERANCE {
                    contacts.push(t_cand);
                }
            }
        }

        // 2. Target endpoints touching boundary
        if bounded && !target.is_closed() {
            if let Some(boundary_curve) = geo_to_curve(geo) {
                let boundary_bounded = matches!(boundary_curve.extent(), KernelExtent::Bounded);
                for &t_end in &[0.0, 1.0] {
                    let pt = target.point_at(t_end);
                    let u = boundary_curve.parameter_at(pt);
                    let valid_u = if boundary_bounded {
                        let slack = CUT_TOLERANCE;
                        if (-slack..=1.0 + slack).contains(&u) {
                            Some(u.clamp(0.0, 1.0))
                        } else {
                            None
                        }
                    } else {
                        Some(u)
                    };
                    if let Some(u_cand) = valid_u {
                        let proj = boundary_curve.point_at(u_cand);
                        let dx = pt[0] - proj[0];
                        let dy = pt[1] - proj[1];
                        if (dx * dx + dy * dy) <= ENDPOINT_TOLERANCE * ENDPOINT_TOLERANCE {
                            contacts.push(t_end);
                        }
                    }
                }
            }
        }
    }

    // Prioritize exact endpoint contact parameters over raw ray intersections,
    // ensuring trimmed endpoints match boundary endpoints down to machine precision.
    let is_closed = target.is_closed();
    for t_contact in contacts {
        if let Some(pos) = ts.iter().position(|&t| {
            let diff = (t - t_contact).abs();
            let cyclic = if is_closed {
                diff.min(1.0 - diff)
            } else {
                diff
            };
            cyclic < 1e-6
        }) {
            ts[pos] = t_contact;
        } else {
            ts.push(t_contact);
        }
    }

    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ts.dedup_by(|a, b| {
        let diff = (*a - *b).abs();
        let cyclic = if is_closed {
            diff.min(1.0 - diff)
        } else {
            diff
        };
        cyclic < 1e-6
    });

    // For closed curves, ensure the first and last cuts don't duplicate across the 0/1 seam
    if is_closed && ts.len() >= 2 {
        let seam_diff = (ts[ts.len() - 1] - (ts[0] + 1.0)).abs();
        if seam_diff < 1e-6 {
            ts.pop();
        }
    }

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

fn polyline_seg_ts(
    p0: [f64; 2],
    p1: [f64; 2],
    bulge: f64,
    target: Handle,
    geos: &[Geo],
) -> Vec<f64> {
    if let Some(ba) = BulgeArc::from_bulge(p0, p1, bulge) {
        let (from, to) = if ba.sweep >= 0.0 {
            (ba.start_angle, ba.start_angle + ba.sweep)
        } else {
            (ba.end_angle, ba.end_angle - ba.sweep)
        };
        let arc_curve = Curve::Arc(KernelArc {
            centre: ba.center,
            radius: ba.radius,
            start_angle: from,
            end_angle: to,
        });
        let ts = cut_params(&arc_curve, target, geos);
        let mut u_ts: Vec<f64> = if ba.sweep >= 0.0 {
            ts
        } else {
            ts.into_iter().map(|t| (1.0 - t).clamp(0.0, 1.0)).collect()
        };
        u_ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        u_ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        u_ts
    } else {
        line_seg_ts(p0[0], p0[1], p1[0], p1[1], target, geos)
    }
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
            let holds = |t: f64| t >= from && t < to;
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
            .push(codec::types::Vector3::new(hit_x, hit_y, z));
    } else {
        new_spl
            .control_points
            .insert(0, codec::types::Vector3::new(hit_x, hit_y, z));
    }
    // Rebuild knots (uniform) for the extended control polygon.
    let degree = new_spl.degree as usize;
    let n = new_spl.control_points.len();
    new_spl.knots = clamped_uniform_knots(degree, n);
    Some(EntityType::Spline(new_spl))
}

// ── Trim helpers ──────────────────────────────────────────────────────────

fn trim_intervals(curve: &Curve, ts: &[f64], t_click: f64) -> Vec<(f64, f64)> {
    kernel_trim_spans(curve, ts, t_click, KernelTolerance::new(CUT_TOLERANCE))
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

/// Return the part removed by this trim pick, found as the gap containing the
/// cursor parameter between the surviving line segments.
fn trimmed_line_removed_preview(
    orig: &LineEnt,
    survivors: &[EntityType],
    t_click: f64,
) -> Option<Vec<[f32; 3]>> {
    let dx = orig.end.x - orig.start.x;
    let dy = orig.end.y - orig.start.y;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return None;
    }
    let parameter = |x: f64, y: f64| {
        (((x - orig.start.x) * dx + (y - orig.start.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let mut spans: Vec<_> = survivors
        .iter()
        .filter_map(|entity| match entity {
            EntityType::Line(line) => {
                let a = parameter(line.start.x, line.start.y);
                let b = parameter(line.end.x, line.end.y);
                Some((a.min(b), a.max(b)))
            }
            _ => None,
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut gaps = Vec::new();
    let mut cursor = 0.0;
    for (start, end) in spans {
        if start > cursor + 1e-8 {
            gaps.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < 1.0 - 1e-8 {
        gaps.push((cursor, 1.0));
    }
    let t_click = t_click.clamp(0.0, 1.0);
    let (from, to) = gaps
        .into_iter()
        .find(|(from, to)| t_click >= from - 1e-8 && t_click <= to + 1e-8)?;
    let point = |t: f64| {
        [
            (orig.start.x + dx * t) as f32,
            (orig.start.y + dy * t) as f32,
            (orig.start.z + (orig.end.z - orig.start.z) * t) as f32,
        ]
    };
    Some(vec![point(from), point(to)])
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
        if (tc >= ta && tc < tb) || (tc + 1.0 >= ta && tc + 1.0 < tb) {
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

/// Extract a sub-polyline between parameter `s0` and `s1` (`s0 < s1`).
/// Preserves and proportionally scales arc bulges on partial segments.
fn extract_sub_polyline(poly: &LwPolyline, s0: f64, s1: f64) -> Option<LwPolyline> {
    if s1 <= s0 + 1e-9 {
        return None;
    }
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    if seg_count == 0 {
        return None;
    }

    let vx = |i: usize| -> [f64; 2] {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    let mut raw_verts: Vec<(f64, f64, f64)> = Vec::new();
    let mut curr = s0;
    while curr < s1 - 1e-9 {
        let seg_idx_unwrapped = curr.floor() as usize;
        let next_int = (seg_idx_unwrapped + 1) as f64;
        let end_curr = s1.min(next_int);
        let seg = seg_idx_unwrapped % seg_count;
        let u_a = (curr - seg_idx_unwrapped as f64).clamp(0.0, 1.0);
        let u_b = (end_curr - seg_idx_unwrapped as f64).clamp(0.0, 1.0);
        if u_b > u_a + 1e-9 {
            let p0 = vx(seg);
            let p1 = vx(seg + 1);
            let b = seg_bulge(seg);
            let (start_pt, sub_bulge) = if let Some(ba) = BulgeArc::from_bulge(p0, p1, b) {
                let sp = ba.sample(u_a);
                let sub_b = if (u_a - 0.0).abs() < 1e-9 && (u_b - 1.0).abs() < 1e-9 {
                    b
                } else {
                    let sweep = ba.sweep * (u_b - u_a);
                    (sweep * 0.25).tan()
                };
                (sp, sub_b)
            } else {
                let sp = [p0[0] + u_a * (p1[0] - p0[0]), p0[1] + u_a * (p1[1] - p0[1])];
                (sp, 0.0)
            };
            raw_verts.push((start_pt[0], start_pt[1], sub_bulge));
        }
        curr = end_curr;
    }

    // End point of the last sub-segment
    let end_pt = {
        let seg_idx_unwrapped = (s1 - 1e-9).floor() as usize;
        let seg = seg_idx_unwrapped % seg_count;
        let u = (s1 - seg_idx_unwrapped as f64).clamp(0.0, 1.0);
        let p0 = vx(seg);
        let p1 = vx(seg + 1);
        let b = seg_bulge(seg);
        if let Some(ba) = BulgeArc::from_bulge(p0, p1, b) {
            ba.sample(u)
        } else {
            [p0[0] + u * (p1[0] - p0[0]), p0[1] + u * (p1[1] - p0[1])]
        }
    };
    raw_verts.push((end_pt[0], end_pt[1], 0.0));

    // Filter out coincident consecutive points to prevent degenerate zero-length segments
    let mut clean_verts: Vec<(f64, f64, f64)> = Vec::new();
    for (x, y, b) in raw_verts {
        if let Some(last) = clean_verts.last_mut() {
            if (last.0 - x).hypot(last.1 - y) < 1e-6 {
                if last.2 == 0.0 {
                    last.2 = b;
                }
                continue;
            }
        }
        clean_verts.push((x, y, b));
    }

    if let Some(last) = clean_verts.last_mut() {
        last.2 = 0.0;
    }

    if clean_verts.len() < 2 {
        return None;
    }

    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    new_poly.is_closed = false;
    new_poly.vertices = clean_verts
        .into_iter()
        .map(|(x, y, b)| {
            let mut v = LwVertex::from_coords(x, y);
            v.bulge = b;
            v
        })
        .collect();
    Some(new_poly)
}

/// Kernel curve for one LwPolyline segment.
///
/// The boolean tells whether the kernel curve runs opposite to the polyline's
/// own segment direction. Negative-bulge arcs are represented by the kernel as
/// a positive-sweep arc, so their intersection parameter has to be reversed
/// back to the polyline's local 0..1 parameter.
fn lwpoly_segment_curve(poly: &LwPolyline, segment: usize) -> Option<(Curve, bool)> {
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }

    let a = &poly.vertices[segment % n];
    let b = &poly.vertices[(segment + 1) % n];

    let p0 = [a.location.x, a.location.y];
    let p1 = [b.location.x, b.location.y];

    if (p1[0] - p0[0]).hypot(p1[1] - p0[1]) < 1e-12 {
        return None;
    }

    if let Some(arc) = BulgeArc::from_bulge(p0, p1, a.bulge) {
        if arc.sweep >= 0.0 {
            Some((
                Curve::Arc(KernelArc {
                    centre: arc.center,
                    radius: arc.radius,
                    start_angle: arc.start_angle,
                    end_angle: arc.start_angle + arc.sweep,
                }),
                false,
            ))
        } else {
            Some((
                Curve::Arc(KernelArc {
                    centre: arc.center,
                    radius: arc.radius,
                    start_angle: arc.end_angle,
                    end_angle: arc.end_angle - arc.sweep,
                }),
                true,
            ))
        }
    } else {
        Some((Curve::Line(KernelLine { start: p0, end: p1 }), false))
    }
}

/// Global polyline parameters at which non-adjacent segments of the same
/// polyline intersect.
///
/// Segment neighbours are deliberately excluded: their common vertex is a
/// normal polyline vertex, not a self-intersection. A polyline cuts itself
/// only when it is one of the cutting edges.
fn lwpoly_self_cut_params(poly: &LwPolyline, geos: &[Geo]) -> Vec<f64> {
    let n = poly.vertices.len();
    if n < 3 || !geos.iter().any(|geo| geo.handle() == poly.common.handle) {
        return Vec::new();
    }

    let closed = poly.is_closed;
    let segment_count = if closed { n } else { n - 1 };
    let total = segment_count as f64;

    let mut cuts = Vec::new();

    for i in 0..segment_count {
        let Some((curve_a, reverse_a)) = lwpoly_segment_curve(poly, i) else {
            continue;
        };

        for j in (i + 1)..segment_count {
            // Ordinary neighbouring segments meet at their common vertex.
            let adjacent = j == i + 1 || (closed && i == 0 && j + 1 == segment_count);

            if adjacent {
                continue;
            }

            let Some((curve_b, reverse_b)) = lwpoly_segment_curve(poly, j) else {
                continue;
            };

            let hits = kernel_intersect(&curve_a, &curve_b, KernelTolerance::new(CUT_TOLERANCE));

            for hit in hits {
                let mut ua = hit.t_a;
                let mut ub = hit.t_b;

                if reverse_a {
                    ua = 1.0 - ua;
                }
                if reverse_b {
                    ub = 1.0 - ub;
                }

                if !ua.is_finite()
                    || !ub.is_finite()
                    || ua < -1e-7
                    || ua > 1.0 + 1e-7
                    || ub < -1e-7
                    || ub > 1.0 + 1e-7
                {
                    continue;
                }

                ua = ua.clamp(0.0, 1.0);
                ub = ub.clamp(0.0, 1.0);

                // If both sides only touch at existing vertices, those
                // vertices already act as trim limits below.
                let a_endpoint = ua <= 1e-7 || ua >= 1.0 - 1e-7;
                let b_endpoint = ub <= 1e-7 || ub >= 1.0 - 1e-7;

                if a_endpoint && b_endpoint {
                    continue;
                }

                let pa = i as f64 + ua;
                let pb = j as f64 + ub;

                cuts.push(if closed { pa.rem_euclid(total) } else { pa });

                cuts.push(if closed { pb.rem_euclid(total) } else { pb });
            }
        }
    }

    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    cuts
}

/// Trim a clicked LwPolyline: remove the portion containing the click, bounded
/// by the nearest boundary intersections on each side. A closed polyline needs
/// ≥2 cuts and becomes an open polyline (the surviving arc); an open one yields
/// the surviving piece(s). Bulges on surviving arc segments are preserved
/// with proportionally scaled curvature.
fn trim_lwpolyline(poly: &LwPolyline, cx: f64, cy: f64, geos: &[Geo]) -> Option<Vec<EntityType>> {
    let handle = poly.common.handle;
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }
    let closed = poly.is_closed;
    let seg_count = if closed { n } else { n - 1 };
    let total = seg_count as f64;

    let vx = |i: usize| -> [f64; 2] {
        let v = &poly.vertices[i % n];
        [v.location.x, v.location.y]
    };
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    // External boundary cuts as global params (segment index + local u).
    let mut cuts: Vec<f64> = Vec::new();

    for i in 0..seg_count {
        let p0 = vx(i);
        let p1 = vx(i + 1);
        let b = seg_bulge(i);

        for u in polyline_seg_ts(p0, p1, b, handle, geos) {
            let param = i as f64 + u.clamp(0.0, 1.0);

            cuts.push(if closed {
                param.rem_euclid(total)
            } else {
                param
            });
        }
    }

    // A polyline can also cut itself. These are intersections between
    // non-adjacent segments belonging to this same LwPolyline. Its vertices
    // are not cuts: the trimmed piece runs on past corners to the next
    // cutting edge, and a polyline no edge crosses is left alone.
    cuts.extend(lwpoly_self_cut_params(poly, geos));

    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    if cuts.is_empty() {
        return None;
    }

    // Click param: nearest point on the polyline.
    let mut best = (f64::INFINITY, 0.0_f64);
    for i in 0..seg_count {
        let p0 = vx(i);
        let p1 = vx(i + 1);
        let b = seg_bulge(i);
        let (d, u) = if let Some(ba) = BulgeArc::from_bulge(p0, p1, b) {
            let angle = (cy - ba.center[1]).atan2(cx - ba.center[0]);
            let (travelled, sweep) = if ba.sweep >= 0.0 {
                ((angle - ba.start_angle).rem_euclid(TAU), ba.sweep)
            } else {
                ((ba.start_angle - angle).rem_euclid(TAU), -ba.sweep)
            };
            let u = if sweep < 1e-12 {
                0.0
            } else if travelled <= sweep {
                (travelled / sweep).clamp(0.0, 1.0)
            } else {
                let gap = TAU - sweep;
                let past_end = travelled - sweep;
                if past_end <= gap * 0.5 {
                    1.0
                } else {
                    0.0
                }
            };
            let pt = ba.sample(u);
            let dist2 = (pt[0] - cx).powi(2) + (pt[1] - cy).powi(2);
            (dist2, u)
        } else {
            let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
            let len2 = dx * dx + dy * dy;
            let u = if len2 > 1e-12 {
                (((cx - p0[0]) * dx + (cy - p0[1]) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let px = p0[0] + u * dx;
            let py = p0[1] + u * dy;
            let dist2 = (px - cx).powi(2) + (py - cy).powi(2);
            (dist2, u)
        };
        if d < best.0 {
            best = (d, i as f64 + u);
        }
    }
    let t_click = best.1;

    let mut out: Vec<EntityType> = Vec::new();
    if closed {
        if cuts.len() < 2 {
            return None;
        }
        let n = cuts.len();
        let mut removed = None;
        for i in 0..n {
            let a = cuts[i];
            let b = if i + 1 < n {
                cuts[i + 1]
            } else {
                cuts[0] + total
            };
            if (t_click >= a && t_click < b) || (t_click + total >= a && t_click + total < b) {
                removed = Some((a, b));
                break;
            }
        }
        let Some((lo, hi)) = removed else {
            return None;
        };
        let mut s1 = lo;
        while s1 <= hi {
            s1 += total;
        }
        if let Some(piece) = extract_sub_polyline(poly, hi, s1) {
            out.push(EntityType::LwPolyline(piece));
        }
    } else {
        let mut bounds = vec![0.0];
        bounds.extend(cuts.iter().copied());
        bounds.push(total);
        bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        let n = bounds.len();
        let mut removed = None;
        for i in 0..n - 1 {
            let a = bounds[i];
            let b = bounds[i + 1];
            if (t_click >= a && t_click < b) || (i + 2 == n && t_click <= b) {
                removed = Some((a, b));
                break;
            }
        }
        let Some((lo, hi)) = removed else {
            return None;
        };
        if lo > 1e-6 {
            if let Some(piece) = extract_sub_polyline(poly, 0.0, lo) {
                out.push(EntityType::LwPolyline(piece));
            }
        }
        if hi < total - 1e-6 {
            if let Some(piece) = extract_sub_polyline(poly, hi, total) {
                out.push(EntityType::LwPolyline(piece));
            }
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
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
    if n < 2 || poly.is_closed {
        return None;
    }

    let first = &poly.vertices[0];
    let second = &poly.vertices[1];
    let last = &poly.vertices[n - 1];
    let prev = &poly.vertices[n - 2];

    let d_first = (first.location.x - click_x).hypot(first.location.y - click_y);
    let d_last = (last.location.x - click_x).hypot(last.location.y - click_y);
    let extend_end = d_last <= d_first;

    // A bulged terminal segment is a circular arc, not its chord. Extend it
    // on the same circle and update the bulge to represent the longer sweep.
    let segment = if extend_end { n - 2 } else { 0 };
    let start = &poly.vertices[segment];
    let end = &poly.vertices[segment + 1];
    if let Some(arc) = BulgeArc::from_bulge(
        [start.location.x, start.location.y],
        [end.location.x, end.location.y],
        start.bulge,
    ) {
        let sign = arc.sweep.signum();
        if sign == 0.0 {
            return None;
        }
        let terminal_angle = if extend_end {
            arc.start_angle + arc.sweep
        } else {
            arc.start_angle
        };
        // Crossings around the full circle from the terminal point, as CCW
        // fractions of a turn; a CW walk measures them from the far side.
        let ccw = extend_end == (sign > 0.0);
        let [cx, cy] = arc.center;
        let ts = arc_seg_ts(
            cx,
            cy,
            arc.radius,
            terminal_angle,
            terminal_angle + TAU,
            poly.common.handle,
            geos,
        );
        let travel = ts
            .into_iter()
            .map(|t| (if ccw { t } else { 1.0 - t }) * TAU)
            .filter(|&travel| travel > 1e-6)
            .min_by(f64::total_cmp)?;
        let end_angle = if ccw {
            terminal_angle + travel
        } else {
            terminal_angle - travel
        };
        let point = [
            cx + arc.radius * end_angle.cos(),
            cy + arc.radius * end_angle.sin(),
        ];
        // A bulge encodes a sweep shorter than one full revolution. Do not
        // manufacture an invalid/infinite bulge when the next crossing would
        // require a full circle or more.
        let new_sweep = arc.sweep.abs() + travel;
        if new_sweep >= TAU - 1e-6 {
            return None;
        }
        let mut new_poly = poly.clone();
        new_poly.common.handle = Handle::NULL;
        let new_bulge = (new_sweep * 0.25).tan() * sign;
        if extend_end {
            let vertex = new_poly.vertices.last_mut()?;
            vertex.location.x = point[0];
            vertex.location.y = point[1];
            new_poly.vertices[n - 2].bulge = new_bulge;
        } else {
            let vertex = new_poly.vertices.first_mut()?;
            vertex.location.x = point[0];
            vertex.location.y = point[1];
            new_poly.vertices[0].bulge = new_bulge;
        }
        return Some(EntityType::LwPolyline(new_poly));
    }

    // Extract the terminal straight segment as a virtual line.
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

const TRIM_REMOVE_GRAY: [f32; 4] = [0.40, 0.40, 0.40, 0.88];

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
            let xy_pts = preview_sample_xy(e);
            xy_pts
                .into_iter()
                .map(|pt| [pt[0] as f32, pt[1] as f32, elev])
                .collect()
        }
        EntityType::Polyline(_) | EntityType::Polyline2D(_) | EntityType::Polyline3D(_) => {
            preview_sample_xy(e)
                .into_iter()
                .map(|pt| [pt[0] as f32, pt[1] as f32, 0.0])
                .collect()
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
    /// mOde: choosing between Quick and Standard (TRIMEXTENDMODE).
    ChooseMode,
}

/// TRIMEXTENDMODE: 1 (Quick) starts TRIM and EXTEND ready to pick, every
/// object a cutting or boundary edge; 0 (Standard) starts them selecting
/// those edges first.
static QUICK_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn quick_mode() -> bool {
    QUICK_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_quick_mode(quick: bool) {
    QUICK_MODE.store(quick, std::sync::atomic::Ordering::Relaxed);
}

/// The mOde prompt, offering the current mode as the default.
fn mode_prompt(command: &str) -> String {
    let current = if quick_mode() { "Quick" } else { "Standard" };
    crate::tf!("{command}  Enter a trim mode option [Quick/Standard] <{current}>:").into_owned()
}

fn mode_options() -> Vec<crate::command::CmdOption> {
    use crate::command::CmdOption;
    vec![
        CmdOption::new("Quick", "Q"),
        CmdOption::new("Standard", "S"),
    ]
}

/// The mOde option and its answer, shared by TRIM and EXTEND. Returns the
/// step's result and whether the edges must be rebuilt, or `None` for input
/// that is not about the mode.
fn choose_mode(
    mode: &mut TrimMode,
    edge_set: &mut Vec<Handle>,
    text: &str,
) -> Option<(CmdResult, bool)> {
    let word = text.trim().to_uppercase();
    match (&*mode, word.as_str()) {
        (TrimMode::Pick, "O" | "MODE") => *mode = TrimMode::ChooseMode,
        (TrimMode::ChooseMode, "Q" | "QUICK") => {
            set_quick_mode(true);
            edge_set.clear();
            *mode = TrimMode::Pick;
            return Some((CmdResult::NeedPoint, true));
        }
        (TrimMode::ChooseMode, "S" | "STANDARD") => {
            set_quick_mode(false);
            edge_set.clear();
            *mode = TrimMode::SelectEdges;
        }
        (TrimMode::ChooseMode, _) => {
            let message = crate::t!("Invalid option keyword.").into_owned();
            return Some((CmdResult::ReportError(message), false));
        }
        _ => return None,
    }
    Some((CmdResult::NeedPoint, false))
}

/// The first step: picking in Quick mode or when edges were preselected,
/// selecting edges in Standard mode.
fn starting_mode(initial_edges: &[Handle]) -> TrimMode {
    if quick_mode() || !initial_edges.is_empty() {
        TrimMode::Pick
    } else {
        TrimMode::SelectEdges
    }
}

#[derive(Clone, Copy)]
struct CrossingWindow {
    min: [f64; 2],
    max: [f64; 2],
}

fn segment_window_range(p1: [f64; 2], p2: [f64; 2], window: CrossingWindow) -> Option<(f64, f64)> {
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for axis in 0..2 {
        let d = p2[axis] - p1[axis];
        if d.abs() < 1e-12 {
            if p1[axis] < window.min[axis] - 1e-9 || p1[axis] > window.max[axis] + 1e-9 {
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
    let seg_bulge = |i: usize| -> f64 { poly.vertices[i % n].bulge };

    let mut removed = Vec::<(f64, f64)>::new();

    // Compute the polyline's own non-adjacent segment intersections once.
    // These global parameters will later be mapped back into each source segment.
    let self_cuts = lwpoly_self_cut_params(poly, geos);

    for i in 0..seg_count {
        let a = vertex_xy(i);
        let b = vertex_xy(i + 1);
        let bulge = seg_bulge(i);
        let window_range = if let Some(ba) = BulgeArc::from_bulge(a, b, bulge) {
            let steps = 32usize;
            let mut min_t: Option<f64> = None;
            let mut max_t: Option<f64> = None;
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let pt = ba.sample(t);
                if pt[0] >= window.min[0] - 1e-9
                    && pt[0] <= window.max[0] + 1e-9
                    && pt[1] >= window.min[1] - 1e-9
                    && pt[1] <= window.max[1] + 1e-9
                {
                    min_t = Some(min_t.map_or(t, |m: f64| m.min(t)));
                    max_t = Some(max_t.map_or(t, |m: f64| m.max(t)));
                }
            }
            match (min_t, max_t) {
                (Some(lo), Some(hi)) => Some((lo, hi)),
                _ => None,
            }
        } else {
            segment_window_range(a, b, window)
        };
        let Some((inside_lo, inside_hi)) = window_range else {
            continue;
        };
        // External intersections on this segment.
        let mut cuts = polyline_seg_ts(a, b, bulge, handle, geos);

        // Also include self-intersections belonging to this source segment.
        // `lwpoly_self_cut_params()` stores them as global polyline parameters:
        //     segment_index + local_parameter
        //
        // Convert the ones belonging to segment `i` back to local 0..1.
        let seg_start = i as f64;
        let seg_end = seg_start + 1.0;

        for global_t in self_cuts.iter().copied() {
            if global_t > seg_start + 1e-6 && global_t < seg_end - 1e-6 {
                cuts.push(global_t - seg_start);
            }
        }

        // The source segment endpoints are also valid trim limits.
        cuts.push(0.0);
        cuts.push(1.0);

        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);

        // Remove every trim span that is actually touched by the selection box.
        //
        // A single source segment may contain several sub-spans when external
        // boundaries cross it. Only the sub-spans overlapping the window are removed.
        for span in cuts.windows(2) {
            let lo = span[0].clamp(0.0, 1.0);
            let hi = span[1].clamp(0.0, 1.0);

            if hi - lo <= 1e-6 {
                continue;
            }

            // This trim span participates when its parameter interval overlaps the
            // portion of the source segment lying inside the crossing rectangle.
            let overlaps_window = hi >= inside_lo - 1e-6 && lo <= inside_hi + 1e-6;

            if overlaps_window {
                removed.push((i as f64 + lo, i as f64 + hi));
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
            .filter_map(|(start, end)| {
                extract_sub_polyline(poly, start, end).map(EntityType::LwPolyline)
            })
            .collect(),
    )
}

/// A detached circular fill is itself one complete closed TRIM fragment.
/// Unlike a rectangular fill with a hole, it can be removed with a single
/// click without selecting another cutting edge. Do not delete arbitrary
/// multi-loop / polygonal hatches when a trim solver finds no intersection.
fn hatch_is_standalone_circle(hatch: &codec::entities::Hatch) -> bool {
    if hatch.paths.len() != 1 { return false; }
    let edges = &hatch.paths[0].edges;
    if edges.len() != 1 { return false; }
    let codec::entities::BoundaryEdge::CircularArc(arc) = &edges[0] else {
        return false;
    };
    arc.radius.is_finite()
        && arc.radius > 0.0
        && ((arc.end_angle - arc.start_angle) - std::f64::consts::TAU).abs() < 1.0e-6
}

/// Cut a polygonal HATCH with an exact CIRCLE: create an internal round
/// opening, or replace part of its outer boundary with an analytic arc.
/// Do not approximate the final arc with chords (DWG must retain a true arc).
/// Return None for complex topology that would require a region boolean.
fn trim_hatch_circle(
    hatch: &codec::entities::Hatch,
    rings: &[Vec<[f64; 2]>],
    outer_index: usize,
    circle: &Geo,
    pick: [f64; 2],
) -> Option<codec::entities::Hatch> {
    use codec::entities::{BoundaryEdge, BoundaryPath, BoundaryPathFlags, CircularArcEdge, LineEdge};
    use codec::types::Vector2;
    use std::f64::consts::TAU;
    let Geo::Circle { handle, cx, cy, r } = *circle else {
        return None;
    };
    if handle == hatch.common.handle || !r.is_finite() || r <= 1.0e-9 {
        return None;
    }
    let centre = [cx, cy];
    let d2 = |p: [f64; 2]| (p[0] - cx).powi(2) + (p[1] - cy).powi(2);
    let tolerance = 1.0e-8_f64.max(r * 1.0e-9);
    let r2 = r * r;
    // A click INSIDE the cutter removes the circular portion of the HATCH.
    // Clicking outside must not unexpectedly remove the entire hatch.
    if d2(pick) >= (r - tolerance).max(0.0).powi(2) {
        return None;
    }
    let polygon_contains = |ring: &[[f64; 2]], p: [f64; 2]| {
        let mut inside = false;
        for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len()) {
            if (a[1] > p[1]) != (b[1] > p[1]) {
                let cross_x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
                if p[0] < cross_x {
                    inside = !inside;
                }
            }
        }
        inside
    };
    let ring = &rings[outer_index];
    if ring.len() < 3 || !polygon_contains(ring, pick) {
        return None;
    }
    // The incoming tessellation can repeat the first point at the end.
    let mut points = ring.clone();
    if points.len() >= 2
        && (points[0][0] - points.last().unwrap()[0])
            .hypot(points[0][1] - points.last().unwrap()[1]) <= tolerance
    {
        points.pop();
    }
    if points.len() < 3 {
        return None;
    }
    #[derive(Clone, Copy)]
    struct Hit {
        at: f64,
        point: [f64; 2],
    }
    let hits_on = |polygon: &[[f64; 2]]| -> Vec<Hit> {
        let mut hits = Vec::new();
        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];
            let dir = [b[0] - a[0], b[1] - a[1]];
            let f = [a[0] - cx, a[1] - cy];
            let aa = dir[0] * dir[0] + dir[1] * dir[1];
            if aa <= tolerance * tolerance {
                continue;
            }
            let bb = 2.0 * (dir[0] * f[0] + dir[1] * f[1]);
            let cc = f[0] * f[0] + f[1] * f[1] - r2;
            let discriminant = bb * bb - 4.0 * aa * cc;
            if discriminant <= tolerance * tolerance {
                continue;
            }
            for t in [(-bb - discriminant.sqrt()) / (2.0 * aa),
                      (-bb + discriminant.sqrt()) / (2.0 * aa)] {
                if t < -tolerance || t > 1.0 + tolerance {
                    continue;
                }
                let t = t.clamp(0.0, 1.0);
                let point = [a[0] + t * dir[0], a[1] + t * dir[1]];
                if !hits.iter().any(|prev: &Hit| {
                    (point[0] - prev.point[0]).hypot(point[1] - prev.point[1]) <= tolerance
                }) {
                    hits.push(Hit { at: i as f64 + t, point });
                }
            }
        }
        hits.sort_by(|a, b| a.at.total_cmp(&b.at));
        hits
    };
    let hits = hits_on(&points);
    // Don't attempt to combine a cutter with existing holes. The hole must
    // remain completely separate from this disk.
    for (index, hole) in rings.iter().enumerate() {
        if index == outer_index { continue; }
        if hits_on(hole).len() != 0
            || hole.iter().any(|p| d2(*p) <= (r + tolerance).powi(2))
            || polygon_contains(hole, centre)
        {
            return None;
        }
    }
    let mut result = hatch.clone();
    result.common.handle = Handle::NULL;
    result.is_associative = false;
    match hits.len() {
        0 => {
            // A circle completely inside the outer region becomes an exact
            // circular island/hole. The circle itself is left untouched.
            if !polygon_contains(&points, centre) {
                return None;
            }
            for offset in [[r, 0.0], [-r, 0.0], [0.0, r], [0.0, -r]] {
                if !polygon_contains(&points, [cx + offset[0], cy + offset[1]]) {
                    return None;
                }
            }
            let mut hole = BoundaryPath::new();
            hole.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(cx, cy),
                radius: r,
                start_angle: 0.0,
                end_angle: TAU,
                counter_clockwise: true,
            }));
            result.paths.push(hole);
        }
        2 => {
            let n = points.len() as f64;
            let forward_chain = |a: Hit, b: Hit| -> Vec<[f64; 2]> {
                let finish = if b.at <= a.at { b.at + n } else { b.at };
                let mut chain = vec![a.point];
                let mut vertex = a.at.floor() + 1.0;
                while vertex < finish - tolerance && chain.len() <= points.len() + 2 {
                    chain.push(points[vertex as usize % points.len()]);
                    vertex += 1.0;
                }
                chain.push(b.point);
                chain
            };
            let a = forward_chain(hits[0], hits[1]);
            let b = forward_chain(hits[1], hits[0]);
            let outside = |chain: &[[f64; 2]]| {
                chain.windows(2).all(|pair| {
                    let p = [(pair[0][0] + pair[1][0]) * 0.5,
                             (pair[0][1] + pair[1][1]) * 0.5];
                    d2(p) >= (r - tolerance).max(0.0).powi(2)
                })
            };
            let chain = match (outside(&a), outside(&b)) {
                (true, false) => a,
                (false, true) => b,
                _ => return None,
            };
            if chain.len() < 2 || chain.len() > points.len() + 2 {
                return None;
            }
            // Close the outer path with the arc on the INSIDE of the old
            // polygon, NOT the opposite circle arc outside the filled area.
            let arc_start = *chain.last().unwrap();
            let arc_end = chain[0];
            let t0 = (arc_start[1] - cy).atan2(arc_start[0] - cx);
            let t1 = (arc_end[1] - cy).atan2(arc_end[0] - cx);
            let ccw_sweep = (t1 - t0).rem_euclid(TAU);
            let cw_sweep = (t0 - t1).rem_euclid(TAU);
            let midpoint = |angle: f64| [cx + r * angle.cos(), cy + r * angle.sin()];
            let ccw_inside = polygon_contains(&points, midpoint(t0 + ccw_sweep * 0.5));
            let cw_inside = polygon_contains(&points, midpoint(t0 - cw_sweep * 0.5));
            let counter_clockwise = match (ccw_inside, cw_inside) {
                (true, false) => true,
                (false, true) => false,
                _ => return None,
            };
            let sweep = if counter_clockwise { ccw_sweep } else { cw_sweep };
            if sweep <= tolerance || sweep >= TAU - tolerance {
                return None;
            }
            let stored_start = if counter_clockwise { t0 } else { -t0 }.rem_euclid(TAU);
            let mut path = BoundaryPath::with_flags(BoundaryPathFlags::OUTERMOST);
            for pair in chain.windows(2) {
                if (pair[0][0] - pair[1][0]).hypot(pair[0][1] - pair[1][1]) > tolerance {
                    path.add_edge(BoundaryEdge::Line(LineEdge {
                        start: Vector2::new(pair[0][0], pair[0][1]),
                        end: Vector2::new(pair[1][0], pair[1][1]),
                    }));
                }
            }
            path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                center: Vector2::new(cx, cy),
                radius: r,
                start_angle: stored_start,
                end_angle: stored_start + sweep,
                counter_clockwise,
            }));
            result.paths[outer_index] = path;
        }
        // More intersections can split the hatch into several disconnected
        // pieces or merge holes. Until a full region boolean exists, refuse.
        _ => return None,
    }
    for path in &mut result.paths {
        path.boundary_handles.clear();
        path.flags.set_external(false);
    }
    Some(result)
}

/// Trim a boundary with mixed exact LINE and CIRCULAR ARC edges against
/// one straight cutting segment. Work in parameter space: retain the chain
/// on the opposite side of the click and close it along the cutter. A new
/// CircularArcEdge is emitted for the surviving portion of any split arc.
fn trim_hatch_curved_outer(
    path: &codec::entities::BoundaryPath,
    p1: [f64; 2],
    p2: [f64; 2],
    keep_sign: f64,
) -> Option<codec::entities::BoundaryPath> {
    use codec::entities::{BoundaryEdge, BoundaryPath, BoundaryPathFlags, CircularArcEdge, LineEdge};
    use codec::types::Vector2;
    const EPS: f64 = 1.0e-8;
    const TAU: f64 = std::f64::consts::TAU;
    #[derive(Clone, Copy)]
    enum Segment {
        Line { a: [f64; 2], b: [f64; 2] },
        Arc { center: [f64; 2], radius: f64, start: f64, sweep: f64, ccw: bool },
    }
    impl Segment {
        fn point(self, t: f64) -> [f64; 2] {
            match self {
                Segment::Line { a, b } => [
                    a[0] + t * (b[0] - a[0]),
                    a[1] + t * (b[1] - a[1]),
                ],
                Segment::Arc { center, radius, start, sweep, ccw } => {
                    let angle = start + if ccw { sweep * t } else { -sweep * t };
                    [center[0] + radius * angle.cos(), center[1] + radius * angle.sin()]
                }
            }
        }
        fn portion(self, t0: f64, t1: f64) -> Option<BoundaryEdge> {
            if t1 - t0 <= 1.0e-10 { return None; }
            match self {
                Segment::Line { .. } => {
                    let a = self.point(t0);
                    let b = self.point(t1);
                    ((a[0] - b[0]).hypot(a[1] - b[1]) > 1.0e-10).then_some(
                        BoundaryEdge::Line(LineEdge {
                            start: Vector2::new(a[0], a[1]),
                            end: Vector2::new(b[0], b[1]),
                        })
                    )
                }
                Segment::Arc { center, radius, start, sweep, ccw } => {
                    let actual_start = start + if ccw { sweep * t0 } else { -sweep * t0 };
                    let stored = if ccw { actual_start } else { -actual_start }.rem_euclid(TAU);
                    Some(BoundaryEdge::CircularArc(CircularArcEdge {
                        center: Vector2::new(center[0], center[1]),
                        radius,
                        start_angle: stored,
                        end_angle: stored + sweep * (t1 - t0),
                        counter_clockwise: ccw,
                    }))
                }
            }
        }
    }
    let mut edges = Vec::new();
    for edge in &path.edges {
        match edge {
            BoundaryEdge::Line(line) => edges.push(Segment::Line {
                a: [line.start.x, line.start.y],
                b: [line.end.x, line.end.y],
            }),
            BoundaryEdge::Polyline(poly)
                if poly.is_closed
                    && poly.vertices.len() >= 3
                    && poly.vertices.iter().all(|v| v.z.abs() <= 1.0e-12) =>
            {
                for j in 0..poly.vertices.len() {
                    let a = &poly.vertices[j];
                    let b = &poly.vertices[(j + 1) % poly.vertices.len()];
                    edges.push(Segment::Line {
                        a: [a.x, a.y],
                        b: [b.x, b.y],
                    });
                }
            }
            BoundaryEdge::CircularArc(arc) => {
                let sweep = arc.end_angle - arc.start_angle;
                if !sweep.is_finite() || sweep <= 0.0 || sweep > TAU + EPS {
                    return None;
                }
                edges.push(Segment::Arc {
                    center: [arc.center.x, arc.center.y],
                    radius: arc.radius,
                    start: if arc.counter_clockwise { arc.start_angle } else { -arc.start_angle },
                    sweep,
                    ccw: arc.counter_clockwise,
                });
            }
            _ => return None,
        }
    }
    if edges.len() < 2 || !edges.iter().any(|edge| matches!(edge, Segment::Arc { .. })) {
        return None;
    }
    let n = edges.len();
    for i in 0..n {
        let a = edges[i].point(1.0);
        let b = edges[(i + 1) % n].point(0.0);
        if (a[0] - b[0]).hypot(a[1] - b[1]) > EPS {
            return None; // malformed or unordered DXF path
        }
    }
    let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
    let length = dx.hypot(dy);
    if length <= EPS { return None; }
    let (ux, uy) = (dx / length, dy / length);
    let side = |p: [f64; 2]| ux * (p[1] - p1[1]) - uy * (p[0] - p1[0]);
    #[derive(Clone, Copy)]
    struct Hit { at: f64, point: [f64; 2] }
    let mut hits: Vec<Hit> = Vec::new();
    let mut record = |edge_index: usize, t: f64, edge: Segment| {
        let p = edge.point(t.clamp(0.0, 1.0));
        let proj = (p[0] - p1[0]) * ux + (p[1] - p1[1]) * uy;
        if proj < -EPS || proj > length + EPS { return; }
        if hits.iter().any(|h| (h.point[0] - p[0]).hypot(h.point[1] - p[1]) <= EPS) {
            return;
        }
        let position = (edge_index as f64 + t).rem_euclid(n as f64);
        hits.push(Hit { at: position, point: p });
    };
    for (i, edge) in edges.iter().copied().enumerate() {
        match edge {
            Segment::Line { a, b } => {
                let da = side(a);
                let db = side(b);
                if da.abs() <= EPS && db.abs() <= EPS { return None; }
                if da.abs() <= EPS { record(i, 0.0, edge); }
                if db.abs() <= EPS { record(i, 1.0, edge); }
                if da * db < -EPS * EPS {
                    record(i, da / (da - db), edge);
                }
            }
            Segment::Arc { center, radius, start, sweep, ccw } => {
                let signed = side(center);
                if signed.abs() > radius + EPS { continue; }
                let along = (radius * radius - signed * signed).max(0.0).sqrt();
                for proj in [-along, along] {
                    let p = [center[0] - signed * (-uy) + proj * ux,
                             center[1] - signed * ux + proj * uy];
                    let angle = (p[1] - center[1]).atan2(p[0] - center[0]);
                    let angle_delta = if ccw { (angle - start).rem_euclid(TAU) }
                                      else { (start - angle).rem_euclid(TAU) };
                    if angle_delta <= sweep + EPS {
                        let t = (angle_delta / sweep).clamp(0.0, 1.0);
                        record(i, t, edge);
                    }
                }
            }
        }
    }
    if hits.len() != 2 { return None; }
    hits.sort_by(|a, b| a.at.total_cmp(&b.at));
    let create = |from: Hit, to: Hit| -> Option<BoundaryPath> {
        let end = if to.at <= from.at { to.at + n as f64 } else { to.at };
        let mut position = from.at;
        let mut parts: Vec<BoundaryEdge> = Vec::new();
        // Every surviving edge midpoint must be strictly inside the retained
        // halfplane; this rejects disjoint/concave portions without rewriting
        // the hatch into a self-intersecting polygon.
        while position < end - 1.0e-10 {
            let segment_index = position.floor() as usize;
            let next = ((segment_index + 1) as f64).min(end);
            let edge = edges[segment_index % n];
            let begin_t = position - segment_index as f64;
            let end_t = next - segment_index as f64;
            if keep_sign * side(edge.point((begin_t + end_t) * 0.5)) < -EPS {
                return None;
            }
            if let Some(edge) = edge.portion(begin_t, end_t) {
                parts.push(edge);
            }
            position = next;
            if parts.len() > n + 3 { return None; }
        }
        if parts.is_empty() { return None; }
        parts.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(to.point[0], to.point[1]),
            end: Vector2::new(from.point[0], from.point[1]),
        }));
        let mut result = BoundaryPath::with_flags(BoundaryPathFlags::OUTERMOST);
        result.edges = parts;
        Some(result)
    };
    let first = create(hits[0], hits[1]);
    let second = create(hits[1], hits[0]);
    match (first, second) {
        (Some(path), None) | (None, Some(path)) => Some(path),
        _ => None,
    }
}

/// TRIM a planar hatch as a REGION, not as an individual hatch boundary edge.
///
/// The picked point chooses the half-plane to REMOVE. Only a finite straight
/// cutting edge intersecting the outer ring exactly twice is supported here.
/// Disconnected cuts, curved original edges, and cutters through holes are
/// deliberately refused rather than damaging the hatch topology. An edited
/// hatch is detached from its original associative source handles: these
/// source curves have not changed to match the new boundary.
fn trim_hatch_at(
    hatch: &codec::entities::Hatch,
    geos: &[Geo],
    px: f64,
    py: f64,
) -> Option<codec::entities::Hatch> {
    use codec::entities::{BoundaryEdge, BoundaryPath, BoundaryPathFlags, PolylineEdge};
    use codec::types::Vector2;
    // The working plane of TRIM is XY. Do not quietly clip a tilted OCS
    // boundary using an unrelated XY cutting edge.
    if hatch.normal.x.abs() > 1.0e-9
        || hatch.normal.y.abs() > 1.0e-9
        || (hatch.normal.z - 1.0).abs() > 1.0e-9
        || hatch.paths.is_empty()
    {
        return None;
    }
    // Inner paths (including exact circles) do not need to be clipped when
    // entirely on one side of the cutter. Preserve those analytic curves as
    // they are rather than refusing to trim the hatch again.
    let rings: Vec<Vec<[f64; 2]>> = hatch
        .paths
        .iter()
        .map(crate::scene::hatch_path_ring)
        .collect::<Option<Vec<_>>>()?;
    let depths = kernel::geom2d::ring_nesting_depths(&rings);
    let outer: Vec<usize> = depths
        .iter()
        .enumerate()
        .filter_map(|(index, depth)| (*depth == 0).then_some(index))
        .collect();
    // A single, connected outer ring can safely be clipped by one line.
    if outer.len() != 1 {
        return None;
    }
    let outer_index = outer[0];
    // Only edit outer curves for which we can reconstruct exact HATCH edges.
    // Never silently convert an imported SPLINE/ELLIPSE into a sampled
    // polyline when TRIM is run a second time.
    if !hatch.paths[outer_index].edges.iter().all(|edge| match edge {
        BoundaryEdge::Line(_) | BoundaryEdge::CircularArc(_) => true,
        BoundaryEdge::Polyline(poly) => {
            poly.is_closed
                && poly.vertices.len() >= 3
                && poly.vertices.iter().all(|vertex| vertex.z.abs() <= 1.0e-12)
        }
        _ => false,
    }) {
        return None;
    }
    // Preserve exact outer curves; the mixed-edge solver below handles
    // successive straight cuts even after a curved notch has been made.
    let ring = &rings[outer_index];
    const EPS: f64 = 1.0e-8;
    let ring_area = |points: &[[f64; 2]]| -> f64 {
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>()
            .abs() * 0.5
    };
    let old_area = ring_area(ring);
    if !old_area.is_finite() || old_area <= EPS {
        return None;
    }
    let mut best: Option<(f64, codec::entities::Hatch)> = None;
    // A click inside a circle should cut that disk out of the hatch. Do
    // not let an unrelated nearby straight edge win this pick.
    for circle in geos.iter().filter(|geo| matches!(geo, Geo::Circle { .. })) {
        if let Some(clipped) = trim_hatch_circle(hatch, &rings, outer_index, circle, [px, py]) {
            if let Geo::Circle { cx, cy, r, .. } = *circle {
                let distance = (r - (px - cx).hypot(py - cy)).abs();
                if best.as_ref().is_none_or(|(d, _)| distance < *d) {
                    best = Some((distance, clipped));
                }
            }
        }
    }
    if best.is_some() {
        return best.map(|(_, result)| result);
    }
    for geo in geos {
        let Geo::Line { handle, p1, p2 } = geo else {
            continue;
        };
        if *handle == hatch.common.handle {
            continue;
        }
        let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
        let len2 = dx * dx + dy * dy;
        if !len2.is_finite() || len2 < EPS * EPS {
            continue;
        }
        let len = len2.sqrt();
        let side = |p: [f64; 2]| (dx * (p[1] - p1[1]) - dy * (p[0] - p1[0])) / len;
        let clicked_side = side([px, py]);
        if clicked_side.abs() <= EPS {
            continue;
        }
        let keep_sign = -clicked_side.signum();
        let distances: Vec<_> = ring.iter().copied().map(side).collect();
        // The original ring must cross the cutter at exactly two distinct
        // points, both ON the finite cutting line (no implicit Edge: Extend).
        let mut intersections: Vec<[f64; 2]> = Vec::new();
        for j in 0..ring.len() {
            let next = (j + 1) % ring.len();
            let a = ring[j];
            let b = ring[next];
            let sa = distances[j];
            let sb = distances[next];
            let mut crossing = None;
            if sa.abs() <= EPS {
                crossing = Some(a);
            } else if sa * sb < -EPS * EPS {
                let t = sa / (sa - sb);
                crossing = Some([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
            if let Some(p) = crossing {
                if !intersections
                    .iter()
                    .any(|prev| (prev[0] - p[0]).hypot(prev[1] - p[1]) <= EPS)
                {
                    intersections.push(p);
                }
            }
        }
        if intersections.len() != 2
            || intersections.iter().any(|p| {
                let t = ((p[0] - p1[0]) * dx + (p[1] - p1[1]) * dy) / len2;
                t < -EPS || t > 1.0 + EPS
            })
        {
            continue;
        }
        // Sutherland-Hodgman against the retained half-plane. Exactly two
        // outer intersections ensure this creates one connected closed ring.
        let inside = |p: [f64; 2]| keep_sign * side(p) >= -EPS;
        let mut clipped = Vec::new();
        for j in 0..ring.len() {
            let next = (j + 1) % ring.len();
            let (a, b) = (ring[j], ring[next]);
            let (a_inside, b_inside) = (inside(a), inside(b));
            if a_inside != b_inside {
                let sa = side(a);
                let sb = side(b);
                if (sa - sb).abs() > EPS {
                    let t = sa / (sa - sb);
                    clipped.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
                }
            }
            if b_inside {
                clipped.push(b);
            }
        }
        let mut cleaned: Vec<[f64; 2]> = Vec::new();
        for p in clipped {
            if cleaned.last().is_none_or(|prev| {
                let prev: &[f64; 2] = prev;
                (prev[0] - p[0]).hypot(prev[1] - p[1]) > EPS
            }) {
                cleaned.push(p);
            }
        }
        if cleaned.len() > 1
            && (cleaned[0][0] - cleaned[cleaned.len() - 1][0])
                .hypot(cleaned[0][1] - cleaned[cleaned.len() - 1][1]) <= EPS
        {
            cleaned.pop();
        }
        let new_area = ring_area(&cleaned);
        if cleaned.len() < 3 || !new_area.is_finite()
            || new_area <= EPS || new_area >= old_area - EPS
        {
            continue;
        }
        // On a previously circular-notched hatch, use exact original arc
        // segments rather than tessellating the curved boundary to polylines.
        let curved_outer = hatch.paths[outer_index].edges.iter().any(|edge| {
            matches!(edge, BoundaryEdge::CircularArc(_))
        });
        let analytic_outer = if curved_outer {
            let Some(path) = trim_hatch_curved_outer(
                &hatch.paths[outer_index], *p1, *p2, keep_sign,
            ) else {
                continue;
            };
            Some(path)
        } else {
            None
        };
        let mut new_paths = Vec::with_capacity(hatch.paths.len());
        let mut safe = true;
        for (index, path) in hatch.paths.iter().enumerate() {
            if index == outer_index {
                if let Some(ref path) = analytic_outer {
                    new_paths.push(path.clone());
                } else {
                    let mut replacement = BoundaryPath::with_flags(BoundaryPathFlags::OUTERMOST);
                    replacement.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
                        cleaned
                            .iter()
                            .map(|p| Vector2::new(p[0], p[1]))
                            .collect(),
                        true,
                    )));
                    new_paths.push(replacement);
                }
            } else {
                // A hole entirely retained keeps its exact path; a hole
                // entirely removed disappears. Straddling holes need a
                // topological boolean operation, so do NOT modify the hatch.
                let ring_sides: Vec<f64> = rings[index].iter().copied().map(side).collect();
                let keeps = ring_sides.iter().all(|s| keep_sign * *s > EPS);
                let removes = ring_sides.iter().all(|s| keep_sign * *s < -EPS);
                if !keeps && !removes {
                    safe = false;
                    break;
                }
                if keeps {
                    let mut path = path.clone();
                    path.boundary_handles.clear();
                    path.flags.set_external(false);
                    new_paths.push(path);
                }
            }
        }
        if !safe {
            continue;
        }
        let mut result = hatch.clone();
        result.common.handle = Handle::NULL;
        result.paths = new_paths;
        result.is_associative = false;
        // Clearing associations must apply to every path, not just the outer
        // one, including inner holes inherited from the original hatch.
        for path in &mut result.paths {
            path.boundary_handles.clear();
            path.flags.set_external(false);
        }
        let distance = clicked_side.abs();
        if best.as_ref().is_none_or(|(d, _)| distance < *d) {
            best = Some((distance, result));
        }
    }
    best.map(|(_, result)| result)
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
            if b < 1e-9 {
                return None;
            }
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 {
                t1 += TAU;
            }
            let ts = ellipse_seg_ts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, geos);
            if ts.is_empty() {
                return None;
            }
            // t_click: project mouse onto ellipse local param
            let rx = px - e.center.x;
            let ry = py - e.center.y;
            let xl = rx * nx + ry * ny;
            let yl = -rx * ny + ry * nx;
            let t_ell = ellipse_closest_parameter(a, b, xl, yl);
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
        Some(EntityType::MLine(m)) => {
            super::mledit::trim_mline_at(m, DVec3::new(px, py, 0.0), |a, b| {
                line_seg_ts(a.x, a.y, b.x, b.y, handle, geos)
            })
            .map(|trimmed| vec![EntityType::MLine(trimmed)])
        }
        Some(EntityType::Hatch(hatch)) => trim_hatch_at(hatch, geos, px, py)
            .map(|result| vec![EntityType::Hatch(Box::new(result))]),
        _ => None,
    };
    result
}

/// Quick-mode trim also removes a valid line when it has no boundary
/// intersections. Standard mode continues to treat that pick as a no-op.
fn quick_trim_at(
    all: &[EntityType],
    geos: &[Geo],
    handle: Handle,
    px: f64,
    py: f64,
) -> Option<Vec<EntityType>> {
    if let Some(EntityType::Line(line)) = all.iter().find(|e| e.common().handle == handle) {
        let dx = line.end.x - line.start.x;
        let dy = line.end.y - line.start.y;
        if dx.hypot(dy) > 1e-9
            && line_seg_ts(
                line.start.x,
                line.start.y,
                line.end.x,
                line.end.y,
                handle,
                geos,
            )
            .is_empty()
        {
            return Some(Vec::new());
        }
    }
    pick_trim_at(all, geos, handle, px, py)
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
            let b = a * e.minor_axis_ratio;
            if b < 1e-9 {
                return None;
            }
            let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
            let rx = px - e.center.x;
            let ry = py - e.center.y;
            let xl = rx * nx + ry * ny;
            let yl = -rx * ny + ry * nx;
            let t_ell = ellipse_closest_parameter(a, b, xl, yl);
            let t_click = arc_t(t_ell, t0, t1);
            let _ = span;
            extend_ellipse(e, t_click, geos)
        }
        Some(EntityType::LwPolyline(p)) => extend_lwpoly(p, px, py, geos),
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
        let mut ts = line_seg_ts(
            w[0][0],
            w[0][1],
            w[1][0],
            w[1][1],
            Handle::new(FENCE_PROBE),
            fence_geos,
        );
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for t in ts {
            out.push(lerp2(w[0], w[1], t));
        }
    }
    out
}

/// A representative point where an entity meets or lies inside a crossing
/// window. Checking sampled vertices alone misses a straight segment whose
/// endpoints are both outside the rectangle.
fn entity_point_in_window(e: &EntityType, window: CrossingWindow) -> Option<[f64; 2]> {
    let pts = fence_sample_xy(e);
    if let Some(point) = pts.iter().copied().find(|p| {
        p[0] >= window.min[0]
            && p[0] <= window.max[0]
            && p[1] >= window.min[1]
            && p[1] <= window.max[1]
    }) {
        return Some(point);
    }
    pts.windows(2).find_map(|segment| {
        segment_window_range(segment[0], segment[1], window)
            .map(|(lo, hi)| lerp2(segment[0], segment[1], (lo + hi) * 0.5))
    })
}

/// Find cutting edges touched by a drag rectangle. A left-to-right window
/// keeps only entities fully inside; a right-to-left crossing window also
/// keeps entities that intersect the rectangle boundary.
fn drag_selected_handles(
    entities: &[EntityType],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    crossing: bool,
) -> Vec<Handle> {
    let fence_geos = build_fence_geos(fence);
    let lasso: Vec<Curve> = fence_geos.iter().filter_map(geo_to_curve).collect();
    entities
        .iter()
        .filter(|entity| !entity.common().handle.is_null())
        .filter(|entity| {
            let Some(window) = window else {
                // Lasso: edges crossing it or lying wholly inside it.
                return !fence_cross_points(entity, &fence_geos).is_empty()
                    || fence_sample_xy(entity).first().is_some_and(|point| {
                        kernel::geom2d::contains(
                            &lasso,
                            *point,
                            KernelTolerance::new(CUT_TOLERANCE),
                        )
                    });
            };
            if crossing {
                !fence_cross_points(entity, &fence_geos).is_empty()
                    || entity_point_in_window(entity, window).is_some()
            } else {
                let points = fence_sample_xy(entity);
                !points.is_empty()
                    && points.iter().all(|point| {
                        point[0] >= window.min[0]
                            && point[0] <= window.max[0]
                            && point[1] >= window.min[1]
                            && point[1] <= window.max[1]
                    })
            }
        })
        .map(|entity| entity.common().handle)
        .collect()
}

fn trim_edge_glow(entity: &EntityType, candidate: bool) -> Vec<WireModel> {
    let points = entity_pts(entity);
    if points.len() < 2 {
        return Vec::new();
    }
    let layers: [(f32, f32); 3] = if candidate {
        [(8.0, 0.035), (5.0, 0.065), (2.5, 0.11)]
    } else {
        [(8.0, 0.075), (5.0, 0.15), (2.5, 0.34)]
    };
    layers
        .into_iter()
        .enumerate()
        .map(|(index, (width, alpha))| {
            let mut wire = WireModel::solid(
                format!("trim_edge_glow_{index}"),
                points.clone(),
                [0.22, 0.60, 1.0, alpha],
                false,
            );
            wire.line_weight_px = wire.line_weight_px.max(width);
            wire
        })
        .collect()
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
                    if let Some(p) = entity_point_in_window(&tmp_all[0], window) {
                        cps.push(p);
                    }
                }
            }
            for cp in cps {
                let res = if extend {
                    pick_extend_at(&tmp_all, geos, target, cp[0], cp[1]).map(|x| vec![x])
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
    if changed {
        Some(pieces)
    } else {
        None
    }
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
/// gray, its surviving pieces in cyan — the same convention as the pick hover.
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
                TRIM_REMOVE_GRAY,
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

/// EXTEND's normal box selection applies ONE extension per selected entity.
/// Unlike the multi-round Fence/Crossing operator, this cannot extend an
/// already-updated line again after it reaches its first boundary.
/// The box is only a selector; all targets still use the chosen common
/// boundary-edge geometry (`geos`).
fn extend_box_pass(
    all: &[EntityType],
    geos: &[Geo],
    boundary_handles: &[Handle],
    fence: &[[f64; 2]],
    window: Option<CrossingWindow>,
    crossing: bool,
) -> Vec<(Handle, Vec<EntityType>)> {
    let selected = drag_selected_handles(all, fence, window, crossing);
    if selected.is_empty() {
        return Vec::new();
    }
    let selected: std::collections::HashSet<_> = selected.into_iter().collect();
    let boundaries: std::collections::HashSet<_> = boundary_handles.iter().copied().collect();
    let mut replacements = Vec::new();
    for entity in all {
        let handle = entity.common().handle;
        if !selected.contains(&handle) || boundaries.contains(&handle) {
            continue;
        }
        // Prefer the end visibly touched by the selection rectangle.
        // If both ends are within it, use the nearest reachable boundary;
        // if neither end is within it, follow the box crossing position.
        // This is independent of line direction in the file.
        let extended = if let EntityType::Line(line) = entity {
            let contains = |x: f64, y: f64| window.is_some_and(|w| {
                x >= w.min[0] && x <= w.max[0]
                    && y >= w.min[1] && y <= w.max[1]
            });
            let start_in = contains(line.start.x, line.start.y);
            let end_in = contains(line.end.x, line.end.y);
            if start_in && end_in {
                let start = extend_line(line, 0.0, geos);
                let end = extend_line(line, 1.0, geos);
                match (start, end) {
                    (Some(a), Some(b)) => {
                        let dist = |result: &EntityType| {
                            let EntityType::Line(extended) = result else { return f64::INFINITY; };
                            (extended.start.x - line.start.x).hypot(extended.start.y - line.start.y)
                                + (extended.end.x - line.end.x).hypot(extended.end.y - line.end.y)
                        };
                        Some(if dist(&a) <= dist(&b) { a } else { b })
                    }
                    (a @ Some(_), None) | (None, a @ Some(_)) => a,
                    (None, None) => None,
                }
            } else if start_in {
                extend_line(line, 0.0, geos)
            } else if end_in {
                extend_line(line, 1.0, geos)
            } else {
                let pick = window.and_then(|w| entity_point_in_window(entity, w));
                pick.and_then(|p| pick_extend_at(all, geos, handle, p[0], p[1]))
            }
        } else {
            // Keep the existing analytic extension for arcs, polylines,
            // ellipses and splines instead of introducing another solver.
            window.and_then(|w| entity_point_in_window(entity, w))
                .or_else(|| fence_cross_points(entity, &build_fence_geos(fence)).first().copied())
                .and_then(|p| pick_extend_at(all, geos, handle, p[0], p[1]))
        };
        if let Some(new_entity) = extended {
            replacements.push((handle, vec![new_entity]));
        }
    }
    replacements
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
fn implied_cut_guides(all: &[EntityType], target: Handle, cuts: &[[f64; 2]]) -> Vec<WireModel> {
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
                        let mut w =
                            WireModel::solid(format!("edge_guide_{ci}"), pts, OPT_YELLOW, false);
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

/// A TRIM/EXTEND working copy: the objects, their index and the boundaries
/// cut against.
struct Snapshot<'a> {
    all: &'a mut Vec<EntityType>,
    index: &'a mut ModifyEntityIndex,
    geos: &'a mut Vec<Geo>,
    edge_set: &'a mut Vec<Handle>,
    implied_edges: bool,
}

/// Apply one confirmed replacement to a command's entity snapshot: the old
/// entry goes and its pieces join under the handles the document assigned.
/// Edits stay pending until this callback, so a rejected edit (locked layer)
/// leaves the snapshot untouched. Only the replaced object's entries change;
/// rebuilding every boundary and the index per pick made each click cost the
/// whole drawing (#1978). Returns whether the boundaries need a full rebuild
/// (the last picked cutting edge went, so every object cuts again).
fn apply_pending_replacement(
    snapshot: Snapshot,
    pending: &mut Vec<(Handle, Vec<EntityType>)>,
    old: Handle,
    new_handles: &[Handle],
) -> bool {
    let Some(index) = pending.iter().position(|(handle, _)| *handle == old) else {
        return false;
    };
    let (_, pieces) = pending.swap_remove(index);
    snapshot.index.remove(snapshot.all, old);
    snapshot.geos.retain(|geo| geo.handle() != old);
    let had_edges = !snapshot.edge_set.is_empty();
    snapshot.edge_set.retain(|handle| *handle != old);
    // Pieces never join a picked edge set; with none, every object cuts.
    let all_cut = snapshot.edge_set.is_empty();
    for (mut piece, &handle) in pieces.into_iter().zip(new_handles) {
        piece.as_entity_mut().set_handle(handle);
        if all_cut {
            let mut geos = build_geos(std::slice::from_ref(&piece));
            if snapshot.implied_edges {
                imply_edge_geos(&mut geos);
            }
            snapshot.geos.extend(geos);
        }
        snapshot.index.push(snapshot.all, piece);
    }
    had_edges && all_cut
}

pub struct TrimCommand {
    all_entities: Vec<EntityType>,
    /// Exploded block contents used only as virtual cutting geometry.
    block_boundaries: Vec<(Handle, Vec<EntityType>)>,
    entity_index: ModifyEntityIndex,
    geos: Vec<Geo>,
    /// Replacements sent to the host, applied once their handles are known.
    pending: Vec<(Handle, Vec<EntityType>)>,
    mode: TrimMode,
    /// Whether a no-intersection line pick is a quick full erase.
    quick: bool,
    /// Cutting-edge selection; empty = every object cuts (quick mode).
    edge_set: Vec<Handle>,
    /// Mode to restore if a temporary limit-selection step is cancelled.
    selection_previous_quick: Option<bool>,
    /// Edge option: boundaries extrapolate past their drawn extent.
    implied_edges: bool,
    /// Live Shift state — Shift+click extends instead of trims (#336).
    shift: bool,
}

impl TrimCommand {
    fn choose_mode(&mut self, text: &str) -> Option<CmdResult> {
        let (result, rebuild) = choose_mode(&mut self.mode, &mut self.edge_set, text)?;
        match text.trim().to_uppercase().as_str() {
            "Q" | "QUICK" => {
                self.quick = true;
                self.implied_edges = false;
                self.selection_previous_quick = None;
            }
            "S" | "STANDARD" => self.quick = false,
            _ => {}
        }
        if rebuild {
            self.rebuild_geos();
        }
        Some(result)
    }

    pub fn new(all_entities: Vec<EntityType>) -> Self {
        Self::with_cutting_edges(all_entities, Vec::new())
    }

    pub fn with_cutting_edges(all_entities: Vec<EntityType>, initial_edges: Vec<Handle>) -> Self {
        Self::with_block_boundaries(all_entities, Vec::new(), initial_edges)
    }

    pub fn with_block_boundaries(
        all_entities: Vec<EntityType>,
        block_boundaries: Vec<(Handle, Vec<EntityType>)>,
        initial_edges: Vec<Handle>,
    ) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .into_iter()
            .map(entity_with_lwpolyline_world_xy_owned)
            .collect();
        let block_boundaries = block_boundaries
            .into_iter()
            .map(|(handle, entities)| {
                (
                    handle,
                    entities
                        .into_iter()
                        .map(entity_with_lwpolyline_world_xy_owned)
                        .collect(),
                )
            })
            .collect();

        let mut cmd = Self {
            all_entities,
            block_boundaries,
            entity_index: ModifyEntityIndex::empty(),
            geos: Vec::new(),
            pending: Vec::new(),
            mode: starting_mode(&initial_edges),
            quick: quick_mode(),
            edge_set: initial_edges,
            selection_previous_quick: None,
            implied_edges: false,
            shift: false,
        };

        cmd.rebuild_geos();

        cmd
    }

    /// Boundary geometry from the edge selection (or everything when none),
    /// with the Edge option's implied extrapolation applied on top.
    fn rebuild_geos(&mut self) {
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        let mut geos = if self.edge_set.is_empty() {
            build_geos(&self.all_entities)
        } else {
            // A set, not the Vec: every trim rebuilds this, and a selection
            // of thousands of edges made it edges × objects per click (#1867).
            let edges: std::collections::HashSet<Handle> = self.edge_set.iter().copied().collect();
            let picked: Vec<EntityType> = self
                .all_entities
                .iter()
                .filter(|e| edges.contains(&e.common().handle))
                .cloned()
                .collect();
            build_geos(&picked)
        };
        let selected_edges: Option<std::collections::HashSet<Handle>> = (!self.edge_set.is_empty())
            .then(|| self.edge_set.iter().copied().collect());
        for (insert_handle, entities) in &self.block_boundaries {
            if selected_edges
                .as_ref()
                .is_none_or(|handles| handles.contains(insert_handle))
            {
                geos.extend(build_geos_with_handle(entities, Some(*insert_handle)));
            }
        }
        self.geos = geos;
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
        let Some(target_entity) = self.entity_index.get(&self.all_entities, handle) else {
            return self.geos.clone();
        };
        let Some(target_bounds) = entity_world_aabb_f64(target_entity) else {
            return self.geos.clone();
        };
        self.geos
            .iter()
            .filter(|geo| {
                handles.contains(&geo.handle())
                    || self
                        .block_boundaries
                        .iter()
                        .any(|(owner, _)| *owner == geo.handle()
                            && geo_overlaps_aabb(geo, target_bounds))
            })
            .cloned()
            .collect()
    }

    fn hatch_trim_at(&self, handle: Handle, point: DVec3) -> Option<hatch_region::HatchTrim> {
        let EntityType::Hatch(hatch) = self.entity_index.get(&self.all_entities, handle)? else {
            return None;
        };
        let nearby = (!self.implied_edges)
            .then(|| self.entity_index.nearby_handles(&self.all_entities, handle))
            .flatten();
        let edges: std::collections::HashSet<_> = self.edge_set.iter().copied().collect();
        let cutters = self.all_entities.iter().filter(|entity| {
            let handle = entity.common().handle;
            is_trim_boundary_entity(entity)
                && (edges.is_empty() || edges.contains(&handle))
                && nearby
                    .as_ref()
                    .is_none_or(|handles| handles.contains(&handle))
        });
        hatch_region::trim(hatch, cutters, point, self.implied_edges)
    }

    fn fence_run(&mut self, fence: &[[f64; 2]], window: Option<CrossingWindow>) -> CmdResult {
        let replacements = fence_pass(&self.all_entities, &self.geos, fence, window, self.shift);
        if replacements.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.pending = replacements.clone();
        CmdResult::ReplaceManyContinue(replacements)
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
            TrimMode::ChooseMode => mode_prompt("TRIM"),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.mode {
            TrimMode::Pick => {
                let mut options = vec![
                    CmdOption::new("Cutting edges", "T"),
                    CmdOption::new("Fence", "F"),
                    CmdOption::new("Crossing", "C"),
                ];
                if !self.quick {
                    options.push(CmdOption::new(
                        if self.implied_edges {
                            "Edge: Extend"
                        } else {
                            "Edge: No extend"
                        },
                        "E",
                    ));
                }
                options.extend([
                    CmdOption::new("Erase", "R"),
                    CmdOption::new("mOde", "O"),
                    CmdOption::enter("Done"),
                ]);
                options
            }
            TrimMode::ChooseMode => mode_options(),
            _ => vec![CmdOption::enter("Done")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        // Consumed inputs return Some(NeedPoint) — None would be offered to
        // the command a second time by the driver.
        if let Some(result) = self.choose_mode(text) {
            return Some(result);
        }
        let pick = matches!(self.mode, TrimMode::Pick);
        match text.trim().to_uppercase().as_str() {
            // Options apply at the main prompt only, but stay consumed in
            // every step so "E"/"F"/"C" are never read as hex handles.
            "T" | "CUTTING" | "B" | "BOUNDARY" | "F" | "FENCE" | "C" | "CROSSING" | "E"
            | "EDGE" | "R" | "ERASE"
                if !pick =>
            {
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" if self.quick => Some(CmdResult::ReportError(
                "Invalid option keyword.".to_string(),
            )),
            "T" | "CUTTING" | "B" | "BOUNDARY" => {
                self.selection_previous_quick = Some(self.quick);
                self.quick = false;
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

    fn entity_pick_includes_fills(&self) -> bool {
        matches!(self.mode, TrimMode::Pick)
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
                self.pending = vec![(handle, Vec::new())];
                CmdResult::ReplaceEntity(handle, vec![])
            }
            _ => {
                // A detached, complete circular HATCH is a standalone fill
                // fragment, not a hole in another hatch. TRIM removes it on
                // a click as requested; no cutting line is necessary.
                if !self.shift
                    && self.entity_index.get(&self.all_entities, handle).is_some_and(
                        |entity| matches!(
                            entity,
                            EntityType::Hatch(hatch) if hatch_is_standalone_circle(hatch)
                        ),
                    )
                {
                    self.pending = vec![(handle, Vec::new())];
                    return CmdResult::ReplaceEntity(handle, Vec::new());
                }
                let (px, py) = (pt.x, pt.y);
                let entity = self.entity_index.get(&self.all_entities, handle);
                let new_entities = if self.shift {
                    // Shift+click swaps to Extend for this pick (#336).
                    pick_extend_at(&self.all_entities, &self.geos, handle, px, py).map(|e| vec![e])
                } else if matches!(entity, Some(EntityType::Hatch(_))) {
                    // Treat a HATCH as a filled arrangement of cells. This extends
                    // the legacy half-plane solver to multi-edge partitions and
                    // curved / tilted boundaries while retaining exact paths.
                    self.hatch_trim_at(handle, pt)
                        .map(|trim| vec![EntityType::Hatch(Box::new(trim.kept))])
                } else {
                    let geos = self.nearby_geos(handle);
                    if self.quick {
                        quick_trim_at(&self.all_entities, &geos, handle, px, py)
                    } else {
                        pick_trim_at(&self.all_entities, &geos, handle, px, py)
                    }
                };
                if let Some(new_entities) = new_entities {
                    // The snapshot updates in on_entity_replaced once the
                    // document has accepted the edit and assigned handles.
                    self.pending = vec![(handle, new_entities.clone())];
                    CmdResult::ReplaceEntity(handle, new_entities)
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[codec::Handle]) {
        self.on_entities_replaced(&[(old, new_handles.to_vec())]);
    }

    fn on_entities_replaced(&mut self, replacements: &[(Handle, Vec<codec::Handle>)]) {
        let mut rebuild = false;
        for (old, new_handles) in replacements {
            let snapshot = Snapshot {
                all: &mut self.all_entities,
                index: &mut self.entity_index,
                geos: &mut self.geos,
                edge_set: &mut self.edge_set,
                implied_edges: self.implied_edges,
            };
            rebuild |= apply_pending_replacement(snapshot, &mut self.pending, *old, new_handles);
        }
        if rebuild {
            self.rebuild_geos();
        }
    }

    fn on_drag_selection(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
        crossing: bool,
    ) -> Option<CmdResult> {
        if fence.len() < 2 {
            return None;
        }
        let window = window.map(|(min, max)| CrossingWindow { min, max });
        if matches!(self.mode, TrimMode::SelectEdges) {
            let handles = drag_selected_handles(&self.all_entities, fence, window, crossing);
            if self.shift {
                self.edge_set.retain(|handle| !handles.contains(handle));
            } else {
                for handle in handles {
                    if !self.edge_set.contains(&handle) {
                        self.edge_set.push(handle);
                    }
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        if !matches!(self.mode, TrimMode::Pick) {
            return None;
        }
        Some(self.fence_run(fence, window))
    }

    fn accepts_drag_selection(&self) -> bool {
        matches!(self.mode, TrimMode::Pick | TrimMode::SelectEdges)
    }

    fn persistent_preview_wires(&self) -> Vec<WireModel> {
        if !matches!(self.mode, TrimMode::SelectEdges) {
            return Vec::new();
        }
        self.edge_set
            .iter()
            .filter_map(|handle| self.entity_index.get(&self.all_entities, *handle))
            .flat_map(|entity| trim_edge_glow(entity, false))
            .collect()
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        match self.mode {
            TrimMode::SelectEdges => {
                let mut out: Vec<WireModel> = self
                    .edge_set
                    .iter()
                    .filter_map(|h| self.entity_index.get(&self.all_entities, *h))
                    .flat_map(|entity| trim_edge_glow(entity, false))
                    .collect();
                if !handle.is_null() && !self.edge_set.contains(&handle) {
                    if let Some(e) = self.entity_index.get(&self.all_entities, handle) {
                        out.extend(trim_edge_glow(e, true));
                    }
                }
                return out;
            }
            TrimMode::Erase => {
                // Erase mode: the hovered object previews fully gray.
                if let Some(e) = self
                    .all_entities
                    .iter()
                    .find(|e| e.common().handle == handle)
                {
                    return vec![WireModel::solid(
                        "erase_prev".into(),
                        entity_pts(e),
                        TRIM_REMOVE_GRAY,
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
            if let Some(ext) = pick_extend_at(&self.all_entities, &self.geos, handle, pt.x, pt.y) {
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
        let entity = self.entity_index.get(&self.all_entities, handle);

        let mut hover_wires = match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let ts = line_seg_ts(ax, ay, bx, by, handle, geos);
                if ts.is_empty() {
                    if self.quick && (bx - ax).hypot(by - ay) > 1e-9 {
                        return vec![WireModel::solid(
                            "trim_delete".into(),
                            entity_pts(&EntityType::Line(l.clone())),
                            TRIM_REMOVE_GRAY,
                            false,
                        )];
                    }
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
                let removed_points = trimmed_line_removed_preview(l, &survivors, t_click)
                    .unwrap_or_else(|| entity_pts(&EntityType::Line(l.clone())));
                let removed =
                    WireModel::solid("trim_rm".into(), removed_points, TRIM_REMOVE_GRAY, false);
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
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
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
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
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
                let removed =
                    WireModel::solid("trim_rm".into(), vec![base, far], TRIM_REMOVE_GRAY, false);
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
                let removed =
                    WireModel::solid("trim_rm".into(), vec![neg, pos_pt], TRIM_REMOVE_GRAY, false);
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
                if b < 1e-9 {
                    return vec![];
                }
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 {
                    t1 += TAU;
                }
                let ts = ellipse_seg_ts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, geos);
                if ts.is_empty() {
                    return vec![];
                }
                let rx = pt.x as f64 - e.center.x;
                let ry = pt.y as f64 - e.center.y;
                let xl = rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_ell = ellipse_closest_parameter(a, b, xl, yl);
                let t_click = arc_t(t_ell, t0, t1);
                let survivors = trim_ellipse(e, &ts, t_click);
                let orig_pts =
                    ellipse_pts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, e.center.z);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
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
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
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
            Some(EntityType::Hatch(hatch)) => {
                if hatch_is_standalone_circle(hatch) {
                    return hatch.paths.iter().enumerate().filter_map(|(index, path)| {
                        let mut points = crate::scene::hatch_path_ring(path)?;
                        points.push(*points.first()?);
                        Some(WireModel::solid_f64(
                            format!("trim_hatch_erase_{index}"),
                            points.into_iter().map(|p| [p[0], p[1], hatch.elevation]).collect(),
                            TRIM_REMOVE_GRAY,
                            false,
                        ))
                    }).collect();
                }
                self.hatch_trim_at(handle, pt)
                    .map(|trim| trim.preview_wires())
                    .unwrap_or_default()
            }
            Some(EntityType::LwPolyline(p)) => {
                let Some(survivors) = trim_lwpolyline(p, pt.x as f64, pt.y as f64, geos) else {
                    return vec![];
                };
                let orig = WireModel::solid(
                    "trim_rm".into(),
                    entity_pts(entity.unwrap()),
                    TRIM_REMOVE_GRAY,
                    false,
                );
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
                self.selection_previous_quick = None;
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            TrimMode::Fence(pick) if pick.is_usable() => self.fence_run(&pick.path(), None),
            TrimMode::Fence(_)
            | TrimMode::CrossFirst
            | TrimMode::CrossSecond(_)
            | TrimMode::Erase
            | TrimMode::ChooseMode => CmdResult::NeedPoint,
            TrimMode::Pick => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.mode, TrimMode::Pick) {
            TrimMode::Pick => CmdResult::Cancel,
            TrimMode::SelectEdges => {
                self.edge_set.clear();
                if let Some(previous_quick) = self.selection_previous_quick.take() {
                    self.quick = previous_quick;
                }
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            _ => {
                self.selection_previous_quick = None;
                CmdResult::NeedPoint
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ExtendCommand
// ══════════════════════════════════════════════════════════════════════════

pub struct ExtendCommand {
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    geos: Vec<Geo>,
    /// Replacements sent to the host, applied once their handles are known.
    pending: Vec<(Handle, Vec<EntityType>)>,
    mode: TrimMode,
    /// Whether Shift+pick uses quick TRIM's full-erase rule for plain lines.
    quick: bool,
    /// Boundary-edge selection; empty = every object is a boundary.
    edge_set: Vec<Handle>,
    /// Mode to restore if a temporary limit-selection step is cancelled.
    selection_previous_quick: Option<bool>,
    /// Edge option: boundaries extrapolate past their drawn extent.
    implied_edges: bool,
    /// Live Shift state — Shift+click trims instead of extends (#336).
    shift: bool,
    /// Cached live box preview; rebuilding extensions on every raw pointer
    /// event can stall large drawings. Final commit always recalculates.
    box_preview_wires: Vec<WireModel>,
    box_preview_at: Option<iced::time::Instant>,
}

impl ExtendCommand {
    fn choose_mode(&mut self, text: &str) -> Option<CmdResult> {
        let (result, rebuild) = choose_mode(&mut self.mode, &mut self.edge_set, text)?;
        match text.trim().to_uppercase().as_str() {
            "Q" | "QUICK" => {
                self.quick = true;
                self.implied_edges = false;
                self.selection_previous_quick = None;
            }
            "S" | "STANDARD" => self.quick = false,
            _ => {}
        }
        if rebuild {
            self.rebuild_geos();
        }
        Some(result)
    }

    pub fn new(all_entities: Vec<EntityType>) -> Self {
        Self::with_boundary_edges(all_entities, Vec::new())
    }

    /// Objects selected before the command are its boundary edges, as TRIM
    /// takes them as cutting edges.
    pub fn with_boundary_edges(all_entities: Vec<EntityType>, initial_edges: Vec<Handle>) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .into_iter()
            .map(entity_with_lwpolyline_world_xy_owned)
            .collect();
        let mut cmd = Self {
            all_entities,
            entity_index: ModifyEntityIndex::empty(),
            geos: Vec::new(),
            pending: Vec::new(),
            mode: starting_mode(&initial_edges),
            quick: quick_mode(),
            edge_set: initial_edges,
            selection_previous_quick: None,
            implied_edges: false,
            shift: false,
            box_preview_wires: Vec::new(),
            box_preview_at: None,
        };
        cmd.rebuild_geos();
        cmd
    }

    fn rebuild_geos(&mut self) {
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.geos = if self.edge_set.is_empty() {
            build_geos(&self.all_entities)
        } else {
            // A set, not the Vec: every trim rebuilds this, and a selection
            // of thousands of edges made it edges × objects per click (#1867).
            let edges: std::collections::HashSet<Handle> = self.edge_set.iter().copied().collect();
            let picked: Vec<EntityType> = self
                .all_entities
                .iter()
                .filter(|e| edges.contains(&e.common().handle))
                .cloned()
                .collect();
            build_geos(&picked)
        };
        if self.implied_edges {
            imply_edge_geos(&mut self.geos);
        }
    }

    fn fence_run(&mut self, fence: &[[f64; 2]], window: Option<CrossingWindow>) -> CmdResult {
        // EXTEND's fence extends; Shift held at Enter swaps it to trim.
        let replacements = fence_pass(&self.all_entities, &self.geos, fence, window, !self.shift);
        if replacements.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.pending = replacements.clone();
        CmdResult::ReplaceManyContinue(replacements)
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
            TrimMode::Pick => {
                crate::tf!("EXTEND{edge}  Click near an end or draw a crossing box to extend multiple objects (Shift trims):")
                    .into_owned()
            }
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
            TrimMode::ChooseMode => mode_prompt("EXTEND"),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.mode {
            TrimMode::Pick => {
                let mut options = vec![
                    CmdOption::new("Boundary edges", "B"),
                    CmdOption::new("Fence", "F"),
                    CmdOption::new("Crossing", "C"),
                ];
                if !self.quick {
                    options.push(CmdOption::new(
                        if self.implied_edges {
                            "Edge: Extend"
                        } else {
                            "Edge: No extend"
                        },
                        "E",
                    ));
                }
                options.extend([CmdOption::new("mOde", "O"), CmdOption::enter("Done")]);
                options
            }
            TrimMode::ChooseMode => mode_options(),
            _ => vec![CmdOption::enter("Done")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Some(result) = self.choose_mode(text) {
            return Some(result);
        }
        let pick = matches!(self.mode, TrimMode::Pick);
        match text.trim().to_uppercase().as_str() {
            // Consumed in every step so they are never read as hex handles.
            "B" | "BOUNDARY" | "T" | "CUTTING" | "F" | "FENCE" | "C" | "CROSSING" | "E"
            | "EDGE"
                if !pick =>
            {
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" if self.quick => Some(CmdResult::ReportError(
                "Invalid option keyword.".to_string(),
            )),
            "B" | "BOUNDARY" | "T" | "CUTTING" => {
                self.selection_previous_quick = Some(self.quick);
                self.quick = false;
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
                    if self.quick {
                        quick_trim_at(&self.all_entities, &self.geos, handle, px, py)
                    } else {
                        pick_trim_at(&self.all_entities, &self.geos, handle, px, py)
                    }
                } else {
                    pick_extend_at(&self.all_entities, &self.geos, handle, px, py).map(|e| vec![e])
                };
                if let Some(new_entities) = new_entities {
                    // Same snapshot bookkeeping as TRIM.
                    self.pending = vec![(handle, new_entities.clone())];
                    CmdResult::ReplaceEntity(handle, new_entities)
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[codec::Handle]) {
        self.on_entities_replaced(&[(old, new_handles.to_vec())]);
    }

    fn on_entities_replaced(&mut self, replacements: &[(Handle, Vec<codec::Handle>)]) {
        let mut rebuild = false;
        for (old, new_handles) in replacements {
            let snapshot = Snapshot {
                all: &mut self.all_entities,
                index: &mut self.entity_index,
                geos: &mut self.geos,
                edge_set: &mut self.edge_set,
                implied_edges: self.implied_edges,
            };
            rebuild |= apply_pending_replacement(snapshot, &mut self.pending, *old, new_handles);
        }
        if rebuild {
            self.rebuild_geos();
        }
    }

    fn drag_selection_preview_enabled(&self) -> bool {
        matches!(self.mode, TrimMode::Pick) && !self.shift
    }

    fn on_drag_selection_preview(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
        crossing: bool,
        candidate_handles: &[Handle],
    ) -> Option<Vec<WireModel>> {
        if !matches!(self.mode, TrimMode::Pick) || self.shift || fence.len() < 2 {
            return None;
        }
        // A 20 Hz geometric update keeps the live box responsive without
        // intersecting every visible edge on each raw mouse move.
        if self.box_preview_at.is_some_and(|at| {
            at.elapsed() < std::time::Duration::from_millis(50)
        }) {
            return Some(self.box_preview_wires.clone());
        }
        let window = window.map(|(min, max)| CrossingWindow { min, max });
        // The scene's interaction index has already narrowed this list to
        // the box. A bounded preview is important with survey-size DWGs;
        // the final commit has no such limit and processes all targets.
        let entities: Vec<EntityType> = candidate_handles
            .iter()
            .take(1_000)
            .filter_map(|&handle| self.entity_index.get(&self.all_entities, handle).cloned())
            .collect();
        let replacements = extend_box_pass(
            &entities, &self.geos, &self.edge_set, fence, window, crossing,
        );
        let mut preview = Vec::new();
        for (handle, extended) in replacements {
            let Some(original) = self.entity_index.get(&self.all_entities, handle) else {
                continue;
            };
            for entity in extended {
                preview.extend(extend_hover_wires(
                    original, &entity, &self.all_entities, handle, self.implied_edges,
                ));
            }
        }
        self.box_preview_wires = preview;
        self.box_preview_at = Some(iced::time::Instant::now());
        Some(self.box_preview_wires.clone())
    }

    fn on_drag_selection(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
        crossing: bool,
    ) -> Option<CmdResult> {
        // Never reuse throttled preview geometry for the final edit.
        self.box_preview_wires.clear();
        self.box_preview_at = None;
        if fence.len() < 2 {
            return None;
        }
        let window = window.map(|(min, max)| CrossingWindow { min, max });
        match self.mode {
            TrimMode::SelectEdges => {
                let handles = drag_selected_handles(&self.all_entities, fence, window, crossing);
                if self.shift {
                    self.edge_set.retain(|handle| !handles.contains(handle));
                } else {
                    for handle in handles {
                        if !self.edge_set.contains(&handle) {
                            self.edge_set.push(handle);
                        }
                    }
                }
                Some(CmdResult::NeedPoint)
            }
            TrimMode::Pick => {
                // Shift retains EXTEND's usual inverse TRIM gesture.
                if self.shift {
                    return Some(self.fence_run(fence, window));
                }
                let replacements = extend_box_pass(
                    &self.all_entities,
                    &self.geos,
                    &self.edge_set,
                    fence,
                    window,
                    crossing,
                );
                if replacements.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.pending = replacements.clone();
                Some(CmdResult::ReplaceManyContinue(replacements))
            }
            _ => None,
        }
    }

    fn accepts_drag_selection(&self) -> bool {
        matches!(self.mode, TrimMode::Pick | TrimMode::SelectEdges)
    }

    fn persistent_preview_wires(&self) -> Vec<WireModel> {
        if !matches!(self.mode, TrimMode::SelectEdges) {
            return Vec::new();
        }
        self.edge_set
            .iter()
            .filter_map(|handle| self.entity_index.get(&self.all_entities, *handle))
            .flat_map(|entity| trim_edge_glow(entity, false))
            .collect()
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        match self.mode {
            TrimMode::SelectEdges => {
                let mut out: Vec<WireModel> = self
                    .edge_set
                    .iter()
                    .filter_map(|h| self.entity_index.get(&self.all_entities, *h))
                    .flat_map(|entity| trim_edge_glow(entity, false))
                    .collect();
                if !handle.is_null() && !self.edge_set.contains(&handle) {
                    if let Some(e) = self.entity_index.get(&self.all_entities, handle) {
                        out.extend(trim_edge_glow(e, true));
                    }
                }
                return out;
            }
            TrimMode::Pick => {}
            _ => return vec![],
        }
        if self.shift {
            // Shift held: preview the trim result instead.
            let trim = if self.quick {
                quick_trim_at(&self.all_entities, &self.geos, handle, pt.x, pt.y)
            } else {
                pick_trim_at(&self.all_entities, &self.geos, handle, pt.x, pt.y)
            };
            if let Some(pieces) = trim {
                if pieces.is_empty() {
                    if let Some(entity) = self
                        .all_entities
                        .iter()
                        .find(|e| e.common().handle == handle)
                    {
                        return vec![WireModel::solid(
                            "trim_delete".into(),
                            entity_pts(entity),
                            TRIM_REMOVE_GRAY,
                            false,
                        )];
                    }
                    return vec![];
                }
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

        let entity = self.entity_index.get(&self.all_entities, handle);
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
                    let b = a * e.minor_axis_ratio;
                    if b >= 1e-9 {
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
                        let t_ell = ellipse_closest_parameter(a, b, xl, yl);
                        let t_click = arc_t(t_ell, t0, t1);
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
                self.selection_previous_quick = None;
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            TrimMode::Fence(pick) if pick.is_usable() => self.fence_run(&pick.path(), None),
            TrimMode::Fence(_)
            | TrimMode::CrossFirst
            | TrimMode::CrossSecond(_)
            | TrimMode::Erase
            | TrimMode::ChooseMode => CmdResult::NeedPoint,
            TrimMode::Pick => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.mode, TrimMode::Pick) {
            TrimMode::Pick => CmdResult::Cancel,
            TrimMode::SelectEdges => {
                self.edge_set.clear();
                if let Some(previous_quick) = self.selection_previous_quick.take() {
                    self.quick = previous_quick;
                }
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            _ => {
                self.selection_previous_quick = None;
                CmdResult::NeedPoint
            }
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["EXTEND"] }); // ExtendCommand
inventory::submit!(crate::command::CommandRegistration { names: &["TRIM"] }); // TrimCommand

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn rect_hatch(handle: u64, associative: bool) -> codec::entities::Hatch {
        use codec::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
        use codec::types::Vector2;
        let mut hatch = codec::entities::Hatch::new();
        hatch.common.handle = Handle::new(handle);
        hatch.is_associative = associative;
        hatch.is_solid = true;
        let corners = [
            [0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0],
        ];
        let mut boundary = BoundaryPath::new();
        boundary.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            corners.into_iter().map(|p| Vector2::new(p[0], p[1])).collect(),
            true,
        )));
        if associative {
            boundary.boundary_handles.push(Handle::new(50));
            boundary.flags.set_external(true);
        }
        hatch.paths.push(boundary);
        hatch
    }

    fn add_rect_hole(hatch: &mut codec::entities::Hatch, xmin: f64, ymin: f64, xmax: f64, ymax: f64) {
        use codec::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
        use codec::types::Vector2;
        let points = [[xmin, ymin], [xmax, ymin], [xmax, ymax], [xmin, ymax]];
        let mut hole = BoundaryPath::new();
        hole.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            points.into_iter().map(|p| Vector2::new(p[0], p[1])).collect(), true,
        )));
        hatch.paths.push(hole);
    }

    #[test]
    fn trim_hatch_removes_clicked_side_and_detaches_association() {
        let hatch = rect_hatch(1, true);
        let boundary = xline_at(5.0, -2.0, 5.0, 12.0, 50);
        let all = vec![EntityType::Hatch(Box::new(hatch.clone())), boundary];
        let mut cmd = TrimCommand::new(all);
        assert!(cmd.entity_pick_includes_fills());
        assert!(!cmd.on_hover_entity(Handle::new(1), DVec3::new(2.0, 5.0, 0.0)).is_empty());
        let CmdResult::ReplaceEntity(handle, edited) = cmd.on_entity_pick(
            Handle::new(1), DVec3::new(2.0, 5.0, 0.0),
        ) else {
            panic!("TRIM should clip a picked hatch");
        };
        assert_eq!(handle, Handle::new(1));
        let EntityType::Hatch(trimmed) = &edited[0] else { panic!("HATCH expected"); };
        assert!(!trimmed.is_associative);
        assert!(trimmed.paths.iter().all(|p| p.boundary_handles.is_empty()));
        assert_eq!(trimmed.common.handle, Handle::new(1));
        assert_eq!(trimmed.paths.len(), 1);
        let ring = crate::scene::hatch_path_ring(&trimmed.paths[0]).unwrap();
        assert!(ring.iter().all(|p| p[0] >= 5.0 - 1e-8));
        assert!((crate::entities::hatch::boundary_area(trimmed) - 50.0).abs() < 1e-7);
        assert!(hatch.is_associative, "input is unchanged");
        assert!(!hatch.paths[0].boundary_handles.is_empty());
    }

    #[test]
    fn trim_hatch_preserves_untouched_hole_and_refuses_hole_crossing() {
        let mut hatch = rect_hatch(1, false);
        add_rect_hole(&mut hatch, 6.0, 3.0, 8.0, 7.0);
        let cutter = [Geo::Line {
            handle: Handle::new(50), p1: [5.0, -2.0], p2: [5.0, 12.0],
        }];
        let kept = trim_hatch_at(&hatch, &cutter, 2.0, 5.0).expect("hole retained");
        assert_eq!(kept.paths.len(), 2);
        assert!((crate::entities::hatch::boundary_area(&kept) - 42.0).abs() < 1e-7);
        let mut crossing = rect_hatch(1, false);
        add_rect_hole(&mut crossing, 4.0, 3.0, 6.0, 7.0);
        assert!(trim_hatch_at(&crossing, &cutter, 2.0, 5.0).is_none());
    }

    #[test]
    fn trim_circle_inside_hatch_creates_exact_round_hole() {
        use std::f64::consts::PI;
        let hatch = rect_hatch(1, true);
        let cutter = [Geo::Circle {
            handle: Handle::new(70), cx: 5.0, cy: 5.0, r: 3.0,
        }];
        let result = trim_hatch_at(&hatch, &cutter, 5.0, 5.0)
            .expect("circle inside hatch must make a hole");
        assert_eq!(result.paths.len(), 2);
        assert!(!result.is_associative);
        assert!(result.paths.iter().all(|path| path.boundary_handles.is_empty()));
        assert!(matches!(result.paths[1].edges.first(),
            Some(codec::entities::BoundaryEdge::CircularArc(arc))
                if arc.counter_clockwise && (arc.end_angle - arc.start_angle - std::f64::consts::TAU).abs() < 1e-9));
        assert!((crate::entities::hatch::boundary_area(&result) - (100.0 - PI * 9.0)).abs() < 1e-5);
        let render = crate::scene::Scene::hatch_model_from_dxf(&result, [1.0; 4])
            .expect("round hole must render as a HATCH");
        assert!(!render.fill_mesh().1.is_empty());
    }

    #[test]
    fn trim_circle_intersecting_side_creates_exact_arc_notch() {
        let hatch = rect_hatch(1, false);
        let cutter = [Geo::Circle {
            handle: Handle::new(70), cx: 10.5, cy: 5.0, r: 3.0,
        }];
        let result = trim_hatch_at(&hatch, &cutter, 9.0, 5.0)
            .expect("circle crossing one side must cut a curved notch");
        assert_eq!(result.paths.len(), 1);
        let arcs: Vec<_> = result.paths[0].edges.iter().filter_map(|edge| {
            let codec::entities::BoundaryEdge::CircularArc(arc) = edge else { return None; };
            Some(arc)
        }).collect();
        assert_eq!(arcs.len(), 1);
        assert!((arcs[0].center.x - 10.5).abs() < 1e-9);
        assert!((arcs[0].radius - 3.0).abs() < 1e-9);
        let area = crate::entities::hatch::boundary_area(&result);
        assert!(area > 0.0 && area < 100.0, "clipped area={area}");
        let render = crate::scene::Scene::hatch_model_from_dxf(&result, [1.0; 4])
            .expect("mixed lines and arc must render as a HATCH");
        assert!(!render.fill_mesh().1.is_empty());
    }

    #[test]
    fn trim_command_click_and_hover_accepts_circle_as_cutting_boundary() {
        let mut circle = CircleEnt::from_coords(5.0, 5.0, 0.0, 3.0);
        circle.common.handle = Handle::new(70);
        let all = vec![
            EntityType::Hatch(Box::new(rect_hatch(1, true))),
            EntityType::Circle(circle),
        ];
        let mut cmd = TrimCommand::with_cutting_edges(all, vec![Handle::new(70)]);
        assert!(cmd.entity_pick_includes_fills());
        let preview = cmd.on_hover_entity(Handle::new(1), DVec3::new(5.0, 5.0, 0.0));
        assert!(!preview.is_empty(), "circle cutter must produce hatch hover preview");
        let CmdResult::ReplaceEntity(handle, results) = cmd.on_entity_pick(
            Handle::new(1), DVec3::new(5.0, 5.0, 0.0),
        ) else {
            panic!("circle cutting edge must trim the picked hatch");
        };
        assert_eq!(handle, Handle::new(1));
        let EntityType::Hatch(hatch) = &results[0] else { panic!("hatch expected"); };
        assert_eq!(hatch.paths.len(), 2);
    }

    #[test]
    fn line_trim_preserves_exact_round_hole_after_circle_trim() {
        use std::f64::consts::PI;
        let source = rect_hatch(1, true);
        let circle = [Geo::Circle {
            handle: Handle::new(70), cx: 7.0, cy: 7.0, r: 1.0,
        }];
        let cut_circle = trim_hatch_at(&source, &circle, 7.0, 7.0)
            .expect("first trim must make a round hole");
        assert_eq!(cut_circle.paths.len(), 2);
        let line = [Geo::Line {
            handle: Handle::new(71), p1: [5.0, -1.0], p2: [5.0, 11.0],
        }];
        let cut_line = trim_hatch_at(&cut_circle, &line, 2.0, 5.0)
            .expect("next trim by line must retain round hole");
        assert_eq!(cut_line.paths.len(), 2);
        assert!(matches!(cut_line.paths[1].edges.first(),
            Some(codec::entities::BoundaryEdge::CircularArc(_))));
        assert!((crate::entities::hatch::boundary_area(&cut_line)
            - (50.0 - PI)).abs() < 1.0e-5);
    }

    #[test]
    fn line_trim_after_circular_notch_keeps_exact_analytic_arc() {
        let source = rect_hatch(1, false);
        let circle = [Geo::Circle {
            handle: Handle::new(70), cx: 10.5, cy: 5.0, r: 3.0,
        }];
        let notch = trim_hatch_at(&source, &circle, 9.0, 5.0)
            .expect("first trim must create arc notch");
        let original_area = crate::entities::hatch::boundary_area(&notch);
        let line = [Geo::Line {
            handle: Handle::new(71), p1: [3.0, -1.0], p2: [3.0, 11.0],
        }];
        let trimmed = trim_hatch_at(&notch, &line, 1.0, 5.0)
            .expect("subsequent line must trim the already notched hatch");
        assert_eq!(trimmed.paths.len(), 1);
        assert!(trimmed.paths[0].edges.iter().any(|edge| matches!(
            edge, codec::entities::BoundaryEdge::CircularArc(arc)
                if (arc.center.x - 10.5).abs() < 1.0e-9
                    && (arc.radius - 3.0).abs() < 1.0e-9
        )));
        let area = crate::entities::hatch::boundary_area(&trimmed);
        assert!(area > 0.0 && area < original_area);
        assert!(crate::scene::Scene::hatch_model_from_dxf(&trimmed, [1.0; 4]).is_some());
    }

    #[test]
    fn trim_click_erases_standalone_circular_hatch_not_plain_rectangle() {
        use codec::entities::{BoundaryEdge, BoundaryPath, CircularArcEdge};
        use codec::types::Vector2;
        let mut round = codec::entities::Hatch::new();
        round.common.handle = Handle::new(90);
        round.is_solid = true;
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
            center: Vector2::new(5.0, 5.0), radius: 3.0,
            start_angle: 0.0, end_angle: std::f64::consts::TAU,
            counter_clockwise: true,
        }));
        round.paths.push(path);
        let all = vec![
            EntityType::Hatch(Box::new(round)),
            EntityType::Hatch(Box::new(rect_hatch(91, false))),
        ];
        let mut cmd = TrimCommand::new(all);
        cmd.mode = TrimMode::Pick;
        assert!(!cmd.on_hover_entity(Handle::new(90), DVec3::new(5.0, 5.0, 0.0)).is_empty());
        assert!(matches!(
            cmd.on_entity_pick(Handle::new(90), DVec3::new(5.0, 5.0, 0.0)),
            CmdResult::ReplaceEntity(handle, pieces)
                if handle == Handle::new(90) && pieces.is_empty()
        ));
        assert!(matches!(
            cmd.on_entity_pick(Handle::new(91), DVec3::new(1.0, 1.0, 0.0)),
            CmdResult::NeedPoint
        ), "ordinary polygon hatch without cutting boundary must survive");
    }

    #[test]
    fn trim_circle_never_cuts_holes_or_unrelated_regions() {
        let mut hatch = rect_hatch(1, false);
        add_rect_hole(&mut hatch, 4.0, 4.0, 6.0, 6.0);
        let cutter = [Geo::Circle {
            handle: Handle::new(70), cx: 5.0, cy: 5.0, r: 3.0,
        }];
        assert!(trim_hatch_at(&hatch, &cutter, 5.0, 5.0).is_none());
        let clean = rect_hatch(1, false);
        assert!(trim_hatch_at(&clean, &cutter, 1.0, 1.0).is_none());
        let outside = [Geo::Circle {
            handle: Handle::new(70), cx: 20.0, cy: 20.0, r: 3.0,
        }];
        assert!(trim_hatch_at(&clean, &outside, 5.0, 5.0).is_none());
    }

    #[test]
    fn trim_hatch_rejects_cutter_that_does_not_span_boundary() {
        let hatch = rect_hatch(1, false);
        let too_short = [Geo::Line {
            handle: Handle::new(50), p1: [5.0, 3.0], p2: [5.0, 7.0],
        }];
        assert!(trim_hatch_at(&hatch, &too_short, 2.0, 5.0).is_none());
        let tangent = [Geo::Line {
            handle: Handle::new(50), p1: [10.0, -2.0], p2: [10.0, 12.0],
        }];
        assert!(trim_hatch_at(&hatch, &tangent, 2.0, 5.0).is_none());
    }

    fn xline_at(x1: f64, y1: f64, x2: f64, y2: f64, h: u64) -> EntityType {
        let mut l = LineEnt::new();
        l.start = Vector3::new(x1, y1, 0.0);
        l.end = Vector3::new(x2, y2, 0.0);
        l.common.handle = Handle::new(h);
        EntityType::Line(l)
    }

    fn vertical_boundary(x: f64, y1: f64, y2: f64, handle: u64) -> EntityType {
        xline_at(x, y1, x, y2, handle)
    }

    #[test]
    fn extend_live_box_preview_shows_all_targets_without_modifying_them() {
        let all = vec![
            xline_at(0.0, 0.0, 0.0, 4.0, 1),
            xline_at(2.0, 0.0, 2.0, 5.0, 2),
            xline_at(-2.0, 10.0, 5.0, 10.0, 100),
        ];
        let mut cmd = ExtendCommand::new(all);
        cmd.mode = TrimMode::Pick;
        cmd.edge_set = vec![Handle::new(100)];
        cmd.rebuild_geos();
        let fence = [
            [-1.0, 3.0], [3.0, 3.0], [3.0, 6.0], [-1.0, 6.0], [-1.0, 3.0],
        ];
        let window = Some(([-1.0, 3.0], [3.0, 6.0]));
        let preview = cmd.on_drag_selection_preview(
            &fence, window, true,
            &[Handle::new(1), Handle::new(2), Handle::new(100)],
        ).expect("EXTEND must preview crossing boxes");
        // Same cyan complete-object overlay and dashed added tail as a single
        // hover, once for every valid target (not for the cutting boundary).
        assert_eq!(preview.iter().filter(|w| w.name == "extend_prev").count(), 2);
        assert_eq!(preview.iter().filter(|w| w.name == "extend_tail").count(), 2);
        for wire in preview.iter().filter(|w| w.name == "extend_prev") {
            assert!(wire.points.iter().any(|p| (p[1] - 10.0).abs() < 1e-6));
        }
        assert!(cmd.pending.is_empty(), "preview must not commit geometry");
        assert_eq!(cmd.all_entities.len(), 3);
        // Mouse-up still computes actual edits even if a cached preview exists.
        assert!(matches!(
            cmd.on_drag_selection(&fence, window, true),
            Some(CmdResult::ReplaceManyContinue(replacements)) if replacements.len() == 2
        ));
    }

    #[test]
    fn extend_crossing_box_reaches_one_common_horizontal_boundary() {
        let all = vec![
            xline_at(0.0, 0.0, 0.0, 4.0, 1),
            // Reversed direction: the upper endpoint is START, not END.
            xline_at(2.0, 5.0, 2.0, 1.0, 2),
            // Both ends in the box: only the upward extension is reachable.
            xline_at(4.0, 2.0, 4.0, 3.0, 3),
            xline_at(8.0, 0.0, 8.0, 4.0, 4),
            xline_at(-2.0, 10.0, 10.0, 10.0, 100),
        ];
        let mut cmd = ExtendCommand::new(all);
        cmd.mode = TrimMode::Pick;
        cmd.edge_set = vec![Handle::new(100)];
        cmd.rebuild_geos();
        let fence = [
            [-0.5, 1.5], [4.5, 1.5], [4.5, 6.0], [-0.5, 6.0], [-0.5, 1.5],
        ];
        let CmdResult::ReplaceManyContinue(replacements) = cmd
            .on_drag_selection(&fence, Some(([-0.5, 1.5], [4.5, 6.0])), true)
            .expect("EXTEND accepts crossing-box selection")
        else {
            panic!("the crossing box must extend all selected lines");
        };
        assert_eq!(replacements.len(), 3);
        assert!(replacements.iter().all(|(handle, _)|
            *handle != Handle::new(4) && *handle != Handle::new(100)
        ));
        for (handle, entities) in replacements {
            let EntityType::Line(line) = &entities[0] else {
                panic!("line expected");
            };
            if handle == Handle::new(2) {
                assert!((line.start.y - 10.0).abs() < 1e-9);
                assert!((line.end.y - 1.0).abs() < 1e-9);
            } else {
                assert!((line.end.y - 10.0).abs() < 1e-9);
                assert!((line.start.y - if handle == Handle::new(1) { 0.0 } else { 2.0 }).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn extend_box_does_not_modify_targets_without_reachable_boundary() {
        let lines = vec![
            xline_at(0.0, 0.0, 0.0, 4.0, 1),
            xline_at(2.0, 0.0, 2.0, 4.0, 2),
        ];
        let mut cmd = ExtendCommand::new(lines);
        cmd.mode = TrimMode::Pick;
        let fence = [
            [-1.0, 3.0], [3.0, 3.0], [3.0, 5.0], [-1.0, 5.0], [-1.0, 3.0],
        ];
        assert!(matches!(
            cmd.on_drag_selection(&fence, Some(([-1.0, 3.0], [3.0, 5.0])), true),
            Some(CmdResult::NeedPoint)
        ));
    }

    #[test]
    fn extend_box_window_left_to_right_requires_full_containment() {
        let all = vec![
            xline_at(0.0, 0.0, 0.0, 4.0, 1),
            xline_at(-1.0, 10.0, 1.0, 10.0, 100),
        ];
        let mut cmd = ExtendCommand::new(all);
        cmd.mode = TrimMode::Pick;
        let fence = [
            [-0.5, 3.0], [0.5, 3.0], [0.5, 5.0], [-0.5, 5.0], [-0.5, 3.0],
        ];
        assert!(matches!(
            cmd.on_drag_selection(&fence, Some(([-0.5, 3.0], [0.5, 5.0])), false),
            Some(CmdResult::NeedPoint)
        ));
    }

    #[test]
    fn edge_drag_window_selects_only_fully_contained_objects() {
        let entities = vec![
            xline_at(2.0, 2.0, 4.0, 2.0, 1),
            xline_at(0.0, 3.0, 5.0, 3.0, 2),
            xline_at(6.0, 3.0, 8.0, 3.0, 3),
        ];
        let window = CrossingWindow {
            min: [1.0, 1.0],
            max: [5.0, 4.0],
        };
        let rectangle = [[1.0, 1.0], [5.0, 1.0], [5.0, 4.0], [1.0, 4.0], [1.0, 1.0]];

        assert_eq!(
            drag_selected_handles(&entities, &rectangle, Some(window), false),
            vec![Handle::new(1)]
        );
    }

    #[test]
    fn edge_drag_crossing_selects_inside_and_intersecting_objects() {
        let entities = vec![
            xline_at(2.0, 2.0, 4.0, 2.0, 1),
            xline_at(0.0, 3.0, 5.0, 3.0, 2),
            xline_at(6.0, 3.0, 8.0, 3.0, 3),
        ];
        let window = CrossingWindow {
            min: [1.0, 1.0],
            max: [5.0, 4.0],
        };
        let rectangle = [[1.0, 1.0], [5.0, 1.0], [5.0, 4.0], [1.0, 4.0], [1.0, 1.0]];

        assert_eq!(
            drag_selected_handles(&entities, &rectangle, Some(window), true),
            vec![Handle::new(1), Handle::new(2)]
        );
    }

    #[test]
    fn trim_cutting_edge_selection_drag_adds_and_shift_removes_edges() {
        let entities = vec![
            xline_at(2.0, 2.0, 4.0, 2.0, 1),
            xline_at(2.0, 3.0, 4.0, 3.0, 2),
        ];
        let mut command = TrimCommand::new(entities);
        command.mode = TrimMode::SelectEdges;
        let fence = [[1.0, 1.0], [5.0, 1.0], [5.0, 4.0], [1.0, 4.0], [1.0, 1.0]];
        let window = Some(([1.0, 1.0], [5.0, 4.0]));

        assert!(matches!(
            command.on_drag_selection(&fence, window, false),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.edge_set, vec![Handle::new(1), Handle::new(2)]);
        command.set_shift(true);
        command.on_drag_selection(&fence, window, false);
        assert!(command.edge_set.is_empty());
    }

    #[test]
    fn trim_selected_edge_glow_is_persistent_and_subtle() {
        let mut command = TrimCommand::new(vec![xline_at(0.0, 0.0, 5.0, 0.0, 1)]);
        command.mode = TrimMode::SelectEdges;
        command.edge_set.push(Handle::new(1));

        let initial = command.persistent_preview_wires();
        command.on_hover_entity(Handle::NULL, DVec3::ZERO);
        let after_empty_hover = command.persistent_preview_wires();

        assert_eq!(initial.len(), 3);
        assert_eq!(after_empty_hover.len(), initial.len());
        assert!(after_empty_hover.iter().all(|wire| wire.color[3] <= 0.34));
    }

    #[test]
    fn selected_legacy_polyline_boundary_has_visible_glow() {
        use codec::entities::{Polyline2D, Vertex2D};

        let mut boundary = Polyline2D::new();
        boundary.vertices = vec![
            Vertex2D::new(Vector3::new(2.0, -1.0, 0.0)),
            Vertex2D::new(Vector3::new(2.0, 1.0, 0.0)),
        ];
        boundary.common.handle = Handle::new(2);
        let mut command = TrimCommand::new(vec![EntityType::Polyline2D(boundary)]);
        command.mode = TrimMode::SelectEdges;

        assert!(matches!(
            command.on_entity_pick(Handle::new(2), DVec3::new(2.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let preview = command.persistent_preview_wires();
        assert_eq!(preview.len(), 3);
        assert!(preview.iter().all(|wire| wire.points.len() >= 2));
    }

    #[test]
    fn trim_can_select_a_polyline2d_as_cutting_edge() {
        use codec::entities::{Polyline2D, Vertex2D};

        let target = xline_at(0.0, 0.0, 4.0, 0.0, 1);
        let mut boundary = Polyline2D::new();
        boundary.vertices = vec![
            Vertex2D::new(Vector3::new(2.0, -1.0, 0.0)),
            Vertex2D::new(Vector3::new(2.0, 1.0, 0.0)),
        ];
        boundary.common.handle = Handle::new(2);
        let mut command = TrimCommand::new(vec![target, EntityType::Polyline2D(boundary)]);
        command.quick = false;
        command.mode = TrimMode::SelectEdges;

        command.on_entity_pick(Handle::new(2), DVec3::new(2.0, 0.0, 0.0));
        assert_eq!(command.persistent_preview_wires().len(), 3);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        let result = command.on_entity_pick(Handle::new(1), DVec3::new(3.0, 0.0, 0.0));
        let CmdResult::ReplaceEntity(handle, entities) = result else {
            panic!("expected target line to trim against selected POLYLINE2D");
        };
        assert_eq!(handle, Handle::new(1));
        let EntityType::Line(line) = &entities[0] else {
            panic!("expected a line survivor");
        };
        assert!((line.start.x - 0.0).abs() < 1e-6);
        assert!((line.end.x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn trim_line_removed_preview_marks_only_the_deleted_end_or_gap_in_gray() {
        let line = match xline_at(0.0, 0.0, 10.0, 0.0, 1) {
            EntityType::Line(line) => line,
            _ => unreachable!(),
        };
        let survivors = vec![
            xline_at(0.0, 0.0, 3.0, 0.0, 2),
            xline_at(7.0, 0.0, 10.0, 0.0, 3),
        ];
        let removed = trimmed_line_removed_preview(&line, &survivors, 0.5).unwrap();
        let removed_end =
            trimmed_line_removed_preview(&line, &[xline_at(0.0, 0.0, 6.0, 0.0, 2)], 0.8).unwrap();

        assert_eq!(removed, vec![[3.0, 0.0, 0.0], [7.0, 0.0, 0.0]]);
        assert_eq!(removed_end, vec![[6.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    }

    #[test]
    fn quick_trim_without_intersection_previews_and_erases_line() {
        let all = vec![xline_at(0.0, 0.0, 10.0, 0.0, 1)];
        assert!(pick_trim_at(&all, &build_geos(&all), Handle::new(1), 5.0, 0.0).is_none());
        assert!(
            quick_trim_at(&all, &build_geos(&all), Handle::new(1), 5.0, 0.0)
                .is_some_and(|pieces| pieces.is_empty())
        );

        let mut standard = TrimCommand::new(all.clone());
        standard.quick = false;
        standard.mode = TrimMode::Pick;
        assert!(matches!(
            standard.on_entity_pick(Handle::new(1), glam::DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));

        let mut command = TrimCommand::new(all);
        command.quick = true;
        command.mode = TrimMode::Pick;
        let cursor = glam::DVec3::new(5.0, 0.0, 0.0);
        assert!(!command.on_hover_entity(Handle::new(1), cursor).is_empty());
        match command.on_entity_pick(Handle::new(1), cursor) {
            CmdResult::ReplaceEntity(handle, entities) => {
                assert_eq!(handle, Handle::new(1));
                assert!(entities.is_empty());
            }
            _ => panic!("expected full-line erase"),
        }
        command.on_entity_replaced(Handle::new(1), &[]);
        assert!(command.all_entities.is_empty());
    }

    #[test]
    fn trim_against_block_contents_keeps_only_the_clicked_block_interval() {
        let target = xline_at(0.0, 5.0, 10.0, 5.0, 1);
        let block_handle = Handle::new(90);
        let triangle = vec![
            xline_at(3.0, 3.0, 7.0, 5.0, 101),
            xline_at(7.0, 5.0, 3.0, 7.0, 102),
            xline_at(3.0, 7.0, 3.0, 3.0, 103),
        ];

        for (quick, initial_edges) in [(true, Vec::new()), (false, vec![block_handle])] {
            let mut command = TrimCommand::with_block_boundaries(
                vec![target.clone()],
                vec![(block_handle, triangle.clone())],
                initial_edges,
            );
            command.quick = quick;
            command.mode = TrimMode::Pick;

            let CmdResult::ReplaceEntity(handle, survivors) = command.on_entity_pick(
                Handle::new(1),
                DVec3::new(5.0, 5.0, 0.0),
            ) else {
                panic!("TRIM should cut against the block contents");
            };
            assert_eq!(handle, Handle::new(1));
            assert_eq!(survivors.len(), 2);
            let spans: Vec<_> = survivors
                .iter()
                .map(|entity| match entity {
                    EntityType::Line(line) => (line.start.x, line.end.x),
                    _ => panic!("expected line survivors"),
                })
                .collect();
            assert!((spans[0].0 - 0.0).abs() < 1e-6);
            assert!((spans[0].1 - 3.0).abs() < 1e-6);
            assert!((spans[1].0 - 7.0).abs() < 1e-6);
            assert!((spans[1].1 - 10.0).abs() < 1e-6);
        }
    }

    #[test]
    fn extend_shift_uses_quick_trim_full_erase_for_line_without_boundaries() {
        let all = vec![xline_at(0.0, 0.0, 10.0, 0.0, 1)];
        let mut command = ExtendCommand::new(all);
        command.quick = true;
        command.mode = TrimMode::Pick;
        command.shift = true;
        let cursor = glam::DVec3::new(5.0, 0.0, 0.0);

        assert!(!command.on_hover_entity(Handle::new(1), cursor).is_empty());
        assert!(matches!(
            command.on_entity_pick(Handle::new(1), cursor),
            CmdResult::ReplaceEntity(handle, entities)
                if handle == Handle::new(1) && entities.is_empty()
        ));
    }

    #[test]
    fn implied_edge_option_is_available_only_in_standard_mode() {
        let mut trim = TrimCommand::new(Vec::new());
        trim.mode = TrimMode::Pick;
        trim.quick = true;
        assert!(!trim.options().iter().any(|option| option.keyword == "E"));
        assert!(matches!(
            trim.on_text_input("E"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(!trim.implied_edges);
        trim.quick = false;
        assert!(trim.options().iter().any(|option| option.keyword == "E"));
        assert!(matches!(
            trim.on_text_input("E"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(trim.implied_edges);

        let mut extend = ExtendCommand::new(Vec::new());
        extend.mode = TrimMode::Pick;
        extend.quick = true;
        assert!(!extend.options().iter().any(|option| option.keyword == "E"));
        assert!(matches!(
            extend.on_text_input("E"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(!extend.implied_edges);
        extend.quick = false;
        assert!(extend.options().iter().any(|option| option.keyword == "E"));
        assert!(matches!(
            extend.on_text_input("E"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(extend.implied_edges);

        // Outside the main prompt the keywords are swallowed, never handed
        // back to be read as hex handles.
        trim.mode = TrimMode::SelectEdges;
        extend.mode = TrimMode::SelectEdges;
        for keyword in ["E", "F", "C"] {
            assert!(trim.on_text_input(keyword).is_some());
            assert!(extend.on_text_input(keyword).is_some());
        }
    }

    #[test]
    fn canceling_boundary_selection_restores_quick_mode_and_discards_temporary_edges() {
        let line = xline_at(0.0, 0.0, 10.0, 0.0, 1);
        let mut trim = TrimCommand::new(vec![line.clone()]);
        trim.mode = TrimMode::Pick;
        trim.quick = true;
        assert!(matches!(
            trim.on_text_input("B"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(!trim.quick);
        trim.on_entity_pick(Handle::new(1), glam::DVec3::ZERO);
        assert_eq!(trim.edge_set, vec![Handle::new(1)]);
        assert!(matches!(trim.on_escape(), CmdResult::NeedPoint));
        assert!(trim.quick);
        assert!(trim.edge_set.is_empty());
        assert!(matches!(trim.mode, TrimMode::Pick));

        let mut extend = ExtendCommand::new(vec![line]);
        extend.mode = TrimMode::Pick;
        extend.quick = true;
        assert!(matches!(
            extend.on_text_input("B"),
            Some(CmdResult::NeedPoint)
        ));
        extend.on_entity_pick(Handle::new(1), glam::DVec3::ZERO);
        assert!(matches!(extend.on_escape(), CmdResult::NeedPoint));
        assert!(extend.quick);
        assert!(extend.edge_set.is_empty());
        assert!(matches!(extend.mode, TrimMode::Pick));
    }

    #[test]
    fn fence_edits_commit_as_one_undoable_gesture_and_leave_commands_active() {
        let trim_all = vec![
            xline_at(0.0, 0.0, 10.0, 0.0, 1),
            vertical_boundary(6.0, -2.0, 2.0, 2),
        ];
        let mut trim = TrimCommand::new(trim_all);
        trim.mode = TrimMode::Pick;
        let trim_result = trim.fence_run(&[[8.0, -1.0], [8.0, 1.0]], None);
        assert!(matches!(trim_result, CmdResult::ReplaceManyContinue(_)));
        assert!(matches!(trim.mode, TrimMode::Pick));
        // A rejected edit (locked layer) never calls back: snapshot untouched.
        let trim_result = trim.fence_run(&[[8.0, -1.0], [8.0, 1.0]], None);
        assert!(matches!(trim_result, CmdResult::ReplaceManyContinue(_)));
        assert!(trim.all_entities.iter().all(|e| !e.common().handle.is_null()));
        // An accepted one swaps in the pieces under their document handles.
        trim.on_entities_replaced(&[(Handle::new(1), vec![Handle::new(10)])]);
        let handles: Vec<_> = trim.all_entities.iter().map(|e| e.common().handle).collect();
        assert_eq!(handles, vec![Handle::new(2), Handle::new(10)]);

        let extend_all = vec![
            xline_at(0.0, 0.0, 4.0, 0.0, 1),
            vertical_boundary(8.0, -2.0, 2.0, 2),
        ];
        let mut extend = ExtendCommand::new(extend_all);
        extend.mode = TrimMode::Pick;
        let extend_result = extend.fence_run(&[[3.5, -1.0], [3.5, 1.0]], None);
        assert!(matches!(extend_result, CmdResult::ReplaceManyContinue(_)));
        assert!(matches!(extend.mode, TrimMode::Pick));
        extend.on_entities_replaced(&[(Handle::new(1), vec![Handle::new(10)])]);
        let handles: Vec<_> = extend.all_entities.iter().map(|e| e.common().handle).collect();
        assert_eq!(handles, vec![Handle::new(2), Handle::new(10)]);
    }

    #[test]
    fn extend_polyline_terminal_bulge_follows_arc_and_keeps_circle_center() {
        let mut poly = LwPolyline::new();
        let mut start = LwVertex::from_coords(0.0, 0.0);
        start.bulge = 0.5;
        poly.vertices = vec![start, LwVertex::from_coords(2.0, 0.0)];
        poly.common.handle = Handle::new(1);
        let boundary = xline_at(1.5, 0.75, 3.0, 0.75, 2);
        let all = vec![EntityType::LwPolyline(poly.clone()), boundary];

        let result = pick_extend_at(&all, &build_geos(&all), Handle::new(1), 1.9, 0.05)
            .expect("the terminal arc meets the boundary ahead of its endpoint");
        let EntityType::LwPolyline(extended) = result else {
            panic!("expected an extended lightweight polyline");
        };
        let old_arc = BulgeArc::from_bulge([0.0, 0.0], [2.0, 0.0], 0.5).unwrap();
        let new_end = extended.vertices.last().unwrap();
        let distance_to_center =
            (new_end.location.x - old_arc.center[0]).hypot(new_end.location.y - old_arc.center[1]);
        assert!((distance_to_center - old_arc.radius).abs() < 1e-6);
        assert!((new_end.location.x - 2.25).abs() < 1e-6);
        assert!((new_end.location.y - 0.75).abs() < 1e-6);
        assert!(extended.vertices[0].bulge > 0.5);
        assert!(extended.vertices[0].bulge.is_finite());
    }

    #[test]
    fn trim_circle_with_two_boundaries_is_stable_under_entity_order() {
        let mut circle = CircleEnt::new();
        circle.center = Vector3::new(0.0, 0.0, 0.0);
        circle.radius = 10.0;
        circle.common.handle = Handle::new(1);
        let left = vertical_boundary(-5.0, -12.0, 12.0, 2);
        let right = vertical_boundary(5.0, -12.0, 12.0, 3);
        let variants = [
            vec![
                EntityType::Circle(circle.clone()),
                left.clone(),
                right.clone(),
            ],
            vec![right, EntityType::Circle(circle.clone()), left],
        ];
        let mut canonical: Option<Vec<(f64, f64)>> = None;
        for all in variants {
            let result = pick_trim_at(&all, &build_geos(&all), Handle::new(1), 0.0, 10.0)
                .expect("circle has two cutting boundaries");
            let mut arcs: Vec<_> = result
                .iter()
                .map(|entity| match entity {
                    EntityType::Arc(arc) => (arc.start_angle, arc.end_angle),
                    other => panic!("expected circle trim arcs, got {other:?}"),
                })
                .collect();
            arcs.sort_by(|a, b| a.0.total_cmp(&b.0));
            if let Some(expected) = &canonical {
                assert_eq!(arcs.len(), expected.len());
                for (actual, expected) in arcs.iter().zip(expected) {
                    assert!((actual.0 - expected.0).abs() < 1e-7);
                    assert!((actual.1 - expected.1).abs() < 1e-7);
                }
            } else {
                canonical = Some(arcs);
            }
        }
    }

    #[test]
    fn polyline2d_bulge_segment_is_used_as_a_curved_trim_boundary() {
        use codec::entities::{Polyline2D, Vertex2D};

        let target = xline_at(-2.0, 0.0, 4.0, 0.0, 1);
        let mut boundary = Polyline2D::new();
        let mut first = Vertex2D::new(Vector3::new(0.0, -2.0, 0.0));
        first.bulge = 1.0;
        boundary.vertices = vec![first, Vertex2D::new(Vector3::new(0.0, 2.0, 0.0))];
        boundary.common.handle = Handle::new(2);
        let all = vec![target, EntityType::Polyline2D(boundary)];

        let result = pick_trim_at(&all, &build_geos(&all), Handle::new(1), 3.0, 0.0)
            .expect("the bulged POLYLINE2D boundary crosses the target line twice");
        assert_eq!(result.len(), 1);
        let EntityType::Line(line) = &result[0] else {
            panic!("expected a line survivor");
        };
        // The half-circle bulge reaches x=2, so the clicked right interval is
        // removed and the left interval survives.
        assert!((line.start.x + 2.0).abs() < 1e-6);
        assert!((line.end.x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn extend_line_reaches_polyline2d_boundary() {
        use codec::entities::{Polyline2D, Vertex2D};

        let target = xline_at(0.0, 0.0, 1.0, 0.0, 1);
        let mut boundary = Polyline2D::new();
        boundary.vertices = vec![
            Vertex2D::new(Vector3::new(3.0, -1.0, 0.0)),
            Vertex2D::new(Vector3::new(3.0, 1.0, 0.0)),
        ];
        boundary.common.handle = Handle::new(2);
        let all = vec![target, EntityType::Polyline2D(boundary)];
        let result = pick_extend_at(&all, &build_geos(&all), Handle::new(1), 0.9, 0.0)
            .expect("the line should extend to the POLYLINE2D boundary");
        let EntityType::Line(line) = result else {
            panic!("expected an extended line");
        };
        assert!((line.end.x - 3.0).abs() < 1e-6);
    }

    #[test]
    fn trim_can_select_a_lightweight_polyline_as_cutting_edge() {
        let target = xline_at(0.0, 0.0, 4.0, 0.0, 1);
        let mut boundary = LwPolyline::new();
        boundary.common.handle = Handle::new(2);
        boundary.vertices = vec![
            LwVertex::from_coords(2.0, -1.0),
            LwVertex::from_coords(2.0, 1.0),
        ];
        let mut command = TrimCommand::with_cutting_edges(
            vec![target, EntityType::LwPolyline(boundary)],
            Vec::new(),
        );
        command.quick = false;
        command.mode = TrimMode::SelectEdges;

        assert!(matches!(
            command.on_entity_pick(Handle::new(2), DVec3::new(2.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert_eq!(command.edge_set, vec![Handle::new(2)]);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        let result = command.on_entity_pick(Handle::new(1), DVec3::new(3.0, 0.0, 0.0));
        let CmdResult::ReplaceEntity(handle, entities) = result else {
            panic!("expected target line to trim against selected polyline");
        };
        assert_eq!(handle, Handle::new(1));
        assert_eq!(entities.len(), 1);
        let EntityType::Line(line) = &entities[0] else {
            panic!("expected a line survivor");
        };
        assert!((line.start.x - 0.0).abs() < 1e-6);
        assert!((line.end.x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn trim_uses_projected_fallback_for_imported_lwpolyline_with_tilted_ocs() {
        let target = xline_at(-5.0, 0.0, 5.0, 0.0, 1);
        let mut boundary = LwPolyline::new();
        boundary.common.handle = Handle::new(2);
        boundary.normal = Vector3::new(0.5, 0.0, 3.0_f64.sqrt() / 2.0);
        boundary.is_closed = true;
        boundary.vertices = vec![
            LwVertex::from_coords(-1.0, -1.0),
            LwVertex::from_coords(1.0, -1.0),
            LwVertex::from_coords(1.0, 1.0),
            LwVertex::from_coords(-1.0, 1.0),
        ];
        let boundary = EntityType::LwPolyline(boundary);

        // This is the imported OCS case: the existing explode helper cannot
        // materialize world-XY segments, but the shared curve projection can.
        assert!(crate::modules::draw::modify::explode::explode_polyline_segments(&boundary).is_empty());
        let mut command = TrimCommand::new(vec![target, boundary]);
        command.quick = false;
        command.mode = TrimMode::SelectEdges;
        command.on_entity_pick(Handle::new(2), DVec3::ZERO);
        assert_eq!(command.persistent_preview_wires().len(), 3);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));

        let result = command.on_entity_pick(Handle::new(1), DVec3::ZERO);
        let CmdResult::ReplaceEntity(handle, pieces) = result else {
            panic!("expected the imported polyline boundary to trim the target");
        };
        assert_eq!(handle, Handle::new(1));
        assert_eq!(pieces.len(), 2);
    }

    #[test]
    fn extend_line_can_reach_a_lightweight_polyline_boundary() {
        let target = xline_at(0.0, 0.0, 1.0, 0.0, 1);
        let mut boundary = LwPolyline::new();
        boundary.common.handle = Handle::new(2);
        boundary.vertices = vec![
            LwVertex::from_coords(3.0, -1.0),
            LwVertex::from_coords(3.0, 1.0),
        ];
        let all = vec![target, EntityType::LwPolyline(boundary)];
        let result = pick_extend_at(&all, &build_geos(&all), Handle::new(1), 0.9, 0.0)
            .expect("the line should extend to the polyline boundary");
        let EntityType::Line(line) = result else {
            panic!("expected an extended line");
        };
        assert!((line.start.x - 0.0).abs() < 1e-6);
        assert!((line.end.x - 3.0).abs() < 1e-6);
    }

    #[test]
    fn fence_handles_repeated_crossings_and_multiple_targets_independent_of_order() {
        let targets = [
            xline_at(0.0, 0.0, 10.0, 0.0, 1),
            xline_at(0.0, 4.0, 10.0, 4.0, 4),
        ];
        let boundaries = [
            vertical_boundary(3.0, -2.0, 6.0, 2),
            vertical_boundary(7.0, -2.0, 6.0, 3),
        ];
        let fence = [[2.0, -1.0], [2.0, 5.0], [5.0, -1.0], [5.0, 5.0]];
        let mut expected: Option<Vec<(Handle, Vec<([f64; 2], [f64; 2])>)>> = None;
        for all in [
            vec![
                targets[0].clone(),
                targets[1].clone(),
                boundaries[0].clone(),
                boundaries[1].clone(),
            ],
            vec![
                boundaries[1].clone(),
                targets[1].clone(),
                boundaries[0].clone(),
                targets[0].clone(),
            ],
        ] {
            let replacements = fence_pass(&all, &build_geos(&all), &fence, None, false);
            let mut actual: Vec<_> = replacements
                .into_iter()
                .map(|(handle, pieces)| {
                    let mut segments: Vec<_> = pieces
                        .iter()
                        .map(|piece| match piece {
                            EntityType::Line(line) => {
                                ([line.start.x, line.start.y], [line.end.x, line.end.y])
                            }
                            other => panic!("expected line piece, got {other:?}"),
                        })
                        .collect();
                    segments
                        .sort_by(|a, b| a.0[1].total_cmp(&b.0[1]).then(a.0[0].total_cmp(&b.0[0])));
                    (handle, segments)
                })
                .collect();
            actual.sort_by_key(|(handle, _)| format!("{handle:?}"));
            assert!(actual.iter().any(|(handle, _)| *handle == Handle::new(1)));
            assert!(actual.iter().any(|(handle, _)| *handle == Handle::new(4)));
            assert!(actual.iter().all(|(_, pieces)| !pieces.is_empty()));
            if let Some(expected) = &expected {
                assert_eq!(actual.len(), expected.len());
                for ((ha, pa), (he, pe)) in actual.iter().zip(expected) {
                    assert_eq!(ha, he);
                    assert_eq!(pa.len(), pe.len());
                    for (a, e) in pa.iter().zip(pe) {
                        for axis in 0..2 {
                            assert!((a.0[axis] - e.0[axis]).abs() < 1e-6);
                            assert!((a.1[axis] - e.1[axis]).abs() < 1e-6);
                        }
                    }
                }
            } else {
                expected = Some(actual);
            }
        }
    }

    #[test]
    fn crossing_window_trims_objects_inside_and_crossing_it() {
        let all = vec![
            xline_at(2.0, 2.0, 8.0, 2.0, 1),
            xline_at(-2.0, 5.0, 12.0, 5.0, 2),
            vertical_boundary(5.0, -1.0, 11.0, 3),
        ];
        let window = CrossingWindow {
            min: [1.0, 1.0],
            max: [9.0, 9.0],
        };
        let replacements = fence_pass(&all, &build_geos(&all), &[], Some(window), false);
        assert!(replacements
            .iter()
            .any(|(handle, _)| *handle == Handle::new(1)));
        assert!(replacements
            .iter()
            .any(|(handle, _)| *handle == Handle::new(2)));
    }

    #[test]
    fn drag_lasso_applies_to_multiple_entities_in_any_entity_order() {
        let target_a = xline_at(0.0, 0.0, 10.0, 0.0, 1);
        let target_b = xline_at(0.0, 4.0, 10.0, 4.0, 2);
        let boundary_a = vertical_boundary(5.0, -2.0, 6.0, 3);
        let boundary_b = vertical_boundary(6.0, -2.0, 6.0, 4);
        let mut canonical: Option<Vec<(Handle, Vec<([f64; 2], [f64; 2])>)>> = None;
        for all in [
            vec![
                target_a.clone(),
                target_b.clone(),
                boundary_a.clone(),
                boundary_b.clone(),
            ],
            vec![
                boundary_b.clone(),
                target_b.clone(),
                boundary_a.clone(),
                target_a.clone(),
            ],
        ] {
            let mut command = TrimCommand::new(all);
            command.mode = TrimMode::Pick;
            let fence = [[4.0, -1.0], [4.0, 5.0], [7.0, 5.0]];
            let result = command.on_drag_selection(&fence, None, false);
            let Some(CmdResult::ReplaceManyContinue(replacements)) = result else {
                panic!("expected a multi-entity lasso trim");
            };
            assert!(replacements
                .iter()
                .any(|(handle, _)| *handle == Handle::new(1)));
            assert!(replacements
                .iter()
                .any(|(handle, _)| *handle == Handle::new(2)));
            let mut actual: Vec<_> = replacements
                .into_iter()
                .map(|(handle, pieces)| {
                    let mut segments: Vec<_> = pieces
                        .iter()
                        .map(|piece| match piece {
                            EntityType::Line(line) => {
                                ([line.start.x, line.start.y], [line.end.x, line.end.y])
                            }
                            other => panic!("expected lasso line survivor, got {other:?}"),
                        })
                        .collect();
                    segments.sort_by(|a, b| a.0[1].total_cmp(&b.0[1]));
                    (handle, segments)
                })
                .collect();
            actual.sort_by_key(|(handle, _)| format!("{handle:?}"));
            assert!(actual.iter().all(|(_, pieces)| !pieces.is_empty()));
            if let Some(expected) = &canonical {
                assert_eq!(actual, *expected, "lasso output must ignore entity order");
            } else {
                canonical = Some(actual);
            }
        }
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
                assert!(
                    (l.end.x - 5.0).abs() < 1e-6 && l.end.y.abs() < 1e-6,
                    "extends to the implied boundary, got ({}, {})",
                    l.end.x,
                    l.end.y
                );
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
        assert_eq!(
            repl.len(),
            1,
            "exactly the crossed line is trimmed: {repl:?}"
        );
        assert_eq!(repl[0].0, Handle::new(1));
        assert_eq!(repl[0].1.len(), 1);
        match &repl[0].1[0] {
            EntityType::Line(l) => {
                assert!(
                    (l.end.x - 5.0).abs() < 1e-6 && (l.end.y - 5.0).abs() < 1e-6,
                    "arm cut back to the intersection, got end ({}, {})",
                    l.end.x,
                    l.end.y
                );
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

    #[test]
    fn trim_polyline_arc_segment_preserves_bulge() {
        use std::f64::consts::FRAC_PI_8;
        // Arc from (0, 0) to (2, 0) with bulge 1.0 (semicircle sweeping CCW under the chord)
        let mut poly = LwPolyline::new();
        poly.common.handle = Handle::new(10);
        let mut v0 = LwVertex::from_coords(0.0, 0.0);
        v0.bulge = 1.0;
        let v1 = LwVertex::from_coords(2.0, 0.0);
        poly.vertices = vec![v0, v1];

        // Vertical cutter line at x = 1.0 crossing the arc at (1.0, -1.0)
        let mut cutter = LineEnt::new();
        cutter.start = Vector3::new(1.0, -2.0, 0.0);
        cutter.end = Vector3::new(1.0, 2.0, 0.0);
        cutter.common.handle = Handle::new(20);

        let all = vec![EntityType::LwPolyline(poly), EntityType::Line(cutter)];
        let geos = build_geos(&all);

        // Click first half near (0.5, -0.8) -> first half removed, second half kept from (1.0, -1.0) to (2.0, 0.0)
        let res = pick_trim_at(&all, &geos, Handle::new(10), 0.5, -0.8)
            .expect("should trim polyline arc");
        assert_eq!(res.len(), 1);
        match &res[0] {
            EntityType::LwPolyline(p) => {
                assert_eq!(p.vertices.len(), 2);
                let p0 = &p.vertices[0];
                let p1 = &p.vertices[1];
                assert!(
                    (p0.location.x - 1.0).abs() < 1e-5,
                    "expected x=1.0, got {}",
                    p0.location.x
                );
                assert!(
                    (p0.location.y + 1.0).abs() < 1e-5,
                    "expected y=-1.0, got {}",
                    p0.location.y
                );
                assert!(
                    (p1.location.x - 2.0).abs() < 1e-5,
                    "expected x=2.0, got {}",
                    p1.location.x
                );
                assert!(
                    p1.location.y.abs() < 1e-5,
                    "expected y=0.0, got {}",
                    p1.location.y
                );
                // The surviving quarter circle should have bulge = tan(pi / 8)
                let expected_bulge = FRAC_PI_8.tan();
                assert!(
                    (p0.bulge - expected_bulge).abs() < 1e-4,
                    "surviving arc should retain quarter-circle bulge {}, got {}",
                    expected_bulge,
                    p0.bulge
                );
                assert_eq!(p1.bulge, 0.0);
            }
            other => panic!("expected LwPolyline, got {other:?}"),
        }

        // Click second half near (1.5, -0.8) -> second half removed, first half kept from (0.0, 0.0) to (1.0, -1.0)
        let res2 = pick_trim_at(&all, &geos, Handle::new(10), 1.5, -0.8)
            .expect("should trim polyline arc");
        assert_eq!(res2.len(), 1);
        match &res2[0] {
            EntityType::LwPolyline(p) => {
                assert_eq!(p.vertices.len(), 2);
                let p0 = &p.vertices[0];
                let p1 = &p.vertices[1];
                assert!(
                    p0.location.x.abs() < 1e-5,
                    "expected x=0.0, got {}",
                    p0.location.x
                );
                assert!(
                    p0.location.y.abs() < 1e-5,
                    "expected y=0.0, got {}",
                    p0.location.y
                );
                assert!(
                    (p1.location.x - 1.0).abs() < 1e-5,
                    "expected x=1.0, got {}",
                    p1.location.x
                );
                assert!(
                    (p1.location.y + 1.0).abs() < 1e-5,
                    "expected y=-1.0, got {}",
                    p1.location.y
                );
                let expected_bulge = FRAC_PI_8.tan();
                assert!(
                    (p0.bulge - expected_bulge).abs() < 1e-4,
                    "surviving arc should retain quarter-circle bulge {}, got {}",
                    expected_bulge,
                    p0.bulge
                );
                assert_eq!(p1.bulge, 0.0);
            }
            other => panic!("expected LwPolyline, got {other:?}"),
        }
    }

    #[test]
    fn trim_multi_segment_polyline_with_arc() {
        use std::f64::consts::FRAC_PI_8;
        // Segment 0: straight line from (-2, 0) to (0, 0)
        // Segment 1: arc from (0, 0) to (2, 0) with bulge 1.0
        let mut poly = LwPolyline::new();
        poly.common.handle = Handle::new(30);
        let v0 = LwVertex::from_coords(-2.0, 0.0);
        let mut v1 = LwVertex::from_coords(0.0, 0.0);
        v1.bulge = 1.0;
        let v2 = LwVertex::from_coords(2.0, 0.0);
        poly.vertices = vec![v0, v1, v2];

        // Vertical cutter at x = 1.0
        let mut cutter = LineEnt::new();
        cutter.start = Vector3::new(1.0, -2.0, 0.0);
        cutter.end = Vector3::new(1.0, 2.0, 0.0);
        cutter.common.handle = Handle::new(40);

        let all = vec![EntityType::LwPolyline(poly), EntityType::Line(cutter)];
        let geos = build_geos(&all);

        // Click the far end of the arc near (1.5, -0.8) -> removes end, keeps straight seg + first half of arc
        let res = pick_trim_at(&all, &geos, Handle::new(30), 1.5, -0.8).expect("should trim");
        assert_eq!(res.len(), 1);
        match &res[0] {
            EntityType::LwPolyline(p) => {
                assert_eq!(p.vertices.len(), 3);
                // Vertex 0: (-2, 0), straight
                assert!((p.vertices[0].location.x + 2.0).abs() < 1e-5);
                assert!(p.vertices[0].bulge.abs() < 1e-5);
                // Vertex 1: (0, 0), arc with quarter-turn bulge
                assert!(p.vertices[1].location.x.abs() < 1e-5);
                let expected_bulge = FRAC_PI_8.tan();
                assert!((p.vertices[1].bulge - expected_bulge).abs() < 1e-4);
                // Vertex 2: (1, -1), end
                assert!((p.vertices[2].location.x - 1.0).abs() < 1e-5);
                assert!((p.vertices[2].location.y + 1.0).abs() < 1e-5);
            }
            other => panic!("expected LwPolyline, got {other:?}"),
        }
    }

    #[test]
    fn trim_negative_bulge_polyline_arc() {
        use std::f64::consts::FRAC_PI_8;
        // Negative bulge means CW arc: from (0, 0) to (2, 0) arches above the chord (y > 0)
        let mut poly = LwPolyline::new();
        poly.common.handle = Handle::new(50);
        let mut v0 = LwVertex::from_coords(0.0, 0.0);
        v0.bulge = -1.0;
        let v1 = LwVertex::from_coords(2.0, 0.0);
        poly.vertices = vec![v0, v1];

        // Vertical cutter at x = 1.0 crossing at (1.0, 1.0)
        let mut cutter = LineEnt::new();
        cutter.start = Vector3::new(1.0, -2.0, 0.0);
        cutter.end = Vector3::new(1.0, 2.0, 0.0);
        cutter.common.handle = Handle::new(60);

        let all = vec![EntityType::LwPolyline(poly), EntityType::Line(cutter)];
        let geos = build_geos(&all);

        // Click first half near (0.5, 0.8) -> removes first half, keeps second half from (1.0, 1.0) to (2.0, 0.0)
        let res = pick_trim_at(&all, &geos, Handle::new(50), 0.5, 0.8).expect("should trim CW arc");
        assert_eq!(res.len(), 1);
        match &res[0] {
            EntityType::LwPolyline(p) => {
                assert_eq!(p.vertices.len(), 2);
                let p0 = &p.vertices[0];
                let p1 = &p.vertices[1];
                assert!((p0.location.x - 1.0).abs() < 1e-5);
                assert!((p0.location.y - 1.0).abs() < 1e-5);
                assert!((p1.location.x - 2.0).abs() < 1e-5);
                assert!(p1.location.y.abs() < 1e-5);
                let expected_bulge = -FRAC_PI_8.tan();
                assert!(
                    (p0.bulge - expected_bulge).abs() < 1e-4,
                    "expected negative bulge {}, got {}",
                    expected_bulge,
                    p0.bulge
                );
            }
            other => panic!("expected LwPolyline, got {other:?}"),
        }
    }

    #[test]
    fn trim_closed_polyline_with_arc() {
        use std::f64::consts::FRAC_PI_8;
        // Closed polyline:
        // Vertex 0: (0, 0), bulge 1.0 (arc to (2, 0) below chord, peak at y=-1)
        // Vertex 1: (2, 0), bulge 0.0 (straight to (2, 2))
        // Vertex 2: (2, 2), bulge 0.0 (straight to (0, 2))
        // Vertex 3: (0, 2), bulge 0.0 (straight back to (0, 0))
        let mut poly = LwPolyline::new();
        poly.common.handle = Handle::new(70);
        poly.is_closed = true;
        let mut v0 = LwVertex::from_coords(0.0, 0.0);
        v0.bulge = 1.0;
        let v1 = LwVertex::from_coords(2.0, 0.0);
        let v2 = LwVertex::from_coords(2.0, 2.0);
        let v3 = LwVertex::from_coords(0.0, 2.0);
        poly.vertices = vec![v0, v1, v2, v3];

        // Cutter 1: x = 1.0 cuts the arc at (1.0, -1.0)
        let mut c1 = LineEnt::new();
        c1.start = Vector3::new(1.0, -3.0, 0.0);
        c1.end = Vector3::new(1.0, -0.5, 0.0);
        c1.common.handle = Handle::new(80);

        // Cutter 2: y = 1.0 cuts vertical edges at x=0 and x=2
        let mut c2 = LineEnt::new();
        c2.start = Vector3::new(-1.0, 1.0, 0.0);
        c2.end = Vector3::new(3.0, 1.0, 0.0);
        c2.common.handle = Handle::new(81);

        let all = vec![
            EntityType::LwPolyline(poly),
            EntityType::Line(c1),
            EntityType::Line(c2),
        ];
        let geos = build_geos(&all);

        // Click first half of arc near (0.5, -0.8):
        // Nearest cuts are at (1.0, -1.0) on seg 0 and at (0.0, 1.0) on seg 3.
        // Trim removes that corner/arc portion and leaves an open polyline
        let res = pick_trim_at(&all, &geos, Handle::new(70), 0.5, -0.8)
            .expect("should trim closed polyline");
        assert_eq!(res.len(), 1);
        match &res[0] {
            EntityType::LwPolyline(p) => {
                assert!(!p.is_closed);
                // Starts at (1.0, -1.0) with remaining quarter-circle arc
                let start_v = &p.vertices[0];
                assert!((start_v.location.x - 1.0).abs() < 1e-5);
                assert!((start_v.location.y + 1.0).abs() < 1e-5);
                let expected_bulge = FRAC_PI_8.tan();
                assert!((start_v.bulge - expected_bulge).abs() < 1e-4);
            }
            other => panic!("expected LwPolyline, got {other:?}"),
        }
    }

    /// #1318 repro: a 3D polyline whose first segment is not horizontal has no
    /// plan-view shape, so it samples to nothing. The seam test then compared
    /// two empty ends as equal and sliced the empty sample from index 1.
    #[test]
    fn sampling_skips_segments_with_no_plan_shape() {
        use codec::entities::{Polyline3D, Vertex3DPolyline};

        let mut pl = Polyline3D::new();
        pl.vertices = vec![
            // Vertical: an upright curve plane, dropped by `entity_curve_xy`.
            Vertex3DPolyline::from_xyz(0.0, 0.0, 0.0),
            Vertex3DPolyline::from_xyz(0.0, 0.0, 10.0),
            // Horizontal, so this one does sample.
            Vertex3DPolyline::from_xyz(10.0, 0.0, 10.0),
        ];

        let pts = sample_entity_xy(&EntityType::Polyline3D(pl));
        assert!(
            !pts.is_empty(),
            "the horizontal segment still has to be sampled"
        );
        assert!(
            pts.iter().all(|p| p.iter().all(|c| c.is_finite())),
            "sampled points stay finite: {pts:?}"
        );
    }

    /// The same skip must not swallow a leading segment that does sample, and
    /// must keep dropping the duplicated seam vertex between two of them.
    #[test]
    fn sampling_still_joins_segments_at_their_seam() {
        use codec::entities::{Polyline3D, Vertex3DPolyline};

        let mut pl = Polyline3D::new();
        pl.vertices = vec![
            Vertex3DPolyline::from_xyz(0.0, 0.0, 0.0),
            Vertex3DPolyline::from_xyz(10.0, 0.0, 0.0),
            Vertex3DPolyline::from_xyz(10.0, 10.0, 0.0),
        ];

        let pts = sample_entity_xy(&EntityType::Polyline3D(pl));
        let seam = [10.0, 0.0];
        assert_eq!(
            pts.iter().filter(|p| **p == seam).count(),
            1,
            "the shared vertex appears once, not twice: {pts:?}"
        );
    }

    #[test]
    fn preview_sampling_skips_edge_on_polyline2d_arc_segments() {
        use codec::entities::{Polyline2D, Vertex2D};

        let mut pl = Polyline2D::new();
        let mut start = Vertex2D::new(Vector3::new(0.0, 0.0, 0.0));
        start.bulge = 1.0;
        pl.vertices = vec![start, Vertex2D::new(Vector3::new(2.0, 0.0, 0.0))];
        pl.normal = Vector3::new(1.0, 0.0, 0.0);

        let pts = preview_sample_xy(&EntityType::Polyline2D(pl));
        assert!(
            pts.is_empty(),
            "edge-on arc segments are skipped instead of panicking: {pts:?}"
        );
    }

    #[test]
    fn trim_circle_with_tangent_arcs_touching_at_endpoints() {
        use codec::entities::{Arc as ArcEnt, Circle as CircleEnt};
        let mut c = CircleEnt::new();
        c.common.handle = Handle::new(1);
        c.center = Vector3::new(0.0, 0.0, 0.0);
        c.radius = 10.0;

        // Arc 1: center (20, 0), r = 10, from PI/2 to PI (endpoint at (10, 0))
        let mut a1 = ArcEnt::new();
        a1.common.handle = Handle::new(2);
        a1.center = Vector3::new(20.0, 0.0, 0.0);
        a1.radius = 10.0;
        a1.start_angle = std::f64::consts::FRAC_PI_2;
        a1.end_angle = std::f64::consts::PI;

        // Arc 2: center (-20, 0), r = 10, from 0 to PI/2 (endpoint at (-10, 0))
        let mut a2 = ArcEnt::new();
        a2.common.handle = Handle::new(3);
        a2.center = Vector3::new(-20.0, 0.0, 0.0);
        a2.radius = 10.0;
        a2.start_angle = 0.0;
        a2.end_angle = std::f64::consts::FRAC_PI_2;

        let all = vec![
            EntityType::Circle(c),
            EntityType::Arc(a1),
            EntityType::Arc(a2),
        ];
        let geos = build_geos(&all);

        // Click on top of the circle at (0, 10)
        let survivors = pick_trim_at(&all, &geos, Handle::new(1), 0.0, 10.0);
        assert!(
            survivors.is_some(),
            "trimming circle between two tangent arc endpoints must succeed"
        );
        let pieces = survivors.unwrap();
        assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
        if let EntityType::Arc(arc) = &pieces[0] {
            assert!((arc.radius - 10.0).abs() < 1e-6);
            let span = {
                let s = (arc.end_angle - arc.start_angle).rem_euclid(TAU);
                if s == 0.0 {
                    TAU
                } else {
                    s
                }
            };
            let mid_ang = arc.start_angle + span * 0.5;
            let mid_y = mid_ang.sin();
            assert!(
                mid_y < 0.0,
                "bottom half should survive when clicking top half"
            );
        } else {
            panic!("expected Arc survivor");
        }
    }

    #[test]
    fn test_trim_circle_between_two_tangent_lines() {
        use codec::entities::{Circle as CircleEnt, Line as LineEnt};
        let mut c = CircleEnt::new();
        c.common.handle = Handle::new(1);
        c.center = Vector3::new(0.0, 0.0, 0.0);
        c.radius = 10.0;

        // Line 1: tangent at top (0, 10), from (-20, 10) to (20, 10)
        let mut l1 = LineEnt::new();
        l1.common.handle = Handle::new(2);
        l1.start = Vector3::new(-20.0, 10.0, 0.0);
        l1.end = Vector3::new(20.0, 10.0, 0.0);

        // Line 2: tangent at bottom (0, -10), from (-20, -10) to (20, -10)
        let mut l2 = LineEnt::new();
        l2.common.handle = Handle::new(3);
        l2.start = Vector3::new(-20.0, -10.0, 0.0);
        l2.end = Vector3::new(20.0, -10.0, 0.0);

        let all = vec![
            EntityType::Circle(c),
            EntityType::Line(l1),
            EntityType::Line(l2),
        ];
        let geos = build_geos(&all);

        // Trim circle by clicking right side at (10, 0)
        let survivors = pick_trim_at(&all, &geos, Handle::new(1), 10.0, 0.0);
        assert!(
            survivors.is_some(),
            "trimming circle between two tangent lines must succeed"
        );
        let pieces = survivors.unwrap();
        assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
        let EntityType::Arc(arc) = &pieces[0] else {
            panic!("expected Arc")
        };

        let arc_start = glam::DVec3::new(
            arc.radius * arc.start_angle.cos(),
            arc.radius * arc.start_angle.sin(),
            0.0,
        );
        let arc_end = glam::DVec3::new(
            arc.radius * arc.end_angle.cos(),
            arc.radius * arc.end_angle.sin(),
            0.0,
        );

        println!("arc start: {:?}, end: {:?}", arc_start, arc_end);

        // Now trim line 1 by clicking at (10, 10)
        let l1_survivors = pick_trim_at(&all, &geos, Handle::new(2), 10.0, 10.0);
        assert!(l1_survivors.is_some(), "trimming line 1 must succeed");
        let l1_pieces = l1_survivors.unwrap();
        assert_eq!(l1_pieces.len(), 1);
        let EntityType::Line(trimmed_l1) = &l1_pieces[0] else {
            panic!("expected Line")
        };
        println!(
            "trimmed line 1 start: {:?}, end: {:?}",
            trimmed_l1.start, trimmed_l1.end
        );

        // Check distance between trimmed line 1 end and arc start
        let line_end = glam::DVec3::new(trimmed_l1.end.x, trimmed_l1.end.y, trimmed_l1.end.z);
        let diff = (line_end - arc_start).length();
        println!(
            "Distance between line endpoint and arc endpoint: {:e}",
            diff
        );

        // Now test an angled / non-axis-aligned case from external point
        let c_center = glam::DVec3::new(123.45, 67.89, 0.0);
        let c_radius = 25.0;
        let p_ext1 = glam::DVec3::new(200.0, 150.0, 0.0);
        let p_ext2 = glam::DVec3::new(50.0, -20.0, 0.0);

        let get_tangents = |p: glam::DVec3| {
            let k_curve = kernel::geom2d::Curve::Circle(kernel::geom2d::Circle {
                centre: [c_center.x, c_center.y],
                radius: c_radius,
            });
            let pts = kernel::geom2d::tangent_from(&k_curve, [p.x, p.y]);
            (
                glam::DVec3::new(pts[0].point[0], pts[0].point[1], 0.0),
                glam::DVec3::new(pts[1].point[0], pts[1].point[1], 0.0),
            )
        };
        let t1 = get_tangents(p_ext1).0;
        let t2 = get_tangents(p_ext2).1;

        let mut c_rot = CircleEnt::new();
        c_rot.common.handle = Handle::new(10);
        c_rot.center = Vector3::new(c_center.x, c_center.y, c_center.z);
        c_rot.radius = c_radius;

        // Line 1 from p_ext1 extending PAST t1 to make it a boundary line crossing tangent
        let dir1 = (t1 - p_ext1).normalize();
        let mut l_rot1 = LineEnt::new();
        l_rot1.common.handle = Handle::new(11);
        l_rot1.start = Vector3::new(p_ext1.x, p_ext1.y, 0.0);
        l_rot1.end = Vector3::new((t1 + dir1 * 20.0).x, (t1 + dir1 * 20.0).y, 0.0);

        let dir2 = (t2 - p_ext2).normalize();
        let mut l_rot2 = LineEnt::new();
        l_rot2.common.handle = Handle::new(12);
        l_rot2.start = Vector3::new(p_ext2.x, p_ext2.y, 0.0);
        l_rot2.end = Vector3::new((t2 + dir2 * 20.0).x, (t2 + dir2 * 20.0).y, 0.0);

        let all_rot = vec![
            EntityType::Circle(c_rot.clone()),
            EntityType::Line(l_rot1.clone()),
            EntityType::Line(l_rot2.clone()),
        ];
        let geos_rot = build_geos(&all_rot);

        // Click circle between them
        let mid_click = c_center + (t1 + t2 - 2.0 * c_center).normalize() * c_radius;
        let rot_survivors = pick_trim_at(
            &all_rot,
            &geos_rot,
            Handle::new(10),
            mid_click.x,
            mid_click.y,
        );
        assert!(
            rot_survivors.is_some(),
            "trimming circle between angled tangent lines must succeed"
        );
        let pieces = rot_survivors.unwrap();
        assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
        let EntityType::Arc(a) = &pieces[0] else {
            panic!("expected Arc")
        };
        let a_start = c_center
            + glam::DVec3::new(
                a.radius * a.start_angle.cos(),
                a.radius * a.start_angle.sin(),
                0.0,
            );
        let a_end = c_center
            + glam::DVec3::new(
                a.radius * a.end_angle.cos(),
                a.radius * a.end_angle.sin(),
                0.0,
            );
        let err_t1 = (a_start - t1).length().min((a_end - t1).length());
        let err_t2 = (a_start - t2).length().min((a_end - t2).length());
        assert!(
            err_t1 < 1e-9,
            "cut point must land within grip tolerance of t1: got {err_t1:e}"
        );
        assert!(
            err_t2 < 1e-9,
            "cut point must land within grip tolerance of t2: got {err_t2:e}"
        );
    }

    #[test]
    fn trim_ellipse_with_tangent_arcs_touching_at_endpoints() {
        use codec::entities::{Arc as ArcEnt, Ellipse as EllipseEnt};
        let mut e = EllipseEnt::new();
        e.common.handle = Handle::new(10);
        e.center = Vector3::new(0.0, 0.0, 0.0);
        e.major_axis = Vector3::new(20.0, 0.0, 0.0);
        e.minor_axis_ratio = 0.5; // b = 10
        e.start_parameter = 0.0;
        e.end_parameter = std::f64::consts::TAU;

        // Arc 1 touches right vertex at (20, 0): center (30, 0), r = 10, from PI/2 to PI
        let mut a1 = ArcEnt::new();
        a1.common.handle = Handle::new(11);
        a1.center = Vector3::new(30.0, 0.0, 0.0);
        a1.radius = 10.0;
        a1.start_angle = std::f64::consts::FRAC_PI_2;
        a1.end_angle = std::f64::consts::PI;

        // Arc 2 touches left vertex at (-20, 0): center (-30, 0), r = 10, from 0 to PI/2
        let mut a2 = ArcEnt::new();
        a2.common.handle = Handle::new(12);
        a2.center = Vector3::new(-30.0, 0.0, 0.0);
        a2.radius = 10.0;
        a2.start_angle = 0.0;
        a2.end_angle = std::f64::consts::FRAC_PI_2;

        let all = vec![
            EntityType::Ellipse(e),
            EntityType::Arc(a1),
            EntityType::Arc(a2),
        ];
        let geos = build_geos(&all);

        // Click top of ellipse at (0, 10)
        let survivors = pick_trim_at(&all, &geos, Handle::new(10), 0.0, 10.0);
        assert!(
            survivors.is_some(),
            "trimming ellipse between two tangent arc endpoints must succeed"
        );
        let pieces = survivors.unwrap();
        assert_eq!(pieces.len(), 1, "expected 1 surviving ellipse piece");
        if let EntityType::Ellipse(ell) = &pieces[0] {
            assert!((ell.major_axis.x - 20.0).abs() < 1e-6);
        } else {
            panic!("expected Ellipse survivor");
        }
    }

    #[test]
    fn trim_ellipse_selection_tracking_near_breakpoint() {
        use codec::entities::{Ellipse as EllipseEnt, Line as LineEnt};
        use std::f64::consts::TAU;

        let mut e = EllipseEnt::new();
        e.common.handle = Handle::new(10);
        e.center = Vector3::new(0.0, 0.0, 0.0);
        e.major_axis = Vector3::new(20.0, 0.0, 0.0);
        e.minor_axis_ratio = 0.5; // a = 20, b = 10
        e.start_parameter = 0.0;
        e.end_parameter = TAU;

        // Cutting line 1 crossing at t = PI/4 (45 degrees parametric):
        // At t = PI/4: x = 20 * cos(PI/4) = 14.142, y = 10 * sin(PI/4) = 7.071
        // (Polar angle is atan2(7.071, 14.142) = 26.56 degrees!)
        let mut l1 = LineEnt::new();
        l1.common.handle = Handle::new(11);
        l1.start = Vector3::new(0.0, 0.0, 0.0);
        l1.end = Vector3::new(30.0, 15.0, 0.0); // passes through (14.142, 7.071)

        // Cutting line 2 crossing at t = 3*PI/4:
        let mut l2 = LineEnt::new();
        l2.common.handle = Handle::new(12);
        l2.start = Vector3::new(0.0, 0.0, 0.0);
        l2.end = Vector3::new(-30.0, 15.0, 0.0);

        let all = vec![
            EntityType::Ellipse(e),
            EntityType::Line(l1),
            EntityType::Line(l2),
        ];
        let geos = build_geos(&all);

        // Click 1: Click at t = 40 deg parametric (x = 20*cos(40°), y = 10*sin(40°))
        // Polar angle is atan(0.5 * tan(40°)) ≈ 22.7°.
        // This is before PI/4. Surviving piece should run between the cuts.
        let t_before = 40.0_f64.to_radians();
        let px_before = 20.0 * t_before.cos();
        let py_before = 10.0 * t_before.sin();
        let res_before = pick_trim_at(&all, &geos, Handle::new(10), px_before, py_before);
        assert!(res_before.is_some(), "trimming at 40 deg must succeed");
        let pieces_before = res_before.unwrap();
        assert_eq!(pieces_before.len(), 1);

        // Click 2: Click at t = 50 deg parametric (x = 20*cos(50°), y = 10*sin(50°))
        // Polar angle is atan(0.5 * tan(50°)) ≈ 30.77°.
        // Visually and parametrically, 50° is in the top segment (between PI/4 and 3*PI/4).
        // Without the fix, 30.77° polar angle would be < 45° parametric, selecting the WRONG segment!
        let t_after = 50.0_f64.to_radians();
        let px_after = 20.0 * t_after.cos();
        let py_after = 10.0 * t_after.sin();
        let res_after = pick_trim_at(&all, &geos, Handle::new(10), px_after, py_after);
        assert!(res_after.is_some(), "trimming at 50 deg must succeed");
        let pieces_after = res_after.unwrap();
        assert_eq!(pieces_after.len(), 1);

        // Click 3 & 4: Click slightly INSIDE and OUTSIDE the ellipse near 50 deg.
        // Even when displaced by 3 units (pick-box offset when zoomed out),
        // orthogonal projection must still correctly identify the top segment!
        let nx = 10.0 * t_after.cos();
        let ny = 20.0 * t_after.sin();
        let nlen = nx.hypot(ny);
        let (unx, uny) = (nx / nlen, ny / nlen);

        // Displaced outside:
        let res_out = pick_trim_at(
            &all,
            &geos,
            Handle::new(10),
            px_after + 3.0 * unx,
            py_after + 3.0 * uny,
        );
        assert!(
            res_out.is_some(),
            "trimming at displaced outside point must succeed"
        );

        // Displaced inside:
        let res_in = pick_trim_at(
            &all,
            &geos,
            Handle::new(10),
            px_after - 3.0 * unx,
            py_after - 3.0 * uny,
        );
        assert!(
            res_in.is_some(),
            "trimming at displaced inside point must succeed"
        );

        if let (
            EntityType::Ellipse(surv_after),
            EntityType::Ellipse(surv_out),
            EntityType::Ellipse(surv_in),
        ) = (&pieces_after[0], &res_out.unwrap()[0], &res_in.unwrap()[0])
        {
            assert!(
                (surv_out.start_parameter - surv_after.start_parameter).abs() < 1e-4,
                "displaced outside point should select the same segment as point on curve"
            );
            assert!(
                (surv_in.start_parameter - surv_after.start_parameter).abs() < 1e-4,
                "displaced inside point should select the same segment as point on curve"
            );
        }
    }

    #[test]
    fn trim_arc_selection_tracking_near_start_endpoint() {
        use codec::entities::{Arc as ArcEnt, Line as LineEnt};
        // Arc from 0.5 to 2.0 rad (~28.6° to 114.6°), radius 10.0
        let mut a = ArcEnt::new();
        a.common.handle = Handle::new(20);
        a.center = Vector3::new(0.0, 0.0, 0.0);
        a.radius = 10.0;
        a.start_angle = 0.5;
        a.end_angle = 2.0;

        // Boundary line crossing arc at midpoint ~ 1.25 rad
        let mid = 1.25_f64;
        let mut l = LineEnt::new();
        l.common.handle = Handle::new(21);
        l.start = Vector3::new(0.0, 0.0, 0.0);
        l.end = Vector3::new(20.0 * mid.cos(), 20.0 * mid.sin(), 0.0);

        let all = vec![EntityType::Arc(a), EntityType::Line(l)];
        let geos = build_geos(&all);

        // Hover slightly CW of start endpoint (e.g. angle = 0.49 rad, outside the arc by ~0.5 degree)
        let click_ang = 0.49_f64;
        let px = 10.0 * click_ang.cos();
        let py = 10.0 * click_ang.sin();
        let res = pick_trim_at(&all, &geos, Handle::new(20), px, py);
        assert!(res.is_some(), "trim near start endpoint must succeed");
        let pieces = res.unwrap();
        assert_eq!(pieces.len(), 1, "expected 1 surviving arc piece");
        if let EntityType::Arc(surv) = &pieces[0] {
            // It must remove the start-half [0.5, 1.25] and keep the end-half [1.25, 2.0].
            // If the bug were present, it would snap to 1.0 (end-half removed, start-half kept).
            assert!(
                (surv.end_angle - 2.0).abs() < 0.05,
                "survivor should be the end-half [1.25, 2.0], got start={} end={}",
                surv.start_angle,
                surv.end_angle
            );
        } else {
            panic!("expected Arc survivor");
        }
    }

    #[test]
    fn test_tangent_circles_or_arcs_trim() {
        use codec::entities::{Arc as ArcEnt, Circle as CircleEnt};
        let mut c1 = CircleEnt::new();
        c1.common.handle = Handle::new(1);
        c1.center = Vector3::new(0.0, 0.0, 0.0);
        c1.radius = 10.0;

        let mut c2 = CircleEnt::new();
        c2.common.handle = Handle::new(2);
        c2.center = Vector3::new(20.0, 0.0, 0.0);
        c2.radius = 10.0;

        let all = vec![
            EntityType::Circle(c1.clone()),
            EntityType::Circle(c2.clone()),
        ];
        let geos = build_geos(&all);
        let curve1 = entity_curve_xy(&EntityType::Circle(c1.clone())).unwrap();
        let ts = cut_params(&curve1, Handle::new(1), &geos);
        assert_eq!(
            ts,
            vec![0.0],
            "circle1 tangent to circle2 must have cut at t=0"
        );

        // Arc1 touching circle2 tangently
        let mut a1 = ArcEnt::new();
        a1.common.handle = Handle::new(3);
        a1.center = Vector3::new(0.0, 0.0, 0.0);
        a1.radius = 10.0;
        a1.start_angle = -1.0;
        a1.end_angle = 1.0;

        let all_arc = vec![EntityType::Arc(a1.clone()), EntityType::Circle(c2.clone())];
        let geos_arc = build_geos(&all_arc);
        let curve_arc = entity_curve_xy(&EntityType::Arc(a1.clone())).unwrap();
        let ts_arc = cut_params(&curve_arc, Handle::new(3), &geos_arc);
        assert_eq!(
            ts_arc,
            vec![0.5],
            "arc1 tangent to circle2 at midpoint must have cut at t=0.5"
        );

        let trim_res = pick_trim_at(&all_arc, &geos_arc, Handle::new(3), 10.0, 5.0);
        assert!(
            trim_res.is_some(),
            "trimming arc1 at tangent circle2 must succeed"
        );
        let pieces = trim_res.unwrap();
        assert_eq!(
            pieces.len(),
            1,
            "trimming half of arc should leave 1 survivor piece"
        );

        // 1. External tangency: Test across multiple angles and slight epsilons
        for angle in [0.0f64, 0.5, 1.0, 1.57, 2.5, std::f64::consts::PI, 4.0, 5.0] {
            for eps in [-1e-8, 0.0, 1e-8] {
                let dist = 20.0 + eps;
                let dx = dist * angle.cos();
                let dy = dist * angle.sin();
                let mut c_off = CircleEnt::new();
                c_off.common.handle = Handle::new(4);
                c_off.center = Vector3::new(dx, dy, 0.0);
                c_off.radius = 10.0;
                let all_off = vec![EntityType::Circle(c1.clone()), EntityType::Circle(c_off)];
                let geos_off = build_geos(&all_off);
                let ts_off = cut_params(&curve1, Handle::new(1), &geos_off);
                assert_eq!(
                    ts_off.len(),
                    1,
                    "must find exactly 1 tangency cut at angle {angle}, eps {eps}"
                );
                let expected_t = (angle / TAU).rem_euclid(1.0);
                let actual_t = ts_off[0];
                let diff = (actual_t - expected_t).abs();
                let cyclic_diff = diff.min(1.0 - diff);
                assert!(
                    cyclic_diff < 1e-5,
                    "tangency t must match angle: got {actual_t}, expected {expected_t}"
                );
            }
        }

        // 2. Internal tangency: c1 (r=10) and c_inner (r=4) at (6, 0)
        let mut c_inner = CircleEnt::new();
        c_inner.common.handle = Handle::new(5);
        c_inner.center = Vector3::new(6.0, 0.0, 0.0);
        c_inner.radius = 4.0;
        let all_int = vec![
            EntityType::Circle(c1.clone()),
            EntityType::Circle(c_inner.clone()),
        ];
        let geos_int = build_geos(&all_int);
        let ts_int = cut_params(&curve1, Handle::new(1), &geos_int);
        assert_eq!(
            ts_int,
            vec![0.0],
            "internal tangency on c1 must have cut at t=0"
        );

        // And from c_inner's perspective (r=4 inside r=10):
        let curve_inner = entity_curve_xy(&EntityType::Circle(c_inner.clone())).unwrap();
        let ts_inner = cut_params(&curve_inner, Handle::new(5), &geos_int);
        assert_eq!(
            ts_inner,
            vec![0.0],
            "internal tangency on c_inner must have cut at t=0"
        );

        // 3. Two arcs touching tangently at (10, 0)
        // a_left: center (0, 0), radius 10, start=0, end=PI (passes through (10,0) at start t=0)
        let mut a_left = ArcEnt::new();
        a_left.common.handle = Handle::new(6);
        a_left.center = Vector3::new(0.0, 0.0, 0.0);
        a_left.radius = 10.0;
        a_left.start_angle = 0.0;
        a_left.end_angle = std::f64::consts::PI;

        // a_right: center (20, 0), radius 10, start=PI/2, end=3*PI/2 (passes through (10,0) at angle PI, midpoint t=0.5)
        let mut a_right = ArcEnt::new();
        a_right.common.handle = Handle::new(7);
        a_right.center = Vector3::new(20.0, 0.0, 0.0);
        a_right.radius = 10.0;
        a_right.start_angle = std::f64::consts::FRAC_PI_2;
        a_right.end_angle = 3.0 * std::f64::consts::FRAC_PI_2;

        let all_arcs = vec![
            EntityType::Arc(a_left.clone()),
            EntityType::Arc(a_right.clone()),
        ];
        let geos_arcs = build_geos(&all_arcs);
        let curve_right = entity_curve_xy(&EntityType::Arc(a_right.clone())).unwrap();
        let ts_right = cut_params(&curve_right, Handle::new(7), &geos_arcs);
        assert_eq!(
            ts_right,
            vec![0.5],
            "a_right must be cut at midpoint t=0.5 by tangent a_left"
        );

        let trim_right = pick_trim_at(&all_arcs, &geos_arcs, Handle::new(7), 15.0, 8.0);
        assert!(
            trim_right.is_some(),
            "trimming a_right at tangent a_left must succeed"
        );

        // 4. Circle trimmed by 2 tangent circles
        // c1 at (0, 0) r=10. c_right at (20, 0) r=10. c_left at (-20, 0) r=10.
        let mut c_left = CircleEnt::new();
        c_left.common.handle = Handle::new(8);
        c_left.center = Vector3::new(-20.0, 0.0, 0.0);
        c_left.radius = 10.0;
        let all_3c = vec![
            EntityType::Circle(c1.clone()),
            EntityType::Circle(c2.clone()),
            EntityType::Circle(c_left.clone()),
        ];
        let geos_3c = build_geos(&all_3c);
        let ts_3c = cut_params(&curve1, Handle::new(1), &geos_3c);
        assert_eq!(
            ts_3c.len(),
            2,
            "c1 touched by 2 tangent circles must have 2 cut points"
        );
        assert_eq!(ts_3c, vec![0.0, 0.5]);

        let trim_3c = pick_trim_at(&all_3c, &geos_3c, Handle::new(1), 0.0, 10.0);
        assert!(
            trim_3c.is_some(),
            "c1 with 2 tangent circles must be trimmable"
        );
        if let Some(surv) = trim_3c {
            assert_eq!(surv.len(), 1, "trimming c1 should leave 1 arc survivor");
        }

        // 5. Extending an arc to meet a tangent circle
        // a_short: center (0, 0), radius 10, start=0.5, end=1.5 (span ~ 1.0 rad)
        // Tangent circle c2 is at (20, 0), tangent point is at (10, 0) which is angle 0.
        // Clicking near start extends start CW towards 0.0.
        let mut a_short = ArcEnt::new();
        a_short.common.handle = Handle::new(9);
        a_short.center = Vector3::new(0.0, 0.0, 0.0);
        a_short.radius = 10.0;
        a_short.start_angle = 0.5;
        a_short.end_angle = 1.5;
        let all_ext = vec![
            EntityType::Arc(a_short.clone()),
            EntityType::Circle(c2.clone()),
        ];
        let geos_ext = build_geos(&all_ext);
        let ext_res = pick_extend_at(
            &all_ext,
            &geos_ext,
            Handle::new(9),
            10.0 * 0.6_f64.cos(),
            10.0 * 0.6_f64.sin(),
        );
        assert!(
            ext_res.is_some(),
            "extending arc towards tangent circle must succeed"
        );
        if let Some(EntityType::Arc(ext_arc)) = ext_res {
            let start_norm = norm(ext_arc.start_angle);
            assert!(
                start_norm < 1e-5 || (TAU - start_norm) < 1e-5,
                "extended arc start must reach 0.0 (tangent point)"
            );
        }
    }

    #[test]
    fn test_skinny_ellipse_crossing_arc() {
        use codec::entities::{Arc as ArcEnt, Ellipse as EllipseEnt};
        let mut e = EllipseEnt::new();
        e.common.handle = Handle::new(100);
        e.center = Vector3::new(0.0, 0.0, 0.0);
        e.major_axis = Vector3::new(100.0, 0.0, 0.0);
        e.minor_axis_ratio = 0.05; // b = 5, very skinny!
        e.start_parameter = 0.0;
        e.end_parameter = std::f64::consts::TAU;

        // Test arcs that physically cross the ellipse near its ends (x = 80, 90, 95, 98, 99)
        for &r in &[80.0, 90.0, 95.0, 98.0, 99.0] {
            let mut a = ArcEnt::new();
            a.common.handle = Handle::new(101);
            a.center = Vector3::new(0.0, 0.0, 0.0);
            a.radius = r;
            a.start_angle = -0.3;
            a.end_angle = 0.3;

            let all = vec![EntityType::Ellipse(e.clone()), EntityType::Arc(a)];
            let geos = build_geos(&all);

            // Trim the ellipse tip beyond the arc (near x = 99.5, y = 0)
            let ell_res = pick_trim_at(&all, &geos, Handle::new(100), 99.9, 0.0);
            // Trim the arc near y = 0 (x = r, y = 0)
            let arc_res = pick_trim_at(&all, &geos, Handle::new(101), r, 0.0);
            assert!(
                ell_res.is_some(),
                "ellipse trim should succeed for crossing arc at r={r}"
            );
            assert!(
                arc_res.is_some(),
                "arc trim should succeed for crossing arc at r={r}"
            );
        }

        // Test off-axis arc crossing near the tip (center at (96, 20), radius 20, passing near (96, 0))
        {
            let mut a = ArcEnt::new();
            a.common.handle = Handle::new(102);
            a.center = Vector3::new(96.0, 20.0, 0.0);
            a.radius = 20.0;
            // Angle around -PI/2 (bottom point is (96, 0))
            a.start_angle = -std::f64::consts::FRAC_PI_2 - 0.4;
            a.end_angle = -std::f64::consts::FRAC_PI_2 + 0.4;

            let all = vec![EntityType::Ellipse(e.clone()), EntityType::Arc(a)];
            let geos = build_geos(&all);

            let ell_res = pick_trim_at(&all, &geos, Handle::new(100), 99.0, 0.0);
            let arc_res = pick_trim_at(&all, &geos, Handle::new(102), 96.0, 0.0);
            assert!(
                ell_res.is_some(),
                "ellipse trim should succeed with off-axis crossing arc"
            );
            assert!(
                arc_res.is_some(),
                "arc trim should succeed with off-axis crossing arc"
            );
        }
    }

    #[test]
    fn test_trim_line_tangent_to_arc() {
        use codec::entities::{Arc as ArcEnt, Line as LineEnt};
        let mut l1 = LineEnt::new();
        l1.common.handle = Handle::new(1);
        l1.start = Vector3::new(-10.0, 0.0, 0.0);
        l1.end = Vector3::new(10.0, 0.0, 0.0);

        let mut a = ArcEnt::new();
        a.common.handle = Handle::new(2);
        a.center = Vector3::new(0.0, 5.0, 0.0);
        a.radius = 5.0;
        a.start_angle = -std::f64::consts::FRAC_PI_2;
        a.end_angle = std::f64::consts::PI;

        let all = vec![EntityType::Line(l1.clone()), EntityType::Arc(a.clone())];
        let geos = build_geos(&all);
        let curve_line = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
        let ts_line = cut_params(&curve_line, Handle::new(1), &geos);
        assert!(
            !ts_line.is_empty(),
            "line must have cut point where tangent arc touches it: ts={:?}",
            ts_line
        );

        let curve_arc = entity_curve_xy(&EntityType::Arc(a.clone())).unwrap();
        let ts_arc = cut_params(&curve_arc, Handle::new(2), &geos);
        println!("ts_arc: {:?}", ts_arc);
    }

    #[test]
    fn test_trim_arc_tangent_to_two_lines() {
        use crate::command::{TangentObject, WorkingPlane};
        use crate::modules::draw::draw::arc::{solve_arc_3_inputs, ArcPick};
        use codec::entities::{Arc as CadArc, Line as LineEnt};

        let mut l1 = LineEnt::new();
        l1.common.handle = Handle::new(1);
        l1.start = Vector3::new(-10.0, 0.0, 0.0);
        l1.end = Vector3::new(10.0, 0.0, 0.0);

        let mut l2 = LineEnt::new();
        l2.common.handle = Handle::new(2);
        l2.start = Vector3::new(0.0, -10.0, 0.0);
        l2.end = Vector3::new(0.0, 10.0, 0.0);

        let t1 = TangentObject::Line {
            p1: DVec3::new(-10.0, 0.0, 0.0),
            p2: DVec3::new(10.0, 0.0, 0.0),
        };
        let t2 = TangentObject::Line {
            p1: DVec3::new(0.0, -10.0, 0.0),
            p2: DVec3::new(0.0, 10.0, 0.0),
        };

        // Middle point: (1 - sqrt(0.5), 1 - sqrt(0.5)) -> radius ~ 1
        let mid = DVec3::new(1.0 - (0.5_f64).sqrt(), 1.0 - (0.5_f64).sqrt(), 0.0);
        let sol = solve_arc_3_inputs(
            ArcPick::Tangent {
                target: t1,
                hit: DVec3::new(2.0, 0.0, 0.0),
            },
            ArcPick::Point(mid),
            ArcPick::Tangent {
                target: t2,
                hit: DVec3::new(0.0, 2.0, 0.0),
            },
            WorkingPlane::default(),
        );

        let (center, radius, sa, ea) = sol.expect("must solve arc tangent to two lines");
        let mut arc = CadArc::new();
        arc.common.handle = Handle::new(3);
        arc.center = Vector3::new(center.x, center.y, center.z);
        arc.radius = radius;
        arc.start_angle = sa;
        arc.end_angle = ea;

        let all = vec![
            EntityType::Line(l1.clone()),
            EntityType::Line(l2.clone()),
            EntityType::Arc(arc.clone()),
        ];
        let geos = build_geos(&all);

        let curve_l1 = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
        let ts_l1 = cut_params(&curve_l1, Handle::new(1), &geos);
        println!("ts_l1: {:?}", ts_l1);

        let curve_l2 = entity_curve_xy(&EntityType::Line(l2.clone())).unwrap();
        let ts_l2 = cut_params(&curve_l2, Handle::new(2), &geos);
        println!("ts_l2: {:?}", ts_l2);

        let curve_arc = entity_curve_xy(&EntityType::Arc(arc.clone())).unwrap();
        let ts_arc = cut_params(&curve_arc, Handle::new(3), &geos);
        println!("ts_arc: {:?}", ts_arc);

        assert!(!ts_l1.is_empty(), "Line 1 must be cut: ts_l1={:?}", ts_l1);
        assert!(!ts_l2.is_empty(), "Line 2 must be cut: ts_l2={:?}", ts_l2);

        // Test trimming line 1 arm between (1, 0) and (10, 0)
        let trim_l1 = pick_trim_at(&all, &geos, Handle::new(1), 5.0, 0.0);
        println!("trim_l1: {:?}", trim_l1);
        assert!(trim_l1.is_some(), "trimming line 1 arm must succeed");

        // Test trimming line 2 arm between (0, 1) and (0, 10)
        let trim_l2 = pick_trim_at(&all, &geos, Handle::new(2), 0.0, 5.0);
        println!("trim_l2: {:?}", trim_l2);
        assert!(trim_l2.is_some(), "trimming line 2 arm must succeed");

        // Test trimming the arc
        let trim_arc = pick_trim_at(&all, &geos, Handle::new(3), mid.x, mid.y);
        println!("trim_arc: {:?}", trim_arc);
    }

    #[test]
    fn test_trim_arc_tangent_angled_lines() {
        use crate::command::{TangentObject, WorkingPlane};
        use crate::modules::draw::draw::arc::{solve_arc_3_inputs, ArcPick};
        use codec::entities::{Arc as CadArc, Line as LineEnt};

        // Two lines meeting at (0, 0): one along (cos(20 deg), sin(20 deg)), other along (cos(80 deg), sin(80 deg))
        let a1 = 20.0_f64.to_radians();
        let a2 = 80.0_f64.to_radians();
        let dir1 = DVec3::new(a1.cos(), a1.sin(), 0.0);
        let dir2 = DVec3::new(a2.cos(), a2.sin(), 0.0);

        let mut l1 = LineEnt::new();
        l1.common.handle = Handle::new(1);
        l1.start = Vector3::new(-10.0 * dir1.x, -10.0 * dir1.y, 0.0);
        l1.end = Vector3::new(50.0 * dir1.x, 50.0 * dir1.y, 0.0);

        let mut l2 = LineEnt::new();
        l2.common.handle = Handle::new(2);
        l2.start = Vector3::new(-10.0 * dir2.x, -10.0 * dir2.y, 0.0);
        l2.end = Vector3::new(50.0 * dir2.x, 50.0 * dir2.y, 0.0);

        let t1 = TangentObject::Line {
            p1: DVec3::new(l1.start.x, l1.start.y, 0.0),
            p2: DVec3::new(l1.end.x, l1.end.y, 0.0),
        };
        let t2 = TangentObject::Line {
            p1: DVec3::new(l2.start.x, l2.start.y, 0.0),
            p2: DVec3::new(l2.end.x, l2.end.y, 0.0),
        };

        // Midpoint between points at distance 20 on the bisector
        let bisector_angle = (a1 + a2) * 0.5;
        let mid = DVec3::new(
            15.0 * bisector_angle.cos(),
            15.0 * bisector_angle.sin(),
            0.0,
        );

        let sol = solve_arc_3_inputs(
            ArcPick::Tangent {
                target: t1,
                hit: DVec3::new(20.0 * dir1.x, 20.0 * dir1.y, 0.0),
            },
            ArcPick::Point(mid),
            ArcPick::Tangent {
                target: t2,
                hit: DVec3::new(20.0 * dir2.x, 20.0 * dir2.y, 0.0),
            },
            WorkingPlane::default(),
        );

        let (center, radius, sa, ea) = sol.expect("must solve arc tangent to two angled lines");
        let mut arc = CadArc::new();
        arc.common.handle = Handle::new(3);
        arc.center = Vector3::new(center.x, center.y, center.z);
        arc.radius = radius;
        arc.start_angle = sa;
        arc.end_angle = ea;

        let all = vec![
            EntityType::Line(l1.clone()),
            EntityType::Line(l2.clone()),
            EntityType::Arc(arc.clone()),
        ];
        let geos = build_geos(&all);

        let curve_l1 = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
        let ts_l1 = cut_params(&curve_l1, Handle::new(1), &geos);
        println!("angled ts_l1: {:?}", ts_l1);

        let curve_l2 = entity_curve_xy(&EntityType::Line(l2.clone())).unwrap();
        let ts_l2 = cut_params(&curve_l2, Handle::new(2), &geos);
        println!("angled ts_l2: {:?}", ts_l2);

        // We expect l1 to have at least 2 cuts: the intersection with l2 at (0,0) and the tangent point with arc!
        assert!(
            ts_l1.len() >= 2,
            "Line 1 must have intersection and tangent cut: got {:?}",
            ts_l1
        );
        assert!(
            ts_l2.len() >= 2,
            "Line 2 must have intersection and tangent cut: got {:?}",
            ts_l2
        );

        // Trimming l1 arm past the tangent point
        let click_l1 = 40.0 * dir1;
        let trim_l1 = pick_trim_at(&all, &geos, Handle::new(1), click_l1.x, click_l1.y);
        println!("angled trim_l1: {:?}", trim_l1);
        assert!(trim_l1.is_some(), "trimming line 1 must succeed");
    }

    #[test]
    fn test_kernel_intersect_tangency() {
        use std::f64::consts::FRAC_PI_2;
        let line = Curve::Line(KernelLine {
            start: [-10.0, 0.0],
            end: [10.0, 0.0],
        });

        // Case 1: Exact tangency at interior of arc
        let arc_exact = Curve::Arc(KernelArc {
            centre: [0.0, 5.0],
            radius: 5.0,
            start_angle: -FRAC_PI_2 - 0.2,
            end_angle: -FRAC_PI_2 + 0.2,
        });
        let hits_exact = kernel_intersect(&line, &arc_exact, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_exact: {:?}", hits_exact);

        // Case 2: Distance slightly greater than R by 1e-12 (within tolerance 1e-7)
        let arc_eps = Curve::Arc(KernelArc {
            centre: [0.0, 5.0 + 1e-12],
            radius: 5.0,
            start_angle: -FRAC_PI_2 - 0.2,
            end_angle: -FRAC_PI_2 + 0.2,
        });
        let hits_eps = kernel_intersect(&line, &arc_eps, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_eps: {:?}", hits_eps);

        // Case 3: Distance slightly greater than R by 1e-9
        let arc_eps9 = Curve::Arc(KernelArc {
            centre: [0.0, 5.0 + 1e-9],
            radius: 5.0,
            start_angle: -FRAC_PI_2 - 0.2,
            end_angle: -FRAC_PI_2 + 0.2,
        });
        let hits_eps9 = kernel_intersect(&line, &arc_eps9, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_eps9: {:?}", hits_eps9);

        // Case 4: Tangency at arc start endpoint
        let arc_start = Curve::Arc(KernelArc {
            centre: [0.0, 5.0],
            radius: 5.0,
            start_angle: -FRAC_PI_2,
            end_angle: 0.0,
        });
        let hits_start = kernel_intersect(&line, &arc_start, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_start (-PI/2): {:?}", hits_start);

        // Case 5: Tangency at arc start endpoint with normalized angle 3*PI/2
        let arc_norm = Curve::Arc(KernelArc {
            centre: [0.0, 5.0],
            radius: 5.0,
            start_angle: 3.0 * FRAC_PI_2,
            end_angle: 0.0, // wait! In CCW, from 3*PI/2 to 0 is span PI/2, or 2*PI?
        });
        let hits_norm = kernel_intersect(&line, &arc_norm, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_norm (3*PI/2): {:?}", hits_norm);

        // Case 6: Tangency at arc end endpoint
        let arc_end = Curve::Arc(KernelArc {
            centre: [0.0, 5.0],
            radius: 5.0,
            start_angle: std::f64::consts::PI,
            end_angle: -FRAC_PI_2,
        });
        let hits_end = kernel_intersect(&line, &arc_end, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_end (-PI/2): {:?}", hits_end);

        // Case 7: Large coordinates (e.g. 5000, 5005)
        let line_large = Curve::Line(KernelLine {
            start: [4990.0, 5000.0],
            end: [5010.0, 5000.0],
        });
        let arc_large = Curve::Arc(KernelArc {
            centre: [5000.0, 5005.0],
            radius: 5.0,
            start_angle: -FRAC_PI_2,
            end_angle: 0.0,
        });
        let hits_large =
            kernel_intersect(&line_large, &arc_large, KernelTolerance::new(CUT_TOLERANCE));
        println!("hits_large: {:?}", hits_large);

        // Test with cut_params:
        let geos_large = vec![Geo::Arc {
            handle: Handle::new(2),
            cx: 5000.0,
            cy: 5005.0,
            r: 5.0,
            a0: -FRAC_PI_2,
            a1: 0.0,
        }];
        let cuts_large = cut_params(&line_large, Handle::new(1), &geos_large);
        let _ = cuts_large;
    }

    #[test]
    fn test_arc_tangent_line_precision_sweep() {
        use crate::command::{TangentObject, WorkingPlane};
        use crate::modules::draw::draw::arc::{solve_arc_3_inputs, ArcPick};
        use codec::entities::{Arc as CadArc, Line as LineEnt};

        let mut max_dist = 0.0_f64;
        let mut failed_cuts = 0;

        for i in 1..36 {
            let angle_deg = i as f64 * 5.0; // 5 to 175 degrees
            let a2 = angle_deg.to_radians();

            let dir1 = DVec3::new(1.0, 0.0, 0.0);
            let dir2 = DVec3::new(a2.cos(), a2.sin(), 0.0);

            let mut l1 = LineEnt::new();
            l1.common.handle = Handle::new(1);
            l1.start = Vector3::new(-50.0, 0.0, 0.0);
            l1.end = Vector3::new(150.0, 0.0, 0.0);

            let mut l2 = LineEnt::new();
            l2.common.handle = Handle::new(2);
            l2.start = Vector3::new(-50.0 * dir2.x, -50.0 * dir2.y, 0.0);
            l2.end = Vector3::new(150.0 * dir2.x, 150.0 * dir2.y, 0.0);

            let t1 = TangentObject::Line {
                p1: DVec3::new(l1.start.x, l1.start.y, 0.0),
                p2: DVec3::new(l1.end.x, l1.end.y, 0.0),
            };
            let t2 = TangentObject::Line {
                p1: DVec3::new(l2.start.x, l2.start.y, 0.0),
                p2: DVec3::new(l2.end.x, l2.end.y, 0.0),
            };

            let bisector = (dir1 + dir2).normalize();
            let mid = bisector * 30.0;

            let sol = solve_arc_3_inputs(
                ArcPick::Tangent {
                    target: t1,
                    hit: DVec3::new(50.0, 0.0, 0.0),
                },
                ArcPick::Point(mid),
                ArcPick::Tangent {
                    target: t2,
                    hit: dir2 * 50.0,
                },
                WorkingPlane::default(),
            );

            if let Some((center, radius, sa, ea)) = sol {
                let mut arc = CadArc::new();
                arc.common.handle = Handle::new(3);
                arc.center = Vector3::new(center.x, center.y, center.z);
                arc.radius = radius;
                arc.start_angle = sa;
                arc.end_angle = ea;

                let all = vec![
                    EntityType::Line(l1.clone()),
                    EntityType::Line(l2.clone()),
                    EntityType::Arc(arc.clone()),
                ];
                let geos = build_geos(&all);

                let curve_l1 = entity_curve_xy(&EntityType::Line(l1.clone())).unwrap();
                let ts_l1 = cut_params(&curve_l1, Handle::new(1), &geos);

                let curve_l2 = entity_curve_xy(&EntityType::Line(l2.clone())).unwrap();
                let ts_l2 = cut_params(&curve_l2, Handle::new(2), &geos);

                if ts_l1.len() < 2 || ts_l2.len() < 2 {
                    failed_cuts += 1;
                    println!(
                        "FAILED at angle {angle_deg} deg: ts_l1={:?}, ts_l2={:?}",
                        ts_l1, ts_l2
                    );
                }

                // Check distance of arc endpoints to line
                let ep1 = [center.x + radius * sa.cos(), center.y + radius * sa.sin()];
                let d1 = (ep1[1] - 0.0).abs(); // distance to l1 (y=0)
                if d1 > max_dist {
                    max_dist = d1;
                }
            }
        }
        println!("Sweep results: failed_cuts={failed_cuts}, max_dist={max_dist:e}");
        assert_eq!(failed_cuts, 0, "No cuts should fail across angle sweep");
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
            // A segment with no plan-view shape samples to nothing — an
            // edge-on plane has no XY curve, so `entity_curve_xy` declines it.
            // Skipping it here also keeps the seam test below from comparing
            // two `None`s and then slicing an empty sample (#1318).
            if sp.is_empty() {
                continue;
            }
            if pts.last() == sp.first() {
                pts.extend_from_slice(&sp[1..]);
            } else {
                pts.extend(sp);
            }
        }
        if pts.is_empty() {
            return projected_polyline_points_xy(e);
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
                    [
                        c.center.x + c.radius * a.cos(),
                        c.center.y + c.radius * a.sin(),
                    ]
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
                if sp.is_empty() {
                    continue;
                }
                if pts.last() == sp.first() {
                    pts.extend_from_slice(&sp[1..]);
                } else {
                    pts.extend(sp);
                }
            }
            if pts.is_empty() {
                return projected_polyline_points_xy(e);
            }
            pts
        }
        _ => sample_entity_xy(e),
    }
}

/// Project a planar polyline into the XY working plane and sample its curve.
/// This covers imported OCS polylines whose extrusion normal prevents the
/// older entity-explosion path from producing world-XY line/arc pieces.
fn projected_polyline_points_xy(e: &EntityType) -> Vec<[f64; 2]> {
    entity_curve_xy(e)
        .filter(|curve| curve.extent() == KernelExtent::Bounded)
        .map(|curve| curve.tessellate(SAMPLE_SEGMENTS_PER_RADIAN))
        .unwrap_or_default()
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
        bg_adapt: None,
        point_marker: None,
        taper_widths: Vec::new(),
        pattern_stations: Vec::new(),
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        snap_only: false,
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

        ..Default::default()
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
        Self {
            all,
            boundary: None,
            geos: Vec::new(),
        }
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
                        let mid = [
                            a.center.x + a.radius * am.cos(),
                            a.center.y + a.radius * am.sin(),
                        ];
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

inventory::submit!(crate::command::CommandRegistration { names: &["EXTRIM"] }); // ExtrimCommand
