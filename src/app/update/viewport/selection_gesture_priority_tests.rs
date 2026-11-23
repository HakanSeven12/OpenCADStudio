use super::*;

mod selection_gesture_priority_tests {
use super::*;

#[test]
fn selection_drag_filters_pointer_jitter() {
    let start = Point::new(100.0, 100.0);
    let jitter = Point::new(103.0, 102.0);
    let moved = Point::new(106.0, 100.0);
    assert!(!selection_drag_should_start(start, jitter, 0, false, false));
    assert!(selection_drag_should_start(start, moved, 0, false, false));
}

#[test]
fn a_quick_moving_click_stays_a_click_when_it_can_arm_a_window() {
    let start = Point::new(100.0, 100.0);
    let wobble = Point::new(110.0, 107.0);
    let far = Point::new(130.0, 100.0);
    assert!(!selection_drag_should_start(start, wobble, 0, true, false));
    assert!(!selection_drag_should_start(start, far, 0, true, true));
    assert!(selection_drag_should_start(start, far, 0, true, false));
    assert!(selection_drag_should_start(
        start,
        wobble,
        POLY_START_DELAY_MS,
        true,
        true
    ));
}

#[test]
fn quick_selection_click_anchors_where_the_button_went_down() {
    let start = Point::new(100.0, 100.0);
    let moved = Point::new(190.0, 135.0);
    assert_eq!(
        selection_click_position(moved, Some(start), true, true),
        start
    );
    // During a command or a genuine press-drag, use the moving cursor.
    assert_eq!(
        selection_click_position(moved, Some(start), true, false),
        moved
    );
    assert_eq!(
        selection_click_position(moved, Some(start), false, true),
        moved
    );
    assert_eq!(selection_click_position(moved, None, true, true), moved);
}
}
