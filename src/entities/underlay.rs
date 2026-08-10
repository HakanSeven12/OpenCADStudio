// Underlay entity — PDF/DWF/DGN reference.
//
// Render: clip boundary polygon (or cross at insertion if no boundary).
// Grips:  insertion point + clip boundary vertices.
// Props:  position, scales, rotation, contrast, fade, flags.

use acadrust::entities::{Underlay, UnderlayDisplayFlags};
use crate::t;
use glam::DVec3;

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, edit_angle_prop as edit_angle, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn v3(v: &acadrust::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn v3f32(v: &acadrust::types::Vector3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// Small cross marker at the insertion point (used when no clip boundary).
fn cross_wire(origin: [f64; 3], size: f64) -> Vec<[f64; 3]> {
    let [ox, oy, oz] = origin;
    vec![
        [ox - size, oy, oz],
        [ox + size, oy, oz],
        [f64::NAN; 3],
        [ox, oy - size, oz],
        [ox, oy + size, oz],
    ]
}

// ── RenderConvertible ──────────────────────────────────────────────────────────

/// The underlay's page rectangle in world space (CCW from the insertion),
/// when its definition resolves and the page rasterises: page inches × the
/// entity scale, rotated about the insertion. `None` keeps the cross
/// placeholder.
fn page_quad(
    u: &Underlay,
    document: &acadrust::CadDocument,
) -> Option<[[f64; 3]; 4]> {
    let def = match document.objects.get(&u.definition_handle) {
        Some(acadrust::objects::ObjectType::UnderlayDefinition(d)) => d,
        _ => return None,
    };
    let page = if def.page_name.trim().is_empty() {
        "1"
    } else {
        def.page_name.trim()
    };
    let raster = crate::scene::model::pdf_raster::rasterize_page(&def.file_path, page)?;
    let w = raster.width as f64 / raster.dpi as f64 * u.x_scale;
    let h = raster.height as f64 / raster.dpi as f64 * u.y_scale;
    let (c, s) = (u.rotation.cos(), u.rotation.sin());
    let o = &u.insertion_point;
    let ux = (c * w, s * w);
    let vx = (-s * h, c * h);
    Some([
        [o.x, o.y, o.z],
        [o.x + ux.0, o.y + ux.1, o.z],
        [o.x + ux.0 + vx.0, o.y + ux.1 + vx.1, o.z],
        [o.x + vx.0, o.y + vx.1, o.z],
    ])
}

impl RenderConvertible for Underlay {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        let origin = v3(&self.insertion_point);
        let _origin_f32 = v3f32(&self.insertion_point);

        if !self.clip_boundary_vertices.is_empty() {
            // Draw clip boundary polygon + close it.
            let world_verts = self.world_clip_boundary();
            let mut pts: Vec<[f64; 3]> = world_verts.iter().map(|v| [v.x, v.y, v.z]).collect();
            // Close polygon.
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
            // Insertion grip.
            let key: Vec<[f64; 3]> = pts.clone();
            // Interior pick surface over the clip polygon so the underlay
            // selects on a click anywhere inside, not just on its outline.
            let ring: Vec<[f64; 3]> = world_verts.iter().map(|v| [v.x, v.y, v.z]).collect();
            let pick_tris = crate::entities::mesh::triangulate_planar(&ring);
            Some(RenderEntity {
                pick_tris,
                object: RenderObject::Lines(pts),
                snap_pts: vec![(glam::DVec3::new(self.insertion_point.x, self.insertion_point.y, self.insertion_point.z), SnapHint::Node)],
                tangent_geoms: vec![],
                key_vertices: key,
                fill_tris: vec![],
            })
        } else if let Some(q) = page_quad(self, _document) {
            // Rendered page: draw its frame so selection/pick cover the
            // visible extent instead of a lone cross under the raster.
            let pts = vec![q[0], q[1], q[2], q[3], q[0]];
            let pick_tris = crate::entities::mesh::triangulate_planar(&q.to_vec());
            Some(RenderEntity {
                pick_tris,
                object: RenderObject::Lines(pts),
                snap_pts: vec![(glam::DVec3::new(self.insertion_point.x, self.insertion_point.y, self.insertion_point.z), SnapHint::Node)],
                tangent_geoms: vec![],
                key_vertices: q.to_vec(),
                fill_tris: vec![],
            })
        } else {
            // No clip boundary: draw a cross at insertion point.
            let pts = cross_wire(origin, 1.0);
            Some(RenderEntity {
                pick_tris: Vec::new(),
                object: RenderObject::Lines(pts),
                snap_pts: vec![(glam::DVec3::new(self.insertion_point.x, self.insertion_point.y, self.insertion_point.z), SnapHint::Node)],
                tangent_geoms: vec![],
                key_vertices: vec![origin],
                fill_tris: vec![],
            })
        }
    }
}

// ── Grippable ─────────────────────────────────────────────────────────────────

impl Grippable for Underlay {
    fn grips(&self) -> Vec<GripDef> {
        let origin = glam::DVec3::new(
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        );
        let mut grips = vec![square_grip(0, origin)];

        if !self.clip_boundary_vertices.is_empty() {
            let world_verts = self.world_clip_boundary();
            for (i, v) in world_verts.iter().enumerate() {
                grips.push(center_grip(i + 1, glam::DVec3::new(v.x, v.y, v.z)));
            }
        }

        grips
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            // Insertion point grip.
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
        } else {
            // Clip boundary vertex grip (grip_id = vertex_index + 1).
            let idx = grip_id - 1;
            if idx >= self.clip_boundary_vertices.len() {
                return;
            }
            // Clip boundary vertices are in local (underlay) space.
            // We need to invert the world transform to apply the grip.
            let cos_r = self.rotation.cos();
            let sin_r = self.rotation.sin();
            let new_world = match apply {
                GripApply::Absolute(p) => {
                    // world → local: translate, un-rotate, un-scale
                    let wx = p.x as f64 - self.insertion_point.x;
                    let wy = p.y as f64 - self.insertion_point.y;
                    let lx = (wx * cos_r + wy * sin_r) / self.x_scale.max(1e-10);
                    let ly = (-wx * sin_r + wy * cos_r) / self.y_scale.max(1e-10);
                    (lx, ly)
                }
                GripApply::Translate(d) => {
                    let v = &self.clip_boundary_vertices[idx];
                    let wx = d.x as f64 / self.x_scale.max(1e-10);
                    let wy = d.y as f64 / self.y_scale.max(1e-10);
                    let lx = wx * cos_r + wy * sin_r;
                    let ly = -wx * sin_r + wy * cos_r;
                    (v.x + lx, v.y + ly)
                }
            };
            self.clip_boundary_vertices[idx].x = new_world.0;
            self.clip_boundary_vertices[idx].y = new_world.1;
        }
    }
}

