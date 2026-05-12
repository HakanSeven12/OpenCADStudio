// Tessellation — convert acadrust EntityType to GPU-ready WireModel or MeshModel.
//
// Flow:
//   EntityType
//     ↓  acad_to_truck::convert()
//   TruckEntity  { object: TruckObject, snap_pts, tangent_geoms, key_vertices }
//     ↓  truck_tess::tessellate_*()
//   TruckTessResult::Lines → WireModel
//   TruckTessResult::Point → WireModel (small cross)
//   TruckTessResult::Mesh  → MeshModel
//   TruckObject::Text      → one WireModel per glyph stroke (elevation from entity Z)
//
// Entities not handled by acad_to_truck (Viewport, Hatch, …) are tessellated
// by the legacy geometry() path so nothing regresses.

use crate::entities::multileader::{catmull_rom_pts, v_offset_for_attachment};
use acadrust::entities::{
    Dimension, Leader, LeaderContentType, MultiLeader, MultiLeaderPathType, Text,
    TextAttachmentPointType,
};
use acadrust::types::{Color as AcadColor, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use glam::Vec3;

use crate::scene::acad_to_truck::{convert, TruckObject};
use crate::scene::mesh_model::MeshModel;
use crate::scene::truck_tess::{
    self, tessellate_edge, tessellate_solid, tessellate_vertex, tessellate_wire, TruckTessResult,
};
use crate::scene::wire_model::{SnapHint, TangentGeom, WireModel};

const TAU: f64 = std::f64::consts::TAU;

// ── Colour helper ──────────────────────────────────────────────────────────

/// Convert an acadrust Color (ACI index or true-color) to a GPU RGBA value.
pub fn aci_to_rgba(color: &AcadColor) -> [f32; 4] {
    if let Some((r, g, b)) = color.rgb() {
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    } else {
        WireModel::WHITE
    }
}

// ── Public entry points ────────────────────────────────────────────────────

/// Tessellate one entity into a WireModel.
/// For Text/MText entities this produces one WireModel with all glyph strokes
/// encoded as NaN-separated segments (wire_gpu skips NaN pairs).
/// For Solid3D entities this returns an empty wire; use `tessellate_mesh` instead.
pub fn tessellate(
    document: &CadDocument,
    handle: Handle,
    entity: &EntityType,
    selected: bool,
    entity_color: [f32; 4],
    pattern_length: f32,
    pattern: [f32; 8],
    line_weight_px: f32,
    world_offset: [f64; 3],
    anno_scale: f32,
) -> WireModel {
    let color = if selected {
        WireModel::SELECTED
    } else {
        entity_color
    };
    let name = handle.value().to_string();

    // Determine effective annotation scale for this entity.
    // AutoCAD marks annotative objects with "AcAnnoPO" xdata (R2007+).
    // If no xdata at all: old file without annotation system → treat as annotative.
    // If xdata present but no "AcAnnoPO": R2007+ file, entity is non-annotative → no scaling.
    let anno_scale = {
        let xdata = &entity.common().extended_data;
        let is_annotative = if xdata.is_empty() {
            // Old DXF / no annotation metadata — treat as annotative.
            true
        } else {
            xdata.get_record("AcAnnoPO").is_some()
        };
        if is_annotative {
            anno_scale
        } else {
            1.0
        }
    };

    // MultiLeader is handled by scene/mod.rs since it emits multiple WireModels
    // (leader, text, frame, fill) with distinct colors.
    if let EntityType::Leader(leader) = entity {
        return tessellate_leader_single(
            handle,
            leader,
            selected,
            entity_color,
            line_weight_px,
            world_offset,
            anno_scale,
        );
    }

    // ── Try the truck path first ───────────────────────────────────────────
    if let Some(te) = convert(entity, document) {
        match te.object {
            // ── Text / MText: pre-tessellated glyph strokes ───────────────
            TruckObject::Text(stroke_groups) => {
                // Each TextStroke keeps its strokes in glyph-local space and
                // its world origin as f64.  Subtract world_offset in f64 before
                // casting to f32 so large UTM coordinates don't crush precision.
                let [ox, oy, oz] = world_offset;
                let elev = entity_z(entity) - oz as f32;

                let mut points: Vec<[f32; 3]> = Vec::new();
                let mut first = true;
                // Annotation scale: scale glyph strokes relative to the text
                // insertion point (first group's origin) so multi-line MText
                // lines spread apart correctly as well as growing in size.
                let ref_origin = stroke_groups
                    .first()
                    .map(|g| g.origin)
                    .unwrap_or([0.0, 0.0]);
                let ref_lx = (ref_origin[0] - ox) as f32;
                let ref_ly = (ref_origin[1] - oy) as f32;
                for group in &stroke_groups {
                    let lx = (group.origin[0] - ox) as f32;
                    let ly = (group.origin[1] - oy) as f32;
                    let slx = (lx - ref_lx) * anno_scale + ref_lx;
                    let sly = (ly - ref_ly) * anno_scale + ref_ly;
                    for stroke in &group.strokes {
                        if stroke.len() < 2 {
                            continue;
                        }
                        if !first && !points.is_empty() {
                            points.push([f32::NAN, f32::NAN, f32::NAN]);
                        }
                        first = false;
                        for &[x, y] in stroke {
                            points.push([x * anno_scale + slx, y * anno_scale + sly, elev]);
                        }
                    }
                }

                let snap_pts = offset_snap_pts(te.snap_pts, world_offset);
                let [ox, oy, oz] = world_offset;
                let key_vertices: Vec<[f32; 3]> = te
                    .key_vertices
                    .into_iter()
                    .map(|[x, y, z]| [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32])
                    .collect();
                return WireModel {
                    name,
                    points,
                    color,
                    selected,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px,
                    snap_pts,
                    tangent_geoms: te.tangent_geoms,
                    aci: 0,
                    key_vertices,
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    vp_scissor: None,
                    fill_tris: vec![],
                };
            }

            // ── Standard topology objects ─────────────────────────────────
            TruckObject::Point(v) => {
                let result = tessellate_vertex(&v, world_offset);
                match result {
                    TruckTessResult::Point([x, y, z]) => {
                        let s = 0.1_f32;
                        let snap_pts = offset_snap_pts(te.snap_pts, world_offset);
                        let [ox, oy, oz] = world_offset;
                        let key_vertices: Vec<[f32; 3]> = te
                            .key_vertices
                            .into_iter()
                            .map(|[kx, ky, kz]| {
                                [(kx - ox) as f32, (ky - oy) as f32, (kz - oz) as f32]
                            })
                            .collect();
                        return WireModel {
                            name,
                            points: vec![
                                [x - s, y, z],
                                [x + s, y, z],
                                [x, y - s, z],
                                [x, y + s, z],
                            ],
                            color,
                            selected,
                            pattern_length: 0.0,
                            pattern: [0.0; 8],
                            line_weight_px: 1.0,
                            snap_pts,
                            tangent_geoms: te.tangent_geoms,
                            aci: 0,
                            key_vertices,
                            aabb: WireModel::UNBOUNDED_AABB,
                            plinegen: true,
                            vp_scissor: None,
                            fill_tris: vec![],
                        };
                    }
                    _ => {}
                }
            }

            TruckObject::Curve(e) => {
                if let TruckTessResult::Lines(points) = tessellate_edge(&e, world_offset) {
                    let [ox, oy, oz] = world_offset;
                    let snap_pts = offset_snap_pts(te.snap_pts, world_offset);
                    let key_vertices: Vec<[f32; 3]> = te
                        .key_vertices
                        .into_iter()
                        .map(|[x, y, z]| [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32])
                        .collect();
                    return WireModel {
                        name,
                        points,
                        color,
                        selected,
                        pattern_length,
                        pattern,
                        line_weight_px,
                        snap_pts,
                        tangent_geoms: te.tangent_geoms,
                        aci: 0,
                        key_vertices,
                        aabb: WireModel::UNBOUNDED_AABB,
                        plinegen: true,
                        vp_scissor: None,
                        fill_tris: vec![],
                    };
                }
            }

            TruckObject::Contour(w) => {
                if let TruckTessResult::Lines(points) = tessellate_wire(&w, world_offset) {
                    let [ox, oy, oz] = world_offset;
                    let snap_pts = offset_snap_pts(te.snap_pts, world_offset);
                    let key_vertices: Vec<[f32; 3]> = te
                        .key_vertices
                        .into_iter()
                        .map(|[x, y, z]| [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32])
                        .collect();
                    return WireModel {
                        name,
                        points,
                        color,
                        selected,
                        pattern_length,
                        pattern,
                        line_weight_px,
                        snap_pts,
                        tangent_geoms: te.tangent_geoms,
                        aci: 0,
                        key_vertices,
                        aabb: WireModel::UNBOUNDED_AABB,
                        plinegen: true,
                        vp_scissor: None,
                        fill_tris: vec![],
                    };
                }
            }

            TruckObject::Lines(points) => {
                // Points are world-space f64 from entity converters (polyline,
                // leader, mesh, solid2d, etc.). Subtract world_offset in f64
                // before casting to f32 so drawings at large UTM-style
                // coordinates keep sub-unit precision in the wire model.
                let [ox, oy, oz] = world_offset;
                let local_pts: Vec<[f32; 3]> = points
                    .into_iter()
                    .map(|[x, y, z]| {
                        if x.is_nan() {
                            [f32::NAN, f32::NAN, f32::NAN]
                        } else {
                            [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32]
                        }
                    })
                    .collect();
                let snap_pts = offset_snap_pts(te.snap_pts, world_offset);
                let key_vertices: Vec<[f32; 3]> = te
                    .key_vertices
                    .into_iter()
                    .map(|[x, y, z]| [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32])
                    .collect();
                let fill_tris: Vec<[f32; 3]> = te
                    .fill_tris
                    .into_iter()
                    .map(|[x, y, z]| [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32])
                    .collect();
                return WireModel {
                    name,
                    points: local_pts,
                    color,
                    selected,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px,
                    snap_pts,
                    tangent_geoms: te.tangent_geoms,
                    aci: 0,
                    key_vertices,
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    vp_scissor: None,
                    fill_tris,
                };
            }

            TruckObject::SegmentedLines(points) => {
                let [ox, oy, oz] = world_offset;
                let local_pts: Vec<[f32; 3]> = points
                    .into_iter()
                    .map(|[x, y, z]| {
                        if x.is_nan() {
                            [f32::NAN, f32::NAN, f32::NAN]
                        } else {
                            [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32]
                        }
                    })
                    .collect();
                let snap_pts = offset_snap_pts(te.snap_pts, world_offset);
                let key_vertices: Vec<[f32; 3]> = te
                    .key_vertices
                    .into_iter()
                    .map(|[x, y, z]| [(x - ox) as f32, (y - oy) as f32, (z - oz) as f32])
                    .collect();
                return WireModel {
                    name,
                    points: local_pts,
                    color,
                    selected,
                    pattern_length,
                    pattern,
                    line_weight_px,
                    snap_pts,
                    tangent_geoms: te.tangent_geoms,
                    aci: 0,
                    key_vertices,
                    plinegen: false,
                    vp_scissor: None,
                    aabb: WireModel::UNBOUNDED_AABB,
                    fill_tris: vec![],
                };
            }

            TruckObject::Volume(_) => {
                // Solid3D / Region / Body → handled by tessellate_mesh().
                // As a wire fallback, render the pre-computed edge wires
                // stored in the entity when present (e.g. from SOLVIEW output
                // or when the SAT kernel cannot parse the ACIS data).
                let wire_pts = solid_wire_fallback(entity, world_offset);
                let mut wm = WireModel::solid(name, wire_pts, color, selected);
                // Add insertion snap at point_of_reference.
                let [ox, oy, oz] = world_offset;
                let por = match entity {
                    EntityType::Solid3D(s) => Some(&s.point_of_reference),
                    EntityType::Region(r) => Some(&r.point_of_reference),
                    EntityType::Body(b) => Some(&b.point_of_reference),
                    _ => None,
                };
                if let Some(p) = por {
                    let sp = Vec3::new((p.x - ox) as f32, (p.y - oy) as f32, (p.z - oz) as f32);
                    wm.snap_pts.push((sp, SnapHint::Insertion));
                }
                return wm;
            }
        }
    }

    // ── Legacy fallback for Viewport and other unhandled types ────────────
    let (points, snap_pts, tangent_geoms, key_vertices) = legacy_geometry(entity, world_offset);
    WireModel {
        name,
        points,
        color,
        selected,
        aci: 0,
        pattern_length,
        pattern,
        line_weight_px,
        snap_pts,
        tangent_geoms,
        key_vertices,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
    }
}

pub fn tessellate_dimension(
    document: &CadDocument,
    handle: Handle,
    dim: &Dimension,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    world_offset: [f64; 3],
    anno_scale: f32,
) -> Vec<WireModel> {
    let color = if selected {
        WireModel::SELECTED
    } else {
        entity_color
    };
    let name = handle.value().to_string();
    let style_name = &dim.base().style_name;
    let (arrow_size, dimexo, dimexe) = document
        .dim_styles
        .iter()
        .find(|s| {
            s.name.eq_ignore_ascii_case(style_name)
                || (style_name.trim().is_empty() && s.name.eq_ignore_ascii_case("Standard"))
        })
        .map(|s| {
            let scale = (if s.dimscale > 1e-6 { s.dimscale } else { 1.0 }) * anno_scale as f64;
            (
                ((s.dimasz * scale) as f32).max(0.001),
                (s.dimexo * scale) as f32,
                (s.dimexe * scale) as f32,
            )
        })
        .unwrap_or((0.12, 0.0, 0.0));
    let points = dimension_geometry(dim, arrow_size, dimexo, dimexe, world_offset);
    let key_vertices = points
        .iter()
        .copied()
        .filter(|p| !(p[0].is_nan() || p[1].is_nan() || p[2].is_nan()))
        .collect();

    let snap_pts = dimension_snap_pts(dim, world_offset);

    let mut wires = vec![WireModel {
        name: name.clone(),
        points,
        color,
        selected,
        aci: 0,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px,
        snap_pts,
        tangent_geoms: vec![],
        key_vertices,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
    }];

    if let Some(text) = dimension_text_entity(dim) {
        let mut wire = tessellate(
            document,
            handle,
            &EntityType::Text(text),
            selected,
            entity_color,
            0.0,
            [0.0; 8],
            line_weight_px,
            world_offset,
            anno_scale,
        );
        wire.name = name;
        wires.push(wire);
    }

    wires
}

fn tessellate_leader_single(
    handle: Handle,
    leader: &Leader,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    world_offset: [f64; 3],
    anno_scale: f32,
) -> WireModel {
    let color = if selected {
        WireModel::SELECTED
    } else {
        entity_color
    };
    let name = handle.value().to_string();
    let [ox, oy, oz] = world_offset;
    let p3 =
        |v: &Vector3| -> [f32; 3] { [(v.x - ox) as f32, (v.y - oy) as f32, (v.z - oz) as f32] };
    let nan = [f32::NAN; 3];

    let verts = &leader.vertices;

    if verts.len() < 2 {
        return WireModel {
            name,
            points: vec![],
            color,
            selected,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px,
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            vp_scissor: None,
            fill_tris: vec![],
        };
    }

    let mut points: Vec<[f32; 3]> = verts.iter().map(|v| p3(v)).collect();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let key_vertices: Vec<[f32; 3]> = verts.iter().map(|v| p3(v)).collect();

    for i in 0..verts.len().saturating_sub(1) {
        tangents.push(TangentGeom::Line {
            p1: p3(&verts[i]),
            p2: p3(&verts[i + 1]),
        });
    }

    if leader.arrow_enabled {
        let tip = &verts[0];
        let next = &verts[1];
        let dx = (next.x - tip.x) as f32;
        let dy = (next.y - tip.y) as f32;
        let len = (dx * dx + dy * dy).sqrt().max(1e-9);
        let (dx, dy) = (dx / len, dy / len);
        let sz = (leader.text_height as f32).max(1.0) * 0.8 * anno_scale;
        let a = std::f32::consts::PI / 6.0;
        let (s, c) = a.sin_cos();
        let tip_f = p3(tip);
        points.push(nan);
        points.push([
            tip_f[0] + (dx * c - dy * s) * sz,
            tip_f[1] + (dx * s + dy * c) * sz,
            tip_f[2],
        ]);
        points.push(tip_f);
        points.push([
            tip_f[0] + (dx * c + dy * s) * sz,
            tip_f[1] + (-dx * s + dy * c) * sz,
            tip_f[2],
        ]);
    }

    if leader.hookline_enabled {
        let last = verts.last().unwrap();
        let prev = &verts[verts.len() - 2];
        let sign = if (last.x - prev.x) >= 0.0 {
            1.0_f32
        } else {
            -1.0_f32
        };
        let land_len = leader.text_height as f32 * 1.5 * anno_scale;
        let last_f = p3(last);
        points.push(nan);
        points.push(last_f);
        points.push([last_f[0] + sign * land_len, last_f[1], last_f[2]]);
    }

    WireModel {
        name,
        points,
        color,
        selected,
        aci: 0,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px,
        snap_pts: vec![],
        tangent_geoms: tangents,
        key_vertices,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
    }
}

/// Convert an acadrust `Color` to RGBA, falling back to `inherited` for
/// `ByLayer` / `ByBlock` (assumes those are already resolved upstream).
fn color_or_inherit(c: &AcadColor, inherited: [f32; 4]) -> [f32; 4] {
    match c.rgb() {
        Some((r, g, b)) => [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            inherited[3],
        ],
        None => inherited,
    }
}

pub fn tessellate_multileader(
    document: &CadDocument,
    handle: Handle,
    ml: &MultiLeader,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    world_offset: [f64; 3],
    anno_scale: f32,
) -> Vec<WireModel> {
    let line_color = if selected {
        WireModel::SELECTED
    } else {
        entity_color
    };
    let name = handle.value().to_string();
    let nan = [f32::NAN; 3];
    let [ox, oy, oz] = world_offset;
    let p3 = |v: &acadrust::types::Vector3| -> [f32; 3] {
        [(v.x - ox) as f32, (v.y - oy) as f32, (v.z - oz) as f32]
    };

    // ── Scaling ──────────────────────────────────────────────────────────────
    // ml.scale_factor is always applied; anno_scale is only applied when the
    // multileader is marked annotative.
    let effective_scale = (ml.scale_factor as f32)
        * if ml.enable_annotation_scale {
            anno_scale
        } else {
            1.0
        };

    let arrow_size = ml.arrowhead_size as f32 * effective_scale;
    let draw_arrow = arrow_size > 0.0;
    let invisible = ml.path_type == MultiLeaderPathType::Invisible;

    // ── Leader / arrow / dogleg points ───────────────────────────────────────
    let mut points: Vec<[f32; 3]> = Vec::new();
    let mut key_verts: Vec<[f32; 3]> = Vec::new();
    let mut snap_pts: Vec<(Vec3, SnapHint)> = Vec::new();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut first = true;

    for root in &ml.context.leader_roots {
        let cp = &root.connection_point;
        let cp_f = p3(cp);
        snap_pts.push((Vec3::from(cp_f), SnapHint::Node));

        for line in &root.lines {
            if line.points.is_empty() {
                continue;
            }

            if !invisible {
                if !first {
                    points.push(nan);
                }
                first = false;

                let mut ctrl: Vec<[f32; 3]> = line.points.iter().map(|p| p3(p)).collect();
                let last_f = *ctrl.last().unwrap_or(&cp_f);
                let dist = ((last_f[0] - cp_f[0]).powi(2) + (last_f[1] - cp_f[1]).powi(2)).sqrt();
                if dist > 1e-9 {
                    ctrl.push(cp_f);
                }
                for &c in &ctrl {
                    key_verts.push(c);
                    snap_pts.push((Vec3::from(c), SnapHint::Node));
                }

                if ml.path_type == MultiLeaderPathType::Spline && ctrl.len() >= 2 {
                    let ctrl_f64: Vec<[f64; 3]> = ctrl
                        .iter()
                        .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
                        .collect();
                    for pt in catmull_rom_pts(&ctrl_f64, 8) {
                        points.push([pt[0] as f32, pt[1] as f32, pt[2] as f32]);
                    }
                } else {
                    for &c in &ctrl {
                        points.push(c);
                    }
                }
                for i in 0..ctrl.len().saturating_sub(1) {
                    tangents.push(TangentGeom::Line {
                        p1: ctrl[i],
                        p2: ctrl[i + 1],
                    });
                }
            }

            if draw_arrow {
                let tip = &line.points[0];
                let tip_f = p3(tip);
                let next = if line.points.len() >= 2 {
                    line.points[1]
                } else {
                    *cp
                };
                let dx = (next.x - tip.x) as f32;
                let dy = (next.y - tip.y) as f32;
                let dl = (dx * dx + dy * dy).sqrt().max(1e-9);
                let (dx, dy) = (dx / dl, dy / dl);
                let a = std::f32::consts::PI / 6.0;
                let (s, c) = a.sin_cos();
                points.push(nan);
                points.push([
                    tip_f[0] + (dx * c - dy * s) * arrow_size,
                    tip_f[1] + (dx * s + dy * c) * arrow_size,
                    tip_f[2],
                ]);
                points.push(tip_f);
                points.push([
                    tip_f[0] + (dx * c + dy * s) * arrow_size,
                    tip_f[1] + (-dx * s + dy * c) * arrow_size,
                    tip_f[2],
                ]);
            }
        }

        if ml.enable_landing && ml.enable_dogleg && ml.dogleg_length > 0.0 {
            let dir = &root.direction;
            let dl = (dir.x * dir.x + dir.y * dir.y).sqrt().max(1e-9);
            let d = ml.dogleg_length * effective_scale as f64;
            points.push(nan);
            points.push(cp_f);
            points.push([
                (cp.x + dir.x / dl * d - ox) as f32,
                (cp.y + dir.y / dl * d - oy) as f32,
                cp_f[2],
            ]);
        }
    }

    // The leader/arrow/dogleg wire goes out as a single WireModel. Text, frame,
    // and background fill (each with their own color) are appended as separate
    // WireModels so the renderer respects per-piece coloring.
    let mut wires: Vec<WireModel> = Vec::new();
    wires.push(WireModel {
        name: name.clone(),
        points,
        color: line_color,
        selected,
        aci: 0,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px,
        snap_pts,
        tangent_geoms: tangents,
        key_vertices: key_verts,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        vp_scissor: None,
        fill_tris: vec![],
    });

    // ── Text strokes / frame / background fill ──────────────────────────────
    // Strip inline format codes, split / word-wrap into lines, then place each
    // line according to text_attachment_point (horizontal) and
    // text_left_attachment (vertical), with text_rotation/text_direction applied.
    if ml.content_type == LeaderContentType::MText && !ml.context.text_string.is_empty() {
        let ctx = &ml.context;
        let raw_height = if ctx.text_height > 0.0 {
            ctx.text_height
        } else {
            ml.text_height
        } as f32;
        let height = raw_height * effective_scale;

        let ins = &ctx.text_location;
        // Subtract world_offset in f64 before casting to f32: drawings often
        // sit at large absolute coordinates and casting first then subtracting
        // throws away the precision needed for the rotated sub-glyph offsets.
        let local_ins_x = (ins.x - ox) as f32;
        let local_ins_y = (ins.y - oy) as f32;
        let z = (ins.z - oz) as f32;

        // Rotation: prefer text_direction (transforms survive rotations / mirrors
        // when acadrust updates it) and fall back to text_rotation.
        let td = ctx.text_direction;
        let rot = if td.x.abs() > 1e-9 || td.y.abs() > 1e-9 {
            (td.y as f32).atan2(td.x as f32)
        } else {
            ctx.text_rotation as f32
        };
        let (cos_r, sin_r) = (rot.cos(), rot.sin());

        // Resolve text style via handle when available, falling back to STANDARD.
        let style_name = ctx
            .text_style_handle
            .as_ref()
            .and_then(|h| {
                document
                    .text_styles
                    .iter()
                    .find(|s| s.handle == *h)
                    .map(|s| s.name.clone())
            })
            .unwrap_or_else(|| "STANDARD".to_string());
        let style = crate::entities::text_support::resolve_text_style(&style_name, document);
        let font_name = style.font_name;
        let font = crate::scene::cxf::get_font(&font_name);
        let width_factor = style.width_factor.max(0.01);
        let oblique = style.oblique_angle;

        // Strip MText format codes (e.g. `{\fArial Black|b0|i0|c162|p34;...}`),
        // then split on \P / \n / \N and optionally word-wrap to text_width.
        let plain = crate::entities::text_support::strip_mtext_codes(&ctx.text_string);
        let explicit_lines = crate::entities::text_support::split_mtext_lines(&plain);
        let lines: Vec<String> = if ctx.text_width > 0.0 {
            let scale = height / 9.0 * width_factor;
            let max_w = ctx.text_width as f32 * effective_scale;
            explicit_lines
                .iter()
                .flat_map(|line| {
                    crate::entities::text_support::word_wrap(line, max_w, scale, font)
                })
                .collect()
        } else {
            explicit_lines
        };

        let ls_factor = if ctx.line_spacing_factor > 0.0 {
            ctx.line_spacing_factor as f32
        } else {
            1.0
        };
        let line_h = height * ls_factor * (5.0 / 3.0) * font.line_spacing;
        let n_lines = lines.len().max(1) as f32;

        let h_anchor = match ctx.text_attachment_point {
            TextAttachmentPointType::Left => 0.0_f32,
            TextAttachmentPointType::Center => 0.5,
            TextAttachmentPointType::Right => 1.0,
        };
        // Vertical anchor: use text_left_attachment (matches the leader-to-text
        // attachment convention for the common case of left-side leaders).
        let v_offset =
            v_offset_for_attachment(ctx.text_left_attachment, n_lines, height, line_h);

        let scale = height / 9.0 * width_factor;
        let line_widths: Vec<f32> = lines
            .iter()
            .map(|line| crate::entities::text_support::measure_mtext_chars(line, scale, font))
            .collect();
        let max_line_w = line_widths.iter().cloned().fold(0.0_f32, f32::max);

        // Resolve text color (falls back to entity color for ByLayer / ByBlock).
        let text_color = if selected {
            line_color
        } else {
            color_or_inherit(&ctx.text_color, entity_color)
        };

        // Build per-line stroke points in WCS.
        let mut text_points: Vec<[f32; 3]> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let li = i as f32;
            let line_y_local = -li * line_h + v_offset;
            let line_w = line_widths[i];
            let h_shift_local = -line_w * h_anchor;
            let wcs_dx = h_shift_local * cos_r - line_y_local * sin_r;
            let wcs_dy = h_shift_local * sin_r + line_y_local * cos_r;
            // Origin already in offset-relative space — tessellator will rotate
            // glyph offsets around it and produce points directly in render space.
            let origin = [local_ins_x + wcs_dx, local_ins_y + wcs_dy];
            let strokes = crate::scene::cxf::tessellate_text_ex(
                origin,
                height,
                rot,
                width_factor,
                oblique,
                &font_name,
                line,
            );
            for stroke in &strokes {
                if stroke.len() < 2 {
                    continue;
                }
                text_points.push(nan);
                for &[x, y] in stroke {
                    text_points.push([x, y, z]);
                }
            }
        }

        if !text_points.is_empty() {
            wires.push(WireModel {
                name: name.clone(),
                points: text_points,
                color: text_color,
                selected,
                aci: 0,
                pattern_length: 0.0,
                pattern: [0.0; 8],
                line_weight_px,
                snap_pts: vec![(
                    Vec3::new(local_ins_x, local_ins_y, z),
                    SnapHint::Node,
                )],
                tangent_geoms: vec![],
                key_vertices: vec![],
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                vp_scissor: None,
                fill_tris: vec![],
            });
        }

        // Text frame / background-fill rectangle in local frame, then rotated to WCS.
        if ml.text_frame || ctx.background_fill_enabled {
            // Visual gap so the frame/fill doesn't touch glyph caps.
            let pad = height * 0.25;
            let block_top = v_offset + height + pad;
            let block_bottom = v_offset - (n_lines - 1.0) * line_h - pad;
            let block_left = -max_line_w * h_anchor - pad;
            let block_right = max_line_w * (1.0 - h_anchor) + pad;
            let local_corners: [[f32; 2]; 4] = [
                [block_left, block_bottom],
                [block_right, block_bottom],
                [block_right, block_top],
                [block_left, block_top],
            ];
            let wcs_corners: [[f32; 3]; 4] = std::array::from_fn(|i| {
                let lx = local_corners[i][0];
                let ly = local_corners[i][1];
                let wx = local_ins_x + lx * cos_r - ly * sin_r;
                let wy = local_ins_y + lx * sin_r + ly * cos_r;
                [wx, wy, z]
            });

            // Background fill — emit two triangles; renders under the text strokes.
            if ctx.background_fill_enabled {
                let fill_color = if selected {
                    line_color
                } else {
                    color_or_inherit(&ctx.background_fill_color, entity_color)
                };
                let fill_tris: Vec<[f32; 3]> = vec![
                    wcs_corners[0],
                    wcs_corners[1],
                    wcs_corners[2],
                    wcs_corners[0],
                    wcs_corners[2],
                    wcs_corners[3],
                ];
                wires.push(WireModel {
                    name: name.clone(),
                    points: vec![],
                    color: fill_color,
                    selected,
                    aci: 0,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px: 1.0,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    vp_scissor: None,
                    fill_tris,
                });
            }

            // Text frame — closed rectangle, matches text color.
            if ml.text_frame {
                let frame_points: Vec<[f32; 3]> = vec![
                    wcs_corners[0],
                    wcs_corners[1],
                    wcs_corners[2],
                    wcs_corners[3],
                    wcs_corners[0],
                ];
                wires.push(WireModel {
                    name,
                    points: frame_points,
                    color: text_color,
                    selected,
                    aci: 0,
                    pattern_length: 0.0,
                    pattern: [0.0; 8],
                    line_weight_px,
                    snap_pts: vec![],
                    tangent_geoms: vec![],
                    key_vertices: vec![],
                    aabb: WireModel::UNBOUNDED_AABB,
                    plinegen: true,
                    vp_scissor: None,
                    fill_tris: vec![],
                });
            }
        }
    }

    wires
}

