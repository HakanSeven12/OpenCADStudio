//! Tab cycles the object snaps under the cursor at a point prompt.

use super::*;
use crate::snap::SnapType;
use document::{entities::Line, types::Vector3, EntityType};
use glam::DVec3;

/// Start points of the two lines: 0.875 units apart, both in the aperture.
const P: DVec3 = DVec3::new(40.0, 30.0, 0.0);
const Q: DVec3 = DVec3::new(40.75, 30.45, 0.0);
/// The far endpoints, outside the aperture.
const P_FAR: DVec3 = DVec3::new(90.0, 47.0, 0.0);
const Q_FAR: DVec3 = DVec3::new(13.0, 71.0, 0.0);
const APERTURE_PX: f32 = 12.0;

/// LINE waits for its first point over two angled lines whose start points
/// are both in the aperture. Only Endpoint is on.
fn two_lines_app(modes: &[SnapType]) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    for (a, b) in [(P, P_FAR), (Q, Q_FAR)] {
        app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(a.x, a.y, a.z),
                Vector3::new(b.x, b.y, b.z),
            )));
    }
    app.snapper.snap_enabled = true;
    app.snapper.enabled = modes.iter().copied().collect();
    app.snapper.snap3d_enabled = false;
    app.snapper.grid_snap_on = false;
    app.snapper.otrack_enabled = false;
    app.snapper.osnap_radius_px = APERTURE_PX;
    app.ortho_mode = false;
    app.polar_mode = false;
    app.dynamic_ucs = false;
    app.dyn_input = true;
    let _ = app.run_command_line("ZOOM EXTENTS");
    let _ = app.run_command_line("LINE");
    assert!(app.tabs[i].active_cmd.is_some());
    app
}

fn screen(app: &OpenCADStudio, point: DVec3) -> Point {
    let p = app.tabs[app.active_tab]
        .scene
        .camera
        .borrow()
        .project(
            point,
            iced::Rectangle::with_size(iced::Size::new(800.0, 600.0)),
        )
        .unwrap();
    Point::new(p.x, p.y)
}

/// One pointer move, with the coalescing gate shut (see
/// `rapid_drawing_tests::move_to`).
fn move_to(app: &mut OpenCADStudio, at: Point) {
    app.move_pass_due = None;
    let _ = app.update(Message::ViewportMove(at));
}

/// The pointer two pixels from P, towards Q.
fn near_p(app: &OpenCADStudio) -> Point {
    let (p, q) = (screen(app, P), screen(app, Q));
    let d = p.distance(q);
    // More than the 5 px hold margin of a held snap, and in the aperture.
    assert!(d > 6.0 && d < APERTURE_PX - 2.0, "P and Q are {d} px apart");
    Point::new(p.x + (q.x - p.x) * 2.0 / d, p.y + (q.y - p.y) * 2.0 / d)
}

fn snapped(app: &OpenCADStudio) -> (SnapType, DVec3) {
    let hit = app.tabs[app.active_tab].snap_result.expect("a snap");
    (hit.snap_type, hit.world)
}

fn assert_at(app: &OpenCADStudio, expected: DVec3, step: &str) {
    let (kind, world) = snapped(app);
    assert_eq!(kind, SnapType::Endpoint, "{step}");
    assert!(
        (world - expected).length() < 1e-6,
        "{step}: snap at {world}, expected {expected}"
    );
}

#[test]
fn tab_steps_through_the_snaps_under_the_cursor_and_starts_again() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    assert_at(&app, P, "the nearest endpoint first");

    // The aperture first, then the other points of the lines under the
    // cursor, nearest first; then the cycle starts again.
    let near_far = if P_FAR.distance(P) < Q_FAR.distance(P) {
        [P_FAR, Q_FAR]
    } else {
        [Q_FAR, P_FAR]
    };
    for (step, expected) in [Q, near_far[0], near_far[1], P, Q].into_iter().enumerate() {
        let _ = app.update(Message::DynTabNext);
        assert_at(&app, expected, &format!("Tab {}", step + 1));
    }
    assert_eq!(app.tabs[app.active_tab].dyn_active, 0, "the field stays");

    // A click takes the snap Tab showed, and ends the cycle.
    let _ = app.update(Message::ViewportLeftPressAt(at));
    let _ = app.update(Message::ViewportLeftRelease);
    assert!((app.last_point.expect("a point") - Q).length() < 1e-6);
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn a_small_move_keeps_the_cycle_and_leaving_the_aperture_ends_it() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    let _ = app.update(Message::DynTabNext);
    assert_at(&app, Q, "Tab");

    // A move onto P on the throttled path keeps the skipped P away.
    app.snap_dwell
        .as_mut()
        .expect("the full pass arms the dwell")
        .last_preview_snap_at = Instant::now() - std::time::Duration::from_millis(100);
    let on_p = screen(&app, P);
    move_to(&mut app, on_p);
    assert_at(&app, Q, "a small move");

    // No pending dwell: these moves take the full snap pass.
    app.snap_dwell = None;
    move_to(&mut app, Point::new(at.x + 3.0 * APERTURE_PX, at.y));
    assert_eq!(app.snapper.cycle_len(), 0);
    app.snap_dwell = None;
    move_to(&mut app, at);
    assert_at(&app, P, "back after leaving the aperture");
}

