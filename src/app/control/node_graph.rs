//! `node_graph` — drive the node graph: list the node library, read the graph
//! with every node's ports and values, and edit it (add, link, set, remove,
//! fold, save, load). Edits run through the same paths as the canvas, so each
//! is one undo step and re-evaluates the graph.

use super::*;
use crate::app::node_graph::ops;
use crate::ui::node_graph::{GraphMsg, NodeRow, ObjectKind, PaletteItem};
use graph::{Kind, NodeId, Port};

type Parsed<T> = Result<T, Value>;

fn node_id(req: &Value, key: &str) -> Parsed<NodeId> {
    req[key]
        .as_u64()
        .and_then(|id| NodeId::try_from(id).ok())
        .ok_or_else(|| failure("node_required", format!("Supply {key}: <node id>")))
}

fn port(req: &Value, key: &str) -> Parsed<Port> {
    let value = &req[key];
    let field = value["field"]
        .as_str()
        .ok_or_else(|| failure("port_required", format!("Supply {key}: {{\"node\":id,\"field\":name}}")))?;
    Ok(Port {
        node: node_id(value, "node")?,
        field: field.to_owned(),
    })
}

fn row_json(row: &NodeRow, shown: bool, linked: bool) -> Value {
    json!({
        "field": row.field,
        "label": row.label,
        "value": row.value,
        "input": row.input,
        "output": row.output,
        "linked": linked,
        "group": row.group,
        "shown": shown,
    })
}

fn spec_json(spec: &graph::Spec) -> Value {
    json!({
        "type": spec.name,
        "kind": "operation",
        "category": spec.category,
        "inputs": spec.inputs.iter().map(|input| json!({
            "name": input.name,
            "default": input.default.value(),
            "list": input.list,
        })).collect::<Vec<_>>(),
        "outputs": spec.outputs,
    })
}

