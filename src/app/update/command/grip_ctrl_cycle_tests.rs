use super::*;

use crate::app::{Message, OpenCADStudio};
use crate::scene::model::object::GripMenuAction;
use document::entities::LwPolyline;
use document::types::Vector2;
use document::{EntityType, Handle};
use glam::DVec3;

fn app_with_polyline() -> (OpenCADStudio, usize, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let polyline = LwPolyline::from_points(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(10.0, 10.0),
    ]);
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::LwPolyline(polyline));
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    (app, i, handle)
}

fn pick_up(app: &mut OpenCADStudio, i: usize, handle: Handle, grip_id: usize, at: DVec3) {
    let edit = app.grip_edit_for_hit(i, handle, grip_id, false, at);
    app.tabs[i].active_grip = Some(edit);
}

fn press_ctrl(app: &mut OpenCADStudio) {
    let _ = app.update(Message::SetModifiers {
        shift: false,
        ctrl: true,
        alt: false,
    });
    let _ = app.update(Message::SetModifiers {
        shift: false,
        ctrl: false,
        alt: false,
    });
}

fn polyline(app: &OpenCADStudio, i: usize, handle: Handle) -> LwPolyline {
    match app.tabs[i].scene.document.get_entity(handle) {
        Some(EntityType::LwPolyline(polyline)) => polyline.clone(),
        other => panic!("expected a polyline, found {other:?}"),
    }
}

#[test]
fn ctrl_cycles_a_vertex_grip_and_a_full_turn_changes_nothing() {
    let (mut app, i, handle) = app_with_polyline();
    let dirty = app.tabs[i].dirty;
    pick_up(&mut app, i, handle, 1, DVec3::new(10.0, 0.0, 0.0));

    press_ctrl(&mut app);
    assert_eq!(
        polyline(&app, i, handle).vertices.len(),
        4,
        "Add Vertex is provisional"
    );
    assert_eq!(app.grip_ctrl_one_shot(i), None);
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|grip| grip.grip_id),
        Some(2)
    );

    press_ctrl(&mut app);
    assert_eq!(
        polyline(&app, i, handle).vertices.len(),
        3,
        "Add Vertex left behind"
    );
    assert_eq!(
        app.grip_ctrl_one_shot(i),
        Some(GripMenuAction::RemoveVertex)
    );

    press_ctrl(&mut app);
    assert_eq!(polyline(&app, i, handle).vertices.len(), 3);
    assert_eq!(app.grip_ctrl_one_shot(i), None);
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|grip| grip.grip_id),
        Some(1)
    );
    assert_eq!(
        app.tabs[i].dirty, dirty,
        "a full turn leaves the drawing clean"
    );
}

#[test]
fn the_click_after_ctrl_removes_the_vertex_as_one_undo_step() {
    let (mut app, i, handle) = app_with_polyline();
    pick_up(&mut app, i, handle, 1, DVec3::new(10.0, 0.0, 0.0));
    press_ctrl(&mut app);
    press_ctrl(&mut app);

    // The cursor has nothing to place while the removal waits for a click.
    let _ = app.update(Message::ViewportMove(iced::Point::new(50.0, 50.0)));
    assert_eq!(
        polyline(&app, i, handle).vertices[1].location,
        Vector2::new(10.0, 0.0)
    );

    let _ = app.on_viewport_left_release();
    assert_eq!(polyline(&app, i, handle).vertices.len(), 2);
    assert!(app.tabs[i].active_grip.is_none());
    assert!(app.grip_ctrl_cycle.is_none());

    let _ = app.update(Message::Undo);
    assert_eq!(polyline(&app, i, handle).vertices.len(), 3);
}

#[test]
fn ctrl_cycles_a_midpoint_grip_through_add_vertex_and_arc() {
    let (mut app, i, handle) = app_with_polyline();
    // Grips 0..3 are the vertices; grip 3 is the first segment's midpoint.
    pick_up(&mut app, i, handle, 3, DVec3::new(5.0, 0.0, 0.0));

    press_ctrl(&mut app);
    assert_eq!(polyline(&app, i, handle).vertices.len(), 4);

    press_ctrl(&mut app);
    let arc = polyline(&app, i, handle);
    assert_eq!(arc.vertices.len(), 3);
    assert!(
        arc.vertices[0].bulge.abs() > 1e-9,
        "the segment is an arc while placing"
    );

    press_ctrl(&mut app);
    let back = polyline(&app, i, handle);
    assert_eq!(back.vertices.len(), 3);
    assert_eq!(
        back.vertices[0].bulge, 0.0,
        "leaving the stage restores the line"
    );
}

#[test]
fn escape_during_the_cycle_restores_the_polyline() {
    let (mut app, i, handle) = app_with_polyline();
    pick_up(&mut app, i, handle, 1, DVec3::new(10.0, 0.0, 0.0));
    press_ctrl(&mut app);
    assert!(app.cancel_active_grip_edit());
    assert_eq!(polyline(&app, i, handle).vertices.len(), 3);
    assert!(app.tabs[i].active_grip.is_none());
    assert!(app.grip_ctrl_cycle.is_none());
}

#[test]
fn ctrl_leaves_other_grips_alone() {
    use document::entities::Line;
    use document::types::Vector3;
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    let handle = app.tabs[i].scene.add_entity(EntityType::Line(line));
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    pick_up(&mut app, i, handle, 0, DVec3::ZERO);
    press_ctrl(&mut app);
    assert!(app.grip_ctrl_cycle.is_none());
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|grip| grip.grip_id),
        Some(0)
    );
}
