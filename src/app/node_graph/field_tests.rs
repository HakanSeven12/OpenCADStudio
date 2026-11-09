//! The node graph's text fields, driven by real pointer and key events: the
//! interface is rebuilt after each event, as the application does, so a commit
//! between a press and its release is covered.

use super::*;
use crate::ui::node_graph::PaletteItem;
use iced::keyboard::{self, key::Named, Key};
use iced::mouse::{self, Button};
use iced::{Event, Point, Size};
use iced_test::core::renderer::{Headless, Settings};
use iced_test::runtime::user_interface::{Cache, UserInterface};
use iced_test::simulator::{press_key, typewrite};

const CANVAS: Point = Point::new(900.0, 700.0);
const CLOSE: f64 = 1e-9;

struct Rig {
    app: OpenCADStudio,
    i: usize,
    cache: Option<Cache>,
    renderer: iced::Renderer,
    cursor: Point,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Rig {
    fn new() -> Self {
        let lock = crate::headless_ui_lock();
        let renderer =
            iced_test::futures::futures::executor::block_on(<iced::Renderer as Headless>::new(
                Settings::default(),
                std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
            ))
            .expect("headless renderer");
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        Rig {
            app,
            i,
            cache: Some(Cache::default()),
            renderer,
            cursor: Point::ORIGIN,
            _lock: lock,
        }
    }

    fn op(&mut self, name: &str, at: Point) -> NodeId {
        let spec = graph::LIBRARY
            .iter()
            .chain(ops::LIBRARY)
            .find(|spec| spec.name == name)
            .unwrap_or_else(|| panic!("no node {name}"));
        let id = self.app.tabs[self.i]
            .graph
            .add_node(PaletteItem::Op(spec), vec![], at);
        let _ = self.app.graph_run(self.i);
        id
    }

    fn object(&mut self, kind: ObjectKind, at: Point) -> NodeId {
        let graph = &mut self.app.tabs[self.i].graph;
        let id = graph.add_node(PaletteItem::Object(kind), vec![], at);
        let _ = self.app.graph_run(self.i);
        id
    }

    fn link(&mut self, from: NodeId, output: &str, to: NodeId, input: &str) {
        let (from, to) = (port(from, output), port(to, input));
        assert!(self.app.graph_connect(self.i, from, to));
        let _ = self.app.graph_run(self.i);
    }

    /// One event against a freshly built interface, then its messages.
    fn send(&mut self, event: Event) {
        let mut messages = Vec::new();
        {
            let sections = self.app.graph_all_sections(self.i);
            let view = self.app.tabs[self.i].graph.view(sections);
            let cache = self.cache.take().expect("cache");
            let mut ui =
                UserInterface::build(view, Size::new(1200.0, 800.0), cache, &mut self.renderer);
            ui.update(
                &iced_test::core::window::Headless,
                &iced_test::core::shell::Waker::noop(),
                std::slice::from_ref(&event),
                mouse::Cursor::Available(self.cursor),
                &mut self.renderer,
                &mut messages,
            );
            self.cache = Some(ui.into_cache());
        }
        for message in messages {
            if let Message::Graph(message) = message {
                let _ = self.app.on_graph(message);
            }
        }
    }

    /// The cursor shown with the pointer at `at`.
    fn cursor_at(&mut self, at: Point) -> mouse::Interaction {
        self.cursor = at;
        let event = Event::Mouse(mouse::Event::CursorMoved { position: at });
        let sections = self.app.graph_all_sections(self.i);
        let view = self.app.tabs[self.i].graph.view(sections);
        let cache = self.cache.take().expect("cache");
        let mut ui =
            UserInterface::build(view, Size::new(1200.0, 800.0), cache, &mut self.renderer);
        let mut messages = Vec::new();
        let (state, _) = ui.update(
            &iced_test::core::window::Headless,
            &iced_test::core::shell::Waker::noop(),
            std::slice::from_ref(&event),
            mouse::Cursor::Available(at),
            &mut self.renderer,
            &mut messages,
        );
        self.cache = Some(ui.into_cache());
        match state {
            iced_test::runtime::user_interface::State::Updated {
                mouse_interaction, ..
            } => mouse_interaction,
            other => panic!("outdated interface: {other:?}"),
        }
    }

    fn click(&mut self, at: Point) {
        self.cursor = at;
        self.send(Event::Mouse(mouse::Event::CursorMoved { position: at }));
        self.send(Event::Mouse(mouse::Event::ButtonPressed(Button::Left)));
        self.send(Event::Mouse(mouse::Event::ButtonReleased(Button::Left)));
    }

    fn typed(&mut self, text: &str) {
        for event in typewrite(text).collect::<Vec<_>>() {
            self.send(event);
        }
    }

    fn key(&mut self, key: Named) {
        self.send(press_key(Key::Named(key), None));
    }

    fn shift_tab(&mut self) {
        let mut event = press_key(Key::Named(Named::Tab), None);
        if let Event::Keyboard(keyboard::Event::KeyPressed { modifiers, .. }) = &mut event {
            *modifiers = keyboard::Modifiers::SHIFT;
        }
        self.send(event);
    }

    /// The centre of `field`'s text box.
    fn field(&self, id: NodeId, field: &str) -> Point {
        let all = self.app.graph_all_sections(self.i);
        let graph = &self.app.tabs[self.i].graph;
        let index = graph
            .engine
            .nodes
            .iter()
            .position(|node| node.id == id)
            .expect("node");
        let ui = &graph.ui[&id];
        let mut y = 26.0;
        for section in &all[index] {
            if !section.title.is_empty() {
                y += 22.0;
                if !ui.open.contains(&section.title) {
                    continue;
                }
            }
            for row in &section.rows {
                if row
                    .group
                    .as_ref()
                    .is_some_and(|group| !ui.open.contains(group))
                {
                    continue;
                }
                if row.field == field {
                    return Point::new(ui.pos.x + 190.0, ui.pos.y + y + 11.0);
                }
                y += 22.0;
            }
        }
        panic!("no field {field}");
    }

    fn header(&self, id: NodeId) -> Point {
        let pos = self.app.tabs[self.i].graph.ui[&id].pos;
        Point::new(pos.x + 60.0, pos.y + 13.0)
    }

    fn param(&self, id: NodeId, field: &str) -> Value {
        let node = self.app.tabs[self.i].graph.engine.node(id).expect("node");
        node.params.get(field).cloned().unwrap_or(Value::Null)
    }

    fn number(&self, id: NodeId, field: &str) -> f64 {
        let value = self.param(id, field);
        value
            .as_f64()
            .unwrap_or_else(|| panic!("{field} is {value}, not a number"))
    }

    fn point(&self, id: NodeId) -> [f64; 3] {
        let node = self.app.tabs[self.i].graph.engine.node(id).expect("node");
        graph::value::point(&node.outputs["point"]).expect("a point")
    }

    fn drafts(&self) -> usize {
        self.app.tabs[self.i].graph.drafts.len()
    }

    /// The location of a Point object node's entity.
    fn location(&self, id: NodeId) -> [f64; 3] {
        let handle = self.app.tabs[self.i].graph.ui[&id].handles[0];
        match self.app.tabs[self.i].scene.document.get_entity(handle) {
            Some(EntityType::Point(point)) => {
                [point.location.x, point.location.y, point.location.z]
            }
            other => panic!("not a point: {other:?}"),
        }
    }

    fn open_coordinates(&mut self, id: NodeId) {
        let _ = self
            .app
            .on_graph(GraphMsg::Section(id, LOCATION.to_owned()));
    }

    /// Marks `id`'s outputs; any evaluation of the graph replaces them.
    fn mark_unrun(&mut self, id: NodeId) {
        let node = self.app.tabs[self.i]
            .graph
            .engine
            .node_mut(id)
            .expect("node");
        node.outputs.insert(UNRUN.to_owned(), Value::Bool(true));
    }

    fn has_run(&self, id: NodeId) -> bool {
        let node = self.app.tabs[self.i].graph.engine.node(id).expect("node");
        !node.outputs.contains_key(UNRUN)
    }
}

use document::EntityType;

const LOCATION: &str = "loc_x|loc_y|loc_z";
const UNRUN: &str = "not evaluated since marked";

fn port(node: NodeId, field: &str) -> Port {
    Port {
        node,
        field: field.to_owned(),
    }
}

fn assert_point(actual: [f64; 3], expected: [f64; 3]) {
    let close = actual
        .iter()
        .zip(expected)
        .all(|(a, b)| (a - b).abs() < CLOSE);
    assert!(close, "{actual:?} != {expected:?}");
}

#[test]
fn enter_commits_each_axis_of_a_point() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    for (field, text) in [("x", "100"), ("y", "10.5"), ("z", "-12.75")] {
        let at = rig.field(id, field);
        rig.click(at);
        rig.typed(text);
        rig.key(Named::Enter);
    }
    assert_point(rig.point(id), [100.0, 10.5, -12.75]);
    assert_eq!(rig.drafts(), 0);
}

