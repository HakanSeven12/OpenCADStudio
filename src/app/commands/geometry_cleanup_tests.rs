use crate::app::{Message, OpenCADStudio};
use crate::modules::manage::geometry_cleanup_settings::{CleanupSettings, SAMPLE_GCS};
use crate::ui::dock::PanelId;
use crate::ui::window::geometry_cleanup::{Finding, Fix, GeometryCleanupMsg};
use document::entities::{Arc, Line, LwPolyline};
use document::types::{Vector2, Vector3};
use document::{EntityType, Handle};
use ocs_geom::geom2d::cleanup::EndErrorKind;

fn app() -> (OpenCADStudio, usize) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    (app, i)
}

fn add(app: &mut OpenCADStudio, entity: EntityType) -> Handle {
    let i = app.active_tab;
    app.tabs[i].scene.add_entity(entity)
}

fn line(app: &mut OpenCADStudio, a: [f64; 2], b: [f64; 2]) -> Handle {
    add(
        app,
        EntityType::Line(Line::from_points(
            Vector3::new(a[0], a[1], 0.0),
            Vector3::new(b[0], b[1], 0.0),
        )),
    )
}

fn ends(app: &OpenCADStudio, handle: Handle) -> [[f64; 2]; 2] {
    match app.tabs[app.active_tab].scene.document.get_entity(handle) {
        Some(EntityType::Line(line)) => [[line.start.x, line.start.y], [line.end.x, line.end.y]],
        other => panic!("expected a line, found {other:?}"),
    }
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
}

/// Scan `objects` as Select Objects does.
fn scan(app: &mut OpenCADStudio, objects: &[Handle]) -> Vec<Finding> {
    let list: Vec<String> = objects.iter().map(|h| format!("{:X}", h.value())).collect();
    let _ = app.dispatch_command(&format!("_GEOMETRYCLEANUPSCAN {}", list.join(",")));
    findings(app)
}

fn findings(app: &OpenCADStudio) -> Vec<Finding> {
    app.geometry_cleanup
        .scan
        .as_ref()
        .map(|scan| scan.findings.clone())
        .unwrap_or_default()
}

fn fix(app: &mut OpenCADStudio, kind: EndErrorKind, fix: Fix) {
    let index = findings(app)
        .iter()
        .position(|finding| finding.kind == kind)
        .expect("the error is listed");
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::Fix(
        index, fix,
    )));
}

/// A wall (x = 10), a line 0.4 past it, a line 0.3 short of it, and two
/// lines whose ends are 0.2 apart. The mean length is 51.98, so the
/// proposed tolerance (1% of it) is 0.5198.
fn errors_drawing(app: &mut OpenCADStudio) -> [Handle; 5] {
    let wall = line(app, [10.0, -100.0], [10.0, 100.0]);
    let over = line(app, [0.0, 50.0], [10.4, 50.0]);
    let under = line(app, [0.0, -50.0], [9.7, -50.0]);
    let left = line(app, [-60.0, 0.0], [-40.0, 0.0]);
    let right = line(app, [-39.8, 0.0], [-20.0, 0.0]);
    [wall, over, under, left, right]
}

#[test]
fn open_and_close_show_and_hide_the_palette() {
    let (mut app, _) = app();
    let _ = app.dispatch_command("GEOMETRYCLEANUPOPEN");
    assert!(app.geometry_cleanup.show);
    assert!(app.dock_panel_wanted(PanelId::GeometryCleanup));
    let _ = app.dispatch_command("GEOMETRYCLEANUPCLOSE");
    assert!(!app.geometry_cleanup.show);
}

#[test]
fn a_scan_lists_each_end_error_of_the_selection() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    let [_, over, under, left, right] = objects;
    let found = scan(&mut app, &objects);
    let tolerance = app
        .geometry_cleanup
        .scan
        .as_ref()
        .unwrap()
        .tolerances
        .gap
        .unwrap();
    assert!(tolerance > 0.4, "proposed tolerance {tolerance}");
    let mut kinds: Vec<(EndErrorKind, Handle)> = found.iter().map(|f| (f.kind, f.handle)).collect();
    kinds.sort_by_key(|(kind, handle)| (*kind, handle.value()));
    assert_eq!(
        kinds,
        [
            (EndErrorKind::Gap, left),
            (EndErrorKind::Overshoot, over),
            (EndErrorKind::Undershoot, under),
        ]
    );
    let gap = found.iter().find(|f| f.kind == EndErrorKind::Gap).unwrap();
    assert_eq!(gap.other, Some((right, true)));
}

