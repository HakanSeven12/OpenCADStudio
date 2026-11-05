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
use crate::scene::model::solid_history;
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
    let mut polyline = codec::entities::LwPolyline::new();
    polyline.is_closed = true;
    polyline.elevation = z;
    polyline.vertices = vertices
        .iter()
        .map(|p| codec::entities::LwVertex::new(codec::types::Vector2::new(p[0], p[1])))
        .collect();
    EntityType::LwPolyline(polyline)
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
                        codec::types::Vector3::new(location.x, location.y, location.z),
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
                        let mut region = codec::entities::Region::new();
                        region.point_of_reference =
                            codec::types::Vector3::new(origin[0], origin[1], origin[2]);
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
                let kind = codec::entities::UnderlayType::Pdf;
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
    fn solid(&mut self, body: kernel::brep::Body, history: codec::objects::SolidHistoryOperation) -> Value {
        let Some(sat) = crate::scene::convert::acis_export::solid_to_sat(&body) else {
            return Value::Null;
        };
        let mut solid = codec::entities::Solid3D::new();
        solid.set_sat_document(&sat);
        solid.wires = crate::scene::model::solid_model::edge_wires(&body);
        let handle = self.app.add_solid_model(EntityType::Solid3D(solid), body, history);
        self.created(handle)
    }

    /// A primitive solid whose history keeps its sizes editable.
    fn primitive(&mut self, history: codec::objects::SolidHistoryOperation) -> Value {
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
        tab.scene.transform_entities(&[h], &t);
        tab.dirty = true;
        object(h)
    }
}
