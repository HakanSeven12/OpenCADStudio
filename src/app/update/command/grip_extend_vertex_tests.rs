use crate::app::{Message, OpenCADStudio};
use crate::entities::traits::Grippable;
use crate::scene::model::object::GripMenuAction;
use document::entities::LwPolyline;
use document::types::Vector2;
use document::{EntityType, Handle};

/// A selected open polyline (1,2) → (11,3) → (9,14); its first segment is
/// an arc.
fn app_with_polyline() -> (OpenCADStudio, usize, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut polyline = LwPolyline::from_points(vec![
        Vector2::new(1.0, 2.0),
        Vector2::new(11.0, 3.0),
        Vector2::new(9.0, 14.0),
    ]);
    polyline.vertices[0].bulge = 0.4;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let handle = app.tabs[i]
        .scene
        .add_entity(EntityType::LwPolyline(polyline));
    let _ = app.run_command_line("ZOOM W -40,-30 40,30");
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_selected_grips();
    (app, i, handle)
}

fn polyline(app: &OpenCADStudio, i: usize, handle: Handle) -> LwPolyline {
    match app.tabs[i].scene.document.get_entity(handle) {
        Some(EntityType::LwPolyline(polyline)) => polyline.clone(),
        other => panic!("expected a polyline, found {other:?}"),
    }
}

fn locations(polyline: &LwPolyline) -> Vec<(f64, f64)> {
    polyline
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect()
}

/// Open the grip menu of `grip_id` and pick Extend Vertex.
fn pick_extend_vertex(app: &mut OpenCADStudio, i: usize, handle: Handle, grip_id: usize) {
    let items = polyline(app, i, handle).grip_menu(grip_id);
    let index = items
        .iter()
        .position(|item| item.action == GripMenuAction::ExtendVertex)
        .expect("the grip offers Extend Vertex");
    app.grip_popup = Some(crate::app::GripPopup {
        handle,
        grip_id,
        anchor: iced::Point::ORIGIN,
        items,
        selected: index,
        pinned: true,
    });
    let _ = app.update(Message::GripMenuPick(index));
}

/// Click at `world`: the click that places the new vertex.
fn click(app: &mut OpenCADStudio, world: glam::DVec3) {
    let i = app.active_tab;
    let bounds = iced::Rectangle::with_size(iced::Size::new(800.0, 600.0));
    let p = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(world, bounds)
        .unwrap();
    let p = iced::Point::new(p.x, p.y);
    let _ = app.on_viewport_move(p);
    app.tabs[i].scene.selection.borrow_mut().input.last_move_pos = Some(p);
    let _ = app.on_viewport_left_press();
    let _ = app.on_viewport_left_release();
}

/// The vertex locations, to 1e-6.
fn rounded(polyline: &LwPolyline) -> Vec<(f64, f64)> {
    locations(polyline)
        .into_iter()
        .map(|(x, y)| ((x * 1e6).round() / 1e6, (y * 1e6).round() / 1e6))
        .collect()
}

#[test]
fn only_the_end_grips_of_an_open_polyline_offer_extend_vertex() {
    let (app, i, handle) = app_with_polyline();
    let mut open = polyline(&app, i, handle);
    let offers = |polyline: &LwPolyline, grip_id: usize| {
        polyline
            .grip_menu(grip_id)
            .iter()
            .any(|item| item.action == GripMenuAction::ExtendVertex)
    };
    assert!(offers(&open, 0), "the start grip");
    assert!(!offers(&open, 1), "a middle grip");
    assert!(offers(&open, 2), "the end grip");
    assert!(!offers(&open, 3), "a segment grip");
    open.is_closed = true;
    for grip_id in 0..3 {
        assert!(!offers(&open, grip_id), "closed, grip {grip_id}");
    }
}

