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

use acadrust::entities::{Arc as ArcEnt, Line as LineEnt, LwPolyline};

// Shared plane geometry, from cadkernel via the local adapters.
use super::geom;
use super::geom::{arc_points as arc_pts, line_line as ll, normalize_angle as norm_angle};
use cadkernel::geom2d::{
    circle_circle_points as circle_circle_pts, fillet_between_rays, fillets_between, line_circle,
    Arc as KernelArc, Curve as KernelCurve, Fillet as KernelFillet,
    Line as KernelLine, Tolerance,
};
use acadrust::entities::EntityCommon;
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use crate::t;

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

    // Intersection of infinite lines
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

    // Angle between the two keep-directions
    let cos_a = (dir1[0] * dir2[0] + dir1[1] * dir2[1]).clamp(-1.0, 1.0);
    let angle = cos_a.acos();

    // Lines are parallel / anti-parallel
    if angle < 1e-6 || (angle - std::f64::consts::PI).abs() < 1e-6 {
        return None;
    }

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
fn trim_to_xy(
    orig: &LineEnt,
    tangent: [f64; 2],
    dir: [f64; 2],
    unit: [f64; 2],
) -> Option<LineEnt> {
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
}

impl FilletEntity {
    fn from_entity(e: &EntityType) -> Option<Self> {
        match e {
            EntityType::Line(l) => Some(Self::Line(l.clone())),
            EntityType::Arc(a) => Some(Self::Arc(a.clone())),
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
        }
    }

