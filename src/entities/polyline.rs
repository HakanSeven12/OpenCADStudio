use acadrust::entities::{Polyline, Polyline2D, Polyline3D};
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    edit_prop as edit, parse_f64, ro_prop as ro, square_grip, stepper_prop as stepper,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{extrusion_wall_tris, RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::TangentGeom;

// ── Polyline (old-style 3D heavy polyline) ────────────────────────────────────

fn tessellate_polyline(pl: &Polyline) -> RenderEntity {
    let pts: Vec<[f64; 3]> = pl
        .vertices
        .iter()
        .map(|v| [v.location.x, v.location.y, v.location.z])
        .collect();

    let mut points = pts.clone();
    if pl.flags.is_closed() && pts.len() >= 2 {
        points.push(pts[0]);
    }

    let key_verts = pts.clone();
    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(points),
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

impl RenderConvertible for Polyline {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(tessellate_polyline(self))
    }
}

impl Grippable for Polyline {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    glam::DVec3::new(v.location.x, v.location.y, v.location.z),
                )
            })
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.location.x += d.x as f64;
                    v.location.y += d.y as f64;
                    v.location.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    v.location.x = p.x as f64;
                    v.location.y = p.y as f64;
                    v.location.z = p.z as f64;
                }
            }
        }
    }

    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Vertex",
                action: GripMenuAction::AddVertex,
            },
            GripMenuItem {
                label: "Remove Vertex",
                action: GripMenuAction::RemoveVertex,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.vertices.len();
        match action {
            A::AddVertex if grip_id < n => {
                if grip_id == n - 1 && !self.is_closed() {
                    self.vertices.push(self.vertices[grip_id].clone());
                    return;
                }
                let i1 = (grip_id + 1) % n;
                let v0 = self.vertices[grip_id].clone();
                let v1 = self.vertices[i1].clone();
                let mx = (v0.location.x + v1.location.x) * 0.5;
                let my = (v0.location.y + v1.location.y) * 0.5;
                let mz = (v0.location.z + v1.location.z) * 0.5;
                let mut new_v = v0.clone();
                new_v.location.x = mx;
                new_v.location.y = my;
                new_v.location.z = mz;
                let insert_at = (grip_id + 1).min(self.vertices.len());
                self.vertices.insert(insert_at, new_v);
            }
            A::RemoveVertex if grip_id < n && n > 2 => {
                self.vertices.remove(grip_id);
            }
            _ => {}
        }
    }
}

impl PropertyEditable for Polyline {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                ro(t!("Vertices").as_ref(), "vertices", self.vertices.len().to_string()),
                Property {
                    label: t!("Closed").into_owned(),
                    field: "pl_closed",
                    value: PropValue::BoolToggle {
                        field: "pl_closed",
                        value: self.flags.is_closed(),
                    },
                },
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        if field == "pl_closed" {
            let closed = if value == "toggle" {
                !self.flags.is_closed()
            } else {
                value == "true"
            };
            self.flags.set_closed(closed);
        }
    }
}

impl Transformable for Polyline {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.location.x,
                    &mut v.location.y,
                    p1,
                    p2,
                );
            }
        });
    }
}

// ── Polyline2D (heavy 2D polyline with bulge) ─────────────────────────────────

/// The vertices a curve-/spline-fit 2D polyline actually DRAWS: the
/// fit-generated points (VertexFlags 8 / 1) plus any unflagged originals,
/// with the spline-frame CONTROL points (flag 16) dropped — those are editing
/// scaffolding shown only under SPLFRAME, and chaining through them draws
/// spokes across the fitted curve (#408). `None` = no fit data, draw the
/// stored vertices as-is. A fit-flagged polyline whose fit points are missing
/// falls back to the stored vertices rather than drawing nothing.
pub fn drawn_vertices2d(
    pl: &Polyline2D,
) -> Option<Vec<acadrust::entities::Vertex2D>> {
    use acadrust::entities::polyline::VertexFlags;
    let has_fit = pl.vertices.iter().any(|v| {
        v.flags.bits()
            & (VertexFlags::SPLINE_VERTEX.bits() | VertexFlags::EXTRA_VERTEX.bits())
            != 0
    });
    if !has_fit {
        return None;
    }
    let kept: Vec<_> = pl
        .vertices
        .iter()
        .filter(|v| v.flags.bits() & VertexFlags::SPLINE_CONTROL.bits() == 0)
        .cloned()
        .collect();
    (kept.len() >= 2).then_some(kept)
}