#[test]
fn enter_keeps_the_field_for_more_typing() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.typed("1");
    rig.key(Named::Enter);
    rig.typed("2");
    rig.key(Named::Enter);
    assert_eq!(rig.number(id, "x"), 12.0);
}

#[test]
fn tab_commits_and_walks_the_fields() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.typed("-12.75");
    rig.key(Named::Tab);
    assert_eq!(rig.number(id, "x"), -12.75, "Tab commits");
    rig.typed("3.141592");
    rig.key(Named::Tab);
    rig.typed("0.001");
    rig.key(Named::Tab);
    assert_point(rig.point(id), [-12.75, 3.141592, 0.001]);
}

#[test]
fn tab_stays_on_the_card_and_wraps_to_its_top() {
    let mut rig = Rig::new();
    let lower = rig.op("Point", Point::new(40.0, 300.0));
    let right = rig.op("Point", Point::new(500.0, 40.0));
    let left = rig.op("Point", Point::new(40.0, 40.0));
    let start = rig.field(left, "x");
    rig.click(start);
    for value in ["1", "2", "3"] {
        rig.typed(value);
        rig.key(Named::Tab);
    }
    assert_point(rig.point(left), [1.0, 2.0, 3.0]);
    // The Tab from z came back to x, not to another card.
    rig.typed("10");
    rig.key(Named::Tab);
    rig.typed("20");
    rig.key(Named::Enter);
    assert_point(rig.point(left), [10.0, 20.0, 3.0]);
    assert_point(rig.point(right), [0.0, 0.0, 0.0]);
    assert_point(rig.point(lower), [0.0, 0.0, 0.0]);
}