#[test]
fn trim_cuts_an_overshoot_back_as_one_undo_step() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    assert!(near(ends(&app, objects[1])[1], [10.0, 50.0]));
    assert!(
        !findings(&app)
            .iter()
            .any(|f| f.kind == EndErrorKind::Overshoot),
        "the list is scanned again after the fix"
    );
    let _ = app.update(Message::Undo);
    assert!(near(ends(&app, objects[1])[1], [10.4, 50.0]));
}

#[test]
fn extend_runs_an_undershoot_on_to_the_wall() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    fix(&mut app, EndErrorKind::Undershoot, Fix::Extend);
    assert!(near(ends(&app, objects[2])[1], [10.0, -50.0]));
}

#[test]
fn stretch_and_move_close_a_gap() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    fix(&mut app, EndErrorKind::Gap, Fix::Stretch);
    assert_eq!(ends(&app, objects[3]), [[-60.0, 0.0], [-39.8, 0.0]]);

    let (mut app, _) = self::app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    fix(&mut app, EndErrorKind::Gap, Fix::Move);
    let moved = ends(&app, objects[3]);
    assert!(near(moved[0], [-59.8, 0.0]) && near(moved[1], [-39.8, 0.0]));
}

#[test]
fn extend_takes_both_ends_of_a_corner_gap_to_the_corner() {
    let (mut app, _) = app();
    let wide = line(&mut app, [-200.0, -200.0], [200.0, 200.0]);
    let a = line(&mut app, [0.0, 0.0], [10.0, 0.0]);
    let b = line(&mut app, [10.2, 0.1], [10.2, 10.0]);
    let found = scan(&mut app, &[wide, a, b]);
    let gap = found.iter().find(|f| f.kind == EndErrorKind::Gap).copied();
    let gap = gap.expect("a gap");
    assert!(gap.fixes().contains(&Fix::ExtendBoth));
    fix(&mut app, EndErrorKind::Gap, Fix::ExtendBoth);
    assert!(near(ends(&app, a)[1], [10.2, 0.0]));
    assert!(near(ends(&app, b)[0], [10.2, 0.0]));
    let _ = app.update(Message::Undo);
    assert!(near(ends(&app, a)[1], [10.0, 0.0]), "one undo step");
    assert!(near(ends(&app, b)[0], [10.2, 0.1]), "one undo step");
}

