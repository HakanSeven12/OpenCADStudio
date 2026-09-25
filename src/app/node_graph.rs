//! Node graph handling. An object node's rows are the Properties panel's own
//! page for its entity, and every write goes through the panel's handlers (via
//! the control protocol's `set_property_value`) aimed at that entity. The
//! `graph` crate evaluates; [`AppHost`] gives it the drawing.

use super::{Message, OpenCADStudio};
use crate::scene::model::object::PropSection;
use crate::ui::node_graph::{Drag, GraphMsg, NodeRow, NodeSection, ObjectKind, RowWidget};
use codec::Handle;
use graph::{Kind, NodeId, Port};
use iced::Task;
use rustc_hash::FxHashMap as HashMap;
use serde_json::{json, Value};

impl OpenCADStudio {
    /// The Properties panel's single-object page for `handle`.
    fn graph_panel_sections(&self, i: usize, handle: Handle) -> Vec<PropSection> {
        let tab = &self.tabs[i];
        let Some(entity) = tab.scene.document.get_entity(handle) else {
            return Vec::new();
        };
        crate::entities::common::set_unit_context(crate::entities::common::UnitContext::from_header(
            &tab.scene.document.header,
        ));
        crate::entities::common::set_fixed_text_heights(&tab.scene.document);
        let text_style_names: Vec<String> = tab
            .scene
            .document
            .text_styles
            .iter()
            .map(|style| style.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        self.entity_property_sections(
            i,
            handle,
            entity,
            &text_style_names,
            tab.scene.displayed_annotation_scale_handle(),
            0,
        )
        .0
    }

    /// Every row of `handle`'s panel page as `(field, protocol value)`.
    fn graph_fields(&self, i: usize, handle: Handle) -> Vec<(String, Value)> {
        self.graph_panel_sections(i, handle)
            .iter()
            .flat_map(|section| &section.props)
            .map(|property| {
                let json = crate::app::control::property_json(property);
                (property.field.to_owned(), json["value"].clone())
            })
            .collect()
    }

    /// An object node's rows: its object handle(s), then the panel page of its
    /// first entity. With several entities the values shown are the lists.
    fn graph_object_sections(&self, i: usize, id: NodeId) -> Vec<NodeSection> {
        let graph = &self.tabs[i].graph;
        let Some(&first) = graph.ui.get(&id).and_then(|ui| ui.handles.first()) else {
            return Vec::new();
        };
        let outputs = graph.engine.node(id).map(|node| &node.outputs);
        let listed = graph.ui.get(&id).is_some_and(|ui| ui.handles.len() > 1);
        let value = |field: &str, own: Value| match outputs.and_then(|outputs| outputs.get(field)) {
            Some(value) if listed => value.clone(),
            _ => own,
        };
        let panel = self.graph_panel_sections(i, first);
        if panel.is_empty() {
            return Vec::new();
        }
        let object = NodeRow {
            field: "object".to_owned(),
            label: crate::t!("Object").into_owned(),
            value: value("object", json!({ "handle": first.value() })),
            input: false,
            output: true,
            widget: RowWidget::Field,
        };
        std::iter::once(NodeSection { title: String::new(), rows: vec![object] })
            .chain(panel.into_iter().map(|section| NodeSection {
                title: section.title,
                rows: section
                    .props
                    .iter()
                    .map(|property| {
                        let json = crate::app::control::property_json(property);
                        NodeRow {
                            field: property.field.to_owned(),
                            label: property.label.clone(),
                            input: !matches!(
                                json["kind"].as_str(),
                                Some("readonly" | "specialized")
                            ),
                            output: true,
                            value: value(property.field, json["value"].clone()),
                            widget: RowWidget::Field,
                        }
                    })
                    .collect(),
            }))
            .collect()
    }

    /// Every node's rows, parallel to the graph's node list.
    pub(super) fn graph_all_sections(&self, i: usize) -> Vec<Vec<NodeSection>> {
        let graph = &self.tabs[i].graph;
        let objects: HashMap<NodeId, Vec<NodeSection>> = graph
            .engine
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, Kind::Object(_)))
            .map(|node| (node.id, self.graph_object_sections(i, node.id)))
            .collect();
        graph.sections(objects)
    }

    /// Write one Properties row of `handle` through the panel's handlers.
    fn graph_write(&mut self, i: usize, handle: Handle, field: &str, value: &Value) -> Task<Message> {
        // An empty target falls back to the drawing defaults in some handlers.
        if self.tabs[i].scene.is_layer_locked(handle) {
            return Task::none();
        }
        let Some(property) = self
            .graph_panel_sections(i, handle)
            .into_iter()
            .flat_map(|section| section.props)
            .find(|property| property.field == field)
        else {
            return Task::none();
        };
        self.property_target_override = Some(handle);
        let result = self.set_property_value(property, value);
        self.property_target_override = None;
        result.unwrap_or_else(|error| {
            self.command_line
                .push_error(error["error"].as_str().unwrap_or_default());
            Task::none()
        })
    }

    /// Runs `edit`, then re-evaluates the graph, all as one undo step.
    fn graph_step(
        &mut self,
        i: usize,
        edit: impl FnOnce(&mut Self) -> Vec<Task<Message>>,
    ) -> Task<Message> {
        self.push_undo_snapshot(i, "GRAPH");
        self.graph_undo_open = true;
        let mut tasks = edit(self);
        let mut engine = std::mem::take(&mut self.tabs[i].graph.engine);
        let mut host = AppHost { app: self, i, tasks: Vec::new() };
        engine.evaluate(&mut host);
        tasks.append(&mut host.tasks);
        self.tabs[i].graph.engine = engine;
        self.graph_undo_open = false;
        Task::batch(tasks)
    }

    fn graph_run(&mut self, i: usize) -> Task<Message> {
        self.graph_step(i, |_| Vec::new())
    }

    pub(super) fn on_graph(&mut self, message: GraphMsg) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].graph.update_ui(&message);
        match message {
            GraphMsg::Toggle => {
                self.show_node_graph = !self.show_node_graph;
                if self.show_node_graph {
                    return self.graph_run(i);
                }
            }
            GraphMsg::Released => {
                let sections = self.graph_all_sections(i);
                match self.tabs[i].graph.drag.take() {
                    Some(Drag::Palette(item)) => {
                        let pos = self.tabs[i].graph.drop_position();
                        self.tabs[i].graph.add_node(item, Vec::new(), pos);
                        return self.graph_run(i);
                    }
                    Some(Drag::Link(from)) => {
                        if let Some(to) = self.tabs[i].graph.hit_input(&sections) {
                            self.tabs[i].graph.engine.connect(from, to);
                        }
                        // A link dropped on nothing was detached from an input.
                        return self.graph_run(i);
                    }
                    _ => {}
                }
            }
            GraphMsg::NodeDelete(id) => {
                let graph = &mut self.tabs[i].graph;
                graph.engine.remove(id);
                graph.drafts.retain(|port, _| port.node != id);
                let handles = graph.ui.remove(&id).map(|ui| ui.handles).unwrap_or_default();
                return self.graph_step(i, |app| {
                    let live: Vec<Handle> = handles
                        .into_iter()
                        .filter(|handle| app.tabs[i].scene.document.get_entity(*handle).is_some())
                        .collect();
                    if !live.is_empty() {
                        app.tabs[i].scene.erase_entities(&live);
                        app.tabs[i].dirty = true;
                        app.refresh_properties();
                    }
                    Vec::new()
                });
            }
            GraphMsg::Commit(port) => {
                let Some(raw) = self.tabs[i].graph.drafts.remove(&port) else {
                    return Task::none();
                };
                // Typed text is protocol JSON when it parses (numbers, true,
                // {"rgb":[..]}), otherwise a plain string.
                let value = serde_json::from_str(raw.trim()).unwrap_or(Value::String(raw));
                return self.graph_set(i, port, value);
            }
            GraphMsg::Set(port, value) => return self.graph_set(i, port, value),
            _ => {}
        }
        Task::none()
    }

    /// An edited input: an operation keeps it as a parameter; an object node
    /// writes it to every entity it owns.
    fn graph_set(&mut self, i: usize, port: Port, value: Value) -> Task<Message> {
        let graph = &mut self.tabs[i].graph;
        let Some(node) = graph.engine.node_mut(port.node) else {
            return Task::none();
        };
        if let Kind::Op(_) = node.kind {
            node.params.insert(port.field, value);
            return self.graph_run(i);
        }
        let handles = graph.ui.get(&port.node).map(|ui| ui.handles.clone()).unwrap_or_default();
        self.graph_step(i, |app| {
            handles
                .into_iter()
                .map(|handle| app.graph_write(i, handle, &port.field, &value))
                .collect()
        })
    }
}

