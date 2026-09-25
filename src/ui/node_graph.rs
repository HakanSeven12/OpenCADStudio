//! Node graph overlay over the viewport.
//!
//! The graph itself — nodes, links, evaluation, the operation library — is the
//! `graph` crate. This module is its canvas: the palette, node cards, wires
//! and dragging. Object nodes own drawing entities, and their rows are the
//! Properties panel's own rows for that entity (built and written back in
//! `app::node_graph`); operation rows come from the node's spec.

use codec::{EntityType, Handle};
use graph::{Kind, NodeId, Port, Spec};
use iced::widget::{
    button, canvas, checkbox, column, container, mouse_area, opaque, pin, row, scrollable, slider,
    stack, text, text_input, tooltip, Space,
};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{renderer, Renderer as _, Shell};
use iced::{
    mouse, Background, Border, Element, Length, Point, Rectangle, Size, Theme, Transformation,
    Vector,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::{json, Value};

use crate::app::Message;
use crate::t;

pub const NODE_W: f32 = 280.0;
const HEADER_H: f32 = 26.0;
const SECTION_H: f32 = 22.0;
const ROW_H: f32 = 22.0;
const LABEL_W: f32 = 96.0;
const PORT_W: f32 = 14.0;
const PORT_HIT: f32 = 10.0;
pub const TREE_W: f32 = 190.0;
const ZOOM_STEP: f32 = 1.1;
const ZOOM_STEPS: std::ops::RangeInclusive<i32> = -15..=12;

/// Drawing objects a node can create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Line,
    Circle,
    Arc,
    Ellipse,
    Point,
    Polyline,
    Ray,
    XLine,
    Text,
    MText,
}

const OBJECTS: &[ObjectKind] = &[
    ObjectKind::Line,
    ObjectKind::Circle,
    ObjectKind::Arc,
    ObjectKind::Ellipse,
    ObjectKind::Point,
    ObjectKind::Polyline,
    ObjectKind::Ray,
    ObjectKind::XLine,
    ObjectKind::Text,
    ObjectKind::MText,
];

impl ObjectKind {
    /// Display name, also the graph's object type name.
    pub fn name(self) -> &'static str {
        match self {
            ObjectKind::Line => "Line",
            ObjectKind::Circle => "Circle",
            ObjectKind::Arc => "Arc",
            ObjectKind::Ellipse => "Ellipse",
            ObjectKind::Point => "Point",
            ObjectKind::Polyline => "Polyline",
            ObjectKind::Ray => "Ray",
            ObjectKind::XLine => "Construction Line",
            ObjectKind::Text => "Text",
            ObjectKind::MText => "Multiline Text",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        OBJECTS.iter().copied().find(|kind| kind.name() == name)
    }

    /// Whether `entity` is this kind of object.
    pub fn matches(self, entity: &EntityType) -> bool {
        matches!(
            (self, entity),
            (ObjectKind::Line, EntityType::Line(_))
                | (ObjectKind::Circle, EntityType::Circle(_))
                | (ObjectKind::Arc, EntityType::Arc(_))
                | (ObjectKind::Ellipse, EntityType::Ellipse(_))
                | (ObjectKind::Point, EntityType::Point(_))
                | (ObjectKind::Polyline, EntityType::LwPolyline(_))
                | (ObjectKind::Ray, EntityType::Ray(_))
                | (ObjectKind::XLine, EntityType::XLine(_))
                | (ObjectKind::Text, EntityType::Text(_))
                | (ObjectKind::MText, EntityType::MText(_))
        )
    }