#[test]
fn tab_moves_to_the_next_field_when_a_value_is_typed() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let i = app.active_tab;
    let at = near_p(&app);
    move_to(&mut app, at);
    assert!(
        app.tabs[i].dyn_fields.len() >= 2,
        "dynamic input shows fields"
    );
    let _ = app.update(Message::CommandAppendChar("5".into()));
    assert!(
        app.tabs[i].dyn_fields[0].locked(),
        "the value is in the field"
    );

    let _ = app.update(Message::DynTabNext);
    assert_eq!(app.tabs[i].dyn_active, 1);
    assert_at(&app, P, "the snap stays");
}

#[test]
fn tab_keeps_the_snap_when_an_option_is_typed() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    let _ = app.update(Message::CommandAppendChar("U".into()));
    assert_eq!(app.command_line.input, "U");

    let _ = app.update(Message::DynTabNext);
    assert_at(&app, P, "the snap stays");
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn the_cycle_leaves_out_the_grid_point() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    app.snapper.grid_snap_on = true;
    let at = near_p(&app);
    move_to(&mut app, at);
    assert_at(&app, P, "an object snap wins over the grid");
    for _ in 0..4 {
        let _ = app.update(Message::DynTabNext);
    }
    assert_at(&app, P, "after the four endpoints, P again");
}

#[test]
fn tab_leaves_a_snap_it_cannot_step_past() {
    // A Parallel snap comes from the acquired reference line, not from the
    // candidates that Tab steps through: Tab keeps it and starts no cycle.
    let mut app = two_lines_app(&[SnapType::Parallel]);
    let base = DVec3::new(20.0, 60.0, 0.0);
    let click = screen(&app, base);
    let _ = app.update(Message::ViewportLeftPressAt(click));
    let _ = app.update(Message::ViewportLeftRelease);
    assert!((app.last_point.expect("the first point") - base).length() < 1e-3);
    let dir = (P_FAR - P).normalize();
    app.snapper.parallel_ref = Some((dir, P));
    let at = screen(&app, base + dir * 15.0);
    move_to(&mut app, at);
    let (kind, world) = snapped(&app);
    assert_eq!(kind, SnapType::Parallel);

    let _ = app.update(Message::DynTabNext);
    assert_eq!(snapped(&app), (kind, world), "the snap stays");
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn tab_moves_to_the_next_field_when_only_one_snap_is_found() {
    let mut app = two_lines_app(&[SnapType::Midpoint]);
    let i = app.active_tab;
    let mid = (P + P_FAR) / 2.0;
    let at = screen(&app, mid);
    move_to(&mut app, at);
    let (kind, world) = snapped(&app);
    assert_eq!(kind, SnapType::Midpoint);
    assert!((world - mid).length() < 1e-6);
    assert!(
        app.tabs[i].dyn_fields.len() >= 2,
        "dynamic input shows fields"
    );

    let _ = app.update(Message::DynTabNext);
    assert_eq!(app.tabs[i].dyn_active, 1);
    assert_eq!(snapped(&app), (kind, world), "the snap stays");
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn a_new_command_starts_a_new_cycle() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    let _ = app.update(Message::DynTabNext);
    assert_at(&app, Q, "Tab");

    let _ = app.update(Message::CommandEscape);
    let _ = app.run_command_line("LINE");
    app.snap_dwell = None;
    move_to(&mut app, at);
    assert_at(&app, P, "the new command");
}

/// The far endpoints in the order Tab shows them: nearest to P first.
fn far_endpoints() -> [DVec3; 2] {
    if P_FAR.distance(P) < Q_FAR.distance(P) {
        [P_FAR, Q_FAR]
    } else {
        [Q_FAR, P_FAR]
    }
}

#[test]
fn shift_tab_steps_back_through_the_snaps_and_wraps_to_the_last() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    let [far1, far2] = far_endpoints();

    // Tab order: P, Q, far1, far2. Shift+Tab goes the other way, and at
    // the first snap it goes to the last one.
    let steps = [
        (Message::DynTabNext, Q, "Tab"),
        (Message::DynTabNext, far1, "Tab"),
        (Message::DynTabPrev, Q, "Shift+Tab"),
        (Message::DynTabPrev, P, "Shift+Tab to the first"),
        (Message::DynTabPrev, far2, "Shift+Tab to the last"),
        (Message::DynTabPrev, far1, "Shift+Tab"),
        (Message::DynTabNext, far2, "Tab"),
        (Message::DynTabNext, P, "Tab to the first"),
    ];
    for (step, (message, expected, name)) in steps.into_iter().enumerate() {
        let _ = app.update(message);
        assert_at(&app, expected, &format!("step {}: {name}", step + 1));
    }
    assert_eq!(app.tabs[app.active_tab].dyn_active, 0, "the field stays");
}

