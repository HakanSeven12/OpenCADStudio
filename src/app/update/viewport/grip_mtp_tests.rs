use super::*;

use super::*;
use crate::app::OpenCADStudio;
use glam::DVec3;

/// A selected line (0,0)-(100,0) with its end grip hot (engaging click).
fn hot_line_end() -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let line = document::entities::Line::from_coords(0.0, 0.0, 0.0, 100.0, 0.0, 0.0);
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    let _ = app.run_command_line("ZOOM W -50,-150 350,150");
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    let end = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| g.world.distance(DVec3::new(100.0, 0.0, 0.0)) < 1e-9)
        .expect("end grip")
        .clone();
    click(&mut app, end.world);
    assert!(app.tabs[i].active_grip.is_some(), "the end grip is hot");
    (app, handle)
}

fn click(app: &mut OpenCADStudio, world: DVec3) {
    let i = app.active_tab;
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let p = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(world, bounds)
        .unwrap();
    let p = Point::new(p.x, p.y);
    let _ = app.on_viewport_move(p);
    app.tabs[i].scene.selection.borrow_mut().input.last_move_pos = Some(p);
    let _ = app.on_viewport_left_press();
    let _ = app.on_viewport_left_release();
}

fn submit(app: &mut OpenCADStudio, text: &str) {
    app.command_line.input = text.to_string();
    let _ = app.on_command_submit();
}

fn line_end(app: &OpenCADStudio, handle: Handle) -> DVec3 {
    let Some(document::EntityType::Line(line)) =
        app.tabs[app.active_tab].scene.document.get_entity(handle)
    else {
        panic!("a line")
    };
    DVec3::new(line.end.x, line.end.y, line.end.z)
}

#[test]
fn typed_mtp_places_a_hot_grip_midway_between_two_typed_points() {
    let (mut app, handle) = hot_line_end();
    let i = app.active_tab;
    submit(&mut app, "MTP");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "MTP asks for its first point"
    );
    submit(&mut app, "200,0");
    submit(&mut app, "200,100");
    assert!(app.tabs[i].active_grip.is_none(), "the grip was placed");
    assert!(app.tabs[i].active_cmd.is_none());
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(200.0, 50.0, 0.0)) < 1e-6, "{end:?}");
    assert_eq!(app.tabs[i].history.undo_stack.len(), 1, "one undo step");
}

#[test]
fn clicked_mtp_points_place_a_hot_grip_at_their_midpoint() {
    let (mut app, handle) = hot_line_end();
    let i = app.active_tab;
    app.tabs[i].snap_result = None;
    app.snapper.snap_enabled = false;
    let _ = app.update(crate::app::Message::SnapOverrideMtp);
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "the snap menu's MTP starts"
    );
    click(&mut app, DVec3::new(150.0, 80.0, 0.0));
    assert!(
        app.tabs[i].active_grip.is_some(),
        "the first pick is MTP's, not the grip's"
    );
    click(&mut app, DVec3::new(250.0, 80.0, 0.0));
    assert!(app.tabs[i].active_grip.is_none(), "the grip was placed");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(200.0, 80.0, 0.0)) < 0.5, "{end:?}");
}