/// Tessellate a Solid3D entity into a MeshModel (truck Shell/Solid path).
#[allow(dead_code)]
pub fn tessellate_mesh(
    document: &CadDocument,
    handle: Handle,
    entity: &EntityType,
    selected: bool,
    color: [f32; 4],
    world_offset: [f64; 3],
) -> Option<MeshModel> {
    let te = convert(entity, document)?;
    let result = match te.object {
        TruckObject::Volume(solid) => tessellate_solid(&solid, world_offset),
        _ => return None,
    };
    truck_tess::tess_to_mesh_model(
        result,
        handle.value().to_string(),
        if selected { MeshModel::SELECTED } else { color },
        selected,
    )
}

// ── Entity Z helper ───────────────────────────────────────────────────────

/// Extract the Z elevation from a text/mtext entity.
fn entity_z(entity: &EntityType) -> f32 {
    match entity {
        EntityType::Text(t) => t.insertion_point.z as f32,
        EntityType::MText(t) => t.insertion_point.z as f32,
        _ => 0.0,
    }
}

// ── Legacy geometry (Viewport, Hatch outline, unrecognised) ───────────────

type Geometry = (
    Vec<[f32; 3]>,
    Vec<(Vec3, SnapHint)>,
    Vec<TangentGeom>,
    Vec<[f32; 3]>,
);

