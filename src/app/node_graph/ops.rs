//! The application's own nodes: selecting drawing objects, reading and
//! writing their properties, creating and editing them, and measuring curves.
//! The `graph` crate hands every call to [`AppHost::call`](super::AppHost),
//! which lands in [`AppHost::op`].
//!
//! Edits are replayed: each graph step first undoes what the previous one
//! did (in-place edits restored from their snapshots, created objects
//! erased), so evaluating twice never moves an object twice.

use super::AppHost;
use crate::command::EntityTransform;
use codec::{EntityType, Handle};
use glam::DVec3;
use graph::ops::{b, l, n, s, v};
use graph::value::{list, num, number, point, point_value, text};
use graph::{Eval, NodeId, Spec};
use kernel::geom2d::{cross, Tolerance};
use serde_json::{json, Value};

pub const CATEGORIES: &[&str] = &["Select", "Query", "Create", "Modify", "Curve"];

const OBJECT: &[&str] = &["object"];
const OBJECTS: &[&str] = &["objects"];
const RESULT: &[&str] = &["result"];
const POINT: &[&str] = &["point"];

const fn host(
    name: &'static str,
    category: &'static str,
    inputs: &'static [graph::ops::Input],
    outputs: &'static [&'static str],
) -> Spec {
    Spec {
        name,
        category,
        inputs,
        outputs,
        eval: Eval::Host,
    }
}

pub const LIBRARY: &[Spec] = &[
    // ── Select ───────────────────────────────────────────────────────────
    host("Selected Objects", "Select", &[], OBJECTS),
    host("All Objects", "Select", &[], OBJECTS),
    host("Objects By Type", "Select", &[s("type", "Line")], OBJECTS),
    host("Objects By Layer", "Select", &[s("layer", "0")], OBJECTS),
    host("Select", "Select", &[l("objects")], OBJECTS),
    // ── Query ────────────────────────────────────────────────────────────
    host("Object Type", "Query", &[v("object")], &["type"]),
    host("Get Property", "Query", &[v("object"), s("property", "Layer")], &["value"]),
    host("Set Property", "Query", &[v("object"), s("property", "Layer"), v("value")], OBJECT),
    // ── Create ───────────────────────────────────────────────────────────
    host("Polyline By Points", "Create", &[l("points"), b("closed", false)], OBJECT),
    host("Spline By Points", "Create", &[l("points")], OBJECT),
    host("3D Polyline By Points", "Create", &[l("points")], OBJECT),
    host("Aligned Dimension", "Create", &[v("first"), v("second"), v("location")], OBJECT),
    host(
        "Linear Dimension",
        "Create",
        &[v("first"), v("second"), v("location"), n("angle", 0.0)],
        OBJECT,
    ),
    host(
        "Angular Dimension",
        "Create",
        &[v("vertex"), v("first"), v("second"), v("location")],
        OBJECT,
    ),
    host(
        "Block Reference",
        "Create",
        &[s("block", ""), v("point"), n("scale", 1.0), n("rotation", 0.0)],
        OBJECT,
    ),
    // ── Modify ───────────────────────────────────────────────────────────
    host("Move", "Modify", &[v("object"), v("vector")], OBJECT),
    host("Rotate", "Modify", &[v("object"), v("center"), n("angle", 90.0)], OBJECT),
    host("Scale", "Modify", &[v("object"), v("center"), n("factor", 2.0)], OBJECT),
    host("Mirror", "Modify", &[v("object"), v("first"), v("second")], OBJECT),
    host("Copy", "Modify", &[v("object"), v("vector")], OBJECT),
    host("Offset", "Modify", &[v("object"), n("distance", 1.0), v("side")], OBJECTS),
    // ── Curve ────────────────────────────────────────────────────────────
    host("Curve Length", "Curve", &[v("curve")], RESULT),
    host("Curve Area", "Curve", &[v("curve")], RESULT),
    host("Curve Endpoints", "Curve", &[v("curve")], &["start", "end"]),
    host("Point At Parameter", "Curve", &[v("curve"), n("t", 0.5)], POINT),
    host("Point At Distance", "Curve", &[v("curve"), n("distance", 0.0)], POINT),
    host("Parameter At Point", "Curve", &[v("curve"), v("point")], &["t"]),
    host("Divide Curve", "Curve", &[v("curve"), n("count", 10.0)], &["points"]),
    host("Intersect", "Curve", &[v("a"), v("b")], &["points"]),
];

