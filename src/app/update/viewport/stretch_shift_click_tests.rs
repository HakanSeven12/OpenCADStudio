use super::*;

use crate::app::OpenCADStudio;
use document::EntityType;
use glam::DVec3;

#[test]
fn shift_click_takes_a_whole_object_out_of_stretch() {
    use crate::modules::draw::modify::stretch::StretchCommand;
    use document::entities::Line;
    use document::types::Vector3;
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.show_ucs_icon = false;
    let line = Line::from_points(Vector3::new(-10.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    let handle = app.tabs[i].scene.add_entity(EntityType::Line(line));
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let _ = app.run_command_line("ZOOM EXTENTS");
    app.tabs[i].scene.selection.borrow_mut().input.last_move_pos =
        Some(iced::Point::new(400.0, 300.0));
    let window = (DVec3::new(-11.0, -1.0, -1.0), DVec3::new(-9.0, 1.0, 1.0));
    app.tabs[i].active_cmd = Some(Box::new(StretchCommand::with_windows(
        vec![handle],
        vec![],
        vec![window],
        Vec::new(),
    )));
    app.tabs[i].scene.selected.insert(handle);
    app.shift_down = true;

    let _ = app.on_viewport_left_press();
    assert!(
        !app.tabs[i].scene.selected.contains(&handle),
        "the highlight is dropped"
    );
    // Enter has nothing left to stretch.
    let command = app.tabs[i]
        .active_cmd
        .as_mut()
        .expect("STRETCH stays active");
    assert!(matches!(
        command.on_enter(),
        crate::command::CmdResult::Cancel
    ));
}