fn legacy_geometry(entity: &EntityType, world_offset: [f64; 3]) -> Geometry {
    let [ox, oy, oz] = world_offset;
    match entity {
        EntityType::Viewport(vp) => {
            let cx = (vp.center.x - ox) as f32;
            let cy = (vp.center.y - oy) as f32;
            let cz = (vp.center.z - oz) as f32;
            let hw = (vp.width / 2.0) as f32;
            let hh = (vp.height / 2.0) as f32;
            let pts = vec![
                [cx - hw, cy - hh, cz],
                [cx + hw, cy - hh, cz],
                [cx + hw, cy + hh, cz],
                [cx - hw, cy + hh, cz],
                [cx - hw, cy - hh, cz],
            ];
            (pts, vec![], vec![], vec![])
        }
        EntityType::Insert(ins) => {
            let ip = Vec3::new(
                (ins.insert_point.x - ox) as f32,
                (ins.insert_point.y - oy) as f32,
                (ins.insert_point.z - oz) as f32,
            );
            let s = 0.1_f32;
            let pts = vec![
                [ip.x - s, ip.y, ip.z],
                [ip.x + s, ip.y, ip.z],
                [ip.x, ip.y - s, ip.z],
                [ip.x, ip.y + s, ip.z],
            ];
            (pts, vec![(ip, SnapHint::Insertion)], vec![], vec![])
        }
        EntityType::Hatch(h) => {
            let normal = (h.normal.x, h.normal.y, h.normal.z);
            // Convert a 2D OCS hatch boundary point to WCS, then subtract world_offset.
            let to_wcs = |x: f64, y: f64| -> [f32; 3] {
                let (wx, wy, wz) =
                    crate::scene::transform::ocs_point_to_wcs((x, y, h.elevation), normal);
                [(wx - ox) as f32, (wy - oy) as f32, (wz - oz) as f32]
            };
            let mut pts: Vec<[f32; 3]> = Vec::new();
            let mut key_verts: Vec<[f32; 3]> = Vec::new();
            let mut snap_pts: Vec<(Vec3, SnapHint)> = Vec::new();
            for path in &h.paths {
                for edge in &path.edges {
                    match edge {
                        acadrust::entities::BoundaryEdge::Polyline(poly) => {
                            let start_idx = pts.len();
                            for v in &poly.vertices {
                                let p = to_wcs(v.x, v.y);
                                pts.push(p);
                                key_verts.push(p);
                            }
                            if let Some(first) = pts.get(start_idx).cloned() {
                                pts.push(first);
                            }
                        }
                        acadrust::entities::BoundaryEdge::Line(ln) => {
                            let p0 = to_wcs(ln.start.x, ln.start.y);
                            let p1 = to_wcs(ln.end.x, ln.end.y);
                            if !pts.is_empty() {
                                pts.push([f32::NAN; 3]);
                            }
                            pts.push(p0);
                            pts.push(p1);
                            key_verts.push(p0);
                            key_verts.push(p1);
                        }
                        acadrust::entities::BoundaryEdge::CircularArc(arc) => {
                            let sa = arc.start_angle;
                            let ea = arc.end_angle;
                            let ccw = arc.counter_clockwise;
                            let (sa, ea) = if ccw { (sa, ea) } else { (TAU - sa, TAU - ea) };
                            let span = ea - sa;
                            let segs = ((span.abs() / TAU) * 32.0).ceil().max(4.0) as u32;
                            if !pts.is_empty() {
                                pts.push([f32::NAN; 3]);
                            }
                            for i in 0..=segs {
                                let t = sa + span * (i as f64 / segs as f64);
                                let p = to_wcs(
                                    arc.center.x + arc.radius * t.cos() as f64,
                                    arc.center.y + arc.radius * t.sin() as f64,
                                );
                                pts.push(p);
                                if i == 0 || i == segs {
                                    key_verts.push(p);
                                }
                            }
                            snap_pts.push((
                                Vec3::from(to_wcs(arc.center.x, arc.center.y)),
                                SnapHint::Center,
                            ));
                        }
                        acadrust::entities::BoundaryEdge::EllipticArc(ell) => {
                            let r_maj = ((ell.major_axis_endpoint.x * ell.major_axis_endpoint.x
                                + ell.major_axis_endpoint.y * ell.major_axis_endpoint.y)
                                .sqrt()) as f32;
                            let r_min = r_maj * ell.minor_axis_ratio as f32;
                            let rot = (ell.major_axis_endpoint.y as f32)
                                .atan2(ell.major_axis_endpoint.x as f32);
                            const TAU: f32 = std::f32::consts::TAU;
                            let ccw = ell.counter_clockwise;
                            let sa = ell.start_angle as f32;
                            let ea = ell.end_angle as f32;
                            let (sa, ea) = if ccw {
                                (sa, ea)
                            } else {
                                (TAU - sa, TAU - ea)
                            };
                            let span = ea - sa;
                            let segs =
                                ((span.abs() / TAU) * 32.0).ceil().max(4.0) as u32;
                            if !pts.is_empty() {
                                pts.push([f32::NAN; 3]);
                            }
                            for i in 0..=segs {
                                let t = sa + span * (i as f32 / segs as f32);
                                let lx = r_maj * t.cos();
                                let ly = r_min * t.sin();
                                let ocs_x = ell.center.x + (lx * rot.cos() - ly * rot.sin()) as f64;
                                let ocs_y = ell.center.y + (lx * rot.sin() + ly * rot.cos()) as f64;
                                let p = to_wcs(ocs_x, ocs_y);
                                pts.push(p);
                                if i == 0 || i == segs {
                                    key_verts.push(p);
                                }
                            }
                            snap_pts.push((
                                Vec3::from(to_wcs(ell.center.x, ell.center.y)),
                                SnapHint::Center,
                            ));
                        }
                        _ => {}
                    }
                }
            }
            if pts.is_empty() {
                pts = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
            }
            (pts, snap_pts, vec![], key_verts)
        }
        EntityType::Ole2Frame(ole) => {
            // OLE objects carry a bounding rectangle in model space.
            // Render a simple X-through-rectangle placeholder.
            let x0 = (ole.upper_left_corner.x - ox) as f32;
            let y0 = (ole.lower_right_corner.y - oy) as f32;
            let x1 = (ole.lower_right_corner.x - ox) as f32;
            let y1 = (ole.upper_left_corner.y - oy) as f32;
            let z = (ole.upper_left_corner.z - oz) as f32;
            if (x1 - x0).abs() < 1e-6 && (y1 - y0).abs() < 1e-6 {
                // Degenerate / unknown size — show a small cross.
                let s = 0.5_f32;
                return (vec![[-s, 0.0, 0.0], [s, 0.0, 0.0]], vec![], vec![], vec![]);
            }
            let pts = vec![
                // Outer rectangle
                [x0, y0, z],
                [x1, y0, z],
                [x1, y0, z],
                [x1, y1, z],
                [x1, y1, z],
                [x0, y1, z],
                [x0, y1, z],
                [x0, y0, z],
                // Diagonal X
                [x0, y0, z],
                [x1, y1, z],
                [f32::NAN, f32::NAN, f32::NAN],
                [x1, y0, z],
                [x0, y1, z],
            ];
            (pts, vec![], vec![], vec![[x0, y0, z], [x1, y1, z]])
        }
        _ => {
            let s = 0.5_f32;
            (vec![[-s, 0.0, 0.0], [s, 0.0, 0.0]], vec![], vec![], vec![])
        }
    }
}

