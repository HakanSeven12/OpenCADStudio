//! The snap and grid spacings of the Drafting Settings dialog and GRIDUNIT
//! are shown and read in the drawing's units (LUNITS, LUPREC).

use super::*;
use crate::ui::window::drafting_settings::{DraftingSettingsState, DraftingSettingsTab};

/// A new drawing with the given LUNITS (2 decimal, 4 architectural) and
/// LUPREC.
fn app_with_units(lunits: i16, luprec: i16) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let header = &mut app.tabs[i].scene.document.header;
    header.linear_unit_format = lunits;
    header.linear_unit_precision = luprec;
    app
}

fn open_dialog(app: &mut OpenCADStudio) -> DraftingSettingsState {
    let _ = app.update(Message::OpenDraftingSettings(
        DraftingSettingsTab::SnapAndGrid,
    ));
    app.drafting_settings_state
        .clone()
        .expect("the dialog is open")
}

/// A line typed at the command line and ended with Enter.
fn type_line(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn set_spacings(app: &mut OpenCADStudio, snap: f32, grid: (f32, f32)) {
    app.snapper.snap_spacing_x = snap;
    app.snapper.snap_spacing_y = snap;
    app.grid_spacing_x = grid.0;
    app.grid_spacing_y = grid.1;
}

#[test]
fn an_architectural_drawing_shows_the_spacings_in_feet_and_inches() {
    let mut app = app_with_units(4, 4);
    set_spacings(&mut app, 18.0, (6.0, 0.5));
    let state = open_dialog(&mut app);
    assert_eq!(state.snap_x_input, "1'-6\"");
    assert_eq!(state.snap_y_input, "1'-6\"");
    assert_eq!(state.grid_x_input, "6\"");
    assert_eq!(state.grid_y_input, "0 1/2\"");
}

#[test]
fn feet_and_inches_typed_in_the_dialog_set_the_spacings() {
    let mut app = app_with_units(4, 4);
    open_dialog(&mut app);
    let _ = app.update(Message::DraftingSettingsSnapXChanged("1'-6\"".into()));
    let _ = app.update(Message::DraftingSettingsGridXChanged("6\"".into()));
    let _ = app.update(Message::DraftingSettingsGridYChanged("1/2\"".into()));
    let _ = app.update(Message::DraftingSettingsOk);
    assert!(app.active_modal.is_none(), "OK closes the dialog");
    assert_eq!(
        (app.snapper.snap_spacing_x, app.snapper.snap_spacing_y),
        (18.0, 18.0)
    );
    assert_eq!((app.grid_spacing_x, app.grid_spacing_y), (6.0, 0.5));
}

#[test]
fn a_decimal_drawing_shows_the_spacings_at_its_precision() {
    let mut app = app_with_units(2, 2);
    // DIMZIN 0: no trailing zeros are dropped, so LUPREC shows.
    let i = app.active_tab;
    app.tabs[i].scene.document.header.dim_zero_suppression = 0;
    set_spacings(&mut app, 10.0, (2.5, 2.5));
    let state = open_dialog(&mut app);
    assert_eq!(state.snap_x_input, "10.00");
    assert_eq!(state.grid_x_input, "2.50");
}

#[test]
fn an_unchanged_spacing_keeps_the_value_that_its_text_rounds() {
    let mut app = app_with_units(2, 4);
    // Each value rounds at LUPREC 4, and each is different.
    set_spacings(&mut app, 0.03125, (0.15625, 0.046875));
    let state = open_dialog(&mut app);
    assert_eq!(
        (state.snap_x_input.as_str(), state.grid_x_input.as_str()),
        ("0.0313", "0.1563")
    );
    assert_eq!(state.grid_y_input, "0.0469");
    let _ = app.update(Message::DraftingSettingsOk);
    assert_eq!(app.snapper.snap_spacing_x, 0.03125);
    assert_eq!(app.snapper.snap_spacing_y, 0.03125);
    assert_eq!(app.grid_spacing_x, 0.15625);
    assert_eq!(app.grid_spacing_y, 0.046875);
}

#[test]
fn a_spacing_that_is_not_a_positive_length_is_refused() {
    let mut app = app_with_units(4, 4);
    set_spacings(&mut app, 10.0, (10.0, 10.0));
    open_dialog(&mut app);
    let _ = app.update(Message::DraftingSettingsGridXChanged("-6\"".into()));
    let _ = app.update(Message::DraftingSettingsOk);
    assert!(app.active_modal.is_some(), "the dialog stays open");
    assert_eq!(app.grid_spacing_x, 10.0);
}

#[test]
fn gridunit_reads_feet_and_inches() {
    let mut app = app_with_units(4, 4);
    type_line(&mut app, "GRIDUNIT");
    type_line(&mut app, "1'-0\",6\"");
    assert_eq!((app.grid_spacing_x, app.grid_spacing_y), (12.0, 6.0));
    type_line(&mut app, "GRIDUNIT");
    type_line(&mut app, "2'");
    assert_eq!((app.grid_spacing_x, app.grid_spacing_y), (24.0, 24.0));
}

#[test]
fn gridunit_shows_the_current_spacing_in_the_drawing_units() {
    let mut app = app_with_units(4, 4);
    set_spacings(&mut app, 10.0, (12.0, 6.0));
    type_line(&mut app, "GRIDUNIT");
    let prompt = app.command_line.history.last().unwrap().text.clone();
    assert!(prompt.contains("<1'-0\",6\">"), "{prompt}");
}
