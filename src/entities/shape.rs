// SHAPE entity — reference to an .SHX shape-file glyph.
//
// Since .SHX binary files are not parsed, we render a small diamond marker
// at the insertion point (same approach as unknown/unsupported entities).
// The shape_name and style_name are surfaced as read-only properties.

use acadrust::entities::Shape;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection};
use crate::scene::transform;
use crate::scene::wire_model::SnapHint;

// ── Marker geometry ───────────────────────────────────────────────────────────

/// Small diamond marker at the shape insertion point.
fn shape_marker(ox: f64, oy: f64, oz: f64, size: f64) -> Vec<[f64; 3]> {
    let s = size * 0.5;
    vec![
        [ox, oy + s, oz], // top
        [ox + s, oy, oz], // right
        [ox, oy - s, oz], // bottom
        [ox - s, oy, oz], // left
        [ox, oy + s, oz], // close
        [f64::NAN; 3],
    ]
}

// ── TruckConvertible ──────────────────────────────────────────────────────────

impl TruckConvertible for Shape {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        let ox = self.insertion_point.x;
        let oy = self.insertion_point.y;
        let oz = self.insertion_point.z;
        let size = self.size.abs().max(0.5);

        let snap_pt = Vec3::new(ox as f32, oy as f32, oz as f32);
        let pts = shape_marker(ox, oy, oz, size);

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
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
            Vec3::new(
                self.insertion_point.x as f32,
                self.insertion_point.y as f32,
                self.insertion_point.z as f32,
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
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("Name", "shp_name", self.shape_name.clone()),
                ro("Style", "shp_style", self.style_name.clone()),
                edit("Insert X", "shp_ix", self.insertion_point.x),
                edit("Insert Y", "shp_iy", self.insertion_point.y),
                edit("Insert Z", "shp_iz", self.insertion_point.z),
                edit("Size", "shp_sz", self.size),
                edit("Rotation", "shp_rot", self.rotation.to_degrees()),
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "shp_ix" => self.insertion_point.x = v,
            "shp_iy" => self.insertion_point.y = v,
            "shp_iz" => self.insertion_point.z = v,
            "shp_sz" => self.size = v.max(0.001),
            "shp_rot" => self.rotation = v.to_radians(),
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
