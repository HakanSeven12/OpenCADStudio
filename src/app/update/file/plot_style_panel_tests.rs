use super::*;

use crate::app::{Message, OpenCADStudio};

fn app_with_test_table() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let mut table = crate::io::plot_style::PlotStyleTable::identity("Test.ctb");
    table.aci_entries[1].screening = 65;
    app.plot_dialog.style_name = table.name.clone();
    app.active_plot_style = Some(table);
    app
}

#[test]
fn invalid_screening_keeps_last_valid_value_and_reports_error() {
    let mut app = app_with_test_table();

    let old_error_revision = app.command_line.error_revision;
    let _ = app.update(Message::PlotStylePanelScreenBuf("200".into()));

    assert_eq!(
        app.active_plot_style.as_ref().unwrap().aci_entries[1].screening,
        65,
        "invalid input must not overwrite the last valid screening"
    );
    assert!(
        app.command_line.error_revision > old_error_revision,
        "invalid input should report feedback on the command line"
    );
}

#[test]
fn invalid_color_is_reported_without_changing_the_entry() {
    let mut app = app_with_test_table();
    let entry = &mut app.active_plot_style.as_mut().unwrap().aci_entries[1];
    entry.color = Some([12, 34, 56]);
    entry.lineweight = 4;
    app.ps_lineweight_buf = "5".into();
    app.ps_screening_buf = "70".into();
    let old_error_revision = app.command_line.error_revision;

    let _ = app.update(Message::PlotStylePanelColorBuf("#fff".into()));

    let entry = &app.active_plot_style.as_ref().unwrap().aci_entries[1];
    assert_eq!(entry.color, Some([12, 34, 56]));
    assert_eq!(entry.lineweight, 4);
    assert_eq!(entry.screening, 65);
    assert!(app.command_line.error_revision > old_error_revision);
}

#[test]
fn invalid_lineweight_is_reported_without_changing_the_entry() {
    let mut app = app_with_test_table();
    let entry = &mut app.active_plot_style.as_mut().unwrap().aci_entries[1];
    entry.color = Some([12, 34, 56]);
    entry.lineweight = 4;
    app.ps_color_buf = "#010203".into();
    app.ps_lineweight_buf = "200".into();
    app.ps_screening_buf = "70".into();
    let old_error_revision = app.command_line.error_revision;

    let _ = app.update(Message::PlotStylePanelLwBuf("200".into()));

    let entry = &app.active_plot_style.as_ref().unwrap().aci_entries[1];
    assert_eq!(entry.color, Some([12, 34, 56]));
    assert_eq!(entry.lineweight, 4);
    assert_eq!(entry.screening, 65);
    assert!(app.command_line.error_revision > old_error_revision);
}

#[test]
fn closing_plot_style_panel_restores_unsaved_table_edits() {
    let mut app = app_with_test_table();
    let _ = app.update(Message::PlotStylePanelOpen);

    let _ = app.update(Message::PlotStylePanelScreenBuf("72".into()));
    assert_eq!(
        app.active_plot_style.as_ref().unwrap().aci_entries[1].screening,
        72
    );

    app.close_active_modal();

    assert_eq!(
        app.active_plot_style.as_ref().unwrap().aci_entries[1].screening,
        65,
        "closing without saving should restore the table from editor open"
    );
}

#[test]
fn closing_after_save_keeps_saved_table_and_discards_later_edits() {
    let mut app = app_with_test_table();
    let _ = app.update(Message::PlotStylePanelOpen);

    let _ = app.update(Message::PlotStylePanelScreenBuf("72".into()));
    app.commit_plotstyle_panel_baseline();

    let _ = app.update(Message::PlotStylePanelScreenBuf("80".into()));
    app.close_active_modal();

    assert_eq!(
        app.active_plot_style.as_ref().unwrap().aci_entries[1].screening,
        72,
        "closing should keep the last successfully saved table state"
    );
}