/// Extract pre-computed edge-wire points from Solid3D / Region / Body entities.
///
/// AutoCAD stores explicit wire geometry (from SOLVIEW / 3DPLOT) alongside the
/// ACIS data.  We use this as a visible fallback when the SAT tessellator
/// produces no mesh (e.g. binary SAB data or unsupported geometry).
fn solid_wire_fallback(entity: &EntityType, world_offset: [f64; 3]) -> Vec<[f32; 3]> {
    let [ox, oy, oz] = world_offset;
    let wires: &[acadrust::entities::Wire] = match entity {
        EntityType::Solid3D(s) => &s.wires,
        EntityType::Region(r) => &r.wires,
        EntityType::Body(b) => &b.wires,
        _ => return vec![],
    };

    if wires.is_empty() {
        return vec![];
    }

    let mut pts: Vec<[f32; 3]> = Vec::new();
    for wire in wires {
        if wire.points.len() < 2 {
            continue;
        }
        for v in &wire.points {
            pts.push([(v.x - ox) as f32, (v.y - oy) as f32, (v.z - oz) as f32]);
        }
        // NaN sentinel separates distinct wire segments.
        pts.push([f32::NAN, f32::NAN, f32::NAN]);
    }
    pts
}

fn dimension_geometry(
    dim: &Dimension,
    arrow_size: f32,
    dimexo: f32,
    dimexe: f32,
    world_offset: [f64; 3],
) -> Vec<[f32; 3]> {
    let lv = |v| vec3_local(v, world_offset);
    let mut points = Vec::new();
    match dim {
        Dimension::Aligned(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let axis = normalized_or(second - first, Vec3::X);
            append_linear_dimension(
                &mut points,
                first,
                second,
                def,
                axis,
                arrow_size,
                dimexo,
                dimexe,
            );
        }
        Dimension::Linear(d) => {
            let first = lv(d.first_point);
            let second = lv(d.second_point);
            let def = lv(d.definition_point);
            let axis = Vec3::new(d.rotation.cos() as f32, d.rotation.sin() as f32, 0.0);
            append_linear_dimension(
                &mut points,
                first,
                second,
                def,
                normalized_or(axis, Vec3::X),
                arrow_size,
                dimexo,
                dimexe,
            );
        }
        Dimension::Radius(d) => {
            let center = lv(d.angle_vertex);
            let point = lv(d.definition_point);
            let text = dimension_text_position(dim, world_offset);
            add_segment(&mut points, center, point);
            add_segment(&mut points, point, text);
            append_arrow(
                &mut points,
                point,
                normalized_or(center - point, Vec3::X),
                arrow_size,
            );
        }
        Dimension::Diameter(d) => {
            let p1 = lv(d.angle_vertex);
            let p2 = lv(d.definition_point);
            add_segment(&mut points, p1, p2);
            append_arrow(&mut points, p1, normalized_or(p2 - p1, Vec3::X), arrow_size);
            append_arrow(&mut points, p2, normalized_or(p1 - p2, Vec3::X), arrow_size);
        }
        Dimension::Angular2Ln(d) => {
            append_angular_dimension(
                &mut points,
                lv(d.angle_vertex),
                lv(d.first_point),
                lv(d.second_point),
                lv(d.dimension_arc),
                arrow_size,
            );
        }
        Dimension::Angular3Pt(d) => {
            append_angular_dimension(
                &mut points,
                lv(d.angle_vertex),
                lv(d.first_point),
                lv(d.second_point),
                lv(d.definition_point),
                arrow_size,
            );
        }
        Dimension::Ordinate(d) => {
            add_segment(&mut points, lv(d.feature_location), lv(d.definition_point));
            add_segment(&mut points, lv(d.definition_point), lv(d.leader_endpoint));
        }
    }
    points
}

