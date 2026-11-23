//! Work counts: a user step on a large drawing must not do work for each
//! entity of the drawing, and an event that changes no setting must not
//! write the settings file.
//!
//! A slower release was found by users three times (#2756, #3339, #3392),
//! because no test fails when a step does more work and gives the same
//! result. Times on a CI runner are not stable, so these tests count work:
//! each one does a step on a drawing of [`LINES`] lines and compares the
//! counters of the app with the limit of the step in [`LIMITS`].
//!
//! If a test here fails after your change, the step now does more work on
//! a large drawing. Find why, before you change a limit: a full build of
//! the wire set, or a tessellation context for each entity, is a stop that
//! the user sees on a drawing of 400,000 entities; a write of the settings
//! file for each pointer event made the app unusable (#3392). Raise a limit
//! only when
//! the new work is necessary, and say why in a comment on its line.

use crate::app::{Message, OpenCADStudio};
use crate::scene::view::camera::Camera;
use document::entities::Line;
use document::types::Vector3;
use document::{EntityType, Handle};

/// The size of the made drawing. The limits do not change with it: that is
/// what the tests show.
const LINES: usize = 5_000;

/// What a step did, or the most that it can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Work {
    /// The wire set of the drawing assembled whole.
    full_builds: u64,
    /// The kept wire set brought up to date for the changed entities only.
    patches: u64,
    /// Tessellation contexts resolved; each one scans the drawing's
    /// objects, so a pass over many entities takes one.
    contexts: u64,
    /// Writes of the settings file.
    settings_writes: u64,
}

/// The most work that each step can do, on a drawing of any size: full
/// builds, patches, contexts, settings writes.
const LIMITS: &[(&str, Work)] = &[
    ("add one line", limit(0, 1, 0, 0)),
    ("erase one line", limit(0, 1, 0, 0)),
    ("undo one erase", limit(0, 1, 0, 0)),
    ("move one line", limit(0, 1, 1, 0)),
    ("pointer move with no command", limit(0, 0, 0, 0)),
    ("pointer move in LINE", limit(0, 0, 0, 0)),
    ("make a layer", limit(0, 0, 0, 0)),
    ("zoom to the extents", limit(0, 0, 0, 0)),
    // The preview of the selected entities is one pass: one context, not
    // one for each entity (#3427).
    ("end the selection of all lines in MOVE", limit(0, 0, 1, 0)),
    ("pointer move in MOVE of all lines", limit(0, 0, 0, 0)),
    ("move all lines", limit(0, 1, 0, 0)),
    // One write for the new value, and none for the events after it (#3392).
    ("change a setting", limit(0, 0, 0, 1)),
    ("pointer move after a changed setting", limit(0, 0, 0, 0)),
];

const fn limit(full_builds: u64, patches: u64, contexts: u64, settings_writes: u64) -> Work {
    Work {
        full_builds,
        patches,
        contexts,
        settings_writes,
    }
}

/// An app with a drawing of [`LINES`] lines that was drawn one time.
fn large_drawing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    for i in 0..LINES {
        let x = (i % 100) as f64 * 10.0;
        let y = (i / 100) as f64 * 10.0;
        app.tabs[tab]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(x, y, 0.0),
                Vector3::new(x + 5.0, y + 5.0, 0.0),
            )));
    }
    let before = work(&app);
    assert_eq!(draw(&app), LINES, "one wire for each line");
    assert_eq!(
        since(&app, before).full_builds,
        1,
        "the first draw assembles the whole set, and the counter sees it"
    );
    app
}

/// Draws model space, as a frame does, and gives the number of wires.
fn draw(app: &OpenCADStudio) -> usize {
    let scene = &app.tabs[app.active_tab].scene;
    scene
        .model_tile_wires_arc(0, &Camera::default(), 1.0, 1.0)
        .len()
}

/// The counters now.
fn work(app: &OpenCADStudio) -> Work {
    let display = app.tabs[app.active_tab].scene.display_work();
    Work {
        full_builds: display.full_builds,
        patches: display.patches,
        contexts: display.contexts,
        settings_writes: crate::app::config::SAVES.with(|count| count.get()) as u64,
    }
}

fn since(app: &OpenCADStudio, before: Work) -> Work {
    let now = work(app);
    Work {
        full_builds: now.full_builds - before.full_builds,
        patches: now.patches - before.patches,
        contexts: now.contexts - before.contexts,
        settings_writes: now.settings_writes - before.settings_writes,
    }
}

fn script(app: &mut OpenCADStudio, line: &str) {
    let _ = app.update_root(Message::ScriptLine(line.to_string()));
}

/// One instalment of a command: the command stays at its next prompt.
fn script_part(app: &mut OpenCADStudio, line: &str) {
    let _ = app.run_command_line_streaming(line, false);
}

fn pointer_to(app: &mut OpenCADStudio, x: f32, y: f32) {
    let _ = app.update(Message::ViewportMove(iced::Point::new(x, y)));
}

fn entities(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].scene.document.entities().count()
}

fn prompt(app: &OpenCADStudio) -> String {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .map(|command| command.prompt())
        .unwrap_or_default()
}

/// The start point of the line `handle`.
fn start_of(app: &OpenCADStudio, handle: Handle) -> (f64, f64) {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Line(line)) => (line.start.x, line.start.y),
        _ => panic!("the line exists"),
    }
}