/// The drawing as the graph sees it.
struct AppHost<'a> {
    app: &'a mut OpenCADStudio,
    i: usize,
    tasks: Vec<Task<Message>>,
}

impl graph::Host for AppHost<'_> {
    fn objects(
        &mut self,
        node: NodeId,
        kind: &str,
        instances: &[Vec<(String, Value)>],
    ) -> Vec<Vec<(String, Value)>> {
        let (app, i) = (&mut *self.app, self.i);
        let Some(kind) = ObjectKind::from_name(kind) else {
            return Vec::new();
        };
        let mut handles: Vec<Handle> = app.tabs[i]
            .graph
            .ui
            .get(&node)
            .map(|ui| ui.handles.clone())
            .unwrap_or_default();
        handles.retain(|handle| app.tabs[i].scene.document.get_entity(*handle).is_some());
        // New entities copy the node's first one, so edits made by hand to
        // unlinked rows carry over to every element.
        while handles.len() < instances.len() {
            let mut entity = handles
                .first()
                .and_then(|handle| app.tabs[i].scene.document.get_entity(*handle))
                .cloned()
                .unwrap_or_else(|| kind.entity());
            entity.common_mut().handle = Handle::NULL;
            let Some(handle) = app.commit_entity_handle(entity) else {
                break;
            };
            handles.push(handle);
            app.tabs[i].dirty = true;
        }
        if handles.len() > instances.len() {
            let extra = handles.split_off(instances.len());
            app.tabs[i].scene.erase_entities(&extra);
            app.tabs[i].dirty = true;
        }
        if let Some(ui) = app.tabs[i].graph.ui.get_mut(&node) {
            ui.handles = handles.clone();
        }
        for (handle, fields) in handles.iter().zip(instances) {
            for (field, value) in fields {
                let current = app
                    .graph_fields(i, *handle)
                    .into_iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, value)| value);
                if current.as_ref() != Some(value) {
                    let task = app.graph_write(i, *handle, field, value);
                    self.tasks.push(task);
                }
            }
        }
        handles
            .iter()
            .map(|handle| {
                let mut fields = app.graph_fields(i, *handle);
                fields.push(("object".to_owned(), json!({ "handle": handle.value() })));
                fields
            })
            .collect()
    }

    fn curve(&self, object: &Value) -> Option<kernel::space::PlanarCurve> {
        let handle = Handle::new(object.get("handle")?.as_u64()?);
        let entity = self.app.tabs[self.i].scene.document.get_entity(handle)?;
        crate::entities::curve::entity_curve(entity)
    }

    fn expression(&self, text: &str) -> Option<f64> {
        crate::app::expr_eval::eval_number(text)
    }
}
