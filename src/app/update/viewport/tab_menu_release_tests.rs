//! A drawing tab's right-click menu closes on the next left press but still
//! captures that press's release, so the viewport never hears it. The
//! held press must not become a lasso that follows the cursor for good.

use super::*;
use std::time::Duration;

/// What the window event listener sends for `event` with `status`.
fn listener(event: iced::mouse::Event, status: iced::event::Status) -> Option<Message> {
    crate::app::view::window_event_with_context(
        iced::Event::Mouse(event),
        status,
        iced::window::Id::unique(),
        true,
    )
}

#[test]
fn a_click_that_closes_the_tab_menu_does_not_leave_a_lasso_running() {
    use iced::event::Status;
    use iced::mouse::{Button, Event as Mouse};
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.pick_auto |= 4; // lasso on
    app.pick_drag_mode = 2; // press-drag selection on
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-2., -2., 0.),
        glam::DVec3::new(34., 34., 0.),
        1600. / 900.,
    );

    // 1. Right-click on the drawing tab: the menu widget opens and captures
    //    the press; the app gets no message for it.
    assert!(listener(Mouse::ButtonPressed(Button::Right), Status::Captured).is_none());

    // 2. One plain left click in model space. The press closes the menu and
    //    reaches the viewport; the release is captured by the closing menu.
    let at = Point::new(800.0, 450.0);
    let _ = app.on_viewport_move(at);
    if let Some(message) = listener(Mouse::ButtonPressed(Button::Left), Status::Ignored) {
        let _ = app.update(message);
    }
    let _ = app.update(Message::ViewportLeftPressAt(at));
    if let Some(message) = listener(Mouse::ButtonReleased(Button::Left), Status::Captured) {
        let _ = app.update(message);
    }

    // 3. The cursor moves on, well after the press.
    {
        let mut sel = app.tabs[i].scene.selection.borrow_mut();
        sel.input.left_press_time = sel
            .input
            .left_press_time
            .map(|time| time - Duration::from_millis(1000));
    }
    for step in 1..=6 {
        let d = 30.0 * step as f32;
        let _ = app.update(Message::ViewportMove(Point::new(at.x + d, at.y + d / 2.0)));
    }

    let sel = app.tabs[i].scene.selection.borrow();
    assert!(
        !sel.gesture.poly_active
            && sel.gesture.box_anchor.is_none()
            && !sel.input.left_down
            && !sel.input.left_dragging,
        "gesture still running: poly_active={} box_anchor={:?} left_down={} dragging={}",
        sel.gesture.poly_active,
        sel.gesture.box_anchor,
        sel.input.left_down,
        sel.input.left_dragging,
    );
}