    fn command(self) -> &'static str {
        match self {
            ObjectKind::Line => "LINE",
            ObjectKind::Circle => "CIRCLE",
            ObjectKind::Arc => "ARC",
            ObjectKind::Ellipse => "ELLIPSE",
            ObjectKind::Point => "POINT",
            ObjectKind::Polyline => "PLINE",
            ObjectKind::Ray => "RAY",
            ObjectKind::XLine => "XLINE",
            ObjectKind::Text => "TEXT",
            ObjectKind::MText => "MTEXT",
        }
    }

    /// The entity a new node starts from.
    pub fn entity(self) -> EntityType {
        use codec::entities as e;
        use codec::types::{Vector2, Vector3};
        let origin = Vector3::new(0.0, 0.0, 0.0);
        let x_axis = Vector3::new(1.0, 0.0, 0.0);
        match self {
            ObjectKind::Line => EntityType::Line(e::Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0)),
            ObjectKind::Circle => EntityType::Circle(e::Circle::from_coords(0.0, 0.0, 0.0, 5.0)),
            ObjectKind::Arc => EntityType::Arc(e::Arc::from_coords(
                0.0,
                0.0,
                0.0,
                5.0,
                0.0,
                std::f64::consts::PI,
            )),
            ObjectKind::Ellipse => EntityType::Ellipse(e::Ellipse::from_center_axes(
                origin,
                Vector3::new(10.0, 0.0, 0.0),
                0.5,
            )),
            ObjectKind::Point => EntityType::Point(e::Point::from_coords(0.0, 0.0, 0.0)),
            ObjectKind::Polyline => EntityType::LwPolyline(e::LwPolyline::from_points(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
            ])),
            ObjectKind::Ray => EntityType::Ray(e::Ray::new(origin, x_axis)),
            ObjectKind::XLine => EntityType::XLine(e::XLine::new(origin, x_axis)),
            ObjectKind::Text => {
                EntityType::Text(e::Text::with_value("Text", origin).with_height(2.5))
            }
            ObjectKind::MText => {
                EntityType::MText(e::MText::with_value("MText", origin).with_height(2.5))
            }
        }
    }
}

/// What a palette entry adds.
#[derive(Clone, Copy, Debug)]
pub enum PaletteItem {
    Object(ObjectKind),
    Op(&'static Spec),
}

impl PaletteItem {
    fn label(self) -> &'static str {
        match self {
            PaletteItem::Object(kind) => kind.name(),
            PaletteItem::Op(spec) => spec.name,
        }
    }
}

/// Canvas state of one node.
pub struct NodeUi {
    pub pos: Point,
    /// Expanded section titles; a new object node opens only Geometry.
    pub open: HashSet<String>,
    /// Entities an object node owns, one per list element.
    pub handles: Vec<Handle>,
}

pub enum RowWidget {
    Field,
    Slider { min: f64, max: f64, step: f64 },
    Toggle,
}

/// A group of rows; an empty title has no header and is always open.
pub struct NodeSection {
    pub title: String,
    pub rows: Vec<NodeRow>,
}

pub struct NodeRow {
    pub field: String,
    pub label: String,
    pub value: Value,
    pub input: bool,
    pub output: bool,
    pub widget: RowWidget,
}

pub enum Drag {
    Palette(PaletteItem),
    Node(NodeId, Vector),
    Link(Port),
    Pan(Point, Vector),
}

#[derive(Default)]
pub struct Graph {
    pub engine: graph::Graph,
    pub ui: HashMap<NodeId, NodeUi>,
    pub pan: Vector,
    /// Wheel steps; the scale is `ZOOM_STEP ^ zoom_steps`.
    zoom_steps: i32,
    pub cursor: Point,
    pub drag: Option<Drag>,
    pub drafts: HashMap<Port, String>,
    closed_categories: HashSet<&'static str>,
}

#[derive(Debug, Clone)]
pub enum GraphMsg {
    Toggle,
    Category(&'static str),
    PalettePress(PaletteItem),
    Moved(Point),
    Pressed,
    Released,
    NodePress(NodeId),
    NodeDelete(NodeId),
    Section(NodeId, String),
    OutPress(Port),
    InPress(Port),
    Input(Port, String),
    Commit(Port),
    /// A slider or toggle set an operation input.
    Set(Port, Value),
    Zoom(mouse::ScrollDelta),
    /// Start an empty graph; the drawing keeps the old graph's objects.
    New,
    Save,
    /// `None` when the dialog was cancelled.
    Saved(Option<Result<String, String>>),
    Open,
    Loaded(Option<Vec<u8>>),
}

fn msg(m: GraphMsg) -> Message {
    Message::Graph(m)
}

/// Compact display of a port value.
pub fn display(value: &Value) -> String {
    let number = |x: f64| {
        let s = format!("{:.4}", x);
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    match value {
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), number),
        Value::Array(items) => {
            let shown: Vec<String> = items.iter().take(4).map(display).collect();
            let more = if items.len() > 4 { ", …" } else { "" };
            format!("[{}] {}{more}", items.len(), shown.join(", "))
        }
        Value::Object(_) => match graph::value::point(value) {
            Some(p) => format!("({}, {}, {})", number(p[0]), number(p[1]), number(p[2])),
            None => value.to_string(),
        },
        other => graph::value::text(other),
    }
}