#[test]
fn an_arc_end_is_trimmed_along_its_circle() {
    let (mut app, i) = app();
    let wall = line(&mut app, [0.0, -100.0], [0.0, 100.0]);
    // A quarter arc about (10, 5) whose end runs 0.02 rad past the wall.
    let arc = add(
        &mut app,
        EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(10.0, 5.0, 0.0),
            10.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI + 0.02,
        )),
    );
    let found = scan(&mut app, &[wall, arc]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, EndErrorKind::Overshoot);
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    match app.tabs[i].scene.document.get_entity(arc) {
        Some(EntityType::Arc(arc)) => {
            let end = std::f64::consts::PI;
            assert!((arc.end_angle.rem_euclid(std::f64::consts::TAU) - end).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_polyline_arc_end_keeps_its_circle_when_extended() {
    let (mut app, i) = app();
    let wall = line(&mut app, [-100.0, 10.0], [100.0, 10.0]);
    // From (20, 0) a straight segment to (10, 0), then an arc of radius 10
    // about (10, 10) that stops 0.03 rad short of the top wall.
    let stop = -std::f64::consts::FRAC_PI_2 - (std::f64::consts::FRAC_PI_2 - 0.03);
    let end = [10.0 + 10.0 * stop.cos(), 10.0 + 10.0 * stop.sin()];
    let mut polyline = LwPolyline::from_points(vec![
        Vector2::new(20.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(end[0], end[1]),
    ]);
    // Clockwise from -90° to `stop`.
    let sweep = stop + std::f64::consts::FRAC_PI_2;
    polyline.vertices[1].bulge = (sweep / 4.0).tan();
    let handle = add(&mut app, EntityType::LwPolyline(polyline));
    let found = scan(&mut app, &[wall, handle]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, EndErrorKind::Undershoot);
    assert!(near(found[0].target, [0.0, 10.0]), "{found:?}");
    fix(&mut app, EndErrorKind::Undershoot, Fix::Extend);
    match app.tabs[i].scene.document.get_entity(handle) {
        Some(EntityType::LwPolyline(polyline)) => {
            let last = polyline.vertices[2].location;
            assert!(near([last.x, last.y], [0.0, 10.0]));
            let quarter = (-std::f64::consts::FRAC_PI_2 / 4.0).tan();
            assert!((polyline.vertices[1].bulge - quarter).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn locked_layers_and_lines_off_the_xy_plane_are_not_scanned() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    {
        let document = &mut app.tabs[i].scene.document;
        let mut layer = document::tables::Layer::new("Locked");
        layer.handle = document.allocate_handle();
        layer.flags.locked = true;
        document.layers.add(layer).unwrap();
        document
            .get_entity_mut(objects[1])
            .unwrap()
            .common_mut()
            .layer = "Locked".into();
        if let Some(EntityType::Line(line)) = document.get_entity_mut(objects[2]) {
            line.end.z = 1.0;
        }
    }
    let kinds: Vec<EndErrorKind> = scan(&mut app, &objects).iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [EndErrorKind::Gap]);
}

#[test]
fn a_fix_after_the_drawing_changed_changes_nothing() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    // The overshooting line is moved away after the scan.
    if let Some(EntityType::Line(line)) = app.tabs[i].scene.document.get_entity_mut(objects[1]) {
        line.start.x -= 50.0;
        line.end.x -= 50.0;
    }
    let before = ends(&app, objects[1]);
    let undo = app.tabs[i].history.undo_stack.len();
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    assert_eq!(ends(&app, objects[1]), before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), undo);
}

#[test]
fn select_objects_scans_what_the_user_selects() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::SelectObjects));
    let cmd = app.tabs[i].active_cmd.as_ref().expect("a selection prompt");
    assert_eq!(cmd.name(), "GEOMETRYCLEANUPOPEN");
    assert!(cmd.is_selection_gathering());
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_selection_complete(objects.to_vec());
    let _ = app.apply_cmd_result(result);
    assert_eq!(findings(&app).len(), 3);
    assert!(app.tabs[i].scene.selected.is_empty());
}

#[test]
fn picking_an_error_selects_its_objects() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    let found = scan(&mut app, &objects);
    let gap = found
        .iter()
        .position(|f| f.kind == EndErrorKind::Gap)
        .unwrap();
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::Pick(gap)));
    assert_eq!(app.geometry_cleanup.picked, Some(gap));
    let selected = &app.tabs[i].scene.selected;
    assert!(selected.contains(&objects[3]) && selected.contains(&objects[4]));
    assert_eq!(selected.len(), 2);
}

/// Open the palette and scan `objects` in it.
fn open_and_scan(app: &mut OpenCADStudio, objects: &[Handle]) {
    let _ = app.dispatch_command("GEOMETRYCLEANUPOPEN");
    scan(app, objects);
}

/// Move a line by hand, outside the palette.
fn shift(app: &mut OpenCADStudio, handle: Handle, dx: f64) {
    let i = app.active_tab;
    if let Some(EntityType::Line(line)) = app.tabs[i].scene.document.get_entity_mut(handle) {
        line.start.x += dx;
        line.end.x += dx;
    }
    app.tabs[i]
        .scene
        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
}

#[test]
fn the_open_palette_lists_an_error_again_after_its_repair_is_undone() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    open_and_scan(&mut app, &objects);
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    assert!(!findings(&app)
        .iter()
        .any(|f| f.kind == EndErrorKind::Overshoot));
    let _ = app.update(Message::Undo);
    assert!(findings(&app)
        .iter()
        .any(|f| f.kind == EndErrorKind::Overshoot));
}

