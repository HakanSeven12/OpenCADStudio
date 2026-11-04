use super::*;
use glam::DVec3;

fn mtext_with_corner_preview() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.run_command_line("MTEXT");
    let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
    let i = app.active_tab;
    app.tabs[i].last_cursor_world = DVec3::new(20.0, -10.0, 0.0);
    app.refresh_active_cmd_preview(i);
    let wires = &app.tabs[i].scene.preview_wires;
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].name, "rubber_band");
    assert_eq!(wires[0].color, crate::scene::WireModel::CYAN);
    assert_eq!(wires[0].points.len(), 5);
    assert_eq!(wires[0].points.first(), wires[0].points.last());
    app
}

fn open_editor(app: &mut OpenCADStudio) {
    let _ = app.feed_command(StepInput::Point(DVec3::new(20.0, -10.0, 0.0)));
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!(app.mtext_editor.is_some());
}

#[test]
fn mtext_preview_cleared_after_commit() {
    let mut app = mtext_with_corner_preview();
    open_editor(&mut app);
    app.mtext_type("Synthetic text");
    let _ = app.update(Message::MTextOk);
    assert!(app.mtext_editor.is_none());
    assert_eq!(app.tabs[app.active_tab].scene.document.entities().count(), 1);
    assert!(app.tabs[app.active_tab].scene.preview_wires.is_empty(),
        "MTEXT corner rectangle remains after commit");
}

#[test]
fn mtext_preview_cleared_after_editor_escape() {
    let mut app = mtext_with_corner_preview();
    // Command Esc already clears the preview before the editor opens.
    let _ = app.feed_command(StepInput::Escape);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!(app.tabs[app.active_tab].scene.preview_wires.is_empty());
    let _ = app.run_command_line("MTEXT");
    let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
    let i = app.active_tab;
    app.refresh_active_cmd_preview(i);
    assert!(!app.tabs[i].scene.preview_wires.is_empty());
    open_editor(&mut app);
    let _ = app.update(Message::CommandEscape);
    assert!(app.mtext_editor.is_none());
    assert_eq!(app.tabs[i].scene.document.entities().count(), 0);
    assert!(app.tabs[i].scene.preview_wires.is_empty(),
        "MTEXT corner rectangle remains after editor Esc");
}