/// Extend Vertex on the start grip adds a vertex before the start, where
/// the user places it, with no need to reverse the polyline.
#[test]
fn extend_vertex_on_the_start_grip_adds_a_vertex_before_the_start() {
    let (mut app, i, handle) = app_with_polyline();
    pick_extend_vertex(&mut app, i, handle, 0);
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|grip| grip.grip_id),
        Some(0),
        "the new vertex follows the cursor"
    );
    click(&mut app, glam::DVec3::new(-5.0, -3.0, 0.0));
    let placed = polyline(&app, i, handle);
    assert_eq!(
        rounded(&placed),
        [(-5.0, -3.0), (1.0, 2.0), (11.0, 3.0), (9.0, 14.0)]
    );
    assert_eq!(placed.vertices[0].bulge, 0.0, "the new segment is straight");
    assert_eq!(placed.vertices[1].bulge, 0.4, "the old first arc stays");
    assert!(app.tabs[i].active_grip.is_none());

    let _ = app.update(Message::Undo);
    assert_eq!(
        locations(&polyline(&app, i, handle)),
        [(1.0, 2.0), (11.0, 3.0), (9.0, 14.0)],
        "one undo step takes the vertex back"
    );
}

#[test]
fn extend_vertex_on_the_end_grip_adds_a_vertex_after_the_end() {
    let (mut app, i, handle) = app_with_polyline();
    pick_extend_vertex(&mut app, i, handle, 2);
    assert_eq!(
        app.tabs[i].active_grip.as_ref().map(|grip| grip.grip_id),
        Some(3)
    );
    click(&mut app, glam::DVec3::new(4.0, 20.0, 0.0));
    assert_eq!(
        rounded(&polyline(&app, i, handle)),
        [(1.0, 2.0), (11.0, 3.0), (9.0, 14.0), (4.0, 20.0)]
    );
}

#[test]
fn escape_while_placing_leaves_the_polyline_as_it_was() {
    for grip_id in [0, 2] {
        let (mut app, i, handle) = app_with_polyline();
        let before = polyline(&app, i, handle);
        pick_extend_vertex(&mut app, i, handle, grip_id);
        let _ = app.update(Message::CommandEscape);
        let after = polyline(&app, i, handle);
        assert_eq!(locations(&after), locations(&before), "grip {grip_id}");
        assert_eq!(after.vertices[0].bulge, 0.4, "grip {grip_id}");
    }
}

/// The polyline of `app_with_polyline`, without an app.
fn bare_polyline(closed: bool) -> LwPolyline {
    let mut polyline = LwPolyline::from_points(vec![
        Vector2::new(1.0, 2.0),
        Vector2::new(11.0, 3.0),
        Vector2::new(9.0, 14.0),
    ]);
    polyline.vertices[0].bulge = 0.4;
    polyline.is_closed = closed;
    polyline
}

/// The menu gives a closed polyline no Extend Vertex, and the action
/// changes nothing there either.
#[test]
fn extend_vertex_changes_nothing_on_a_closed_polyline() {
    for grip_id in 0..3 {
        let mut polyline = bare_polyline(true);
        polyline.apply_grip_menu(grip_id, GripMenuAction::ExtendVertex);
        assert_eq!(
            locations(&polyline),
            [(1.0, 2.0), (11.0, 3.0), (9.0, 14.0)],
            "grip {grip_id}"
        );
    }
}

/// Add Vertex on the last grip still adds past the end of an open polyline,
/// and splits the closing segment of a closed one.
#[test]
fn add_vertex_on_the_last_grip_keeps_its_behaviour() {
    let mut open = bare_polyline(false);
    open.apply_grip_menu(2, GripMenuAction::AddVertex);
    assert_eq!(
        locations(&open),
        [(1.0, 2.0), (11.0, 3.0), (9.0, 14.0), (9.0, 14.0)]
    );
    let mut closed = bare_polyline(true);
    closed.apply_grip_menu(2, GripMenuAction::AddVertex);
    assert_eq!(
        locations(&closed),
        [(1.0, 2.0), (11.0, 3.0), (9.0, 14.0), (5.0, 8.0)]
    );
}