#[test]
fn shift_tab_from_the_top_wraps_to_the_bottom_of_the_card() {
    let mut rig = Rig::new();
    let other = rig.op("Point", Point::new(40.0, 300.0));
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.shift_tab();
    rig.typed("9");
    rig.key(Named::Enter);
    assert_point(rig.point(id), [0.0, 0.0, 9.0]);
    assert_point(rig.point(other), [0.0, 0.0, 0.0]);
}

#[test]
fn tab_on_a_card_with_one_field_stays_in_it() {
    let mut rig = Rig::new();
    let id = rig.op("Number", Point::new(40.0, 40.0));
    let other = rig.op("Number", Point::new(40.0, 300.0));
    let value = rig.field(id, "value");
    rig.click(value);
    rig.typed("4");
    rig.key(Named::Tab);
    assert_eq!(rig.number(id, "value"), 4.0);
    rig.typed("5");
    rig.key(Named::Enter);
    assert_eq!(rig.number(id, "value"), 5.0);
    assert_eq!(rig.number(other, "value"), 0.0);
}

#[test]
fn shift_tab_goes_back() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let z = rig.field(id, "z");
    rig.click(z);
    rig.typed("3");
    rig.shift_tab();
    rig.typed("2");
    rig.shift_tab();
    rig.typed("1");
    rig.key(Named::Enter);
    assert_point(rig.point(id), [1.0, 2.0, 3.0]);
}

