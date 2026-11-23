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

mod palette {
    use super::*;
    use crate::scene::bricscad_parameters::author_tests::LINES;
    use crate::ui::window::bricscad_visibility::VisMsg;

    fn send(app: &mut OpenCADStudio, message: VisMsg) {
        let _ = app.update(Message::BricscadVisibility(message));
    }

    fn states(app: &OpenCADStudio) -> Vec<(String, Vec<String>, usize)> {
        let i = app.active_tab;
        let model = app
            .bricscad_visibility_model(&app.tabs[i])
            .expect("model space");
        model
            .parameters
            .as_ref()
            .expect("the parameters can be edited")
            .iter()
            .map(|p| (p.name.clone(), p.states.clone(), p.current))
            .collect()
    }

    fn shown_in(app: &OpenCADStudio, parameter: &str, state: &str) -> Vec<u64> {
        let i = app.active_tab;
        let model = app.bricscad_visibility_model(&app.tabs[i]).unwrap();
        let p = model
            .parameters
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.name == parameter)
            .unwrap()
            .clone();
        let s = p.states.iter().position(|x| x == state).unwrap();
        p.objects
            .iter()
            .filter(|(_, pattern)| pattern[s])
            .map(|(h, _)| h.value())
            .collect()
    }

    #[test]
    fn the_palette_switches_moves_adds_and_each_step_undoes() {
        let mut app = opened("authoring-select.dwg");
        let i = app.active_tab;
        let _ = app.dispatch_command("VISIBILITYSTATESPANELOPEN");
        assert!(app.bricscad_visibility.show);
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            states(&app)[0],
            ("P1".into(), s(&["S1", "S2", "S3", "S4"]), 2)
        );

        // A click on a state shows it.
        send(&mut app, VisMsg::Select("P1".into(), "S1".into()));
        assert_eq!(states(&app)[0].2, 0);

        // Up and down: the order Properties lists them in.
        send(&mut app, VisMsg::Move("P1".into(), "S4".into(), true));
        assert_eq!(
            states(&app)[0],
            ("P1".into(), s(&["S1", "S2", "S4", "S3"]), 0)
        );
        let _ = app.update(Message::Undo);
        assert_eq!(states(&app)[0].1, s(&["S1", "S2", "S3", "S4"]));

        // A new state opens its page; a tick shows an object in it.
        send(&mut app, VisMsg::NewState("P1".into()));
        send(&mut app, VisMsg::NewStateName("Leer".into()));
        send(&mut app, VisMsg::NewStateCommit);
        assert_eq!(
            app.bricscad_visibility.page,
            Some(("P1".into(), "Leer".into()))
        );
        assert_eq!(shown_in(&app, "P1", "Leer"), Vec::<u64>::new());
        send(&mut app, VisMsg::Show(Handle::from(LINES[0]), true));
        assert_eq!(shown_in(&app, "P1", "Leer"), vec![LINES[0]]);

        // Add objects: picked in the drawing, they show in this state only.
        send(&mut app, VisMsg::AddObjects);
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "the palette asks for objects"
        );
        let _ = app.tabs[i]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(Handle::from(LINES[4]), glam::DVec3::ZERO);
        feed(&mut app, "");
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(shown_in(&app, "P1", "Leer"), vec![LINES[0], LINES[4]]);
        assert_eq!(shown_in(&app, "P1", "S1"), vec![LINES[0], LINES[1]]);

        // Removed from the parameter: no state rules it any more.
        send(&mut app, VisMsg::Remove(Handle::from(LINES[4])));
        assert_eq!(shown_in(&app, "P1", "Leer"), vec![LINES[0]]);
        let _ = app.update(Message::Undo);
        assert_eq!(shown_in(&app, "P1", "Leer"), vec![LINES[0], LINES[4]]);
    }

    #[test]
    fn a_new_parameter_starts_with_the_objects_picked() {
        let mut app = opened("authoring-start.dwg");
        let i = app.active_tab;
        let _ = app.dispatch_command("VISIBILITYSTATESPANELOPEN");
        assert_eq!(states(&app), vec![]);
        send(&mut app, VisMsg::NewParameter);
        send(&mut app, VisMsg::NewParameterName("LAMPE".into()));
        send(&mut app, VisMsg::NewParameterState("An".into()));
        send(&mut app, VisMsg::NewParameterPick);
        for handle in [LINES[1], LINES[2]] {
            let _ = app.tabs[i]
                .active_cmd
                .as_mut()
                .expect("the palette asks for objects")
                .on_entity_pick(Handle::from(handle), glam::DVec3::ZERO);
        }
        feed(&mut app, "");
        assert_eq!(states(&app), vec![("LAMPE".into(), vec!["An".into()], 0)]);
        assert_eq!(shown_in(&app, "LAMPE", "An"), vec![LINES[1], LINES[2]]);
        assert_eq!(
            app.bricscad_visibility.page,
            Some(("LAMPE".into(), "An".into()))
        );
    }

    #[test]
    fn outside_model_space_the_palette_shows_nothing_to_edit() {
        let mut app = opened("visibility-states.dwg");
        let i = app.active_tab;
        assert!(app.bricscad_visibility_model(&app.tabs[i]).is_some());
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", 0xEE));
        assert!(app.bricscad_visibility_model(&app.tabs[i]).is_none());
    }
}