fn tessellate_polyline2d(pl: &Polyline2D) -> RenderEntity {
    let filtered = drawn_vertices2d(pl);
    let verts: &[acadrust::entities::Vertex2D] = filtered.as_deref().unwrap_or(&pl.vertices);
    if verts.is_empty() {
        return RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(vec![]),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
        };
    }

    let elev = pl.elevation;
    let normal = (pl.normal.x, pl.normal.y, pl.normal.z);
    let count = verts.len();
    let seg_count = if pl.is_closed() { count } else { count - 1 };
    let mut tangents: Vec<TangentGeom> = Vec::new();
    let mut key_verts: Vec<[f64; 3]> = Vec::new();

    let to_wcs = |x: f64, y: f64| -> (f64, f64, f64) {
        crate::scene::view::transform::ocs_point_to_wcs((x, y, elev), normal)
    };
    let to_pt = |v: &acadrust::entities::Vertex2D| -> [f64; 3] {
        let (wx, wy, wz) = to_wcs(v.location.x, v.location.y);
        [wx, wy, wz]
    };

    if pl.thickness.abs() > 1e-10 {
        let (nx, ny, nz) = normal;
        let t = pl.thickness;
        let off = |p: [f64; 3]| -> [f64; 3] { [p[0] + t * nx, p[1] + t * ny, p[2] + t * nz] };
        let to_f32 = |p: [f64; 3]| -> [f32; 3] { [p[0] as f32, p[1] as f32, p[2] as f32] };
        let mut path: Vec<[f64; 3]> = Vec::new();
        let mut kv: Vec<[f64; 3]> = Vec::new();
        let mut tgs: Vec<TangentGeom> = Vec::new();
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
                    p1: to_f32(p1_pt),
                    p2: to_f32(p2_pt),
                });
            } else if let Some(arc) =
                crate::entities::common::BulgeArc::from_bulge([ox0, oy0], [ox1, oy1], bulge)
            {
                let (wcx, wcy, wcz) = to_wcs(arc.center[0], arc.center[1]);
                tgs.push(TangentGeom::Circle {
                    center: [wcx as f32, wcy as f32, wcz as f32],
                    radius: arc.radius as f32,
                });
                for j in 1..=16usize {
                    let s = arc.sample(j as f64 / 16.0);
                    let (wx, wy, wz) = to_wcs(s[0], s[1]);
                    path.push([wx, wy, wz]);
                }
            }
            let (wbx, wby, wbz) = to_wcs(ox1, oy1);
            kv.push([wbx, wby, wbz]);
        }
        // A wide Polyline2D extrudes its whole band into a solid tube (walls +
        // caps), same as a thickened LwPolyline; a zero-width one falls through
        // to the centre-line extrusion below.
        let is_wide = pl.start_width > 1e-9
            || pl.end_width > 1e-9
            || pl
                .vertices
                .iter()
                .any(|v| v.start_width > 1e-9 || v.end_width > 1e-9);
        if is_wide {
            let (origin, fills) = wide_fills(pl);
            let (fill_tris, lines) =
                crate::entities::common::thick_band_tube(origin, &fills, t, normal, &to_wcs);
            return RenderEntity {
                pick_tris: fill_tris.clone(),
                object: RenderObject::Lines(lines),
                snap_pts: vec![],
                tangent_geoms: tgs,
                key_vertices: kv,
                fill_tris,
            };
        }
        let mut pts: Vec<[f64; 3]> = Vec::with_capacity(path.len() * 2 + kv.len() * 3 + 4);
        pts.extend_from_slice(&path);
        pts.push([f64::NAN; 3]);
        for &p in &path {
            pts.push(off(p));
        }
        if !kv.is_empty() {
            pts.push([f64::NAN; 3]);
            for (i, &pb) in kv.iter().enumerate() {
                pts.push(pb);
                pts.push(off(pb));
                if i + 1 < kv.len() {
                    pts.push([f64::NAN; 3]);
                }
            }
        }
        return RenderEntity {
            pick_tris: extrusion_wall_tris(&path, [t * nx, t * ny, t * nz]),
            object: RenderObject::Lines(pts),
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
            tangents.push(TangentGeom::Line {
                p1: [p0[0] as f32, p0[1] as f32, p0[2] as f32],
                p2: [p1[0] as f32, p1[1] as f32, p1[2] as f32],
            });
        } else if let Some(arc) = crate::entities::common::BulgeArc::from_bulge(
            [v0.location.x, v0.location.y],
            [v1.location.x, v1.location.y],
            bulge,
        ) {
            let (wcx, wcy, wcz) = to_wcs(arc.center[0], arc.center[1]);
            tangents.push(TangentGeom::Circle {
                center: [wcx as f32, wcy as f32, wcz as f32],
                radius: arc.radius as f32,
            });
        }

        if i == 0 {
            key_verts.push([p0[0], p0[1], p0[2]]);
        }
        key_verts.push([p1[0], p1[1], p1[2]]);
    }

    let (fill_origin, fills) = wide_fills(pl);
    // A wide Polyline2D whose per-vertex widths VARY renders a smooth taper; a
    // uniform-width one keeps the constant-band Contour.
    let object = match tapered_band_verts_2d(pl) {
        Some(band_verts) => {
            let (pts, widths) = crate::entities::common::tapered_band_points(
                &band_verts,
                pl.is_closed(),
                &to_wcs,
            );
            RenderObject::TaperedLines(pts, widths)
        }
        None => RenderObject::Lines(
            crate::entities::curve::polyline2d_curve(pl)
                .map(|planar| crate::entities::curve::curve_points(&planar))
                .unwrap_or_default(),
        ),
    };
    RenderEntity {
        pick_tris: crate::entities::common::wide_band_tris(fill_origin, &fills),
        object,
        snap_pts: vec![],
        tangent_geoms: tangents,
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

/// Per-vertex `(location, bulge, start_width, end_width)` band description for a
/// wide Polyline2D whose width VARIES — `None` when the width is uniform.
fn tapered_band_verts_2d(
    pl: &acadrust::entities::Polyline2D,
) -> Option<Vec<([f64; 2], f64, f64, f64)>> {
    let default_start = pl.start_width;
    let default_end = pl.end_width;
    let filtered = drawn_vertices2d(pl);
    let verts: &[acadrust::entities::Vertex2D] = filtered.as_deref().unwrap_or(&pl.vertices);
    let band: Vec<([f64; 2], f64, f64, f64)> = verts
        .iter()
        .map(|v| {
            let sw = if v.start_width > 1e-9 {
                v.start_width
            } else {
                default_start
            };
            let ew = if v.end_width > 1e-9 {
                v.end_width
            } else {
                default_end
            };
            ([v.location.x, v.location.y], v.bulge, sw, ew)
        })
        .collect();
    let w0 = band.first().map_or(0.0, |v| v.2);
    let varies = band
        .iter()
        .any(|&(_, _, sw, ew)| (sw - w0).abs() > 1e-9 || (ew - w0).abs() > 1e-9);
    if varies && w0.max(band.iter().map(|v| v.3).fold(0.0, f64::max)) > 1e-9 {
        Some(band)
    } else {
        None
    }
}

impl RenderConvertible for Polyline2D {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(tessellate_polyline2d(self))
    }
}