#[test]
fn tab_skips_linked_inputs() {
    let mut rig = Rig::new();
    let number = rig.op("Number", Point::new(40.0, 40.0));
    let point = rig.op("Point", Point::new(40.0, 200.0));
    rig.link(number, "value", point, "x");
    let number_field = rig.field(number, "value");
    rig.click(number_field);
    rig.typed("6");
    rig.key(Named::Enter);
    let y = rig.field(point, "y");
    rig.click(y);
    rig.typed("7");
    rig.key(Named::Tab);
    rig.typed("8");
    rig.key(Named::Tab);
    // From z the card wraps past the linked x, back to y.
    rig.typed("9");
    rig.key(Named::Enter);
    assert_point(rig.point(point), [6.0, 9.0, 8.0]);
}

#[test]
fn tab_skips_folded_coordinates_until_they_open() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    // Folded: from the group row, the next stop is not a component.
    let group = rig.field(id, LOCATION);
    rig.click(group);
    rig.typed("(1, 2, 3)");
    rig.key(Named::Tab);
    assert_point(rig.location(id), [1.0, 2.0, 3.0]);

    rig.open_coordinates(id);
    let group = rig.field(id, LOCATION);
    rig.click(group);
    rig.key(Named::Tab);
    rig.typed("-4.5");
    rig.key(Named::Tab);
    assert_point(rig.location(id), [-4.5, 2.0, 3.0]);
}

#[test]
fn leaving_a_field_any_way_commits() {
    let leave: [(&str, fn(&mut Rig, NodeId)); 3] = [
        ("another field", |rig, id| {
            let y = rig.field(id, "y");
            rig.click(y);
        }),
        ("the canvas", |rig, _| rig.click(CANVAS)),
        ("a node header", |rig, id| {
            let header = rig.header(id);
            rig.click(header);
        }),
    ];
    for (name, leave) in leave {
        for (text, expected) in [("-12.75", -12.75), ("0.1", 0.1), ("1234.5678", 1234.5678)] {
            let mut rig = Rig::new();
            let id = rig.op("Point", Point::new(40.0, 40.0));
            let x = rig.field(id, "x");
            rig.click(x);
            rig.typed(text);
            leave(&mut rig, id);
            assert_eq!(rig.number(id, "x"), expected, "leaving by {name}");
            assert_eq!(rig.drafts(), 0, "leaving by {name}");
        }
    }
}

#[test]
fn clicking_inside_the_field_being_edited_keeps_the_edit_open() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.typed("5");
    rig.click(x);
    assert_eq!(rig.drafts(), 1, "still editing");
    assert_eq!(rig.number(id, "x"), 0.0, "not committed yet");
    rig.key(Named::Enter);
    assert_eq!(rig.number(id, "x"), 5.0);
}

#[test]
fn editing_x_then_clicking_y_is_ready_to_type_in_y() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let (x, y) = (rig.field(id, "x"), rig.field(id, "y"));
    rig.click(x);
    rig.typed("100");
    rig.click(y);
    assert_eq!(rig.number(id, "x"), 100.0, "x committed");
    // The 0 already there is replaced, not added to.
    rig.typed("10");
    rig.key(Named::Enter);
    assert_point(rig.point(id), [100.0, 10.0, 0.0]);
}

#[test]
fn leaving_without_an_edit_does_not_run_the_graph() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let (x, y) = (rig.field(id, "x"), rig.field(id, "y"));
    rig.mark_unrun(id);

    rig.click(x);
    rig.key(Named::Tab);
    rig.click(y);
    rig.click(CANVAS);
    rig.click(x);
    rig.key(Named::Enter);
    assert!(!rig.has_run(id), "untouched fields");

    // Typing the value it already has is no edit either.
    rig.click(x);
    rig.typed("0");
    rig.click(CANVAS);
    rig.click(x);
    rig.typed("0.0");
    rig.key(Named::Tab);
    assert!(!rig.has_run(id), "same value retyped");
    assert_eq!(rig.drafts(), 0);
}

#[test]
fn a_refused_entry_does_not_run_the_graph() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.mark_unrun(id);
    rig.click(x);
    rig.typed("abc");
    rig.click(CANVAS);
    assert!(!rig.has_run(id));
}

#[test]
fn an_object_field_left_without_an_edit_writes_nothing() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    rig.open_coordinates(id);
    let x = rig.field(id, "loc_x");
    rig.mark_unrun(id);
    rig.click(x);
    rig.key(Named::Tab);
    rig.click(CANVAS);
    assert!(!rig.has_run(id));
    assert_eq!(rig.param(id, "loc_x"), Value::Null);
}