impl Graph {
    fn zoom(&self) -> f32 {
        ZOOM_STEP.powi(self.zoom_steps)
    }

    /// Screen point → graph coordinates.
    fn to_graph(&self, screen: Point) -> Point {
        let zoom = self.zoom();
        Point::new((screen.x - self.pan.x) / zoom, (screen.y - self.pan.y) / zoom)
    }

    /// Graph coordinates → screen point.
    fn to_screen(&self, graph: Point) -> Point {
        let zoom = self.zoom();
        Point::new(self.pan.x + graph.x * zoom, self.pan.y + graph.y * zoom)
    }

    /// The `.ocg` file: the graph, the canvas, and the fingerprint of the
    /// drawing its object nodes' entities live in.
    pub fn to_file(&self, drawing: &str) -> Vec<u8> {
        let nodes: serde_json::Map<String, Value> = self
            .ui
            .iter()
            .map(|(id, ui)| {
                let mut open: Vec<&String> = ui.open.iter().collect();
                open.sort();
                let handles: Vec<u64> = ui.handles.iter().map(|handle| handle.value()).collect();
                (id.to_string(), json!({ "pos": [ui.pos.x, ui.pos.y], "open": open, "handles": handles }))
            })
            .collect();
        let file = json!({
            "format": "ocg",
            "version": 1,
            "drawing": drawing,
            "graph": self.engine.to_json(),
            "canvas": { "pan": [self.pan.x, self.pan.y], "zoom": self.zoom_steps, "nodes": nodes },
        });
        serde_json::to_vec_pretty(&file).unwrap_or_default()
    }

    /// Reads an `.ocg` file, returning the graph and the drawing fingerprint
    /// it was saved against. Handles come back unchecked.
    pub fn from_file(bytes: &[u8]) -> Result<(Self, String), String> {
        let file: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        if file["format"] != "ocg" {
            return Err(t!("Not a node graph file").into_owned());
        }
        let mut graph = Graph {
            engine: graph::Graph::from_json(&file["graph"])?,
            ..Graph::default()
        };
        let canvas = &file["canvas"];
        let pair = |value: &Value| {
            Some(Vector::new(value[0].as_f64()? as f32, value[1].as_f64()? as f32))
        };
        graph.pan = pair(&canvas["pan"]).unwrap_or_default();
        graph.zoom_steps = canvas["zoom"]
            .as_i64()
            .map_or(0, |steps| (steps as i32).clamp(*ZOOM_STEPS.start(), *ZOOM_STEPS.end()));
        let ids: Vec<NodeId> = graph.engine.nodes.iter().map(|node| node.id).collect();
        for (index, id) in ids.into_iter().enumerate() {
            let saved = &canvas["nodes"][id.to_string()];
            let step = (index % 6) as f32 * 30.0;
            let pos = pair(&saved["pos"]).unwrap_or(Vector::new(TREE_W + 40.0 + step, 40.0 + step));
            let open: HashSet<String> = match saved["open"].as_array() {
                Some(titles) => titles.iter().filter_map(|title| title.as_str().map(str::to_owned)).collect(),
                None => std::iter::once(t!("Geometry").into_owned()).collect(),
            };
            let handles = saved["handles"]
                .as_array()
                .map(|values| values.iter().filter_map(Value::as_u64).map(Handle::new).collect())
                .unwrap_or_default();
            graph.ui.insert(id, NodeUi { pos: Point::ORIGIN + pos, open, handles });
        }
        Ok((graph, file["drawing"].as_str().unwrap_or_default().to_owned()))
    }

