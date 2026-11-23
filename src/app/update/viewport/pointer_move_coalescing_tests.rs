use super::*;

use crate::app::{Message, OpenCADStudio};
use iced::time::Instant;
use iced::Point;
use std::time::Duration;

/// A drawing-sized app. `gate` arms the coalescing gate the way a pass of
/// that cost would have left it; `None` leaves it as a fresh app has it.
fn app(gate: Option<Duration>) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    app.move_pass_due = gate.map(|cost| Instant::now() + cost);
    app
}

#[test]
fn a_pass_that_keeps_up_leaves_the_gate_shut() {
    let mut app = app(None);
    // An empty drawing answers a move far inside `MOVE_COALESCE_FROM_MS`,
    // so nothing about this path may change for it.
    let _ = app.on_viewport_move(Point::new(100.0, 100.0));
    assert!(
        app.move_pass_due.is_none(),
        "a pass that fits inside a frame must not arm the gate"
    );
    let _ = app.on_pointer_move_coalesced(Point::new(101.0, 101.0));
    assert!(
        app.move_owed.is_none(),
        "with the gate shut every move runs its own pass"
    );
}

#[test]
fn a_withheld_move_keeps_only_the_latest_position() {
    let mut app = app(Some(Duration::from_millis(50)));
    let tab = app.active_tab;
    let _ = app.on_pointer_move_coalesced(Point::new(10.0, 20.0));
    assert_eq!(
        app.move_owed,
        Some((Point::new(10.0, 20.0), tab)),
        "a withheld move must keep the position it was asked about"
    );
    let _ = app.on_pointer_move_coalesced(Point::new(30.0, 40.0));
    assert_eq!(
        app.move_owed,
        Some((Point::new(30.0, 40.0), tab)),
        "the older position is where the cursor no longer is: it is dropped, \
             not queued behind the newer one"
    );
}

#[test]
fn the_tick_waits_for_the_gap_and_then_answers() {
    let mut app = app(Some(Duration::from_millis(50)));
    let _ = app.on_pointer_move_coalesced(Point::new(10.0, 20.0));
    let _ = app.on_move_coalesce_tick();
    assert!(
        app.move_owed.is_some(),
        "a tick inside the gap must leave the position owed — answering it \
             there is the queue the gate exists to prevent"
    );
    app.move_pass_due = Some(Instant::now() - Duration::from_millis(1));
    let _ = app.on_move_coalesce_tick();
    assert!(
        app.move_owed.is_none(),
        "past the gap the withheld position must be answered, or a pointer \
             that stopped never gets its last move"
    );
}

#[test]
fn a_message_that_reads_the_cursor_answers_the_withheld_move_first() {
    let mut app = app(Some(Duration::from_millis(50)));
    let i = app.active_tab;
    let _ = app.on_pointer_move_coalesced(Point::new(10.0, 20.0));
    assert!(app.move_owed.is_some(), "the move must be withheld first");
    // A bare press carries no position and resolves from `last_move_pos`,
    // which only an answered move writes.
    let _ = app.flush_owed_pointer_move(&Message::ViewportLeftPress);
    assert!(
        app.move_owed.is_none(),
        "a message that reads the cursor must not find the move still owed"
    );
    assert_eq!(
        app.tabs[i].scene.selection.borrow().input.last_move_pos,
        Some(Point::new(10.0, 20.0)),
        "the flush must answer the withheld position, not skip it"
    );
}

#[test]
fn a_newer_move_does_not_flush_the_one_it_supersedes() {
    let mut app = app(Some(Duration::from_millis(50)));
    let _ = app.on_pointer_move_coalesced(Point::new(10.0, 20.0));
    let _ = app.flush_owed_pointer_move(&Message::ViewportMove(Point::new(30.0, 40.0)));
    assert!(
        app.move_owed.is_some(),
        "flushing for the next move would answer a position it replaces"
    );
}

#[test]
fn a_press_drops_the_position_the_gate_withheld() {
    let mut app = app(Some(Duration::from_millis(50)));
    let _ = app.on_pointer_move_coalesced(Point::new(10.0, 20.0));
    assert!(app.move_owed.is_some(), "the move must be withheld first");
    app.set_cursor_for_press(Point::new(300.0, 300.0));
    assert!(
        app.move_owed.is_none(),
        "answering a pre-press position after the press would send the \
             cursor backwards"
    );
}
