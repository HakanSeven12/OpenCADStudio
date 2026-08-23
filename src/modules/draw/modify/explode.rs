// Explode tool — ribbon definition + command implementation.
//
// Command:  EXPLODE (X)
//   EXPLODE: Breaks compound objects into their constituent simple entities.
//
//   Supported:
//     LwPolyline  → Lines (straight segments) + Arcs (bulge segments)
//     Polyline2D  → Lines + Arcs
//     Polyline3D  → Lines
//     Polyline    → Lines
//     Insert      → constituent entities (via acadrust explode_from_document)
//     MLine       → Lines (spine + offset lines per miter direction)
//     Dimension   → Lines (extension + dimension + arrows) + Text
//
//   Unsupported entity types are skipped silently.

use super::geom::normalize_angle as norm_angle;

use acadrust::entities::EntityCommon;
use acadrust::entities::{
    Arc as ArcEnt, Block, BlockEnd, Circle as CircleEnt, Dimension, Line as LineEnt, LwPolyline,
    MLine,
};
use acadrust::entities::{Polyline, Polyline2D};
use acadrust::tables::BlockRecord;
use acadrust::types::Vector3;
use acadrust::{CadDocument, EntityType, Handle};

use crate::command::{CadCommand, CmdResult};
use crate::entities::curve::lwpolyline_world_xy;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use glam::DVec3;
use crate::t;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "EXPLODE",
        label: "Explode",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/explode.svg")),
        event: ModuleEvent::Command("EXPLODE".to_string()),
    }
}

// ── Geometry helpers ────────────────────────────────────────────────────────

/// Explode just the polyline family (LwPolyline / Polyline / Polyline2D /
/// Polyline3D) into Line + Arc segments. No document needed — used where a
/// polyline must be treated as its constituent edges (e.g. TRIM boundaries).
/// Returns empty for any other entity type.
pub fn explode_polyline_segments(entity: &EntityType) -> Vec<EntityType> {
    match entity {
        EntityType::LwPolyline(p) => explode_lwpolyline(p),
        EntityType::Polyline2D(p) => explode_polyline2d(p),
        EntityType::Polyline(p) => explode_polyline(p),
        EntityType::Polyline3D(p) => explode_polyline3d(p),
        _ => vec![],
    }
}

/// Decompose an entity into its constituent simple entities.
/// Returns an empty vec if the entity cannot be exploded.
pub fn explode_entity(entity: &EntityType, document: &CadDocument) -> Vec<EntityType> {
    match entity {
        EntityType::Line(line) => {
            let Some(association) = acadrust::entities::CenterMarkAssociation::read(
                &line.common.extended_data,
            ) else {
                return vec![];
            };
            crate::scene::centermark::mark_segments(&association)
                .into_iter()
                .map(|segment| {
                    let mut common = line.common.clone();
                    common.handle = Handle::NULL;
                    acadrust::entities::CenterMarkAssociation::remove(&mut common.extended_data);
                    EntityType::Line(LineEnt {
                        common,
                        start: Vector3::new(segment[0].x, segment[0].y, segment[0].z),
                        end: Vector3::new(segment[1].x, segment[1].y, segment[1].z),
                        ..LineEnt::new()
                    })
                })
                .collect()
        }
        EntityType::LwPolyline(p) => explode_lwpolyline(p),
        EntityType::Polyline2D(p) => explode_polyline2d(p),
        EntityType::Polyline(p) => explode_polyline(p),
        EntityType::Polyline3D(p) => explode_polyline3d(p),
        EntityType::Insert(ins) => ins
            .explode_from_document(document)
            .into_iter()
            .map(normalize_insert_entity)
            .collect(),
        EntityType::MLine(ml) => explode_mline(ml),
        EntityType::Dimension(dim) => explode_dimension(dim, document),
        _ => vec![],
    }
}

fn explode_polyline(p: &Polyline) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = p.flags.is_closed();
    let n_segs = if closed { n } else { n - 1 };
    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let mut common = p.common.clone();
        common.handle = Handle::NULL;
        result.push(EntityType::Line(LineEnt {
            common,
            start: v0.location.clone(),
            end: v1.location.clone(),
            ..LineEnt::new()
        }));
    }
    result
}

fn explode_polyline3d(p: &acadrust::entities::Polyline3D) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = p.is_closed();
    let n_segs = if closed { n } else { n - 1 };
    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let mut common = p.common.clone();
        common.handle = Handle::NULL;
        result.push(EntityType::Line(LineEnt {
            common,
            start: v0.position.clone(),
            end: v1.position.clone(),
            ..LineEnt::new()
        }));
    }
    result
}

fn explode_polyline2d(p: &Polyline2D) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = p.is_closed();
    let n_segs = if closed { n } else { n - 1 };
    let elevation = p.elevation;

    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let p0 = [v0.location.x, v0.location.y];
        let p1 = [v1.location.x, v1.location.y];

        if v0.bulge.abs() < 1e-10 {
            let mut common = p.common.clone();
            common.handle = Handle::NULL;
            result.push(EntityType::Line(LineEnt {
                common,
                start: Vector3::new(p0[0], p0[1], elevation),
                end: Vector3::new(p1[0], p1[1], elevation),
                ..LineEnt::new()
            }));
        } else if let Some(arc) = bulge_to_arc(p0, p1, v0.bulge, elevation, &p.common) {
            result.push(arc);
        }
    }
    result
}

pub fn normalize_insert_entity(mut entity: EntityType) -> EntityType {
    match &mut entity {
        EntityType::Ellipse(ell) => {
            let major_len = ell.major_axis_length();
            let full_span = {
                let mut span = ell.end_parameter - ell.start_parameter;
                if span < 0.0 {
                    span += std::f64::consts::TAU;
                }
                (span - std::f64::consts::TAU).abs() < 1e-6
            };
            if (ell.minor_axis_ratio - 1.0).abs() < 1e-6 && full_span {
                let mut circle = CircleEnt::new();
                circle.common = ell.common.clone();
                circle.center = ell.center;
                circle.radius = major_len;
                circle.normal = ell.normal;
                entity = EntityType::Circle(circle);
            }
        }
        _ => {}
    }

    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = Handle::NULL;
    entity
}

pub fn normalize_entity_for_block(entity: EntityType) -> EntityType {
    entity
}

fn explode_lwpolyline(p: &LwPolyline) -> Vec<EntityType> {
    let Some(p) = lwpolyline_world_xy(p) else {
        return vec![];
    };
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }

    let elevation = p.elevation;
    let n_segs = if p.is_closed { n } else { n - 1 };

    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];

        let p0 = [v0.location.x, v0.location.y];
        let p1 = [v1.location.x, v1.location.y];

        if v0.bulge.abs() < 1e-10 {
            // Straight segment → Line
            let mut common = p.common.clone();
            common.handle = Handle::NULL;
            let line = LineEnt {
                common,
                start: Vector3::new(p0[0], p0[1], elevation),
                end: Vector3::new(p1[0], p1[1], elevation),
                ..LineEnt::new()
            };
            result.push(EntityType::Line(line));
        } else {
            // Arc segment from bulge
            if let Some(arc) = bulge_to_arc(p0, p1, v0.bulge, elevation, &p.common) {
                result.push(arc);
            }
        }
    }
    result
}

