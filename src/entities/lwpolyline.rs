use acadrust::entities::{LwPolyline, LwVertex};
use glam::Vec3;
use truck_modeling::{builder, Edge, Point3, Wire};

use crate::command::EntityTransform;
use crate::entities::common::{
    diamond_grip, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::TangentGeom;

const TAU: f64 = std::f64::consts::TAU;

/// Midpoint position on an arc segment defined by its bulge.
fn arc_midpoint(p0: [f64; 2], p1: [f64; 2], bulge: f64) -> [f64; 2] {
    let angle = 4.0 * bulge.atan();
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let d = (dx * dx + dy * dy).sqrt();
    if d < 1e-12 {
        return [(p0[0] + p1[0]) * 0.5, (p0[1] + p1[1]) * 0.5];
    }
    let r = (d / 2.0) / (angle / 2.0).sin().abs();
    let mx = (p0[0] + p1[0]) * 0.5;
    let my = (p0[1] + p1[1]) * 0.5;
    let px = -dy / d;
    let py = dx / d;
    let sign = if bulge > 0.0 { 1.0_f64 } else { -1.0_f64 };
    let h = r - (r * r - d * d / 4.0).max(0.0).sqrt();
    let cx = mx + sign * px * (r - h);
    let cy = my + sign * py * (r - h);
    let a0 = (p0[1] - cy).atan2(p0[0] - cx);
    let a1 = (p1[1] - cy).atan2(p1[0] - cx);
    let (sa, mut ea) = if bulge > 0.0 { (a0, a1) } else { (a1, a0) };
    if ea < sa {
        ea += TAU;
    }
    let mid_a = sa + (ea - sa) * 0.5;
    [cx + r * mid_a.cos(), cy + r * mid_a.sin()]
}

/// Compute the DXF bulge for an arc that passes through p0, mid_pt, and p1.
/// Returns None when the three points are collinear (straight segment).
fn bulge_from_midpoint(p0: [f64; 2], p1: [f64; 2], mid: [f64; 2]) -> Option<f64> {
    // Circumcircle of (p0, mid, p1)
    let ax = 2.0 * (mid[0] - p0[0]);
    let ay = 2.0 * (mid[1] - p0[1]);
    let bx = 2.0 * (p1[0] - p0[0]);
    let by = 2.0 * (p1[1] - p0[1]);
    let ca = mid[0] * mid[0] + mid[1] * mid[1] - p0[0] * p0[0] - p0[1] * p0[1];
    let cb = p1[0] * p1[0] + p1[1] * p1[1] - p0[0] * p0[0] - p0[1] * p0[1];
    let det = ax * by - ay * bx;
    if det.abs() < 1e-12 {
        return None; // collinear
    }
    let cx = (ca * by - cb * ay) / det;
    let cy = (ax * cb - bx * ca) / det;
    let a0 = (p0[1] - cy).atan2(p0[0] - cx);
    let a1 = (p1[1] - cy).atan2(p1[0] - cx);
    // Determine arc direction: cross product (p1-p0) × (mid-p0)
    let cross = (p1[0] - p0[0]) * (mid[1] - p0[1]) - (p1[1] - p0[1]) * (mid[0] - p0[0]);
    let (sa, mut ea) = if cross > 0.0 { (a0, a1) } else { (a1, a0) };
    if ea < sa {
        ea += TAU;
    }
    let span = ea - sa; // central angle in (0, TAU]
    let bulge = (span / 4.0).tan();
    Some(if cross >= 0.0 { bulge } else { -bulge })
}

/// Tessellate a thick polyline segment list into NaN-separated Lines geometry.
/// Shared by LwPolyline and Polyline2D thickness paths.
fn thick_segments(
    seg_data: &[(f64, f64, f64, f64)], // (x0, y0, x1, y1) per seg — or use run of (x,y,bulge)
    path_pts: &[[f64; 3]],
    thickness: f64,
    normal: (f64, f64, f64),
    key_verts: Vec<[f64; 3]>,
    tangents: Vec<TangentGeom>,
) -> TruckEntity {
    let (nx, ny, nz) = normal;
    let t = thickness;
    let off = |p: [f64; 3]| -> [f64; 3] {
        [p[0] + t * nx, p[1] + t * ny, p[2] + t * nz]
    };
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(path_pts.len() * 2 + seg_data.len() * 3 + 4);
    // Bottom path
    pts.extend_from_slice(path_pts);
    pts.push([f64::NAN; 3]);
    // Top path
    for &p in path_pts {
        pts.push(off(p));
    }
    // Walls at each vertex (seg_data.0/.1 = start x/y of each seg, last seg appends its end too)
    if !seg_data.is_empty() {
        pts.push([f64::NAN; 3]);
        for (k, &(x0, y0, _x1, _y1)) in seg_data.iter().enumerate() {
            let pb = key_verts[k];
            let _ = (x0, y0); // key_verts already has correct WCS
            pts.push(pb);
            pts.push(off(pb));
            if k + 1 < seg_data.len() {
                pts.push([f64::NAN; 3]);
            }
        }
        // Last wall at the final vertex
        if let Some(&last) = key_verts.last() {
            pts.push([f64::NAN; 3]);
            pts.push(last);
            pts.push(off(last));
        }
    }
    TruckEntity {
        object: TruckObject::Lines(pts),
        snap_pts: vec![],
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

fn to_truck(pline: &LwPolyline) -> TruckEntity {
    let verts = &pline.vertices;
    if verts.is_empty() {
        return TruckEntity {
            object: TruckObject::Point(builder::vertex(Point3::new(0.0, 0.0, 0.0))),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let elev = pline.elevation;
    let normal = (pline.normal.x, pline.normal.y, pline.normal.z);
    let count = verts.len();
    let seg_count = if pline.is_closed { count } else { count - 1 };
    let mut edges: Vec<Edge> = Vec::new();
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut key_verts: Vec<[f64; 3]> = Vec::new();

    // Convert OCS (x, y, elevation) to WCS Point3.
    let to_wcs = |x: f64, y: f64| -> (f64, f64, f64) {
        crate::scene::transform::ocs_point_to_wcs((x, y, elev), normal)
    };
    let to_pt = |v: &LwVertex| -> Point3 {
        let (wx, wy, wz) = to_wcs(v.location.x, v.location.y);
        Point3::new(wx, wy, wz)
    };

    if pline.thickness.abs() > 1e-10 {
        let mut path: Vec<[f64; 3]> = Vec::new();
        let mut kv: Vec<[f64; 3]> = Vec::new();
        let mut tgs: Vec<TangentGeom> = Vec::new();
        let mut seg_data: Vec<(f64, f64, f64, f64)> = Vec::new();
        // First vertex
        let (w0x, w0y, w0z) = to_wcs(verts[0].location.x, verts[0].location.y);
        path.push([w0x, w0y, w0z]);
        kv.push([w0x, w0y, w0z]);
        for i in 0..seg_count {
            let va = &verts[i];
            let vb = &verts[(i + 1) % count];
            let (ox0, oy0) = (va.location.x, va.location.y);
            let (ox1, oy1) = (vb.location.x, vb.location.y);
            let bulge = va.bulge;
            if bulge.abs() < 1e-9 {
                let (wx, wy, wz) = to_wcs(ox1, oy1);
                path.push([wx, wy, wz]);
                let p1_pt = path[path.len() - 2];
                let p2_pt = *path.last().unwrap();
                tgs.push(TangentGeom::Line {
                    p1: [p1_pt[0] as f32, p1_pt[1] as f32, p1_pt[2] as f32],
                    p2: [p2_pt[0] as f32, p2_pt[1] as f32, p2_pt[2] as f32],
                });
            } else {
                let angle = 4.0 * bulge.atan();
                let dx = ox1 - ox0;
                let dy = oy1 - oy0;
                let d = (dx * dx + dy * dy).sqrt().max(1e-12);
                let r = (d / 2.0) / (angle / 2.0).sin().abs();
                let mx = (ox0 + ox1) * 0.5;
                let my = (oy0 + oy1) * 0.5;
                let px = -dy / d;
                let py = dx / d;
                let ss = if bulge > 0.0 { 1.0_f64 } else { -1.0_f64 };
                let h = r - (r * r - d * d / 4.0).max(0.0).sqrt();
                let ocx = mx + ss * px * (r - h);
                let ocy = my + ss * py * (r - h);
                let a0 = (oy0 - ocy).atan2(ox0 - ocx);
                let mut a1 = (oy1 - ocy).atan2(ox1 - ocx);
                if bulge > 0.0 {
                    if a1 < a0 {
                        a1 += TAU;
                    }
                } else {
                    if a1 > a0 {
                        a1 -= TAU;
                    }
                }
                let (wcx, wcy, wcz) = to_wcs(ocx, ocy);
                tgs.push(TangentGeom::Circle {
                    center: [wcx as f32, wcy as f32, wcz as f32],
                    radius: r as f32,
                });
                for j in 1..=16usize {
                    let a = a0 + (a1 - a0) * (j as f64 / 16.0);
                    let (wx, wy, wz) = to_wcs(ocx + r * a.cos(), ocy + r * a.sin());
                    path.push([wx, wy, wz]);
                }
            }
            let (wbx, wby, wbz) = to_wcs(ox1, oy1);
            kv.push([wbx, wby, wbz]);
            seg_data.push((ox0, oy0, ox1, oy1));
        }
        return thick_segments(&seg_data, &path, pline.thickness, normal, kv, tgs);
    }

    // plinegen=false: NaN-separated segments so the linetype pattern restarts per vertex.
    if !pline.plinegen {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut tgs: Vec<TangentGeom> = Vec::new();
        let mut kv: Vec<[f64; 3]> = Vec::new();
        let to_f32 = |p: [f64; 3]| -> [f32; 3] { [p[0] as f32, p[1] as f32, p[2] as f32] };
        for i in 0..seg_count {
            let va = &verts[i];
            let vb = &verts[(i + 1) % count];
            let (ox0, oy0) = (va.location.x, va.location.y);
            let (ox1, oy1) = (vb.location.x, vb.location.y);
            let bulge = va.bulge;
            let (wx0, wy0, wz0) = to_wcs(ox0, oy0);
            let p_start = [wx0, wy0, wz0];
            pts.push(p_start);
            if i == 0 {
                kv.push(p_start);
            }
            if bulge.abs() < 1e-9 {
                let (wx1, wy1, wz1) = to_wcs(ox1, oy1);
                let p_end = [wx1, wy1, wz1];
                pts.push(p_end);
                kv.push(p_end);
                tgs.push(TangentGeom::Line {
                    p1: to_f32(p_start),
                    p2: to_f32(p_end),
                });
            } else {
                let angle = 4.0 * bulge.atan();
                let dx = ox1 - ox0;
                let dy = oy1 - oy0;
                let d = (dx * dx + dy * dy).sqrt().max(1e-12);
                let r = (d / 2.0) / (angle / 2.0).sin().abs();
                let mx = (ox0 + ox1) * 0.5;
                let my = (oy0 + oy1) * 0.5;
                let px = -dy / d;
                let py = dx / d;
                let ss = if bulge > 0.0 { 1.0_f64 } else { -1.0_f64 };
                let h = r - (r * r - d * d / 4.0).max(0.0).sqrt();
                let ocx = mx + ss * px * (r - h);
                let ocy = my + ss * py * (r - h);
                let a0 = (oy0 - ocy).atan2(ox0 - ocx);
                let mut a1 = (oy1 - ocy).atan2(ox1 - ocx);
                if bulge > 0.0 {
                    if a1 < a0 {
                        a1 += TAU;
                    }
                } else {
                    if a1 > a0 {
                        a1 -= TAU;
                    }
                }
                for j in 1..=16usize {
                    let a = a0 + (a1 - a0) * (j as f64 / 16.0);
                    let (wx, wy, wz) = to_wcs(ocx + r * a.cos(), ocy + r * a.sin());
                    pts.push([wx, wy, wz]);
                }
                let (wx1, wy1, wz1) = to_wcs(ox1, oy1);
                kv.push([wx1, wy1, wz1]);
                let (wcx, wcy, wcz) = to_wcs(ocx, ocy);
                tgs.push(TangentGeom::Circle {
                    center: [wcx as f32, wcy as f32, wcz as f32],
                    radius: r as f32,
                });
            }
            if i + 1 < seg_count {
                pts.push([f64::NAN; 3]);
            }
        }
        return TruckEntity {
            object: TruckObject::SegmentedLines(pts),
            snap_pts: vec![],
            tangent_geoms: tgs,
            key_vertices: kv,
            fill_tris: vec![],
        };
    }

    for i in 0..seg_count {
        let v0 = &verts[i];
        let v1 = &verts[(i + 1) % count];
        let p0 = to_pt(v0);
        let p1 = to_pt(v1);
        let bulge = v0.bulge;

        if bulge.abs() < 1e-9 {
            let tv0 = builder::vertex(p0);
            let tv1 = builder::vertex(p1);
            edges.push(builder::line(&tv0, &tv1));
            tangents.push(TangentGeom::Line {
                p1: [p0.x as f32, p0.y as f32, p0.z as f32],
                p2: [p1.x as f32, p1.y as f32, p1.z as f32],
            });
        } else {
            // Arc centre/midpoint computed in OCS, then transformed to WCS.
            let (ox0, oy0) = (v0.location.x, v0.location.y);
            let (ox1, oy1) = (v1.location.x, v1.location.y);
            let angle = 4.0 * (bulge as f64).atan();
            let dx = ox1 - ox0;
            let dy = oy1 - oy0;
            let d = (dx * dx + dy * dy).sqrt();
            let r = (d / 2.0) / (angle / 2.0).sin().abs();
            let mx = (ox0 + ox1) * 0.5;
            let my = (oy0 + oy1) * 0.5;
            let len = d.max(1e-12);
            let px = -dy / len;
            let py = dx / len;
            let sagitta_sign = if bulge > 0.0 { 1.0_f64 } else { -1.0_f64 };
            let h = r - (r * r - d * d / 4.0).max(0.0).sqrt();
            let ocx = mx + sagitta_sign * px * (r - h);
            let ocy = my + sagitta_sign * py * (r - h);
            let mid_a = {
                let a0 = (oy0 - ocy).atan2(ox0 - ocx);
                let a1 = (oy1 - ocy).atan2(ox1 - ocx);
                let (sa, mut ea) = if bulge > 0.0 { (a0, a1) } else { (a1, a0) };
                if ea < sa {
                    ea += TAU;
                }
                sa + (ea - sa) * 0.5
            };
            let (mid_wx, mid_wy, mid_wz) = to_wcs(ocx + r * mid_a.cos(), ocy + r * mid_a.sin());
            let p_mid = Point3::new(mid_wx, mid_wy, mid_wz);
            let tv0 = builder::vertex(p0);
            let tv1 = builder::vertex(p1);
            edges.push(builder::circle_arc(&tv0, &tv1, p_mid));
            let (wcx, wcy, wcz) = to_wcs(ocx, ocy);
            tangents.push(TangentGeom::Circle {
                center: [wcx as f32, wcy as f32, wcz as f32],
                radius: r as f32,
            });
        }

        if i == 0 {
            key_verts.push([p0.x, p0.y, p0.z]);
        }
        key_verts.push([p1.x, p1.y, p1.z]);
    }

    TruckEntity {
        object: TruckObject::Contour(edges.into_iter().collect::<Wire>()),
        snap_pts: vec![],
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

fn grips(pline: &LwPolyline) -> Vec<GripDef> {
    let elev = pline.elevation as f32;
    let n = pline.vertices.len();
    let seg_count = if pline.is_closed {
        n
    } else {
        n.saturating_sub(1)
    };

    let mut out: Vec<GripDef> = pline
        .vertices
        .iter()
        .enumerate()
        .map(|(i, v)| square_grip(i, Vec3::new(v.location.x as f32, v.location.y as f32, elev)))
        .collect();

    // Diamond midpoint grip for each arc segment
    for i in 0..seg_count {
        let v0 = &pline.vertices[i];
        if v0.bulge.abs() < 1e-9 {
            continue;
        }
        let v1 = &pline.vertices[(i + 1) % n];
        let mid = arc_midpoint(
            [v0.location.x, v0.location.y],
            [v1.location.x, v1.location.y],
            v0.bulge,
        );
        out.push(diamond_grip(
            n + i,
            Vec3::new(mid[0] as f32, mid[1] as f32, elev),
        ));
    }
    out
}

fn properties(pline: &LwPolyline) -> PropSection {
    PropSection {
        title: "Geometry".into(),
        props: vec![
            ro("Vertices", "vertices", pline.vertices.len().to_string()),
            ro(
                "Closed",
                "closed",
                if pline.is_closed { "Yes" } else { "No" },
            ),
            edit("Elevation", "elevation", pline.elevation),
        ],
    }
}

fn apply_geom_prop(pline: &mut LwPolyline, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    if field == "elevation" {
        pline.elevation = v;
    }
}

fn apply_grip(pline: &mut LwPolyline, grip_id: usize, apply: GripApply) {
    let n = pline.vertices.len();
    if grip_id < n {
        // Vertex position grip
        let v = &mut pline.vertices[grip_id];
        match apply {
            GripApply::Absolute(p) => {
                v.location.x = p.x as f64;
                v.location.y = p.y as f64;
            }
            GripApply::Translate(d) => {
                v.location.x += d.x as f64;
                v.location.y += d.y as f64;
            }
        }
    } else {
        // Arc midpoint grip for segment (grip_id - n)
        let seg = grip_id - n;
        let count = if pline.is_closed {
            n
        } else {
            n.saturating_sub(1)
        };
        if seg >= count {
            return;
        }
        let new_mid: [f64; 2] = match apply {
            GripApply::Absolute(p) => [p.x as f64, p.y as f64],
            GripApply::Translate(d) => {
                let v0 = &pline.vertices[seg];
                let v1 = &pline.vertices[(seg + 1) % n];
                let old = arc_midpoint(
                    [v0.location.x, v0.location.y],
                    [v1.location.x, v1.location.y],
                    v0.bulge,
                );
                [old[0] + d.x as f64, old[1] + d.y as f64]
            }
        };
        let p0 = [
            pline.vertices[seg].location.x,
            pline.vertices[seg].location.y,
        ];
        let p1 = [
            pline.vertices[(seg + 1) % n].location.x,
            pline.vertices[(seg + 1) % n].location.y,
        ];
        if let Some(new_bulge) = bulge_from_midpoint(p0, p1, new_mid) {
            pline.vertices[seg].bulge = new_bulge.clamp(-1e6, 1e6);
        }
    }
}

fn apply_transform(pline: &mut LwPolyline, t: &EntityTransform) {
    crate::scene::transform::apply_standard_entity_transform(pline, t, |entity, p1, p2| {
        for v in &mut entity.vertices {
            crate::scene::transform::reflect_xy_point(&mut v.location.x, &mut v.location.y, p1, p2);
        }
    });
}

impl TruckConvertible for LwPolyline {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        Some(to_truck(self))
    }
}

impl Grippable for LwPolyline {
    fn grips(&self) -> Vec<GripDef> {
        grips(self)
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        apply_grip(self, grip_id, apply);
    }
}

impl PropertyEditable for LwPolyline {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        properties(self)
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        apply_geom_prop(self, field, value);
    }
}

impl Transformable for LwPolyline {
    fn apply_transform(&mut self, t: &EntityTransform) {
        apply_transform(self, t);
    }
}