// ── PropertyEditable ──────────────────────────────────────────────────────────

impl PropertyEditable for Underlay {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        // Width / Height derived from the world-space clip boundary bounds
        // (the only in-entity source of the placed footprint size).
        let (width, height) = if self.clip_boundary_vertices.is_empty() {
            (0.0_f64, 0.0_f64)
        } else {
            let verts = self.world_clip_boundary();
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for v in &verts {
                min_x = min_x.min(v.x);
                min_y = min_y.min(v.y);
                max_x = max_x.max(v.x);
                max_y = max_y.max(v.y);
            }
            (max_x - min_x, max_y - min_y)
        };

        let show = self.flags.contains(UnderlayDisplayFlags::ON);
        let clipping = self.flags.contains(UnderlayDisplayFlags::CLIPPING);
        let monochrome = self.flags.contains(UnderlayDisplayFlags::MONOCHROME);
        let adjust_bg = self.flags.contains(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND);

        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Position X").as_ref(), "ul_ix", self.insertion_point.x),
                    edit(t!("Position Y").as_ref(), "ul_iy", self.insertion_point.y),
                    edit(t!("Position Z").as_ref(), "ul_iz", self.insertion_point.z),
                    edit(t!("Scale X").as_ref(), "ul_sx", self.x_scale),
                    edit(t!("Scale Y").as_ref(), "ul_sy", self.y_scale),
                    edit(t!("Scale Z").as_ref(), "ul_sz", self.z_scale),
                    ro(t!("Width").as_ref(), "ul_width", format!("{:.4}", width)),
                    ro(t!("Height").as_ref(), "ul_height", format!("{:.4}", height)),
                    edit_angle(t!("Rotation").as_ref(), "ul_rot", self.rotation.to_degrees()),
                ],
            },
            PropSection {
                title: t!("Underlay Adjust").into_owned(),
                props: vec![
                    edit(t!("Contrast").as_ref(), "ul_contrast", self.contrast as f64),
                    edit(t!("Fade").as_ref(), "ul_fade", self.fade as f64),
                    Property {
                        label: t!("Monochrome").into_owned(),
                        field: "ul_mono",
                        value: PropValue::BoolToggle {
                            field: "ul_mono",
                            value: monochrome,
                        },
                    },
                    Property {
                        label: t!("Adjust Colors for Background").into_owned(),
                        field: "ul_adjust_bg",
                        value: PropValue::BoolToggle {
                            field: "ul_adjust_bg",
                            value: adjust_bg,
                        },
                    },
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    // Underlay name/path/layers live on the separate
                    // UnderlayDefinition object, not reachable from the entity.
                    ro(t!("Underlay name").as_ref(), "ul_name", String::new()),
                    ro(t!("Underlay path").as_ref(), "ul_path", String::new()),
                    Property {
                        label: t!("Show underlay").into_owned(),
                        field: "ul_on",
                        value: PropValue::BoolToggle {
                            field: "ul_on",
                            value: show,
                        },
                    },
                    Property {
                        label: t!("Clipping").into_owned(),
                        field: "ul_clip",
                        value: PropValue::BoolToggle {
                            field: "ul_clip",
                            value: clipping,
                        },
                    },
                    Property {
                        label: t!("Show clipped").into_owned(),
                        field: "ul_clip_inverted",
                        value: PropValue::BoolToggle {
                            field: "ul_clip_inverted",
                            value: self.clip_inverted,
                        },
                    },
                    ro(t!("Underlay layers").as_ref(), "ul_layers", String::new()),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ul_mono" => {
                let on = if value == "toggle" {
                    !self.flags.contains(UnderlayDisplayFlags::MONOCHROME)
                } else {
                    value == "true"
                };
                self.set_monochrome(on);
                return;
            }
            "ul_adjust_bg" => {
                let on = if value == "toggle" {
                    !self.flags.contains(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND)
                } else {
                    value == "true"
                };
                if on {
                    self.flags |= UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND;
                } else {
                    self.flags -= UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND;
                }
                return;
            }
            "ul_on" => {
                let on = if value == "toggle" {
                    !self.flags.contains(UnderlayDisplayFlags::ON)
                } else {
                    value == "true"
                };
                self.set_on(on);
                return;
            }
            "ul_clip" => {
                let on = if value == "toggle" {
                    !self.flags.contains(UnderlayDisplayFlags::CLIPPING)
                } else {
                    value == "true"
                };
                if on {
                    self.flags |= UnderlayDisplayFlags::CLIPPING;
                } else {
                    self.flags -= UnderlayDisplayFlags::CLIPPING;
                }
                return;
            }
            "ul_clip_inverted" => {
                let on = if value == "toggle" {
                    !self.clip_inverted
                } else {
                    value == "true"
                };
                self.clip_inverted = on;
                return;
            }
            _ => {}
        }
        if let Ok(v) = value.trim().parse::<f64>() {
            match field {
                "ul_ix" => self.insertion_point.x = v,
                "ul_iy" => self.insertion_point.y = v,
                "ul_iz" => self.insertion_point.z = v,
                "ul_sx" => self.x_scale = v,
                "ul_sy" => self.y_scale = v,
                "ul_sz" => self.z_scale = v,
                "ul_rot" => self.rotation = v.to_radians(),
                "ul_contrast" => self.set_contrast(v.clamp(0.0, 100.0) as u8),
                "ul_fade" => self.set_fade(v.clamp(0.0, 80.0) as u8),
                _ => {}
            }
        }
    }
}

