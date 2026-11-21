//! A click in empty space that moves a little while the button is down is
//! still the first corner of a click-click window when both gestures are on.

use super::*;
use document::entities::Line;
use document::types::Vector3;
use document::Handle;
use std::time::Duration;

fn drawing() -> (OpenCADStudio, Handle, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let mut line = |a: [f64; 2], b: [f64; 2]| {
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            )))
    };
    let inside = line([4.0, 4.0], [6.0, 6.0]);
    let away = line([30.0, 30.0], [32.0, 30.0]);
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-2., -2., 0.),
        glam::DVec3::new(34., 34., 0.),
        1600. / 900.,
    );
    (app, inside, away)
}

fn screen(app: &OpenCADStudio, x: f64, y: f64) -> Point {
    let scene = &app.tabs[app.active_tab].scene;
    let (w, h) = scene.selection.borrow().view.vp_size;
    let b = scene.active_model_tile_bounds(w, h);
    let s = scene
        .camera
        .borrow()
        .project(
            glam::DVec3::new(x, y, 0.0),
            iced::Rectangle {
                x: 0.,
                y: 0.,
                width: b.width,
                height: b.height,
            },
        )
        .unwrap();
    Point::new(s.x + b.x, s.y + b.y)
}

/// Press at `at`, move by `offset` screen pixels after `held_ms` of holding,
/// release where the cursor ended.
fn press_move_release(app: &mut OpenCADStudio, at: Point, offset: (f32, f32), held_ms: u64) {
    let _ = app.on_viewport_move(at);
    let _ = app.update(Message::ViewportLeftPressAt(at));
    {
        let i = app.active_tab;
        let mut sel = app.tabs[i].scene.selection.borrow_mut();
        sel.input.left_press_time = sel
            .input
            .left_press_time
            .map(|time| time - Duration::from_millis(held_ms));
    }
    let _ = app.on_viewport_move(Point::new(at.x + offset.0, at.y + offset.1));
    let _ = app.update(Message::ViewportLeftRelease);
}

fn armed_window(app: &OpenCADStudio) -> bool {
    let sel = app.tabs[app.active_tab].scene.selection.borrow();
    sel.gesture.box_anchor.is_some() && !sel.gesture.poly_active && !sel.input.left_dragging
}

fn selected(app: &OpenCADStudio, h: Handle) -> bool {
    app.tabs[app.active_tab].scene.selected.contains(&h)
}

/// The first click wobbles 8 px, the second click closes the window.
fn wobbly_window(pick_auto: u8) -> (OpenCADStudio, Handle, Handle) {
    let (mut app, inside, away) = drawing();
    app.pick_auto = pick_auto;
    app.pick_drag_mode = 2;
    let first = screen(&app, 2.0, 2.0);
    press_move_release(&mut app, first, (6.0, -5.0), 0);
    assert!(
        armed_window(&app),
        "PICKAUTO {pick_auto}: a quick click that wobbles must arm a window"
    );
    let second = screen(&app, 10.0, 10.0);
    press_move_release(&mut app, second, (0.0, 0.0), 0);
    (app, inside, away)
}

#[test]
fn a_wobbly_click_starts_a_click_click_window_with_the_lasso_on() {
    let (app, inside, away) = wobbly_window(5);
    assert!(selected(&app, inside));
    assert!(!selected(&app, away));
}

#[test]
fn a_wobbly_click_starts_a_click_click_window_with_the_lasso_off() {
    let (app, inside, away) = wobbly_window(1);
    assert!(selected(&app, inside));
    assert!(!selected(&app, away));
}

#[test]
fn a_held_press_and_drag_still_selects_on_release() {
    let (mut app, inside, away) = drawing();
    app.pick_auto = 1;
    app.pick_drag_mode = 2;
    let from = screen(&app, 2.0, 2.0);
    let to = screen(&app, 10.0, 10.0);
    press_move_release(&mut app, from, (to.x - from.x, to.y - from.y), 400);
    assert!(selected(&app, inside));
    assert!(!selected(&app, away));
    assert!(!armed_window(&app));
}

#[test]
fn a_fast_long_rectangle_stroke_is_a_drag_without_the_lasso() {
    let (mut app, inside, _) = drawing();
    app.pick_auto = 1;
    app.pick_drag_mode = 2;
    let from = screen(&app, 2.0, 2.0);
    let to = screen(&app, 10.0, 10.0);
    press_move_release(&mut app, from, (to.x - from.x, to.y - from.y), 0);
    assert!(selected(&app, inside));
    assert!(!armed_window(&app));
}

#[test]
fn press_and_drag_only_keeps_its_short_start() {
    let (mut app, _, _) = drawing();
    app.pick_auto = 1;
    app.pick_drag_mode = 1;
    let from = screen(&app, 2.0, 2.0);
    let _ = app.on_viewport_move(from);
    let _ = app.update(Message::ViewportLeftPressAt(from));
    let _ = app.on_viewport_move(Point::new(from.x + 6.0, from.y));
    let i = app.active_tab;
    assert!(app.tabs[i].scene.selection.borrow().input.left_dragging);
}
