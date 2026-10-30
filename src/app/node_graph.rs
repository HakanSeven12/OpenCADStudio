//! Node graph handling. An object node's rows are the Properties panel's own
//! page for its entity, and every write goes through the panel's handlers (via
//! the control protocol's `set_property_value`) aimed at that entity. The
//! `graph` crate evaluates; [`AppHost`] gives it the drawing and the
//! application's own nodes ([`ops`]).
//!
//! Besides its rows, an object node has a port on each section header,
//! carrying the whole section as `{field: value}`, and on each coordinate
//! group the panel folds ("Start X/Y/Z"), carrying a point. Linking two
//! headers or two groups copies every value across.

pub(crate) mod ops;

use super::{Message, OpenCADStudio};
use crate::scene::model::object::PropSection;
use crate::ui::node_graph::{Drag, GraphMsg, NodeRow, NodeSection, ObjectKind, RowWidget};
use codec::Handle;
use graph::{Kind, NodeId, Port};
use iced::Task;
use rustc_hash::FxHashMap as HashMap;
use serde_json::{json, Value};

/// Field-key prefix of a section header's port. The key goes on with the
/// section's first field, which does not change with the interface language.
const SECTION: &str = "section:";

/// A coordinate group's components (its field joins them with `|`).
fn components(field: &str) -> Option<std::str::Split<'_, char>> {
    field.contains('|').then(|| field.split('|'))
}

/// A point from a point value or from typed text such as `(1, 2, 3)`.
fn typed_point(value: &Value) -> Option<[f64; 3]> {
    graph::value::point(value).or_else(|| {
        let numbers: Option<Vec<f64>> = value
            .as_str()?
            .trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace())
            .split(',')
            .map(|part| crate::app::expr_eval::eval_number(part.trim()))
            .collect();
        match numbers?.as_slice() {
            [x, y] => Some([*x, *y, 0.0]),
            [x, y, z] => Some([*x, *y, *z]),
            _ => None,
        }
    })
}

/// A panel value as a number: plain, numeric, or as the panel shows it.
fn row_number(value: &Value) -> Option<f64> {
    graph::value::num(value)
        .or_else(|| super::block_parameter_properties::parse_value(value.as_str()?))
}