    pub fn add_node(&mut self, item: PaletteItem, handles: Vec<Handle>, pos: Point) -> NodeId {
        let kind = match item {
            PaletteItem::Object(kind) => Kind::Object(kind.name().to_owned()),
            PaletteItem::Op(spec) => Kind::Op(spec),
        };
        let id = self.engine.add(kind);
        let mut open = HashSet::default();
        open.insert(t!("Geometry").into_owned());
        self.ui.insert(id, NodeUi { pos, open, handles });
        id
    }

    /// Where a palette entry lands: under the cursor when dropped on the
    /// canvas, staggered beside the palette when clicked.
    pub fn drop_position(&self) -> Point {
        if self.cursor.x > TREE_W {
            return self.to_graph(self.cursor);
        }
        let step = (self.engine.nodes.len() % 6) as f32 * 30.0;
        self.to_graph(Point::new(TREE_W + 40.0 + step, 40.0 + step))
    }

    /// Rows of an operation node, read from its spec. An input sharing its
    /// name with an output is one row with both ports.
    fn op_sections(&self, id: NodeId, spec: &'static Spec) -> Vec<NodeSection> {
        let Some(node) = self.engine.node(id) else {
            return Vec::new();
        };
        let param = |name: &str| node.params.get(name).and_then(graph::value::num);
        let mut rows: Vec<NodeRow> = spec
            .inputs
            .iter()
            .map(|input| {
                let port = Port { node: id, field: input.name.to_owned() };
                let widget = match (spec.name, input.name) {
                    ("Number Slider" | "Integer Slider", "value") => RowWidget::Slider {
                        min: param("min").unwrap_or(0.0),
                        max: param("max").unwrap_or(1.0),
                        step: param("step").unwrap_or(0.0),
                    },
                    ("Boolean", "value") => RowWidget::Toggle,
                    _ => RowWidget::Field,
                };
                let output = spec.outputs.contains(&input.name);
                NodeRow {
                    field: input.name.to_owned(),
                    label: input.name.to_owned(),
                    value: if output {
                        node.outputs.get(input.name).cloned().unwrap_or(Value::Null)
                    } else {
                        self.engine.input(&port)
                    },
                    input: true,
                    output,
                    widget,
                }
            })
            .collect();
        rows.extend(
            spec.outputs
                .iter()
                .filter(|name| !spec.inputs.iter().any(|input| input.name == **name))
                .map(|name| NodeRow {
                    field: (*name).to_owned(),
                    label: (*name).to_owned(),
                    value: node.outputs.get(*name).cloned().unwrap_or(Value::Null),
                    input: false,
                    output: true,
                    widget: RowWidget::Field,
                }),
        );
        vec![NodeSection { title: String::new(), rows }]
    }

    /// Every node's rows, parallel to `engine.nodes`. `objects` holds the
    /// object nodes' rows, which only the app can build.
    pub fn sections(&self, mut objects: HashMap<NodeId, Vec<NodeSection>>) -> Vec<Vec<NodeSection>> {
        self.engine
            .nodes
            .iter()
            .map(|node| match node.kind {
                Kind::Op(spec) => self.op_sections(node.id, spec),
                Kind::Object(_) => objects.remove(&node.id).unwrap_or_default(),
            })
            .collect()
    }

    fn is_open(&self, id: NodeId, section: &NodeSection) -> bool {
        section.title.is_empty()
            || self.ui.get(&id).is_some_and(|ui| ui.open.contains(&section.title))
    }

    /// Row centre of `field`, relative to the node's top. Rows of a collapsed
    /// section keep their ports on its header.
    fn port_offset(&self, id: NodeId, sections: &[NodeSection], field: &str, input: bool) -> Option<f32> {
        let mut y = HEADER_H;
        for section in sections {
            let matches = |row: &NodeRow| row.field == field && if input { row.input } else { row.output };
            let header = if section.title.is_empty() { 0.0 } else { SECTION_H };
            if !self.is_open(id, section) {
                if section.rows.iter().any(matches) {
                    return Some(y + SECTION_H * 0.5);
                }
                y += header;
                continue;
            }
            y += header;
            for row in &section.rows {
                if matches(row) {
                    return Some(y + ROW_H * 0.5);
                }
                y += ROW_H;
            }
        }
        None
    }