/// Convert a polyline bulge segment to an Arc entity.
///   Arc angles are measured from the +X axis.
fn bulge_to_arc(
    p0: [f64; 2],
    p1: [f64; 2],
    bulge: f64,
    elevation: f64,
    common_src: &EntityCommon,
) -> Option<EntityType> {
    let ba = crate::entities::common::BulgeArc::from_bulge(p0, p1, bulge)?;

    // acadrust Arc is always CCW from start_angle to end_angle. Negative
    // bulge means the polyline goes p0→p1 the CW way around the centre,
    // which is the same circular arc traversed p1→p0 the CCW way — so
    // swap endpoints when bulge < 0.
    let (start_angle, end_angle) = if bulge > 0.0 {
        (norm_angle(ba.start_angle), norm_angle(ba.end_angle))
    } else {
        (norm_angle(ba.end_angle), norm_angle(ba.start_angle))
    };

    let mut common = common_src.clone();
    common.handle = Handle::NULL;

    let arc = ArcEnt {
        common,
        center: Vector3::new(ba.center[0], ba.center[1], elevation),
        radius: ba.radius,
        start_angle,
        end_angle,
        ..ArcEnt::new()
    };
    Some(EntityType::Arc(arc))
}

fn explode_mline(ml: &MLine) -> Vec<EntityType> {
    let n = ml.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = ml.flags.contains(acadrust::entities::MLineFlags::CLOSED);
    let scale = ml.scale_factor;
    let n_segs = if closed { n } else { n - 1 };
    let mut result = Vec::new();

    // Helper: build a Line from two Vector3 positions.
    let make_line = |common: &acadrust::entities::EntityCommon,
                     s: &acadrust::types::Vector3,
                     e: &acadrust::types::Vector3|
     -> EntityType {
        let mut c = common.clone();
        c.handle = Handle::NULL;
        EntityType::Line(LineEnt {
            common: c,
            start: s.clone(),
            end: e.clone(),
            ..LineEnt::new()
        })
    };

    // For each segment, emit the center-spine line and the two ±scale/2 offset lines.
    for i in 0..n_segs {
        let v0 = &ml.vertices[i];
        let v1 = &ml.vertices[(i + 1) % n];

        // Spine line
        result.push(make_line(&ml.common, &v0.position, &v1.position));

        if scale.abs() > 1e-9 {
            let half = scale * 0.5;
            for &sign in &[-1.0_f64, 1.0_f64] {
                let off = half * sign;
                // Use miter direction at each vertex to offset the endpoints.
                let s = Vector3::new(
                    v0.position.x + v0.miter.x * off,
                    v0.position.y + v0.miter.y * off,
                    v0.position.z + v0.miter.z * off,
                );
                let e = Vector3::new(
                    v1.position.x + v1.miter.x * off,
                    v1.position.y + v1.miter.y * off,
                    v1.position.z + v1.miter.z * off,
                );
                result.push(make_line(&ml.common, &s, &e));
            }
        }
    }

    result
}

// ── Dimension explode ──────────────────────────────────────────────────────

/// Convert a Dimension entity into Lines (geometry) + Text (label).
/// A NULL-handle line segment for a baked dimension block.
fn dim_seg(a: Vector3, b: Vector3, common: &acadrust::entities::EntityCommon) -> EntityType {
    let mut c = common.clone();
    c.handle = Handle::NULL;
    EntityType::Line(LineEnt {
        common: c,
        start: a,
        end: b,
        ..LineEnt::new()
    })
}

/// A dimension-line terminator at `tip`, body extending back along the unit
/// vector `(dx,dy)` (toward the dim line). When DIMTSZ>0 it's an oblique 45°
/// tick; otherwise a closed *filled* arrowhead (DXF SOLID) of length DIMASZ —
/// matching the style so a baked block keeps the original arrow type rather
/// than a generic open stroke.
fn dim_terminator(
    tip: Vector3,
    dx: f64,
    dy: f64,
    arrow: &crate::scene::convert::tessellate::ArrowKind,
    common: &acadrust::entities::EntityCommon,
) -> Vec<EntityType> {
    use crate::scene::convert::tessellate::ArrowKind as A;
    let (dx, dy) = norm2(dx, dy, 1.0, 0.0);
    let (px, py) = (-dy, dx);
    // Point at `along` (down the dim line from the tip) and `perp` (sideways).
    let pt = |along: f64, perp: f64| {
        Vector3::new(tip.x + dx * along + px * perp, tip.y + dy * along + py * perp, tip.z)
    };
    let mut out: Vec<EntityType> = Vec::new();
    let tri = |a: Vector3, b: Vector3, c: Vector3, out: &mut Vec<EntityType>| {
        let mut s = acadrust::entities::Solid::triangle(a, b, c);
        s.common = common.clone();
        s.common.handle = Handle::NULL;
        out.push(EntityType::Solid(s));
    };
    match arrow {
        A::None => {}
        A::Triangle { size, filled, size_mul } => {
            let size = (*size * *size_mul) as f64;
            let hw = size / 6.0;
            let (l, r) = (pt(size, hw), pt(size, -hw));
            if *filled {
                tri(tip, l, r, &mut out);
            } else {
                out.push(dim_seg(tip, l, common));
                out.push(dim_seg(l, r, common));
                out.push(dim_seg(r, tip, common));
            }
        }
        A::Tick { size } => {
            let s = *size as f64;
            let (ox, oy) = (dx + px, dy + py);
            let m = (ox * ox + oy * oy).sqrt().max(1e-9);
            let (ox, oy) = (ox / m * s, oy / m * s);
            out.push(dim_seg(
                Vector3::new(tip.x - ox, tip.y - oy, tip.z),
                Vector3::new(tip.x + ox, tip.y + oy, tip.z),
                common,
            ));
        }
        A::Open { size, half_angle } => {
            let size = *size as f64;
            let hw = size * (*half_angle as f64).tan();
            out.push(dim_seg(tip, pt(size, hw), common));
            out.push(dim_seg(tip, pt(size, -hw), common));
        }
        A::Dot { size, filled } => {
            let r = *size as f64 * 0.5;
            terminator_circle(tip, r, *filled, common, &mut out);
        }
        A::Origin { size } => {
            terminator_circle(tip, *size as f64 * 0.25, true, common, &mut out);
            let half = *size as f64 * 0.5;
            out.push(dim_seg(pt(0.0, -half), pt(0.0, half), common));
        }
        A::Box_ { size, filled } => {
            let h = *size as f64 * 0.5;
            let (p1, p2, p3, p4) = (pt(-h, -h), pt(h, -h), pt(h, h), pt(-h, h));
            out.push(dim_seg(p1, p2, common));
            out.push(dim_seg(p2, p3, common));
            out.push(dim_seg(p3, p4, common));
            out.push(dim_seg(p4, p1, common));
            if *filled {
                tri(p1, p2, p3, &mut out);
                tri(p1, p3, p4, &mut out);
            }
        }
        A::Datum { size, filled } => {
            let half = *size as f64 * 0.5;
            let (ba, bb, apex) = (pt(0.0, half), pt(0.0, -half), pt(*size as f64, 0.0));
            out.push(dim_seg(ba, apex, common));
            out.push(dim_seg(apex, bb, common));
            out.push(dim_seg(bb, ba, common));
            if *filled {
                tri(ba, apex, bb, &mut out);
            }
        }
        A::Custom { size, lines, fill } => {
            let custom_pt = |point: &[f32; 3]| {
                let mut placed = pt(
                    -(point[0] as f64) * *size as f64,
                    -(point[1] as f64) * *size as f64,
                );
                placed.z += point[2] as f64 * *size as f64;
                placed
            };
            let mut previous = None;
            for point in lines {
                if point[0].is_nan() {
                    previous = None;
                    continue;
                }
                let current = custom_pt(point);
                if let Some(previous) = previous {
                    out.push(dim_seg(previous, current, common));
                }
                previous = Some(current);
            }
            for triangle in fill.chunks_exact(3) {
                tri(
                    custom_pt(&triangle[0]),
                    custom_pt(&triangle[1]),
                    custom_pt(&triangle[2]),
                    &mut out,
                );
            }
        }
    }
    out
}