#[test]
fn a_real_edit_runs_the_graph() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    rig.open_coordinates(id);
    let x = rig.field(id, "loc_x");
    rig.mark_unrun(id);
    rig.click(x);
    rig.typed("7.25");
    rig.click(CANVAS);
    assert!(rig.has_run(id));
    assert_point(rig.location(id), [7.25, 0.0, 0.0]);
}

#[test]
fn number_fields_refuse_text_as_it_is_typed() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.typed("abc");
    rig.click(CANVAS);
    assert_eq!(rig.number(id, "x"), 0.0);

    rig.click(x);
    rig.typed("1a2b.c5");
    rig.key(Named::Enter);
    assert_eq!(rig.number(id, "x"), 12.5, "only the letters are refused");

    rig.click(x);
    rig.typed("1-2");
    rig.key(Named::Enter);
    assert_eq!(rig.number(id, "x"), 12.0, "a minus only leads");
}

#[test]
fn an_unfinished_number_goes_back_to_the_value() {
    for unfinished in ["-", ".", "-.", "1e", "1e-"] {
        let mut rig = Rig::new();
        let id = rig.op("Point", Point::new(40.0, 40.0));
        let x = rig.field(id, "x");
        rig.click(x);
        rig.typed("9");
        rig.key(Named::Enter);
        rig.click(CANVAS);
        rig.click(x);
        rig.typed(unfinished);
        rig.click(CANVAS);
        assert_eq!(rig.number(id, "x"), 9.0, "{unfinished}");
        assert_point(rig.point(id), [9.0, 0.0, 0.0]);
    }
}

#[test]
fn a_cleared_number_goes_back_to_the_value() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.typed("9");
    rig.key(Named::Enter);
    rig.click(CANVAS);
    rig.click(x);
    rig.key(Named::Backspace);
    rig.click(CANVAS);
    assert_eq!(rig.number(id, "x"), 9.0);
}

#[test]
fn numbers_are_stored_as_numbers() {
    let mut rig = Rig::new();
    let id = rig.op("Point", Point::new(40.0, 40.0));
    let x = rig.field(id, "x");
    rig.click(x);
    rig.typed("05");
    rig.key(Named::Enter);
    assert_eq!(rig.param(id, "x"), graph::value::number(5.0));
}

#[test]
fn object_number_fields_refuse_text_on_commit() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    rig.open_coordinates(id);
    let x = rig.field(id, "loc_x");
    rig.click(x);
    rig.typed("abc");
    rig.click(CANVAS);
    assert_eq!(rig.param(id, "loc_x"), Value::Null, "nothing stored");
    assert_eq!(rig.drafts(), 0);
    assert_point(rig.location(id), [0.0, 0.0, 0.0]);
}

#[test]
fn coordinate_groups_refuse_text_on_commit() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    let group = rig.field(id, LOCATION);
    rig.click(group);
    rig.typed("nowhere");
    rig.click(CANVAS);
    assert_eq!(rig.param(id, LOCATION), Value::Null);
    assert_point(rig.location(id), [0.0, 0.0, 0.0]);
}

#[test]
fn object_number_fields_take_what_the_panel_takes() {
    for (text, expected) in [
        ("2*3", 6.0),
        ("5'6\"", 66.0),
        ("-12.75", -12.75),
        ("-2*3.5", -7.0),
    ] {
        let mut rig = Rig::new();
        let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
        rig.open_coordinates(id);
        let x = rig.field(id, "loc_x");
        rig.click(x);
        rig.typed(text);
        rig.click(CANVAS);
        assert_point(rig.location(id), [expected, 0.0, 0.0]);
    }
}