/// Upper bound on generated point counts, as in the graph's own library.
const MAX_ITEMS: usize = 100_000;

/// The node called `name`: the application's first, then the graph's.
pub fn resolve(name: &str) -> Option<&'static Spec> {
    LIBRARY
        .iter()
        .find(|spec| spec.name == name)
        .or_else(|| graph::spec(name))
}

pub fn handle(value: &Value) -> Option<Handle> {
    Some(Handle::new(value.get("handle")?.as_u64()?))
}

pub fn object(handle: Handle) -> Value {
    json!({ "handle": handle.value() })
}

fn vec3(value: &Value) -> Option<DVec3> {
    point(value).map(DVec3::from_array)
}

fn same_plane(a: &kernel::space::Plane, b: &kernel::space::Plane) -> bool {
    let close = |p: [f64; 3], q: [f64; 3]| p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 1e-9);
    close(a.origin, b.origin) && close(a.x_axis, b.x_axis) && close(a.y_axis, b.y_axis)
}

impl AppHost<'_> {
    pub(super) fn op(&mut self, _node: NodeId, spec: &'static Spec, a: &[Value]) -> Vec<Value> {
        let one = |value: Value| vec![value];
        let objects = |handles: Vec<Handle>| vec![Value::Array(handles.into_iter().map(object).collect())];
        match spec.name {
            "Selected Objects" => objects(
                self.scene()
                    .selected_entities()
                    .into_iter()
                    .map(|(handle, _)| handle)
                    .collect(),
            ),
            "All Objects" => objects(self.space(|_| true)),
            "Objects By Type" => {
                let kind = text(&a[0]);
                objects(self.space(|entity| crate::app::automation::entity_type_matches(entity, &kind)))
            }
            "Objects By Layer" => {
                let layer = text(&a[0]);
                objects(self.space(|entity| entity.common().layer.eq_ignore_ascii_case(&layer)))
            }
            "Select" => {
                let handles: Vec<Handle> = list(&a[0]).iter().filter_map(handle).collect();
                self.app.tabs[self.i]
                    .scene
                    .replace_selection(handles.iter().copied().collect());
                self.app.refresh_properties();
                objects(handles)
            }
            "Object Type" => one(
                self.entity(&a[0])
                    .map_or(Value::Null, |entity| json!(crate::entities::names::ui_name(entity))),
            ),
            "Get Property" => one(
                handle(&a[0])
                    .and_then(|h| self.app.graph_field(self.i, h, &text(&a[1])))
                    .map_or(Value::Null, |(_, value)| value),
            ),
            "Set Property" => {
                let Some(h) = handle(&a[0]) else {
                    return one(Value::Null);
                };
                if let Some((field, current)) = self.app.graph_field(self.i, h, &text(&a[1])) {
                    if current != a[2] {
                        let task = self.app.graph_write(self.i, h, &field, &a[2]);
                        self.tasks.push(task);
                    }
                }
                one(object(h))
            }
            "Polyline By Points" => {
                let vertices: Vec<Value> = list(&a[0])
                    .iter()
                    .filter_map(point)
                    .map(|p| json!([p[0], p[1]]))
                    .collect();
                one(self.build(json!({
                    "type": "LWPOLYLINE",
                    "vertices": vertices,
                    "closed": graph::value::boolean(&a[1]).unwrap_or(false),
                })))
            }
            "Spline By Points" => {
                let points: Vec<[f64; 3]> = list(&a[0]).iter().filter_map(point).collect();
                one(self.build(json!({ "type": "SPLINE", "fit_points": points })))
            }
            "3D Polyline By Points" => {
                let points: Vec<codec::types::Vector3> = list(&a[0])
                    .iter()
                    .filter_map(point)
                    .map(|p| codec::types::Vector3::new(p[0], p[1], p[2]))
                    .collect();
                if points.len() < 2 {
                    return one(Value::Null);
                }
                one(self.create(EntityType::Polyline3D(
                    codec::entities::Polyline3D::from_points(points),
                )))
            }
            "Aligned Dimension" => one(match (vec3(&a[0]), vec3(&a[1]), vec3(&a[2])) {
                (Some(first), Some(second), Some(location)) if first != second => self.create(
                    crate::modules::annotate::aligned_dim::aligned_dimension_entity(
                        first, second, location, None,
                    ),
                ),
                _ => Value::Null,
            }),
            "Linear Dimension" => one(match (vec3(&a[0]), vec3(&a[1]), vec3(&a[2])) {
                (Some(first), Some(second), Some(location)) if first != second => {
                    let (sin, cos) = num(&a[3]).unwrap_or(0.0).to_radians().sin_cos();
                    self.create(crate::modules::annotate::linear_dim::linear_dimension_entity(
                        first,
                        second,
                        location,
                        DVec3::new(cos, sin, 0.0),
                        None,
                    ))
                }
                _ => Value::Null,
            }),
            "Angular Dimension" => {
                let entity = match (vec3(&a[0]), vec3(&a[1]), vec3(&a[2]), vec3(&a[3])) {
                    (Some(vertex), Some(first), Some(second), Some(location)) => {
                        crate::modules::annotate::angular_dim::angular_three_point_entity(
                            vertex, first, second, location, None,
                        )
                    }
                    _ => None,
                };
                one(entity.map_or(Value::Null, |entity| self.create(entity)))
            }
            "Block Reference" => one(match point(&a[1]) {
                Some(p) => self.build(json!({
                    "type": "INSERT",
                    "block": text(&a[0]),
                    "position": p,
                    "scale": num(&a[2]).unwrap_or(1.0),
                    "rotation_deg": num(&a[3]).unwrap_or(0.0),
                })),
                None => Value::Null,
            }),
            "Move" => one(match vec3(&a[1]) {
                Some(delta) => self.edit(&a[0], EntityTransform::Translate(delta)),
                None => a[0].clone(),
            }),
            "Rotate" => {
                let t = EntityTransform::Rotate {
                    center: vec3(&a[1]).unwrap_or(DVec3::ZERO),
                    axis: DVec3::Z,
                    angle_rad: num(&a[2]).unwrap_or(0.0).to_radians(),
                };
                one(self.edit(&a[0], t))
            }
            "Scale" => one(match num(&a[2]).filter(|f| f.is_finite() && *f != 0.0) {
                Some(factor) => self.edit(
                    &a[0],
                    EntityTransform::Scale {
                        center: vec3(&a[1]).unwrap_or(DVec3::ZERO),
                        factor,
                    },
                ),
                None => a[0].clone(),
            }),
            "Mirror" => one(match (vec3(&a[1]), vec3(&a[2])) {
                (Some(p1), Some(p2)) if p1 != p2 => self.edit(
                    &a[0],
                    EntityTransform::Mirror {
                        p1,
                        p2,
                        working_normal: DVec3::Z,
                    },
                ),
                _ => a[0].clone(),
            }),
            "Copy" => one(match (handle(&a[0]), vec3(&a[1])) {
                (Some(h), Some(delta)) => {
                    let tab = &mut self.app.tabs[self.i];
                    let copies = tab
                        .scene
                        .copy_entities(&[h], &EntityTransform::Translate(delta));
                    tab.dirty |= !copies.is_empty();
                    tab.graph.created.extend(&copies);
                    copies.first().copied().map_or(Value::Null, object)
                }
                _ => Value::Null,
            }),
            "Offset" => {
                let (Some(entity), Some(distance), Some(side)) =
                    (self.entity(&a[0]).cloned(), num(&a[1]), point(&a[2]))
                else {
                    return objects(Vec::new());
                };
                let side = glam::Vec3::new(side[0] as f32, side[1] as f32, side[2] as f32);
                let results =
                    crate::modules::draw::modify::offset::compute_offsets(&entity, distance.abs(), side);
                vec![Value::Array(
                    results.into_iter().map(|entity| self.create(entity)).collect(),
                )]
            }
            "Curve Length" => one(self.curve(&a[0]).map_or(Value::Null, |c| number(c.length()))),
            "Curve Area" => one(
                self.curve(&a[0])
                    .map_or(Value::Null, |c| number(c.curve.enclosed_area().abs())),
            ),
            "Curve Endpoints" => match self.curve(&a[0]) {
                Some(c) => vec![point_value(c.point_at(0.0)), point_value(c.point_at(1.0))],
                None => vec![Value::Null; 2],
            },
            "Point At Parameter" => one(match (self.curve(&a[0]), num(&a[1])) {
                (Some(c), Some(t)) => point_value(c.point_at(t)),
                _ => Value::Null,
            }),
            "Point At Distance" => one(match (self.curve(&a[0]), num(&a[1])) {
                (Some(c), Some(d)) => point_value(c.point_at_distance(d)),
                _ => Value::Null,
            }),
            "Parameter At Point" => one(match (self.curve(&a[0]), point(&a[1])) {
                (Some(c), Some(p)) => c.parameter_at(p).map_or(Value::Null, number),
                _ => Value::Null,
            }),
            "Divide Curve" => one(match (self.curve(&a[0]), num(&a[1])) {
                (Some(c), Some(count)) if count >= 1.0 => {
                    let (count, length) = ((count as usize).min(MAX_ITEMS), c.length());
                    Value::Array(
                        (0..=count)
                            .map(|k| point_value(c.point_at_distance(length * k as f64 / count as f64)))
                            .collect(),
                    )
                }
                _ => Value::Null,
            }),
            // Curves on different planes meet at most in isolated points the
            // plane-curve intersection cannot see; they report none.
            "Intersect" => one(match (self.curve(&a[0]), self.curve(&a[1])) {
                (Some(p), Some(q)) if same_plane(&p.plane, &q.plane) => Value::Array(
                    cross::intersect(&p.curve, &q.curve, Tolerance::new(1e-9))
                        .into_iter()
                        .map(|crossing| point_value(p.plane.point_at(crossing.point)))
                        .collect(),
                ),
                _ => Value::Array(Vec::new()),
            }),
            _ => Vec::new(),
        }
    }

    fn scene(&self) -> &crate::scene::Scene {
        &self.app.tabs[self.i].scene
    }

    fn entity(&self, value: &Value) -> Option<&EntityType> {
        self.scene().document.get_entity(handle(value)?)
    }

    fn curve(&self, value: &Value) -> Option<kernel::space::PlanarCurve> {
        crate::entities::curve::entity_curve(self.entity(value)?)
    }

    /// Objects of the space being edited that `keep` accepts.
    fn space(&self, keep: impl Fn(&EntityType) -> bool) -> Vec<Handle> {
        let scene = self.scene();
        let block = scene.interaction_block_handle();
        scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == block)
            .map(|record| {
                record
                    .entity_handles
                    .iter()
                    .copied()
                    .filter(|h| scene.document.get_entity(*h).is_some_and(&keep))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Adds `entity` the way a drawing command does; the next step erases it.
    fn create(&mut self, entity: EntityType) -> Value {
        let Some(h) = self.app.commit_entity_handle(entity) else {
            return Value::Null;
        };
        let tab = &mut self.app.tabs[self.i];
        tab.graph.created.push(h);
        tab.dirty = true;
        object(h)
    }

    /// [`create`](Self::create) from an `entities_create` definition.
    fn build(&mut self, definition: Value) -> Value {
        let built =
            crate::entity_creation::build_entity(&definition, Some(&self.scene().document));
        built.map_or(Value::Null, |entity| self.create(entity))
    }

    /// Transforms an object in place, keeping its prior state so the next
    /// step can put it back first.
    fn edit(&mut self, value: &Value, t: EntityTransform) -> Value {
        let Some(h) = handle(value) else {
            return Value::Null;
        };
        let tab = &mut self.app.tabs[self.i];
        let Some(before) = tab.scene.document.get_entity(h).cloned() else {
            return Value::Null;
        };
        tab.graph.applied.push((h, before));
        tab.scene.transform_entities(&[h], &t);
        tab.dirty = true;
        object(h)
    }
}