    fn elevation(&self) -> f64 {
        match self {
            Self::Line(l) => l.start.z,
            Self::Arc(a) => a.center.z,
            Self::LwPoly { poly, .. } => poly.elevation,
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
            // Adjacent segments share a corner vertex — fillet that corner.
            let (low, high) = if *s1 < *s2 { (*s1, *s2) } else { (*s2, *s1) };
            let n = p1.vertices.len();
            // The wrap-around corner of a closed polyline joins the last
            // segment (n-1) and the first (0); their shared vertex is v0.
            let wrap = p1.is_closed && low == 0 && high == n.saturating_sub(1);
            // Segments must be consecutive, or the wrap-around pair above.
            if !(high == low + 1 || wrap) {
                return None;
            }
            // `before_seg` ends at the shared corner vertex, `after_seg` starts
            // there. For the wrap corner that is seg n-1 → v0 → seg 0; for a
            // normal corner it is seg low → v[high] → seg high.
            let (before_seg, after_seg, corner_idx) = if wrap {
                (high, low, 0)
            } else {
                (low, high, high)
            };
            let l1 = lwpoly_seg_as_line(p1, before_seg);
            let l2 = lwpoly_seg_as_line(p1, after_seg);
            // Re-map click to whichever segment each was picked on.
            let (c1, c2) = if *s1 == before_seg {
                (click1, click2)
            } else {
                (click2, click1)
            };
            match compute_fillet(&l1, c1, &l2, c2, radius)? {
                (EntityType::Line(tl1), EntityType::Line(tl2), maybe_arc) => {
                    let t1 = [tl1.end.x, tl1.end.y]; // trimmed end of seg before corner
                    let t2 = [tl2.start.x, tl2.start.y]; // trimmed start of seg after corner
                    let bulge = if let Some(EntityType::Arc(ref fa)) = maybe_arc {
                        // center from arc entity
                        compute_bulge(t1, t2, [fa.center.x, fa.center.y])
                    } else {
                        0.0 // r=0, sharp corner
                    };
                    let new_poly = lwpoly_replace_corner(p1, corner_idx, t1, t2, bulge);
                    let et = EntityType::LwPolyline(new_poly);
                    // Return same rebuilt poly for both slots; caller uses only h1.
                    Some((et.clone(), et, None))
                }
                _ => None,
            }
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
    let (ac, ar, a_start, a_end, _) = arc_geom(arc);

    // Intersection of infinite line with the arc's circle
    let ts = line_circle(p1, u, ac, ar);

    if radius < 1e-9 {
        // r=0: trim to intersection (nearest to each click)
        let t_best = ts.iter().copied().min_by(|a, b| {
            let da = (p1[0] + a * u[0] - click_line[0]).powi(2)
                + (p1[1] + a * u[1] - click_line[1]).powi(2);
            let db = (p1[0] + b * u[0] - click_line[0]).powi(2)
                + (p1[1] + b * u[1] - click_line[1]).powi(2);
            da.partial_cmp(&db).unwrap()
        })?;
        let ix = p1[0] + t_best * u[0];
        let iy = p1[1] + t_best * u[1];

        // Trim line to intersection
        let new_line = trim_line_to_point(line, [ix, iy], click_line)?;
        // Trim arc to intersection angle
        let i_angle = arc_angle_at(ac, [ix, iy]);
        let i_clamped = clamp_angle_to_arc(i_angle, a_start, a_end);
        let arc_click_angle = arc_angle_at(ac, click_arc);
        let arc_click_clamped = clamp_angle_to_arc(arc_click_angle, a_start, a_end);
        // Keep the arc side from i_clamped toward the click
        let new_arc = if {
            let sp_to_click = arc_span(i_clamped, arc_click_clamped);
            let sp_click_to_end = arc_span(arc_click_clamped, a_end);
            sp_to_click <= sp_click_to_end
        } {
            trim_arc(arc, i_clamped, a_end)
        } else {
            trim_arc(arc, a_start, i_clamped)
        };
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
        // The touch on the arc has to be on the drawn part of it, not on the
        // rest of the circle it belongs to.
        let tp_arc_angle = arc_angle_at(ac, fillet.tangent2);
        let tp_arc_clamped = clamp_angle_to_arc(tp_arc_angle, a_start, a_end);
        if (norm_angle(tp_arc_angle) - norm_angle(tp_arc_clamped)).abs() > 0.01 {
            continue;
        }
        let Some(new_line) = trim_line_to_point(line, fillet.tangent1, click_line) else {
            continue;
        };
        // Keep the side of the arc the click is on.
        let arc_click_rel = (arc_angle_at(ac, click_arc) - a_start).rem_euclid(TAU);
        let tp_arc_rel = (tp_arc_clamped - a_start).rem_euclid(TAU);
        let new_arc = if tp_arc_rel <= arc_click_rel {
            trim_arc(arc, tp_arc_clamped, a_end)
        } else {
            trim_arc(arc, a_start, tp_arc_clamped)
        };
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
    let (c1, r1, s1, e1, _) = arc_geom(a1);
    let (c2, r2, s2, e2, _) = arc_geom(a2);

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
                .partial_cmp(&(b[0] - cx).hypot(b[1] - cy))
                .unwrap()
        })?;

        let ia1 = arc_angle_at(c1, ip);
        let ia2 = arc_angle_at(c2, ip);
        let ic1 = clamp_angle_to_arc(ia1, s1, e1);
        let ic2 = clamp_angle_to_arc(ia2, s2, e2);

        let ca1 = clamp_angle_to_arc(arc_angle_at(c1, click1), s1, e1);
        let ca2 = clamp_angle_to_arc(arc_angle_at(c2, click2), s2, e2);

        let new_a1 = if (ic1 - s1).rem_euclid(TAU) <= (ca1 - s1).rem_euclid(TAU) {
            trim_arc(a1, ic1, e1)
        } else {
            trim_arc(a1, s1, ic1)
        };
        let new_a2 = if (ic2 - s2).rem_euclid(TAU) <= (ca2 - s2).rem_euclid(TAU) {
            trim_arc(a2, ic2, e2)
        } else {
            trim_arc(a2, s2, ic2)
        };
        return Some((EntityType::Arc(new_a1), EntityType::Arc(new_a2), None));
    }

