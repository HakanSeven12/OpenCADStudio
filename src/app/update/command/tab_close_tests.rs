use super::*;

/// Reproductions for the stale-tab-index crashes: iced dispatches every
/// message produced from one view snapshot before rebuilding it, so a second
/// `TabClose` (or a `PendingClose::Tab` stashed while the unsaved dialog is
/// open) can carry an index the first removal already invalidated.
use crate::app::{Message, OpenCADStudio};

/// `[Start, Drawing A, Drawing B]` — three clean tabs.
fn three_tabs() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.push_test_document();
    app.push_test_document();
    assert_eq!(app.tabs.len(), 3, "start page plus two drawings");
    assert!(!app.tabs[1].is_start && !app.tabs[2].is_start);
    app
}

/// Two close messages built from the same view snapshot: the first one
/// removes the tab, the second carries the now-outdated reference. The
/// close request must degrade to a no-op, not panic.
#[test]
fn stale_tab_close_is_a_no_op() {
    let mut app = three_tabs();
    let id = app.tabs[app.tabs.len() - 1].id;

    let _ = app.update(Message::TabClose(id));
    assert_eq!(app.tabs.len(), 2, "first close removed the tab");

    // Same target again, as the batched second message from one snapshot.
    let _ = app.update(Message::TabClose(id));
    assert_eq!(
        app.tabs.len(),
        2,
        "stale close must not remove another tab or panic"
    );
}

/// A closing drawing takes its remembered ATTDEF placement with it: the
/// coordinates belong to the drawing that placed them.
#[test]
fn closing_a_tab_drops_the_place_it_remembered() {
    let mut app = three_tabs();
    let i = app
        .tabs
        .iter()
        .position(|tab| !tab.is_start)
        .expect("a drawing tab, not the fixed start page");
    let id = app.tabs[i].id;
    crate::modules::draw::draw::attdef::session().set_place(id, &Default::default());
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(id)
            .is_some(),
        "the drawing remembered a place"
    );

    let _ = app.update(Message::TabClose(id));

    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(id)
            .is_none(),
        "the place belonged to the drawing that just closed"
    );
}

/// Discarding through the unsaved prompt is a close as much as clicking
/// the × is: the discarded drawing's remembered place goes with it, and a
/// drawing that stayed open keeps its own.
#[test]
fn discarding_a_dirty_tab_drops_the_place_it_remembered() {
    let mut app = three_tabs();
    let kept_id = app.tabs[1].id;
    let closed_id = app.tabs[2].id;
    crate::modules::draw::draw::attdef::session().set_place(kept_id, &Default::default());
    crate::modules::draw::draw::attdef::session().set_place(closed_id, &Default::default());
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(closed_id)
            .is_some(),
        "the drawing remembered a place"
    );
    app.tabs[2].dirty = true;

    let _ = app.update(Message::TabClose(closed_id));
    assert!(
        app.pending_close.is_some(),
        "closing a dirty drawing opens the unsaved prompt"
    );
    let _ = app.update(Message::UnsavedDialogDiscard);

    assert!(
        app.tabs.iter().all(|t| t.id != closed_id),
        "the discarded drawing is gone"
    );
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(closed_id)
            .is_none(),
        "and the place it remembered went with it"
    );
    assert!(
        crate::modules::draw::draw::attdef::session()
            .last_in(kept_id)
            .is_some(),
        "the drawing still open keeps its own place"
    );
}

/// A text line is remembered the same way an attribute placement is: the
/// next TEXT in the drawing continues below it, so closing the drawing has
/// to take the line with it.
#[test]
fn closing_a_tab_drops_the_line_it_remembered() {
    let mut app = three_tabs();
    let i = app
        .tabs
        .iter()
        .position(|tab| !tab.is_start)
        .expect("a drawing tab, not the fixed start page");
    let id = app.tabs[i].id;
    crate::modules::annotate::text::session().set_place(id, &Default::default());
    assert!(
        crate::modules::annotate::text::session()
            .last_in(id)
            .is_some(),
        "the drawing remembered a line"
    );

    let _ = app.update(Message::TabClose(id));

    assert!(
        crate::modules::annotate::text::session()
            .last_in(id)
            .is_none(),
        "the line belonged to the drawing that just closed"
    );
}

