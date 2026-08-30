//! DGN line-style rendering (first pass).
//!
//! Imported DGN linetypes store their real pattern as DGN
//! line-style objects (`AcDbLS*`), not standard `LTYPE` dashes — the standard
//! table entry is empty, so acadrust exposes the structure in
//! [`CadDocument::dgn_ls_definitions`] / `dgn_ls_components` instead. See
//! `objects/dgn_linestyle.rs` in acadrust and `~/Documents/OCS/DGN_LINESTYLE_PLAN.md`.
//!
//! The visible content combines **symbol components**, each of which references
//! an anonymous block (e.g. a pipe's end circle), with typed stroke patterns.
//! Symbols are rendered at the host polyline's endpoints and stroke dash/gap
//! lengths are carried through to the pipe walls.

use acadrust::objects::DgnLsComponentType;
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use std::collections::HashSet;

use crate::scene::model::wire_model::WireModel;

/// A symbol placement in a linetype's DGN line-style tree: the anonymous block
/// to draw and the scale divisor to draw it at (`geometry / scale`).
pub struct DgnSymbol {
    pub block: Handle,
    pub scale: f64,
}

/// Symbol placements referenced by a linetype's DGN line-style tree, in tree
/// order. Empty when the linetype is not a DGN line style.
pub fn symbol_blocks(doc: &CadDocument, lt_name: &str) -> Vec<DgnSymbol> {
    let Some(def) = doc
        .dgn_ls_definitions
        .values()
        .find(|d| d.name.eq_ignore_ascii_case(lt_name))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    walk(doc, def.root_component, &mut out, &mut seen);
    out
}

fn walk(doc: &CadDocument, h: Handle, out: &mut Vec<DgnSymbol>, seen: &mut HashSet<Handle>) {
    if !seen.insert(h) {
        return;
    }
    let Some(c) = doc.dgn_ls_components.get(&h) else {
        return;
    };
    match c.component_type {
        DgnLsComponentType::Compound | DgnLsComponentType::Point => {
            for r in &c.refs {
                let Some(sub) = doc.dgn_ls_components.get(r) else {
                    continue;
                };
                if sub.component_type == DgnLsComponentType::Symbol {
                    if let Some(block) = sub.symbol_block() {
                        if !out.iter().any(|s| s.block == block) {
                            out.push(DgnSymbol {
                                block,
                                scale: sub.scale,
                            });
                        }
                    }
                } else {
                    walk(doc, *r, out, seen);
                }
            }
        }
        _ => {}
    }
}

/// Signed native dash lengths in a typed stroke component: dashes are positive
/// and gaps negative. Empty when the component has no usable stroke lengths.
fn stroke_dashes(doc: &CadDocument, h: Handle) -> Vec<f64> {
    use acadrust::objects::{DgnLineStyleData, DgnLsComponentData, ObjectType};
    let Some(ObjectType::DgnLineStyle(style)) = doc.objects.get(&h)
    else {
        return Vec::new();
    };
    let DgnLineStyleData::Component {
        component: DgnLsComponentData::Stroke(pattern),
        ..
    } = &style.data
    else {
        return Vec::new();
    };
    pattern
        .strokes
        .iter()
        .filter_map(|stroke| {
            let length = stroke.length.abs();
            (length.is_finite() && length > 0.0)
                .then_some(if stroke.is_dash { length } else { -length })
        })
        .collect()
}

/// Native dash pattern of a DGN line style's pipe walls: the dash lengths of the
/// first stroke that is a **direct** child of the root compound and carries at
/// least two values (a dash + a gap). The base stroke a point component sits on
/// (a single long length) is intentionally skipped — it is the solid placement
/// guide, not the visible dash. Empty for a solid style.
pub fn wall_dashes(doc: &CadDocument, lt_name: &str) -> Vec<f64> {
    let Some(def) = doc
        .dgn_ls_definitions
        .values()
        .find(|d| d.name.eq_ignore_ascii_case(lt_name))
    else {
        return Vec::new();
    };
    let Some(root) = doc.dgn_ls_components.get(&def.root_component) else {
        return Vec::new();
    };
    for r in &root.refs {
        if doc.dgn_ls_components.get(r).map(|c| c.component_type)
            == Some(DgnLsComponentType::Stroke)
        {
            let dashes = stroke_dashes(doc, *r);
            if dashes.len() >= 2 {
                return dashes;
            }
        }
    }
    Vec::new()
}