#[test]
fn negative_numbers_reach_every_kind_of_number_field() {
    let mut rig = Rig::new();
    let op = rig.op("Point", Point::new(40.0, 40.0));
    let object = rig.object(ObjectKind::Point, Point::new(500.0, 40.0));

    let x = rig.field(op, "x");
    rig.click(x);
    rig.typed("-1");
    rig.key(Named::Enter);
    assert_eq!(rig.number(op, "x"), -1.0);

    let group = rig.field(object, LOCATION);
    rig.click(group);
    rig.typed("(-1.5, 2, -3)");
    rig.click(CANVAS);
    assert_point(rig.location(object), [-1.5, 2.0, -3.0]);

    rig.open_coordinates(object);
    let z = rig.field(object, "loc_z");
    rig.click(z);
    rig.typed("-0.25");
    rig.key(Named::Enter);
    assert_point(rig.location(object), [-1.5, 2.0, -0.25]);
}

#[test]
fn text_fields_still_take_text() {
    let mut rig = Rig::new();
    let id = rig.op("Text", Point::new(40.0, 40.0));
    let field = rig.app.graph_all_sections(rig.i)[0][0].rows[0]
        .field
        .clone();
    let at = rig.field(id, &field);
    rig.click(at);
    rig.typed("hello");
    rig.click(CANVAS);
    assert_eq!(rig.param(id, &field), Value::String("hello".to_owned()));
}

#[test]
fn every_number_row_accepts_its_own_display() {
    let mut rig = Rig::new();
    let mut refused = Vec::new();
    for kind in crate::ui::node_graph::object_kinds() {
        let Some(_) = kind.entity(&rig.app.tabs[rig.i].scene.document) else {
            continue;
        };
        let id = rig.object(*kind, Point::new(40.0, 40.0));
        let index = rig.app.tabs[rig.i]
            .graph
            .engine
            .nodes
            .iter()
            .position(|node| node.id == id)
            .expect("node");
        for section in &rig.app.graph_all_sections(rig.i)[index] {
            for row in &section.rows {
                let shown = crate::ui::node_graph::display(&row.value);
                if matches!(row.widget, RowWidget::Measure) && !is_measure(&shown) {
                    refused.push(format!("{}.{}={shown}", kind.name(), row.field));
                }
            }
        }
    }
    assert!(refused.is_empty(), "{refused:#?}");
}

#[test]
fn card_controls_show_the_arrow() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    rig.op("Boolean", Point::new(500.0, 40.0));
    let pos = Point::new(40.0, 40.0);

    let close = Point::new(pos.x + 280.0 - 13.0, pos.y + 13.0);
    assert_eq!(
        rig.cursor_at(close),
        mouse::Interaction::Idle,
        "close button"
    );
    // The object row, then the first section's toggle button.
    let section = Point::new(pos.x + 140.0, pos.y + 26.0 + 22.0 + 11.0);
    assert_eq!(
        rig.cursor_at(section),
        mouse::Interaction::Idle,
        "section toggle"
    );
    let toggle = Point::new(500.0 + 14.0 + 2.0 + 96.0 + 2.0 + 7.0, 40.0 + 26.0 + 11.0);
    assert_eq!(rig.cursor_at(toggle), mouse::Interaction::Idle, "checkbox");

    // A text field keeps its own cursor.
    let field = rig.field(id, LOCATION);
    assert_eq!(rig.cursor_at(field), mouse::Interaction::Text);
}

#[test]
fn sliders_show_the_arrow() {
    let mut rig = Rig::new();
    rig.op("Number Slider", Point::new(40.0, 40.0));
    let slider = Point::new(40.0 + 170.0, 40.0 + 26.0 + 11.0);
    assert_eq!(rig.cursor_at(slider), mouse::Interaction::Idle, "hovering");
    rig.send(Event::Mouse(mouse::Event::ButtonPressed(Button::Left)));
    assert_eq!(rig.cursor_at(slider), mouse::Interaction::Idle, "dragging");
}

/// What `corner` on a Rectangle holds after typing `text` and leaving.
fn rectangle_corner(text: &str) -> (Value, Value) {
    let mut rig = Rig::new();
    let id = rig.op("Rectangle", Point::new(40.0, 40.0));
    let corner = rig.field(id, "corner");
    rig.click(corner);
    rig.typed(text);
    rig.click(CANVAS);
    assert_eq!(rig.drafts(), 0);
    let node = rig.app.tabs[rig.i].graph.engine.node(id).expect("node");
    (rig.param(id, "corner"), node.outputs["object"].clone())
}

