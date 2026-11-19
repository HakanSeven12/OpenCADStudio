//! The Node Graph window's Open parses its file on a worker.
use crate::app::{Message, OpenCADStudio};
use crate::ui::node_graph::{GraphMsg, PaletteItem};

fn fresh_drawing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let response = app.automation_op(r#"{"op":"new"}"#);
    assert_eq!(response["ok"], true, "{response}");
    app
}

fn nodes(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].graph.engine.nodes.len()
}

/// The bytes of a graph file with one node; the graph is cleared after.
fn graph_bytes(app: &mut OpenCADStudio) -> Vec<u8> {
    let i = app.active_tab;
    let spec = super::ops::resolve("Read CSV").expect("node type");
    app.tabs[i]
        .graph
        .add_node(PaletteItem::Op(spec), Vec::new(), iced::Point::ORIGIN);
    let bytes = app.graph_file_bytes(i);
    let _ = app.on_graph(GraphMsg::New);
    assert_eq!(nodes(app), 0);
    bytes
}

/// The file is parsed on a worker: nothing is loaded until the parse ends.
#[test]
fn an_open_parses_its_file_on_a_worker() {
    let mut app = fresh_drawing();
    let bytes = graph_bytes(&mut app);
    let task = app.on_graph(GraphMsg::Loaded(Some(bytes)));
    assert_eq!(nodes(&app), 0, "nothing is loaded during the parse");
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(message, Message::Graph(GraphMsg::Parsed(_, _, Ok(_)))),
        "{message:?}"
    );
    let _ = app.update(message);
    assert_eq!(nodes(&app), 1);
}

/// A parse that ends after its drawing was replaced loads nothing.
#[test]
fn a_parse_for_a_replaced_drawing_is_dropped() {
    let mut app = fresh_drawing();
    let bytes = graph_bytes(&mut app);
    let task = app.on_graph(GraphMsg::Loaded(Some(bytes)));
    let i = app.active_tab;
    app.tabs[i].document_generation += 1;
    let _ = app.update(OpenCADStudio::first_task_message(task));
    assert_eq!(nodes(&app), 0);
}

/// A file that is no graph is reported, as before; nothing is loaded.
#[test]
fn a_file_that_is_no_graph_is_reported() {
    let mut app = fresh_drawing();
    let errors = app.command_line.error_revision;
    let task = app.on_graph(GraphMsg::Loaded(Some(b"not a graph".to_vec())));
    let _ = app.update(OpenCADStudio::first_task_message(task));
    assert!(
        app.command_line.error_revision > errors,
        "the error is shown"
    );
    assert_eq!(nodes(&app), 0);
}

/// A parser panic is caught on the worker: nothing is loaded, and the app
/// goes on.
#[test]
fn a_parser_panic_keeps_the_app() {
    let mut app = fresh_drawing();
    let i = app.active_tab;
    let task = app.graph_parse_with(i, Vec::new(), |_| panic!("a parser bug"));
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(message, Message::BackgroundTaskFailed { .. }),
        "{message:?}"
    );
    let _ = app.update(message);
    assert_eq!(nodes(&app), 0);
}