// ── Transformable ─────────────────────────────────────────────────────────────

impl Transformable for Underlay {
    fn apply_transform(&mut self, t: &EntityTransform) {
        use crate::scene::view::transform::reflect_xy_point;
        match t {
            EntityTransform::Translate(d) => {
                self.insertion_point.x += d.x as f64;
                self.insertion_point.y += d.y as f64;
                self.insertion_point.z += d.z as f64;
            }
            EntityTransform::Mirror { p1, p2, working_normal } => {
                if !working_normal.normalize_or(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-10) {
                    acadrust::Entity::apply_transform(
                        self,
                        &crate::scene::view::transform::reflection_about_working_line(
                            *p1,
                            *p2,
                            *working_normal,
                        ),
                    );
                    return;
                }
                reflect_xy_point(
                    &mut self.insertion_point.x,
                    &mut self.insertion_point.y,
                    *p1,
                    *p2,
                );
                // Reflect rotation angle.
                let dx = (p2.x - p1.x) as f64;
                let dy = (p2.y - p1.y) as f64;
                let axis_angle = dy.atan2(dx);
                self.rotation = 2.0 * axis_angle - self.rotation;
            }
            EntityTransform::Scale { center, factor } => {
                let bx = center.x as f64;
                let by = center.y as f64;
                let bz = center.z as f64;
                let f = *factor as f64;
                self.insertion_point.x = bx + (self.insertion_point.x - bx) * f;
                self.insertion_point.y = by + (self.insertion_point.y - by) * f;
                self.insertion_point.z = bz + (self.insertion_point.z - bz) * f;
                self.x_scale *= f;
                self.y_scale *= f;
                self.z_scale *= f;
            }
            EntityTransform::Rotate { center, axis, angle_rad } => {
                if !axis.normalize_or(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-10) {
                    crate::scene::view::transform::apply_standard_transform(
                        self,
                        *center,
                        *axis,
                        *angle_rad,
                    );
                    return;
                }
                let bx = center.x as f64;
                let by = center.y as f64;
                let a = *angle_rad as f64;
                let cos_a = a.cos();
                let sin_a = a.sin();
                let dx = self.insertion_point.x - bx;
                let dy = self.insertion_point.y - by;
                self.insertion_point.x = bx + dx * cos_a - dy * sin_a;
                self.insertion_point.y = by + dx * sin_a + dy * cos_a;
                self.rotation += a;
            }
            EntityTransform::Affine(transform) => {
                acadrust::Entity::apply_transform(self, transform);
            }
        }
    }
}