/// A dot terminator: a 16-gon ring outline, optionally filled with a triangle fan.
fn terminator_circle(
    center: Vector3,
    r: f64,
    filled: bool,
    common: &acadrust::entities::EntityCommon,
    out: &mut Vec<EntityType>,
) {
    const N: usize = 16;
    let ring: Vec<Vector3> = (0..=N)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / N as f64;
            Vector3::new(center.x + a.cos() * r, center.y + a.sin() * r, center.z)
        })
        .collect();
    for w in ring.windows(2) {
        out.push(dim_seg(w[0], w[1], common));
    }
    if filled {
        for i in 0..N {
            let mut s = acadrust::entities::Solid::triangle(center, ring[i], ring[i + 1]);
            s.common = common.clone();
            s.common.handle = Handle::NULL;
            out.push(EntityType::Solid(s));
        }
    }
}

/// Center-mark cross at `center`, arm length |DIMCEN|. Empty when DIMCEN == 0.
fn dim_center_mark(
    center: Vector3,
    dimcen: f64,
    radius: f64,
    common: &acadrust::entities::EntityCommon,
) -> Vec<EntityType> {
    let mag = dimcen.abs();
    if mag < 1e-9 {
        return Vec::new();
    }
    let v = |x: f64, y: f64| Vector3::new(x, y, center.z);
    // Central "+" mark.
    let mut out = vec![
        dim_seg(v(center.x - mag, center.y), v(center.x + mag, center.y), common),
        dim_seg(v(center.x, center.y - mag), v(center.x, center.y + mag), common),
    ];
    // Negative DIMCEN adds centre LINES: four radial strokes spanning the edge.
    if dimcen < 0.0 && radius > mag + 1e-6 {
        let inner = (radius - mag).max(0.0);
        let outer = radius + mag;
        out.push(dim_seg(v(center.x + inner, center.y), v(center.x + outer, center.y), common));
        out.push(dim_seg(v(center.x - inner, center.y), v(center.x - outer, center.y), common));
        out.push(dim_seg(v(center.x, center.y + inner), v(center.x, center.y + outer), common));
        out.push(dim_seg(v(center.x, center.y - inner), v(center.x, center.y - outer), common));
    }
    out
}

/// Arc from angle `a1` to `a2` (radians, swept the short way) at `radius` about
/// `center`, approximated by straight chords (world f64).
fn dim_arc_segs(
    center: Vector3,
    radius: f64,
    a1: f64,
    a2: f64,
    common: &acadrust::entities::EntityCommon,
) -> Vec<EntityType> {
    use std::f64::consts::PI;
    let mut sweep = a2 - a1;
    while sweep > PI {
        sweep -= 2.0 * PI;
    }
    // Half-open at -PI so an exact 180° span sweeps the same way as the
    // terminator code and the live render (CCW), not the opposite semicircle.
    while sweep <= -PI {
        sweep += 2.0 * PI;
    }
    dim_arc_segs_with_sweep(center, radius, a1, sweep, common)
}

fn dim_arc_segs_with_sweep(
    center: Vector3,
    radius: f64,
    start: f64,
    sweep: f64,
    common: &acadrust::entities::EntityCommon,
) -> Vec<EntityType> {
    use std::f64::consts::PI;
    let steps = 12usize.max((sweep.abs() / (PI / 36.0)).ceil() as usize);
    let pt = |a: f64| Vector3::new(center.x + radius * a.cos(), center.y + radius * a.sin(), center.z);
    let mut out = Vec::new();
    let mut prev = pt(start);
    for i in 1..=steps {
        let cur = pt(start + sweep * (i as f64 / steps as f64));
        out.push(dim_seg(prev, cur, common));
        prev = cur;
    }
    out
}

/// Resolved, DIMSCALE-applied dimension metrics for baking.
struct DimMetrics {
    dimasz: f64,
    dimcen: f64,
    dimexo: f64,
    dimexe: f64,
    dimtsz: f64,
    dimdle: f64,
    dimfxl: f64,
    dimfxlon: bool,
    dimse1: bool,
    dimse2: bool,
    dimsd1: bool,
    dimsd2: bool,
    dimsoxd: bool,
    dimclrd: i16,
    dimclre: i16,
    dimclrt: i16,
    dimlwd: i16,
    dimlwe: i16,
    /// Resolved terminator shapes for the first / second end (DIMTSZ tick,
    /// DIMBLK/DIMBLK1/DIMBLK2 per DIMSAH, else closed-filled), so the bake
    /// reproduces the style's actual arrow type.
    arrow1: crate::scene::convert::tessellate::ArrowKind,
    arrow2: crate::scene::convert::tessellate::ArrowKind,
}

