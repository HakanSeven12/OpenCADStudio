// Underlay entity — PDF/DWF/DGN reference.
//
// Render: clip boundary polygon (or cross at insertion if no boundary).
// Grips:  insertion point + clip boundary vertices.
// Props:  position, scales, rotation, contrast, fade, flags.

use acadrust::entities::{Underlay, UnderlayDisplayFlags};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{diamond_grip, edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::wire_model::SnapHint;

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

// ── TruckConvertible ──────────────────────────────────────────────────────────

impl TruckConvertible for Underlay {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let origin = v3(&self.insertion_point);
        let origin_f32 = v3f32(&self.insertion_point);

        if !self.clip_boundary_vertices.is_empty() {
            // Draw clip boundary polygon + close it.
            let world_verts = self.world_clip_boundary();
            let mut pts: Vec<[f64; 3]> = world_verts
                .iter()
                .map(|v| [v.x, v.y, v.z])
                .collect();
            // Close polygon.
            if let Some(&first) = pts.first() {
                pts.push(first);
            }
            // Insertion grip.
            let key: Vec<[f64; 3]> = pts.clone();
            Some(TruckEntity {
                object: TruckObject::Lines(pts),
                snap_pts: vec![(Vec3::from(origin_f32), SnapHint::Node)],
                tangent_geoms: vec![],
                key_vertices: key,
                fill_tris: vec![],
            })
        } else {
            // No clip boundary: draw a cross at insertion point.
            let pts = cross_wire(origin, 1.0);
            Some(TruckEntity {
                object: TruckObject::Lines(pts),
                snap_pts: vec![(Vec3::from(origin_f32), SnapHint::Node)],
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
        let origin = Vec3::from(v3f32(&self.insertion_point));
        let mut grips = vec![square_grip(0, origin)];

        if !self.clip_boundary_vertices.is_empty() {
            let world_verts = self.world_clip_boundary();
            for (i, v) in world_verts.iter().enumerate() {
                grips.push(diamond_grip(
                    i + 1,
                    Vec3::new(v.x as f32, v.y as f32, v.z as f32),
                ));
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
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        let type_str = match self.underlay_type {
            acadrust::entities::UnderlayType::Pdf => "PDF",
            acadrust::entities::UnderlayType::Dwf => "DWF",
            acadrust::entities::UnderlayType::Dgn => "DGN",
        };
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("Type", "ul_type", type_str),
                edit("Ins X", "ul_ix", self.insertion_point.x),
                edit("Ins Y", "ul_iy", self.insertion_point.y),
                edit("Ins Z", "ul_iz", self.insertion_point.z),
                edit("X Scale", "ul_sx", self.x_scale),
                edit("Y Scale", "ul_sy", self.y_scale),
                edit("Z Scale", "ul_sz", self.z_scale),
                edit("Rotation", "ul_rot", self.rotation.to_degrees()),
                edit("Contrast", "ul_contrast", self.contrast as f64),
                edit("Fade", "ul_fade", self.fade as f64),
                ro(
                    "On",
                    "ul_on",
                    if self.flags.contains(UnderlayDisplayFlags::ON) {
                        "Yes"
                    } else {
                        "No"
                    },
                ),
                ro(
                    "Clipping",
                    "ul_clip",
                    if self.flags.contains(UnderlayDisplayFlags::CLIPPING) {
                        "Yes"
                    } else {
                        "No"
                    },
                ),
                ro(
                    "Monochrome",
                    "ul_mono",
                    if self.flags.contains(UnderlayDisplayFlags::MONOCHROME) {
                        "Yes"
                    } else {
                        "No"
                    },
                ),
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
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
        use crate::scene::transform::reflect_xy_point;
        match t {
            EntityTransform::Translate(d) => {
                self.insertion_point.x += d.x as f64;
                self.insertion_point.y += d.y as f64;
                self.insertion_point.z += d.z as f64;
            }
            EntityTransform::Mirror { p1, p2 } => {
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
            EntityTransform::Rotate { center, angle_rad } => {
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
        }
    }
}
