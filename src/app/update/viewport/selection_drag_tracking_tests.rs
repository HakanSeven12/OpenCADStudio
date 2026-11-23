use super::*;

use super::*;

/// A click-move-click box waiting for its second corner survives the
/// pointer leaving the canvas (#3365).
#[test]
fn exit_keeps_a_pending_click_move_click_box() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    {
        let mut sel = app.tabs[i].scene.selection.borrow_mut();
        sel.input.left_down = false;
        sel.gesture.box_anchor = Some(Point::new(100.0, 100.0));
        sel.gesture.box_current = Some(Point::new(150.0, 150.0));
    }
    let _ = app.on_viewport_exit();
    let sel = app.tabs[i].scene.selection.borrow();
    assert_eq!(sel.gesture.box_anchor, Some(Point::new(100.0, 100.0)));
    assert!(sel.input.pointer_away);
}