/// Metrics from the dim's style, mirroring what the live renderer applies, so a
/// baked block reproduces the same gaps, arrow type, suppression, colours and
/// lineweights: DIMASZ (arrow), DIMCEN (centre mark), DIMEXO/DIMEXE (extension
/// gap/overshoot), DIMTSZ (oblique tick; >0 = ticks not arrows), DIMDLE (dim
/// line overshoot past ticks), DIMSE1/2 + DIMSD1/2 (extension / dim-line
/// suppression), DIMCLRD/E/T (colours) and DIMLWD/E (lineweights).
fn dim_metrics(dim: &Dimension, doc: &CadDocument) -> DimMetrics {
    let name = dim.base().style_name.as_str();
    let style = doc.dim_styles.iter().find(|s| {
        s.name.eq_ignore_ascii_case(name)
            || (name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
    });
    let scale = style
        .map(|s| if s.dimscale > 1e-6 { s.dimscale } else { 1.0 })
        .unwrap_or(1.0);
    use crate::scene::convert::tessellate::{arrow_from_block, ArrowKind};
    let dimasz = style.map(|s| s.dimasz * scale).unwrap_or(0.18 * scale).max(1e-6);
    let dimtsz = style.map(|s| s.dimtsz * scale).unwrap_or(0.0);
    let asz = dimasz as f32;
    // Resolve the terminator shapes exactly like the live render: ticks when
    // DIMTSZ>0, otherwise the DIMBLK / DIMBLK1+DIMBLK2 (per DIMSAH) arrow blocks,
    // else closed-filled.
    let (arrow1, arrow2) = if dimtsz > 1e-9 {
        let t = ArrowKind::Tick { size: dimtsz as f32 };
        (t.clone(), t)
    } else if let Some(s) = style {
        if s.dimsah {
            (arrow_from_block(doc, s.dimblk1, asz), arrow_from_block(doc, s.dimblk2, asz))
        } else {
            let a = arrow_from_block(doc, s.dimblk, asz);
            (a.clone(), a)
        }
    } else {
        let a = ArrowKind::Triangle { size: asz, filled: true, size_mul: 1.0 };
        (a.clone(), a)
    };
    DimMetrics {
        dimasz,
        dimcen: style.map(|s| s.dimcen * scale).unwrap_or(0.09 * scale),
        dimexo: style.map(|s| s.dimexo * scale).unwrap_or(0.0),
        dimexe: style.map(|s| s.dimexe * scale).unwrap_or(0.0),
        dimtsz,
        dimdle: style.map(|s| s.dimdle * scale).unwrap_or(0.0),
        dimfxl: style.map(|s| s.dimfxl * scale).unwrap_or(1.0),
        dimfxlon: style.map(|s| s.dimfxlon).unwrap_or(false),
        dimse1: style.map(|s| s.dimse1).unwrap_or(false),
        dimse2: style.map(|s| s.dimse2).unwrap_or(false),
        dimsd1: style.map(|s| s.dimsd1).unwrap_or(false),
        dimsd2: style.map(|s| s.dimsd2).unwrap_or(false),
        dimsoxd: style.map(|s| s.dimsoxd).unwrap_or(false),
        dimclrd: style.map(|s| s.dimclrd).unwrap_or(0),
        dimclre: style.map(|s| s.dimclre).unwrap_or(0),
        dimclrt: style.map(|s| s.dimclrt).unwrap_or(0),
        dimlwd: style.map(|s| s.dimlwd).unwrap_or(-2),
        dimlwe: style.map(|s| s.dimlwe).unwrap_or(-2),
        arrow1,
        arrow2,
    }
}

/// A baked sub-entity common with a per-element dimension colour / lineweight.
/// A DIMCLR* of 0 (ByBlock) or 256 (ByLayer) keeps the dimension's own colour
/// so the block inherits it; a specific ACI overrides. DIMLW* < 0 (ByBlock /
/// ByLayer) keeps the dimension's lineweight.
fn dim_common(base: &acadrust::entities::EntityCommon, clr: i16, lw: i16) -> acadrust::entities::EntityCommon {
    let mut c = base.clone();
    c.handle = Handle::NULL;
    if clr != 0 && clr != 256 {
        c.color = acadrust::types::Color::from_index(clr);
    }
    if lw >= 0 {
        c.line_weight = acadrust::types::LineWeight::from_value(lw);
    }
    c
}

/// Baked geometry for an angular dimension, matching the live render exactly:
/// an extension line from each measured ray point flush out to the dimension
/// arc, the swept arc, and a terminator at each arc end. (The live angular path
/// applies neither DIMEXO/DIMEXE nor DIMSE suppression, so the bake mustn't
/// either, or the dim would shift on reload. Adding those to both is tracked as
/// a separate live-render enhancement.) #181 / DIM-022.
fn angular_block_segs(
    vertex: Vector3,
    p1: Vector3,
    p2: Vector3,
    arc_loc: Vector3,
    met: &DimMetrics,
    ext_c: &acadrust::entities::EntityCommon,
    dim_c: &acadrust::entities::EntityCommon,
    explicit_sweep: Option<(f64, f64)>,
) -> Vec<EntityType> {
    use std::f64::consts::PI;
    let measured_a1 = (p1.y - vertex.y).atan2(p1.x - vertex.x);
    let measured_a2 = (p2.y - vertex.y).atan2(p2.x - vertex.x);
    let (a1, a2) = explicit_sweep.unwrap_or((measured_a1, measured_a2));
    let radius = ((arc_loc.x - vertex.x).powi(2) + (arc_loc.y - vertex.y).powi(2)).sqrt();
    let mut out = Vec::new();
    if radius < 1e-9 {
        out.push(dim_seg(p1, vertex, ext_c));
        out.push(dim_seg(p2, vertex, ext_c));
        return out;
    }
    // Extension lines run flush from each measured ray point out to the arc.
    let e1 = Vector3::new(vertex.x + a1.cos() * radius, vertex.y + a1.sin() * radius, vertex.z);
    let e2 = Vector3::new(vertex.x + a2.cos() * radius, vertex.y + a2.sin() * radius, vertex.z);
    out.push(dim_seg(p1, e1, ext_c));
    out.push(dim_seg(p2, e2, ext_c));
    let mut sweep = a2 - a1;
    if explicit_sweep.is_some() {
        while sweep <= 0.0 {
            sweep += 2.0 * PI;
        }
        out.extend(dim_arc_segs_with_sweep(
            vertex,
            radius,
            a1,
            sweep,
            dim_c,
        ));
    } else {
        out.extend(dim_arc_segs(vertex, radius, a1, a2, dim_c));
    }
    // Terminators tangent to the arc at each end, pointing along the sweep.
    if explicit_sweep.is_none() {
        while sweep > PI {
            sweep -= 2.0 * PI;
        }
        while sweep <= -PI {
            sweep += 2.0 * PI;
        }
    }
    let sgn = if sweep >= 0.0 { 1.0 } else { -1.0 };
    out.extend(dim_terminator(e1, -a1.sin() * sgn, a1.cos() * sgn, &met.arrow1, dim_c));
    out.extend(dim_terminator(e2, a2.sin() * sgn, -a2.cos() * sgn, &met.arrow2, dim_c));
    out
}

fn jogged_radial_break(
    chord: Vector3,
    jog: Vector3,
    override_center: Vector3,
    jog_angle: f64,
) -> (Vector3, Vector3) {
    let radial = norm2(
        chord.x - override_center.x,
        chord.y - override_center.y,
        1.0,
        0.0,
    );
    let (sin, cos) = jog_angle.sin_cos();
    let transverse = (
        radial.0 * cos - radial.1 * sin,
        radial.0 * sin + radial.1 * cos,
    );
    let length = ((chord.x - override_center.x).powi(2)
        + (chord.y - override_center.y).powi(2))
    .sqrt();
    let half = (length * 0.04).max(1e-6);
    let first = Vector3::new(
        jog.x - transverse.0 * half,
        jog.y - transverse.1 * half,
        jog.z,
    );
    let second = Vector3::new(
        jog.x + transverse.0 * half,
        jog.y + transverse.1 * half,
        jog.z,
    );
    let distance_squared = |point: Vector3| {
        (point.x - chord.x).powi(2)
            + (point.y - chord.y).powi(2)
            + (point.z - chord.z).powi(2)
    };
    if distance_squared(first) <= distance_squared(second) {
        (first, second)
    } else {
        (second, first)
    }
}

/// The text anchor for a radial leader: the saved text middle point when set,
/// else the midpoint of `a` and `b` — mirroring the live `dimension_text_position`.
fn dim_text_anchor(
    base: &acadrust::entities::dimension::DimensionBase,
    a: Vector3,
    b: Vector3,
) -> Vector3 {
    let p = base.text_middle_point;
    if p.x * p.x + p.y * p.y + p.z * p.z > 1e-8 {
        p
    } else {
        Vector3::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, (a.z + b.z) * 0.5)
    }
}

/// The (start, end) of a linear/aligned extension line. `origin` is the
/// measured point, `land` its landing on the dim line, `sign` the perpendicular
/// direction, `(edx,edy)` the (possibly oblique) extension direction. Normally
/// the line runs from `origin + DIMEXO` to `land + DIMEXE`; with DIMFXLON it is
/// a fixed length DIMFXL back from the dim line instead. DIM-FXL.
fn ext_endpoints(
    origin: Vector3,
    land: Vector3,
    sign: f64,
    edx: f64,
    edy: f64,
    met: &DimMetrics,
) -> (Vector3, Vector3) {
    let end = Vector3::new(
        land.x + edx * (sign * met.dimexe),
        land.y + edy * (sign * met.dimexe),
        land.z,
    );
    let start = if met.dimfxlon {
        Vector3::new(
            land.x - edx * (sign * met.dimfxl),
            land.y - edy * (sign * met.dimfxl),
            land.z,
        )
    } else {
        Vector3::new(
            origin.x + edx * (sign * met.dimexo),
            origin.y + edy * (sign * met.dimexo),
            origin.z,
        )
    };
    (start, end)
}

/// Normalise `(dx,dy)`, falling back to `(fx,fy)` when it is ~zero length.
fn norm2(dx: f64, dy: f64, fx: f64, fy: f64) -> (f64, f64) {
    let m = (dx * dx + dy * dy).sqrt();
    if m > 1e-9 {
        (dx / m, dy / m)
    } else {
        (fx, fy)
    }
}

