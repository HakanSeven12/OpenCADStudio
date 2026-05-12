use acadrust::entities::MLine;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, TruckConvertible};
use crate::scene::acad_to_truck::{TruckEntity, TruckObject};
use crate::scene::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::wire_model::SnapHint;

impl TruckConvertible for MLine {
    fn to_truck(&self, _document: &acadrust::CadDocument) -> Option<TruckEntity> {
        if self.vertices.is_empty() {
            return None;
        }

        let n = self.vertices.len();
        let closed = self.flags.contains(acadrust::entities::MLineFlags::CLOSED);

        // Spine: center line connecting all vertex positions.
        // Also attempt to draw parallel offset lines (±scale/2 in miter direction)
        // when scale_factor is non-zero.
        let scale = self.scale_factor;

        let mut pts: Vec<[f64; 3]> = Vec::new();

        // Center spine.
        for v in &self.vertices {
            pts.push([v.position.x, v.position.y, v.position.z]);
        }
        if closed && n >= 2 {
            pts.push([
                self.vertices[0].position.x,
                self.vertices[0].position.y,
                self.vertices[0].position.z,
            ]);
        }

        // Parallel offset lines — one at +scale/2 and one at -scale/2
        // along each vertex's miter direction.
        if scale.abs() > 1e-6 {
            let half = scale * 0.5;
            for sign in [-1.0_f64, 1.0_f64] {
                let offset = half * sign;
                pts.push([f64::NAN; 3]);
                for v in &self.vertices {
                    let mx = v.miter.x;
                    let my = v.miter.y;
                    let mz = v.miter.z;
                    pts.push([
                        v.position.x + mx * offset,
                        v.position.y + my * offset,
                        v.position.z + mz * offset,
                    ]);
                }
                if closed && n >= 2 {
                    let v0 = &self.vertices[0];
                    let mx = v0.miter.x;
                    let my = v0.miter.y;
                    let mz = v0.miter.z;
                    pts.push([
                        v0.position.x + mx * offset,
                        v0.position.y + my * offset,
                        v0.position.z + mz * offset,
                    ]);
                }
            }

            // Start and end caps: perpendicular line connecting the two offset lines
            // at the first and last vertex of an open MLine.
            if !closed {
                let cap_v = |v: &acadrust::entities::MLineVertex| {
                    let mx = v.miter.x;
                    let my = v.miter.y;
                    let mz = v.miter.z;
                    let px = v.position.x;
                    let py = v.position.y;
                    let pz = v.position.z;
                    [
                        [f64::NAN; 3],
                        [px + mx * (-half), py + my * (-half), pz + mz * (-half)],
                        [px + mx * half, py + my * half, pz + mz * half],
                    ]
                };
                pts.extend_from_slice(&cap_v(&self.vertices[0]));
                pts.extend_from_slice(&cap_v(&self.vertices[n - 1]));
            }
        }

        let key_verts: Vec<[f64; 3]> = self
            .vertices
            .iter()
            .map(|v| [v.position.x, v.position.y, v.position.z])
            .collect();

        let snap_pts = self
            .vertices
            .iter()
            .map(|v| {
                (
                    Vec3::new(
                        v.position.x as f32,
                        v.position.y as f32,
                        v.position.z as f32,
                    ),
                    SnapHint::Node,
                )
            })
            .collect();

        Some(TruckEntity {
            object: TruckObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices: key_verts,
            fill_tris: vec![],
        })
    }
}

impl Grippable for MLine {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    Vec3::new(
                        v.position.x as f32,
                        v.position.y as f32,
                        v.position.z as f32,
                    ),
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
}

impl PropertyEditable for MLine {
    fn geometry_properties(&self, _text_style_names: &[String]) -> PropSection {
        PropSection {
            title: "Geometry".into(),
            props: vec![
                ro("Style", "ml_style", self.style_name.clone()),
                ro("Vertices", "ml_verts", self.vertices.len().to_string()),
                edit("Scale", "ml_scale", self.scale_factor),
                Property {
                    label: "Closed".into(),
                    field: "ml_closed",
                    value: PropValue::BoolToggle {
                        field: "ml_closed",
                        value: self.flags.contains(acadrust::entities::MLineFlags::CLOSED),
                    },
                },
            ],
        }
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ml_closed" => {
                let closed = if value == "toggle" {
                    !self.flags.contains(acadrust::entities::MLineFlags::CLOSED)
                } else {
                    value == "true"
                };
                self.flags
                    .set(acadrust::entities::MLineFlags::CLOSED, closed);
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        if field == "ml_scale" && v != 0.0 {
            self.scale_factor = v;
        }
    }
}

impl Transformable for MLine {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::transform::reflect_xy_point(
                    &mut v.position.x,
                    &mut v.position.y,
                    p1,
                    p2,
                );
            }
            crate::scene::transform::reflect_xy_point(
                &mut entity.start_point.x,
                &mut entity.start_point.y,
                p1,
                p2,
            );
        });
    }
}
