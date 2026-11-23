use super::*;

use super::*;
use crate::app::OpenCADStudio;
use crate::ui::overlay::NavCursor;

fn zoom_armed_app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.tabs[0].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let _ = app.dispatch_command("ZOOM DYNAMIC");
    app
}

fn drag_to(app: &mut OpenCADStudio, y: f32) {
    let _ = app.update(Message::ViewportMove(Point::new(400.0, y)));
}

fn zoom_dir_out(app: &OpenCADStudio) -> bool {
    app.tabs[0].scene.selection.borrow().input.zoom_dir_out
}

/// The magnifier has to carry the sign of the zoom actually under way:
/// dragging up zooms in, dragging back down zooms out.
#[test]
fn the_magnifier_follows_the_drag_direction() {
    let mut app = zoom_armed_app();
    assert!(app.tabs[0].zoom_dynamic_mode);
    drag_to(&mut app, 300.0);
    let _ = app.update(Message::ViewportLeftPress);
    // A fresh drag starts neutral — "+" until told otherwise.
    assert!(!zoom_dir_out(&app));

    drag_to(&mut app, 200.0);
    assert!(!zoom_dir_out(&app), "dragging up zooms in");

    drag_to(&mut app, 400.0);
    assert!(zoom_dir_out(&app), "dragging down zooms out");

    drag_to(&mut app, 250.0);
    assert!(!zoom_dir_out(&app), "dragging up again zooms in");

    // Releasing disarms the direction, so the next hover shows "+".
    let _ = app.update(Message::ViewportLeftRelease);
    assert!(!zoom_dir_out(&app));
}

/// Sub-pixel wobble while a hand holds still must not strobe the glyph.
#[test]
fn jitter_below_the_dead_zone_does_not_flip_the_glyph() {
    let mut app = zoom_armed_app();
    drag_to(&mut app, 300.0);
    let _ = app.update(Message::ViewportLeftPress);
    drag_to(&mut app, 400.0);
    assert!(zoom_dir_out(&app));

    // Back up by less than ZOOM_DIR_FLIP_PX: still zooming out.
    drag_to(&mut app, 399.0);
    assert!(zoom_dir_out(&app), "1 px of wobble must not flip the sign");
}

/// Each navigation tool owns its own cursor. They used to share the pan
/// hand, which read as "drag the sheet" during a zoom and an orbit.
#[test]
fn each_navigation_tool_reports_its_own_mode() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    for (command, want) in [
        ("PAN", NavCursor::Pan),
        ("ORBIT", NavCursor::Orbit),
        ("ZOOM DYNAMIC", NavCursor::Zoom),
    ] {
        let _ = app.dispatch_command(command);
        assert_eq!(
            crate::app::view::nav_cursor(&app.tabs[0]),
            want,
            "{command} must have its own cursor"
        );
        let _ = app.update(Message::CommandEscape);
        assert_eq!(
            crate::app::view::nav_cursor(&app.tabs[0]),
            NavCursor::None,
            "Esc must disarm {command}"
        );
    }
}