fn last_line(app: &OpenCADStudio) -> Handle {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .map(|entity| entity.common().handle)
        .max_by_key(|handle| handle.value())
        .expect("the drawing has entities")
}

/// Does `step`, draws one frame, and fails if the two together did more
/// work than the limit of `name`.
fn within_limit(app: &mut OpenCADStudio, name: &str, step: impl FnOnce(&mut OpenCADStudio)) {
    let (_, limit) = LIMITS
        .iter()
        .find(|(step_name, _)| *step_name == name)
        .unwrap_or_else(|| panic!("LIMITS has no line for the step \"{name}\""));
    let before = work(app);
    step(app);
    draw(app);
    let did = since(app, before);
    assert!(
        did.full_builds <= limit.full_builds
            && did.patches <= limit.patches
            && did.contexts <= limit.contexts
            && did.settings_writes <= limit.settings_writes,
        "The step \"{name}\" did more work than its limit, on a drawing of {LINES} lines:\n  \
         did:   {did:?}\n  limit: {limit:?}\n\
         A step must not do work for each entity of the drawing, and an event that changes \
         no setting must not write the settings file. If your change caused this, find why \
         the step now does more work. Raise the limit in LIMITS only \
         when the new work is necessary, with the reason in a comment on its line.",
    );
}

#[test]
fn each_limit_has_a_test() {
    let source = include_str!("work_count_tests.rs");
    for (name, _) in LIMITS {
        let uses = source.matches(&format!("\"{name}\"")).count();
        assert!(uses >= 2, "the step \"{name}\" has a limit and no test");
    }
}

#[test]
fn a_new_line_is_patched_into_the_display() {
    let mut app = large_drawing();
    within_limit(&mut app, "add one line", |app| {
        script(app, "LINE 0,0 10,0 ");
    });
    assert_eq!(entities(&app), LINES + 1);
    assert_eq!(draw(&app), LINES + 1, "the display shows the new line");
}

#[test]
fn an_erase_and_its_undo_are_patched_into_the_display() {
    let mut app = large_drawing();
    within_limit(&mut app, "erase one line", |app| {
        script(app, "ERASE L ");
    });
    assert_eq!(entities(&app), LINES - 1);

    within_limit(&mut app, "undo one erase", |app| {
        let _ = app.update(Message::Undo);
    });
    assert_eq!(entities(&app), LINES);
}

#[test]
fn a_move_of_one_line_is_patched_into_the_display() {
    let mut app = large_drawing();
    let line = last_line(&app);
    let (x, y) = start_of(&app, line);
    within_limit(&mut app, "move one line", |app| {
        script(app, "MOVE L  ");
        script(app, "0,0 ");
        script(app, "5,5 ");
    });
    assert_eq!(start_of(&app, line), (x + 5.0, y + 5.0), "the line moved");
}

#[test]
fn a_pointer_move_does_no_work() {
    let mut app = large_drawing();
    within_limit(&mut app, "pointer move with no command", |app| {
        pointer_to(app, 400.0, 300.0);
        pointer_to(app, 410.0, 305.0);
    });

    script_part(&mut app, "LINE 0,0");
    assert!(
        prompt(&app).contains("next point"),
        "LINE has its first point and waits for the next: {}",
        prompt(&app)
    );
    within_limit(&mut app, "pointer move in LINE", |app| {
        pointer_to(app, 420.0, 300.0);
        pointer_to(app, 430.0, 310.0);
    });
    assert_eq!(entities(&app), LINES, "the pointer moves added nothing");
}

#[test]
fn a_new_layer_and_a_zoom_do_no_work() {
    let mut app = large_drawing();
    within_limit(&mut app, "make a layer", |app| {
        script(app, "-LAYER M WORK  ");
    });
    assert!(
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get("WORK")
            .is_some(),
        "the layer exists"
    );

    within_limit(&mut app, "zoom to the extents", |app| {
        script(app, "ZOOM E ");
    });
}

#[test]
fn a_move_of_all_lines_does_no_work_for_each_line_until_its_second_point() {
    let mut app = large_drawing();
    let line = last_line(&app);
    let (x, y) = start_of(&app, line);

    within_limit(&mut app, "end the selection of all lines in MOVE", |app| {
        script(app, "MOVE ALL  ");
    });
    assert!(
        prompt(&app).contains("base point"),
        "MOVE has its selection and asks for the base point: {}",
        prompt(&app)
    );

    script(&mut app, "0,0 ");
    within_limit(&mut app, "pointer move in MOVE of all lines", |app| {
        pointer_to(app, 410.0, 300.0);
        pointer_to(app, 420.0, 310.0);
    });

    within_limit(&mut app, "move all lines", |app| {
        script(app, "5,5 ");
    });
    assert_eq!(start_of(&app, line), (x + 5.0, y + 5.0), "the lines moved");
    assert_eq!(draw(&app), LINES, "the display shows each line");
}

#[test]
fn a_changed_setting_is_written_one_time_and_not_again() {
    let mut app = large_drawing();
    let before = work(&app);
    within_limit(&mut app, "change a setting", |app| {
        script(app, "PICKBOX 7 ");
    });
    assert_eq!(
        since(&app, before).settings_writes,
        1,
        "the new value is written, and the counter sees it"
    );

    within_limit(&mut app, "pointer move after a changed setting", |app| {
        pointer_to(app, 400.0, 300.0);
        pointer_to(app, 410.0, 305.0);
    });
}