    fn port_point(&self, sections: &[Vec<NodeSection>], port: &Port, input: bool) -> Option<Point> {
        let index = self.engine.nodes.iter().position(|node| node.id == port.node)?;
        let ui = self.ui.get(&port.node)?;
        let y = self.port_offset(port.node, &sections[index], &port.field, input)?;
        let x = if input { PORT_W * 0.5 } else { NODE_W - PORT_W * 0.5 };
        Some(self.to_screen(ui.pos + Vector::new(x, y)))
    }

    /// The input port under the cursor.
    pub fn hit_input(&self, sections: &[Vec<NodeSection>]) -> Option<Port> {
        self.engine.nodes.iter().zip(sections).find_map(|(node, node_sections)| {
            node_sections
                .iter()
                .filter(|section| self.is_open(node.id, section))
                .flat_map(|section| &section.rows)
                .filter(|row| row.input)
                .map(|row| Port { node: node.id, field: row.field.clone() })
                .find(|port| {
                    self.port_point(sections, port, true)
                        .is_some_and(|p| p.distance(self.cursor) <= PORT_HIT * self.zoom().max(0.5))
                })
        })
    }

    pub fn view(&self, sections: Vec<Vec<NodeSection>>) -> Element<'_, Message> {
        let wires = self
            .engine
            .links
            .iter()
            .filter_map(|link| {
                Some((
                    self.port_point(&sections, &link.from, false)?,
                    self.port_point(&sections, &link.to, true)?,
                ))
            })
            .collect();
        let pending = match &self.drag {
            Some(Drag::Link(from)) => self
                .port_point(&sections, from, false)
                .map(|start| (start, self.cursor)),
            _ => None,
        };
        let zoom = self.zoom();
        let mut nodes = stack![];
        for (node, node_sections) in self.engine.nodes.iter().zip(sections) {
            let Some(ui) = self.ui.get(&node.id) else {
                continue;
            };
            nodes = nodes.push(pin(self.node_view(node, node_sections)).x(ui.pos.x).y(ui.pos.y));
        }
        let mut layers = stack![
            canvas(Wires { wires, pending, width: 2.0 * zoom.max(0.5) })
                .width(Length::Fill)
                .height(Length::Fill),
            Zoomed {
                content: nodes.width(Length::Fill).height(Length::Fill).into(),
                pan: self.pan,
                zoom,
            },
            self.palette_view(),
        ];
        if let Some(Drag::Palette(item)) = self.drag {
            let ghost = container(text(t!(item.label())).size(11))
                .padding([3, 8])
                .style(container::bordered_box);
            layers = layers.push(pin(ghost).x(self.cursor.x + 10.0).y(self.cursor.y + 10.0));
        }
        let area = mouse_area(layers.width(Length::Fill).height(Length::Fill))
            .on_move(|p| msg(GraphMsg::Moved(p)))
            .on_press(msg(GraphMsg::Pressed))
            .on_release(msg(GraphMsg::Released))
            .on_middle_press(msg(GraphMsg::Pressed))
            .on_middle_release(msg(GraphMsg::Released))
            .on_scroll(|delta| msg(GraphMsg::Zoom(delta)));
        opaque(area)
    }

    fn palette_entry(&self, item: PaletteItem) -> Element<'_, Message> {
        let icon: Element<'_, Message> = match item {
            PaletteItem::Object(kind) => crate::modules::registry::command_icon(kind.command())
                .map_or_else(|| Space::new().width(14.0).into(), |bytes| {
                    crate::ui::icons::semantic(bytes, 14.0)
                }),
            PaletteItem::Op(_) => Space::new().width(14.0).into(),
        };
        mouse_area(
            container(
                row![icon, text(t!(item.label())).size(11)]
                    .spacing(6)
                    .align_y(iced::Center),
            )
            .padding([3, 16])
            .width(Length::Fill),
        )
        .on_press(msg(GraphMsg::PalettePress(item)))
        .interaction(mouse::Interaction::Grab)
        .into()
    }

    fn palette_view(&self) -> Element<'_, Message> {
        let tool = |icon: &'static [u8], tip: &str, message: GraphMsg| {
            tooltip(
                button(crate::ui::icons::themed(icon, 12.0))
                    .on_press(msg(message))
                    .style(button::text)
                    .padding([2, 4]),
                container(text(t!(tip)).size(11))
                    .padding([3, 6])
                    .style(container::bordered_box),
                tooltip::Position::Bottom,
            )
        };
        let header = row![
            text(t!("Nodes")).size(12).width(Length::Fill),
            tool(crate::ui::icons::DOC_NEW, "New graph", GraphMsg::New),
            tool(crate::ui::icons::FOLDER_OPEN, "Open graph", GraphMsg::Open),
            tool(crate::ui::icons::SAVE, "Save graph", GraphMsg::Save),
        ]
        .align_y(iced::Center);
        let mut col = column![header].spacing(2).padding(8);
        let categories = std::iter::once("Objects").chain(graph::CATEGORIES.iter().copied());
        for category in categories {
            let open = !self.closed_categories.contains(category);
            col = col.push(
                button(
                    row![
                        crate::ui::icons::themed_arrow_toggle(open, 9.0),
                        text(t!(category)).size(11)
                    ]
                    .spacing(4)
                    .align_y(iced::Center),
                )
                .on_press(msg(GraphMsg::Category(category)))
                .style(button::text)
                .width(Length::Fill)
                .padding([2, 2]),
            );
            if !open {
                continue;
            }
            if category == "Objects" {
                for &kind in OBJECTS {
                    col = col.push(self.palette_entry(PaletteItem::Object(kind)));
                }
            } else {
                for spec in graph::LIBRARY.iter().filter(|spec| spec.category == category) {
                    col = col.push(self.palette_entry(PaletteItem::Op(spec)));
                }
            }
        }
        opaque(
            container(scrollable(col))
                .width(TREE_W)
                .height(Length::Fill)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().background.strong.color,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
        )
    }

    fn node_view(&self, node: &graph::Node, sections: Vec<NodeSection>) -> Element<'_, Message> {
        let id = node.id;
        let title = match &node.kind {
            Kind::Op(spec) => t!(spec.name).into_owned(),
            Kind::Object(name) => {
                let count = self.ui.get(&id).map_or(0, |ui| ui.handles.len());
                let name = t!(name.as_str()).into_owned();
                if count > 1 { format!("{name} ×{count}") } else { name }
            }
        };
        let header = row![
            mouse_area(
                container(text(title).size(12))
                    .padding([4, 8])
                    .width(Length::Fill)
                    .height(HEADER_H),
            )
            .on_press(msg(GraphMsg::NodePress(id)))
            .interaction(mouse::Interaction::Grab),
            button(crate::ui::icons::themed(crate::ui::icons::CLOSE, 10.0))
                .on_press(msg(GraphMsg::NodeDelete(id)))
                .style(button::text)
                .padding([6, 8]),
        ]
        .height(HEADER_H);
        let header = container(header).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().primary.weak.color)),
            text_color: Some(theme.palette().primary.weak.text),
            ..Default::default()
        });
        let mut col = column![header];
        if sections.is_empty() {
            col = col.push(
                container(text(t!("Object deleted")).size(11))
                    .padding([4, 8])
                    .height(ROW_H),
            );
        }
        for section in sections {
            let open = self.is_open(id, &section);
            if !section.title.is_empty() {
                col = col.push(
                    button(
                        row![
                            crate::ui::icons::themed_arrow_toggle(open, 9.0),
                            text(section.title.clone()).size(11)
                        ]
                        .spacing(4)
                        .align_y(iced::Center),
                    )
                    .on_press(msg(GraphMsg::Section(id, section.title)))
                    .style(button::secondary)
                    .width(Length::Fill)
                    .height(SECTION_H)
                    .padding([0, 6]),
                );
            }
            if open {
                for node_row in section.rows {
                    col = col.push(self.row_view(id, node_row));
                }
            }
        }
        opaque(
            container(col)
                .width(NODE_W)
                .style(|theme: &Theme| container::Style {
                    background: Some(Background::Color(theme.palette().background.base.color)),
                    border: Border {
                        color: theme.palette().background.strong.color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }),
        )
    }

    fn row_view(&self, id: NodeId, node_row: NodeRow) -> Element<'_, Message> {
        let port = Port { node: id, field: node_row.field };
        let linked_in = self.engine.is_linked(&port);
        let linked_out = self.engine.links.iter().any(|link| link.from == port);
        let shown = display(&node_row.value);
        let in_port: Element<'_, Message> = if node_row.input {
            mouse_area(port_dot(linked_in))
                .on_press(msg(GraphMsg::InPress(port.clone())))
                .into()
        } else {
            Space::new().width(PORT_W).into()
        };
        let out_port: Element<'_, Message> = if node_row.output {
            mouse_area(port_dot(linked_out))
                .on_press(msg(GraphMsg::OutPress(port.clone())))
                .into()
        } else {
            Space::new().width(PORT_W).into()
        };
        let value: Element<'_, Message> = if node_row.input && !linked_in {
            match node_row.widget {
                RowWidget::Slider { min, max, step } => {
                    let current = graph::value::num(&node_row.value).unwrap_or(min);
                    let set = port.clone();
                    let mut bar = slider(min.min(max)..=max.max(min), current, move |x| {
                        msg(GraphMsg::Set(set.clone(), graph::value::number(x)))
                    });
                    if step > 0.0 {
                        bar = bar.step(step);
                    }
                    row![bar.width(Length::Fill), text(shown).size(11)]
                        .spacing(4)
                        .align_y(iced::Center)
                        .width(Length::Fill)
                        .into()
                }
                RowWidget::Toggle => {
                    let set = port.clone();
                    checkbox(graph::value::boolean(&node_row.value).unwrap_or(false))
                        .on_toggle(move |on| msg(GraphMsg::Set(set.clone(), Value::Bool(on))))
                        .size(14)
                        .width(Length::Fill)
                        .into()
                }
                RowWidget::Field => {
                    let draft = self.drafts.get(&port).cloned().unwrap_or(shown);
                    let edit = port.clone();
                    text_input("", &draft)
                        .on_input(move |s| msg(GraphMsg::Input(edit.clone(), s)))
                        .on_submit(msg(GraphMsg::Commit(port)))
                        .size(11)
                        .padding([2, 4])
                        .width(Length::Fill)
                        .into()
                }
            }
        } else {
            text(shown).size(11).width(Length::Fill).into()
        };
        row![in_port, text(node_row.label).size(11).width(LABEL_W), value, out_port]
            .spacing(2)
            .height(ROW_H)
            .align_y(iced::Center)
            .into()
    }

    /// Pure canvas state; document and evaluation work is the app's.
    pub fn update_ui(&mut self, message: &GraphMsg) {
        match message {
            GraphMsg::Category(category) => {
                if !self.closed_categories.remove(category) {
                    self.closed_categories.insert(category);
                }
            }
            GraphMsg::PalettePress(item) => self.drag = Some(Drag::Palette(*item)),
            GraphMsg::Moved(p) => {
                self.cursor = *p;
                match self.drag {
                    Some(Drag::Node(id, grab)) => {
                        let pos = self.to_graph(*p) - grab;
                        if let Some(ui) = self.ui.get_mut(&id) {
                            ui.pos = pos;
                        }
                    }
                    Some(Drag::Pan(start, start_pan)) => self.pan = start_pan + (*p - start),
                    _ => {}
                }
            }
            GraphMsg::Pressed => self.drag = Some(Drag::Pan(self.cursor, self.pan)),
            GraphMsg::NodePress(id) => {
                // Raise the grabbed node above the others.
                if let Some(index) = self.engine.nodes.iter().position(|node| node.id == *id) {
                    let node = self.engine.nodes.remove(index);
                    self.engine.nodes.push(node);
                }
                if let Some(ui) = self.ui.get(id) {
                    self.drag = Some(Drag::Node(*id, self.to_graph(self.cursor) - ui.pos));
                }
            }
            GraphMsg::Section(id, title) => {
                if let Some(ui) = self.ui.get_mut(id) {
                    if !ui.open.remove(title) {
                        ui.open.insert(title.clone());
                    }
                }
            }
            GraphMsg::OutPress(port) => self.drag = Some(Drag::Link(port.clone())),
            GraphMsg::InPress(port) => {
                // Grabbing a linked input detaches the link to re-route it.
                if let Some(link) = self.engine.disconnect(port) {
                    self.drag = Some(Drag::Link(link.from));
                }
            }
            GraphMsg::Zoom(delta) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if lines != 0.0 {
                    // Keep the graph point under the cursor where it is.
                    let anchor = self.to_graph(self.cursor);
                    self.zoom_steps = (self.zoom_steps + lines.signum() as i32)
                        .clamp(*ZOOM_STEPS.start(), *ZOOM_STEPS.end());
                    let zoom = self.zoom();
                    self.pan = Vector::new(self.cursor.x - anchor.x * zoom, self.cursor.y - anchor.y * zoom);
                }
            }
            GraphMsg::Input(port, value) => {
                self.drafts.insert(port.clone(), value.clone());
            }
            _ => {}
        }
    }
}