    // Same construction as the line-and-arc case, from the same place: the
    // centre is where an offset of one circle crosses an offset of the other.
    for fillet in ranked_fillets(&arc_curve(a1), click1, &arc_curve(a2), click2, radius) {
        // Both touches have to be on the drawn parts of their arcs.
        let tp1_angle = arc_angle_at(c1, fillet.tangent1);
        let tp2_angle = arc_angle_at(c2, fillet.tangent2);
        let tc1 = clamp_angle_to_arc(tp1_angle, s1, e1);
        let tc2 = clamp_angle_to_arc(tp2_angle, s2, e2);
        if (norm_angle(tp1_angle) - norm_angle(tc1)).abs() > 0.01
            || (norm_angle(tp2_angle) - norm_angle(tc2)).abs() > 0.01
        {
            continue;
        }

        let ca1 = clamp_angle_to_arc(arc_angle_at(c1, click1), s1, e1);
        let ca2 = clamp_angle_to_arc(arc_angle_at(c2, click2), s2, e2);
        let new_a1 = if (tc1 - s1).rem_euclid(TAU) <= (ca1 - s1).rem_euclid(TAU) {
            trim_arc(a1, tc1, e1)
        } else {
            trim_arc(a1, s1, tc1)
        };
        let new_a2 = if (tc2 - s2).rem_euclid(TAU) <= (ca2 - s2).rem_euclid(TAU) {
            trim_arc(a2, tc2, e2)
        } else {
            trim_arc(a2, s2, tc2)
        };
        return Some((
            EntityType::Arc(new_a1),
            EntityType::Arc(new_a2),
            Some(EntityType::Arc(fillet_entity(&fillet, radius, z, &a1.common))),
        ));
    }
    None
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
    WaitingForRadius,
    Second {
        h1: Handle,
        e1: FilletEntity,
        click1: [f64; 2],
    },
}

pub struct FilletCommand {
    radius: f64,
    step: FilletStep,
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    /// First-object pick to restore after a radius entry made mid-selection
    /// (i.e. "R" pressed after the first object was already picked), so the
    /// command resumes at the second pick instead of restarting selection.
    resume_second: Option<(Handle, FilletEntity, [f64; 2])>,
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
            step: FilletStep::First,
            all_entities,
            entity_index,
            resume_second: None,
        }
    }

    /// Switch to the radius sub-step, remembering the first pick (if any) so
    /// it can be restored afterwards.
    fn enter_radius_substep(&mut self) {
        self.resume_second = if let FilletStep::Second { h1, e1, click1 } = &self.step {
            Some((*h1, e1.clone(), *click1))
        } else {
            None
        };
        self.step = FilletStep::WaitingForRadius;
    }

    /// Leave the radius sub-step, resuming the second pick when a first object
    /// was already chosen, otherwise restarting at the first pick.
    fn resume_after_radius(&mut self) {
        self.step = match self.resume_second.take() {
            Some((h1, e1, click1)) => FilletStep::Second { h1, e1, click1 },
            None => FilletStep::First,
        };
    }
}