#[test]
fn the_shift_tab_key_shows_the_last_snap_and_a_click_takes_it() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    let task = app.update_root(Message::ShortcutPressed("SHIFT+TAB".into()));
    app.drive_headless_task(task).unwrap();
    let last = far_endpoints()[1];
    assert_at(&app, last, "Shift+Tab");

    let _ = app.update(Message::ViewportLeftPressAt(at));
    let _ = app.update(Message::ViewportLeftRelease);
    assert!((app.last_point.expect("a point") - last).length() < 1e-6);
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn shift_tab_keeps_the_snap_when_a_value_is_typed() {
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    let _ = app.update(Message::CommandAppendChar("5".into()));

    let _ = app.update(Message::DynTabPrev);
    assert_at(&app, P, "the snap stays");
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn shift_tab_leaves_a_snap_it_cannot_step_past() {
    let mut app = two_lines_app(&[SnapType::Midpoint]);
    let mid = (P + P_FAR) / 2.0;
    let at = screen(&app, mid);
    move_to(&mut app, at);
    let (kind, world) = snapped(&app);
    assert_eq!(kind, SnapType::Midpoint);

    let _ = app.update(Message::DynTabPrev);
    assert_eq!(snapped(&app), (kind, world), "the snap stays");
    assert_eq!(app.snapper.cycle_len(), 0);
}

#[test]
fn shift_tab_steps_past_a_bounded_number_of_snaps() {
    use crate::app::update::viewport_snap::MAX_CYCLE_STEPS;
    use document::{entities::LwPolyline, types::Vector2};
    // A polyline from P with more vertices than the bound: each one is an
    // endpoint snap of the object under the cursor.
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let i = app.active_tab;
    let vertices = (0..2 * MAX_CYCLE_STEPS)
        .map(|k| Vector2::new(P.x + 0.4 * k as f64, P.y - 0.3 * (k % 2) as f64))
        .collect();
    app.tabs[i]
        .scene
        .add_entity(EntityType::LwPolyline(LwPolyline::from_points(vertices)));
    let at = near_p(&app);
    move_to(&mut app, at);
    assert_at(&app, P, "the nearest endpoint first");

    let _ = app.update(Message::DynTabPrev);
    assert_eq!(app.snapper.cycle_len(), MAX_CYCLE_STEPS - 1);
    assert_ne!(snapped(&app).1, P, "a snap past the first one shows");
}

#[test]
fn shift_tab_moves_up_the_suggestions_when_they_show() {
    use crate::command::CommandSuggestion;
    let mut app = two_lines_app(&[SnapType::Endpoint]);
    let at = near_p(&app);
    move_to(&mut app, at);
    app.command_line
        .set_contextual_suggestions(["A", "B", "C"].map(CommandSuggestion::plain).into());

    let _ = app.update(Message::DynTabPrev);
    assert_eq!(
        app.command_line.autocomplete_cursor,
        Some(2),
        "the last row"
    );
    assert_at(&app, P, "the snap stays");
    assert_eq!(app.snapper.cycle_len(), 0);
}