impl Grippable for Polyline2D {
    fn grips(&self) -> Vec<GripDef> {
        let elev = self.elevation;
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| square_grip(i, glam::DVec3::new(v.location.x, v.location.y, elev)))
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.location.x += d.x as f64;
                    v.location.y += d.y as f64;
                }
                GripApply::Absolute(p) => {
                    v.location.x = p.x as f64;
                    v.location.y = p.y as f64;
                }
            }
        }
    }

    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Vertex",
                action: GripMenuAction::AddVertex,
            },
            GripMenuItem {
                label: "Remove Vertex",
                action: GripMenuAction::RemoveVertex,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.vertices.len();
        let elev = self.elevation;
        match action {
            A::AddVertex if grip_id < n => {
                if grip_id == n - 1 && !self.is_closed() {
                    self.vertices[grip_id].bulge = 0.0;
                    let mut new_v = self.vertices[grip_id].clone();
                    new_v.bulge = 0.0;
                    new_v.id = 0;
                    self.vertices.push(new_v);
                    return;
                }
                let i1 = (grip_id + 1) % n;
                let v0 = self.vertices[grip_id].clone();
                let v1 = self.vertices[i1].clone();
                let midpoint = if v0.bulge.abs() >= 1e-9 {
                    crate::entities::common::BulgeArc::from_bulge(
                        [v0.location.x, v0.location.y],
                        [v1.location.x, v1.location.y],
                        v0.bulge,
                    )
                    .map(|arc| arc.sample(0.5))
                    .unwrap_or([
                        (v0.location.x + v1.location.x) * 0.5,
                        (v0.location.y + v1.location.y) * 0.5,
                    ])
                } else {
                    [
                        (v0.location.x + v1.location.x) * 0.5,
                        (v0.location.y + v1.location.y) * 0.5,
                    ]
                };
                let mut new_v = v0.clone();
                new_v.location.x = midpoint[0];
                new_v.location.y = midpoint[1];
                new_v.location.z = elev;
                new_v.id = 0;
                let effective_start = if v0.start_width > 1e-9 {
                    v0.start_width
                } else {
                    self.start_width
                };
                let effective_end = if v0.end_width > 1e-9 {
                    v0.end_width
                } else {
                    self.end_width
                };
                let middle_width = (effective_start + effective_end) * 0.5;
                self.vertices[grip_id].end_width = middle_width;
                new_v.start_width = middle_width;
                if v0.bulge.abs() >= 1e-9 {
                    new_v.bulge = (v0.bulge.atan() * 0.5).tan();
                }
                let insert_at = (grip_id + 1).min(self.vertices.len());
                if v0.bulge.abs() >= 1e-9 {
                    self.vertices[grip_id].bulge = new_v.bulge;
                }
                self.vertices.insert(insert_at, new_v);
            }
            A::RemoveVertex if grip_id < n && n > 2 => {
                self.vertices.remove(grip_id);
            }
            _ => {}
        }
    }
}