#[test]
fn coordinates_are_taken_with_or_without_parentheses() {
    for text in ["0,0,0", "(0,0,0)", "( 0, 0, 0 )"] {
        let (corner, object) = rectangle_corner(text);
        assert_eq!(corner, graph::value::point_value([0.0, 0.0, 0.0]), "{text}");
        assert!(ops::handle(&object).is_some(), "{text} makes the rectangle");
    }
}

#[test]
fn coordinates_show_formatted_after_commit() {
    for (text, shown) in [
        ("1.5,-2,3", "(1.5, -2, 3)"),
        ("(-12.75,0.25,-3)", "(-12.75, 0.25, -3)"),
        ("4,5", "(4, 5, 0)"),
        ("2*3,1,0", "(6, 1, 0)"),
    ] {
        let (corner, _) = rectangle_corner(text);
        assert_eq!(crate::ui::node_graph::display(&corner), shown, "{text}");
    }
}

#[test]
fn coordinates_refuse_anything_else() {
    for text in ["abc", "1,2,3,4", "1,,3", "(1,x,3)"] {
        let (corner, object) = rectangle_corner(text);
        assert_eq!(corner, Value::Null, "{text}");
        assert!(ops::handle(&object).is_none(), "{text}");
    }
}

#[test]
fn a_coordinate_can_be_cleared() {
    let mut rig = Rig::new();
    let id = rig.op("Rectangle", Point::new(40.0, 40.0));
    let corner = rig.field(id, "corner");
    rig.click(corner);
    rig.typed("1,2,3");
    rig.key(Named::Enter);
    rig.click(CANVAS);
    rig.click(corner);
    rig.key(Named::Backspace);
    rig.click(CANVAS);
    assert_eq!(rig.param(id, "corner"), Value::Null);
}

#[test]
fn retyping_a_coordinate_does_not_run_the_graph() {
    let mut rig = Rig::new();
    let id = rig.op("Rectangle", Point::new(40.0, 40.0));
    let corner = rig.field(id, "corner");
    rig.click(corner);
    rig.typed("1,2,3");
    rig.key(Named::Enter);
    rig.click(CANVAS);
    rig.mark_unrun(id);
    rig.click(corner);
    rig.typed("(1, 2, 3)");
    rig.click(CANVAS);
    assert!(!rig.has_run(id));
}

#[test]
fn coordinate_group_rows_show_formatted_after_commit() {
    let mut rig = Rig::new();
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    let group = rig.field(id, LOCATION);
    rig.click(group);
    rig.typed("1,2,3");
    rig.click(CANVAS);
    assert_eq!(
        rig.param(id, LOCATION),
        graph::value::point_value([1.0, 2.0, 3.0])
    );
    assert_point(rig.location(id), [1.0, 2.0, 3.0]);
}

#[test]
fn coordinate_inputs_are_told_from_objects() {
    let widget = |node: &str, field: &str| {
        let mut rig = Rig::new();
        rig.op(node, Point::new(40.0, 40.0));
        let sections = rig.app.graph_all_sections(rig.i);
        let found = sections[0][0].rows.iter().find(|row| row.field == field);
        matches!(found.expect("row").widget, RowWidget::Coordinate)
    };
    for (node, field) in [
        ("Rectangle", "corner"),
        ("Distance", "a"),
        ("Aligned Dimension", "first"),
        ("Fillet", "first point"),
        ("Move", "vector"),
    ] {
        assert!(widget(node, field), "{node}.{field}");
    }
    for (node, field) in [
        ("Fillet", "first"),
        ("Intersect", "a"),
        ("Equal", "a"),
        ("Move", "object"),
    ] {
        assert!(!widget(node, field), "{node}.{field}");
    }
}

/// A Point op at (5, 6, 7) to link from.
fn sender(rig: &mut Rig) -> NodeId {
    let id = rig.op("Point", Point::new(40.0, 400.0));
    for (field, value) in [("x", 5.0), ("y", 6.0), ("z", 7.0)] {
        let node = rig.app.tabs[rig.i].graph.engine.node_mut(id).expect("node");
        node.params
            .insert(field.to_owned(), graph::value::number(value));
    }
    let _ = rig.app.graph_run(rig.i);
    id
}

