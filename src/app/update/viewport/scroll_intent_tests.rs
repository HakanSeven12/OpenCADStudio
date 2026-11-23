use super::*;

use super::{pinch_zoom_steps, scroll_intent, ScrollIntent};
use crate::scene::view::camera::Camera;
use iced::mouse::ScrollDelta;

/// With the touchpad option, a scroll pans (wheel lines as pixels) and
/// Ctrl+scroll — a pinch — still zooms. On macOS lines come from a
/// notched wheel only, which zooms.
#[cfg(not(target_os = "macos"))]
#[test]
fn touchpad_pan_scrolls_pan_and_ctrl_zooms() {
    assert_eq!(
        scroll_intent(ScrollDelta::Lines { x: 0.5, y: -0.25 }, true, false, false),
        ScrollIntent::Pan {
            dx: 20.0,
            dy: -10.0
        }
    );
    assert_eq!(
        scroll_intent(ScrollDelta::Lines { x: 0.0, y: 1.0 }, true, true, false),
        ScrollIntent::Zoom { notches: 1.0 }
    );
}

#[test]
fn wheel_notches_zoom() {
    assert_eq!(
        scroll_intent(ScrollDelta::Lines { x: 0.0, y: 1.0 }, false, false, false),
        ScrollIntent::Zoom { notches: 1.0 }
    );
}

#[test]
fn smooth_mouse_scroll_zooms_without_touchpad_mode() {
    for (y, notches) in [(100.0, 1.0), (-100.0, -1.0), (0.0, 0.0)] {
        assert_eq!(
            scroll_intent(ScrollDelta::Pixels { x: 4.0, y }, false, false, false),
            ScrollIntent::Zoom { notches }
        );
    }
}

/// Preserve both axes when touchpad panning is explicitly enabled.
#[test]
fn two_finger_scroll_pans_in_touchpad_mode() {
    assert_eq!(
        scroll_intent(ScrollDelta::Pixels { x: 4.0, y: 6.0 }, true, false, false),
        ScrollIntent::Pan { dx: 4.0, dy: 6.0 }
    );
}

#[test]
fn ctrl_pixel_scroll_zooms_in_touchpad_mode() {
    assert_eq!(
        scroll_intent(ScrollDelta::Pixels { x: 4.0, y: 100.0 }, true, true, false),
        ScrollIntent::Zoom { notches: 1.0 }
    );
}

#[test]
fn smooth_mouse_scroll_changes_camera_zoom_and_honors_reverse() {
    use crate::app::{Message, OpenCADStudio};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.touchpad_pan = false;
    app.ctrl_down = false;
    app.zoom_wheel_reversed = false;
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    let before = app.tabs[i].scene.camera.borrow().distance;
    let scroll = ScrollDelta::Pixels { x: 0.0, y: 100.0 };

    let _ = app.update(Message::ViewportScroll(scroll));
    let zoomed = app.tabs[i].scene.camera.borrow().distance;
    assert!(zoomed < before, "scroll must zoom instead of panning");

    app.zoom_wheel_reversed = true;
    let _ = app.update(Message::ViewportScroll(scroll));
    let reversed = app.tabs[i].scene.camera.borrow().distance;
    assert!(reversed > zoomed, "ZOOMWHEEL must reverse smooth scrolling");
    assert!((reversed - before).abs() < before * 1e-5);
}

/// A pinch is a scale factor, so the view has to end up exactly that much
/// closer: fingers 5% apart divide the camera distance by 1.05.
///
/// Asked of the camera, not of the formula the step was derived from. The
/// version that recomputed `1 - step / 10` here kept passing when the
/// camera moved to an exponential zoom and the pinch quietly stopped
/// being 1:1.
#[test]
fn pinch_is_one_to_one() {
    for magnification in [0.01_f32, 0.05, 0.2, 0.5, 1.0] {
        let mut camera = Camera::default();
        let before = camera.distance;
        camera.zoom(pinch_zoom_steps(magnification));
        let want = before / (1.0 + magnification);
        assert!(
            (camera.distance - want).abs() < 1e-4 * want,
            "m = {magnification}: {} wanted {want}",
            camera.distance
        );
    }
    assert!(pinch_zoom_steps(-0.05) < 0.0, "pinching in zooms out");
    // No pinch, no step: a zero delta must not divide the view away.
    assert_eq!(pinch_zoom_steps(0.0), 0.0);
    let mut camera = Camera::default();
    let before = camera.distance;
    camera.zoom(pinch_zoom_steps(0.0));
    assert_eq!(camera.distance, before);
}

/// A gesture the platform should never send must not be able to throw the
/// camera out of the world.
#[test]
fn a_degenerate_pinch_stays_finite() {
    for magnification in [-1.0_f32, -2.0, f32::NAN, f32::INFINITY] {
        let mut camera = Camera::default();
        camera.zoom(pinch_zoom_steps(magnification));
        assert!(
            camera.distance.is_finite() && camera.distance > 0.0,
            "m = {magnification} left distance at {}",
            camera.distance
        );
    }
}