fn port_dot<'a>(linked: bool) -> Element<'a, Message> {
    container(
        container(Space::new())
            .width(8.0)
            .height(8.0)
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: linked.then_some(Background::Color(palette.primary.base.color)),
                    border: Border {
                        color: palette.primary.base.color,
                        width: 1.5,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            }),
    )
    .width(PORT_W)
    .height(ROW_H)
    .align_x(iced::Center)
    .align_y(iced::Center)
    .into()
}

/// Translucent backdrop plus link curves.
struct Wires {
    wires: Vec<(Point, Point)>,
    pending: Option<(Point, Point)>,
    width: f32,
}

fn wire_path(a: Point, b: Point) -> canvas::Path {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0);
    canvas::Path::new(|path| {
        path.move_to(a);
        path.bezier_curve_to(Point::new(a.x + dx, a.y), Point::new(b.x - dx, b.y), b);
    })
}

impl canvas::Program<Message> for Wires {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let palette = theme.palette();
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            palette.background.base.color.scale_alpha(0.6),
        );
        let stroke = canvas::Stroke::default()
            .with_color(palette.primary.base.color)
            .with_width(self.width);
        for (a, b) in &self.wires {
            frame.stroke(&wire_path(*a, *b), stroke);
        }
        if let Some((a, b)) = self.pending {
            frame.stroke(
                &wire_path(a, b),
                stroke.with_color(palette.primary.weak.color),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// Lays its content out in graph space and draws and hit-tests it through
/// `pan` and `zoom`, so every widget inside scales as one picture.
struct Zoomed<'a> {
    content: Element<'a, Message>,
    pan: Vector,
    zoom: f32,
}

impl Zoomed<'_> {
    /// Content coordinates → screen, for a layer whose top-left is `origin`.
    fn transformation(&self, origin: Point) -> Transformation {
        Transformation::translate(origin.x + self.pan.x, origin.y + self.pan.y)
            * Transformation::scale(self.zoom)
            * Transformation::translate(-origin.x, -origin.y)
    }
}