impl OpenCADStudio {
    /// The Properties panel's single-object page for `handle`.
    fn graph_panel_sections(&self, i: usize, handle: Handle) -> Vec<PropSection> {
        let tab = &self.tabs[i];
        let Some(entity) = tab.scene.document.get_entity(handle) else {
            return Vec::new();
        };
        crate::entities::common::set_unit_context(
            crate::entities::common::UnitContext::from_header(&tab.scene.document.header),
        );
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

    /// `handle`'s panel page as node sections: a header port per section,
    /// and coordinate rows folded under a group row.
    fn graph_entity_sections(&self, i: usize, handle: Handle) -> Vec<NodeSection> {
        use crate::ui::properties::{coord_base, coord_component, coord_group_len};
        self.graph_panel_sections(i, handle)
            .into_iter()
            .map(|section| {
                let mut rows = Vec::new();
                let props = &section.props;
                let row = |index: usize, label: &str, group: Option<String>| {
                    let property = &props[index];
                    let json = crate::app::control::property_json(property);
                    NodeRow {
                        field: crate::app::control::row_id(property),
                        label: label.to_owned(),
                        input: !matches!(json["kind"].as_str(), Some("readonly" | "specialized")),
                        output: true,
                        value: json["value"].clone(),
                        widget: RowWidget::Field,
                        group,
                    }
                };
                let mut index = 0;
                while index < props.len() {
                    let len = coord_group_len(props, index);
                    if len < 2 {
                        rows.push(row(index, &props[index].label, None));
                        index += 1;
                        continue;
                    }
                    let members: Vec<NodeRow> = (index..index + len)
                        .map(|k| row(k, coord_component(&props[k].label).trim(), None))
                        .collect();
                    let field = members
                        .iter()
                        .map(|member| member.field.as_str())
                        .collect::<Vec<_>>()
                        .join("|");
                    let mut p = [0.0; 3];
                    let mut complete = true;
                    for (axis, member) in members.iter().enumerate() {
                        match row_number(&member.value) {
                            Some(x) => p[axis] = x,
                            None => complete = false,
                        }
                    }
                    rows.push(NodeRow {
                        field: field.clone(),
                        label: coord_base(&props[index].label).to_owned(),
                        value: if complete {
                            graph::value::point_value(p)
                        } else {
                            Value::Null
                        },
                        input: members.iter().all(|member| member.input),
                        output: true,
                        widget: RowWidget::Group,
                        group: None,
                    });
                    rows.extend(members.into_iter().map(|member| NodeRow {
                        group: Some(field.clone()),
                        ..member
                    }));
                    index += len;
                }
                let plain = || rows.iter().filter(|row| !matches!(row.widget, RowWidget::Group));
                let header = NodeRow {
                    field: format!("{SECTION}{}", props.first().map_or("", |p| p.field)),
                    label: section.title.clone(),
                    value: Value::Object(
                        plain()
                            .map(|row| (row.field.clone(), row.value.clone()))
                            .collect(),
                    ),
                    input: plain().any(|row| row.input),
                    output: true,
                    widget: RowWidget::Field,
                    group: None,
                };
                NodeSection {
                    title: section.title,
                    header: Some(header),
                    rows,
                }
            })
            .collect()
    }

    /// Every port of `handle`'s panel page as `(field, value)`: headers,
    /// groups and rows.
    fn graph_fields(&self, i: usize, handle: Handle) -> Vec<(String, Value)> {
        self.graph_entity_sections(i, handle)
            .into_iter()
            .flat_map(|section| section.header.into_iter().chain(section.rows))
            .map(|row| (row.field, row.value))
            .collect()
    }

    /// The port of `handle` called `name` by field or by label, as
    /// `(field, value)`.
    pub(super) fn graph_field(&self, i: usize, handle: Handle, name: &str) -> Option<(String, Value)> {
        let name = name.trim();
        self.graph_entity_sections(i, handle)
            .into_iter()
            .flat_map(|section| section.header.into_iter().chain(section.rows))
            .find(|row| row.field == name || row.label.eq_ignore_ascii_case(name))
            .map(|row| (row.field, row.value))
    }

    /// An object node's rows: its object handle(s), then the panel page of its
    /// first entity. With several entities the values shown are the lists.
    fn graph_object_sections(&self, i: usize, id: NodeId, kind: Option<ObjectKind>) -> Vec<NodeSection> {
        let graph = &self.tabs[i].graph;
        let first = graph.ui.get(&id).and_then(|ui| ui.handles.first()).copied();
        let outputs = graph.engine.node(id).map(|node| &node.outputs);
        let listed = graph.ui.get(&id).is_some_and(|ui| ui.handles.len() > 1);
        let value = |field: &str, own: Value| match outputs.and_then(|outputs| outputs.get(field)) {
            Some(value) if listed => value.clone(),
            _ => own,
        };
        // An `Object` node takes its object in; the others make their own.
        let input = kind.is_some_and(|kind| !kind.owns());
        let panel = first.map_or_else(Vec::new, |first| self.graph_entity_sections(i, first));
        if panel.is_empty() && !input {
            return Vec::new();
        }
        let object = NodeRow {
            field: "object".to_owned(),
            label: crate::t!("Object").into_owned(),
            value: value(
                "object",
                first.map_or(Value::Null, |first| json!({ "handle": first.value() })),
            ),
            input,
            output: true,
            widget: RowWidget::Field,
            group: None,
        };
        let listed_row = |row: NodeRow| NodeRow {
            value: value(&row.field, row.value),
            ..row
        };
        std::iter::once(NodeSection {
            title: String::new(),
            header: None,
            rows: vec![object],
        })
        .chain(panel.into_iter().map(|section| NodeSection {
            title: section.title,
            header: section.header.map(listed_row),
            rows: section.rows.into_iter().map(listed_row).collect(),
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
            .filter_map(|node| match &node.kind {
                Kind::Object(name) => Some((
                    node.id,
                    self.graph_object_sections(i, node.id, ObjectKind::from_name(name)),
                )),
                Kind::Op(_) => None,
            })
            .collect();
        graph.sections(objects)
    }

    /// Write one port of `handle` through the panel's handlers: a row, every
    /// component of a group, or every writable row of a section that the
    /// value names.
    pub(super) fn graph_write(
        &mut self,
        i: usize,
        handle: Handle,
        field: &str,
        value: &Value,
    ) -> Task<Message> {
        // An empty target falls back to the drawing defaults in some handlers.
        if self.tabs[i].scene.is_layer_locked(handle) {
            return Task::none();
        }
        if field.starts_with(SECTION) || components(field).is_some() {
            let wanted: Vec<(String, Value)> = match components(field) {
                Some(members) => match typed_point(value) {
                    Some(p) => members
                        .zip(p)
                        .map(|(member, x)| (member.to_owned(), graph::value::number(x)))
                        .collect(),
                    None => Vec::new(),
                },
                None => value
                    .as_object()
                    .map(|values| values.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
            };
            let rows: Vec<NodeRow> = self
                .graph_entity_sections(i, handle)
                .into_iter()
                .flat_map(|section| section.rows)
                .filter(|row| row.input && !matches!(row.widget, RowWidget::Group))
                .collect();
            let tasks: Vec<Task<Message>> = wanted
                .into_iter()
                .filter_map(|(member, value)| {
                    let row = rows.iter().find(|row| row.field == member)?;
                    let same = row.value == value
                        || row_number(&row.value)
                            .zip(row_number(&value))
                            .is_some_and(|(a, b)| (a - b).abs() <= 1e-12 * a.abs().max(1.0));
                    (!same).then_some((member, value))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(member, value)| self.graph_write(i, handle, &member, &value))
                .collect();
            return Task::batch(tasks);
        }
        let Some(property) = self
            .graph_panel_sections(i, handle)
            .into_iter()
            .flat_map(|section| section.props)
            .find(|property| crate::app::control::row_id(property) == field)
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
        self.graph_unwind(i);
        let mut tasks = edit(self);
        let mut engine = std::mem::take(&mut self.tabs[i].graph.engine);
        let mut host = AppHost {
            app: self,
            i,
            tasks: Vec::new(),
        };
        engine.evaluate(&mut host);
        tasks.append(&mut host.tasks);
        self.tabs[i].graph.engine = engine;
        self.graph_undo_open = false;
        Task::batch(tasks)
    }

    /// Takes back the last evaluation's edits: objects edited in place get
    /// their prior state back (latest edit first), created ones are erased.
    fn graph_unwind(&mut self, i: usize) {
        let tab = &mut self.tabs[i];
        let applied = std::mem::take(&mut tab.graph.applied);
        let created = std::mem::take(&mut tab.graph.created);
        if applied.is_empty() && created.is_empty() {
            return;
        }
        for (_, entity) in applied.into_iter().rev() {
            tab.scene.update_entity(entity);
        }
        let live: Vec<Handle> = created
            .into_iter()
            .filter(|handle| tab.scene.document.get_entity(*handle).is_some())
            .collect();
        if !live.is_empty() {
            tab.scene.erase_entities(&live);
        }
        tab.dirty = true;
    }

    pub(super) fn graph_run(&mut self, i: usize) -> Task<Message> {
        self.graph_step(i, |_| Vec::new())
    }

    pub(super) fn on_graph(&mut self, message: GraphMsg) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].graph.update_ui(&message);
        match message {
            GraphMsg::Toggle => {
                self.show_node_graph = !self.show_node_graph;
                if self.show_node_graph {
                    // The node library docks on the right on first use; after
                    // that it stays wherever the user moved it.
                    let id = crate::ui::dock::PanelId::NodeGraph;
                    self.dock_open_at_default(id);
                    return self.graph_run(i);
                }
            }
            GraphMsg::PaletteRelease => {
                if let Some(Drag::Palette(item)) = self.tabs[i].graph.drag.take() {
                    let pos = self.tabs[i].graph.drop_position(false);
                    self.tabs[i].graph.add_node(item, Vec::new(), pos);
                    return self.graph_run(i);
                }
            }
            GraphMsg::Released => {
                let sections = self.graph_all_sections(i);
                match self.tabs[i].graph.drag.take() {
                    Some(Drag::Palette(item)) => {
                        let pos = self.tabs[i].graph.drop_position(true);
                        self.tabs[i].graph.add_node(item, Vec::new(), pos);
                        return self.graph_run(i);
                    }
                    Some(Drag::Link(from)) => {
                        if let Some(to) = self.tabs[i].graph.hit_input(&sections) {
                            self.graph_connect(i, from, to);
                        }
                        // A link dropped on nothing was detached from an input.
                        return self.graph_run(i);
                    }
                    _ => {}
                }
            }
            GraphMsg::NodeDelete(id) => {
                let graph = &mut self.tabs[i].graph;
                // Only the entities a node made go with it.
                let owns = graph.engine.node(id).is_some_and(|node| match &node.kind {
                    Kind::Object(name) => ObjectKind::from_name(name).is_some_and(ObjectKind::owns),
                    Kind::Op(_) => false,
                });
                graph.engine.remove(id);
                graph.drafts.retain(|port, _| port.node != id);
                let handles = graph
                    .ui
                    .remove(&id)
                    .filter(|_| owns)
                    .map(|ui| ui.handles)
                    .unwrap_or_default();
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
            // The last evaluation's edits stay in the drawing as they are.
            GraphMsg::New => self.tabs[i].graph = Default::default(),
            GraphMsg::Save => {
                let bytes = self.graph_file_bytes(i);
                return Task::perform(
                    async move {
                        let handle = crate::sys::file_dialog()
                            .set_title(crate::t!("Save Node Graph").as_ref())
                            .add_filter(crate::t!("Node Graph").as_ref(), &["ocg"])
                            .set_file_name("graph.ocg")
                            .save_file()
                            .await?;
                        let name = crate::sys::handle_path(&handle)
                            .to_string_lossy()
                            .into_owned();
                        Some(
                            handle
                                .write(&bytes)
                                .await
                                .map(|()| name)
                                .map_err(|e| e.to_string()),
                        )
                    },
                    |result| Message::Graph(GraphMsg::Saved(result)),
                );
            }
            GraphMsg::Saved(Some(Ok(name))) => {
                self.command_line
                    .push_output(crate::tf!("Node graph saved to {}", name).as_ref());
            }
            GraphMsg::Saved(Some(Err(error))) => self.command_line.push_error(&error),
            GraphMsg::Open => {
                return Task::perform(
                    async {
                        let handle = crate::sys::file_dialog()
                            .set_title(crate::t!("Open Node Graph").as_ref())
                            .add_filter(crate::t!("Node Graph").as_ref(), &["ocg"])
                            .pick_file()
                            .await?;
                        let _ = crate::sys::handle_path(&handle);
                        Some(handle.read().await)
                    },
                    |bytes| Message::Graph(GraphMsg::Loaded(bytes)),
                );
            }
            GraphMsg::Loaded(Some(bytes)) => {
                let (mut graph, drawing) = match crate::ui::node_graph::Graph::from_file(&bytes) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        self.command_line.push_error(&error);
                        return Task::none();
                    }
                };
                // Entities are reused only in the drawing the graph was saved
                // against, and only while they are still the node's kind;
                // otherwise evaluation creates fresh ones. The same holds for
                // the last evaluation's edits, which the next one takes back.
                let document = &self.tabs[i].scene.document;
                let same_drawing =
                    !drawing.is_empty() && drawing == document.header.fingerprint_guid;
                for node in &graph.engine.nodes {
                    let kind = match &node.kind {
                        Kind::Object(name) => ObjectKind::from_name(name).filter(|kind| kind.owns()),
                        Kind::Op(_) => None,
                    };
                    if let Some(ui) = graph.ui.get_mut(&node.id) {
                        ui.handles.retain(|handle| {
                            same_drawing
                                && kind
                                    .zip(document.get_entity(*handle))
                                    .is_some_and(|(kind, entity)| kind.matches(entity))
                        });
                    }
                }
                if !same_drawing {
                    graph.applied.clear();
                    graph.created.clear();
                }
                // Evaluating unwinds what the open graph did first.
                let (applied, created) = {
                    let open = &mut self.tabs[i].graph;
                    (std::mem::take(&mut open.applied), std::mem::take(&mut open.created))
                };
                graph.applied.splice(0..0, applied);
                graph.created.extend(created);
                self.tabs[i].graph = graph;
                return self.graph_run(i);
            }
            _ => {}
        }
        Task::none()
    }

    /// Links `from` into `to`; false when that would close a loop. The link
    /// replaces what was typed into an object's row or group; an operation
    /// keeps its value for when the link goes again.
    pub(super) fn graph_connect(&mut self, i: usize, from: Port, to: Port) -> bool {
        let engine = &mut self.tabs[i].graph.engine;
        if !engine.connect(from, to.clone()) {
            return false;
        }
        if let Some(node) = engine.node_mut(to.node) {
            if matches!(node.kind, Kind::Object(_)) {
                node.params.retain(|field, _| {
                    *field != to.field && !to.field.split('|').any(|member| member == field)
                });
            }
        }
        true
    }

    /// The `.ocg` file of tab `i`'s graph. The file names the drawing by its
    /// fingerprint so reopening it elsewhere cannot take over unrelated
    /// entities; a drawing that never had one gets it now and saves it with
    /// its next save.
    pub(super) fn graph_file_bytes(&mut self, i: usize) -> Vec<u8> {
        let tab = &mut self.tabs[i];
        let header = &mut tab.scene.document.header;
        if header.fingerprint_guid.is_empty() {
            header.fingerprint_guid =
                format!("{{{}}}", crate::app::control::new_guid_v4().to_uppercase());
            tab.dirty = true;
        }
        tab.graph.to_file(&tab.scene.document.header.fingerprint_guid)
    }

    /// An edited input, kept as the node's parameter: an operation reads it
    /// while nothing is linked, an object node writes it to every entity it
    /// stands for on each evaluation.
    fn graph_set(&mut self, i: usize, port: Port, value: Value) -> Task<Message> {
        let Some(node) = self.tabs[i].graph.engine.node_mut(port.node) else {
            return Task::none();
        };
        node.params.insert(port.field, value);
        self.graph_run(i)
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
        let targets: Vec<Option<Handle>> = if kind.owns() {
            let mut handles: Vec<Handle> = app.tabs[i]
                .graph
                .ui
                .get(&node)
                .map(|ui| ui.handles.clone())
                .unwrap_or_default();
            handles.retain(|handle| app.tabs[i].scene.document.get_entity(*handle).is_some());
            // New entities copy the node's first one, so edits made by hand
            // to unlinked rows carry over to every element.
            while handles.len() < instances.len() {
                let Some(mut entity) = handles
                    .first()
                    .and_then(|handle| app.tabs[i].scene.document.get_entity(*handle))
                    .cloned()
                    .or_else(|| kind.entity())
                else {
                    break;
                };
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
            handles.into_iter().map(Some).collect()
        } else {
            // `Object` stands for whatever each instance links in.
            instances
                .iter()
                .map(|fields| {
                    fields
                        .iter()
                        .find(|(field, _)| field == "object")
                        .and_then(|(_, value)| ops::handle(value))
                        .filter(|handle| app.tabs[i].scene.document.get_entity(*handle).is_some())
                })
                .collect()
        };
        if let Some(ui) = app.tabs[i].graph.ui.get_mut(&node) {
            ui.handles = targets.iter().flatten().copied().collect();
        }
        for (handle, fields) in targets.iter().zip(instances) {
            let Some(handle) = *handle else {
                continue;
            };
            for (field, value) in fields.iter().filter(|(field, _)| field != "object") {
                let current = app
                    .graph_fields(i, handle)
                    .into_iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, value)| value);
                if current.as_ref() != Some(value) {
                    let task = app.graph_write(i, handle, field, value);
                    self.tasks.push(task);
                }
            }
        }
        targets
            .iter()
            .map(|handle| match handle {
                Some(handle) => {
                    let mut fields = app.graph_fields(i, *handle);
                    fields.push(("object".to_owned(), ops::object(*handle)));
                    fields
                }
                None => Vec::new(),
            })
            .collect()
    }

    fn call(&mut self, node: NodeId, spec: &'static graph::Spec, args: &[Value]) -> Vec<Value> {
        self.op(node, spec, args)
    }

    fn expression(&self, text: &str) -> Option<f64> {
        crate::app::expr_eval::eval_number(text)
    }
}
