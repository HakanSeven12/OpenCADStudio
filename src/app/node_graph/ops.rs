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
use document::{EntityType, Handle};
use glam::DVec3;
use graph::ops::{b, l, n, s, v};
use graph::value::{list, num, number, point, point_value, text};
use graph::{Eval, NodeId, Spec};
use kernel::geom2d::{cross, Tolerance};
use crate::scene::model::solid_history;
use serde_json::{json, Value};

pub const CATEGORIES: &[&str] = &["Select", "Query", "Create", "Modify", "Model", "Curve"];

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
    host(
        "Rectangle",
        "Create",
        &[v("corner"), n("width", 10.0), n("height", 5.0), n("rotation", 0.0)],
        OBJECT,
    ),
    host(
        "Polygon",
        "Create",
        &[
            v("center"),
            n("radius", 5.0),
            n("sides", 6.0),
            b("circumscribed", false),
            n("rotation", 0.0),
        ],
        OBJECT,
    ),
    host("Donut", "Create", &[v("center"), n("inside", 0.5), n("outside", 1.0)], OBJECT),
    host(
        "Revision Cloud",
        "Create",
        &[l("points"), n("arc length", 0.5), b("closed", true)],
        OBJECT,
    ),
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
        "Box",
        "Create",
        &[v("corner"), n("length", 10.0), n("width", 10.0), n("height", 10.0)],
        OBJECT,
    ),
    host(
        "Wedge",
        "Create",
        &[v("corner"), n("length", 10.0), n("width", 10.0), n("height", 10.0)],
        OBJECT,
    ),
    host("Cylinder", "Create", &[v("center"), n("radius", 5.0), n("height", 10.0)], OBJECT),
    host(
        "Cone",
        "Create",
        &[v("center"), n("radius", 5.0), n("top radius", 0.0), n("height", 10.0)],
        OBJECT,
    ),
    host("Sphere", "Create", &[v("center"), n("radius", 5.0)], OBJECT),
    host("Torus", "Create", &[v("center"), n("radius", 10.0), n("tube radius", 2.0)], OBJECT),
    host(
        "Pyramid",
        "Create",
        &[
            v("center"),
            n("radius", 5.0),
            n("top radius", 0.0),
            n("height", 10.0),
            n("sides", 4.0),
        ],
        OBJECT,
    ),
    host("Region", "Create", &[v("object")], OBJECT),
    host("Extrude", "Create", &[v("object"), n("height", 10.0)], OBJECT),
    host("Viewport", "Create", &[v("first"), v("second")], OBJECT),
    host("Raster Image", "Create", &[s("file", ""), v("point"), n("width", 10.0)], OBJECT),
    host("Embedded Image", "Create", &[s("file", ""), v("point"), n("width", 10.0)], OBJECT),
    host(
        "PDF Underlay",
        "Create",
        &[s("file", ""), s("page", "1"), v("point"), n("scale", 1.0)],
        OBJECT,
    ),
    host("Radius Dimension", "Create", &[v("object"), v("location")], OBJECT),
    host("Diameter Dimension", "Create", &[v("object"), v("location")], OBJECT),
    host("Ordinate Dimension", "Create", &[v("feature"), v("leader")], OBJECT),
    host("Arc Length Dimension", "Create", &[v("object"), v("location")], OBJECT),
    host(
        "Jogged Radius Dimension",
        "Create",
        &[v("object"), v("center"), v("location"), v("jog")],
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
    host("Trim", "Modify", &[v("object"), v("point")], OBJECTS),
    host("Extend", "Modify", &[v("object"), v("point")], OBJECTS),
    host(
        "Fillet",
        "Modify",
        &[v("first"), v("first point"), v("second"), v("second point"), n("radius", 1.0)],
        OBJECTS,
    ),
    host(
        "Chamfer",
        "Modify",
        &[
            v("first"),
            v("first point"),
            v("second"),
            v("second point"),
            n("distance", 1.0),
            n("second distance", 1.0),
        ],
        OBJECTS,
    ),
    host(
        "Blend",
        "Modify",
        &[v("first"), v("first point"), v("second"), v("second point")],
        OBJECTS,
    ),
    host("Break", "Modify", &[v("object"), v("first point"), v("second point")], OBJECTS),
    host("Break At Point", "Modify", &[v("object"), v("point")], OBJECTS),
    host("Lengthen", "Modify", &[v("object"), v("point"), n("delta", 1.0)], OBJECTS),
    host("Join", "Modify", &[l("objects")], OBJECTS),
    host("Explode", "Modify", &[l("objects")], OBJECTS),
    host("Convert To Polyline", "Modify", &[v("object"), v("point")], OBJECTS),
    host("Reverse", "Modify", &[l("objects")], OBJECTS),
    host(
        "Stretch",
        "Modify",
        &[v("first corner"), v("second corner"), v("base"), v("second")],
        OBJECTS,
    ),
    host(
        "Align",
        "Modify",
        &[l("objects"), v("first source"), v("first target"), v("second source"), v("second target"), b("scale", false)],
        OBJECTS,
    ),
    host(
        "Rectangular Array",
        "Modify",
        &[l("objects"), n("rows", 2.0), n("columns", 3.0), n("row spacing", 10.0), n("column spacing", 10.0)],
        OBJECTS,
    ),
    host(
        "Polar Array",
        "Modify",
        &[l("objects"), v("center"), n("count", 6.0), n("angle", 360.0)],
        OBJECTS,
    ),
    host("Path Array", "Modify", &[l("objects"), v("path"), n("count", 5.0)], OBJECTS),
    host("Boundary", "Modify", &[v("point")], OBJECTS),
    host("Hatch At Point", "Modify", &[v("point")], OBJECTS),
    host("Hatch Objects", "Modify", &[l("objects")], OBJECTS),
    host("Centerline", "Modify", &[v("first"), v("first point"), v("second"), v("second point")], OBJECTS),
    host("Center Mark", "Modify", &[v("object"), v("point")], OBJECTS),
    // ── Model ────────────────────────────────────────────────────────────
    host("Union", "Model", &[l("objects")], OBJECTS),
    host("Subtract", "Model", &[l("from"), l("objects")], OBJECTS),
    host("Intersection", "Model", &[l("objects")], OBJECTS),
    host(
        "Revolve",
        "Model",
        &[l("objects"), v("axis start"), v("axis end"), n("angle", 360.0)],
        OBJECTS,
    ),
    host("Sweep", "Model", &[l("objects"), v("path")], OBJECTS),
    host("Loft", "Model", &[l("sections")], OBJECTS),
    host("Extrude Along Path", "Model", &[l("objects"), v("path")], OBJECTS),
    host("Extrude Tapered", "Model", &[l("objects"), n("height", 10.0), n("taper", 5.0)], OBJECTS),
    host("Fillet Edge", "Model", &[v("solid"), v("edge point"), n("radius", 1.0)], OBJECTS),
    host("Chamfer Edge", "Model", &[v("solid"), v("edge point"), n("distance", 1.0)], OBJECTS),
    host("Shell", "Model", &[v("solid"), v("face point"), n("thickness", 1.0)], OBJECTS),
    host("Slice", "Model", &[l("objects"), v("first"), v("second"), v("third"), v("keep")], OBJECTS),
    host("Thicken", "Model", &[l("objects"), n("thickness", 1.0)], OBJECTS),
    host("Presspull", "Model", &[v("object"), v("point"), n("distance", 5.0)], OBJECTS),
    host(
        "3D Rotate",
        "Model",
        &[l("objects"), v("axis start"), v("axis end"), n("angle", 90.0)],
        OBJECTS,
    ),
    host("3D Mirror", "Model", &[l("objects"), v("first"), v("second"), v("third")], OBJECTS),
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

/// A closed plan polyline through `vertices` at height `z`.
fn closed_polyline(vertices: &[[f64; 2]], z: f64) -> EntityType {
    let mut polyline = document::entities::LwPolyline::new();
    polyline.is_closed = true;
    polyline.elevation = z;
    polyline.vertices = vertices
        .iter()
        .map(|p| document::entities::LwVertex::new(document::types::Vector2::new(p[0], p[1])))
        .collect();
    EntityType::LwPolyline(polyline)
}

/// A pick of `object` at `at`.
fn pick(object: &Value, at: &Value) -> Option<crate::command::StepInput> {
    Some(crate::command::StepInput::EntityPick(handle(object)?, vec3(at)?))
}

/// A pick of `object` anywhere on it.
fn pick_on(object: &Value) -> Option<crate::command::StepInput> {
    Some(crate::command::StepInput::EntityPick(handle(object)?, DVec3::ZERO))
}

/// The objects of `value` (one or a list) as a finished selection.
fn selection(value: &Value) -> Option<crate::command::StepInput> {
    let handles: Vec<Handle> = list(value).iter().filter_map(handle).collect();
    (!handles.is_empty()).then_some(crate::command::StepInput::SelectionComplete(handles))
}

/// A typed answer.
fn typed(value: impl std::fmt::Display) -> crate::command::StepInput {
    crate::command::StepInput::Text(value.to_string())
}

/// A history transform placing a primitive at `origin`.
fn placed(origin: DVec3) -> [f64; 16] {
    glam::DMat4::from_translation(origin).to_cols_array()
}

fn same_plane(a: &kernel::space::Plane, b: &kernel::space::Plane) -> bool {
    let close = |p: [f64; 3], q: [f64; 3]| p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 1e-9);
    close(a.origin, b.origin) && close(a.x_axis, b.x_axis) && close(a.y_axis, b.y_axis)
}

