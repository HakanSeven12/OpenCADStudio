//! CUTBASE: the base point it puts on the clipboard, its one undo step,
//! locked layers, and its key and menu entries.

use crate::app::{Message, OpenCADStudio};
use document::entities::Line;
use document::types::Vector3;
use document::{EntityType, Handle};

fn new_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

/// Two lines off the axes, so a wrong base point or offset shows.
fn add_lines(app: &mut OpenCADStudio) -> Vec<Handle> {
    let i = app.active_tab;
    [
        (Vector3::new(1.25, 2.5, 0.0), Vector3::new(7.75, 4.125, 0.0)),
        (Vector3::new(-3.5, 6.0, 0.0), Vector3::new(2.0, 9.375, 0.0)),
    ]
    .into_iter()
    .map(|(start, end)| {
        app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(start, end)))
    })
    .collect()
}

fn select(app: &mut OpenCADStudio, handles: &[Handle]) {
    let i = app.active_tab;
    for &handle in handles {
        app.tabs[i].scene.select_entity(handle, false);
    }
}

fn type_line(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn exists(app: &OpenCADStudio, handle: Handle) -> bool {
    app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(handle)
        .is_some()
}

fn active_name(app: &OpenCADStudio) -> Option<&'static str> {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .map(|command| command.name())
}

fn out(app: &OpenCADStudio) -> String {
    app.command_line.history_plain_text()
}

/// The start of each line in the drawing, sorted.
fn line_starts(app: &OpenCADStudio) -> Vec<(f64, f64)> {
    let mut starts: Vec<(f64, f64)> = app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Line(line) => Some((line.start.x, line.start.y)),
            _ => None,
        })
        .collect();
    starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    starts
}

#[test]
fn cutbase_copies_with_the_picked_base_point_and_erases() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    select(&mut app, &lines);

    type_line(&mut app, "CUTBASE");
    assert_eq!(active_name(&app), Some("CUTBASE"), "the base point prompt");
    assert!(
        out(&app).contains("CUTBASE  Specify base point:"),
        "{}",
        out(&app)
    );
    assert!(
        exists(&app, lines[0]),
        "nothing is cut before the base point"
    );
    type_line(&mut app, "3.5,-2.25");

    assert_eq!(app.clipboard.len(), 2);
    assert_eq!(app.clipboard_base, glam::DVec3::new(3.5, -2.25, 0.0));
    assert!(!exists(&app, lines[0]) && !exists(&app, lines[1]));
    assert!(app.tabs[app.active_tab]
        .scene
        .selected_entities()
        .is_empty());
    assert!(app.tabs[app.active_tab].dirty);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!(
        out(&app).contains("2 object(s) cut to clipboard (base 3.500,-2.250)."),
        "{}",
        out(&app)
    );
}

#[test]
fn cutbase_without_a_selection_asks_for_the_base_point_first() {
    let mut app = new_app();
    let lines = add_lines(&mut app);

    type_line(&mut app, "CUTBASE");
    assert_eq!(active_name(&app), Some("CUTBASE"), "the base point first");
    type_line(&mut app, "-1.5,4.75");
    let i = app.active_tab;
    assert!(
        app.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.is_selection_gathering()),
        "then Select objects"
    );
    assert!(
        out(&app).contains("CUTBASE  Select objects:"),
        "{}",
        out(&app)
    );
    assert!(
        exists(&app, lines[0]),
        "nothing is cut before the selection"
    );
    type_line(&mut app, "ALL");
    type_line(&mut app, "");

    assert_eq!(app.clipboard.len(), 2);
    assert_eq!(app.clipboard_base, glam::DVec3::new(-1.5, 4.75, 0.0));
    assert!(!exists(&app, lines[0]) && !exists(&app, lines[1]));
    assert!(app.tabs[i].active_cmd.is_none());
}

#[test]
fn enter_with_no_objects_cuts_nothing() {
    let mut app = new_app();
    let lines = add_lines(&mut app);

    type_line(&mut app, "CUTBASE");
    type_line(&mut app, "-1.5,4.75");
    assert!(
        app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.is_selection_gathering()),
        "Select objects is open"
    );
    type_line(&mut app, "");

    assert!(app.clipboard.is_empty());
    assert!(exists(&app, lines[0]) && exists(&app, lines[1]));
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!(!app.tabs[app.active_tab].dirty);
}

#[test]
fn one_undo_puts_back_every_cut_object() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    select(&mut app, &lines);
    type_line(&mut app, "CUTBASE");
    type_line(&mut app, "3.5,-2.25");
    assert!(!exists(&app, lines[0]) && !exists(&app, lines[1]));

    let _ = app.update(Message::Undo);
    assert!(
        exists(&app, lines[0]) && exists(&app, lines[1]),
        "one undo step restores both"
    );
    assert_eq!(app.clipboard.len(), 2, "the clipboard keeps the copy");

    let _ = app.update(Message::Redo);
    assert!(!exists(&app, lines[0]) && !exists(&app, lines[1]));
}