#[test]
fn the_open_palette_scans_again_after_an_edit_and_keeps_the_picked_error() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    open_and_scan(&mut app, &objects);
    let gap = findings(&app)
        .iter()
        .position(|f| f.kind == EndErrorKind::Gap)
        .unwrap();
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::Pick(gap)));
    let picked = findings(&app)[gap];
    // The overshooting line is moved away from the wall.
    shift(&mut app, objects[1], -50.0);
    app.refresh_geometry_cleanup_if_stale();
    let now = findings(&app);
    assert!(
        !now.iter().any(|f| f.kind == EndErrorKind::Overshoot),
        "{now:?}"
    );
    let index = app.geometry_cleanup.picked.expect("still picked");
    assert!(now[index].same_error(&picked));
}

#[test]
fn the_palette_waits_for_a_running_command_before_it_scans_again() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    open_and_scan(&mut app, &objects);
    let before = findings(&app);
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::SelectObjects));
    assert!(app.tabs[i].active_cmd.is_some());
    shift(&mut app, objects[1], -50.0);
    app.refresh_geometry_cleanup_if_stale();
    assert_eq!(findings(&app), before, "no scan while the command runs");
    let _ = app.update(Message::CommandEscape);
    assert!(app.tabs[i].active_cmd.is_none());
    assert_ne!(findings(&app), before, "the scan comes when it ends");
}

#[test]
fn a_closed_palette_does_not_scan_again() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    let before = findings(&app);
    shift(&mut app, objects[1], -50.0);
    app.refresh_geometry_cleanup_if_stale();
    assert_eq!(findings(&app), before);
}

fn picked(app: &OpenCADStudio) -> Option<Finding> {
    app.geometry_cleanup
        .picked
        .map(|index| findings(app)[index])
}

fn selected(app: &OpenCADStudio) -> Vec<Handle> {
    let mut handles: Vec<Handle> = app.tabs[app.active_tab]
        .scene
        .selected
        .iter()
        .copied()
        .collect();
    handles.sort_by_key(|handle| handle.value());
    handles
}

/// The rows are Gap, Overshoot, Undershoot. A repair picks the next row,
/// or the row above it at the end, and selects its objects.
#[test]
fn a_repair_picks_the_next_row() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    let [_, _, under, left, right] = objects;
    open_and_scan(&mut app, &objects);
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    assert_eq!(picked(&app).map(|f| f.kind), Some(EndErrorKind::Undershoot));
    assert_eq!(selected(&app), [under]);
    fix(&mut app, EndErrorKind::Undershoot, Fix::Extend);
    assert_eq!(picked(&app).map(|f| f.kind), Some(EndErrorKind::Gap));
    assert_eq!(selected(&app), [left, right]);
    fix(&mut app, EndErrorKind::Gap, Fix::Stretch);
    assert_eq!(picked(&app), None, "no error is left");
}

#[test]
fn a_folded_type_is_skipped_by_the_next_pick() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    open_and_scan(&mut app, &objects);
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::Fold(
        EndErrorKind::Undershoot,
    )));
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    assert_eq!(picked(&app).map(|f| f.kind), Some(EndErrorKind::Gap));
}

#[test]
fn a_repair_that_fails_keeps_the_pick() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    open_and_scan(&mut app, &objects);
    let gap = findings(&app)
        .iter()
        .position(|f| f.kind == EndErrorKind::Gap)
        .unwrap();
    let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::Pick(gap)));
    // The overshoot is gone before its repair is pressed; the list is not
    // scanned again in between, as in a palette that waits for a command.
    let i = app.active_tab;
    if let Some(EntityType::Line(line)) = app.tabs[i].scene.document.get_entity_mut(objects[1]) {
        line.end.x = 10.0;
    }
    fix(&mut app, EndErrorKind::Overshoot, Fix::Trim);
    assert_eq!(picked(&app).map(|f| f.kind), Some(EndErrorKind::Gap));
}

#[test]
fn opening_the_palette_scans_the_selected_objects_only_when_there_are_some() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    let _ = app.dispatch_command("GEOMETRYCLEANUPOPEN");
    assert!(app.geometry_cleanup.scan.is_none(), "nothing was selected");
    for &handle in &objects {
        app.tabs[i].scene.select_entity(handle, false);
    }
    let _ = app.dispatch_command("GEOMETRYCLEANUPOPEN");
    assert_eq!(findings(&app).len(), 3);
}