fn explode_dimension(dim: &Dimension, doc: &CadDocument) -> Vec<EntityType> {

    let base = dim.base();
    let met = dim_metrics(dim, doc);
    // Per-element commons so a baked block keeps the style's colours/lineweights:
    // extension lines (DIMCLRE/DIMLWE), dimension line + arrows + centre mark
    // (DIMCLRD/DIMLWD), text (DIMCLRT).
    let ext_c = dim_common(&base.common, met.dimclre, met.dimlwe);
    let dim_c = dim_common(&base.common, met.dimclrd, met.dimlwd);
    let mut result: Vec<EntityType> = Vec::new();

    // Helper: make a line segment with a given common.
    let make_seg = |a: &Vector3, b: &Vector3, common: &EntityCommon| -> EntityType {
        let mut c = common.clone();
        c.handle = Handle::NULL;
        EntityType::Line(LineEnt {
            common: c,
            start: a.clone(),
            end: b.clone(),
            ..LineEnt::new()
        })
    };

    let v3 = |x: f64, y: f64, z: f64| Vector3::new(x, y, z);

    // Dimension line for a linear/aligned dim, between d1 and d2 with the unit
    // direction (ux,uy): DIMDLE overshoot when ticks are used, omitted when both
    // halves are suppressed. When the arrows don't fit between the extension
    // lines (gap < 2·DIMASZ, arrows only) they flip to the outside with short
    // stubs unless DIMSOXD — matching the live fit logic. DIM-ARROWS-OUTSIDE.
    let push_dim_line = |result: &mut Vec<EntityType>, d1: Vector3, d2: Vector3, ux: f64, uy: f64| {
        let ticks = met.dimtsz > 1e-9;
        if !(met.dimsd1 && met.dimsd2) {
            let dle = if ticks { met.dimdle } else { 0.0 };
            let a = v3(d1.x - ux * dle, d1.y - uy * dle, d1.z);
            let b = v3(d2.x + ux * dle, d2.y + uy * dle, d2.z);
            result.push(make_seg(&a, &b, &dim_c));
        }
        let gap = ((d2.x - d1.x).powi(2) + (d2.y - d1.y).powi(2)).sqrt();
        let outside = !ticks && met.dimasz > 1e-6 && gap < 2.0 * met.dimasz;
        if outside {
            // Arrow bodies extend outward (tips still at the origins).
            result.extend(dim_terminator(d1, -ux, -uy, &met.arrow1, &dim_c));
            result.extend(dim_terminator(d2, ux, uy, &met.arrow2, &dim_c));
            if !met.dimsoxd {
                let stub = 2.0 * met.dimasz;
                result.push(make_seg(&v3(d1.x - ux * stub, d1.y - uy * stub, d1.z), &d1, &dim_c));
                result.push(make_seg(&d2, &v3(d2.x + ux * stub, d2.y + uy * stub, d2.z), &dim_c));
            }
        } else {
            result.extend(dim_terminator(d1, ux, uy, &met.arrow1, &dim_c));
            result.extend(dim_terminator(d2, -ux, -uy, &met.arrow2, &dim_c));
        }
    };

    match dim {
        Dimension::Aligned(d) => {
            let fx = d.first_point.x;
            let fy = d.first_point.y;
            let sx = d.second_point.x;
            let sy = d.second_point.y;
            let axis_angle = (sy - fy).atan2(sx - fx);
            let perp_x = -(axis_angle.sin());
            let perp_y = axis_angle.cos();
            let offset =
                (d.definition_point.x - fx) * perp_x + (d.definition_point.y - fy) * perp_y;
            let d1 = v3(fx + perp_x * offset, fy + perp_y * offset, d.first_point.z);
            let d2 = v3(sx + perp_x * offset, sy + perp_y * offset, d.second_point.z);
            let s = if offset >= 0.0 { 1.0 } else { -1.0 };
            // DIMEDIT "Oblique": extension lines slant by ext_line_rotation (the
            // dim line itself stays on the unrotated perp). DIM-EXT-OBLIQUE.
            let (es, ec) = d.ext_line_rotation.sin_cos();
            let edx = perp_x * ec - perp_y * es;
            let edy = perp_x * es + perp_y * ec;
            let (e1a, e1b) = ext_endpoints(d.first_point, d1, s, edx, edy, &met);
            let (e2a, e2b) = ext_endpoints(d.second_point, d2, s, edx, edy, &met);
            if !met.dimse1 {
                result.push(make_seg(&e1a, &e1b, &ext_c));
            }
            if !met.dimse2 {
                result.push(make_seg(&e2a, &e2b, &ext_c));
            }
            let ml = ((d2.x - d1.x).powi(2) + (d2.y - d1.y).powi(2)).sqrt().max(1e-12);
            let (ux, uy) = ((d2.x - d1.x) / ml, (d2.y - d1.y) / ml);
            push_dim_line(&mut result, d1, d2, ux, uy);
        }
        Dimension::Linear(d) => {
            // `rotation` is the dimension-line angle, already in radians.
            let angle = d.rotation;
            let perp_x = -(angle.sin());
            let perp_y = angle.cos();
            let fx = d.first_point.x;
            let fy = d.first_point.y;
            let sx = d.second_point.x;
            let sy = d.second_point.y;
            // Project each extension origin onto the dim line independently — a
            // shared offset tilts the line over non-level origins (#181).
            let dperp = d.definition_point.x * perp_x + d.definition_point.y * perp_y;
            let off1 = dperp - (fx * perp_x + fy * perp_y);
            let off2 = dperp - (sx * perp_x + sy * perp_y);
            let d1 = v3(fx + perp_x * off1, fy + perp_y * off1, d.first_point.z);
            let d2 = v3(sx + perp_x * off2, sy + perp_y * off2, d.second_point.z);
            // Extension lines: DIMEXO start gap from the point, DIMEXE overshoot
            // past the dim line, in the perp direction toward it. #181 / DIM-023.
            let s1 = if off1 >= 0.0 { 1.0 } else { -1.0 };
            let s2 = if off2 >= 0.0 { 1.0 } else { -1.0 };
            // DIMEDIT "Oblique": extension lines slant by ext_line_rotation.
            let (es, ec) = d.ext_line_rotation.sin_cos();
            let edx = perp_x * ec - perp_y * es;
            let edy = perp_x * es + perp_y * ec;
            let (e1a, e1b) = ext_endpoints(d.first_point, d1, s1, edx, edy, &met);
            let (e2a, e2b) = ext_endpoints(d.second_point, d2, s2, edx, edy, &met);
            if !met.dimse1 {
                result.push(make_seg(&e1a, &e1b, &ext_c));
            }
            if !met.dimse2 {
                result.push(make_seg(&e2a, &e2b, &ext_c));
            }
            let ml = ((d2.x - d1.x).powi(2) + (d2.y - d1.y).powi(2)).sqrt().max(1e-12);
            let (ux, uy) = ((d2.x - d1.x) / ml, (d2.y - d1.y) / ml);
            push_dim_line(&mut result, d1, d2, ux, uy);
        }
        Dimension::Radius(d) => {
            let (center, point) = (d.angle_vertex, d.definition_point);
            result.push(make_seg(&center, &point, &dim_c));
            let len = ((center.x - point.x).powi(2) + (center.y - point.y).powi(2))
                .sqrt()
                .max(1e-12);
            result.extend(dim_terminator(
                point,
                (center.x - point.x) / len,
                (center.y - point.y) / len,
                &met.arrow1,
                &dim_c,
            ));
            // Leader from the arrow tip toward the text — to the text anchor when
            // leader_length is 0, else that far along the text direction (matches
            // the live radius render). DIM-RAD-LEADER.
            let anchor = dim_text_anchor(base, center, point);
            let ld = norm2(anchor.x - point.x, anchor.y - point.y, 1.0, 0.0);
            let leader = if d.leader_length.abs() > 1e-9 {
                v3(point.x + ld.0 * d.leader_length, point.y + ld.1 * d.leader_length, point.z)
            } else {
                anchor
            };
            result.push(make_seg(&point, &leader, &dim_c));
            result.extend(dim_center_mark(center, met.dimcen, len, &dim_c));
        }
        Dimension::Diameter(d) => {
            // Full diameter through the centre (far edge -> near edge), inward
            // terminators at both edges, plus the centre mark.
            let (center, edge) = (d.angle_vertex, d.definition_point);
            let far = v3(2.0 * center.x - edge.x, 2.0 * center.y - edge.y, edge.z);
            result.push(make_seg(&far, &edge, &dim_c));
            let len = ((edge.x - far.x).powi(2) + (edge.y - far.y).powi(2))
                .sqrt()
                .max(1e-12);
            let (ux, uy) = ((edge.x - far.x) / len, (edge.y - far.y) / len);
            result.extend(dim_terminator(edge, -ux, -uy, &met.arrow1, &dim_c));
            result.extend(dim_terminator(far, ux, uy, &met.arrow2, &dim_c));
            // Optional leader past the near edge toward the text. DIM-DIA-LEADER.
            if d.leader_length.abs() > 1e-9 {
                let anchor = dim_text_anchor(base, center, edge);
                let ld = norm2(anchor.x - edge.x, anchor.y - edge.y, ux, uy);
                result.push(make_seg(
                    &edge,
                    &v3(edge.x + ld.0 * d.leader_length, edge.y + ld.1 * d.leader_length, edge.z),
                    &dim_c,
                ));
            }
            result.extend(dim_center_mark(center, met.dimcen, len * 0.5, &dim_c));
        }
        Dimension::Angular2Ln(d) => {
            result.extend(angular_block_segs(
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.dimension_arc,
                &met,
                &ext_c,
                &dim_c,
                None,
            ));
        }
        Dimension::Angular3Pt(d) => {
            result.extend(angular_block_segs(
                d.angle_vertex,
                d.first_point,
                d.second_point,
                d.definition_point,
                &met,
                &ext_c,
                &dim_c,
                None,
            ));
        }
        Dimension::Ordinate(d) => {
            result.push(make_seg(&d.feature_location, &d.definition_point, &dim_c));
            result.push(make_seg(&d.definition_point, &d.leader_endpoint, &dim_c));
        }
        Dimension::Arc(d) => {
            result.extend(angular_block_segs(
                d.center_point,
                d.first_extension_point,
                d.second_extension_point,
                d.definition_point,
                &met,
                &ext_c,
                &dim_c,
                d.is_partial.then_some((
                    d.arc_start_parameter,
                    d.arc_end_parameter,
                )),
            ));
            if d.has_leader {
                result.push(make_seg(
                    &d.first_leader_point,
                    &d.second_leader_point,
                    &dim_c,
                ));
            }
        }
        Dimension::LargeRadial(d) => {
            let (near, far) = jogged_radial_break(
                d.chord_point,
                d.jog_point,
                d.override_center,
                d.jog_angle,
            );
            result.push(make_seg(&d.chord_point, &near, &dim_c));
            result.push(make_seg(&near, &far, &dim_c));
            result.push(make_seg(&far, &d.override_center, &dim_c));
            let len = ((near.x - d.chord_point.x).powi(2)
                + (near.y - d.chord_point.y).powi(2))
            .sqrt()
            .max(1e-12);
            result.extend(dim_terminator(
                d.chord_point,
                (near.x - d.chord_point.x) / len,
                (near.y - d.chord_point.y) / len,
                &met.arrow1,
                &dim_c,
            ));
        }
    }

    // Measurement text is the live render's own Text/MText entity (value,
    // position, height, rotation, alignment, text style, MText handling all
    // shared), so the baked block matches the on-screen dimension and the label
    // doesn't shift when the file is saved and reopened. `None` = suppressed.
    if let Some(mut tent) = crate::entities::dimension::baked_dimension_text_entity(dim, doc, 1.0) {
        // Apply the text colour / lineweight (DIMCLRT) onto the entity.
        let tc = dim_common(&base.common, met.dimclrt, -2);
        tent.common_mut().color = tc.color;
        tent.common_mut().line_weight = tc.line_weight;
        result.push(tent);
    }

    result
}