#[test]
fn cutbase_leaves_objects_on_locked_layers() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    let i = app.active_tab;
    let mut locked = document::tables::Layer::new("LOCKED");
    locked.flags.locked = true;
    let _ = app.tabs[i].scene.document.layers.add(locked);
    app.tabs[i]
        .scene
        .document
        .get_entity_mut(lines[1])
        .unwrap()
        .common_mut()
        .layer = "LOCKED".to_string();
    select(&mut app, &lines);

    type_line(&mut app, "CUTBASE");
    type_line(&mut app, "3.5,-2.25");

    assert_eq!(app.clipboard.len(), 1, "only the unlocked line is cut");
    assert!(!exists(&app, lines[0]));
    assert!(exists(&app, lines[1]), "the locked line stays");
}

#[test]
fn a_paste_after_cutbase_puts_the_base_point_at_the_picked_point() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    select(&mut app, &lines);
    type_line(&mut app, "CUTBASE");
    type_line(&mut app, "3.5,-2.25");
    assert!(line_starts(&app).is_empty());

    type_line(&mut app, "PASTECLIP");
    type_line(&mut app, "10.5,7.75");

    // Each start moves by (10.5, 7.75) − (3.5, −2.25) = (7, 10).
    let starts = line_starts(&app);
    let expected = [(-3.5 + 7.0, 6.0 + 10.0), (1.25 + 7.0, 2.5 + 10.0)];
    assert_eq!(starts.len(), 2, "{starts:?}");
    for (got, want) in starts.iter().zip(expected) {
        assert!(
            (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9,
            "{starts:?}"
        );
    }
}

#[test]
fn copybase_still_keeps_the_objects() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    select(&mut app, &lines);

    type_line(&mut app, "COPYBASE");
    assert_eq!(active_name(&app), Some("COPYBASE"));
    assert!(
        out(&app).contains("COPYBASE  Specify base point:"),
        "{}",
        out(&app)
    );
    type_line(&mut app, "3.5,-2.25");

    assert_eq!(app.clipboard.len(), 2);
    assert_eq!(app.clipboard_base, glam::DVec3::new(3.5, -2.25, 0.0));
    assert!(exists(&app, lines[0]) && exists(&app, lines[1]));
    assert!(!app.tabs[app.active_tab].dirty, "a copy changes nothing");
    assert!(
        out(&app).contains("2 object(s) copied to clipboard (base 3.500,-2.250)."),
        "{}",
        out(&app)
    );
}

#[test]
fn ctrl_shift_x_runs_cutbase() {
    let bindings = crate::app::shortcuts::default_bindings();
    let accel = if cfg!(target_os = "macos") {
        "CMD"
    } else {
        "CTRL"
    };
    assert_eq!(
        bindings
            .get(&format!("{accel}+SHIFT+X"))
            .map(String::as_str),
        Some("CUTBASE")
    );
}

/// CUTCLIP shares the cut with CUTBASE: the lower-left corner is its base
/// point, one undo step puts the objects back, and a locked layer stays.
#[test]
fn cutclip_still_cuts_from_the_lower_left_corner_in_one_undo_step() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    let i = app.active_tab;
    let extra = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(4.0, -1.5, 0.0),
            Vector3::new(6.5, 0.75, 0.0),
        )));
    let mut locked = document::tables::Layer::new("LOCKED");
    locked.flags.locked = true;
    let _ = app.tabs[i].scene.document.layers.add(locked);
    app.tabs[i]
        .scene
        .document
        .get_entity_mut(extra)
        .unwrap()
        .common_mut()
        .layer = "LOCKED".to_string();
    select(&mut app, &[lines[0], lines[1], extra]);

    type_line(&mut app, "CUTCLIP");

    assert_eq!(app.clipboard.len(), 2, "the locked line is not cut");
    // The lower-left corner of the two cut lines, not of the locked one.
    assert_eq!(app.clipboard_base, glam::DVec3::new(-3.5, 2.5, 0.0));
    assert!(!exists(&app, lines[0]) && !exists(&app, lines[1]));
    assert!(exists(&app, extra));
    assert!(
        out(&app).contains("2 object(s) cut to clipboard."),
        "{}",
        out(&app)
    );

    let _ = app.update(Message::Undo);
    assert!(exists(&app, lines[0]) && exists(&app, lines[1]));
}

/// The point token takes three numbers and a selection; anything else
/// leaves the clipboard and the drawing as they were.
#[test]
fn a_point_token_with_no_point_or_no_objects_does_nothing() {
    let mut app = new_app();
    let lines = add_lines(&mut app);
    select(&mut app, &lines[..1]);
    let _ = app.dispatch_command("COPYCLIP");
    let base = app.clipboard_base;
    app.tabs[app.active_tab].scene.deselect_all();

    let _ = app.dispatch_command("COPYBASE_AT 1 2 3");
    assert_eq!(app.clipboard.len(), 1, "no selection: nothing copied");
    assert_eq!(app.clipboard_base, base);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());

    select(&mut app, &lines);
    for token in ["COPYBASE_AT 1 2", "CUTBASE_AT 1 x 3"] {
        let _ = app.dispatch_command(token);
        assert_eq!(app.clipboard.len(), 1, "{token}: nothing copied");
        assert_eq!(app.clipboard_base, base, "{token}");
        assert!(exists(&app, lines[0]) && exists(&app, lines[1]), "{token}");
        assert!(app.tabs[app.active_tab].active_cmd.is_none(), "{token}");
    }
}