impl PropertyEditable for Polyline2D {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let n = self.vertices.len();
        let mut area = 0.0;
        let mut length = 0.0;
        let seg_count = if self.is_closed() { n } else { n.saturating_sub(1) };
        for i in 0..seg_count {
            let a = &self.vertices[i].location;
            let b = &self.vertices[(i + 1) % n].location;
            area += a.x * b.y - b.x * a.y;
            length += ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        }
        area = (area * 0.5).abs();

        let vi = if n == 0 {
            0
        } else {
            crate::scene::view::dispatch::prop_current_vertex().min(n - 1)
        };
        let v = self.vertices.get(vi);
        let vertex_x = v.map(|v| v.location.x).unwrap_or_default();
        let vertex_y = v.map(|v| v.location.y).unwrap_or_default();
        let seg_start_w = v.map(|v| v.start_width).unwrap_or_default();
        let seg_end_w = v.map(|v| v.end_width).unwrap_or_default();
        let vertex_label = if n == 0 {
            "—".to_string()
        } else {
            format!("{} / {}", vi + 1, n)
        };

        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    stepper(t!("Current Vertex").as_ref(), "pl2_current_vertex", vertex_label),
                    edit(t!("Vertex X").as_ref(), "pl2_vertex_x", vertex_x),
                    edit(t!("Vertex Y").as_ref(), "pl2_vertex_y", vertex_y),
                    edit(t!("Start segment width").as_ref(), "pl2_seg_start_w", seg_start_w),
                    edit(t!("End segment width").as_ref(), "pl2_seg_end_w", seg_end_w),
                    edit(t!("Global width").as_ref(), "pl2_start_w", self.start_width),
                    edit(t!("Elevation").as_ref(), "pl2_elevation", self.elevation),
                    ro(t!("Area").as_ref(), "pl2_area", format!("{area:.4}")),
                    ro(t!("Length").as_ref(), "pl2_length", format!("{length:.4}")),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    Property {
                        label: t!("Closed").into_owned(),
                        field: "pl2_closed",
                        value: PropValue::BoolToggle {
                            field: "pl2_closed",
                            value: self.is_closed(),
                        },
                    },
                    Property {
                        label: t!("Linetype generation").into_owned(),
                        field: "pl2_ltype_gen",
                        value: PropValue::BoolToggle {
                            field: "pl2_ltype_gen",
                            value: self.flags.bits() & 128 != 0,
                        },
                    },
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        // Per-vertex edits target the vertex the panel is focused on.
        let n = self.vertices.len();
        let vi = if n == 0 {
            0
        } else {
            crate::scene::view::dispatch::prop_current_vertex().min(n - 1)
        };
        match field {
            "pl2_closed" => {
                let closed = if value == "toggle" {
                    !self.is_closed()
                } else {
                    value == "true"
                };
                if closed {
                    self.close();
                } else {
                    self.flags.set_closed(false);
                }
            }
            "pl2_elevation" => {
                if let Some(v) = parse_f64(value) {
                    self.elevation = v;
                }
            }
            "pl2_start_w" => {
                if let Some(v) = parse_f64(value) {
                    if v >= 0.0 {
                        self.start_width = v;
                        self.end_width = v;
                    }
                }
            }
            "pl2_vertex_x" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.get_mut(vi)) {
                    vert.location.x = v;
                }
            }
            "pl2_vertex_y" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.get_mut(vi)) {
                    vert.location.y = v;
                }
            }
            "pl2_seg_start_w" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.get_mut(vi)) {
                    if v >= 0.0 {
                        vert.start_width = v;
                    }
                }
            }
            "pl2_seg_end_w" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.get_mut(vi)) {
                    if v >= 0.0 {
                        vert.end_width = v;
                    }
                }
            }
            "pl2_ltype_gen" => {
                let on = if value == "toggle" {
                    self.flags.bits() & 128 == 0
                } else {
                    value == "true"
                };
                let bits = if on {
                    self.flags.bits() | 128
                } else {
                    self.flags.bits() & !128
                };
                self.flags = acadrust::entities::polyline::PolylineFlags::from_bits(bits);
            }
            _ => {}
        }
    }
}

