// Fillet / Chamfer — ribbon definitions + full command implementations.
//
// FILLET (F):
//   Supports: Line-Line, Line-Arc, Arc-Line, Arc-Arc.
//   Finds intersection, computes tangent arc of radius R, trims both entities.
//   R=0 just extends/trims to the exact intersection (sharp corner).
//
// CHAMFER (CHA):
//   Pick two lines (line-only; arcs are not chamferable).
//   Finds intersection, backs off dist1 along line 1 and dist2 along line 2.

use codec::entities::{Arc as ArcEnt, Line as LineEnt, LwPolyline, LwVertex};

// Shared plane geometry, from opencadkernel via the local adapters.
use super::geom;
use super::geom::{arc_points as arc_pts, line_line as ll, normalize_angle as norm_angle};
use crate::t;
use codec::entities::EntityCommon;
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::DVec3;
use kernel::geom2d::{
    circle_circle_points as circle_circle_pts, fillet_between_rays, fillets_between, line_circle,
    Arc as KernelArc, Curve as KernelCurve, Fillet as KernelFillet, Line as KernelLine, Tolerance,
};

const TAU: f64 = std::f64::consts::TAU;

/// How close two positions must be to count as one while a fillet is being
/// placed. Loose enough to survive the rounding in a drawing's stored
/// coordinates, tight enough not to merge two distinct candidate centres.
const FILLET_TOLERANCE: f64 = 1.0e-9;

use crate::command::{CadCommand, CmdResult};
use crate::modules::draw::defaults;
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;

use super::entity_index::ModifyEntityIndex;

// ── Dropdown constants ─────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "fillet_chamfer";
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/fillet.svg"));

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    (
        "FILLET",
        "Fillet",
        IconKind::Svg(include_bytes!("../../../../assets/icons/fillet.svg")),
    ),
    (
        "CHAMFER",
        "Chamfer",
        IconKind::Svg(include_bytes!("../../../../assets/icons/chamfer.svg")),
    ),
];

// ══════════════════════════════════════════════════════════════════════════
// Geometry
// ══════════════════════════════════════════════════════════════════════════

/// Extract coords and unit direction for a Line entity.
fn line_geom(l: &LineEnt) -> ([f64; 2], [f64; 2], [f64; 2], f64) {
    let p1 = [l.start.x, l.start.y];
    let p2 = [l.end.x, l.end.y];
    let dx = p2[0] - p1[0];
    let dy = p2[1] - p1[1];
    let len = (dx * dx + dy * dy).sqrt().max(1e-12);
    (p1, p2, [dx / len, dy / len], len)
}

/// Project click onto line, returning t ∈ ℝ.
fn project_click(click: [f64; 2], p1: [f64; 2], unit: [f64; 2]) -> f64 {
    (click[0] - p1[0]) * unit[0] + (click[1] - p1[1]) * unit[1]
}

// ── Fillet ─────────────────────────────────────────────────────────────────
/// Fillet two parallel lines with a semicircle tangent to both.
///
/// The current FILLET radius is intentionally ignored for parallel lines:
/// the effective radius is half the perpendicular distance between them.
///
/// The endpoint of the first line nearest its pick determines where the
/// semicircle is placed. The second line is trimmed or extended to the
/// corresponding perpendicular tangent point.
fn fillet_parallel_lines(
    l1: &LineEnt,
    click1: [f64; 2],
    l2: &LineEnt,
    click2: [f64; 2],
) -> Option<(EntityType, EntityType, Option<EntityType>)> {
    let (p1, p2, u1, len1) = line_geom(l1);
    let (p3, _p4, u2, len2) = line_geom(l2);

    if len1 <= 1.0e-12 || len2 <= 1.0e-12 {
        return None;
    }

    // This helper is only for parallel / anti-parallel lines.
    let cross = u1[0] * u2[1] - u1[1] * u2[0];
    if cross.abs() > 1.0e-6 {
        return None;
    }

    // The pick on the first line determines which end receives the round.
    let dist_sq = |a: [f64; 2], b: [f64; 2]| {
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        dx * dx + dy * dy
    };

    let (tangent1, other1) = if dist_sq(click1, p1) <= dist_sq(click1, p2) {
        (p1, p2)
    } else {
        (p2, p1)
    };

    // Unit normal to line 1. Project tangent1 perpendicularly onto line 2.
    let normal = [-u1[1], u1[0]];
    let separation = (p3[0] - tangent1[0]) * normal[0] + (p3[1] - tangent1[1]) * normal[1];

    if separation.abs() <= 1.0e-9 {
        // Coincident lines do not define a useful semicircle.
        return None;
    }

    let tangent2 = [
        tangent1[0] + normal[0] * separation,
        tangent1[1] + normal[1] * separation,
    ];

    let centre = [
        (tangent1[0] + tangent2[0]) * 0.5,
        (tangent1[1] + tangent2[1]) * 0.5,
    ];

    let radius = separation.abs() * 0.5;

    // Keep the existing part of each selected line and trim/extend the picked
    // end to its tangent point.
    let new_l1 = trim_line_to_point(l1, tangent1, click1)?;
    let new_l2 = trim_line_to_point(l2, tangent2, click2)?;

    // The semicircle must bulge away from the retained portion of line 1.
    // `other1 - tangent1` points back into the line, so negate it.
    let keep = [other1[0] - tangent1[0], other1[1] - tangent1[1]];
    let keep_len = (keep[0] * keep[0] + keep[1] * keep[1]).sqrt();

    if keep_len <= 1.0e-12 {
        return None;
    }

    let bulge_dir = [-keep[0] / keep_len, -keep[1] / keep_len];

    let a1 = norm_angle((tangent1[1] - centre[1]).atan2(tangent1[0] - centre[0]));
    let a2 = norm_angle((tangent2[1] - centre[1]).atan2(tangent2[0] - centre[0]));

    // Between two antipodal points there are two possible semicircles.
    // Determine which CCW orientation bulges toward the selected end.
    let midpoint_angle = a1 + std::f64::consts::FRAC_PI_2;
    let midpoint_dir = [midpoint_angle.cos(), midpoint_angle.sin()];

    let candidate_matches = midpoint_dir[0] * bulge_dir[0] + midpoint_dir[1] * bulge_dir[1] >= 0.0;

    let (start_angle, end_angle) = if candidate_matches {
        (a1, a2)
    } else {
        (a2, a1)
    };

    let mut arc = ArcEnt::new();
    arc.common = l1.common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = Vector3::new(centre[0], centre[1], l1.start.z);
    arc.radius = radius;
    arc.start_angle = start_angle;
    arc.end_angle = end_angle;

    Some((
        EntityType::Line(new_l1),
        EntityType::Line(new_l2),
        Some(EntityType::Arc(arc)),
    ))
}
/// Compute fillet: trim l1/l2 and insert a tangent arc of `radius`.
/// Returns (trimmed_l1, trimmed_l2, fillet_arc).
fn compute_fillet(
    l1: &LineEnt,
    click1: [f64; 2],
    l2: &LineEnt,
    click2: [f64; 2],
    radius: f64,
) -> Option<(EntityType, EntityType, Option<EntityType>)> {
    let (p1, p2, u1, _len1) = line_geom(l1);
    let (p3, p4, u2, _len2) = line_geom(l2);

    // Parallel lines are a special FILLET case: there is no intersection.
    // Build the tangent semicircle directly, temporarily ignoring the
    // configured radius.
    let cross = u1[0] * u2[1] - u1[1] * u2[0];

    if cross.abs() <= 1.0e-6 {
        return fillet_parallel_lines(l1, click1, l2, click2);
    }

    // Intersection of infinite non-parallel lines.
    let (t_p, _u_p) = ll(p1[0], p1[1], u1[0], u1[1], p3[0], p3[1], u2[0], u2[1])?;

    // Intersection point
    let px = p1[0] + t_p * u1[0];
    let py = p1[1] + t_p * u1[1];

    // Direction from P toward each click (the "keep" side)
    let picked_side = |click: [f64; 2], a: [f64; 2], b: [f64; 2], unit: [f64; 2]| {
        let picked = project_click(click, [px, py], unit);
        if picked.abs() > 1e-9 {
            picked
        } else {
            // Clicking exactly on the intersection is common when the two
            // entities already meet. Fall back to the existing segment's
            // midpoint so entity start/end storage order cannot flip the side
            // FILLET keeps (#616).
            project_click([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5], [px, py], unit)
        }
    };
    let s1 = picked_side(click1, p1, p2, u1); // positive = along u1
    let s2 = picked_side(click2, p3, p4, u2);
    let dir1 = if s1 >= 0.0 {
        [u1[0], u1[1]]
    } else {
        [-u1[0], -u1[1]]
    };
    let dir2 = if s2 >= 0.0 {
        [u2[0], u2[1]]
    } else {
        [-u2[0], -u2[1]]
    };

    let z = l1.start.z;

    if radius < 1e-9 {
        // r = 0: extend/trim the endpoint opposite each clicked keep-side.
        // The old path chose by the sign of t_p/u_p, so reversing an entity's
        // stored direction changed the result and an intersection at its start
        // could collapse the whole line (#616).
        let intersection = [px, py];
        let new_l1 = trim_to_xy(l1, intersection, dir1, u1)?;
        let new_l2 = trim_to_xy(l2, intersection, dir2, u2)?;
        return Some((EntityType::Line(new_l1), EntityType::Line(new_l2), None));
    }

    let rounded = fillet_between_rays([px, py], dir1, dir2, radius)?;

    // Trim each line back to where the arc meets it.
    let new_l1 = trim_to_xy(l1, rounded.tangent1, dir1, u1)?;
    let new_l2 = trim_to_xy(l2, rounded.tangent2, dir2, u2)?;

    let mut arc = ArcEnt::new();
    arc.common = l1.common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = Vector3::new(rounded.centre[0], rounded.centre[1], z);
    arc.radius = radius;
    arc.start_angle = rounded.start_angle;
    arc.end_angle = rounded.end_angle;

    Some((
        EntityType::Line(new_l1),
        EntityType::Line(new_l2),
        Some(EntityType::Arc(arc)),
    ))
}

/// Move the endpoint opposite the selected keep-side to the tangent point.
fn trim_to_xy(orig: &LineEnt, tangent: [f64; 2], dir: [f64; 2], unit: [f64; 2]) -> Option<LineEnt> {
    let z = orig.start.z;
    let mut l = orig.clone();
    l.common.handle = Handle::NULL;

    // dir is positive along unit → keep the portion beyond the tangent in that direction
    // dir positive: keep from t_tan to +∞ (i.e. set start to tangent point)
    // dir negative: keep from -∞ to t_tan (i.e. set end to tangent point)
    let dot = dir[0] * unit[0] + dir[1] * unit[1]; // +1 or -1

    if dot > 0.0 {
        // keep from tangent to end → move start to tangent point
        l.start = Vector3::new(tangent[0], tangent[1], z);
    } else {
        // keep from start to tangent → move end to tangent point
        l.end = Vector3::new(tangent[0], tangent[1], z);
    }
    Some(l)
}

// ── Point-generation helpers ──────────────────────────────────────────────

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
        EntityType::LwPolyline(p) => lwpoly_pts(p),
        _ => vec![],
    }
}

// ── Arc geometry helpers ───────────────────────────────────────────────────

/// Extract center, radius, start/end angle (radians), elevation from an arc.
fn arc_geom(a: &ArcEnt) -> ([f64; 2], f64, f64, f64, f64) {
    (
        [a.center.x, a.center.y],
        a.radius,
        a.start_angle,
        a.end_angle,
        a.center.z,
    )
}

/// Return the CCW angular span from `start` to `end`.
fn arc_span(start: f64, end: f64) -> f64 {
    let s = (end - start).rem_euclid(TAU);
    if s < 1e-6 {
        TAU
    } else {
        s
    }
}

/// Project a pick point onto an arc: return the angle in radians.
fn arc_angle_at(center: [f64; 2], pt: [f64; 2]) -> f64 {
    norm_angle((pt[1] - center[1]).atan2(pt[0] - center[0]))
}

/// Clamp angle `a` into the arc range (CCW from `start` to `end`).
/// Returns the nearer endpoint if `a` is outside.
fn clamp_angle_to_arc(a: f64, start: f64, end: f64) -> f64 {
    let span = arc_span(start, end);
    let rel = (a - start).rem_euclid(TAU);
    if rel <= span {
        a
    } else if rel < span + (TAU - span) / 2.0 {
        end
    } else {
        start
    }
}

/// The arc cut back or extended to `angle` on its circle. An angle on the
/// drawn part keeps the side of `pick` (an angle too); one beyond it
/// extends whichever end is nearer, as a corner or fillet meets it there.
fn arc_to_angle(arc: &ArcEnt, angle: f64, pick: f64) -> ArcEnt {
    let (start, end) = (arc.start_angle, arc.end_angle);
    let span = arc_span(start, end);
    let rel = (angle - start).rem_euclid(TAU);
    if rel <= span {
        let pick_rel = (clamp_angle_to_arc(pick, start, end) - start).rem_euclid(TAU);
        return if rel <= pick_rel {
            trim_arc(arc, angle, end)
        } else {
            trim_arc(arc, start, angle)
        };
    }
    if rel - span <= TAU - rel {
        trim_arc(arc, start, angle)
    } else {
        trim_arc(arc, angle, end)
    }
}

/// Trim an arc so it goes from `new_start` to `new_end` (both in radians).
fn trim_arc(orig: &ArcEnt, new_start: f64, new_end: f64) -> ArcEnt {
    let mut a = orig.clone();
    a.common.handle = Handle::NULL;
    a.start_angle = norm_angle(new_start);
    a.end_angle = norm_angle(new_end);
    a
}

// ── LwPolyline helpers ────────────────────────────────────────────────────

/// Find the index of the LwPolyline segment nearest to `click` (DXF XY).
fn lwpoly_nearest_seg(poly: &LwPolyline, click: [f64; 2]) -> usize {
    let n = poly.vertices.len();
    let seg_count = if poly.is_closed {
        n
    } else {
        n.saturating_sub(1)
    };
    let mut best_idx = 0;
    let mut best_dist = f64::MAX;
    for i in 0..seg_count {
        let v0 = &poly.vertices[i];
        let v1 = &poly.vertices[(i + 1) % n];
        let px = v0.location.x;
        let py = v0.location.y;
        let dx = v1.location.x - px;
        let dy = v1.location.y - py;
        let len2 = dx * dx + dy * dy;
        let t = if len2 < 1e-24 {
            0.0
        } else {
            ((click[0] - px) * dx + (click[1] - py) * dy) / len2
        }
        .clamp(0.0, 1.0);
        let cx = px + t * dx - click[0];
        let cy = py + t * dy - click[1];
        let dist = cx * cx + cy * cy;
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
        }
    }
    best_idx
}