#[test]
fn folding_a_type_again_unfolds_it() {
    let (mut app, _) = app();
    let fold = |app: &mut OpenCADStudio| {
        let _ = app.update(Message::GeometryCleanup(GeometryCleanupMsg::Fold(
            EndErrorKind::Gap,
        )));
    };
    fold(&mut app);
    assert_eq!(app.geometry_cleanup.folded, [EndErrorKind::Gap]);
    fold(&mut app);
    assert!(app.geometry_cleanup.folded.is_empty());
}

#[test]
fn move_takes_the_whole_object_across_a_gap_in_x_and_y() {
    let (mut app, _) = app();
    let a = line(&mut app, [-40.0, 5.0], [10.0, 5.0]);
    let b = line(&mut app, [10.2, 5.1], [60.0, 5.1]);
    let kinds: Vec<EndErrorKind> = scan(&mut app, &[a, b]).iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [EndErrorKind::Gap]);
    fix(&mut app, EndErrorKind::Gap, Fix::Move);
    let moved = ends(&app, a);
    assert!(
        near(moved[0], [-39.8, 5.1]) && near(moved[1], [10.2, 5.1]),
        "{moved:?}"
    );
}

#[test]
fn a_repair_that_the_error_does_not_offer_changes_nothing() {
    let (mut app, i) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    let undo = app.tabs[i].history.undo_stack.len();
    fix(&mut app, EndErrorKind::Gap, Fix::Trim);
    assert_eq!(ends(&app, objects[3]), [[-60.0, 0.0], [-40.0, 0.0]]);
    assert_eq!(app.tabs[i].history.undo_stack.len(), undo);
}

#[test]
fn arcs_and_polylines_off_the_world_xy_plane_are_not_read() {
    use crate::modules::manage::geometry_cleanup::cleanup_curve;
    let arc = Arc::from_center_radius_angles(Vector3::new(1.0, 2.0, 0.0), 5.0, 0.0, 1.0);
    let polyline = LwPolyline::from_points(vec![Vector2::new(0.0, 0.0), Vector2::new(5.0, 0.0)]);
    assert!(cleanup_curve(&EntityType::Arc(arc.clone())).is_some());
    assert!(cleanup_curve(&EntityType::LwPolyline(polyline.clone())).is_some());
    for normal in [
        Vector3::new(0.0, 0.0, -1.0),
        Vector3::new(0.6, 0.0, 0.8),
        Vector3::new(0.0, 0.6, 0.8),
    ] {
        let mut tilted = arc.clone();
        tilted.normal = normal;
        assert!(
            cleanup_curve(&EntityType::Arc(tilted)).is_none(),
            "{normal:?}"
        );
        let mut tilted = polyline.clone();
        tilted.normal = normal;
        assert!(
            cleanup_curve(&EntityType::LwPolyline(tilted)).is_none(),
            "{normal:?}"
        );
    }
    let mut raised = arc;
    raised.center.z = 1.0;
    assert!(cleanup_curve(&EntityType::Arc(raised)).is_none());
    let mut raised = polyline;
    raised.elevation = 1.0;
    assert!(cleanup_curve(&EntityType::LwPolyline(raised)).is_none());
}