// ── Dimension block baking (DWG/DXF interop) ────────────────────────────────

/// Mirror each dimension's authoritative geometric definition point into
/// `base.definition_point`, which is the field the DWG/DXF writer emits as
/// group 10. Edits (grips, properties, transforms) update the per-type struct
/// field but not `base`, so without this the saved group 10 goes stale and the
/// dimension's line / leader / origin jumps on reload (#181). Angular-2-line
/// keeps a distinct base point (the second line's point) and is left alone.
fn sync_dimension_base_points_and_collect_pending(doc: &mut CadDocument) -> Vec<Handle> {
    let existing_blocks: rustc_hash::FxHashSet<String> = doc
        .block_records
        .iter()
        .map(|record| record.name.to_ascii_lowercase())
        .collect();
    let dimensions: Vec<(Handle, Option<Vector3>, bool)> = doc
        .entities()
        .filter_map(|entity| {
            let EntityType::Dimension(dimension) = entity else {
                return None;
            };
            let definition_point = match dimension {
                Dimension::Linear(value) => Some(value.definition_point),
                Dimension::Aligned(value) => Some(value.definition_point),
                Dimension::Radius(value) => Some(value.definition_point),
                Dimension::Diameter(value) => Some(value.definition_point),
                Dimension::Ordinate(value) => Some(value.definition_point),
                Dimension::Angular3Pt(value) => Some(value.definition_point),
                Dimension::Arc(value) => Some(value.definition_point),
                Dimension::LargeRadial(value) => Some(value.definition_point),
                Dimension::Angular2Ln(_) => None,
            };
            let base = dimension.base();
            let needs_sync =
                definition_point.filter(|point| *point != base.definition_point);
            let block_missing = base.block_name.trim().is_empty()
                || !existing_blocks.contains(&base.block_name.to_ascii_lowercase());
            Some((base.common.handle, needs_sync, block_missing))
        })
        .collect();

    let mut pending = Vec::new();
    for (handle, definition_point, block_missing) in dimensions {
        if let Some(point) = definition_point {
            if let Some(EntityType::Dimension(dimension)) = doc.get_entity_mut(handle) {
                dimension.base_mut().definition_point = point;
            }
        }
        if block_missing {
            pending.push(handle);
        }
    }
    pending
}

/// Smallest free `*D<n>` anonymous block name in `doc`.
fn next_dimension_block_name(doc: &CadDocument, next: &mut u64) -> String {
    loop {
        let n = *next;
        *next = (*next).saturating_add(1);
        let cand = format!("*D{n}");
        if doc.block_records.get(&cand).is_none() {
            return cand;
        }
    }
}