impl OpenCADStudio {
    pub(super) fn control_node_graph(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let action = req["action"]
            .as_str()
            .unwrap_or("state")
            .trim()
            .to_ascii_lowercase();
        let i = self.active_tab;
        let mut added = None;
        let task = match action.as_str() {
            "library" => {
                self.set_control_result(self.node_graph_library());
                return Ok(Task::none());
            }
            "state" => Task::none(),
            "show" | "hide" => {
                if self.show_node_graph != (action == "show") {
                    self.on_graph(GraphMsg::Toggle)
                } else {
                    Task::none()
                }
            }
            "add" => {
                let name = req["type"]
                    .as_str()
                    .ok_or_else(|| failure("type_required", "Supply type: a node type from action library"))?;
                // A name can be both (Point): `kind` picks one.
                let object = ObjectKind::from_name(name).map(PaletteItem::Object);
                let operation = ops::resolve(name).map(PaletteItem::Op);
                let item = match (req["kind"].as_str().map(str::to_ascii_lowercase).as_deref(), object, operation) {
                    (Some("object"), Some(item), _) | (Some("operation"), _, Some(item)) => item,
                    (None, Some(_), Some(_)) => {
                        return Err(failure(
                            "ambiguous_node_type",
                            format!("{name:?} is both an object and an operation; add kind: object or operation"),
                        ))
                    }
                    (None, Some(item), None) | (None, None, Some(item)) => item,
                    _ => {
                        return Err(failure(
                            "unknown_node_type",
                            format!("No node type {name:?}; see action library"),
                        ))
                    }
                };
                let graph = &mut self.tabs[i].graph;
                let position = match req["position"].as_array() {
                    Some(xy) => iced::Point::new(
                        xy.first().and_then(Value::as_f64).unwrap_or(0.0) as f32,
                        xy.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    ),
                    None => graph.drop_position(false),
                };
                added = Some(graph.add_node(item, Vec::new(), position));
                self.graph_run(i)
            }
            "remove" => {
                let id = self.existing_node(i, req, "node")?;
                self.on_graph(GraphMsg::NodeDelete(id))
            }
            "connect" => {
                let (from, to) = (port(req, "from")?, port(req, "to")?);
                self.require_port(i, &from, false)?;
                self.require_port(i, &to, true)?;
                if !self.graph_connect(i, from, to) {
                    return Err(failure("graph_cycle", "The link would close a loop"));
                }
                self.graph_run(i)
            }
            "disconnect" => {
                let to = port(req, "to")?;
                if self.tabs[i].graph.engine.disconnect(&to).is_none() {
                    return Err(failure("not_linked", "Nothing is linked into that input"));
                }
                self.graph_run(i)
            }
            "set" => {
                let id = self.existing_node(i, req, "node")?;
                let field = req["field"]
                    .as_str()
                    .ok_or_else(|| failure("field_required", "Supply field: an input port"))?;
                let target = Port {
                    node: id,
                    field: field.to_owned(),
                };
                self.require_port(i, &target, true)?;
                self.on_graph(GraphMsg::Set(target, req["value"].clone()))
            }
            "toggle" => {
                // A section folds by its title, a coordinate group by its
                // field; a section's port field names its section too.
                let id = self.existing_node(i, req, "node")?;
                let field = req["field"]
                    .as_str()
                    .ok_or_else(|| failure("field_required", "Supply field: a section port or group field"))?;
                let sections = self.graph_all_sections(i);
                let index = self.tabs[i]
                    .graph
                    .engine
                    .nodes
                    .iter()
                    .position(|node| node.id == id)
                    .unwrap_or_default();
                let key = sections[index]
                    .iter()
                    .find_map(|section| {
                        if section.title == field
                            || section.header.as_ref().is_some_and(|header| header.field == field)
                        {
                            return Some(section.title.clone());
                        }
                        section
                            .rows
                            .iter()
                            .find(|row| row.field == field && row.group.is_none() && row.field.contains('|'))
                            .map(|row| row.field.clone())
                    })
                    .ok_or_else(|| failure("unknown_port", format!("Node {id} has no section or group {field:?}")))?;
                self.on_graph(GraphMsg::Section(id, key))
            }
            "clear" => self.on_graph(GraphMsg::New),
            "run" => self.graph_run(i),
            #[cfg(not(target_arch = "wasm32"))]
            "save" => {
                let path = req["path"]
                    .as_str()
                    .ok_or_else(|| failure("path_required", "Supply path: where to write the .ocg file"))?;
                std::fs::write(path, self.graph_file_bytes(i))
                    .map_err(|error| failure("write_failed", error))?;
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            "load" => {
                let path = req["path"]
                    .as_str()
                    .ok_or_else(|| failure("path_required", "Supply path: the .ocg file to read"))?;
                let bytes = std::fs::read(path).map_err(|error| failure("read_failed", error))?;
                crate::ui::node_graph::Graph::from_file(&bytes)
                    .map_err(|error| failure("invalid_graph", error))?;
                self.on_graph(GraphMsg::Loaded(Some(bytes)))
            }
            other => {
                return Err(failure(
                    "invalid_action",
                    format!(
                        "Unknown node_graph action {other:?}. Use library, state, show, hide, add, remove, connect, disconnect, set, toggle, clear, run, save or load"
                    ),
                ))
            }
        };
        let mut state = self.node_graph_state(i);
        if let Some(id) = added {
            state["node"] = json!(id);
        }
        self.set_control_result(state);
        Ok(task)
    }

    fn existing_node(&self, i: usize, req: &Value, key: &str) -> Parsed<NodeId> {
        let id = node_id(req, key)?;
        match self.tabs[i].graph.engine.node(id) {
            Some(_) => Ok(id),
            None => Err(failure("unknown_node", format!("No node {id}"))),
        }
    }

    /// `port` must be an input (or output) of its node as the graph shows it.
    fn require_port(&self, i: usize, port: &Port, input: bool) -> Parsed<()> {
        let index = self.tabs[i]
            .graph
            .engine
            .nodes
            .iter()
            .position(|node| node.id == port.node)
            .ok_or_else(|| failure("unknown_node", format!("No node {}", port.node)))?;
        let sections = self.graph_all_sections(i);
        let found = sections[index]
            .iter()
            .flat_map(|section| section.header.iter().chain(&section.rows))
            .any(|row| row.field == port.field && if input { row.input } else { row.output });
        if found {
            Ok(())
        } else {
            let side = if input { "input" } else { "output" };
            Err(failure(
                "unknown_port",
                format!("Node {} has no {side} {:?}", port.node, port.field),
            ))
        }
    }

    /// Every node type: the drawing objects, then the operations.
    fn node_graph_library(&self) -> Value {
        let objects: Vec<Value> = crate::ui::node_graph::object_kinds()
            .iter()
            .map(|kind| {
                json!({
                    "type": kind.name(),
                    "kind": "object",
                    "category": "Objects",
                    "creates": kind.owns(),
                })
            })
            .collect();
        let operations: Vec<Value> = ops::LIBRARY
            .iter()
            .chain(graph::LIBRARY)
            .map(spec_json)
            .collect();
        json!({ "objects": objects, "operations": operations })
    }

    /// The graph as the canvas shows it: each node's ports with their values,
    /// which sections and groups are open, and the links.
    fn node_graph_state(&self, i: usize) -> Value {
        let sections = self.graph_all_sections(i);
        let graph = &self.tabs[i].graph;
        let linked = |port: &Port, input: bool| {
            graph.engine.links.iter().any(|link| {
                if input {
                    &link.to == port
                } else {
                    &link.from == port
                }
            })
        };
        let nodes: Vec<Value> = graph
            .engine
            .nodes
            .iter()
            .zip(&sections)
            .map(|(node, node_sections)| {
                let ui = graph.ui.get(&node.id);
                let open = |key: &str| ui.is_some_and(|ui| ui.open.contains(key));
                let (kind, category, family) = match &node.kind {
                    Kind::Op(spec) => (spec.name.to_owned(), spec.category, "operation"),
                    Kind::Object(name) => (name.clone(), "Objects", "object"),
                };
                let row = |row: &NodeRow, section_open: bool| {
                    let port = Port {
                        node: node.id,
                        field: row.field.clone(),
                    };
                    let shown = section_open && row.group.as_deref().is_none_or(open);
                    row_json(row, shown, linked(&port, true) || linked(&port, false))
                };
                json!({
                    "id": node.id,
                    "type": kind,
                    "kind": family,
                    "category": category,
                    "position": ui.map(|ui| [ui.pos.x, ui.pos.y]),
                    "handles": ui.map(|ui| ui.handles.iter().map(|h| format!("{:X}", h.value())).collect::<Vec<_>>()),
                    "params": node.params,
                    "outputs": node.outputs,
                    "sections": node_sections.iter().map(|section| {
                        let section_open = section.title.is_empty() || open(&section.title);
                        json!({
                            "title": section.title,
                            "open": section_open,
                            "port": section.header.as_ref().map(|header| row(header, true)),
                            "rows": section.rows.iter().map(|r| row(r, section_open)).collect::<Vec<_>>(),
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        let port = |port: &Port| json!({ "node": port.node, "field": port.field });
        json!({
            "visible": self.show_node_graph,
            "nodes": nodes,
            "links": graph.engine.links.iter().map(|link| json!({
                "from": port(&link.from),
                "to": port(&link.to),
            })).collect::<Vec<_>>(),
            "applied": graph.applied.len(),
            "created": graph.created.iter().map(|h| format!("{:X}", h.value())).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod shape_node_tests {
    use serde_json::{json, Value};

    /// The polyline each drawing-shape node makes, from `inputs`.
    fn made(node: &str, inputs: Value) -> codec::entities::LwPolyline {
        let mut app = crate::app::OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let spec = super::ops::resolve(node).expect("node type");
        let id = app.tabs[i].graph.add_node(super::PaletteItem::Op(spec), Vec::new(), iced::Point::ORIGIN);
        for (field, value) in inputs.as_object().unwrap() {
            let port = super::Port { node: id, field: field.clone() };
            let _ = app.on_graph(super::GraphMsg::Set(port, value.clone()));
        }
        let i = app.active_tab;
        let made = app.tabs[i].scene.document.entities().find_map(|entity| match entity {
            codec::EntityType::LwPolyline(polyline) => Some(polyline.clone()),
            _ => None,
        });
        made.unwrap_or_else(|| panic!("{node} made no polyline"))
    }

    /// #2589: Rectangle, Polygon, Donut and Revision Cloud nodes.
    #[test]
    fn the_shape_nodes_draw_their_polylines() {
        let corners = |p: &codec::entities::LwPolyline| -> Vec<(f64, f64)> {
            p.vertices.iter().map(|v| (v.location.x, v.location.y)).collect()
        };
        let rectangle = made("Rectangle", json!({"corner": {"x": 1, "y": 2, "z": 3}, "width": 4, "height": 2}));
        assert_eq!(corners(&rectangle), [(1.0, 2.0), (5.0, 2.0), (5.0, 4.0), (1.0, 4.0)]);
        assert_eq!(rectangle.elevation, 3.0);

        let square = made("Polygon", json!({"center": {"x": 0, "y": 0}, "radius": 1, "sides": 4, "circumscribed": true}));
        for (x, y) in corners(&square) {
            assert!((x.abs() - 1.0).abs() < 1e-9 && (y.abs() - 1.0).abs() < 1e-9, "{x},{y}");
        }

        let donut = made("Donut", json!({"center": {"x": 0, "y": 0}, "inside": 2, "outside": 4}));
        assert_eq!(donut.constant_width, 1.0);

        let cloud = made("Revision Cloud", json!({"points": [{"x": 0, "y": 0}, {"x": 10, "y": 0}, {"x": 10, "y": 10}], "arc length": 1}));
        assert!(cloud.vertices.len() > 10 && cloud.vertices.iter().any(|v| v.bulge != 0.0));
    }
}

#[cfg(test)]
mod radial_dimension_node_tests {
    use serde_json::json;

    /// #2591: Radius and Diameter Dimension nodes dimension a linked circle.
    #[test]
    fn radial_dimension_nodes_measure_the_linked_circle() {
        for (node, measured) in [("Radius Dimension", 5.0), ("Diameter Dimension", 10.0)] {
            let mut app = crate::app::OpenCADStudio::new_for_test();
            let _ = app.automation_op(r#"{"op":"new"}"#);
            let _ = app.run_command_line("CIRCLE 0,0 5");
            let i = app.active_tab;
            let circle = app.tabs[i].scene.document.entities().next().unwrap().common().handle;
            let spec = super::ops::resolve(node).expect("node type");
            let id = app.tabs[i].graph.add_node(super::PaletteItem::Op(spec), Vec::new(), iced::Point::ORIGIN);
            for (field, value) in [
                ("object", json!({"handle": circle.value()})),
                ("location", json!({"x": 8, "y": 0})),
            ] {
                let port = super::Port { node: id, field: field.into() };
                let _ = app.on_graph(super::GraphMsg::Set(port, value));
            }
            let dimension = app.tabs[i].scene.document.entities().find_map(|entity| match entity {
                codec::EntityType::Dimension(dimension) => Some(dimension.base().actual_measurement),
                _ => None,
            });
            assert_eq!(dimension, Some(measured), "{node}");
        }
    }
}

#[cfg(test)]
mod object_node_tests {
    use crate::ui::node_graph::ObjectKind;

    /// #2590: Hatch, Multiline, Multileader and Helix nodes make their object.
    #[test]
    fn the_new_object_nodes_make_their_objects() {
        for kind in [ObjectKind::Hatch, ObjectKind::MLine, ObjectKind::MultiLeader, ObjectKind::Helix] {
            let mut app = crate::app::OpenCADStudio::new_for_test();
            let _ = app.automation_op(r#"{"op":"new"}"#);
            let i = app.active_tab;
            app.tabs[i].graph.add_node(super::PaletteItem::Object(kind), Vec::new(), iced::Point::ORIGIN);
            let _ = app.graph_run(i);
            let made = app.tabs[i].scene.document.entities().any(|entity| kind.matches(entity));
            assert!(made, "{} made nothing", kind.name());
        }
    }
}