impl Transformable for Polyline2D {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.location.x,
                    &mut v.location.y,
                    p1,
                    p2,
                );
                // Bulge encodes which side the arc bows to; a reflection
                // reverses it or every curved segment flips to the wrong side.
                v.bulge = -v.bulge;
            }
        });
    }
}

// ── Polyline3D ────────────────────────────────────────────────────────────────

fn tessellate_polyline3d(pl: &Polyline3D) -> RenderEntity {
    let to_pt = |v: &acadrust::entities::Vertex3DPolyline| -> [f64; 3] {
        [v.position.x, v.position.y, v.position.z]
    };

    // DXF vertex flags:  8 = spline-fit curve point,  16 = spline frame control point.
    // When spline-fit vertices are present use them for the wire and control points for snap;
    // otherwise treat all vertices uniformly.
    let spline_curve: Vec<_> = pl.vertices.iter().filter(|v| v.flags & 8 != 0).collect();
    let ctrl_pts: Vec<_> = pl.vertices.iter().filter(|v| v.flags & 16 != 0).collect();

    let (wire_pts, key_verts) = if !spline_curve.is_empty() {
        let wire: Vec<[f64; 3]> = spline_curve.iter().map(|v| to_pt(v)).collect();
        let ctrl: Vec<[f64; 3]> = ctrl_pts.iter().map(|v| to_pt(v)).collect();
        (wire, ctrl)
    } else {
        let pts: Vec<[f64; 3]> = pl.vertices.iter().map(to_pt).collect();
        (pts.clone(), pts)
    };

    let mut points = wire_pts.clone();
    if pl.is_closed() && wire_pts.len() >= 2 {
        points.push(wire_pts[0]);
    }

    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(points),
        snap_pts: vec![],
        tangent_geoms: vec![],
        key_vertices: key_verts,
        fill_tris: vec![],
    }
}

impl RenderConvertible for Polyline3D {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(tessellate_polyline3d(self))
    }
}

impl Grippable for Polyline3D {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    glam::DVec3::new(v.position.x, v.position.y, v.position.z),
                )
            })
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.position.x += d.x as f64;
                    v.position.y += d.y as f64;
                    v.position.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    v.position.x = p.x as f64;
                    v.position.y = p.y as f64;
                    v.position.z = p.z as f64;
                }
            }
        }
    }

    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Vertex",
                action: GripMenuAction::AddVertex,
            },
            GripMenuItem {
                label: "Remove Vertex",
                action: GripMenuAction::RemoveVertex,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.vertices.len();
        match action {
            A::AddVertex if grip_id < n => {
                if grip_id == n - 1 && !self.is_closed() {
                    let mut new_v = self.vertices[grip_id].clone();
                    new_v.handle = acadrust::Handle::NULL;
                    self.vertices.push(new_v);
                    return;
                }
                let i1 = (grip_id + 1) % n;
                let v0 = &self.vertices[grip_id];
                let v1 = &self.vertices[i1];
                let mx = (v0.position.x + v1.position.x) * 0.5;
                let my = (v0.position.y + v1.position.y) * 0.5;
                let mz = (v0.position.z + v1.position.z) * 0.5;
                let mut new_v = v0.clone();
                new_v.position.x = mx;
                new_v.position.y = my;
                new_v.position.z = mz;
                new_v.handle = acadrust::Handle::NULL;
                let insert_at = (grip_id + 1).min(self.vertices.len());
                self.vertices.insert(insert_at, new_v);
            }
            A::RemoveVertex if grip_id < n && n > 2 => {
                self.vertices.remove(grip_id);
            }
            _ => {}
        }
    }
}