fn append_linear_dimension(
    points: &mut Vec<[f32; 3]>,
    first: Vec3,
    second: Vec3,
    def: Vec3,
    axis: Vec3,
    arrow_size: f32,
    dimexo: f32,
    dimexe: f32,
) {
    let perp = Vec3::new(-axis.y, axis.x, 0.0);
    let dim_line_pos = def.dot(perp);
    // Perpendicular offset from each defpoint to the dimension line (signed).
    let offset1 = dim_line_pos - first.dot(perp);
    let offset2 = dim_line_pos - second.dot(perp);
    let d1 = first + perp * offset1;
    let d2 = second + perp * offset2;
    // DIMEXO: gap from the definition point toward the dimension line.
    // DIMEXE: overshoot past the dimension line.
    let sign1 = if offset1 >= 0.0 { 1.0_f32 } else { -1.0 };
    let sign2 = if offset2 >= 0.0 { 1.0_f32 } else { -1.0 };
    let ext1_start = first + perp * (sign1 * dimexo);
    let ext1_end = d1 + perp * (sign1 * dimexe);
    let ext2_start = second + perp * (sign2 * dimexo);
    let ext2_end = d2 + perp * (sign2 * dimexe);
    add_segment(points, ext1_start, ext1_end);
    add_segment(points, ext2_start, ext2_end);
    add_segment(points, d1, d2);
    append_arrow(points, d1, normalized_or(d2 - d1, axis), arrow_size);
    append_arrow(points, d2, normalized_or(d1 - d2, -axis), arrow_size);
}