/// A polyline's bulged segment as the counter-clockwise arc it draws, and
/// whether the polyline runs it clockwise (a negative bulge): then the arc
/// starts at the segment's end vertex.
fn lwpoly_seg_as_arc(poly: &LwPolyline, seg_idx: usize) -> Option<(ArcEnt, bool)> {
    let n = poly.vertices.len();
    let v0 = poly.vertices.get(seg_idx)?;
    let v1 = poly.vertices.get((seg_idx + 1) % n)?;
    let bulge = crate::entities::common::BulgeArc::from_bulge(
        [v0.location.x, v0.location.y],
        [v1.location.x, v1.location.y],
        v0.bulge,
    )?;
    let reversed = bulge.sweep < 0.0;
    let mut arc = ArcEnt::new();
    arc.common = poly.common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = Vector3::new(bulge.center[0], bulge.center[1], poly.elevation);
    arc.radius = bulge.radius;
    let (start, end) = if reversed {
        (bulge.end_angle, bulge.start_angle)
    } else {
        (bulge.start_angle, bulge.end_angle)
    };
    arc.start_angle = norm_angle(start);
    arc.end_angle = norm_angle(end);
    Some((arc, reversed))
}

/// The polyline with its bulged segment `seg_idx` replaced by `arc`, the
/// segment's arc as the fillet cut it back or extended it: both ends of the
/// segment go to the arc's ends, the bulge follows its new sweep.
fn rebuild_poly_from_trimmed_arc(
    poly: &LwPolyline,
    seg_idx: usize,
    reversed: bool,
    arc: &ArcEnt,
) -> LwPolyline {
    let n = poly.vertices.len();
    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    let at = |angle: f64| {
        (
            arc.center.x + arc.radius * angle.cos(),
            arc.center.y + arc.radius * angle.sin(),
        )
    };
    let (first, last) = if reversed {
        (at(arc.end_angle), at(arc.start_angle))
    } else {
        (at(arc.start_angle), at(arc.end_angle))
    };
    let sweep = arc_span(arc.start_angle, arc.end_angle);
    let vertex = &mut new_poly.vertices[seg_idx];
    vertex.location.x = first.0;
    vertex.location.y = first.1;
    vertex.bulge = (sweep / 4.0).tan() * if reversed { -1.0 } else { 1.0 };
    let end = &mut new_poly.vertices[(seg_idx + 1) % n];
    end.location.x = last.0;
    end.location.y = last.1;
    new_poly
}

/// Extract a LwPolyline segment as a virtual `LineEnt` (ignores bulge).
fn lwpoly_seg_as_line(poly: &LwPolyline, seg_idx: usize) -> LineEnt {
    let n = poly.vertices.len();
    let v0 = &poly.vertices[seg_idx];
    let v1 = &poly.vertices[(seg_idx + 1) % n];
    let mut l = LineEnt::new();
    l.common = poly.common.clone();
    l.common.handle = Handle::NULL;
    l.start = Vector3::new(v0.location.x, v0.location.y, poly.elevation);
    l.end = Vector3::new(v1.location.x, v1.location.y, poly.elevation);
    l
}

/// Compute the LwPolyline bulge for the arc from T1 to T2 whose center is `arc_center`.
/// Positive = CCW, negative = CW.
fn compute_bulge(t1: [f64; 2], t2: [f64; 2], arc_center: [f64; 2]) -> f64 {
    let a1 = (t1[1] - arc_center[1]).atan2(t1[0] - arc_center[0]);
    let a2 = (t2[1] - arc_center[1]).atan2(t2[0] - arc_center[0]);
    // CCW angular span from T1 to T2
    let span_ccw = ((a2 - a1) % TAU + TAU) % TAU;
    // Determine whether the fillet arc goes CCW (center to left of T1→T2) or CW.
    let chord = [t2[0] - t1[0], t2[1] - t1[1]];
    let mid = [(t1[0] + t2[0]) * 0.5, (t1[1] + t2[1]) * 0.5];
    let to_c = [arc_center[0] - mid[0], arc_center[1] - mid[1]];
    let cross = chord[0] * to_c[1] - chord[1] * to_c[0];
    if cross >= 0.0 {
        (span_ccw / 4.0).tan() // CCW arc
    } else {
        let span_cw = TAU - span_ccw;
        -(span_cw / 4.0).tan() // CW arc
    }
}
#[derive(Clone, Copy)]
struct PolylineCornerFillet {
    incoming: [f64; 2],
    outgoing: [f64; 2],
    bulge: f64,
}