impl CadCommand for FilletCommand {
    fn name(&self) -> &'static str {
        "FILLET"
    }

    fn prompt(&self) -> String {
        match &self.step {
            FilletStep::First => crate::tf!(
                "FILLET  Select first object (Line/Arc/LwPolyline)  [R={:.4}]:",
                self.radius
            )
            .into_owned(),
            FilletStep::WaitingForRadius => {
                crate::tf!("FILLET  Enter fillet radius <{:.4}>:", self.radius).into_owned()
            }
            FilletStep::Second { .. } => {
                crate::tf!(
                    "FILLET  Select second object (Line/Arc/LwPolyline)  [R={:.4}]:",
                    self.radius
                )
                .into_owned()
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.step {
            // Offer the radius keyword while selecting objects; the "R" token is
            // already handled by on_text_input.
            FilletStep::First | FilletStep::Second { .. } => vec![CmdOption::new("Radius", "R")],
            FilletStep::WaitingForRadius => vec![],
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, FilletStep::WaitingForRadius)
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
            FilletStep::First | FilletStep::Second { .. } => {
                let t = text.trim();
                let upper = t.to_uppercase();
                // "R" alone → enter sub-step to collect radius
                if upper == "R" {
                    self.enter_radius_substep();
                    return Some(CmdResult::NeedPoint);
                }
                // "R 5.0" inline shorthand
                if upper.starts_with('R') {
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
        }
    }

    fn needs_entity_pick(&self) -> bool {
        !matches!(self.step, FilletStep::WaitingForRadius)
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let click = [pt.x as f64, pt.y as f64]; // drawing plane is world XY

        match &self.step {
            FilletStep::WaitingForRadius => return CmdResult::NeedPoint,
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
                            let mut additions = vec![];
                            if let Some(arc) = maybe_arc {
                                additions.push(arc);
                            }
                            if same_entity {
                                // Corner fillet: both results are the same rebuilt poly.
                                CmdResult::ReplaceMany(vec![(h1, vec![new_e1])], additions)
                            } else {
                                CmdResult::ReplaceMany(
                                    vec![(h1, vec![new_e1]), (handle, vec![new_e2])],
                                    additions,
                                )
                            }
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
            FilletStep::WaitingForRadius => vec![],
            FilletStep::First => {
                let pts = self
                    .entity_index
                    .get(&self.all_entities, handle)
                    .and_then(|e| match e {
                        EntityType::LwPolyline(p) => Some(lwpoly_seg_hover_pts(p, click)),
                        _ => FilletEntity::from_entity(e).map(|fe| entity_pts(&fe.to_entity_type())),
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
                        let mut out = vec![
                            WireModel::solid(
                                "fillet_e1".into(),
                                entity_pts(&new_e1),
                                WireModel::CYAN,
                                false,
                            ),
                            WireModel::solid(
                                "fillet_e2".into(),
                                entity_pts(&new_e2),
                                WireModel::CYAN,
                                false,
                            ),
                        ];
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

pub struct ChamferCommand {
    dist1: f64,
    dist2: f64,
    step: ChamferStep,
    all_entities: Vec<EntityType>,
    entity_index: ModifyEntityIndex,
    /// First-object pick (line or polyline segment) to restore after a
    /// distance entry made mid-selection, so the command resumes at the
    /// second pick instead of restarting selection.
    resume_pick: Option<ChamferStep>,
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
            step: ChamferStep::First,
            all_entities,
            entity_index,
            resume_pick: None,
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
            // Offer the distance keyword while selecting objects; the "D" token
            // is already handled by on_text_input.
            ChamferStep::First
            | ChamferStep::Second { .. }
            | ChamferStep::SecondPoly { .. } => vec![CmdOption::new(t!("Distance").as_ref(), "D")],
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
            ChamferStep::First
            | ChamferStep::Second { .. }
            | ChamferStep::SecondPoly { .. } => {
                let t = text.trim();
                let upper = t.to_uppercase();
                // "D" alone → enter sub-step to collect distances
                if upper == "D" {
                    self.enter_dist_substep();
                    return Some(CmdResult::NeedPoint);
                }
                // "D 5.0" or "D 5.0 3.0" inline shorthand
                if upper.starts_with('D') {
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
                match self
                    .entity_index.get(&self.all_entities, handle)
                {
                    Some(EntityType::Line(l)) => {
                        self.step = ChamferStep::Second {
                            h1: handle,
                            l1: l.clone(),
                            click1: click,
                        };
                    }
                    Some(EntityType::LwPolyline(p)) => {
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
                let h1 = *h1;
                let poly = poly.clone();
                let click1 = *click1;
                if handle != h1 {
                    // Chamfer corners only within the same polyline.
                    return CmdResult::NeedPoint;
                }
                match chamfer_lwpoly_corner(&poly, click1, self.dist1, click, self.dist2) {
                    Some(new_poly) => CmdResult::ReplaceMany(vec![(h1, vec![new_poly])], vec![]),
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
                        Some((new_l1, new_l2, chamfer_line)) => CmdResult::ReplaceMany(
                            vec![(h1, vec![new_l1]), (handle, vec![new_l2])],
                            vec![chamfer_line],
                        ),
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
inventory::submit!(crate::command::CommandRegistration { names: &["CHAMFER"] });  // ChamferCommand
inventory::submit!(crate::command::CommandRegistration { names: &["FILLET"] });  // FilletCommand