/// Rendered half-width of a symbol block: its geometry's largest extent from
/// the block base point, divided by the symbol scale (the same divisor
/// [`place_block_wires`] draws it at). For a pipe end-circle this is the circle
/// radius, which is also the offset of the two pipe walls (they sit tangent to
/// the end circles) — so it doubles as the rail offset for the double line.
pub fn symbol_radius(doc: &CadDocument, block: Handle, scale: f64) -> f64 {
    let mut r = 0.0_f64;
    let depths = rustc_hash::FxHashMap::default();
    let graph = crate::scene::render_graph::RenderSceneGraph::new(doc, None, None, true, &depths);
    let Some(block_use) = crate::scene::render_graph::block_use_from_handle(
        doc,
        block,
        crate::scene::render_graph::BlockRole::LineStyleSymbol,
        Vector3::ZERO,
    ) else {
        return 0.0;
    };
    graph.walk_insert(
        &block_use.insert,
        block,
        |_, _| true,
        |entity, context| {
            let mut placed = entity.clone();
            placed.apply_transform(&context.transform);
            let distance = |x: f64, y: f64| x.hypot(y);
            let extent = match &placed {
                EntityType::Ellipse(ellipse) => {
                    distance(ellipse.center.x, ellipse.center.y)
                        + distance(ellipse.major_axis.x, ellipse.major_axis.y)
                }
                EntityType::Circle(circle) => {
                    distance(circle.center.x, circle.center.y) + circle.radius
                }
                EntityType::Arc(arc) => distance(arc.center.x, arc.center.y) + arc.radius,
                EntityType::Line(line) => distance(line.start.x, line.start.y)
                    .max(distance(line.end.x, line.end.y)),
                _ => crate::scene::convert::tess::entity_world_aabb_f64(&placed)
                    .map(|[min_x, min_y, max_x, max_y]| {
                        [
                            distance(min_x, min_y),
                            distance(max_x, min_y),
                            distance(max_x, max_y),
                            distance(min_x, max_y),
                        ]
                        .into_iter()
                        .fold(0.0_f64, f64::max)
                    })
                    .unwrap_or(0.0),
            };
            r = r.max(extent);
        },
    );
    let s = if scale.abs() > 1e-9 { scale } else { 1.0 };
    r / s
}

/// Per-vertex left-normal offset of an XY polyline by `d` (signed). Uses the
/// averaged adjacent-segment normal at interior vertices — good for the gentle
/// bends of a pipe run; sharp corners are not mitred.
fn offset_xy(pts: &[[f64; 3]], d: f64) -> Vec<[f64; 2]> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (ax, ay) = if i == 0 {
            (pts[1][0] - pts[0][0], pts[1][1] - pts[0][1])
        } else {
            (pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1])
        };
        let (bx, by) = if i + 1 < n {
            (pts[i + 1][0] - pts[i][0], pts[i + 1][1] - pts[i][1])
        } else {
            (ax, ay)
        };
        let na = (ax * ax + ay * ay).sqrt().max(1e-12);
        let nb = (bx * bx + by * by).sqrt().max(1e-12);
        let tx = ax / na + bx / nb;
        let ty = ay / na + by / nb;
        let tn = (tx * tx + ty * ty).sqrt().max(1e-12);
        // Left normal of the averaged tangent.
        out.push([pts[i][0] - d * (ty / tn), pts[i][1] + d * (tx / tn)]);
    }
    out
}