#[test]
fn a_two_point_polyline_is_extended_at_its_end() {
    let (mut app, i) = app();
    let wall = line(&mut app, [10.0, -100.0], [10.0, 100.0]);
    let handle = add(
        &mut app,
        EntityType::LwPolyline(LwPolyline::from_points(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(9.7, 0.0),
        ])),
    );
    let kinds: Vec<EndErrorKind> = scan(&mut app, &[wall, handle])
        .iter()
        .map(|f| f.kind)
        .collect();
    assert_eq!(kinds, [EndErrorKind::Undershoot]);
    fix(&mut app, EndErrorKind::Undershoot, Fix::Extend);
    match app.tabs[i].scene.document.get_entity(handle) {
        Some(EntityType::LwPolyline(polyline)) => {
            let last = polyline.vertices[1].location;
            assert!(near([last.x, last.y], [10.0, 0.0]), "{last:?}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn stretch_moves_the_grip_at_the_end_that_it_repairs() {
    use crate::modules::manage::geometry_cleanup::stretched;
    for (start, end, to) in [
        ([10.0, 5.0], [20.0, 5.0], [25.0, 5.0]),
        ([5.0, 10.0], [5.0, 20.0], [5.0, 25.0]),
    ] {
        let entity = EntityType::Line(Line::from_points(
            Vector3::new(start[0], start[1], 0.0),
            Vector3::new(end[0], end[1], 0.0),
        ));
        match stretched(&entity, false, to) {
            Some(EntityType::Line(line)) => {
                assert!(near([line.start.x, line.start.y], start), "{line:?}");
                assert!(near([line.end.x, line.end.y], to), "{line:?}");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn the_same_error_is_the_same_type_at_the_same_end_of_the_same_object() {
    let error = Finding {
        kind: EndErrorKind::Gap,
        handle: Handle::new(1),
        at_start: true,
        location: [0.0, 0.0],
        target: [0.1, 0.0],
        other: None,
        corner: None,
        deviation: 0.1,
    };
    let moved = Finding {
        location: [1.0, 1.0],
        deviation: 0.2,
        ..error
    };
    assert!(error.same_error(&moved));
    for other in [
        Finding {
            kind: EndErrorKind::Overshoot,
            ..error
        },
        Finding {
            handle: Handle::new(2),
            ..error
        },
        Finding {
            at_start: false,
            ..error
        },
    ] {
        assert!(!error.same_error(&other), "{other:?}");
    }
}

#[test]
fn the_settings_are_kept_in_the_user_profile() {
    let (mut app, _) = app();
    app.geometry_cleanup.settings = Some(CleanupSettings::proposed(0.4));
    let json = serde_json::to_string(&app.current_config()).unwrap();
    let loaded: crate::app::config::AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        loaded.geometry_cleanup,
        Some(CleanupSettings::proposed(0.4))
    );
    let (mut other, _) = self::app();
    other.apply_config(loaded);
    assert_eq!(
        other.geometry_cleanup.settings,
        Some(CleanupSettings::proposed(0.4))
    );
}

#[test]
fn a_profile_from_before_the_settings_has_none() {
    let loaded: crate::app::config::AppConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(loaded.geometry_cleanup, None);
}

#[test]
fn a_scan_uses_the_saved_tolerance_of_each_type() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    let mut settings = CleanupSettings::proposed(0.5198);
    settings.set(EndErrorKind::Undershoot, None);
    settings.set(EndErrorKind::Gap, Some(0.1));
    app.geometry_cleanup.settings = Some(settings);
    let kinds: Vec<EndErrorKind> = scan(&mut app, &objects).iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [EndErrorKind::Overshoot]);
}

#[test]
fn without_saved_settings_each_type_uses_the_proposed_tolerance() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    scan(&mut app, &objects);
    let tolerances = app.geometry_cleanup.scan.as_ref().unwrap().tolerances;
    assert!(
        (tolerances.gap.unwrap() - 0.5198).abs() < 1e-9,
        "{tolerances:?}"
    );
    assert_eq!(tolerances.gap, tolerances.overshoot);
    assert_eq!(tolerances.gap, tolerances.undershoot);
}

fn settings_msg(app: &mut OpenCADStudio, m: GeometryCleanupMsg) {
    let _ = app.update(Message::GeometryCleanup(m));
}

/// A folder of its own for each test that writes files.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ocs-gcs-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn shown_kinds(app: &OpenCADStudio) -> Vec<EndErrorKind> {
    let dialog = app.geometry_cleanup.dialog.as_ref().expect("the dialog");
    dialog.rows.iter().map(|(kind, _)| *kind).collect()
}

#[test]
fn the_settings_dialog_opens_with_the_proposed_tolerances() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    assert_eq!(
        app.active_modal,
        Some(crate::app::ModalKind::GeometryCleanupSettings)
    );
    let dialog = app.geometry_cleanup.dialog.as_ref().expect("the dialog");
    assert_eq!(dialog.rows.len(), 3);
    assert!((dialog.proposed - 0.5198).abs() < 1e-9);
    assert_eq!(
        dialog.parsed(),
        Some(CleanupSettings::proposed(0.5198)),
        "the boxes hold the proposed tolerance"
    );
}