#[test]
fn a_linked_coordinate_field_cannot_be_typed_in() {
    let mut rig = Rig::new();
    let from = sender(&mut rig);
    let id = rig.op("Rectangle", Point::new(40.0, 40.0));
    let corner = rig.field(id, "corner");
    rig.click(corner);
    rig.typed("1,2,3");
    rig.key(Named::Enter);
    rig.link(from, "point", id, "corner");
    rig.click(corner);
    rig.typed("9,9,9");
    rig.click(CANVAS);
    assert_eq!(rig.drafts(), 0, "no text field to type in");
    let row_value = |rig: &Rig| {
        let sections = rig.app.graph_all_sections(rig.i);
        let index = rig.app.tabs[rig.i]
            .graph
            .engine
            .nodes
            .iter()
            .position(|node| node.id == id)
            .expect("node");
        let row = sections[index][0]
            .rows
            .iter()
            .find(|row| row.field == "corner");
        row.expect("row").value.clone()
    };
    assert_eq!(row_value(&rig), graph::value::point_value([5.0, 6.0, 7.0]));
}

#[test]
fn a_linked_group_locks_its_components_and_wins() {
    let mut rig = Rig::new();
    let from = sender(&mut rig);
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    rig.open_coordinates(id);
    let x = rig.field(id, "loc_x");
    rig.click(x);
    rig.typed("1");
    rig.key(Named::Enter);
    rig.click(CANVAS);
    rig.link(from, "point", id, LOCATION);
    assert_point(rig.location(id), [5.0, 6.0, 7.0]);

    rig.click(x);
    rig.typed("9");
    rig.click(CANVAS);
    assert_eq!(rig.drafts(), 0, "the component is read-only");
    assert_point(rig.location(id), [5.0, 6.0, 7.0]);
}

#[test]
fn a_linked_component_locks_its_group_and_keeps_its_siblings() {
    let mut rig = Rig::new();
    let number = rig.op("Number", Point::new(40.0, 400.0));
    let node = rig.app.tabs[rig.i]
        .graph
        .engine
        .node_mut(number)
        .expect("node");
    node.params
        .insert("value".to_owned(), graph::value::number(8.0));
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    let group = rig.field(id, LOCATION);
    rig.click(group);
    rig.typed("1,2,3");
    rig.click(CANVAS);
    rig.link(number, "value", id, "loc_x");
    assert_point(rig.location(id), [8.0, 2.0, 3.0]);

    rig.click(group);
    rig.typed("9,9,9");
    rig.click(CANVAS);
    assert_eq!(rig.drafts(), 0, "the group is read-only");
    assert_point(rig.location(id), [8.0, 2.0, 3.0]);

    rig.open_coordinates(id);
    let y = rig.field(id, "loc_y");
    rig.click(y);
    rig.typed("4");
    rig.click(CANVAS);
    assert_point(rig.location(id), [8.0, 4.0, 3.0]);
}

#[test]
fn tab_skips_every_field_a_link_supplies() {
    let mut rig = Rig::new();
    let from = sender(&mut rig);
    let id = rig.object(ObjectKind::Point, Point::new(40.0, 40.0));
    rig.open_coordinates(id);
    rig.link(from, "point", id, LOCATION);
    let order = rig.app.tabs[rig.i]
        .graph
        .tab_order(&rig.app.graph_all_sections(rig.i));
    assert!(
        !order
            .iter()
            .any(|port| port.node == id && port.field.starts_with("loc_")),
        "{order:?}"
    );
}

#[test]
fn removing_a_link_makes_the_field_editable_again() {
    let mut rig = Rig::new();
    let from = sender(&mut rig);
    let id = rig.op("Rectangle", Point::new(40.0, 40.0));
    rig.link(from, "point", id, "corner");
    assert!(rig.app.tabs[rig.i]
        .graph
        .engine
        .disconnect(&port(id, "corner"))
        .is_some());
    let _ = rig.app.graph_run(rig.i);
    let corner = rig.field(id, "corner");
    rig.click(corner);
    rig.typed("1,2,3");
    rig.click(CANVAS);
    assert_eq!(
        rig.param(id, "corner"),
        graph::value::point_value([1.0, 2.0, 3.0])
    );
}
