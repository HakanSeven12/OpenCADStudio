use super::*;

use super::*;
use crate::app::settings::{NavAction, NavButtons, NavigationStyle};

fn app(style: NavigationStyle) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    app.navigation_style = style;
    let _ = app.update(Message::ViewportMove(Point::new(400.0, 300.0)));
    app
}

fn rotation(app: &OpenCADStudio) -> glam::Quat {
    app.tabs[app.active_tab].scene.camera.borrow().rotation
}

#[test]
fn the_cad_default_still_pans_and_orbits_with_shift() {
    let middle = NavButtons {
        middle: true,
        ..Default::default()
    };
    let cad = NavigationStyle::Cad;
    assert_eq!(
        cad.action(middle, false, false, false, true),
        Some(NavAction::Pan)
    );
    assert_eq!(
        cad.action(middle, true, false, false, true),
        Some(NavAction::Orbit)
    );
    let left = NavButtons {
        left: true,
        ..Default::default()
    };
    assert_eq!(cad.action(left, false, false, false, false), None);
}

#[test]
fn a_gesture_left_drag_orbits_and_a_left_click_stays_a_click() {
    let mut app = app(NavigationStyle::Gesture);
    let before = rotation(&app);
    let _ = app.update(Message::ViewportLeftPress);
    let _ = app.update(Message::ViewportMove(Point::new(440.0, 300.0)));
    let _ = app.update(Message::ViewportLeftRelease);
    assert_ne!(rotation(&app), before);
    {
        let sel = app.tabs[app.active_tab].scene.selection.borrow();
        assert!(!sel.input.middle_down && !sel.input.left_down);
        assert!(sel.gesture.box_anchor.is_none());
    }
    let turned = rotation(&app);
    let _ = app.update(Message::ViewportLeftPress);
    assert!(app.nav_pending.is_some());
    let _ = app.update(Message::ViewportLeftRelease);
    assert!(app.nav_pending.is_none() && app.nav_held == NavButtons::default());
    assert_eq!(rotation(&app), turned);
}

#[test]
fn a_catia_right_click_during_a_middle_drag_zooms() {
    let mut app = app(NavigationStyle::Catia);
    let i = app.active_tab;
    let _ = app.update(Message::ViewportMiddlePress);
    let _ = app.update(Message::ViewportRightPress);
    let _ = app.update(Message::ViewportRightRelease);
    assert_eq!(app.nav_drag_action(i), NavAction::Zoom);
    let distance = app.tabs[i].scene.camera.borrow().distance;
    let _ = app.update(Message::ViewportMove(Point::new(400.0, 260.0)));
    assert_ne!(app.tabs[i].scene.camera.borrow().distance, distance);
    let _ = app.update(Message::ViewportMiddleRelease);
    assert!(!app.nav_zoom_latched);
    assert!(!app.tabs[i].scene.selection.borrow().input.middle_down);
}