#[test]
fn scan_saves_the_settings_and_scans_the_drawing() {
    // No scan before: Scan scans all of model space.
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsRemove(EndErrorKind::Undershoot),
    );
    settings_msg(&mut app, GeometryCleanupMsg::SettingsScan);
    assert!(app.geometry_cleanup.dialog.is_none() && app.active_modal.is_none());
    assert!(app.geometry_cleanup.show, "the palette shows the result");
    let saved = app.geometry_cleanup.settings.as_ref().expect("saved");
    assert_eq!(saved.tolerance(EndErrorKind::Undershoot), None);
    let mut kinds: Vec<EndErrorKind> = findings(&app).iter().map(|f| f.kind).collect();
    kinds.sort();
    assert_eq!(kinds, [EndErrorKind::Gap, EndErrorKind::Overshoot]);
}

#[test]
fn scan_rescans_the_objects_of_the_last_scan() {
    let (mut app, _) = app();
    let objects = errors_drawing(&mut app);
    // A line 0.2 short of a second wall, not in the first scan.
    let extra = line(&mut app, [100.0, 0.0], [109.8, 0.0]);
    let _wall = line(&mut app, [110.0, -100.0], [110.0, 100.0]);
    scan(&mut app, &objects);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(&mut app, GeometryCleanupMsg::SettingsScan);
    assert!(
        !findings(&app).iter().any(|f| f.handle == extra),
        "only the objects of the last scan"
    );
    assert_eq!(findings(&app).len(), 3);
}

#[test]
fn scan_waits_for_tolerances_that_are_numbers() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsTolerance(EndErrorKind::Gap, "abc".into()),
    );
    settings_msg(&mut app, GeometryCleanupMsg::SettingsScan);
    assert!(app.geometry_cleanup.dialog.is_some());
    assert!(app.geometry_cleanup.settings.is_none());
}

#[test]
fn cancel_changes_nothing() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsRemove(EndErrorKind::Gap),
    );
    settings_msg(&mut app, GeometryCleanupMsg::SettingsCancel);
    assert!(app.geometry_cleanup.dialog.is_none() && app.active_modal.is_none());
    assert!(app.geometry_cleanup.settings.is_none());
    assert!(app.geometry_cleanup.scan.is_none());
}

#[test]
fn the_proposed_tolerance_is_put_in_the_box_and_a_removed_type_can_be_added_back() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsTolerance(EndErrorKind::Gap, "1".into()),
    );
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsUseProposed(EndErrorKind::Gap),
    );
    let gap = |app: &OpenCADStudio| {
        let dialog = app.geometry_cleanup.dialog.as_ref().unwrap();
        dialog.parsed().unwrap().tolerance(EndErrorKind::Gap)
    };
    assert_eq!(gap(&app), Some(0.5198));
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsRemove(EndErrorKind::Overshoot),
    );
    assert_eq!(
        app.geometry_cleanup.dialog.as_ref().unwrap().removed(),
        [EndErrorKind::Overshoot]
    );
    assert_eq!(
        shown_kinds(&app),
        [EndErrorKind::Gap, EndErrorKind::Undershoot]
    );
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsAdd(EndErrorKind::Overshoot),
    );
    assert_eq!(
        shown_kinds(&app),
        [
            EndErrorKind::Gap,
            EndErrorKind::Overshoot,
            EndErrorKind::Undershoot
        ]
    );
    let dialog = app.geometry_cleanup.dialog.as_ref().unwrap();
    assert_eq!(
        dialog.parsed().unwrap().tolerance(EndErrorKind::Overshoot),
        Some(0.5198)
    );
}

