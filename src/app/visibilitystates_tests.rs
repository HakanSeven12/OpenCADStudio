//! VISIBILITYSTATES typed at the command line on the BricsCAD drawing of five
//! lines in `tests/fixtures/bricscad` leaves what BricsCAD leaves, and each
//! step is one undo step.

use super::*;
use document::Handle;

fn opened(name: &str) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let path = format!(
        "{}/tests/fixtures/bricscad/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    app.automation_op(&serde_json::json!({"op": "open", "path": path}).to_string());
    app
}

fn feed(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

/// One run of the command: the lines by number are selected first, as an
/// operator picks them before or when the command asks.
fn visibility_states(
    app: &mut OpenCADStudio,
    parameter: &str,
    state: &str,
    action: &str,
    lines: &[usize],
) {
    let i = app.active_tab;
    app.tabs[i].scene.deselect_all();
    for (k, n) in lines.iter().enumerate() {
        let handle = Handle::from(crate::scene::bricscad_parameters::author_tests::LINES[n - 1]);
        app.tabs[i].scene.select_entity(handle, k == 0);
    }
    let _ = app.dispatch_command("VISIBILITYSTATES");
    feed(app, parameter);
    feed(app, state);
    feed(app, action);
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "{parameter} {state} {action} left a step open"
    );
}

#[test]
fn typed_at_the_command_line_the_run_leaves_what_bricscad_leaves() {
    let mut app = opened("authoring-start.dwg");
    for (parameter, state, action, lines) in [
        ("P1", "S1", "A", &[1, 2][..]),
        ("P1", "S2", "I", &[1]),
        ("P1", "S3", "I", &[2]),
        ("P1", "S2", "V", &[1]),
        ("P1", "S1", "A", &[3]),
        ("P1", "S1", "R", &[3]),
        ("P2", "Sichtbar", "A", &[4]),
        ("P2", "Unsichtbar", "I", &[4]),
        ("P2", "Sichtbar", "A", &[2]),
        ("P1", "S4", "V", &[1]),
        ("P1", "S2", "S", &[]),
        ("P1", "S3", "S", &[]),
    ] {
        visibility_states(&mut app, parameter, state, action, lines);
    }
    let ours = crate::scene::bricscad_parameters::author_tests::describe(
        &app.tabs[app.active_tab].scene.document,
    );
    let theirs = crate::scene::bricscad_parameters::author_tests::describe(
        &opened("authoring-select.dwg").tabs[0].scene.document,
    );
    // The test runs in English; BricsCAD named them in German.
    let ours: Vec<String> = ours
        .into_iter()
        .map(|line| {
            line.replace("Suppress_", "Unterdrücken_")
                .replace(
                    "controlling visibility of entities in ",
                    "Steuerung der Sichtbarkeit von Objekten in ",
                )
                .replace("Entity_", "Objekt_")
        })
        .collect();
    assert_eq!(ours, theirs);
}

#[test]
fn each_run_is_one_undo_step_and_a_refusal_changes_nothing() {
    let mut app = opened("authoring-start.dwg");
    let i = app.active_tab;
    let start = app.tabs[i].scene.document.clone();
    visibility_states(&mut app, "P1", "S1", "A", &[1, 2]);
    assert_ne!(app.tabs[i].scene.document, start);
    let _ = app.update(Message::Undo);
    assert!(
        crate::scene::bricscad_parameters::parameter_states(
            &app.tabs[i].scene.document,
            app.tabs[i].scene.document.header.model_space_block_handle
        )
        .is_empty(),
        "undo takes the parameter away"
    );
    let _ = app.update(Message::Redo);
    assert_eq!(
        crate::scene::bricscad_parameters::parameter_states(
            &app.tabs[i].scene.document,
            app.tabs[i].scene.document.header.model_space_block_handle
        ),
        [("P1".to_string(), vec!["S1".to_string()])],
        "redo brings it back"
    );
    let _ = app.update(Message::Undo);
    // A state to select that does not exist is refused, the drawing unchanged.
    let before = app.tabs[i].scene.document.clone();
    let undo = app.tabs[i].history.undo_stack.len();
    visibility_states(&mut app, "P9", "S1", "S", &[]);
    assert_eq!(app.tabs[i].history.undo_stack.len(), undo);
    assert!(app.tabs[i].scene.document == before);
}

#[test]
fn in_the_block_editor_the_command_changes_nothing() {
    let mut app = opened("visibility-states.dwg");
    let i = app.active_tab;
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", 0xEE));
    assert!(app.tabs[i].active_block_edit.is_some());
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("VISIBILITYSTATES");
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(app.tabs[i].scene.document == before);
}

#[test]
fn objects_picked_when_the_command_asks_are_the_ones_it_takes() {
    let mut app = opened("authoring-start.dwg");
    let i = app.active_tab;
    app.tabs[i].scene.deselect_all();
    let _ = app.dispatch_command("VISIBILITYSTATES");
    feed(&mut app, "P1");
    feed(&mut app, "S1");
    feed(&mut app, "A");
    let lines = crate::scene::bricscad_parameters::author_tests::LINES;
    for handle in [lines[0], lines[1]] {
        let command = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("the command asks for objects");
        let _ = command.on_entity_pick(Handle::from(handle), glam::DVec3::ZERO);
    }
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let mut scene = crate::scene::bricscad_parameters::author_tests::fixture("authoring-start.dwg");
    let model = scene.document.header.model_space_block_handle;
    scene
        .edit_bricscad_visibility(
            model,
            "P1",
            "S1",
            crate::scene::bricscad_parameters::Action::Add,
            &[Handle::from(lines[0]), Handle::from(lines[1])],
        )
        .unwrap();
    assert_eq!(
        crate::scene::bricscad_parameters::author_tests::describe(&app.tabs[i].scene.document),
        crate::scene::bricscad_parameters::author_tests::describe(&scene.document)
    );
}
