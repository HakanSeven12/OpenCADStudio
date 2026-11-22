//! `update_guarded`: a panic in a message handler is caught, the command is
//! cancelled, its changes are taken back, and the session goes on.

use super::panic_guard::{TestPanicAt, CAUGHT_PANIC_LIMIT, TEST_PANIC};
use crate::app::{Message, OpenCADStudio};

/// An app with one drawing that holds one circle, made by a command, so
/// the undo stack has one record.
fn app_with_a_circle() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.update_root(Message::ScriptLine("CIRCLE 0,0 5".to_string()));
    assert_eq!(entities(&app), 1, "the circle");
    assert_eq!(undo_depth(&app), 1, "one undo record");
    app
}

fn entities(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].scene.document.entities().count()
}

fn undo_depth(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].history.undo_stack.len()
}

fn redo_depth(app: &OpenCADStudio) -> usize {
    app.tabs[app.active_tab].history.redo_stack.len()
}

/// The handles of the entities, in order: the drawing as the user sees it.
fn handles(app: &OpenCADStudio) -> Vec<u64> {
    let mut handles: Vec<u64> = app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .map(|entity| entity.common().handle.value())
        .collect();
    handles.sort_unstable();
    handles
}

fn last_error(app: &OpenCADStudio) -> String {
    app.command_line
        .history
        .iter()
        .rev()
        .find(|entry| entry.text.contains("Internal error"))
        .map(|entry| entry.text.clone())
        .unwrap_or_default()
}

/// Sends `LINE` with a panic at `at`, and returns the app after it and the
/// entity handles from before it.
fn line_with_a_panic(at: TestPanicAt) -> (OpenCADStudio, Vec<u64>) {
    let mut app = app_with_a_circle();
    let before = handles(&app);
    assert_eq!(before.len(), 1, "the circle's handle");
    TEST_PANIC.with(|point| point.set(Some(at)));
    let _ = app.update_root(Message::ScriptLine("LINE 0,0 10,0 ".to_string()));
    assert_eq!(
        TEST_PANIC.with(|point| point.get()),
        None,
        "the panic point was reached"
    );
    (app, before)
}

#[test]
fn a_panic_before_the_handler_changes_nothing_and_the_session_goes_on() {
    let (mut app, before) = line_with_a_panic(TestPanicAt::BeforeHandler);
    assert_eq!(handles(&app), before);
    assert_eq!(undo_depth(&app), 1);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    let error = last_error(&app);
    assert!(error.contains("test panic at BeforeHandler"), "{error}");
    assert!(error.contains("was cancelled."), "{error}");

    // The next command works, and the earlier undo record is still good.
    let _ = app.update_root(Message::ScriptLine("LINE 0,0 10,0 ".to_string()));
    assert_eq!(entities(&app), 2);
    app.undo_steps(2);
    assert_eq!(entities(&app), 0);
}

#[test]
fn a_panic_with_an_open_undo_record_takes_the_change_back() {
    let (mut app, before) = line_with_a_panic(TestPanicAt::BeforeMessageEnd);
    assert_eq!(handles(&app), before, "the line is gone");
    assert_eq!(undo_depth(&app), 1, "only the circle's record");
    assert_eq!(redo_depth(&app), 0, "REDO cannot bring the line back");
    let error = last_error(&app);
    assert!(error.contains("its changes were undone"), "{error}");

    // The circle's record still works.
    app.undo_steps(1);
    assert_eq!(entities(&app), 0);
    assert_eq!(redo_depth(&app), 1);
    app.redo_steps(1);
    assert_eq!(handles(&app), before);
}

#[test]
fn a_panic_after_the_handler_takes_its_complete_record_back() {
    let (app, before) = line_with_a_panic(TestPanicAt::AfterHandler);
    assert_eq!(handles(&app), before);
    assert_eq!(undo_depth(&app), 1);
    assert_eq!(redo_depth(&app), 0);
    assert!(last_error(&app).contains("its changes were undone"));
}

#[test]
fn a_handler_with_no_panic_is_not_changed_by_the_guard() {
    let mut app = app_with_a_circle();
    let _ = app.update_root(Message::ScriptLine("LINE 0,0 10,0 ".to_string()));
    assert_eq!(entities(&app), 2);
    assert_eq!(handles(&app).len(), 2);
    assert_eq!(undo_depth(&app), 2);
    assert!(last_error(&app).is_empty());
    assert_eq!(app.caught_panics, 0);
}

/// The handler made a different drawing active before the panic: its undo
/// records are not the failed command's, and stay.
#[test]
fn a_panic_after_a_tab_switch_takes_nothing_back_in_the_other_drawing() {
    let mut app = app_with_a_circle();
    let circle_tab = app.tabs[app.active_tab].id;
    let _ = app.update_root(Message::TabNew);
    assert_ne!(
        app.tabs[app.active_tab].id, circle_tab,
        "a second drawing is active"
    );
    assert_eq!(undo_depth(&app), 0);
    let with_circle = app
        .tabs
        .iter()
        .position(|tab| tab.id == circle_tab)
        .expect("the first drawing");

    TEST_PANIC.with(|point| point.set(Some(TestPanicAt::AfterHandler)));
    let _ = app.update_root(Message::TabSwitch(with_circle));
    assert_eq!(TEST_PANIC.with(|point| point.get()), None);
    assert_eq!(app.tabs[app.active_tab].id, circle_tab);
    assert_eq!(entities(&app), 1, "the circle stays");
    assert_eq!(undo_depth(&app), 1, "and its undo record");
    assert!(last_error(&app).contains("was cancelled."));
}

#[test]
fn after_the_limit_a_panic_is_not_caught() {
    let mut app = app_with_a_circle();
    for count in 1..=CAUGHT_PANIC_LIMIT {
        TEST_PANIC.with(|point| point.set(Some(TestPanicAt::BeforeHandler)));
        let _ = app.update_root(Message::Noop);
        assert_eq!(app.caught_panics, count);
    }
    TEST_PANIC.with(|point| point.set(Some(TestPanicAt::BeforeHandler)));
    let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = app.update_root(Message::Noop);
    }));
    assert!(escaped.is_err(), "the panic after the limit ends the app");
    assert_eq!(
        entities(&app),
        1,
        "the drawing is still there for the crash path"
    );
}
