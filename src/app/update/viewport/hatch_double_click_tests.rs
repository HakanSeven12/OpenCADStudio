use super::*;

mod hatch_double_click_tests {
use crate::app::{Message, OpenCADStudio};
use document::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
use document::types::Vector2;

/// A solid hatch over (0,0)–(10,10), and the window point at its centre.
fn app_with_hatch() -> (OpenCADStudio, document::Handle, iced::Point, iced::Point) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut path = BoundaryPath::new();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
            .into_iter()
            .map(|p| Vector2::new(p[0], p[1]))
            .collect(),
        true,
    )));
    hatch.paths.push(path);
    let handle = app.tabs[i]
        .scene
        .add_entity(document::EntityType::Hatch(Box::new(hatch)));

    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-5.0, -5.0, 0.0),
        glam::DVec3::new(15.0, 15.0, 0.0),
        800.0 / 600.0,
    );
    let tile = app.tabs[i].scene.active_model_tile_bounds(800.0, 600.0);
    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: tile.width,
        height: tile.height,
    };
    let inside = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(glam::DVec3::new(5.0, 5.0, 0.0), bounds)
        .unwrap();
    let cursor_local = iced::Point::new(inside.x, inside.y);
    let cursor = iced::Point::new(tile.x + cursor_local.x, tile.y + cursor_local.y);
    (app, handle, cursor, cursor_local)
}

fn click(app: &mut OpenCADStudio, cursor: iced::Point) {
    let _ = app.update(Message::ViewportMove(cursor));
    let _ = app.update(Message::ViewportLeftPressAt(cursor));
    let _ = app.update(Message::ViewportLeftRelease);
}

/// Double-clicking inside a hatch's fill selects it and starts HATCHEDIT
/// on it. A hatch has no boundary wire, so the double-click must pick it
/// by its fill, as a single click does; before, nothing happened.
#[test]
fn double_clicking_a_hatch_starts_hatchedit_on_it() {
    let (mut app, handle, cursor, cursor_local) = app_with_hatch();
    let i = app.active_tab;
    // The first click of the pair, 100 ms ago at the same spot.
    app.last_vp_click_time =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(10));
    app.last_vp_click_pos = Some(cursor);
    click(&mut app, cursor);

    let command = app.tabs[i].active_cmd.as_ref().map(|cmd| cmd.name());
    assert_eq!(command, Some("HATCHEDIT"));
    assert_eq!(app.tabs[i].selected_handle, Some(handle));
    // Selected first, HATCHEDIT skipped its pick and edits this hatch.
    assert_eq!(
        app.tabs[i]
            .active_cmd
            .as_ref()
            .map(|cmd| cmd.needs_entity_pick()),
        Some(false)
    );
}

/// With the hatch selected, the right-click menu offers Edit Hatch.
#[test]
fn a_selected_hatch_gets_edit_hatch_in_the_context_menu() {
    use crate::ui::popup::context_menu::MenuContext;
    let (mut app, handle, _, _) = app_with_hatch();
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(handle, true);
    assert!(matches!(
        app.context_menu_context(),
        MenuContext::Idle {
            single_hatch: true,
            single_mleader: false,
            ..
        }
    ));
}

/// HATCHEDIT started with nothing selected picks a hatch by clicking
/// inside its fill; before, its pick only hit wires and no hatch could
/// be chosen.
#[test]
fn hatchedit_picks_a_hatch_by_its_fill() {
    let (mut app, _handle, cursor, _) = app_with_hatch();
    let i = app.active_tab;
    let _ = app.dispatch_command("HATCHEDIT");
    let picking = |app: &OpenCADStudio| {
        app.tabs[i]
            .active_cmd
            .as_ref()
            .map(|cmd| (cmd.name(), cmd.needs_entity_pick()))
    };
    assert_eq!(picking(&app), Some(("HATCHEDIT", true)));

    click(&mut app, cursor);
    assert_eq!(picking(&app), Some(("HATCHEDIT", false)), "hatch picked");
}
}