#[test]
fn import_reads_a_full_file_and_export_writes_its_angle_rows_back() {
    let dir = scratch("import-export");
    let source = dir.join("test.gcs");
    std::fs::write(&source, SAMPLE_GCS).unwrap();
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsImported(source.clone()),
    );
    let dialog = app.geometry_cleanup.dialog.as_ref().unwrap();
    assert_eq!(dialog.rows[0].1, "0.4");
    assert_eq!(dialog.hidden.len(), 4);
    assert_eq!(
        dialog.file.as_deref(),
        Some(source.display().to_string().as_str())
    );
    let target = dir.join("out.gcs");
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsExportTo(target.clone()),
    );
    let written = std::fs::read_to_string(&target).unwrap();
    assert_eq!(
        CleanupSettings::parse_gcs(&written).unwrap(),
        CleanupSettings::parse_gcs(SAMPLE_GCS).unwrap()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_that_cannot_be_read_changes_nothing() {
    let dir = scratch("unreadable");
    let broken = dir.join("broken.gcs");
    std::fs::write(&broken, "not json").unwrap();
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    let before = app.geometry_cleanup.dialog.as_ref().unwrap().parsed();
    for path in [dir.join("missing.gcs"), broken] {
        let errors = app.command_line.history.len();
        settings_msg(&mut app, GeometryCleanupMsg::SettingsImported(path));
        assert_eq!(
            app.geometry_cleanup.dialog.as_ref().unwrap().parsed(),
            before
        );
        assert!(app.command_line.history.len() > errors, "an error is shown");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scan_with_no_types_finds_nothing() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    for kind in [
        EndErrorKind::Gap,
        EndErrorKind::Overshoot,
        EndErrorKind::Undershoot,
    ] {
        settings_msg(&mut app, GeometryCleanupMsg::SettingsRemove(kind));
    }
    settings_msg(&mut app, GeometryCleanupMsg::SettingsScan);
    assert!(app.geometry_cleanup.scan.is_some());
    assert!(findings(&app).is_empty());
}

#[test]
fn scan_from_another_tab_scans_the_current_drawing() {
    let (mut app, _) = app();
    let first = errors_drawing(&mut app);
    scan(&mut app, &first[..1]);
    app.push_test_document();
    app.active_tab = app.tabs.len() - 1;
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(&mut app, GeometryCleanupMsg::SettingsScan);
    let scan = app.geometry_cleanup.scan.as_ref().unwrap();
    assert_eq!(scan.tab, app.tabs[i].id);
    assert_eq!(scan.findings.len(), 3);
}

#[test]
fn a_tolerance_too_large_to_keep_disables_scan() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsTolerance(EndErrorKind::Gap, "1e303".into()),
    );
    assert_eq!(app.geometry_cleanup.dialog.as_ref().unwrap().parsed(), None);
    settings_msg(&mut app, GeometryCleanupMsg::SettingsScan);
    assert!(app.geometry_cleanup.settings.is_none());
}

#[test]
fn a_profile_with_settings_that_cannot_be_read_keeps_the_rest() {
    let json = r#"{"show_command_history": true,
        "geometry_cleanup": {"tolerances": [{"name": "gap", "tolerance": null}]}}"#;
    let loaded: crate::app::config::AppConfig = serde_json::from_str(json).unwrap();
    assert!(
        loaded.show_command_history,
        "the rest of the profile is kept"
    );
    assert_eq!(loaded.geometry_cleanup, None);
}

#[test]
fn the_proposed_tolerance_has_at_most_six_decimals() {
    // Lengths 1, 1 and 2: the mean is 4/3, 1% of it 0.013333...
    let (mut app, _) = app();
    let a = line(&mut app, [0.0, 0.0], [1.0, 0.0]);
    let b = line(&mut app, [0.0, 5.0], [1.0, 5.0]);
    let c = line(&mut app, [0.0, 9.0], [2.0, 9.0]);
    scan(&mut app, &[a, b, c]);
    let tolerances = app.geometry_cleanup.scan.as_ref().unwrap().tolerances;
    assert_eq!(tolerances.gap, Some(0.013333));
}

#[test]
fn a_typed_tolerance_changes_only_its_own_type() {
    let (mut app, _) = app();
    errors_drawing(&mut app);
    settings_msg(&mut app, GeometryCleanupMsg::OpenSettings);
    settings_msg(
        &mut app,
        GeometryCleanupMsg::SettingsTolerance(EndErrorKind::Gap, "0.25".into()),
    );
    let parsed = app
        .geometry_cleanup
        .dialog
        .as_ref()
        .unwrap()
        .parsed()
        .unwrap();
    assert_eq!(parsed.tolerance(EndErrorKind::Gap), Some(0.25));
    assert_eq!(parsed.tolerance(EndErrorKind::Overshoot), Some(0.5198));
    assert_eq!(parsed.tolerance(EndErrorKind::Undershoot), Some(0.5198));
}