/// Bake an anonymous `*D<n>` geometry block for every DIMENSION that doesn't
/// already own one, so the file is valid for AutoCAD-family readers.
///
/// OCS renders dimensions by re-tessellating them on the fly and never
/// materialises the `*D` block that a DWG `DIMENSION` is supposed to reference
/// (the lines / arrows / text that AutoCAD actually draws). A dimension created
/// in OCS therefore goes out referencing a block that doesn't exist, and the
/// writer emits a null block handle — strict readers (DWG TrueView, QCAD) drop
/// the dimension or demand a recovery, and lenient ones (BricsCAD) regenerate it
/// at a different position. Call this on the document about to be written so each
/// such dimension gets a real block built from its exploded geometry (extension
/// lines + dimension line + measurement text, the same decomposition EXPLODE
/// uses) and its `block_name` points at it.
///
/// Dimensions that already reference an existing block (e.g. imported from a real
/// DWG, or copied via the `*D`-cloning copy path) are left untouched so their
/// original graphics are preserved.
pub fn bake_dimension_blocks(doc: &mut CadDocument) {
    // Keep group-10 in step and find missing blocks in one entity pass. Existing
    // `*D` blocks are the save cache: only invalidated/new dimensions enter the
    // relatively expensive geometry generation below.
    let pending = sync_dimension_base_points_and_collect_pending(doc);
    let dimensions: Vec<(Handle, Dimension)> = pending
        .into_iter()
        .filter_map(|handle| match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => Some((handle, d.clone())),
            _ => None,
        })
        .collect();

    use crate::par::prelude::*;
    let doc_ref: &CadDocument = doc;
    let generated: Vec<(Handle, Vec<EntityType>)> = dimensions
        .par_iter()
        .map(|(handle, dim)| (*handle, explode_dimension(dim, doc_ref)))
        .collect();

    let mut next_block_number = 0u64;
    for (handle, subs) in generated {
        if subs.is_empty() {
            continue;
        }

        let name = next_dimension_block_name(doc, &mut next_block_number);
        // Reserve three consecutive handles for the record / block / endblk.
        // Adding the block + endblk (which carry explicit handles) advances the
        // document's handle counter past them, so the NULL-handle sub-entities
        // added afterwards get fresh handles without colliding.
        let next = doc.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut br = BlockRecord::new(&name);
        br.handle = br_handle;
        br.block_entity_handle = block_handle;
        br.block_end_handle = end_handle;
        br.flags.anonymous = true;
        if doc.block_records.add(br).is_err() {
            continue;
        }

        let mut block = Block::new(&name, Vector3::new(0.0, 0.0, 0.0));
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        let _ = doc.add_entity(EntityType::Block(block));

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        let _ = doc.add_entity(EntityType::BlockEnd(block_end));

        for mut sub in subs {
            sub.common_mut().handle = Handle::NULL;
            sub.common_mut().owner_handle = br_handle;
            let _ = doc.add_entity(sub);
        }

        if let Some(EntityType::Dimension(d)) = doc.get_entity_mut(handle) {
            d.base_mut().block_name = name;
            // The block we just baked holds the dimension graphics in absolute
            // WCS, so the DWG group-12 insertion point (base.insertion_point)
            // MUST be the origin. A reader that positions the *D block by that
            // point (BricsCAD / ODA) otherwise draws it shifted by the offset,
            // while OCS — which renders the block in place — shows it correctly.
            // OCS's dimension commands seed insertion_point with the text
            // anchor; reset it here so the saved dimension lands identically in
            // every application. (#181)
            d.base_mut().insertion_point = Vector3::new(0.0, 0.0, 0.0);
        }
    }
}

/// Drop a dimension's baked `*D` block so the next save regenerates it from the
/// dimension's current definition points / text / style.
///
/// OCS renders a dimension live from its definition points, but exports a baked
/// `*D` block that other applications (BricsCAD / ODA) draw instead. An in-place
/// edit — grip drag, DIMTEDIT, restyle, text edit, DIMSPACE — changes the
/// definition points while leaving the old block, so without this the export
/// keeps the pre-edit graphics and the dimension appears wrong everywhere but in
/// OCS. Call this after any such edit; [`bake_dimension_blocks`] then rebuilds a
/// fresh block on save.
///
/// The transform path (MOVE / COPY / PASTE) keeps its own block in sync via
/// `define_transformed_block` and must NOT call this. The removed block's record
/// and owned entities are deleted too, so re-baking on every edit can't
/// accumulate orphan `*D` blocks. No-op when the dimension has no baked block
/// yet (the pending path bakes it fresh on save). (#181)
pub fn invalidate_dim_block(doc: &mut CadDocument, handle: Handle) {
    let bn = match doc.get_entity(handle) {
        Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
        _ => return,
    };
    if bn.trim().is_empty() {
        return;
    }
    // Dimension graphics live in an anonymous "*D…" block, unique to the one
    // dimension, so it is safe to delete along with its sub-entities. Guard on
    // the anonymous prefix so a hand-referenced named block is never removed.
    if bn.starts_with("*D") || bn.starts_with("*d") {
        if let Some(rec) = doc.block_records.remove(&bn) {
            let mut owned = rec.entity_handles.clone();
            owned.push(rec.block_entity_handle);
            owned.push(rec.block_end_handle);
            for h in owned {
                if !h.is_null() {
                    doc.remove_entity(h);
                }
            }
        }
    }
    if let Some(EntityType::Dimension(d)) = doc.get_entity_mut(handle) {
        d.base_mut().block_name.clear();
    }
}

// ── Command stub (kept for future interactive selection mode) ───────────────

pub struct ExplodeCommand;

impl ExplodeCommand {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for ExplodeCommand {
    fn name(&self) -> &'static str {
        "EXPLODE"
    }
    fn prompt(&self) -> String {
        t!("EXPLODE  Select objects to explode:").into_owned()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::DimensionLinear;

    /// A dimension created without a geometry block gets a real `*D` block on
    /// bake, its `block_name` resolves to that block, and a second bake is a
    /// no-op (already has a valid block).
    #[test]
    fn bakes_block_for_blockless_dimension_and_is_idempotent() {
        let mut doc = CadDocument::new();

        let mut d = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        d.base.text_middle_point = Vector3::new(5.0, 5.0, 0.0);
        // block_name is left empty — exactly what OCS-created dimensions carry.
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Linear(d)))
            .unwrap();

        bake_dimension_blocks(&mut doc);

        let block_name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        assert!(!block_name.trim().is_empty(), "block_name should be set");
        assert!(
            doc.block_records.get(&block_name).is_some(),
            "baked block must exist in the block table"
        );

