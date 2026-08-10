// SHAPE entity — reference to an .SHX shape-file glyph.
//
// Since .SHX binary files are not parsed, we render a small diamond marker
// at the insertion point (same approach as unknown/unsupported entities)
// but apply the SHAPE's own rotation / oblique / width factor so the marker
// gives a rough indication of the glyph orientation.

use acadrust::entities::Shape;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{edit_angle_prop as edit_angle, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, Property, PropValue};
use crate::scene::view::transform;
use crate::scene::model::wire_model::SnapHint;

// ── Marker geometry ───────────────────────────────────────────────────────────

/// Small diamond marker at the shape insertion point. The diamond is sized
/// by `size`, stretched horizontally by `relative_x_scale`, sheared by
/// `oblique_angle`, and rotated by `rotation`.
fn shape_marker(
    ox: f64,
    oy: f64,
    oz: f64,
    size: f64,
    rotation: f64,
    rel_x_scale: f64,
    oblique_angle: f64,
) -> Vec<[f64; 3]> {
    let s = size.abs().max(0.001) * 0.5;
    let rx = s * if rel_x_scale.abs() < 1e-9 {
        1.0
    } else {
        rel_x_scale
    };
    let ry = s;
    let local = [(0.0, ry), (rx, 0.0), (0.0, -ry), (-rx, 0.0)];
    let (sin_r, cos_r) = (rotation.sin(), rotation.cos());
    let ob = oblique_angle.tan();
    let mut out: Vec<[f64; 3]> = Vec::with_capacity(6);
    for &(x, y) in &local {
        let sx = x + y * ob;
        let lx = sx * cos_r - y * sin_r + ox;
        let ly = sx * sin_r + y * cos_r + oy;
        out.push([lx, ly, oz]);
    }
    // Close polygon + segment separator.
    let first = out[0];
    out.push(first);
    out.push([f64::NAN; 3]);
    out
}

// ── RenderConvertible ──────────────────────────────────────────────────────────

/// The shape's SHX polylines, resolved through its STYLE's shape file: the
/// style by handle (name fallback), the file with the shared path fallbacks
/// (as stored → relative to the drawing → basename next to it — foreign
/// absolute paths are the norm in traded files).
fn shx_polylines(
    shape: &Shape,
    document: &acadrust::CadDocument,
) -> Option<crate::scene::text::shx::ShapePolylines> {
    let base = document
        .source_path
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|p| p.parent());
    let name = shape.shape_name.trim();

    // Resolve the glyph from one style's shape (.shx) file, preferring the
    // shape NUMBER but falling back to the NAME — a DXF SHAPE carries only the
    // name (code 2), so its `shape_number` is 0.
    let try_style = |style: &acadrust::tables::TextStyle| {
        let font = style.font_file.trim();
        if font.is_empty() {
            return None;
        }
        let resolved = crate::io::resolve_image_file(font, base)?;
        if let Ok(num) = u16::try_from(shape.shape_number) {
            if num != 0 {
                if let Some(p) = crate::scene::text::shx::shape_polylines(&resolved, num) {
                    return Some(p);
                }
            }
        }
        if !name.is_empty() {
            return crate::scene::text::shx::shape_polylines_by_name(&resolved, name);
        }
        None
    };

    // 1. Direct style referenced by the SHAPE (handle, then name).
    let direct = shape
        .style_handle
        .and_then(|h| document.text_styles.iter().find(|s| s.handle == h))
        .or_else(|| {
            let sn = shape.style_name.trim();
            (!sn.is_empty())
                .then(|| {
                    document
                        .text_styles
                        .iter()
                        .find(|s| s.name.eq_ignore_ascii_case(sn))
                })
                .flatten()
        });
    if let Some(style) = direct {
        if let Some(p) = try_style(style) {
            return Some(p);
        }
    }

    // 2. No usable style reference (the DXF case): search every shape-file
    //    style — one with a real font file and an empty table name — for a
    //    glyph named like this shape.
    if !name.is_empty() {
        for style in document.text_styles.iter() {
            if style.name.is_empty() && !style.font_file.trim().is_empty() {
                if let Some(p) = try_style(style) {
                    return Some(p);
                }
            }
        }
    }
    None
}

impl RenderConvertible for Shape {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        // SHAPE insertion point is OCS (planar entity) — map through the
        // arbitrary axis so a mirrored shape's marker lands where it renders.
        let (ox, oy, oz) = crate::scene::view::transform::ocs_point_to_wcs(
            (
                self.insertion_point.x,
                self.insertion_point.y,
                self.insertion_point.z,
            ),
            (self.normal.x, self.normal.y, self.normal.z),
        );
        let size = self.size.abs().max(0.5);

        let snap_pt = glam::DVec3::new(ox, oy, oz);