/// Clone the host polyline with every vertex offset perpendicular by `d` — one
/// wall of the pipe. Returns `None` for entity kinds without an XY vertex list.
pub fn offset_host_entity(e: &EntityType, d: f64) -> Option<EntityType> {
    let mut clone = e.clone();
    match &mut clone {
        EntityType::LwPolyline(p) => {
            // Drop consecutive duplicate vertices first. A zero-length segment
            // gives `offset_xy` a degenerate normal, which leaves that vertex
            // un-offset — folding the wall down to the centre line and drawing a
            // spurious segment that visually links the two walls. (Some DGN pipe
            // polylines carry trailing duplicate end vertices.)
            p.vertices.dedup_by(|a, b| {
                (a.location.x - b.location.x).abs() < 1e-9
                    && (a.location.y - b.location.y).abs() < 1e-9
            });
            if p.vertices.len() < 2 {
                return None;
            }
            let pts: Vec<[f64; 3]> = p
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y, 0.0])
                .collect();
            for (v, o) in p.vertices.iter_mut().zip(offset_xy(&pts, d)) {
                v.location.x = o[0];
                v.location.y = o[1];
            }
        }
        EntityType::Polyline2D(p) => {
            p.vertices.dedup_by(|a, b| {
                (a.location.x - b.location.x).abs() < 1e-9
                    && (a.location.y - b.location.y).abs() < 1e-9
            });
            if p.vertices.len() < 2 {
                return None;
            }
            let pts: Vec<[f64; 3]> = p
                .vertices
                .iter()
                .map(|v| [v.location.x, v.location.y, 0.0])
                .collect();
            for (v, o) in p.vertices.iter_mut().zip(offset_xy(&pts, d)) {
                v.location.x = o[0];
                v.location.y = o[1];
            }
        }
        EntityType::Line(l) => {
            let pts = [[l.start.x, l.start.y, 0.0], [l.end.x, l.end.y, 0.0]];
            let o = offset_xy(&pts, d);
            l.start.x = o[0][0];
            l.start.y = o[0][1];
            l.end.x = o[1][0];
            l.end.y = o[1][1];
        }
        _ => return None,
    }
    Some(clone)
}

/// Host entity's polyline vertices in WCS f64 (consecutive duplicates dropped).
pub fn polyline_points(e: &EntityType) -> Vec<[f64; 3]> {
    let mut v: Vec<[f64; 3]> = match e {
        EntityType::LwPolyline(p) => p
            .vertices
            .iter()
            .map(|w| [w.location.x, w.location.y, 0.0])
            .collect(),
        EntityType::Polyline2D(p) => p
            .vertices
            .iter()
            .map(|w| [w.location.x, w.location.y, 0.0])
            .collect(),
        EntityType::Line(l) => vec![
            [l.start.x, l.start.y, l.start.z],
            [l.end.x, l.end.y, l.end.z],
        ],
        _ => Vec::new(),
    };
    v.dedup();
    v
}

/// Tessellate a symbol block's entities, translated so the block origin lands at
/// `at`, in the host entity's colour. Reuses the normal entity tessellator on
/// translated clones — the symbol geometry (ellipses, lines, …) renders exactly
/// as it would anywhere else.
#[allow(clippy::too_many_arguments)]
pub fn place_block_wires(
    doc: &CadDocument,
    block: Handle,
    scale_divisor: f64,
    at: [f64; 3],
    color: [f32; 4],
    line_weight_px: f32,
    anno_scale: f32,
    world_per_pixel: Option<f32>,
    bg_color: [f32; 4],
) -> Vec<WireModel> {
    let s = if scale_divisor.abs() > 1e-9 {
        1.0 / scale_divisor
    } else {
        1.0
    };
    let Some(mut block_use) = crate::scene::render_graph::block_use_from_handle(
        doc,
        block,
        crate::scene::render_graph::BlockRole::LineStyleSymbol,
        Vector3::new(at[0], at[1], at[2]),
    ) else {
        return Vec::new();
    };
    let insert = &mut block_use.insert;
    insert.set_x_scale(s);
    insert.set_y_scale(s);
    insert.set_z_scale(s);
    let depths = rustc_hash::FxHashMap::default();
    let graph = crate::scene::render_graph::RenderSceneGraph::new(doc, None, None, true, &depths)
        .with_annotation_scale(anno_scale);
    let mut out = Vec::new();
    graph.walk_insert(
        insert,
        block,
        |_, _| true,
        |entity, context| {
            let mut placed = entity.clone();
            placed.apply_transform(&context.transform);
            out.extend(super::tessellate::tessellate(
                doc,
                entity.common().handle,
                &placed,
                false,
                color,
                0.0,
                [0.0; 8],
                line_weight_px,
                anno_scale,
                None,
                world_per_pixel,
                bg_color,
                false,
            ));
        },
    );
    out
}