fn segment_parameter(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> Option<f64> {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len2 = dx * dx + dy * dy;

    if len2 <= 1.0e-20 {
        return None;
    }

    Some(((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / len2)
}

/// Apply the current FILLET radius to every eligible corner of a 2D polyline.
///
/// Initial scope deliberately handles straight-segment corners only. Corners
/// adjacent to an existing bulge arc are preserved unchanged.
fn fillet_entire_lwpolyline(poly: &LwPolyline, radius: f64) -> Option<LwPolyline> {
    let n = poly.vertices.len();

    if n < 3 || radius <= 1.0e-9 {
        return None;
    }

    let mut corners: Vec<Option<PolylineCornerFillet>> = vec![None; n];

    let first_corner = if poly.is_closed { 0 } else { 1 };
    let end_corner = if poly.is_closed {
        n
    } else {
        n.saturating_sub(1)
    };

    for i in first_corner..end_corner {
        let prev = if i == 0 { n - 1 } else { i - 1 };

        // A bulge belongs to the segment starting at that vertex.
        // For now only fillet corners where both adjacent segments are straight.
        if poly.vertices[prev].bulge.abs() >= 1.0e-9 || poly.vertices[i].bulge.abs() >= 1.0e-9 {
            continue;
        }

        let incoming = lwpoly_seg_as_line(poly, prev);
        let outgoing = lwpoly_seg_as_line(poly, i);

        // Pick points halfway along each segment, away from the corner.
        // This tells the existing line-line FILLET code which side to keep.
        let click_in = [
            (incoming.start.x + incoming.end.x) * 0.5,
            (incoming.start.y + incoming.end.y) * 0.5,
        ];
        let click_out = [
            (outgoing.start.x + outgoing.end.x) * 0.5,
            (outgoing.start.y + outgoing.end.y) * 0.5,
        ];

        let Some((trimmed_in, trimmed_out, maybe_arc)) =
            compute_fillet(&incoming, click_in, &outgoing, click_out, radius)
        else {
            continue;
        };

        let (
            EntityType::Line(trimmed_in),
            EntityType::Line(trimmed_out),
            Some(EntityType::Arc(arc)),
        ) = (trimmed_in, trimmed_out, maybe_arc)
        else {
            continue;
        };

        let tangent_in = [trimmed_in.end.x, trimmed_in.end.y];
        let tangent_out = [trimmed_out.start.x, trimmed_out.start.y];

        let in_start = [incoming.start.x, incoming.start.y];
        let in_end = [incoming.end.x, incoming.end.y];
        let out_start = [outgoing.start.x, outgoing.start.y];
        let out_end = [outgoing.end.x, outgoing.end.y];

        let Some(t_in) = segment_parameter(tangent_in, in_start, in_end) else {
            continue;
        };
        let Some(t_out) = segment_parameter(tangent_out, out_start, out_end) else {
            continue;
        };

        // Whole-polyline FILLET should not extend past neighbouring vertices.
        // If this radius cannot fit at this corner, leave that corner unchanged.
        if !(-1.0e-9..=1.0 + 1.0e-9).contains(&t_in) || !(-1.0e-9..=1.0 + 1.0e-9).contains(&t_out) {
            continue;
        }

        corners[i] = Some(PolylineCornerFillet {
            incoming: tangent_in,
            outgoing: tangent_out,
            bulge: compute_bulge(tangent_in, tangent_out, [arc.center.x, arc.center.y]),
        });
    }

    if corners.iter().all(Option::is_none) {
        return None;
    }

    // Adjacent fillets must not consume more than the shared segment.
    // Reject the operation rather than producing an inverted/overlapping edge.
    let segment_count = if poly.is_closed { n } else { n - 1 };

    for i in 0..segment_count {
        let next = (i + 1) % n;

        let start = corners[i]
            .map(|corner| corner.outgoing)
            .unwrap_or([poly.vertices[i].location.x, poly.vertices[i].location.y]);

        let end = corners[next].map(|corner| corner.incoming).unwrap_or([
            poly.vertices[next].location.x,
            poly.vertices[next].location.y,
        ]);

        let original_start = [poly.vertices[i].location.x, poly.vertices[i].location.y];
        let original_end = [
            poly.vertices[next].location.x,
            poly.vertices[next].location.y,
        ];

        let Some(start_t) = segment_parameter(start, original_start, original_end) else {
            continue;
        };
        let Some(end_t) = segment_parameter(end, original_start, original_end) else {
            continue;
        };

        if start_t > end_t + 1.0e-9 {
            return None;
        }
    }

    let mut result = poly.clone();
    result.common.handle = Handle::NULL;
    result.vertices.clear();

    for (i, original) in poly.vertices.iter().enumerate() {
        if let Some(corner) = corners[i] {
            let mut tangent_in = original.clone();
            tangent_in.location.x = corner.incoming[0];
            tangent_in.location.y = corner.incoming[1];
            tangent_in.bulge = corner.bulge;

            let mut tangent_out = original.clone();
            tangent_out.location.x = corner.outgoing[0];
            tangent_out.location.y = corner.outgoing[1];

            // This corner is only eligible when its outgoing segment is straight.
            tangent_out.bulge = 0.0;

            result.vertices.push(tangent_in);
            result.vertices.push(tangent_out);
        } else {
            result.vertices.push(original.clone());
        }
    }

    Some(result)
}
/// Rebuild a LwPolyline, replacing the corner vertex at `corner_idx` with two
/// new vertices T1 (start of fillet arc) and T2 (end of fillet arc).
/// `bulge` encodes the direction and span of the arc (T1 → T2).
fn lwpoly_replace_corner(
    poly: &LwPolyline,
    corner_idx: usize,
    t1: [f64; 2],
    t2: [f64; 2],
    bulge: f64,
) -> LwPolyline {
    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    let orig = poly.vertices[corner_idx].clone();
    let mut vt1 = orig.clone();
    vt1.location.x = t1[0];
    vt1.location.y = t1[1];
    vt1.bulge = bulge;
    let mut vt2 = orig;
    vt2.location.x = t2[0];
    vt2.location.y = t2[1];
    vt2.bulge = 0.0;
    new_poly.vertices.remove(corner_idx);
    new_poly.vertices.insert(corner_idx, vt2);
    new_poly.vertices.insert(corner_idx, vt1);
    new_poly
}

/// Rebuild a LwPolyline after shortening segment `seg_idx`:
/// `new_start` / `new_end` replace the segment's start/end vertex if `Some`.
fn lwpoly_shorten_seg(
    poly: &LwPolyline,
    seg_idx: usize,
    new_start: Option<[f64; 2]>,
    new_end: Option<[f64; 2]>,
) -> LwPolyline {
    let n = poly.vertices.len();
    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    if let Some([x, y]) = new_start {
        new_poly.vertices[seg_idx].location.x = x;
        new_poly.vertices[seg_idx].location.y = y;
        new_poly.vertices[seg_idx].bulge = 0.0;
    }
    if let Some([x, y]) = new_end {
        let end_idx = (seg_idx + 1) % n;
        new_poly.vertices[end_idx].location.x = x;
        new_poly.vertices[end_idx].location.y = y;
    }
    new_poly
}

/// Wire points for a LwPolyline (honours bulge → arcs, for preview/highlight).
fn lwpoly_pts(poly: &LwPolyline) -> Vec<[f32; 3]> {
    let elev = poly.elevation as f32;
    let n = poly.vertices.len();
    let seg_count = if poly.is_closed {
        n
    } else {
        n.saturating_sub(1)
    };
    let mut pts = Vec::with_capacity(seg_count * 2);
    for i in 0..seg_count {
        let v0 = &poly.vertices[i];
        let v1 = &poly.vertices[(i + 1) % n];
        lwpoly_seg_pts(
            [v0.location.x, v0.location.y],
            [v1.location.x, v1.location.y],
            v0.bulge,
            elev,
            &mut pts,
        );
    }
    pts
}

/// Wire points highlighting the polyline segment nearest to `click`.
fn lwpoly_seg_hover_pts(poly: &LwPolyline, click: [f64; 2]) -> Vec<[f32; 3]> {
    let n = poly.vertices.len();
    if n < 2 {
        return vec![];
    }
    let seg = lwpoly_nearest_seg(poly, click);
    let v0 = &poly.vertices[seg];
    let v1 = &poly.vertices[(seg + 1) % n];
    let mut pts = Vec::new();
    lwpoly_seg_pts(
        [v0.location.x, v0.location.y],
        [v1.location.x, v1.location.y],
        v0.bulge,
        poly.elevation as f32,
        &mut pts,
    );
    pts
}

/// Append the wire points of one polyline segment (straight, or a bulge arc).
fn lwpoly_seg_pts(p0: [f64; 2], p1: [f64; 2], bulge: f64, elev: f32, out: &mut Vec<[f32; 3]>) {
    if let Some(arc) = (bulge.abs() >= 1e-9)
        .then(|| crate::entities::common::BulgeArc::from_bulge(p0, p1, bulge))
        .flatten()
    {
        const STEPS: usize = 16;
        for j in 0..STEPS {
            let a = arc.sample(j as f64 / STEPS as f64);
            let b = arc.sample((j + 1) as f64 / STEPS as f64);
            out.push([a[0] as f32, a[1] as f32, elev]);
            out.push([b[0] as f32, b[1] as f32, elev]);
        }
    } else {
        out.push([p0[0] as f32, p0[1] as f32, elev]);
        out.push([p1[0] as f32, p1[1] as f32, elev]);
    }
}

// ── Unified fillet entity type ─────────────────────────────────────────────

/// A pickable entity for FILLET: Line, Arc, or an LwPolyline segment.
#[derive(Clone)]
enum FilletEntity {
    Line(LineEnt),
    Arc(ArcEnt),
    /// A segment of an LwPolyline identified by its entity handle and segment index.
    LwPoly {
        poly: LwPolyline,
        handle: Handle,
        seg_idx: usize,
    },
    /// A circle, ellipse or spline: filleted through the kernel's general
    /// construction, and trimmed only when it has ends to trim.
    Curve(EntityType),
}

impl FilletEntity {
    fn from_entity(e: &EntityType) -> Option<Self> {
        match e {
            EntityType::Line(l) => Some(Self::Line(l.clone())),
            EntityType::Arc(a) => Some(Self::Arc(a.clone())),
            EntityType::Circle(_) | EntityType::Ellipse(_) | EntityType::Spline(_) => {
                crate::entities::curve::entity_curve_xy(e).map(|_| Self::Curve(e.clone()))
            }
            _ => None,
        }
    }

    fn from_lwpoly(poly: &LwPolyline, handle: Handle, click: [f64; 2]) -> Self {
        let seg_idx = lwpoly_nearest_seg(poly, click);
        Self::LwPoly {
            poly: poly.clone(),
            handle,
            seg_idx,
        }
    }

    fn to_entity_type(&self) -> EntityType {
        match self {
            Self::Line(l) => EntityType::Line(l.clone()),
            Self::Arc(a) => EntityType::Arc(a.clone()),
            Self::LwPoly { poly, .. } => EntityType::LwPolyline(poly.clone()),
            Self::Curve(entity) => entity.clone(),
        }
    }

    fn elevation(&self) -> f64 {
        match self {
            Self::Line(l) => l.start.z,
            Self::Arc(a) => a.center.z,
            Self::LwPoly { poly, .. } => poly.elevation,
            Self::Curve(EntityType::Circle(c)) => c.center.z,
            Self::Curve(EntityType::Ellipse(e)) => e.center.z,
            Self::Curve(EntityType::Spline(s)) => s.control_points.first().map_or(0.0, |p| p.z),
            Self::Curve(_) => 0.0,
        }
    }

    /// The shape as a kernel curve in plan.
    fn kernel_curve(&self) -> Option<KernelCurve> {
        match self {
            Self::Line(l) => Some(line_curve(l)),
            Self::Arc(a) => Some(arc_curve(a)),
            Self::LwPoly { poly, seg_idx, .. } => {
                Some(line_curve(&lwpoly_seg_as_line(poly, *seg_idx)))
            }
            Self::Curve(entity) => crate::entities::curve::entity_curve_xy(entity),
        }
    }
}

/// Compute FILLET between two entities (Line, Arc, or LwPolyline segment).
/// Returns (trimmed_e1, trimmed_e2, optional_fillet_arc).
/// For same-poly corner fillet the two returned entities are identical (the rebuilt poly).
fn compute_fillet_entities(
    e1: &FilletEntity,
    click1: [f64; 2],
    e2: &FilletEntity,
    click2: [f64; 2],
    radius: f64,
) -> Option<(EntityType, EntityType, Option<EntityType>)> {
    let z = e1.elevation();
    // A polyline's arc segment fillets as the arc it draws; the cut-back arc
    // then goes back into the polyline as its end point and bulge.
    fn arc_segment(entity: &FilletEntity) -> Option<(&LwPolyline, usize, ArcEnt, bool)> {
        match entity {
            FilletEntity::LwPoly { poly, seg_idx, .. }
                if poly.vertices.get(*seg_idx).is_some_and(|v| v.bulge.abs() > 1e-12) =>
            {
                let (arc, reversed) = lwpoly_seg_as_arc(poly, *seg_idx)?;
                Some((poly, *seg_idx, arc, reversed))
            }
            _ => None,
        }
    }
    let (s1, s2) = (arc_segment(e1), arc_segment(e2));
    if s1.is_some() || s2.is_some() {
        // Two segments of one polyline round their shared corner elsewhere.
        if let (
            FilletEntity::LwPoly { handle: h1, .. },
            FilletEntity::LwPoly { handle: h2, .. },
        ) = (e1, e2)
        {
            if h1 == h2 {
                return None;
            }
        }
        let as_arc = |segment: &Option<(&LwPolyline, usize, ArcEnt, bool)>| {
            segment.as_ref().map(|(_, _, arc, _)| FilletEntity::Arc(arc.clone()))
        };
        let (a1, a2) = (as_arc(&s1), as_arc(&s2));
        let (r1, r2, fillet) = compute_fillet_entities(
            a1.as_ref().unwrap_or(e1),
            click1,
            a2.as_ref().unwrap_or(e2),
            click2,
            radius,
        )?;
        let back = |segment: &Option<(&LwPolyline, usize, ArcEnt, bool)>, result: EntityType| {
            match (segment, result) {
                (Some((poly, seg_idx, _, reversed)), EntityType::Arc(arc)) => Some(
                    EntityType::LwPolyline(rebuild_poly_from_trimmed_arc(
                        poly, *seg_idx, *reversed, &arc,
                    )),
                ),
                (None, result) => Some(result),
                _ => None,
            }
        };
        return Some((back(&s1, r1)?, back(&s2, r2)?, fillet));
    }
    if matches!(e1, FilletEntity::Curve(_)) || matches!(e2, FilletEntity::Curve(_)) {
        return fillet_curves(e1, click1, e2, click2, radius, z);
    }

    match (e1, e2) {
        // ── Line × Line ───────────────────────────────────────────────────
        (FilletEntity::Line(l1), FilletEntity::Line(l2)) => {
            compute_fillet(l1, click1, l2, click2, radius)
        }
        // ── Line × Arc ────────────────────────────────────────────────────
        (FilletEntity::Line(l), FilletEntity::Arc(a)) => {
            fillet_line_arc(l, click1, a, click2, radius, z)
        }
        (FilletEntity::Arc(a), FilletEntity::Line(l)) => {
            fillet_line_arc(l, click2, a, click1, radius, z)
                .map(|(new_l, new_a, arc)| (new_a, new_l, arc))
        }
        // ── Arc × Arc ─────────────────────────────────────────────────────
        (FilletEntity::Arc(a1), FilletEntity::Arc(a2)) => {
            fillet_arc_arc(a1, click1, a2, click2, radius, z)
        }
        // ── LwPoly × LwPoly (same entity — corner fillet) ─────────────────
        (
            FilletEntity::LwPoly {
                poly: p1,
                handle: h1,
                seg_idx: s1,
            },
            FilletEntity::LwPoly {
                handle: h2,
                seg_idx: s2,
                ..
            },
        ) if h1 == h2 => {
            let n = p1.vertices.len();
            let closed = p1.is_closed;
            let seg_count = if closed { n } else { n.saturating_sub(1) };
            let next = |seg: usize| if closed { (seg + 1) % n } else { seg + 1 };
            let straight = |seg: usize| seg < seg_count && p1.vertices[seg].bulge.abs() < 1e-12;
            // `before` runs into the corner and `after` leaves it. Between them
            // sits nothing (a sharp corner), an old fillet arc being replaced,
            // or — on an open polyline — the gap between its two ends, which
            // the fillet closes. Arc segments themselves are never filleted:
            // treating one as its chord bent the result across the shape.
            enum Between {
                Corner,
                Arc(usize),
                OpenEnds,
            }
            let order = |a: usize, b: usize| {
                if next(a) == b {
                    Some((a, b, Between::Corner))
                } else if next(a) < seg_count && next(next(a)) == b && !straight(next(a)) {
                    Some((a, b, Between::Arc(next(a))))
                } else if !closed && seg_count >= 2 && a == seg_count - 1 && b == 0 {
                    Some((a, b, Between::OpenEnds))
                } else {
                    None
                }
            };
            let (before_seg, after_seg, between) = order(*s1, *s2).or_else(|| order(*s2, *s1))?;
            if !straight(before_seg) || !straight(after_seg) {
                return None;
            }
            let l1 = lwpoly_seg_as_line(p1, before_seg);
            let l2 = lwpoly_seg_as_line(p1, after_seg);
            // Re-map click to whichever segment each was picked on.
            let (c1, c2) = if *s1 == before_seg {
                (click1, click2)
            } else {
                (click2, click1)
            };
            let (EntityType::Line(tl1), EntityType::Line(tl2), maybe_arc) =
                compute_fillet(&l1, c1, &l2, c2, radius)?
            else {
                return None;
            };
            let t1 = [tl1.end.x, tl1.end.y]; // trimmed end of seg before corner
            let t2 = [tl2.start.x, tl2.start.y]; // trimmed start of seg after corner
            let bulge = match maybe_arc {
                Some(EntityType::Arc(ref fa)) => compute_bulge(t1, t2, [fa.center.x, fa.center.y]),
                _ => 0.0, // r=0, sharp corner
            };
            let new_poly = match between {
                Between::Corner => lwpoly_replace_corner(p1, after_seg, t1, t2, bulge),
                // The old arc runs from its own vertex to the `after` vertex:
                // move both onto the new tangent points.
                Between::Arc(arc_seg) => {
                    let mut poly = p1.clone();
                    poly.common.handle = Handle::NULL;
                    poly.vertices[arc_seg].location.x = t1[0];
                    poly.vertices[arc_seg].location.y = t1[1];
                    poly.vertices[arc_seg].bulge = bulge;
                    poly.vertices[after_seg].location.x = t2[0];
                    poly.vertices[after_seg].location.y = t2[1];
                    poly
                }
                // The last vertex becomes the start of the fillet and the
                // closing segment is the fillet itself.
                Between::OpenEnds => {
                    let mut poly = p1.clone();
                    poly.common.handle = Handle::NULL;
                    let last = n - 1;
                    poly.vertices[last].location.x = t1[0];
                    poly.vertices[last].location.y = t1[1];
                    poly.vertices[last].bulge = bulge;
                    poly.vertices[0].location.x = t2[0];
                    poly.vertices[0].location.y = t2[1];
                    poly.is_closed = true;
                    poly
                }
            };
            let et = EntityType::LwPolyline(new_poly);
            // Return same rebuilt poly for both slots; caller uses only h1.
            Some((et.clone(), et, None))
        }
        // ── LwPoly × LwPoly (different entities) ──────────────────────────
        (
            FilletEntity::LwPoly {
                poly: p1,
                seg_idx: s1,
                ..
            },
            FilletEntity::LwPoly {
                poly: p2,
                seg_idx: s2,
                ..
            },
        ) => {
            let l1 = lwpoly_seg_as_line(p1, *s1);
            let l2 = lwpoly_seg_as_line(p2, *s2);
            let (tl1_e, tl2_e, maybe_arc) = compute_fillet(&l1, click1, &l2, click2, radius)?;
            if let (EntityType::Line(tl1), EntityType::Line(tl2)) = (&tl1_e, &tl2_e) {
                let np1 = rebuild_poly_from_trimmed_line(p1, *s1, &l1, tl1);
                let np2 = rebuild_poly_from_trimmed_line(p2, *s2, &l2, tl2);
                Some((
                    EntityType::LwPolyline(np1),
                    EntityType::LwPolyline(np2),
                    maybe_arc,
                ))
            } else {
                None
            }
        }
        // ── LwPoly × Line ─────────────────────────────────────────────────
        (FilletEntity::LwPoly { poly, seg_idx, .. }, FilletEntity::Line(l2)) => {
            let l1 = lwpoly_seg_as_line(poly, *seg_idx);
            let (tl1_e, new_l2, maybe_arc) = compute_fillet(&l1, click1, l2, click2, radius)?;
            if let EntityType::Line(tl1) = &tl1_e {
                let np = rebuild_poly_from_trimmed_line(poly, *seg_idx, &l1, tl1);
                Some((EntityType::LwPolyline(np), new_l2, maybe_arc))
            } else {
                None
            }
        }
        (FilletEntity::Line(l1), FilletEntity::LwPoly { poly, seg_idx, .. }) => {
            let l2 = lwpoly_seg_as_line(poly, *seg_idx);
            let (new_l1, tl2_e, maybe_arc) = compute_fillet(l1, click1, &l2, click2, radius)?;
            if let EntityType::Line(tl2) = &tl2_e {
                let np = rebuild_poly_from_trimmed_line(poly, *seg_idx, &l2, tl2);
                Some((new_l1, EntityType::LwPolyline(np), maybe_arc))
            } else {
                None
            }
        }
        // ── LwPoly × Arc ──────────────────────────────────────────────────
        (FilletEntity::LwPoly { poly, seg_idx, .. }, FilletEntity::Arc(a)) => {
            let l = lwpoly_seg_as_line(poly, *seg_idx);
            let (tl_e, new_a, maybe_arc) = fillet_line_arc(&l, click1, a, click2, radius, z)?;
            if let EntityType::Line(tl) = &tl_e {
                let np = rebuild_poly_from_trimmed_line(poly, *seg_idx, &l, tl);
                Some((EntityType::LwPolyline(np), new_a, maybe_arc))
            } else {
                None
            }
        }
        (FilletEntity::Arc(a), FilletEntity::LwPoly { poly, seg_idx, .. }) => {
            let l = lwpoly_seg_as_line(poly, *seg_idx);
            let (tl_e, new_a, maybe_arc) = fillet_line_arc(&l, click2, a, click1, radius, z)?;
            if let EntityType::Line(tl) = &tl_e {
                let np = rebuild_poly_from_trimmed_line(poly, *seg_idx, &l, tl);
                Some((new_a, EntityType::LwPolyline(np), maybe_arc))
            } else {
                None
            }
        }
        // Answered by `fillet_curves` above.
        (FilletEntity::Curve(_), _) | (_, FilletEntity::Curve(_)) => None,
    }
}

/// Rebuild an LwPolyline after a segment was trimmed.
/// Detects which endpoint of the original line moved and updates the vertex accordingly.
fn rebuild_poly_from_trimmed_line(
    poly: &LwPolyline,
    seg_idx: usize,
    orig: &LineEnt,
    trimmed: &LineEnt,
) -> LwPolyline {
    let start_moved = (trimmed.start.x - orig.start.x).hypot(trimmed.start.y - orig.start.y) > 1e-9;
    let end_moved = (trimmed.end.x - orig.end.x).hypot(trimmed.end.y - orig.end.y) > 1e-9;
    let new_start = if start_moved {
        Some([trimmed.start.x, trimmed.start.y])
    } else {
        None
    };
    let new_end = if end_moved {
        Some([trimmed.end.x, trimmed.end.y])
    } else {
        None
    };
    lwpoly_shorten_seg(poly, seg_idx, new_start, new_end)
}

/// Open (if needed) and cut a polyline at a fillet with a line or arc, then
/// join the fillet arc and the trimmed object onto the cut end, as the
/// reference does: the polyline keeps the part on the picked side of the
/// segment, everything past the fillet point is dropped, and the other object
/// becomes part of the polyline. `moved` is the polyline as the plain fillet
/// rebuilt it, with the segment's moved end at the tangent point. `None` when
/// the pieces cannot be joined (curved segment, out-of-plane arc).
fn join_poly_fillet(
    poly: &LwPolyline,
    seg_idx: usize,
    moved: &LwPolyline,
    other: &EntityType,
    other_seg: Option<(&LwPolyline, usize)>,
    fillet: Option<&EntityType>,
) -> Option<LwPolyline> {
    let n = poly.vertices.len();
    if n < 2 || moved.vertices.len() != n || poly.vertices[seg_idx].bulge.abs() > 1e-12 {
        return None;
    }
    let xy = |v: &LwVertex| [v.location.x, v.location.y];
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9;
    let end_idx = (seg_idx + 1) % n;
    let start_moved = !near(xy(&poly.vertices[seg_idx]), xy(&moved.vertices[seg_idx]));
    let end_moved = !near(xy(&poly.vertices[end_idx]), xy(&moved.vertices[end_idx]));
    if start_moved == end_moved {
        return None;
    }
    let t = if start_moved {
        xy(&moved.vertices[seg_idx])
    } else {
        xy(&moved.vertices[end_idx])
    };

    // The fillet arc runs from T to T2 on the other object.
    let (t2, fillet_center) = match fillet {
        Some(EntityType::Arc(a)) if a.normal.z > 0.0 => {
            let (c, r, sa, ea, _) = arc_geom(a);
            let p0 = [c[0] + r * sa.cos(), c[1] + r * sa.sin()];
            let p1 = [c[0] + r * ea.cos(), c[1] + r * ea.sin()];
            let dist = |p: [f64; 2]| (p[0] - t[0]).hypot(p[1] - t[1]);
            (if dist(p0) > dist(p1) { p0 } else { p1 }, Some(c))
        }
        Some(_) => return None,
        None => (t, None),
    };

    // The other object from T2 to its far end: the bulge of its first
    // segment and the vertices after T2, each with the bulge of the segment
    // that leaves it, in that direction.
    let (first_bulge, chain): (f64, Vec<([f64; 2], f64)>) = match other {
        EntityType::Line(l) => {
            let a = [l.start.x, l.start.y];
            let b = [l.end.x, l.end.y];
            let dist = |p: [f64; 2]| (p[0] - t2[0]).hypot(p[1] - t2[1]);
            (0.0, vec![(if dist(a) > dist(b) { a } else { b }, 0.0)])
        }
        EntityType::Arc(a) if a.normal.z > 0.0 => {
            let (c, r, sa, ea, _) = arc_geom(a);
            let p0 = [c[0] + r * sa.cos(), c[1] + r * sa.sin()];
            let p1 = [c[0] + r * ea.cos(), c[1] + r * ea.sin()];
            let quarter = (arc_span(sa, ea) / 4.0).tan();
            let dist = |p: [f64; 2]| (p[0] - t2[0]).hypot(p[1] - t2[1]);
            if dist(p0) <= dist(p1) {
                (quarter, vec![(p1, 0.0)])
            } else {
                (-quarter, vec![(p0, 0.0)])
            }
        }
        // Another polyline, `other_seg` being it before the fillet and the
        // segment the fillet trimmed: it is cut there like this one, keeping
        // the picked side, and its vertices follow on from the cut (#1422).
        EntityType::LwPolyline(other) => {
            let (orig, s) = other_seg?;
            let m = orig.vertices.len();
            if m < 2 || other.vertices.len() != m || orig.vertices[s].bulge.abs() > 1e-12 {
                return None;
            }
            // A closed polyline is opened at its closing segment first.
            let mut v = orig.vertices.clone();
            if orig.is_closed {
                let mut last = v[0].clone();
                last.bulge = 0.0;
                v.push(last);
            }
            let end_moved = !near(
                xy(&orig.vertices[(s + 1) % m]),
                xy(&other.vertices[(s + 1) % m]),
            );
            if end_moved {
                let chain = (0..=s)
                    .rev()
                    .map(|k| (xy(&v[k]), if k > 0 { -v[k - 1].bulge } else { 0.0 }))
                    .collect();
                (0.0, chain)
            } else {
                let chain = (s + 1..v.len())
                    .map(|k| (xy(&v[k]), if k + 1 < v.len() { v[k].bulge } else { 0.0 }))
                    .collect();
                (0.0, chain)
            }
        }
        _ => return None,
    };

    let vertex = |p: [f64; 2], bulge: f64| {
        let mut v = poly.vertices[seg_idx].clone();
        v.location.x = p[0];
        v.location.y = p[1];
        v.bulge = bulge;
        v
    };
    // A closed polyline is opened at its closing segment first.
    let mut open = poly.vertices.clone();
    if poly.is_closed {
        open.push(vertex(xy(&poly.vertices[0]), 0.0));
    }
    let mut vertices = Vec::with_capacity(open.len() + 3);
    if end_moved {
        // Keep the start side: vertices up to the segment, then T, the
        // fillet, and the other object.
        vertices.extend_from_slice(&open[..=seg_idx]);
        let bulge = fillet_center.map_or(0.0, |c| compute_bulge(t, t2, c));
        vertices.push(vertex(t, bulge));
        if fillet_center.is_some() {
            vertices.push(vertex(t2, first_bulge));
        } else if let Some(last) = vertices.last_mut() {
            last.bulge = first_bulge;
        }
        vertices.extend(chain.iter().map(|&(p, b)| vertex(p, b)));
    } else {
        // Keep the end side: the other object run backwards, the fillet,
        // then T and the vertices after the segment. Running backwards, a
        // vertex takes the negated bulge of the segment that arrived at it.
        let arriving: Vec<f64> = std::iter::once(first_bulge)
            .chain(chain.iter().map(|&(_, b)| b))
            .collect();
        for (k, &(p, _)) in chain.iter().enumerate().rev() {
            vertices.push(vertex(p, -arriving[k]));
        }
        if let Some(c) = fillet_center {
            vertices.push(vertex(t2, compute_bulge(t2, t, c)));
        }
        vertices.push(vertex(t, 0.0));
        vertices.extend_from_slice(&open[seg_idx + 1..]);
    }
    let mut joined = moved.clone();
    joined.vertices = vertices;
    joined.is_closed = false;
    Some(joined)
}

/// Fillet a Line and an Arc.
/// Fillets of `radius` tangent to both curves, nearest pick first.
///
/// The candidates come from the kernel, which finds them by crossing the two
/// curves' offsets — the construction that works whether or not the curves
/// meet. What is left here is the choice between them, which needs the two
/// pick points and so cannot be made anywhere else.
fn ranked_fillets(
    a: &KernelCurve,
    click_a: [f64; 2],
    b: &KernelCurve,
    click_b: [f64; 2],
    radius: f64,
) -> Vec<KernelFillet> {
    let reach = |from: [f64; 2], to: [f64; 2]| (to[0] - from[0]).hypot(to[1] - from[1]);
    let mut found = fillets_between(a, b, radius, Tolerance::new(FILLET_TOLERANCE));
    found.sort_by(|first, second| {
        let one = reach(click_a, first.tangent1) + reach(click_b, first.tangent2);
        let other = reach(click_a, second.tangent1) + reach(click_b, second.tangent2);
        one.total_cmp(&other)
    });
    found
}

/// The entity's shape as a kernel curve.
fn line_curve(l: &LineEnt) -> KernelCurve {
    KernelCurve::Line(KernelLine {
        start: [l.start.x, l.start.y],
        end: [l.end.x, l.end.y],
    })
}

fn arc_curve(a: &ArcEnt) -> KernelCurve {
    KernelCurve::Arc(KernelArc {
        centre: [a.center.x, a.center.y],
        radius: a.radius,
        start_angle: a.start_angle,
        end_angle: a.end_angle,
    })
}

/// An arc entity built from a kernel fillet.
fn fillet_entity(fillet: &KernelFillet, radius: f64, z: f64, common: &EntityCommon) -> ArcEnt {
    let mut arc = ArcEnt::new();
    arc.common = common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = Vector3::new(fillet.centre[0], fillet.centre[1], z);
    arc.radius = radius;
    arc.start_angle = fillet.start_angle;
    arc.end_angle = fillet.end_angle;
    arc
}

fn fillet_line_arc(
    line: &LineEnt,
    click_line: [f64; 2],
    arc: &ArcEnt,
    click_arc: [f64; 2],
    radius: f64,
    z: f64,
) -> Option<(EntityType, EntityType, Option<EntityType>)> {
    let (p1, _, u, _) = line_geom(line);
    let (ac, ar, _, _, _) = arc_geom(arc);

    // Intersection of infinite line with the arc's circle
    let ts = line_circle(p1, u, ac, ar);

    if radius < 1e-9 {
        // r=0: trim to intersection (nearest to each click)
        let t_best = ts.iter().copied().min_by(|a, b| {
            let da = (p1[0] + a * u[0] - click_line[0]).powi(2)
                + (p1[1] + a * u[1] - click_line[1]).powi(2);
            let db = (p1[0] + b * u[0] - click_line[0]).powi(2)
                + (p1[1] + b * u[1] - click_line[1]).powi(2);
            da.total_cmp(&db)
        })?;
        let ix = p1[0] + t_best * u[0];
        let iy = p1[1] + t_best * u[1];
        if !(ix.is_finite() && iy.is_finite()) {
            return None;
        }

        // Trim line to intersection
        let new_line = trim_line_to_point(line, [ix, iy], click_line)?;
        // Cut or extend the arc to the same corner.
        let new_arc = arc_to_angle(arc, arc_angle_at(ac, [ix, iy]), arc_angle_at(ac, click_arc));
        return Some((EntityType::Line(new_line), EntityType::Arc(new_arc), None));
    }

    // The candidates come from the kernel: a circle of `radius` tangent to
    // both has its centre where an offset of the line crosses an offset of
    // the arc's circle. What was here enumerated the same four combinations
    // by hand, with its own quadratic and its own sign conventions.
    for fillet in ranked_fillets(
        &line_curve(line),
        click_line,
        &arc_curve(arc),
        click_arc,
        radius,
    ) {
        let Some(new_line) = trim_line_to_point(line, fillet.tangent1, click_line) else {
            continue;
        };
        // The arc is cut back to its touch, or extended to it when the
        // fillet meets its circle past the drawn part.
        let new_arc = arc_to_angle(
            arc,
            arc_angle_at(ac, fillet.tangent2),
            arc_angle_at(ac, click_arc),
        );
        return Some((
            EntityType::Line(new_line),
            EntityType::Arc(new_arc),
            Some(EntityType::Arc(fillet_entity(
                &fillet,
                radius,
                z,
                &line.common,
            ))),
        ));
    }
    None
}

/// Fillet two arcs.
fn fillet_arc_arc(
    a1: &ArcEnt,
    click1: [f64; 2],
    a2: &ArcEnt,
    click2: [f64; 2],
    radius: f64,
    z: f64,
) -> Option<(EntityType, EntityType, Option<EntityType>)> {
    let (c1, r1, _, _, _) = arc_geom(a1);
    let (c2, r2, _, _, _) = arc_geom(a2);

    if radius < 1e-9 {
        // r=0: trim both arcs to their intersection point
        let pts = circle_circle_pts(c1, r1, c2, r2);
        if pts.is_empty() {
            return None;
        }
        // Pick the intersection point nearest to the average of the two clicks
        let cx = (click1[0] + click2[0]) / 2.0;
        let cy = (click1[1] + click2[1]) / 2.0;
        let ip = *pts.iter().min_by(|a, b| {
            (a[0] - cx)
                .hypot(a[1] - cy)
                .total_cmp(&(b[0] - cx).hypot(b[1] - cy))
        })?;
        if !(ip[0].is_finite() && ip[1].is_finite()) {
            return None;
        }

        let new_a1 = arc_to_angle(a1, arc_angle_at(c1, ip), arc_angle_at(c1, click1));
        let new_a2 = arc_to_angle(a2, arc_angle_at(c2, ip), arc_angle_at(c2, click2));
        return Some((EntityType::Arc(new_a1), EntityType::Arc(new_a2), None));
    }

    // Same construction as the line-and-arc case, from the same place: the
    // centre is where an offset of one circle crosses an offset of the other.
    if let Some(fillet) = ranked_fillets(&arc_curve(a1), click1, &arc_curve(a2), click2, radius).into_iter().next() {
        // Each arc is cut back to its touch, or extended to it.
        let new_a1 = arc_to_angle(
            a1,
            arc_angle_at(c1, fillet.tangent1),
            arc_angle_at(c1, click1),
        );
        let new_a2 = arc_to_angle(
            a2,
            arc_angle_at(c2, fillet.tangent2),
            arc_angle_at(c2, click2),
        );
        return Some((
            EntityType::Arc(new_a1),
            EntityType::Arc(new_a2),
            Some(EntityType::Arc(fillet_entity(
                &fillet, radius, z, &a1.common,
            ))),
        ));
    }
    None
}

/// FILLET where one side is a circle, ellipse or spline. The kernel finds the
/// tangent arcs; each side is then cut back to its tangent point, keeping the
/// part that was picked. A closed curve has no end to cut back and is left
/// whole.
fn fillet_curves(
    e1: &FilletEntity,
    click1: [f64; 2],
    e2: &FilletEntity,
    click2: [f64; 2],
    radius: f64,
    z: f64,
) -> Option<(EntityType, EntityType, Option<EntityType>)> {
    if radius < 1e-9 {
        return None;
    }
    let (a, b) = (e1.kernel_curve()?, e2.kernel_curve()?);
    let common = match e1 {
        FilletEntity::Line(l) => l.common.clone(),
        FilletEntity::Arc(a) => a.common.clone(),
        FilletEntity::LwPoly { poly, .. } => poly.common.clone(),
        FilletEntity::Curve(entity) => entity.common().clone(),
    };
    for fillet in ranked_fillets(&a, click1, &b, click2, radius) {
        let (Some(first), Some(second)) = (
            trim_to_tangent(e1, &a, fillet.tangent1, click1),
            trim_to_tangent(e2, &b, fillet.tangent2, click2),
        ) else {
            continue;
        };
        return Some((
            first,
            second,
            Some(EntityType::Arc(fillet_entity(&fillet, radius, z, &common))),
        ));
    }
    None
}

/// One side of a fillet cut back to where the arc touches it, the picked part
/// kept. `None` when the touch is off a bounded curve's drawn part — only a
/// line may be extended to meet the arc.
fn trim_to_tangent(
    entity: &FilletEntity,
    curve: &KernelCurve,
    touch: [f64; 2],
    click: [f64; 2],
) -> Option<EntityType> {
    match entity {
        FilletEntity::Line(l) => trim_line_to_point(l, touch, click).map(EntityType::Line),
        FilletEntity::LwPoly { poly, seg_idx, .. } => {
            let line = lwpoly_seg_as_line(poly, *seg_idx);
            let trimmed = trim_line_to_point(&line, touch, click)?;
            Some(EntityType::LwPolyline(rebuild_poly_from_trimmed_line(
                poly, *seg_idx, &line, &trimmed,
            )))
        }
        FilletEntity::Arc(arc) => {
            let (centre, _, start, end, _) = arc_geom(arc);
            let at = arc_angle_at(centre, touch);
            let clamped = clamp_angle_to_arc(at, start, end);
            if (norm_angle(at) - norm_angle(clamped)).abs() > 0.01 {
                return None;
            }
            let picked = clamp_angle_to_arc(arc_angle_at(centre, click), start, end);
            Some(EntityType::Arc(
                if (clamped - start).rem_euclid(TAU) <= (picked - start).rem_euclid(TAU) {
                    trim_arc(arc, clamped, end)
                } else {
                    trim_arc(arc, start, clamped)
                },
            ))
        }
        FilletEntity::Curve(source) => {
            let at = curve.parameter_at(touch);
            let on = curve.point_at(at);
            let scale = 1e-6 * (1.0 + touch[0].abs().max(touch[1].abs()));
            if (on[0] - touch[0]).hypot(on[1] - touch[1]) > scale.max(1e-6 * curve.length()) {
                return None;
            }
            if curve.is_closed() {
                return Some(source.clone());
            }
            let keep_end = curve.parameter_at(click) >= at;
            match source {
                // Its plan image runs the entity's own parameter only when
                // it faces up; a mirrored one is left alone.
                EntityType::Ellipse(ellipse) if ellipse.normal.z > 0.0 => {
                    let mut trimmed = ellipse.clone();
                    trimmed.common.handle = Handle::NULL;
                    let sweep = {
                        let raw = ellipse.end_parameter - ellipse.start_parameter;
                        if raw <= 0.0 {
                            raw + TAU
                        } else {
                            raw
                        }
                    };
                    let cut = ellipse.start_parameter + at * sweep;
                    if keep_end {
                        trimmed.start_parameter = cut;
                    } else {
                        trimmed.end_parameter = cut;
                    }
                    Some(EntityType::Ellipse(trimmed))
                }
                EntityType::Spline(spline) => {
                    let (low, high) = super::spline_ops::spline_range(spline)?;
                    let (head, tail) =
                        super::spline_ops::spline_cut(spline, low + at * (high - low))?;
                    Some(EntityType::Spline(if keep_end { tail } else { head }))
                }
                _ => None,
            }
        }
    }
}

/// Trim a line endpoint nearest to the intersection point, keeping the click side.
fn trim_line_to_point(line: &LineEnt, isect: [f64; 2], click: [f64; 2]) -> Option<LineEnt> {
    let mut l = line.clone();
    l.common.handle = Handle::NULL;
    let (p1, _, u, len) = line_geom(line);
    if len < 1e-12 {
        return None;
    }
    // Parameter of intersection
    let t_i = (isect[0] - p1[0]) * u[0] + (isect[1] - p1[1]) * u[1];
    // Parameter of click
    let t_c = (click[0] - p1[0]) * u[0] + (click[1] - p1[1]) * u[1];
    // If click is past intersection on the + side, keep intersection..end
    if t_c >= t_i {
        l.start = Vector3::new(isect[0], isect[1], line.start.z);
    } else {
        l.end = Vector3::new(isect[0], isect[1], line.end.z);
    }
    Some(l)
}

// ── Chamfer ────────────────────────────────────────────────────────────────

/// Compute chamfer: trim l1 by dist1 from intersection, l2 by dist2, add chamfer line.
fn compute_chamfer(
    l1: &LineEnt,
    click1: [f64; 2],
    dist1: f64,
    l2: &LineEnt,
    click2: [f64; 2],
    dist2: f64,
) -> Option<(EntityType, EntityType, EntityType)> {
    let (p1, _, u1, _) = line_geom(l1);
    let (p3, _, u2, _) = line_geom(l2);

    let (t_p, _u_p) = ll(p1[0], p1[1], u1[0], u1[1], p3[0], p3[1], u2[0], u2[1])?;

    let px = p1[0] + t_p * u1[0];
    let py = p1[1] + t_p * u1[1];
    let z = l1.start.z;

    let s1 = project_click(click1, [px, py], u1);
    let s2 = project_click(click2, [px, py], u2);
    let dir1 = if s1 >= 0.0 {
        [u1[0], u1[1]]
    } else {
        [-u1[0], -u1[1]]
    };
    let dir2 = if s2 >= 0.0 {
        [u2[0], u2[1]]
    } else {
        [-u2[0], -u2[1]]
    };

    // Chamfer points: back off dist from P along keep-direction
    let c1 = [px + dist1 * dir1[0], py + dist1 * dir1[1]];
    let c2 = [px + dist2 * dir2[0], py + dist2 * dir2[1]];

    // Trim l1 to c1 and l2 to c2
    let new_l1 = trim_to_xy(l1, c1, dir1, u1)?;
    let new_l2 = trim_to_xy(l2, c2, dir2, u2)?;

    // Chamfer line
    let mut cline = l1.clone();
    cline.common.handle = Handle::NULL;
    cline.start = Vector3::new(c1[0], c1[1], z);
    cline.end = Vector3::new(c2[0], c2[1], z);

    Some((
        EntityType::Line(new_l1),
        EntityType::Line(new_l2),
        EntityType::Line(cline),
    ))
}

// ══════════════════════════════════════════════════════════════════════════
// FilletCommand
// ══════════════════════════════════════════════════════════════════════════

enum FilletStep {
    First,
    Polyline,

    // Radius may be typed directly or measured by picking two points.
    WaitingForRadius,

    /// Trim / No trim: whether the picked objects are cut back to the arc.
    TrimMode,

    RadiusSecondPoint {
        first: DVec3,
    },

    Second {
        h1: Handle,
        e1: FilletEntity,
        click1: [f64; 2],
    },
}

pub struct FilletCommand {
    radius: f64,
    /// Whether the picked source entities are shortened to the tangent points
    /// (the shared TRIMMODE, changed through the Trim option).
    trim: bool,
    step: FilletStep,
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    /// First-object pick to restore after a radius entry made mid-selection
    /// (i.e. "R" pressed after the first object was already picked), so the
    /// command resumes at the second pick instead of restarting selection.
    resume_second: Option<(Handle, FilletEntity, [f64; 2])>,
    /// Multiple option: keep filleting until Enter / Esc.
    multiple: bool,
    /// Fillets applied by this command that Undo can still take back.
    made: usize,
}

impl FilletCommand {
    pub fn new(radius: f64, all_entities: Vec<EntityType>) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .iter()
            .map(crate::entities::curve::entity_with_lwpolyline_world_xy)
            .collect();
        let entity_index = ModifyEntityIndex::build(&all_entities);
        Self {
            radius: radius as f64,
            trim: defaults::get_fillet_trim(),
            step: FilletStep::First,
            all_entities,
            entity_index,
            resume_second: None,
            multiple: false,
            made: 0,
        }
    }

    /// Switch to the radius sub-step, remembering the first pick (if any) so
    /// it can be restored afterwards.
    fn enter_radius_substep(&mut self) {
        self.enter_substep(FilletStep::WaitingForRadius);
    }

    fn enter_substep(&mut self, step: FilletStep) {
        self.resume_second = if let FilletStep::Second { h1, e1, click1 } = &self.step {
            Some((*h1, e1.clone(), *click1))
        } else {
            None
        };
        self.step = step;
    }

    /// Leave the radius sub-step, resuming the second pick when a first object
    /// was already chosen, otherwise restarting at the first pick.
    fn resume_after_radius(&mut self) {
        self.step = match self.resume_second.take() {
            Some((h1, e1, click1)) => FilletStep::Second { h1, e1, click1 },
            None => FilletStep::First,
        };
    }

    fn continue_after_fillet(
        &mut self,
        mut replacements: Vec<(Handle, Vec<EntityType>)>,
    ) -> CmdResult {
        for entity in replacements
            .iter_mut()
            .flat_map(|(_, entities)| entities.iter_mut())
        {
            if let EntityType::LwPolyline(poly) = entity {
                drop_zero_length_segments(poly);
            }
        }
        // Single mode replaces and ends, like CHAMFER.
        if !self.multiple {
            return CmdResult::ReplaceMany(replacements, vec![]);
        }
        self.all_entities.retain(|entity| {
            !replacements
                .iter()
                .any(|(handle, _)| entity.common().handle == *handle)
        });
        self.all_entities.extend(
            replacements
                .iter()
                .flat_map(|(_, entities)| entities.iter().cloned()),
        );
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.step = FilletStep::First;
        self.resume_second = None;
        self.made += 1;
        CmdResult::ReplaceManyContinue(replacements)
    }

    /// Keep both source lines and add only the tangent arc. The host commits
    /// the addition as one undo step and notifies this command of its new handle.
    fn continue_after_addition(&mut self, addition: EntityType) -> CmdResult {
        self.all_entities.push(addition.clone());
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.step = FilletStep::First;
        self.resume_second = None;
        self.made += 1;
        if self.multiple {
            CmdResult::CommitEntities(vec![addition])
        } else {
            CmdResult::CommitEntitiesAndExit(vec![addition])
        }
    }

    /// Undo option: take back the last fillet of this command. The host
    /// undoes the document step and then hands the restored entities back
    /// through `on_document_undone`.
    fn undo_last(&mut self) -> Option<CmdResult> {
        if self.made == 0 {
            return None;
        }
        self.made -= 1;
        Some(CmdResult::UndoDocument)
    }
}

impl CadCommand for FilletCommand {
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let FilletStep::RadiusSecondPoint { first } = self.step else {
            return None;
        };

        Some(WireModel::solid_f64(
            "fillet_radius_preview".into(),
            vec![[first.x, first.y, first.z], [pt.x, pt.y, pt.z]],
            WireModel::CYAN,
            false,
        ))
    }
    fn name(&self) -> &'static str {
        "FILLET"
    }

    fn prompt(&self) -> String {
        match &self.step {
            FilletStep::First => crate::tf!(
                "FILLET  Select first object or [Polyline/Radius]  [R={:.4}]:",
                self.radius
            )
            .into_owned(),
            FilletStep::Polyline => {
                crate::tf!("FILLET  Select 2D polyline  [R={:.4}]:", self.radius).into_owned()
            }
            FilletStep::WaitingForRadius => crate::tf!(
                "FILLET  Specify fillet radius or first point <{:.4}>:",
                self.radius
            )
            .into_owned(),

            FilletStep::RadiusSecondPoint { .. } => {
                crate::t!("FILLET  Specify second point for radius:").into_owned()
            }
            FilletStep::TrimMode => if self.trim {
                crate::t!("FILLET  Enter Trim mode option [Trim/No trim] <Trim>:")
            } else {
                crate::t!("FILLET  Enter Trim mode option [Trim/No trim] <No trim>:")
            }
            .into_owned(),
            FilletStep::Second { .. } => crate::tf!(
                "FILLET  Select second object (Line/Arc/LwPolyline)  [R={:.4}]:",
                self.radius
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;

        match self.step {
            FilletStep::First => {
                let mut opts = Vec::new();
                if self.made > 0 {
                    opts.push(CmdOption::new("Undo", "U"));
                }
                opts.push(CmdOption::new("Polyline", "P"));
                opts.push(CmdOption::new("Radius", "R"));
                opts.push(CmdOption::new("Trim", "T"));
                if !self.multiple {
                    opts.push(CmdOption::new("Multiple", "M"));
                }
                opts
            }
            FilletStep::Second { .. } => {
                vec![CmdOption::new("Radius", "R"), CmdOption::new("Trim", "T")]
            }
            FilletStep::TrimMode => {
                vec![CmdOption::new("Trim", "T"), CmdOption::new("No trim", "N")]
            }
            FilletStep::Polyline
            | FilletStep::WaitingForRadius
            | FilletStep::RadiusSecondPoint { .. } => vec![],
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            FilletStep::WaitingForRadius | FilletStep::TrimMode
        )
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.step, FilletStep::WaitingForRadius) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            FilletStep::WaitingForRadius => {
                let t = text.trim();
                if t.is_empty() {
                    // Keep current radius, resume where the radius was requested.
                    self.resume_after_radius();
                    return Some(CmdResult::NeedPoint);
                }
                if let Some(v) = crate::entities::common::parse_typed_length(t) {
                    if v >= 0.0 {
                        self.radius = v;
                        defaults::set_fillet_radius(v);
                    }
                    self.resume_after_radius();
                    return Some(CmdResult::NeedPoint);
                }
                // Invalid — stay and re-prompt
                Some(CmdResult::NeedPoint)
            }
            FilletStep::TrimMode => {
                match text.trim().to_uppercase().as_str() {
                    "" => {}
                    "T" | "TRIM" => {
                        self.trim = true;
                        defaults::set_fillet_trim(true);
                    }
                    "N" | "NO TRIM" | "NOTRIM" => {
                        self.trim = false;
                        defaults::set_fillet_trim(false);
                    }
                    _ => return Some(CmdResult::NeedPoint),
                }
                self.resume_after_radius();
                Some(CmdResult::NeedPoint)
            }
            FilletStep::First | FilletStep::Second { .. } => {
                let t = text.trim();
                let upper = t.to_uppercase();
                if matches!(self.step, FilletStep::First) && upper == "P" {
                    self.step = FilletStep::Polyline;
                    return Some(CmdResult::NeedPoint);
                }
                if upper == "U" || upper == "UNDO" {
                    return self.undo_last();
                }
                if matches!(self.step, FilletStep::First) && (upper == "M" || upper == "MULTIPLE")
                {
                    self.multiple = true;
                    return Some(CmdResult::NeedPoint);
                }
                if upper == "T" || upper == "TRIM" {
                    self.enter_substep(FilletStep::TrimMode);
                    return Some(CmdResult::NeedPoint);
                }
                // "R" alone → enter sub-step to collect radius
                if upper == "R" {
                    self.enter_radius_substep();
                    return Some(CmdResult::NeedPoint);
                }
                // "R 5.0" inline shorthand
                if t.starts_with(['r', 'R']) {
                    let body = t[1..].trim();
                    if let Some(v) = crate::entities::common::parse_typed_length(body) {
                        if v >= 0.0 {
                            self.radius = v;
                            defaults::set_fillet_radius(v);
                        }
                        // Stay in the current step (keeps any first pick).
                        return Some(CmdResult::NeedPoint);
                    }
                    // "R" + invalid body → enter sub-step
                    self.enter_radius_substep();
                    return Some(CmdResult::NeedPoint);
                }
                None
            }
            FilletStep::Polyline | FilletStep::RadiusSecondPoint { .. } => None,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        !matches!(
            self.step,
            FilletStep::WaitingForRadius
                | FilletStep::RadiusSecondPoint { .. }
                | FilletStep::TrimMode
        )
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        self.undo_last()
    }

    fn on_document_undone(&mut self, document: &codec::Document) {
        self.all_entities = document
            .entities()
            .map(crate::entities::curve::entity_with_lwpolyline_world_xy)
            .collect();
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.step = FilletStep::First;
        self.resume_second = None;
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let click = [pt.x as f64, pt.y as f64]; // drawing plane is world XY

        match &self.step {
            FilletStep::WaitingForRadius
            | FilletStep::RadiusSecondPoint { .. }
            | FilletStep::TrimMode => {
                return CmdResult::NeedPoint;
            }

            FilletStep::Polyline => {
                let poly = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|entity| match entity {
                        EntityType::LwPolyline(poly) => Some(poly.clone()),
                        _ => None,
                    });

                let Some(poly) = poly else {
                    return CmdResult::NeedPoint;
                };

                let Some(result) = fillet_entire_lwpolyline(&poly, self.radius) else {
                    return CmdResult::NeedPoint;
                };

                self.continue_after_fillet(vec![(handle, vec![EntityType::LwPolyline(result)])])
            }
            FilletStep::First => {
                let e1 = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|entity| match entity {
                        EntityType::LwPolyline(p) => {
                            Some(FilletEntity::from_lwpoly(p, handle, click))
                        }
                        other => FilletEntity::from_entity(other),
                    });
                if let Some(e) = e1 {
                    self.step = FilletStep::Second {
                        h1: handle,
                        e1: e,
                        click1: click,
                    };
                    CmdResult::NeedPoint
                } else {
                    CmdResult::NeedPoint
                }
            }
            FilletStep::Second { h1, e1, click1 } => {
                let h1 = *h1;
                let e1 = e1.clone();
                let click1 = *click1;
                let same_entity = handle == h1;

                // For non-LwPoly entities, reject same-entity re-picks.
                if same_entity && !matches!(e1, FilletEntity::LwPoly { .. }) {
                    return CmdResult::NeedPoint;
                }

                let e2 = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|entity| match entity {
                        EntityType::LwPolyline(p) => {
                            Some(FilletEntity::from_lwpoly(p, handle, click))
                        }
                        other => FilletEntity::from_entity(other),
                    });

                if let Some(e2) = e2 {
                    match compute_fillet_entities(&e1, click1, &e2, click, self.radius) {
                        Some((new_e1, new_e2, maybe_arc)) => {
                            // No trim: the objects stay as they are; only the
                            // arc is added (#1558).
                            if !self.trim {
                                return maybe_arc
                                    .map(|arc| self.continue_after_addition(arc))
                                    .unwrap_or(CmdResult::NeedPoint);
                            }
                            // A polyline filleted with a line or arc takes
                            // the fillet and the other object in.
                            let joined = match (&e1, &e2, &new_e1, &new_e2) {
                                (
                                    FilletEntity::LwPoly { poly, seg_idx, .. },
                                    FilletEntity::Line(_) | FilletEntity::Arc(_),
                                    EntityType::LwPolyline(moved),
                                    other,
                                ) if !same_entity => join_poly_fillet(
                                    poly,
                                    *seg_idx,
                                    moved,
                                    other,
                                    None,
                                    maybe_arc.as_ref(),
                                )
                                .map(|poly| {
                                    vec![
                                        (h1, vec![EntityType::LwPolyline(poly)]),
                                        (handle, Vec::new()),
                                    ]
                                }),
                                // Two polylines join into the first one.
                                (
                                    FilletEntity::LwPoly { poly, seg_idx, .. },
                                    FilletEntity::LwPoly {
                                        poly: poly2,
                                        seg_idx: seg2,
                                        ..
                                    },
                                    EntityType::LwPolyline(moved),
                                    other,
                                ) if !same_entity => join_poly_fillet(
                                    poly,
                                    *seg_idx,
                                    moved,
                                    other,
                                    Some((poly2, *seg2)),
                                    maybe_arc.as_ref(),
                                )
                                .map(|poly| {
                                    vec![
                                        (h1, vec![EntityType::LwPolyline(poly)]),
                                        (handle, Vec::new()),
                                    ]
                                }),
                                (
                                    FilletEntity::Line(_) | FilletEntity::Arc(_),
                                    FilletEntity::LwPoly { poly, seg_idx, .. },
                                    other,
                                    EntityType::LwPolyline(moved),
                                ) if !same_entity => join_poly_fillet(
                                    poly,
                                    *seg_idx,
                                    moved,
                                    other,
                                    None,
                                    maybe_arc.as_ref(),
                                )
                                .map(|poly| {
                                    vec![
                                        (handle, vec![EntityType::LwPolyline(poly)]),
                                        (h1, Vec::new()),
                                    ]
                                }),
                                _ => None,
                            };
                            if let Some(replacements) = joined {
                                return self.continue_after_fillet(replacements);
                            }
                            let mut first_replacements = vec![new_e1];
                            if let Some(arc) = maybe_arc {
                                first_replacements.push(arc);
                            }
                            let replacements = if same_entity {
                                // Corner fillet: both results are the same rebuilt poly.
                                vec![(h1, first_replacements)]
                            } else {
                                vec![(h1, first_replacements), (handle, vec![new_e2])]
                            };
                            self.continue_after_fillet(replacements)
                        }
                        None => CmdResult::NeedPoint,
                    }
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        if handle.is_null() {
            return vec![];
        }
        let click = [pt.x as f64, pt.y as f64];

        match &self.step {
            FilletStep::WaitingForRadius
            | FilletStep::RadiusSecondPoint { .. }
            | FilletStep::TrimMode => vec![],
            FilletStep::Polyline => {
                let preview = self.entity_index.get(&self.all_entities, handle).and_then(
                    |entity| match entity {
                        EntityType::LwPolyline(poly) => fillet_entire_lwpolyline(poly, self.radius),
                        _ => None,
                    },
                );

                preview
                    .map(|poly| {
                        vec![WireModel::solid(
                            "fillet_polyline_preview".into(),
                            lwpoly_pts(&poly),
                            WireModel::CYAN,
                            false,
                        )]
                    })
                    .unwrap_or_default()
            }
            FilletStep::First => {
                let pts =
                    self.entity_index
                        .get(&self.all_entities, handle)
                        .and_then(|e| match e {
                            EntityType::LwPolyline(p) => Some(lwpoly_seg_hover_pts(p, click)),
                            _ => FilletEntity::from_entity(e)
                                .map(|fe| entity_pts(&fe.to_entity_type())),
                        });
                if let Some(pts) = pts {
                    vec![WireModel::solid(
                        "fillet_hover".into(),
                        pts,
                        WireModel::CYAN,
                        false,
                    )]
                } else {
                    vec![]
                }
            }
            FilletStep::Second { h1, e1, click1 } => {
                let h1 = *h1;
                let e1 = e1.clone();
                let click1 = *click1;
                let e2 = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|entity| match entity {
                        EntityType::LwPolyline(p) => {
                            Some(FilletEntity::from_lwpoly(p, handle, click))
                        }
                        other => FilletEntity::from_entity(other),
                    });
                // For non-LwPoly, skip same-entity hover.
                let _ = h1;
                if let Some(e2) = e2 {
                    if let Some((new_e1, new_e2, maybe_arc)) =
                        compute_fillet_entities(&e1, click1, &e2, click, self.radius)
                    {
                        let mut out = Vec::new();
                        if self.trim {
                            out.push(WireModel::solid(
                                "fillet_e1".into(),
                                entity_pts(&new_e1),
                                WireModel::CYAN,
                                false,
                            ));
                            out.push(WireModel::solid(
                                "fillet_e2".into(),
                                entity_pts(&new_e2),
                                WireModel::CYAN,
                                false,
                            ));
                        }
                        if let Some(arc) = maybe_arc {
                            out.push(WireModel::solid(
                                "fillet_arc".into(),
                                entity_pts(&arc),
                                WireModel::CYAN,
                                false,
                            ));
                        }
                        return out;
                    }
                }
                vec![]
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            FilletStep::WaitingForRadius => {
                // First point of the temporary radius measurement.
                self.step = FilletStep::RadiusSecondPoint { first: pt };
                CmdResult::NeedPoint
            }

            FilletStep::RadiusSecondPoint { first } => {
                let radius = first.distance(pt);

                // Ignore an accidental zero-length measurement.
                if radius > 1.0e-9 {
                    self.radius = radius;
                    defaults::set_fillet_radius(radius);
                    self.resume_after_radius();
                }

                CmdResult::NeedPoint
            }

            _ => CmdResult::NeedPoint,
        }
    }
    fn preserve_commit_layer(&self) -> bool {
        // Derived no-trim geometry stays on the picked line's layer rather
        // than being moved to the current creation layer.
        true
    }

    fn on_entities_committed(&mut self, entities: &[EntityType]) {
        let mut handles = entities.iter().map(|entity| entity.common().handle);
        for entity in self
            .all_entities
            .iter_mut()
            .filter(|entity| entity.common().handle.is_null())
        {
            let Some(handle) = handles.next() else {
                break;
            };
            entity.common_mut().handle = handle;
        }
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
    }

    fn on_entity_replaced(&mut self, _old: Handle, new_handles: &[Handle]) {
        let mut handles = new_handles.iter().copied();
        for entity in self
            .all_entities
            .iter_mut()
            .filter(|entity| entity.common().handle.is_null())
        {
            let Some(handle) = handles.next() else {
                break;
            };
            entity.common_mut().handle = handle;
        }
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// A radius-0 fillet over an existing arc leaves its two ends on one point;
/// the zero-length segment between them blocked every later fillet of that
/// corner (#1422). The later vertex carries the next segment, so it survives.
fn drop_zero_length_segments(poly: &mut LwPolyline) {
    let same = |a: &LwVertex, b: &LwVertex| {
        (a.location.x - b.location.x).abs() < 1e-9 && (a.location.y - b.location.y).abs() < 1e-9
    };
    let mut i = 0;
    while i + 1 < poly.vertices.len() && poly.vertices.len() > 2 {
        if same(&poly.vertices[i], &poly.vertices[i + 1]) {
            poly.vertices.remove(i);
        } else {
            i += 1;
        }
    }
    if poly.is_closed && poly.vertices.len() > 2 {
        let n = poly.vertices.len();
        if same(&poly.vertices[n - 1], &poly.vertices[0]) {
            poly.vertices.pop();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// ChamferCommand
// ══════════════════════════════════════════════════════════════════════════

/// Chamfer a corner of a single LwPolyline: the two picks select two adjacent
/// segments; the shared corner vertex is cut back by dist1/dist2 and replaced
/// with a straight chamfer edge (bulge 0). Returns the rebuilt polyline.
fn chamfer_lwpoly_corner(
    poly: &LwPolyline,
    click1: [f64; 2],
    dist1: f64,
    click2: [f64; 2],
    dist2: f64,
) -> Option<EntityType> {
    let n = poly.vertices.len();
    if n < 3 {
        return None;
    }
    let s1 = lwpoly_nearest_seg(poly, click1);
    let s2 = lwpoly_nearest_seg(poly, click2);
    if s1 == s2 {
        return None;
    }
    let (low, high) = if s1 < s2 { (s1, s2) } else { (s2, s1) };
    // The wrap-around corner of a closed polyline joins the last segment
    // (n-1) and the first (0); their shared vertex is v0.
    let wrap = poly.is_closed && low == 0 && high == n.saturating_sub(1);
    if !(high == low + 1 || wrap) {
        return None;
    }
    // `before_seg` ends at the shared corner vertex, `after_seg` starts there.
    let (before_seg, after_seg, corner_idx) = if wrap {
        (high, low, 0)
    } else {
        (low, high, high)
    };
    let l1 = lwpoly_seg_as_line(poly, before_seg);
    let l2 = lwpoly_seg_as_line(poly, after_seg);
    // Map each click + distance to whichever segment it was picked on.
    let (c1, d1, c2, d2) = if s1 == before_seg {
        (click1, dist1, click2, dist2)
    } else {
        (click2, dist2, click1, dist1)
    };
    match compute_chamfer(&l1, c1, d1, &l2, c2, d2)? {
        (EntityType::Line(tl1), EntityType::Line(tl2), _) => {
            let t1 = [tl1.end.x, tl1.end.y];
            let t2 = [tl2.start.x, tl2.start.y];
            let new_poly = lwpoly_replace_corner(poly, corner_idx, t1, t2, 0.0);
            Some(EntityType::LwPolyline(new_poly))
        }
        _ => None,
    }
}

enum ChamferStep {
    First,
    WaitingForDist1,
    WaitingForDist2,
    Second {
        h1: Handle,
        l1: LineEnt,
        click1: [f64; 2],
    },
    /// First pick was an LwPolyline segment; waiting for the second segment
    /// of the same polyline to chamfer the shared corner.
    SecondPoly {
        h1: Handle,
        poly: LwPolyline,
        click1: [f64; 2],
    },
}

/// The refusal a no-trim CHAMFER prints for anything but two independent Lines.
const NO_TRIM_CHAMFER_LINES_ONLY: &str = "No-trim chamfer supports two independent lines only.";

pub struct ChamferCommand {
    dist1: f64,
    dist2: f64,
    /// Whether the picked source entities are shortened to the chamfer points
    /// (the TRIMMODE FILLET shares). No-trim is defined for two independent
    /// Lines only.
    trim: bool,
    step: ChamferStep,
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    /// First-object pick (line or polyline segment) to restore after a
    /// distance entry made mid-selection, so the command resumes at the
    /// second pick instead of restarting selection.
    resume_pick: Option<ChamferStep>,
    /// Multiple option: keep chamfering until Enter / Esc.
    multiple: bool,
    /// Chamfers applied by this command that Undo can still take back.
    made: usize,
}

impl ChamferCommand {
    pub fn new(dist: f64, all_entities: Vec<EntityType>) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .iter()
            .map(crate::entities::curve::entity_with_lwpolyline_world_xy)
            .collect();
        let entity_index = ModifyEntityIndex::build(&all_entities);
        Self {
            dist1: dist as f64,
            dist2: defaults::get_chamfer_dist2(),
            trim: defaults::get_fillet_trim(),
            step: ChamferStep::First,
            all_entities,
            entity_index,
            resume_pick: None,
            multiple: false,
            made: 0,
        }
    }

    /// Switch to the distance sub-step, remembering the first pick (if any).
    fn enter_dist_substep(&mut self) {
        self.resume_pick = match &self.step {
            ChamferStep::Second { h1, l1, click1 } => Some(ChamferStep::Second {
                h1: *h1,
                l1: l1.clone(),
                click1: *click1,
            }),
            ChamferStep::SecondPoly { h1, poly, click1 } => Some(ChamferStep::SecondPoly {
                h1: *h1,
                poly: poly.clone(),
                click1: *click1,
            }),
            _ => None,
        };
        self.step = ChamferStep::WaitingForDist1;
    }

    /// Leave the distance sub-step, resuming the second pick when a first
    /// object was already chosen, otherwise restarting at the first pick.
    fn resume_after_dist(&mut self) {
        self.step = self.resume_pick.take().unwrap_or(ChamferStep::First);
    }

    /// Publish a chamfer. Single mode replaces and ends; Multiple mode
    /// replaces, refreshes the cached entities and returns to the first pick.
    fn commit(&mut self, replacements: Vec<(Handle, Vec<EntityType>)>) -> CmdResult {
        if !self.multiple {
            return CmdResult::ReplaceMany(replacements, vec![]);
        }
        self.all_entities.retain(|entity| {
            !replacements
                .iter()
                .any(|(handle, _)| entity.common().handle == *handle)
        });
        self.all_entities.extend(
            replacements
                .iter()
                .flat_map(|(_, entities)| entities.iter().cloned()),
        );
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.step = ChamferStep::First;
        self.resume_pick = None;
        self.made += 1;
        CmdResult::ReplaceManyContinue(replacements)
    }

    /// No-trim mode keeps both source lines and commits only the chamfer line.
    fn continue_after_addition(&mut self, addition: EntityType) -> CmdResult {
        self.all_entities.push(addition.clone());
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.step = ChamferStep::First;
        self.resume_pick = None;
        self.made += 1;
        if self.multiple {
            CmdResult::CommitEntities(vec![addition])
        } else {
            CmdResult::CommitEntitiesAndExit(vec![addition])
        }
    }

    fn undo_last(&mut self) -> Option<CmdResult> {
        if self.made == 0 {
            return None;
        }
        self.made -= 1;
        Some(CmdResult::UndoDocument)
    }
}

impl CadCommand for ChamferCommand {
    fn name(&self) -> &'static str {
        "CHAMFER"
    }

    fn prompt(&self) -> String {
        match &self.step {
            ChamferStep::First => {
                let d1 = format!("{:.4}", self.dist1);
                let d2 = format!("{:.4}", self.dist2);
                t!(
                    "CHAMFER  Select first line  [D1=%{d1} D2=%{d2}]:",
                    d1 = d1,
                    d2 = d2
                )
                .into_owned()
            }
            ChamferStep::WaitingForDist1 => {
                let d1 = format!("{:.4}", self.dist1);
                t!("CHAMFER  Enter first chamfer distance <%{d1}>:", d1 = d1).into_owned()
            }
            ChamferStep::WaitingForDist2 => {
                let d2 = format!("{:.4}", self.dist2);
                t!("CHAMFER  Enter second chamfer distance <%{d2}>:", d2 = d2).into_owned()
            }
            ChamferStep::Second { .. } => {
                let d1 = format!("{:.4}", self.dist1);
                let d2 = format!("{:.4}", self.dist2);
                t!(
                    "CHAMFER  Select second line  [D1=%{d1} D2=%{d2}]:",
                    d1 = d1,
                    d2 = d2
                )
                .into_owned()
            }
            ChamferStep::SecondPoly { .. } => {
                let d1 = format!("{:.4}", self.dist1);
                let d2 = format!("{:.4}", self.dist2);
                t!(
                    "CHAMFER  Select the adjacent polyline segment  [D1=%{d1} D2=%{d2}]:",
                    d1 = d1,
                    d2 = d2
                )
                .into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;

        match self.step {
            ChamferStep::First => {
                let mut opts = Vec::new();
                if self.made > 0 {
                    opts.push(CmdOption::new("Undo", "U"));
                }
                opts.push(CmdOption::new("Distance", "D"));
                if !self.multiple {
                    opts.push(CmdOption::new("Multiple", "M"));
                }
                opts.push(if self.trim {
                    CmdOption::new("No trim", "N")
                } else {
                    CmdOption::new("Trim", "T")
                });
                opts
            }
            ChamferStep::Second { .. } | ChamferStep::SecondPoly { .. } => {
                vec![
                    CmdOption::new("Distance", "D"),
                    if self.trim {
                        CmdOption::new("No trim", "N")
                    } else {
                        CmdOption::new("Trim", "T")
                    },
                ]
            }
            ChamferStep::WaitingForDist1 | ChamferStep::WaitingForDist2 => vec![],
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(
            self.step,
            ChamferStep::WaitingForDist1 | ChamferStep::WaitingForDist2
        )
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(
            self.step,
            ChamferStep::WaitingForDist1 | ChamferStep::WaitingForDist2
        ) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            ChamferStep::WaitingForDist1 => {
                let t = text.trim();
                if t.is_empty() {
                    // Keep current dist1, move on to dist2
                    self.step = ChamferStep::WaitingForDist2;
                    return Some(CmdResult::NeedPoint);
                }
                if let Some(v) = crate::entities::common::parse_typed_length(t) {
                    self.dist1 = v.max(0.0);
                    defaults::set_chamfer_dist1(self.dist1);
                    self.step = ChamferStep::WaitingForDist2;
                    return Some(CmdResult::NeedPoint);
                }
                // Invalid — stay and re-prompt
                Some(CmdResult::NeedPoint)
            }
            ChamferStep::WaitingForDist2 => {
                let t = text.trim();
                if t.is_empty() {
                    // Keep current dist2, resume where the distance was requested.
                    self.resume_after_dist();
                    return Some(CmdResult::NeedPoint);
                }
                if let Some(v) = crate::entities::common::parse_typed_length(t) {
                    self.dist2 = v.max(0.0);
                    defaults::set_chamfer_dist2(self.dist2);
                    self.resume_after_dist();
                    return Some(CmdResult::NeedPoint);
                }
                // Invalid — stay and re-prompt
                Some(CmdResult::NeedPoint)
            }
            ChamferStep::First | ChamferStep::Second { .. } | ChamferStep::SecondPoly { .. } => {
                let t = text.trim();
                let upper = t.to_uppercase();
                if matches!(upper.as_str(), "N" | "NOTRIM" | "NO TRIM" | "TRIM=FALSE") {
                    self.trim = false;
                    defaults::set_fillet_trim(false);
                    return Some(CmdResult::NeedPoint);
                }
                if matches!(upper.as_str(), "T" | "TRIM" | "TRIM=TRUE") {
                    self.trim = true;
                    defaults::set_fillet_trim(true);
                    return Some(CmdResult::NeedPoint);
                }
                if matches!(self.step, ChamferStep::First) {
                    match upper.as_str() {
                        "M" | "MULTIPLE" => {
                            self.multiple = true;
                            return Some(CmdResult::NeedPoint);
                        }
                        "U" | "UNDO" => return self.undo_last(),
                        _ => {}
                    }
                }
                // "D" alone → enter sub-step to collect distances
                if upper == "D" {
                    self.enter_dist_substep();
                    return Some(CmdResult::NeedPoint);
                }
                // "D 5.0" or "D 5.0 3.0" inline shorthand
                if t.starts_with(['d', 'D']) {
                    let body = t[1..].trim();
                    let parts: Vec<f64> = body
                        .split_whitespace()
                        .filter_map(crate::entities::common::parse_typed_length)
                        .collect();
                    if !parts.is_empty() {
                        if let Some(&v) = parts.first() {
                            self.dist1 = v.max(0.0);
                            defaults::set_chamfer_dist1(self.dist1);
                        }
                        if let Some(&v) = parts.get(1) {
                            self.dist2 = v.max(0.0);
                            defaults::set_chamfer_dist2(self.dist2);
                        } else {
                            self.dist2 = self.dist1;
                            defaults::set_chamfer_dist2(self.dist2);
                        }
                        return Some(CmdResult::NeedPoint);
                    }
                    // "D" + invalid body → enter sub-step
                    self.enter_dist_substep();
                    return Some(CmdResult::NeedPoint);
                }
                None
            }
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        self.undo_last()
    }

    fn on_document_undone(&mut self, document: &codec::Document) {
        self.all_entities = document
            .entities()
            .map(crate::entities::curve::entity_with_lwpolyline_world_xy)
            .collect();
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.step = ChamferStep::First;
        self.resume_pick = None;
    }

    fn preserve_commit_layer(&self) -> bool {
        // A no-trim chamfer is derived from the picked lines; keep it on their
        // layer instead of moving it to the current creation layer.
        true
    }

    fn on_entities_committed(&mut self, entities: &[EntityType]) {
        let mut handles = entities.iter().map(|entity| entity.common().handle);
        for entity in self
            .all_entities
            .iter_mut()
            .filter(|entity| entity.common().handle.is_null())
        {
            let Some(handle) = handles.next() else {
                break;
            };
            entity.common_mut().handle = handle;
        }
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
    }

    fn needs_entity_pick(&self) -> bool {
        !matches!(
            self.step,
            ChamferStep::WaitingForDist1 | ChamferStep::WaitingForDist2
        )
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let click = [pt.x as f64, pt.y as f64];

        match &self.step {
            ChamferStep::WaitingForDist1 | ChamferStep::WaitingForDist2 => {
                return CmdResult::NeedPoint;
            }
            ChamferStep::First => {
                match self.entity_index.get(&self.all_entities, handle) {
                    Some(EntityType::Line(l)) => {
                        self.step = ChamferStep::Second {
                            h1: handle,
                            l1: l.clone(),
                            click1: click,
                        };
                    }
                    Some(EntityType::LwPolyline(p)) => {
                        if !self.trim {
                            return CmdResult::ReportError(NO_TRIM_CHAMFER_LINES_ONLY.to_string());
                        }
                        self.step = ChamferStep::SecondPoly {
                            h1: handle,
                            poly: p.clone(),
                            click1: click,
                        };
                    }
                    _ => {}
                }
                CmdResult::NeedPoint
            }
            ChamferStep::SecondPoly { h1, poly, click1 } => {
                // A no-trim chamfer cannot preserve a polyline corner as two
                // source entities; refuse rather than trimming implicitly.
                if !self.trim {
                    self.step = ChamferStep::First;
                    self.resume_pick = None;
                    return CmdResult::ReportError(NO_TRIM_CHAMFER_LINES_ONLY.to_string());
                }
                let h1 = *h1;
                let poly = poly.clone();
                let click1 = *click1;
                if handle != h1 {
                    // Chamfer corners only within the same polyline.
                    return CmdResult::NeedPoint;
                }
                match chamfer_lwpoly_corner(&poly, click1, self.dist1, click, self.dist2) {
                    Some(new_poly) => self.commit(vec![(h1, vec![new_poly])]),
                    None => CmdResult::NeedPoint,
                }
            }
            ChamferStep::Second { h1, l1, click1 } => {
                let h1 = *h1;
                let l1 = l1.clone();
                let click1 = *click1;
                if handle == h1 {
                    return CmdResult::NeedPoint;
                }

                let l2 = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|e| {
                        if let EntityType::Line(l) = e {
                            Some(l.clone())
                        } else {
                            None
                        }
                    });

                if let Some(l2) = l2 {
                    match compute_chamfer(&l1, click1, self.dist1, &l2, click, self.dist2) {
                        Some((new_l1, new_l2, chamfer_line)) => {
                            // No-trim keeps both source lines and adds only the
                            // derived chamfer line.
                            if !self.trim {
                                return self.continue_after_addition(chamfer_line);
                            }
                            // Single mode adds the chamfer line as a new entity;
                            // Multiple folds it into the first line's replacement
                            // so the command can carry on.
                            if self.multiple {
                                self.commit(vec![
                                    (h1, vec![new_l1, chamfer_line]),
                                    (handle, vec![new_l2]),
                                ])
                            } else {
                                CmdResult::ReplaceMany(
                                    vec![(h1, vec![new_l1]), (handle, vec![new_l2])],
                                    vec![chamfer_line],
                                )
                            }
                        }
                        None => CmdResult::NeedPoint,
                    }
                } else if !self.trim {
                    self.step = ChamferStep::First;
                    self.resume_pick = None;
                    CmdResult::ReportError(NO_TRIM_CHAMFER_LINES_ONLY.to_string())
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        if handle.is_null() {
            return vec![];
        }
        let click = [pt.x as f64, pt.y as f64];

        match &self.step {
            ChamferStep::WaitingForDist1 | ChamferStep::WaitingForDist2 => return vec![],
            ChamferStep::First => {
                let pts = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|e| match e {
                        EntityType::Line(l) => Some(line_pts(l)),
                        EntityType::LwPolyline(p) => Some(lwpoly_seg_hover_pts(p, click)),
                        _ => None,
                    });
                if let Some(pts) = pts {
                    vec![WireModel::solid(
                        "chamfer_hover".into(),
                        pts,
                        WireModel::CYAN,
                        false,
                    )]
                } else {
                    vec![]
                }
            }
            ChamferStep::Second { l1, click1, .. } => {
                let l1 = l1.clone();
                let click1 = *click1;
                let l2 = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|e| {
                        if let EntityType::Line(l) = e {
                            Some(l.clone())
                        } else {
                            None
                        }
                    });
                if let Some(l2) = l2 {
                    if let Some((new_l1, new_l2, cline)) =
                        compute_chamfer(&l1, click1, self.dist1, &l2, click, self.dist2)
                    {
                        if !self.trim {
                            return vec![WireModel::solid(
                                "chamfer_line".into(),
                                entity_pts(&cline),
                                WireModel::CYAN,
                                false,
                            )];
                        }
                        return vec![
                            WireModel::solid(
                                "chamfer_l1".into(),
                                entity_pts(&new_l1),
                                WireModel::CYAN,
                                false,
                            ),
                            WireModel::solid(
                                "chamfer_l2".into(),
                                entity_pts(&new_l2),
                                WireModel::CYAN,
                                false,
                            ),
                            WireModel::solid(
                                "chamfer_line".into(),
                                entity_pts(&cline),
                                WireModel::CYAN,
                                false,
                            ),
                        ];
                    }
                }
                vec![]
            }
            ChamferStep::SecondPoly { h1, poly, click1 } => {
                if !self.trim {
                    return vec![];
                }
                if handle != *h1 {
                    return vec![];
                }
                if let Some(new_poly) =
                    chamfer_lwpoly_corner(poly, *click1, self.dist1, click, self.dist2)
                {
                    return vec![WireModel::solid(
                        "chamfer_poly".into(),
                        entity_pts(&new_poly),
                        WireModel::CYAN,
                        false,
                    )];
                }
                vec![]
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["CHAMFER"]
}); // ChamferCommand
inventory::submit!(crate::command::CommandRegistration { names: &["FILLET"] }); // FilletCommand

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x1: f64, y1: f64, x2: f64, y2: f64, handle: u64) -> EntityType {
        let mut line = LineEnt::from_coords(x1, y1, 0.0, x2, y2, 0.0);
        line.common.handle = Handle::new(handle);
        EntityType::Line(line)
    }

    fn keywords(cmd: &dyn CadCommand) -> Vec<String> {
        cmd.options().into_iter().map(|o| o.keyword).collect()
    }

    /// The upper half of a radius-5 circle at (5, 0), scaled about the origin
    /// by an infinite factor: the center becomes (inf, NaN).
    fn overflowed_arc(sweep_sign: f64) -> ArcEnt {
        let mut arc = ArcEnt::new();
        arc.center = codec::types::Vector3::new(f64::INFINITY, f64::NAN, 0.0);
        arc.radius = f64::INFINITY;
        arc.start_angle = 0.0;
        arc.end_angle = sweep_sign * std::f64::consts::PI;
        arc
    }

    #[test]
    fn zero_radius_fillet_rejects_non_finite_geometry() {
        let line = LineEnt::from_coords(f64::NAN, f64::NAN, 0.0, f64::INFINITY, f64::INFINITY, 0.0);
        let arc = overflowed_arc(1.0);
        assert!(fillet_line_arc(&line, [5.0, 5.0], &arc, [5.0, 5.0], 0.0, 0.0).is_none());
        let other = overflowed_arc(-1.0);
        assert!(fillet_arc_arc(&arc, [5.0, 5.0], &other, [5.0, -5.0], 0.0, 0.0).is_none());
    }

    #[test]
    fn fillet_undo_option_appears_after_a_fillet_and_refreshes_from_document() {
        let mut command = FilletCommand::new(
            1.0,
            vec![line(0.0, 0.0, 10.0, 0.0, 1), line(0.0, 0.0, 0.0, 10.0, 2)],
        );
        assert_eq!(keywords(&command), ["P", "R", "T", "M"]);
        assert!(command.on_text_input("U").is_none(), "nothing to undo yet");
        assert!(matches!(command.on_text_input("M"), Some(CmdResult::NeedPoint)));
        assert_eq!(keywords(&command), ["P", "R", "T"], "Multiple is not offered twice");
        command.on_entity_pick(Handle::new(1), DVec3::new(5.0, 0.0, 0.0));
        assert!(matches!(
            command.on_entity_pick(Handle::new(2), DVec3::new(0.0, 5.0, 0.0)),
            CmdResult::ReplaceManyContinue(_)
        ));
        assert_eq!(keywords(&command), ["U", "P", "R", "T"]);
        assert!(matches!(
            command.on_text_input("U"),
            Some(CmdResult::UndoDocument)
        ));
        assert_eq!(keywords(&command), ["P", "R", "T"]);
        // The host hands the restored document back; the cache follows it.
        let mut doc = codec::Document::new();
        let _ = doc.add_entity(line(0.0, 0.0, 20.0, 0.0, 7));
        command.on_document_undone(&doc);
        assert_eq!(command.all_entities.len(), 1);
        assert_eq!(command.all_entities[0].common().handle, Handle::new(7));
    }

    /// Filleting two open polylines at their end segments joins them into
    /// one polyline through the fillet arc, as the reference does (#1422).
    #[test]
    fn fillet_joins_two_polylines_into_one() {
        let polyline = |handle: u64, points: &[[f64; 2]]| {
            let mut p = LwPolyline::new();
            for &[x, y] in points {
                p.add_point(codec::types::Vector2::new(x, y));
            }
            p.common.handle = Handle::new(handle);
            EntityType::LwPolyline(p)
        };
        let first = [[0.0, 10.0], [0.0, 3.0]];
        // The fillet on the second one's end segment, or on a middle one:
        // the part past the fillet point is dropped.
        let seconds: [&[[f64; 2]]; 2] = [
            &[[3.0, 0.0], [10.0, 0.0], [10.0, 5.0]],
            &[[3.0, -5.0], [3.0, 0.0], [10.0, 0.0], [10.0, 5.0]],
        ];
        // Each polyline drawn either way round.
        for (second, flip_first, flip_second) in seconds.iter().flat_map(|second| {
            [(false, false), (true, false), (false, true), (true, true)]
                .map(|(a, b)| (*second, a, b))
        }) {
            let mut a = first.to_vec();
            let mut b = second.to_vec();
            if flip_first {
                a.reverse();
            }
            if flip_second {
                b.reverse();
            }
            let all = vec![polyline(1, &a), polyline(2, &b)];
            let mut command = FilletCommand::new(1.0, all);
            let _ = command.on_entity_pick(Handle::new(1), DVec3::new(0.0, 6.0, 0.0));
            let CmdResult::ReplaceMany(replacements, _) =
                command.on_entity_pick(Handle::new(2), DVec3::new(6.0, 0.0, 0.0))
            else {
                panic!("FILLET should replace the polylines");
            };
            let joined = replacements
                .iter()
                .find(|(handle, _)| *handle == Handle::new(1))
                .expect("the first polyline is replaced");
            assert!(replacements
                .iter()
                .any(|(handle, entities)| *handle == Handle::new(2) && entities.is_empty()));
            let [EntityType::LwPolyline(p)] = joined.1.as_slice() else {
                panic!("one polyline");
            };
            let points: Vec<[f64; 2]> =
                p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
            let mut expected = vec![[0.0, 10.0], [0.0, 1.0], [1.0, 0.0], [10.0, 0.0], [10.0, 5.0]];
            // The polyline runs whichever way the first one ran.
            let fillet_start = if flip_first {
                expected.reverse();
                2
            } else {
                1
            };
            assert_eq!(points.len(), expected.len(), "{flip_first} {flip_second}: {points:?}");
            for (got, want) in points.iter().zip(&expected) {
                assert!(
                    (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
                    "{flip_first} {flip_second}: {points:?}"
                );
            }
            // Only the fillet segment is curved, turning the same way both ways.
            for (k, v) in p.vertices.iter().enumerate() {
                let curved = v.bulge.abs() > 1e-12;
                assert_eq!(curved, k == fillet_start, "{flip_first} {flip_second}: {k}");
            }
            let turn = p.vertices[fillet_start].bulge.signum();
            assert_eq!(turn, if flip_first { -1.0 } else { 1.0 }, "{flip_first} {flip_second}");
        }
    }

    #[test]
    fn no_trim_fillet_adds_only_an_arc_and_keeps_source_handles() {
        let first = Handle::new(1);
        let second = Handle::new(2);
        let original = vec![line(0.0, 0.0, 10.0, 0.0, 1), line(0.0, 0.0, 0.0, 10.0, 2)];
        let mut command = FilletCommand::new(1.0, original);
        command.trim = false;
        command.on_text_input("M");
        assert!(matches!(
            command.on_entity_pick(first, DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let addition = match command.on_entity_pick(second, DVec3::new(0.0, 5.0, 0.0)) {
            CmdResult::CommitEntities(mut entities) => {
                assert_eq!(entities.len(), 1);
                entities.pop().unwrap()
            }
            _ => panic!("no-trim FILLET should add one arc"),
        };
        assert!(matches!(addition, EntityType::Arc(_)));
        assert!(command
            .all_entities
            .iter()
            .any(|entity| entity.common().handle == first));
        assert!(command
            .all_entities
            .iter()
            .any(|entity| entity.common().handle == second));
        assert!(command
            .all_entities
            .iter()
            .any(|entity| matches!(entity, EntityType::Arc(arc) if arc.common.handle.is_null())));

        let mut committed = addition;
        committed.common_mut().handle = Handle::new(10);
        command.on_entities_committed(&[committed]);
        assert!(command
            .all_entities
            .iter()
            .any(|entity| entity.common().handle == Handle::new(10)));
        let source_lines: Vec<_> = command
            .all_entities
            .iter()
            .filter_map(|entity| match entity {
                EntityType::Line(line) => Some((line.start, line.end)),
                _ => None,
            })
            .collect();
        assert_eq!(
            source_lines[0],
            (Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0))
        );
        assert_eq!(
            source_lines[1],
            (Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 10.0, 0.0))
        );
    }

    #[test]
    fn chamfer_multiple_keeps_going_and_single_ends() {
        let lines = || vec![line(0.0, 0.0, 10.0, 0.0, 1), line(0.0, 0.0, 0.0, 10.0, 2)];
        let mut single = ChamferCommand::new(1.0, lines());
        assert_eq!(keywords(&single), ["D", "M", "N"]);
        single.on_entity_pick(Handle::new(1), DVec3::new(5.0, 0.0, 0.0));
        assert!(matches!(
            single.on_entity_pick(Handle::new(2), DVec3::new(0.0, 5.0, 0.0)),
            CmdResult::ReplaceMany(..)
        ));

        let mut multi = ChamferCommand::new(1.0, lines());
        assert!(matches!(
            multi.on_text_input("M"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            keywords(&multi),
            ["D", "N"],
            "Multiple is not offered twice"
        );
        multi.on_entity_pick(Handle::new(1), DVec3::new(5.0, 0.0, 0.0));
        match multi.on_entity_pick(Handle::new(2), DVec3::new(0.0, 5.0, 0.0)) {
            CmdResult::ReplaceManyContinue(replacements) => {
                // The chamfer line rides along with the first line's replacement.
                assert_eq!(replacements[0].1.len(), 2);
                assert_eq!(replacements[1].1.len(), 1);
            }
            _ => panic!("multiple mode should keep the command active"),
        }
        assert_eq!(keywords(&multi), ["U", "D", "N"]);
        assert!(matches!(
            multi.on_text_input("U"),
            Some(CmdResult::UndoDocument)
        ));
    }

    #[test]
    fn no_trim_chamfer_adds_only_the_cut_line_and_keeps_source_handles() {
        let first = Handle::new(1);
        let second = Handle::new(2);
        let lines = || vec![line(0.0, 0.0, 10.0, 0.0, 1), line(0.0, 0.0, 0.0, 10.0, 2)];

        let mut single = ChamferCommand::new(1.0, lines());
        single.dist2 = 1.0;
        single.trim = false;
        assert!(!single.trim);
        assert!(matches!(
            single.on_entity_pick(first, DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let addition = match single.on_entity_pick(second, DVec3::new(0.0, 5.0, 0.0)) {
            CmdResult::CommitEntitiesAndExit(mut entities) => {
                assert_eq!(entities.len(), 1);
                entities.pop().unwrap()
            }
            _ => panic!("no-trim CHAMFER should add one line and exit"),
        };
        let EntityType::Line(cut) = &addition else {
            panic!("no-trim CHAMFER should add a line")
        };
        assert!((cut.start.x - 1.0).abs() < 1e-9 && cut.start.y.abs() < 1e-9);
        assert!(cut.end.x.abs() < 1e-9 && (cut.end.y - 1.0).abs() < 1e-9);
        assert!(single
            .all_entities
            .iter()
            .any(|entity| entity.common().handle == first));
        assert!(single
            .all_entities
            .iter()
            .any(|entity| entity.common().handle == second));

        let mut multi = ChamferCommand::new(1.0, lines());
        multi.dist2 = 1.0;
        assert!(matches!(
            multi.on_text_input("M"),
            Some(CmdResult::NeedPoint)
        ));
        multi.trim = false;
        assert!(matches!(
            multi.on_entity_pick(first, DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        match multi.on_entity_pick(second, DVec3::new(0.0, 5.0, 0.0)) {
            CmdResult::CommitEntities(entities) => assert_eq!(entities.len(), 1),
            _ => panic!("multiple no-trim CHAMFER should keep running"),
        }
        assert!(matches!(multi.step, ChamferStep::First));
        let mut committed = addition;
        committed.common_mut().handle = Handle::new(10);
        multi.on_entities_committed(&[committed]);
        assert!(multi
            .all_entities
            .iter()
            .any(|entity| entity.common().handle == Handle::new(10)));
    }

    #[test]
    fn no_trim_chamfer_refuses_polyline_corners() {
        for token in ["N", "NOTRIM", "NO TRIM", "TRIM=FALSE"] {
            let mut command = ChamferCommand::new(1.0, Vec::new());
            assert!(matches!(
                command.on_text_input(token),
                Some(CmdResult::NeedPoint)
            ));
            assert!(!command.trim, "{token}");
        }
        for token in ["T", "TRIM", "TRIM=TRUE"] {
            let mut command = ChamferCommand::new(1.0, Vec::new());
            assert!(matches!(
                command.on_text_input(token),
                Some(CmdResult::NeedPoint)
            ));
            assert!(command.trim, "{token}");
        }

        let mut poly = LwPolyline::new();
        poly.add_point(codec::types::Vector2::new(0.0, 0.0));
        poly.add_point(codec::types::Vector2::new(10.0, 0.0));
        poly.add_point(codec::types::Vector2::new(10.0, 10.0));
        poly.common.handle = Handle::new(3);
        let mut command = ChamferCommand::new(1.0, vec![EntityType::LwPolyline(poly)]);
        command.trim = false;
        assert!(matches!(
            command.on_entity_pick(Handle::new(3), DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::ReportError(_)
        ));
        assert!(matches!(command.step, ChamferStep::First));
    }

    #[test]
    fn fillet_continues_with_replacement_entities() {
        let first = Handle::new(1);
        let second = Handle::new(2);
        let mut command = FilletCommand::new(
            1.0,
            vec![line(0.0, 0.0, 10.0, 0.0, 1), line(0.0, 0.0, 0.0, 10.0, 2)],
        );
        command.on_text_input("M");

        assert!(matches!(
            command.on_entity_pick(first, DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let replacements = match command.on_entity_pick(second, DVec3::new(0.0, 5.0, 0.0)) {
            CmdResult::ReplaceManyContinue(replacements) => replacements,
            _ => panic!("fillet should replace entities and stay active"),
        };

        let mut next_handle = 10;
        for (old, entities) in replacements {
            let handles: Vec<_> = entities
                .iter()
                .map(|_| {
                    let handle = Handle::new(next_handle);
                    next_handle += 1;
                    handle
                })
                .collect();
            command.on_entity_replaced(old, &handles);
        }

        assert!(matches!(&command.step, FilletStep::First));
        assert!(matches!(
            command.on_entity_pick(Handle::new(10), DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert!(matches!(&command.step, FilletStep::Second { .. }));
    }
}