impl Widget<Message, Theme, iced::Renderer> for Zoomed<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let inner = layout::Limits::new(Size::ZERO, size * (1.0 / self.zoom));
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &inner);
        layout::Node::with_children(size, vec![content])
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let inverse = self.transformation(layout.position()).inverse();
        let Some(content) = layout.children().next() else {
            return;
        };
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content,
            cursor * inverse,
            renderer,
            shell,
            &(*viewport * inverse),
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let inverse = self.transformation(layout.position()).inverse();
        layout.children().next().map_or(mouse::Interaction::None, |content| {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                content,
                cursor * inverse,
                &(*viewport * inverse),
                renderer,
            )
        })
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        if let Some(content) = layout.children().next() {
            self.content
                .as_widget_mut()
                .operate(&mut tree.children[0], content, renderer, operation);
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let Some(content) = layout.children().next() else {
            return;
        };
        let bounds = layout.bounds();
        let transformation = self.transformation(bounds.position());
        let inverse = transformation.inverse();
        renderer.with_layer(bounds, |renderer| {
            renderer.with_transformation(transformation, |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    content,
                    cursor * inverse,
                    &(bounds * inverse),
                );
            });
        });
    }
}

impl<'a> From<Zoomed<'a>> for Element<'a, Message> {
    fn from(zoomed: Zoomed<'a>) -> Self {
        Element::new(zoomed)
    }
}