/// Discarding a dirty drawing ends it just as closing does, so its line
/// goes too — while the drawings still open keep theirs.
#[test]
fn discarding_a_dirty_tab_drops_the_line_it_remembered() {
    let mut app = three_tabs();
    let kept_id = app.tabs[1].id;
    let closed_id = app.tabs[2].id;
    crate::modules::annotate::text::session().set_place(kept_id, &Default::default());
    crate::modules::annotate::text::session().set_place(closed_id, &Default::default());
    assert!(
        crate::modules::annotate::text::session()
            .last_in(closed_id)
            .is_some(),
        "the drawing remembered a line"
    );
    app.tabs[2].dirty = true;

    let _ = app.update(Message::TabClose(closed_id));
    assert!(
        app.pending_close.is_some(),
        "closing a dirty drawing opens the unsaved prompt"
    );
    let _ = app.update(Message::UnsavedDialogDiscard);

    assert!(
        app.tabs.iter().all(|t| t.id != closed_id),
        "the discarded drawing is gone"
    );
    assert!(
        crate::modules::annotate::text::session()
            .last_in(closed_id)
            .is_none(),
        "the discarded drawing's line went with it"
    );
    assert!(
        crate::modules::annotate::text::session()
            .last_in(kept_id)
            .is_some(),
        "the drawing still open keeps its own line"
    );
}

/// Double-clicking one tab's × in a single batch must close that tab
/// exactly once; the second message must not slide onto the tab that
/// shifted into its place.
#[test]
fn double_close_from_one_snapshot_closes_only_the_clicked_tab() {
    let mut app = three_tabs();
    let first_id = app.tabs[1].id;
    let other_id = app.tabs[2].id;

    let _ = app.update(Message::TabClose(first_id));
    let _ = app.update(Message::TabClose(first_id));

    assert!(
        app.tabs.iter().all(|t| t.id != first_id),
        "the clicked tab must be closed"
    );
    assert!(
        app.tabs.iter().any(|t| t.id == other_id),
        "the tab that shifted into index 1 must survive"
    );
}

/// The unsaved dialog stashes a close target while it is open. Another
/// close landing before the user answers shifts the vector, so Discard
/// must still discard the tab the dialog was opened for.
#[test]
fn unsaved_dialog_discard_closes_the_tab_it_was_opened_for() {
    let mut app = OpenCADStudio::new_for_test();
    app.push_test_document(); // A
    app.push_test_document(); // B
    app.push_test_document(); // C
    assert_eq!(app.tabs.len(), 4);
    let a_id = app.tabs[1].id;
    let b_id = app.tabs[2].id;
    let c_id = app.tabs[3].id;
    app.tabs[2].dirty = true;

    // Close dirty B: opens the dialog and stashes the pending target.
    let _ = app.update(Message::TabClose(b_id));
    assert!(
        app.pending_close.is_some(),
        "dirty close opens the unsaved dialog"
    );

    // A second close lands while the dialog is pending (batched message /
    // automation `close` / save continuation) and shifts indices.
    let _ = app.update(Message::TabClose(a_id));
    assert!(app.tabs.iter().all(|t| t.id != a_id));

    // User picks Discard.
    let _ = app.update(Message::UnsavedDialogDiscard);

    assert!(
        app.tabs.iter().all(|t| t.id != b_id),
        "the dirty tab the dialog was opened for must be discarded"
    );
    assert!(
        app.tabs.iter().any(|t| t.id == c_id),
        "C must survive the stale pending index"
    );
    assert!(app.pending_close.is_none(), "pending target consumed");
    assert!(app.active_tab < app.tabs.len(), "active tab stays valid");
}
