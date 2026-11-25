use super::grip_mtp_tests::{click, hot_line_end, line_end, submit};
use super::*;
use crate::app::OpenCADStudio;
use glam::DVec3;

/// The end grip of the line (0,0)-(100,0) is hot, and typed input goes to the
/// command line (Dynamic Input off).
fn hot_end_typed_at_the_command_line() -> (OpenCADStudio, Handle) {
    let (mut app, handle) = hot_line_end();
    app.dyn_input = false;
    app.sync_dyn_fields();
    (app, handle)
}

#[test]
fn a_typed_point_places_a_hot_grip_there() {
    let (mut app, handle) = hot_end_typed_at_the_command_line();
    let i = app.active_tab;
    submit(&mut app, "150,40");
    assert!(app.tabs[i].active_grip.is_none(), "the grip was placed");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(150.0, 40.0, 0.0)) < 1e-9, "{end:?}");
    assert_eq!(app.tabs[i].history.undo_stack.len(), 1, "one undo step");
}

#[test]
fn a_relative_point_is_measured_from_the_grip() {
    let (mut app, handle) = hot_end_typed_at_the_command_line();
    // A point of an earlier command is not the base.
    app.last_point = Some(DVec3::new(500.0, 500.0, 0.0));
    submit(&mut app, "@20,30");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(120.0, 30.0, 0.0)) < 1e-9, "{end:?}");
}

#[test]
fn a_typed_angle_sets_the_direction_of_the_next_distance() {
    let (mut app, handle) = hot_end_typed_at_the_command_line();
    let i = app.active_tab;
    app.last_point = Some(DVec3::new(500.0, 500.0, 0.0));
    submit(&mut app, "<90");
    assert!(app.tabs[i].active_grip.is_some(), "the grip stays hot");
    assert!(app.angle_override.is_some(), "the angle is held");
    submit(&mut app, "25");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(100.0, 25.0, 0.0)) < 1e-9, "{end:?}");
    assert!(app.angle_override.is_none(), "the angle is spent");
}

#[test]
fn a_click_after_a_typed_angle_places_the_grip_on_that_direction() {
    let (mut app, handle) = hot_end_typed_at_the_command_line();
    let i = app.active_tab;
    app.snapper.snap_enabled = false;
    submit(&mut app, "<90");
    click(&mut app, DVec3::new(130.0, 40.0, 0.0));
    assert!(app.tabs[i].active_grip.is_none(), "the grip was placed");
    let end = line_end(&app, handle);
    assert!((end.x - 100.0).abs() < 1e-6, "on the 90° line: {end:?}");
    assert!((end.y - 40.0).abs() < 0.5, "{end:?}");
    assert!(app.angle_override.is_none(), "the angle is spent");
}

#[test]
fn cancelling_the_grip_drops_a_typed_angle() {
    let (mut app, _) = hot_end_typed_at_the_command_line();
    // No earlier point: the grip is the base of the angle.
    app.last_point = None;
    submit(&mut app, "<90");
    assert!(app.angle_override.is_some());
    assert!(app.cancel_active_grip_edit());
    assert!(
        app.angle_override.is_none(),
        "the next command does not inherit the angle"
    );
}

#[test]
fn a_typed_point_moves_a_circle_by_its_centre_grip() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let circle = document::entities::Circle::from_coords(50.0, 30.0, 0.0, 10.0);
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Circle(circle));
    let _ = app.run_command_line("ZOOM W -150,-150 250,150");
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    click(&mut app, DVec3::new(50.0, 30.0, 0.0));
    assert!(app.tabs[i].active_grip.is_some(), "the centre grip is hot");
    app.dyn_input = false;
    app.sync_dyn_fields();
    submit(&mut app, "60,-20");
    let Some(document::EntityType::Circle(circle)) = app.tabs[i].scene.document.get_entity(handle)
    else {
        panic!("a circle")
    };
    let centre = DVec3::new(circle.center.x, circle.center.y, circle.center.z);
    assert!(
        centre.distance(DVec3::new(60.0, -20.0, 0.0)) < 1e-9,
        "{centre:?}"
    );
    assert!((circle.radius - 10.0).abs() < 1e-9, "the radius stays");
}

/// The end grip of the line (0,0)-(100,0) is hot under a UCS at (10,20)
/// turned 90°, and typed input goes to the command line.
fn hot_end_in_a_turned_ucs() -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let line = document::entities::Line::from_coords(0.0, 0.0, 0.0, 100.0, 0.0, 0.0);
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Line(line));
    let _ = app.run_command_line("ZOOM W -50,-150 350,150");
    let _ = app.run_command_line("UCS ORIGIN 10,20,0");
    let _ = app.run_command_line("UCS Z 90");
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    click(&mut app, DVec3::new(100.0, 0.0, 0.0));
    assert!(app.tabs[i].active_grip.is_some(), "the end grip is hot");
    app.dyn_input = false;
    app.sync_dyn_fields();
    (app, handle)
}

#[test]
fn a_typed_point_is_read_in_the_current_ucs() {
    let (mut app, handle) = hot_end_in_a_turned_ucs();
    // UCS X runs along world Y from (10,20): UCS (5,5) is world (5,25).
    submit(&mut app, "5,5");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(5.0, 25.0, 0.0)) < 1e-9, "{end:?}");
}

#[test]
fn a_starred_point_is_read_along_the_world_axes() {
    let (mut app, handle) = hot_end_in_a_turned_ucs();
    submit(&mut app, "*5,5");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(5.0, 5.0, 0.0)) < 1e-9, "{end:?}");
}

#[test]
fn a_relative_offset_turns_with_the_ucs() {
    let (mut app, handle) = hot_end_in_a_turned_ucs();
    // UCS +X is world +Y.
    submit(&mut app, "@10,0");
    let end = line_end(&app, handle);
    assert!(end.distance(DVec3::new(100.0, 10.0, 0.0)) < 1e-9, "{end:?}");
}

#[test]
fn a_typed_point_places_the_corner_of_a_rectangle_being_resized() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut rectangle = document::entities::LwPolyline::new();
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)] {
        rectangle.add_point(document::types::Vector2::new(x, y));
    }
    rectangle.is_closed = true;
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::LwPolyline(rectangle));
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    app.dyn_input = false;
    // A session always has a pointer position; the commit reads it.
    {
        let mut selection = app.tabs[i].scene.selection.borrow_mut();
        selection.view.vp_size = (800.0, 600.0);
        selection.input.last_move_pos = Some(Point::new(400.0, 300.0));
    }
    // The grip menu's Resize on the (100,50) corner.
    app.tabs[i].active_grip = Some(crate::scene::pick::grip::GripEdit::rectangle_resize(
        handle,
        2,
        DVec3::new(100.0, 50.0, 0.0),
        DVec3::ZERO,
        DVec3::X,
        DVec3::Y,
    ));
    app.sync_dyn_fields();
    submit(&mut app, "40,30");
    assert!(app.tabs[i].active_grip.is_none(), "the corner was placed");
    let Some(document::EntityType::LwPolyline(rectangle)) =
        app.tabs[i].scene.document.get_entity(handle)
    else {
        panic!("a polyline")
    };
    let corners: Vec<(f64, f64)> = rectangle
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    assert_eq!(
        corners,
        [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)],
        "the rectangle keeps its shape"
    );
}
