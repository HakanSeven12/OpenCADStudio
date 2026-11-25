//! A click on the grid or snap control of the status bar while the Drafting
//! Settings dialog is open changes the setting and shows in the dialog, and
//! OK does not set the old state back.

use super::*;
use crate::app::ModalKind;
use crate::ui::window::drafting_settings::DraftingSettingsTab;

/// The dialog open as the app opens it: a tool window, with the keyboard.
fn app_with_open_dialog() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.snapper.grid_snap_on = false;
    app.show_grid = false;
    let _ = app.update_root(Message::OpenDraftingSettings(
        DraftingSettingsTab::SnapAndGrid,
    ));
    assert!(app.tool_open(ModalKind::DraftingSettings));
    assert_eq!(app.keyboard_tool, Some(ModalKind::DraftingSettings));
    app
}

fn in_dialog(app: &mut OpenCADStudio, msg: Message) {
    let _ = app.update_root(Message::ToolWindow(
        ModalKind::DraftingSettings,
        Box::new(msg),
    ));
}

fn checkboxes(app: &OpenCADStudio) -> (bool, bool) {
    let state = app.drafting_settings_state.as_ref().expect("the dialog");
    (state.grid_on, state.snap_on)
}

#[test]
fn a_snap_click_while_the_dialog_has_the_keyboard_shows_in_it() {
    let mut app = app_with_open_dialog();
    let _ = app.update_root(Message::ToggleGridSnap);
    assert!(app.snapper.grid_snap(), "the click turns the snap on");
    assert_eq!(checkboxes(&app), (false, true), "the checkbox follows");
    assert!(
        !app.drafting_settings_dirty(),
        "nothing to confirm on close"
    );

    in_dialog(&mut app, Message::DraftingSettingsOk);
    assert!(!app.tool_open(ModalKind::DraftingSettings));
    assert!(app.snapper.grid_snap(), "OK keeps the snap on");
}

#[test]
fn a_grid_click_while_the_dialog_has_the_keyboard_shows_in_it() {
    let mut app = app_with_open_dialog();
    let _ = app.update_root(Message::ToggleGrid);
    assert!(app.show_grid, "the click turns the grid on");
    assert_eq!(checkboxes(&app), (true, false));

    in_dialog(&mut app, Message::DraftingSettingsOk);
    assert!(app.show_grid, "OK keeps the grid on");
}

#[test]
fn a_snap_click_keeps_a_change_made_in_the_dialog() {
    let mut app = app_with_open_dialog();
    in_dialog(&mut app, Message::DraftingSettingsToggleGrid);
    let _ = app.update_root(Message::ToggleGridSnap);
    assert_eq!(checkboxes(&app), (true, true));
    assert!(
        app.drafting_settings_dirty(),
        "the grid change is still to apply"
    );

    in_dialog(&mut app, Message::DraftingSettingsOk);
    assert!(app.show_grid && app.snapper.grid_snap());
}

#[test]
fn f9_while_the_dialog_has_the_keyboard_does_not_toggle_the_snap() {
    let mut app = app_with_open_dialog();
    let task = app.update_root(Message::ShortcutPressed("F9".into()));
    app.drive_headless_task(task).unwrap();
    assert!(!app.snapper.grid_snap(), "the key stays with the dialog");
    assert!(app.tool_open(ModalKind::DraftingSettings));

    // With the keyboard back on the drawing, F9 toggles the snap.
    let _ = app.update_root(Message::ViewportLeftPress);
    let task = app.update_root(Message::ShortcutPressed("F9".into()));
    app.drive_headless_task(task).unwrap();
    assert!(app.snapper.grid_snap());
}