        // Real SHX glyph when the style's shape file resolves; the diamond
        // marker stays as the missing-file placeholder.
        if let Some(polys) = shx_polylines(self, _document) {
            let (sin_r, cos_r) = (self.rotation.sin(), self.rotation.cos());
            let ob = self.oblique_angle.tan();
            let xs = if self.relative_x_scale.abs() < 1e-9 {
                1.0
            } else {
                self.relative_x_scale
            };
            let scale = self.size.abs().max(1e-9);
            let mut pts: Vec<[f64; 3]> = Vec::new();
            for poly in polys.iter() {
                if poly.len() < 2 {
                    continue;
                }
                if !pts.is_empty() {
                    pts.push([f64::NAN; 3]);
                }
                for &[px, py] in poly {
                    // unit → entity local: size, X-scale, oblique shear, rotation.
                    let lx = px * scale * xs + py * scale * ob;
                    let ly = py * scale;
                    let wx = lx * cos_r - ly * sin_r;
                    let wy = lx * sin_r + ly * cos_r;
                    let (gx, gy, gz) = crate::scene::view::transform::ocs_point_to_wcs(
                        (
                            self.insertion_point.x + wx,
                            self.insertion_point.y + wy,
                            self.insertion_point.z,
                        ),
                        (self.normal.x, self.normal.y, self.normal.z),
                    );
                    pts.push([gx, gy, gz]);
                }
            }
            if !pts.is_empty() {
                return Some(RenderEntity {
                    pick_tris: Vec::new(),
                    object: RenderObject::Lines(pts),
                    snap_pts: vec![(snap_pt, SnapHint::Insertion)],
                    tangent_geoms: vec![],
                    key_vertices: vec![[ox, oy, oz]],
                    fill_tris: vec![],
                });
            }
        }

        let pts = shape_marker(
            ox,
            oy,
            oz,
            size,
            self.rotation,
            self.relative_x_scale,
            self.oblique_angle,
        );

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts),
            snap_pts: vec![(snap_pt, SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![[ox, oy, oz]],
            fill_tris: vec![],
        })
    }
}

// ── Grippable ─────────────────────────────────────────────────────────────────

impl Grippable for Shape {
    fn grips(&self) -> Vec<GripDef> {
        vec![square_grip(
            0,
            glam::DVec3::new(
                self.insertion_point.x,
                self.insertion_point.y,
                self.insertion_point.z,
            ),
        )]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        }
    }
}

// ── PropertyEditable ──────────────────────────────────────────────────────────

impl PropertyEditable for Shape {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        let style_handle_display = match self.style_handle {
            Some(h) if !h.is_null() => format!("{:X}", h.value()),
            _ => "(none)".to_string(),
        };
        vec![PropSection {
            title: t!("Geometry").into_owned(),
            props: vec![
                ro(t!("Name").as_ref(), "shp_name", self.shape_name.clone()),
                ro(t!("Number").as_ref(), "shp_number", self.shape_number.to_string()),
                Property {
                    label: t!("Style").into_owned(),
                    field: "shp_style",
                    value: PropValue::Choice {
                        selected: self.style_name.clone(),
                        options: text_style_names.to_vec(),
                    },
                },
                ro(t!("Style Handle").as_ref(), "shp_style_handle", style_handle_display),
                edit(t!("Insert X").as_ref(), "shp_ix", self.insertion_point.x),
                edit(t!("Insert Y").as_ref(), "shp_iy", self.insertion_point.y),
                edit(t!("Insert Z").as_ref(), "shp_iz", self.insertion_point.z),
                edit(t!("Size").as_ref(), "shp_sz", self.size),
                edit_angle(t!("Rotation").as_ref(), "shp_rot", self.rotation.to_degrees()),
                edit(t!("Width Factor").as_ref(), "shp_xs", self.relative_x_scale),
                edit_angle(t!("Oblique Angle").as_ref(), "shp_ob", self.oblique_angle.to_degrees()),
                edit(t!("Normal X").as_ref(), "shp_nx", self.normal.x),
                edit(t!("Normal Y").as_ref(), "shp_ny", self.normal.y),
                edit(t!("Normal Z").as_ref(), "shp_nz", self.normal.z),
            ],
        }]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        if field == "shp_style" {
            self.style_name = value.to_string();
            return;
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "shp_ix" => self.insertion_point.x = v,
            "shp_iy" => self.insertion_point.y = v,
            "shp_iz" => self.insertion_point.z = v,
            "shp_sz" => self.size = v.max(0.001),
            "shp_rot" => self.rotation = v.to_radians(),
            "shp_xs" if v.abs() > 1e-9 => self.relative_x_scale = v,
            "shp_ob" => self.oblique_angle = v.to_radians(),
            "shp_nx" => self.normal.x = v,
            "shp_ny" => self.normal.y = v,
            "shp_nz" => self.normal.z = v,
            _ => {}
        }
    }
}

// ── Transformable ─────────────────────────────────────────────────────────────

impl Transformable for Shape {
    fn apply_transform(&mut self, t: &EntityTransform) {
        transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
        });
    }
}