impl AppHost<'_> {
    pub(super) fn op(&mut self, _node: NodeId, spec: &'static Spec, a: &[Value]) -> Vec<Value> {
        use crate::command::StepInput::{Enter, EntityPick, Point};
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
                let points: Vec<document::types::Vector3> = list(&a[0])
                    .iter()
                    .filter_map(point)
                    .map(|p| document::types::Vector3::new(p[0], p[1], p[2]))
                    .collect();
                if points.len() < 2 {
                    return one(Value::Null);
                }
                one(self.create(EntityType::Polyline3D(
                    document::entities::Polyline3D::from_points(points),
                )))
            }
            "Rectangle" => one(match vec3(&a[0]) {
                Some(corner) => {
                    let (w, h) = (num(&a[1]).unwrap_or(0.0), num(&a[2]).unwrap_or(0.0));
                    let (sin, cos) = num(&a[3]).unwrap_or(0.0).to_radians().sin_cos();
                    let at = |x: f64, y: f64| [corner.x + x * cos - y * sin, corner.y + x * sin + y * cos];
                    if w * h == 0.0 || !(w * h).is_finite() {
                        Value::Null
                    } else {
                        self.create(closed_polyline(
                            &[at(0.0, 0.0), at(w, 0.0), at(w, h), at(0.0, h)],
                            corner.z,
                        ))
                    }
                }
                None => Value::Null,
            }),
            // The bottom edge lies level at rotation 0, as POLYGON draws a
            // typed radius; circumscribed puts the radius at the edge middles.
            "Polygon" => {
                let sides = num(&a[2]).unwrap_or(0.0).round();
                let radius = num(&a[1]).unwrap_or(0.0);
                one(match vec3(&a[0]) {
                    Some(c) if (3.0..=1024.0).contains(&sides) && radius > 0.0 => {
                        let step = std::f64::consts::TAU / sides;
                        let r = if graph::value::boolean(&a[3]).unwrap_or(false) {
                            radius / (step / 2.0).cos()
                        } else {
                            radius
                        };
                        let start = num(&a[4]).unwrap_or(0.0).to_radians()
                            - std::f64::consts::FRAC_PI_2
                            + step / 2.0;
                        let vertices: Vec<[f64; 2]> = (0..sides as usize)
                            .map(|i| {
                                let angle = start + step * i as f64;
                                [c.x + r * angle.cos(), c.y + r * angle.sin()]
                            })
                            .collect();
                        self.create(closed_polyline(&vertices, c.z))
                    }
                    _ => Value::Null,
                })
            }
            "Donut" => {
                let (inside, outside) = (num(&a[1]).unwrap_or(0.0), num(&a[2]).unwrap_or(0.0));
                let (inner, outer) = (inside.min(outside) / 2.0, inside.max(outside) / 2.0);
                one(match vec3(&a[0]) {
                    Some(c) if inner >= 0.0 && outer > 0.0 && outer.is_finite() => self.create(
                        crate::modules::draw::draw::donut::make_donut(c.x, c.y, c.z, inner, outer),
                    ),
                    _ => Value::Null,
                })
            }
            "Revision Cloud" => {
                let points: Vec<[f64; 3]> = list(&a[0]).iter().filter_map(point).collect();
                let closed = graph::value::boolean(&a[2]).unwrap_or(true);
                let curve = kernel::geom2d::Curve::Polyline(kernel::geom2d::Polyline {
                    vertices: points
                        .iter()
                        .map(|p| kernel::geom2d::PolylineVertex::straight([p[0], p[1]]))
                        .collect(),
                    closed,
                });
                let cloud = (points.len() >= 2)
                    .then(|| {
                        crate::entities::revcloud::cloud_along(
                            &curve,
                            num(&a[1]).unwrap_or(0.0),
                            false,
                            false,
                            false,
                        )
                    })
                    .flatten();
                one(match cloud {
                    Some(mut cloud) => {
                        cloud.elevation = points[0][2];
                        self.create(EntityType::LwPolyline(cloud))
                    }
                    None => Value::Null,
                })
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
            // The circle or arc of `object` nearest `location`, which also
            // places the text, as DIMRADIUS and DIMDIAMETER take a pick.
            "Radius Dimension" | "Diameter Dimension" => {
                let entity = match (self.entity(&a[0]), vec3(&a[1])) {
                    (Some(entity), Some(location)) => crate::scene::dimension_assoc::radial_source_at(
                        entity,
                        document::types::Vector3::new(location.x, location.y, location.z),
                    )
                    .map(|source| {
                        if spec.name == "Radius Dimension" {
                            crate::modules::annotate::radius_dim::radial_dimension_entity(
                                source, location, None, None,
                            )
                        } else {
                            crate::modules::annotate::diameter_dim::diameter_dimension_entity(
                                source, location, None, None,
                            )
                        }
                    }),
                    _ => None,
                };
                one(entity.map_or(Value::Null, |entity| self.create(entity)))
            }
            "Box" | "Wedge" => {
                let size = [num(&a[1]), num(&a[2]), num(&a[3])].map(|v| v.unwrap_or(0.0));
                one(match vec3(&a[0]) {
                    // The corner is the low one; the history places by centre.
                    Some(corner) if size.iter().all(|v| v.is_finite() && *v > 0.0) => {
                        let center = corner + DVec3::from_array(size) * 0.5;
                        let place = placed(center);
                        self.primitive(if spec.name == "Box" {
                            solid_history::box_op(place, size[0], size[1], size[2])
                        } else {
                            solid_history::wedge_op(place, size[0], size[1], size[2])
                        })
                    }
                    _ => Value::Null,
                })
            }
            "Cylinder" => one(match (vec3(&a[0]), num(&a[1]), num(&a[2])) {
                (Some(c), Some(r), Some(h)) if r > 0.0 && h != 0.0 => {
                    self.primitive(solid_history::cylinder_op(placed(c), r, h))
                }
                _ => Value::Null,
            }),
            "Cone" => one(match (vec3(&a[0]), num(&a[1]), num(&a[2]), num(&a[3])) {
                (Some(c), Some(r), Some(top), Some(h)) if r > 0.0 && top >= 0.0 && h != 0.0 => {
                    self.primitive(solid_history::cone_op(placed(c), r, r, top, h))
                }
                _ => Value::Null,
            }),
            "Sphere" => one(match (vec3(&a[0]), num(&a[1])) {
                (Some(c), Some(r)) if r > 0.0 => self.primitive(solid_history::sphere_op(placed(c), r)),
                _ => Value::Null,
            }),
            "Torus" => one(match (vec3(&a[0]), num(&a[1]), num(&a[2])) {
                (Some(c), Some(r), Some(tube)) if tube > 0.0 && tube < r => {
                    self.primitive(solid_history::torus_op(placed(c), r, tube))
                }
                _ => Value::Null,
            }),
            "Pyramid" => {
                let sides = num(&a[4]).unwrap_or(0.0).round();
                one(match (vec3(&a[0]), num(&a[1]), num(&a[2]), num(&a[3])) {
                    (Some(c), Some(r), Some(top), Some(h))
                        if r > 0.0 && top >= 0.0 && h != 0.0 && (3.0..=32.0).contains(&sides) =>
                    {
                        self.primitive(solid_history::pyramid_op(placed(c), r, top, h, sides as usize, false))
                    }
                    _ => Value::Null,
                })
            }
            // The planar area a closed object bounds, as REGION makes it.
            "Region" => {
                let body = self.entity(&a[0]).and_then(|entity| {
                    let (plane, loops, true) =
                        crate::scene::model::presspull_model::profile_geometry(entity)?
                    else {
                        return None;
                    };
                    Some((plane.origin, kernel::brep::planar_region(plane, &loops)?))
                });
                one(match body {
                    Some((origin, body)) => {
                        let mut region = document::entities::Region::new();
                        region.point_of_reference =
                            document::types::Vector3::new(origin[0], origin[1], origin[2]);
                        let handle = self.app.add_region_model(region, body);
                        self.created(handle)
                    }
                    None => Value::Null,
                })
            }
            // A closed object makes a solid, an open one a surface, along
            // its plane's normal (Z for a line).
            "Extrude" => {
                let height = num(&a[1]).unwrap_or(0.0);
                let Some(entity) = self.entity(&a[0]).cloned().filter(|_| height != 0.0) else {
                    return one(Value::Null);
                };
                use crate::scene::model::sweep_model;
                if let Some(body) = sweep_model::extruded(&entity, height) {
                    let op = solid_history::brep_op(&body);
                    return one(self.solid(body, op));
                }
                let direction = DVec3::new(0.0, 0.0, height);
                one(match sweep_model::extruded_surface(&entity, direction.to_array(), 0.0) {
                    Some(body) => {
                        let surface =
                            crate::modules::insert::solid3d_cmds::empty_extruded_surface(direction, 0.0);
                        let handle = self.app.add_surface_model(surface, body);
                        self.created(handle)
                    }
                    None => Value::Null,
                })
            }
            // A viewport in the layout on screen; model space has none.
            "Viewport" => one(match (vec3(&a[0]), vec3(&a[1])) {
                (Some(first), Some(second)) if self.scene().current_layout != "Model" => {
                    match crate::modules::layout::mview::MviewCommand::viewport_from_corners(first, second) {
                        Some(viewport) => self.create(EntityType::Viewport(Box::new(viewport))),
                        None => Value::Null,
                    }
                }
                _ => Value::Null,
            }),
            "Raster Image" | "Embedded Image" => {
                use crate::modules::draw::draw::raster_image::ImageCommand;
                let (path, width) = (text(&a[0]), num(&a[2]).unwrap_or(0.0));
                let Some(point) = vec3(&a[1]).filter(|_| width > 0.0 && !path.is_empty()) else {
                    return one(Value::Null);
                };
                let command = if spec.name == "Raster Image" {
                    match crate::io::image_file_size(std::path::PathBuf::from(&path)) {
                        Ok((_, w, h)) => ImageCommand::new(path, w, h),
                        Err(_) => return one(Value::Null),
                    }
                } else {
                    let name = std::path::Path::new(&path)
                        .file_name()
                        .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| crate::io::ole_embed::EmbeddedImage::from_bytes(name, bytes))
                    {
                        Ok(image) => ImageCommand::new_embedded(image),
                        Err(_) => return one(Value::Null),
                    }
                };
                one(self.drive(Box::new(command), None, &[point, point + DVec3::X * width]))
            }
            "PDF Underlay" => {
                let (path, page, scale) = (text(&a[0]), text(&a[1]), num(&a[3]).unwrap_or(1.0));
                let Some(point) = vec3(&a[2]).filter(|_| scale > 0.0 && !path.is_empty()) else {
                    return one(Value::Null);
                };
                let kind = document::entities::UnderlayType::Pdf;
                let pages = crate::modules::insert::pdf_attach::underlays_for_pages(
                    kind,
                    &[page],
                    point,
                    scale,
                    0.0,
                );
                let before: std::collections::HashSet<Handle> =
                    self.scene().document.entities().map(|e| e.common().handle).collect();
                let i = self.i;
                self.app.attach_pdf_pages(i, "PDFATTACH".into(), kind, &path, pages);
                let added: Vec<Handle> = self
                    .scene()
                    .document
                    .entities()
                    .map(|e| e.common().handle)
                    .filter(|h| !before.contains(h))
                    .collect();
                one(added.into_iter().fold(Value::Null, |_, h| self.created(h)))
            }
            "Ordinate Dimension" => one(match (vec3(&a[0]), vec3(&a[1])) {
                (Some(feature), Some(leader)) => self.drive(
                    Box::new(crate::modules::annotate::ordinate_dim::OrdinateDimCommand::new()),
                    None,
                    &[feature, leader],
                ),
                _ => Value::Null,
            }),
            "Arc Length Dimension" => one(match vec3(&a[1]) {
                Some(location) => self.drive(
                    Box::new(crate::modules::annotate::arc_length_dim::ArcLengthDimensionCommand::new()),
                    Some((&a[0], location)),
                    &[location],
                ),
                None => Value::Null,
            }),
            "Jogged Radius Dimension" => one(match (vec3(&a[1]), vec3(&a[2]), vec3(&a[3])) {
                (Some(center), Some(location), Some(jog)) => {
                    let defaults = crate::scene::creation_style::current_dimension_defaults(
                        &self.scene().document,
                    );
                    let multiplier = self.scene().creation_annotation_multiplier();
                    self.drive(
                        Box::new(
                            crate::modules::annotate::jogged_radius_dim::JoggedRadiusDimensionCommand::new(
                                defaults, multiplier,
                            ),
                        ),
                        Some((&a[0], location)),
                        &[center, location, jog],
                    )
                }
                _ => Value::Null,
            }),

            // ── Modify and Model: the commands, run with these picks ─────
            "Trim" | "Extend" => {
                let name = spec.name.to_uppercase();
                let steps = pick(&a[0], &a[1]).map(|p| vec![p, Enter]);
                one(self.run(&name, steps))
            }
            "Fillet" => {
                let steps = (|| {
                    Some(vec![
                        typed("R"),
                        typed(num(&a[4])?),
                        pick(&a[0], &a[1])?,
                        pick(&a[2], &a[3])?,
                    ])
                })();
                one(self.run("FILLET", steps))
            }
            "Chamfer" => {
                let steps = (|| {
                    Some(vec![
                        typed("D"),
                        typed(num(&a[4])?),
                        typed(num(&a[5])?),
                        pick(&a[0], &a[1])?,
                        pick(&a[2], &a[3])?,
                    ])
                })();
                one(self.run("CHAMFER", steps))
            }
            "Blend" => {
                let steps = (|| Some(vec![pick(&a[0], &a[1])?, pick(&a[2], &a[3])?]))();
                one(self.run("BLEND", steps))
            }
            "Break" => {
                let steps = (|| Some(vec![pick(&a[0], &a[1])?, Point(vec3(&a[2])?)]))();
                one(self.run("BREAK", steps))
            }
            "Break At Point" => {
                let steps = (|| Some(vec![pick(&a[0], &a[1])?, Point(vec3(&a[1])?)]))();
                one(self.run("BREAKATPOINT", steps))
            }
            "Lengthen" => {
                let steps = (|| Some(vec![typed("DE"), typed(num(&a[2])?), pick(&a[0], &a[1])?, Enter]))();
                one(self.run("LENGTHEN", steps))
            }
            "Join" => one(self.run("JOIN", selection(&a[0]).map(|s| vec![s, Enter]))),
            "Explode" => one(self.run("EXPLODE", selection(&a[0]).map(|s| vec![s, Enter]))),
            "Reverse" => one(self.run("REVERSE", selection(&a[0]).map(|s| vec![s, Enter]))),
            "Convert To Polyline" => {
                let steps = pick(&a[0], &a[1]).map(|p| vec![p, typed("Y"), Enter]);
                one(self.run("PEDIT", steps))
            }
            "Stretch" => {
                let steps = (|| {
                    Some(vec![
                        typed("C"),
                        Point(vec3(&a[0])?),
                        Point(vec3(&a[1])?),
                        Enter,
                        Point(vec3(&a[2])?),
                        Point(vec3(&a[3])?),
                    ])
                })();
                one(self.run("STRETCH", steps))
            }
            "Align" => {
                let steps = (|| {
                    Some(vec![
                        selection(&a[0])?,
                        Point(vec3(&a[1])?),
                        Point(vec3(&a[2])?),
                        Point(vec3(&a[3])?),
                        Point(vec3(&a[4])?),
                        Enter,
                        typed(if graph::value::boolean(&a[5]).unwrap_or(false) { "Y" } else { "N" }),
                    ])
                })();
                one(self.run("ALIGN", steps))
            }
            "Rectangular Array" => {
                let steps = (|| {
                    Some(vec![
                        selection(&a[0])?,
                        typed(num(&a[1])?.round()),
                        typed(num(&a[2])?.round()),
                        typed(num(&a[3])?),
                        typed(num(&a[4])?),
                    ])
                })();
                one(self.run("ARRAYRECT", steps))
            }
            "Polar Array" => {
                let steps = (|| {
                    Some(vec![
                        selection(&a[0])?,
                        Point(vec3(&a[1])?),
                        typed(num(&a[2])?.round()),
                        typed(num(&a[3])?),
                    ])
                })();
                one(self.run("ARRAYPOLAR", steps))
            }
            "Path Array" => {
                let steps = (|| {
                    Some(vec![
                        selection(&a[0])?,
                        pick_on(&a[1])?,
                        typed("DIVIDE"),
                        typed(num(&a[2])?.round()),
                    ])
                })();
                one(self.run("ARRAYPATH", steps))
            }
            "Boundary" => one(self.run("BOUNDARY", vec3(&a[0]).map(|p| vec![Point(p), Enter]))),
            // The current hatch settings; pattern and scale are rows of the
            // hatch it makes.
            "Hatch At Point" => one(self.run("HATCH", vec3(&a[0]).map(|p| vec![Point(p), Enter]))),
            "Hatch Objects" => {
                one(self.run("HATCH", selection(&a[0]).map(|s| vec![typed("S"), s, Enter])))
            }
            "Centerline" => {
                let steps = (|| Some(vec![pick(&a[0], &a[1])?, pick(&a[2], &a[3])?]))();
                one(self.run("CENTERLINE", steps))
            }
            "Center Mark" => one(self.run("CENTERMARK", pick(&a[0], &a[1]).map(|p| vec![p, Enter]))),
            "Union" | "Intersection" => {
                let name = if spec.name == "Union" { "UNION" } else { "INTERSECT" };
                one(self.run(name, selection(&a[0]).map(|s| vec![s])))
            }
            "Subtract" => {
                let steps = (|| Some(vec![selection(&a[0])?, selection(&a[1])?]))();
                one(self.run("SUBTRACT", steps))
            }
            "Revolve" => {
                let steps = (|| {
                    Some(vec![
                        selection(&a[0])?,
                        Point(vec3(&a[1])?),
                        Point(vec3(&a[2])?),
                        typed(num(&a[3])?),
                    ])
                })();
                one(self.run("REVOLVE", steps))
            }
            "Sweep" => {
                let steps = (|| Some(vec![selection(&a[0])?, pick_on(&a[1])?]))();
                one(self.run("SWEEP", steps))
            }
            // Sections are picked one by one, then cross sections only.
            "Loft" => {
                let picks: Option<Vec<_>> = list(&a[0]).iter().map(pick_on).collect();
                one(self.run(
                    "LOFT",
                    picks.filter(|p| p.len() >= 2).map(|mut p| {
                        p.extend([Enter, Enter]);
                        p
                    }),
                ))
            }
            "Extrude Along Path" => {
                let steps = (|| Some(vec![selection(&a[0])?, typed("P"), pick_on(&a[1])?]))();
                one(self.run("EXTRUDE", steps))
            }
            "Extrude Tapered" => {
                let steps = (|| {
                    Some(vec![selection(&a[0])?, typed("T"), typed(num(&a[2])?), typed(num(&a[1])?)])
                })();
                one(self.run("EXTRUDE", steps))
            }
            "Fillet Edge" | "Chamfer Edge" => {
                let (name, option) = if spec.name == "Fillet Edge" {
                    ("FILLETEDGE", "R")
                } else {
                    ("CHAMFEREDGE", "D")
                };
                let steps = (|| {
                    let h = handle(&a[0])?;
                    let at = vec3(&a[1])?;
                    let size = num(&a[2])?;
                    let mut steps = vec![typed(option), typed(size)];
                    if name == "CHAMFEREDGE" {
                        steps.push(typed(size));
                    }
                    steps.extend([EntityPick(h, at), Enter, Enter]);
                    Some(steps)
                })();
                one(self.run(name, steps))
            }
            "Shell" => {
                let steps = (|| {
                    Some(vec![
                        pick(&a[0], &a[1])?,
                        pick(&a[0], &a[1])?,
                        Enter,
                        typed(num(&a[2])?),
                    ])
                })();
                one(self.run("SHELL", steps))
            }
            "Slice" => {
                let steps = (|| {
                    Some(vec![
                        selection(&a[0])?,
                        typed("3"),
                        Point(vec3(&a[1])?),
                        Point(vec3(&a[2])?),
                        Point(vec3(&a[3])?),
                        Point(vec3(&a[4])?),
                    ])
                })();
                one(self.run("SLICE", steps))
            }
            "Thicken" => {
                let steps = (|| Some(vec![selection(&a[0])?, typed(num(&a[1])?)]))();
                one(self.run("THICKEN", steps))
            }
            "Presspull" => {
                let steps = (|| Some(vec![pick(&a[0], &a[1])?, typed(num(&a[2])?)]))();
                one(self.run("PRESSPULL", steps))
            }
            // About the axis through the two points, in place.
            "3D Rotate" => {
                let (Some(start), Some(end), Some(angle)) = (vec3(&a[1]), vec3(&a[2]), num(&a[3])) else {
                    return objects(Vec::new());
                };
                let Some(axis) = (end - start).try_normalize() else {
                    return objects(Vec::new());
                };
                let rotate = EntityTransform::Rotate {
                    center: start,
                    axis,
                    angle_rad: angle.to_radians(),
                };
                vec![Value::Array(
                    list(&a[0]).iter().map(|value| self.edit(value, rotate.clone())).collect(),
                )]
            }
            // A mirrored copy across the plane through the three points.
            "3D Mirror" => {
                let (Some(p1), Some(p2), Some(p3)) = (vec3(&a[1]), vec3(&a[2]), vec3(&a[3])) else {
                    return objects(Vec::new());
                };
                let along = (p2 - p1).normalize_or_zero();
                let across = (p3 - p1) - along * (p3 - p1).dot(along);
                let handles: Vec<Handle> = list(&a[0]).iter().filter_map(handle).collect();
                if along == DVec3::ZERO || across.length() < 1e-12 || handles.is_empty() {
                    return objects(Vec::new());
                }
                let mirror = EntityTransform::Mirror {
                    p1,
                    p2,
                    working_normal: across.normalize(),
                };
                let tab = &mut self.app.tabs[self.i];
                let copies = tab.scene.copy_entities(&handles, &mirror);
                tab.dirty |= !copies.is_empty();
                tab.graph.created.extend(&copies);
                objects(copies)
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
                    .map_or(Value::Null, |c| number(c.curve.chord_closed_area().unwrap_or(0.0).abs())),
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

    /// Runs the command `name` with `steps` as if typed, and keeps what it
    /// changed: changed and erased objects are put back and new ones erased
    /// on the next evaluation. The objects it made or changed come back;
    /// `None` steps (an input missing) run nothing.
    fn run(&mut self, name: &str, steps: Option<Vec<crate::command::StepInput>>) -> Value {
        let Some(steps) = steps else {
            return Value::Null;
        };
        // Arrays are plain copies here: an associative one would leave a
        // block definition behind on every evaluation.
        use crate::modules::draw::defaults;
        let associative = defaults::get_array_assoc();
        defaults::set_array_assoc(0.0);
        let result = self.run_command(name, steps);
        defaults::set_array_assoc(associative);
        result
    }

    fn run_command(&mut self, name: &str, steps: Vec<crate::command::StepInput>) -> Value {
        let i = self.i;
        let app = &mut *self.app;
        // The graph runs between commands; one the user has open stays put.
        let open = app.tabs[i].active_cmd.take();
        let selected = app.tabs[i].scene.selected_handles_in_order();
        app.tabs[i].scene.deselect_all();
        let before: rustc_hash::FxHashMap<Handle, std::sync::Arc<EntityType>> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let handle = entity.common().handle;
                Some((handle, app.tabs[i].scene.document.get_entity_arc(handle)?))
            })
            .collect();
        // Solids keep a construction history beside them; their records go
        // back with them.
        let records: Vec<(Handle, Vec<(Handle, document::objects::ObjectType)>)> = before
            .iter()
            .filter(|(_, entity)| {
                matches!(
                    entity.as_ref(),
                    EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Surface(_)
                )
            })
            .map(|(handle, _)| (*handle, app.tabs[i].scene.solid_history_objects(*handle)))
            .collect();
        self.tasks.push(app.run_command_line(name));
        for step in steps {
            if app.tabs[i].active_cmd.is_none() {
                break;
            }
            let selecting = matches!(step, crate::command::StepInput::SelectionComplete(_));
            self.tasks.push(app.feed_command(step));
            // A selection prompt keeps gathering until Enter.
            if selecting
                && app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.is_selection_gathering())
            {
                self.tasks.push(app.feed_command(crate::command::StepInput::Enter));
            }
        }
        for _ in 0..8 {
            if app.tabs[i].active_cmd.is_none() {
                break;
            }
            self.tasks.push(app.update(crate::app::Message::CommandEscape));
        }
        app.tabs[i].active_cmd = open;
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entities(&selected);
        let tab = &mut app.tabs[i];
        let (document, graph) = (&tab.scene.document, &mut tab.graph);
        let mut changed = Vec::new();
        for (handle, entity) in &before {
            match document.get_entity_arc(*handle) {
                None => graph.applied.push((*handle, (**entity).clone())),
                Some(now) if !std::sync::Arc::ptr_eq(&now, entity) => {
                    graph.applied.push((*handle, (**entity).clone()));
                    changed.push(*handle);
                }
                Some(_) => {}
            }
        }
        let made: Vec<Handle> = document
            .entities()
            .map(|entity| entity.common().handle)
            .filter(|handle| !before.contains_key(handle))
            .collect();
        graph.created.extend(&made);
        for (handle, history) in records {
            if !changed.contains(&handle) && document.get_entity(handle).is_some() {
                continue;
            }
            graph.applied_records.extend(history);
        }
        if !changed.is_empty() || !made.is_empty() {
            tab.dirty = true;
        }
        Value::Array(made.into_iter().chain(changed).map(object).collect())
    }

    /// A created handle, kept to be erased on the next step.
    fn created(&mut self, handle: Handle) -> Value {
        if handle.is_null() {
            return Value::Null;
        }
        let tab = &mut self.app.tabs[self.i];
        tab.graph.created.push(handle);
        tab.dirty = true;
        object(handle)
    }

    /// A solid from `body` with `history`, as the solid commands commit one.
    fn solid(&mut self, body: kernel::brep::Body, history: document::objects::SolidHistoryOperation) -> Value {
        let Some(sat) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
            return Value::Null;
        };
        let mut solid = document::entities::Solid3D::new();
        solid.set_sat_document(&sat);
        solid.wires = crate::scene::model::solid_model::edge_wires(&body);
        let handle = self.app.add_solid_model(EntityType::Solid3D(solid), body, history);
        self.created(handle)
    }

    /// A primitive solid whose history keeps its sizes editable.
    fn primitive(&mut self, history: document::objects::SolidHistoryOperation) -> Value {
        match kernel::acis::rebuild_body(&history) {
            Ok(body) => self.solid(body, history),
            Err(_) => Value::Null,
        }
    }

    /// Runs `command` as a user would, with a pick of `pick`'s object at its
    /// point and then `points`, and creates what it commits.
    fn drive(
        &mut self,
        mut command: Box<dyn crate::command::CadCommand>,
        pick: Option<(&Value, DVec3)>,
        points: &[DVec3],
    ) -> Value {
        use crate::command::CmdResult;
        if let Some((value, at)) = pick {
            let (Some(h), Some(entity)) = (handle(value), self.entity(value).cloned()) else {
                return Value::Null;
            };
            command.inject_picked_entity(entity);
            let _ = command.on_entity_pick(h, at);
        }
        for &point in points {
            match command.on_point(point) {
                CmdResult::NeedPoint => {}
                CmdResult::CommitDimension { entity, .. } | CmdResult::CommitAndExit(entity) => {
                    return self.create(entity)
                }
                _ => return Value::Null,
            }
        }
        Value::Null
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
        // A solid's history moves with it; its records go back with it too.
        let records = tab.scene.solid_history_objects(h);
        tab.graph.applied_records.extend(records);
        tab.scene.transform_entities(&[h], &t);
        tab.dirty = true;
        object(h)
    }
}
