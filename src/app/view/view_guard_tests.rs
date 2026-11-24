//! A panic in the view of a window: the window shows a fallback with the
//! save buttons, the command line says what failed, and the view is not
//! built again until the user asks for it.

use super::TEST_VIEW_PANIC;
use crate::app::{Message, OpenCADStudio};

fn last_error(app: &OpenCADStudio) -> String {
    app.command_line
        .history
        .iter()
        .rev()
        .find(|entry| entry.text.contains("Internal error"))
        .map(|entry| entry.text.clone())
        .unwrap_or_default()
}

fn app_with_a_drawing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

#[test]
fn a_panic_in_a_view_shows_the_fallback_and_the_command_line_says_so() {
    let mut app = app_with_a_drawing();
    let window = iced::window::Id::unique();
    TEST_VIEW_PANIC.with(|panic| panic.set(true));
    let _ = app.view(window);
    assert!(
        app.window_view_failed(window),
        "the window shows the fallback"
    );
    let _ = app.update_root(Message::RetryWindowView(window));
    assert!(!app.window_view_failed(window), "Try again draws the view");
    let error = last_error(&app);
    assert!(
        error.contains(
            "Internal error while drawing: test panic in view. Save your work under a new name."
        ),
        "{error}"
    );
}

#[test]
fn a_failed_view_is_not_built_again_until_the_user_tries_again() {
    let mut app = app_with_a_drawing();
    let window = iced::window::Id::unique();
    TEST_VIEW_PANIC.with(|panic| panic.set(true));
    let _ = app.view(window);
    TEST_VIEW_PANIC.with(|panic| panic.set(true));
    let _ = app.view(window);
    assert!(
        TEST_VIEW_PANIC.with(|panic| panic.get()),
        "the fallback did not build the view again"
    );
    let _ = app.update_root(Message::RetryWindowView(window));
    let _ = app.view(window);
    assert!(
        !TEST_VIEW_PANIC.with(|panic| panic.get()),
        "the view was built again"
    );
    assert!(app.window_view_failed(window), "and it failed again");
    TEST_VIEW_PANIC.with(|panic| panic.set(false));
}

#[test]
fn a_view_that_does_not_panic_shows_the_window() {
    let app = app_with_a_drawing();
    let window = iced::window::Id::unique();
    let _ = app.view(window);
    assert!(!app.window_view_failed(window));
    assert_eq!(last_error(&app), "");
}