fn append_angular_dimension(
    points: &mut Vec<[f32; 3]>,
    vertex: Vec3,
    first: Vec3,
    second: Vec3,
    arc_point: Vec3,
    arrow_size: f32,
) {
    add_segment(points, vertex, first);
    add_segment(points, vertex, second);

    let radius = vertex.distance(arc_point);
    if radius <= 1e-6 {
        return;
    }

    let start = (first.y - vertex.y).atan2(first.x - vertex.x);
    let mut end = (second.y - vertex.y).atan2(second.x - vertex.x);
    let mut delta = end - start;
    while delta <= 0.0 {
        delta += std::f32::consts::TAU;
    }
    if delta > std::f32::consts::PI {
        end -= std::f32::consts::TAU;
        delta = end - start;
    }

    let steps = 32;
    let mut arc_pts = Vec::with_capacity((steps + 1) as usize);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = start + delta * t;
        arc_pts.push(vertex + Vec3::new(a.cos() * radius, a.sin() * radius, 0.0));
    }
    add_polyline(points, &arc_pts);

    if arc_pts.len() >= 2 {
        append_arrow(
            points,
            arc_pts[0],
            normalized_or(arc_pts[1] - arc_pts[0], Vec3::X),
            arrow_size,
        );
        let n = arc_pts.len();
        append_arrow(
            points,
            arc_pts[n - 1],
            normalized_or(arc_pts[n - 2] - arc_pts[n - 1], Vec3::X),
            arrow_size,
        );
    }
}