impl PropertyEditable for Polyline3D {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        use acadrust::entities::polyline3d::SmoothSurfaceType as SST;
        let n = self.vertices.len();
        let v0 = self.vertices.first();
        let vertex_x = v0.map(|v| v.position.x).unwrap_or_default();
        let vertex_y = v0.map(|v| v.position.y).unwrap_or_default();
        let vertex_z = v0.map(|v| v.position.z).unwrap_or_default();
        let fit_smooth = match self.smooth_type {
            SST::None => "None",
            SST::QuadraticBSpline => "Quadratic",
            SST::CubicBSpline => "Cubic",
            SST::Bezier => "Bezier",
        };

        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    ro(t!("Vertex").as_ref(), "pl3_vertex", if n > 0 { "1" } else { "" }),
                    edit(t!("Vertex X").as_ref(), "pl3_vertex_x", vertex_x),
                    edit(t!("Vertex Y").as_ref(), "pl3_vertex_y", vertex_y),
                    edit(t!("Vertex Z").as_ref(), "pl3_vertex_z", vertex_z),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    Property {
                        label: t!("Closed").into_owned(),
                        field: "pl3_closed",
                        value: PropValue::BoolToggle {
                            field: "pl3_closed",
                            value: self.is_closed(),
                        },
                    },
                    ro(t!("Fit/Smooth").as_ref(), "pl3_smooth", fit_smooth),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "pl3_closed" => {
                let closed = if value == "toggle" {
                    !self.is_closed()
                } else {
                    value == "true"
                };
                if closed {
                    self.close();
                } else {
                    self.open();
                }
            }
            "pl3_vertex_x" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.first_mut()) {
                    vert.position.x = v;
                }
            }
            "pl3_vertex_y" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.first_mut()) {
                    vert.position.y = v;
                }
            }
            "pl3_vertex_z" => {
                if let (Some(v), Some(vert)) = (parse_f64(value), self.vertices.first_mut()) {
                    vert.position.z = v;
                }
            }
            _ => {}
        }
    }
}

impl Transformable for Polyline3D {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.position.x,
                    &mut v.position.y,
                    p1,
                    p2,
                );
            }
        });
    }
}
/// Generate solid-fill boundary polygons for each wide segment of a Polyline2D.
/// Solid-fill bands for a wide Polyline2D, plus the `world_origin` they are
/// relative to (the first vertex). See `lwpolyline::wide_fills` — offsets are
/// f32 from `origin` so the band stays precise at UTM-scale coordinates.
pub(crate) fn wide_fills(pl: &acadrust::entities::Polyline2D) -> ([f64; 2], Vec<Vec<[f32; 2]>>) {
    // The stored widths are the band's FULL width and `polyline_segment_fill`
    // offsets ±hw about the centreline — halve them. See `lwpolyline::wide_fills`.
    let hw_default_start = pl.start_width as f32 * 0.5;
    let hw_default_end = pl.end_width as f32 * 0.5;
    let filtered = drawn_vertices2d(pl);
    let verts: &[acadrust::entities::Vertex2D] = filtered.as_deref().unwrap_or(&pl.vertices);
    let n = verts.len();
    if n < 2 {
        return ([0.0; 2], vec![]);
    }
    let origin = [verts[0].location.x, verts[0].location.y];
    let seg_count = if pl.is_closed() { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..seg_count {
        let v0 = &verts[i];
        let v1 = &verts[(i + 1) % n];
        let hw0 = if v0.start_width > 1e-9 {
            v0.start_width as f32 * 0.5
        } else {
            hw_default_start
        };
        let hw1 = if v0.end_width > 1e-9 {
            v0.end_width as f32 * 0.5
        } else {
            hw_default_end
        };
        if hw0 < 1e-6 && hw1 < 1e-6 {
            continue;
        }
        let p0 = [
            (v0.location.x - origin[0]) as f32,
            (v0.location.y - origin[1]) as f32,
        ];
        let p1 = [
            (v1.location.x - origin[0]) as f32,
            (v1.location.y - origin[1]) as f32,
        ];
        if let Some(poly) =
            crate::entities::common::polyline_segment_fill(p0, p1, hw0, hw1, v0.bulge as f32)
        {
            out.push(poly);
        }
    }
    (origin, out)
}