        // Second pass must not create another block for the same dimension.
        let before = doc.block_records.len();
        bake_dimension_blocks(&mut doc);
        assert_eq!(
            doc.block_records.len(),
            before,
            "a dimension that already owns a block must not be re-baked"
        );
    }

    /// Baking a blockless dimension resets its group-12 insertion point to the
    /// origin. OCS's commands seed it with the text anchor; left non-zero, a
    /// reader that positions the absolute-WCS block by that point draws the
    /// dimension shifted. Regression test for #181.
    #[test]
    fn bake_zeroes_insertion_point() {
        let mut doc = CadDocument::new();

        let mut d = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        d.base.text_middle_point = Vector3::new(5.0, 5.0, 0.0);
        // Exactly what OCS-created dimensions carry: insertion == text anchor.
        d.base.insertion_point = d.base.text_middle_point;
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Linear(d)))
            .unwrap();

        bake_dimension_blocks(&mut doc);

        let ins = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().insertion_point,
            _ => panic!("dimension missing"),
        };
        assert_eq!(
            ins,
            Vector3::new(0.0, 0.0, 0.0),
            "baked dimension must carry a zero insertion point (#181)"
        );
    }

    /// Invalidating a baked dimension removes its `*D` block record AND the
    /// sub-entities it owned (no orphans), and clears `block_name` so the next
    /// save re-bakes a fresh block. Guards the stale-export fix for #181.
    #[test]
    fn invalidate_removes_block_and_leaves_no_orphan() {
        let mut doc = CadDocument::new();

        let mut d = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        d.base.text_middle_point = Vector3::new(5.0, 5.0, 0.0);
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Linear(d)))
            .unwrap();

        bake_dimension_blocks(&mut doc);
        let block_name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        assert!(!block_name.trim().is_empty());
        let owned: Vec<Handle> = doc
            .block_records
            .get(&block_name)
            .unwrap()
            .entity_handles
            .clone();
        assert!(!owned.is_empty(), "baked block should own sub-entities");

        invalidate_dim_block(&mut doc, handle);

        // block_name cleared → the dimension is pending again.
        match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => {
                assert!(d.base().block_name.trim().is_empty(), "block_name must be cleared");
            }
            _ => panic!("dimension missing"),
        }
        // Block record gone.
        assert!(
            doc.block_records.get(&block_name).is_none(),
            "stale block record must be removed"
        );
        // No orphaned sub-entities left behind.
        for h in owned {
            assert!(
                doc.get_entity(h).is_none(),
                "sub-entity {h:?} of the dropped block leaked"
            );
        }

        // A fresh bake re-creates a block, so the round-trip stays valid.
        bake_dimension_blocks(&mut doc);
        let rebaked = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        assert!(!rebaked.trim().is_empty(), "re-bake must assign a new block");
    }

    // Collect the line segments baked into the dimension's `*D` block.
    fn baked_segments(doc: &CadDocument, block_name: &str) -> Vec<(Vector3, Vector3)> {
        let rec = doc.block_records.get(block_name).expect("block record");
        doc.entities()
            .filter_map(|e| match e {
                EntityType::Line(l) if l.common.owner_handle == rec.handle => {
                    Some((l.start, l.end))
                }
                _ => None,
            })
            .collect()
    }

    // A horizontal (rotation = 0) linear dimension whose two measured points sit
    // at *different* heights must still bake a level dimension line — both
    // extension origins project onto the same line. Regression test for #181,
    // where a shared offset tilted the dimension line.
    #[test]
    fn linear_dim_line_stays_level_over_sloped_points() {
        let mut doc = CadDocument::new();
        let mut d = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 5.0, 0.0));
        d.rotation = 0.0;
        d.definition_point = Vector3::new(0.0, 8.0, 0.0);
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Linear(d)))
            .unwrap();
        bake_dimension_blocks(&mut doc);
        let name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        // The dimension line is the segment spanning both x extents; it must be
        // horizontal at the definition-point level (y = 8).
        let dim_line = baked_segments(&doc, &name)
            .into_iter()
            .find(|(a, b)| (a.x - 0.0).abs() < 1e-6 && (b.x - 10.0).abs() < 1e-6)
            .expect("dimension line segment");
        assert!(
            (dim_line.0.y - 8.0).abs() < 1e-6 && (dim_line.1.y - 8.0).abs() < 1e-6,
            "dimension line must be level at y=8, got {:?}",
            dim_line
        );
    }

    // A linear dimension bakes closed FILLED arrowheads (SOLID entities), not
    // generic open strokes — so a saved+reloaded dim keeps the style's arrow
    // type. (Default style: DIMTSZ=0 -> arrows, two of them.)
    #[test]
    fn linear_dim_bakes_filled_arrowheads() {
        let mut doc = CadDocument::new();
        let mut d = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        d.definition_point = Vector3::new(0.0, 5.0, 0.0);
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Linear(d)))
            .unwrap();
        bake_dimension_blocks(&mut doc);
        let name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        let rec = doc.block_records.get(&name).expect("block");
        let solids = doc
            .entities()
            .filter(|e| matches!(e, EntityType::Solid(s) if s.common.owner_handle == rec.handle))
            .count();
        assert_eq!(solids, 2, "expected 2 filled (SOLID) arrowheads, got {solids}");
    }

    // An angular dimension must bake its swept ARC (not just two rays), else a
    // saved+reloaded angular dim collapses to a V. The block should carry the
    // two extension lines plus many arc chords.
    #[test]
    fn angular_dim_bakes_an_arc() {
        use acadrust::entities::DimensionAngular3Pt;
        let mut doc = CadDocument::new();
        let mut d = DimensionAngular3Pt::new(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(0.0, 10.0, 0.0),
        );
        d.definition_point = Vector3::new(5.0, 5.0, 0.0); // arc location
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Angular3Pt(d)))
            .unwrap();
        bake_dimension_blocks(&mut doc);
        let name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        let rec = doc.block_records.get(&name).expect("block");
        let lines = doc
            .entities()
            .filter(|e| matches!(e, EntityType::Line(l) if l.common.owner_handle == rec.handle))
            .count();
        assert!(lines > 5, "angular bake must include arc chords, got {lines} lines");
    }

    // A diameter dimension bakes a line edge-to-edge THROUGH the centre, not a
    // radius-length line. The two extreme endpoints must be equidistant from the
    // centre (angle_vertex) and the centre must lie between them.
    #[test]
    fn diameter_dim_bakes_through_center() {
        use acadrust::entities::DimensionDiameter;
        let mut doc = CadDocument::new();
        let center = Vector3::new(3.0, 4.0, 0.0);
        let edge = Vector3::new(8.0, 4.0, 0.0); // radius 5 along +x
        let mut d = DimensionDiameter::new(center, edge);
        d.base.text_middle_point = Vector3::new(3.0, 9.0, 0.0);
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Diameter(d)))
            .unwrap();
        bake_dimension_blocks(&mut doc);
        let name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        // The longest baked segment is the diameter line; its endpoints span the
        // full diameter (length ~= 2*radius = 10) centred on `center`.
        let diam = baked_segments(&doc, &name)
            .into_iter()
            .find(|(a, b)| {
                let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
                (len - 10.0).abs() < 1e-6
            })
            .expect("diameter line spanning 2*radius");
        let mid_x = (diam.0.x + diam.1.x) * 0.5;
        let mid_y = (diam.0.y + diam.1.y) * 0.5;
        assert!(
            (mid_x - center.x).abs() < 1e-6 && (mid_y - center.y).abs() < 1e-6,
            "diameter line must be centred on the circle centre, mid=({mid_x},{mid_y})"
        );
    }

    // A rotated linear dimension uses `rotation` directly (radians). Before the
    // fix `to_radians()` shrank a 90° dim to ~1.57°, baking a nearly-horizontal
    // line instead of a vertical one.
    #[test]
    fn rotated_linear_dim_bakes_at_its_angle() {
        let mut doc = CadDocument::new();
        let mut d = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 10.0, 0.0));
        d.rotation = std::f64::consts::FRAC_PI_2; // 90°, vertical dimension line
        d.definition_point = Vector3::new(8.0, 0.0, 0.0);
        let handle = doc
            .add_entity(EntityType::Dimension(Dimension::Linear(d)))
            .unwrap();
        bake_dimension_blocks(&mut doc);
        let name = match doc.get_entity(handle) {
            Some(EntityType::Dimension(d)) => d.base().block_name.clone(),
            _ => panic!("dimension missing"),
        };
        // Dimension line spans both y extents and must be vertical at x = 8.
        let dim_line = baked_segments(&doc, &name)
            .into_iter()
            .find(|(a, b)| (a.y - 0.0).abs() < 1e-6 && (b.y - 10.0).abs() < 1e-6)
            .expect("dimension line segment");
        assert!(
            (dim_line.0.x - 8.0).abs() < 1e-6 && (dim_line.1.x - 8.0).abs() < 1e-6,
            "dimension line must be vertical at x=8, got {:?}",
            dim_line
        );
    }
}