fn append_arrow(points: &mut Vec<[f32; 3]>, tip: Vec3, dir: Vec3, size: f32) {
    let dir = normalized_or(dir, Vec3::X) * size;
    let left = rotate(dir, 2.6);
    let right = rotate(dir, -2.6);
    add_segment(points, tip, tip + left);
    add_segment(points, tip, tip + right);
}

fn add_segment(points: &mut Vec<[f32; 3]>, a: Vec3, b: Vec3) {
    if !points.is_empty() {
        points.push([f32::NAN, f32::NAN, f32::NAN]);
    }
    points.push([a.x, a.y, a.z]);
    points.push([b.x, b.y, b.z]);
}

fn add_polyline(points: &mut Vec<[f32; 3]>, polyline: &[Vec3]) {
    if polyline.len() < 2 {
        return;
    }
    if !points.is_empty() {
        points.push([f32::NAN, f32::NAN, f32::NAN]);
    }
    points.extend(polyline.iter().map(|p| [p.x, p.y, p.z]));
}

fn dimension_snap_pts(dim: &Dimension, world_offset: [f64; 3]) -> Vec<(Vec3, SnapHint)> {
    let lv = |v: acadrust::types::Vector3| {
        Vec3::new(
            (v.x - world_offset[0]) as f32,
            (v.y - world_offset[1]) as f32,
            (v.z - world_offset[2]) as f32,
        )
    };
    let node = |v: acadrust::types::Vector3| (lv(v), SnapHint::Node);
    match dim {
        Dimension::Linear(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Aligned(d) => vec![
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Radius(d) => vec![node(d.angle_vertex), node(d.definition_point)],
        Dimension::Diameter(d) => vec![node(d.angle_vertex), node(d.definition_point)],
        Dimension::Angular2Ln(d) => vec![
            node(d.angle_vertex),
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Angular3Pt(d) => vec![
            node(d.angle_vertex),
            node(d.first_point),
            node(d.second_point),
            node(d.definition_point),
        ],
        Dimension::Ordinate(d) => vec![
            node(d.definition_point),
            node(d.feature_location),
            node(d.leader_endpoint),
        ],
    }
}

fn dimension_text_entity(dim: &Dimension) -> Option<Text> {
    let value = dimension_text_value(dim)?;
    // Use f64 position directly to avoid f32 round-trip precision loss at large
    // coordinates (e.g. Turkish UTM ~4,000,000 m). tessellate() will apply
    // world_offset when rendering this synthetic Text entity.
    let pos_f64 = dimension_text_pos_f64(dim);
    let base = dim.base();
    let rotation = if base.text_rotation.abs() > 1e-9 {
        base.text_rotation
    } else {
        dimension_text_natural_rotation(dim)
    };
    let mut text = Text::with_value(value, pos_f64)
        .with_height(dimension_text_height(dim))
        .with_rotation(rotation);
    text.style = base.style_name.clone();
    text.common = base.common.clone();
    Some(text)
}

fn dimension_text_natural_rotation(dim: &Dimension) -> f64 {
    let angle = match dim {
        Dimension::Linear(d) => d.rotation,
        Dimension::Aligned(d) => {
            let dx = d.second_point.x - d.first_point.x;
            let dy = d.second_point.y - d.first_point.y;
            dy.atan2(dx)
        }
        _ => 0.0,
    };
    // Clamp to (-π/2, π/2] so text never appears upside-down.
    let pi = std::f64::consts::PI;
    if angle > pi / 2.0 {
        angle - pi
    } else if angle <= -pi / 2.0 {
        angle + pi
    } else {
        angle
    }
}

fn dimension_text_value(dim: &Dimension) -> Option<String> {
    let base = dim.base();
    if let Some(user_text) = &base.user_text {
        if !user_text.trim().is_empty() {
            return Some(user_text.clone());
        }
    }
    if !base.text.trim().is_empty() {
        return Some(base.text.clone());
    }
    Some(match dim {
        Dimension::Radius(_) => format!("R{:.4}", dim.measurement()),
        Dimension::Diameter(_) => format!("Ø{:.4}", dim.measurement()),
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => {
            format!("{:.2}°", dim.measurement())
        }
        _ => format!("{:.4}", dim.measurement()),
    })
}

fn dimension_text_position(dim: &Dimension, world_offset: [f64; 3]) -> Vec3 {
    let lv = |v| vec3_local(v, world_offset);
    let base = dim.base();
    let pos = lv(base.text_middle_point);
    if pos.length_squared() > 1e-8 {
        return pos;
    }
    match dim {
        Dimension::Aligned(d) => (lv(d.first_point) + lv(d.second_point)) * 0.5,
        Dimension::Linear(d) => (lv(d.first_point) + lv(d.second_point)) * 0.5,
        Dimension::Radius(d) => (lv(d.angle_vertex) + lv(d.definition_point)) * 0.5,
        Dimension::Diameter(d) => (lv(d.angle_vertex) + lv(d.definition_point)) * 0.5,
        Dimension::Angular2Ln(d) => lv(d.dimension_arc),
        Dimension::Angular3Pt(d) => lv(d.definition_point),
        Dimension::Ordinate(d) => lv(d.leader_endpoint),
    }
}

fn dimension_text_height(dim: &Dimension) -> f64 {
    let scale = (dim.measurement().abs() * 0.12).clamp(0.25, 2.0);
    if scale.is_finite() {
        scale
    } else {
        0.25
    }
}

fn vec3_local(v: Vector3, off: [f64; 3]) -> Vec3 {
    Vec3::new(
        (v.x - off[0]) as f32,
        (v.y - off[1]) as f32,
        (v.z - off[2]) as f32,
    )
}

fn offset_snap_pts(pts: Vec<(Vec3, SnapHint)>, off: [f64; 3]) -> Vec<(Vec3, SnapHint)> {
    let [ox, oy, oz] = off;
    pts.into_iter()
        .map(|(p, h)| {
            (
                Vec3::new(p.x - ox as f32, p.y - oy as f32, p.z - oz as f32),
                h,
            )
        })
        .collect()
}

/// Returns the text position of a dimension in DXF world-space (f64, no offset applied).
/// Used when building a synthetic Text entity so tessellate() can apply world_offset itself.
fn dimension_text_pos_f64(dim: &Dimension) -> Vector3 {
    let base = dim.base();
    let p = base.text_middle_point;
    if p.x * p.x + p.y * p.y + p.z * p.z > 1e-16 {
        return p;
    }
    match dim {
        Dimension::Aligned(d) => Vector3::new(
            (d.first_point.x + d.second_point.x) * 0.5,
            (d.first_point.y + d.second_point.y) * 0.5,
            (d.first_point.z + d.second_point.z) * 0.5,
        ),
        Dimension::Linear(d) => Vector3::new(
            (d.first_point.x + d.second_point.x) * 0.5,
            (d.first_point.y + d.second_point.y) * 0.5,
            (d.first_point.z + d.second_point.z) * 0.5,
        ),
        Dimension::Radius(d) => Vector3::new(
            (d.angle_vertex.x + d.definition_point.x) * 0.5,
            (d.angle_vertex.y + d.definition_point.y) * 0.5,
            (d.angle_vertex.z + d.definition_point.z) * 0.5,
        ),
        Dimension::Diameter(d) => Vector3::new(
            (d.angle_vertex.x + d.definition_point.x) * 0.5,
            (d.angle_vertex.y + d.definition_point.y) * 0.5,
            (d.angle_vertex.z + d.definition_point.z) * 0.5,
        ),
        Dimension::Angular2Ln(d) => d.dimension_arc,
        Dimension::Angular3Pt(d) => d.definition_point,
        Dimension::Ordinate(d) => d.leader_endpoint,
    }
}

fn normalized_or(v: Vec3, fallback: Vec3) -> Vec3 {
    if v.length_squared() <= 1e-12 {
        fallback
    } else {
        v.normalize()
    }
}

fn rotate(v: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    Vec3::new(v.x * c - v.y * s, v.x * s + v.y * c, v.z)
}
