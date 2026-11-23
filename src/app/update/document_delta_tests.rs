//! Document-change delta characterisation (API v27). Tests only; no
//! production code changed here.
//!
//! `HostNotification::DocumentChangedDelta` is the exact, categorized
//! successor of `DocumentChangedRegions` (v13): both boundaries — the
//! built-in message boundary (`notify_plugins_document_changed`) and the
//! plugin-write boundary (`HostSession`'s immediate publish) — classify the
//! scene journal between the last published epoch and now through
//! `DocumentTab::take_document_boundary` into added / modified / deleted.
//!
//! Coverage (the commission's acceptance set):
//!   * a line drawn through the real built-in command path dispatches the
//!     new handle in `added` (+1)
//!   * a modification reports `modified` (and nothing else)
//!   * a deletion reports `deleted`
//!   * a multi-entity operation reports the complete exact set (+3)
//!   * undo/redo flip a created entity between `deleted` and `added` (and
//!     undoing a deletion / a modification resolves symmetrically)
//!   * plugin `add_entity` / `update_entity` / `remove_entity` produce the
//!     equivalent deltas
//!   * interleaved edits on two tabs stay scoped: every delta is published
//!     in the boundary of its own tab (tab ids ride the `DrawingChanged`
//!     marker of the same synchronous boundary) and a non-editing tab's
//!     boundary is exactly empty
//!   * a journal gap reports `full_refresh_required` with empty lists
//!   * the delta's snapshot correlation matches the `DocumentChangedV4`
//!     version logged for the same boundary
//!   * plugin record writes keep riding the delta as `(entity, app)` pairs
//!     (the write's own whole-walk `bump_geometry` marks the journal full,
//!     so that boundary also flags the honesty degrade)
//!
//! Observation seam (the plugin_grips_tests pattern): the app dispatches
//! through `plugin::v4_support` under cfg(test), where every broadcast is
//! recorded on the calling thread as the EXACT format string of
//! `record_test_dispatch`'s match — for the delta arm
//! `"DocumentChangedDelta(+A, ~M, -D, R records)"`, plus
//! `", FULL REFRESH REQUIRED"` when the honesty flag is set. One recorded
//! entry stands for one fan-out to every loaded native plugin.
//!
//! Exact handle sets: the message/command paths publish their boundary
//! synchronously (the publisher runs at the end of every `update`), so a
//! test cannot intercept the published `DocumentBoundary` from outside —
//! the log intentionally carries only counts. For exact handle-level
//! assertions the same REAL boundary function
//! (`DocumentTab::take_document_boundary`, the call both publishers make)
//! runs on the real app tab after driving real scene mutations (the shared
//! primitives every command routes through), and where a real undo step is
//! needed the real quiet undo machinery (`undo_steps_reported`) applies it.

use document::entities::{Line, Point};
use document::types::{Handle, Vector3};
use document::EntityType;
use ocs_plugin_api::host::DocumentDeltaSnapshot;

use crate::app::{Message, OpenCADStudio};

fn app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app
}

fn tab(app: &OpenCADStudio) -> usize {
    app.active_tab
}

fn add(app: &mut OpenCADStudio, entity: EntityType) -> Handle {
    let i = tab(app);
    let mut host = crate::app::plugin_host::HostSession::new(app, i);
    host.add_entity(entity)
}

fn line(start: [f64; 3], end: [f64; 3]) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(start[0], start[1], start[2]),
        Vector3::new(end[0], end[1], end[2]),
    ))
}

fn point(at: [f64; 3]) -> EntityType {
    EntityType::Point(Point::from_coords(at[0], at[1], at[2]))
}

fn dispatch_log() -> Vec<String> {
    crate::plugin::v4_support::take_test_dispatch_log()
}

/// Pair every `DocumentChangedDelta` entry in a raw dispatch log with the
/// tab id of the boundary that published it (see the module header).
fn parse_boundary_deltas(log: &[String]) -> Vec<(u64, String)> {
    let mut pairs = Vec::new();
    let mut current_tab: Option<u64> = None;
    for entry in log {
        if let Some(rest) = entry.strip_prefix("DrawingChanged(") {
            let tab_id = rest
                .split(',')
                .next()
                .and_then(|id| id.trim().parse::<u64>().ok());
            current_tab = tab_id;
        }
        if entry.starts_with("DocumentChangedDelta") {
            let Some(tab_id) = current_tab.take() else {
                panic!("a DocumentChangedDelta published outside a tab boundary: {log:?}");
            };
            pairs.push((tab_id, entry.clone()));
        }
    }
    pairs
}

/// Drain the dispatch log into per-boundary delta pairs.
fn boundary_deltas_drained() -> Vec<(u64, String)> {
    parse_boundary_deltas(&dispatch_log())
}

// ── 1/2/3: one entity, one real path each ───────────────────────────────────

#[test]
fn document_delta_built_in_line_create_dispatches_added() {
    let mut app = app();
    let _ = dispatch_log();

    // The real built-in command path: the automation bridge runs the same
    // command line the GUI types into, and its mutation settles at a real
    // message boundary (the boundary publisher runs at the end of update()).
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let i = tab(&app);
    let epoch = app.tabs[i].scene.geometry_epoch;

    let log = dispatch_log();
    let deltas = parse_boundary_deltas(&log);
    assert_eq!(
        deltas.len(),
        1,
        "one settled edit dispatches exactly one delta: {log:?}"
    );
    let (tab_id, delta) = &deltas[0];
    assert_eq!(*tab_id, app.tabs[i].id, "the boundary names its tab");
    assert_eq!(
        delta, "DocumentChangedDelta(+1, ~0, -0, 0 records)",
        "the line's creation rides the boundary as +1 at no extra kinds: {deltas:?}"
    );
    assert!(
        log.iter()
            .any(|entry| entry == &format!("DrawingChanged({}, {})", app.tabs[i].id, epoch)),
        "the same boundary settled the epoch: DrawingChanged({}, {epoch})",
        app.tabs[i].id
    );
}

#[test]
fn document_delta_built_in_modify_dispatches_modified_only() {
    let mut app = app();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line"}"#)["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    let _ = dispatch_log();

    // A real built-in modification: MOVE runs the same transform the
    // interactive command does.
    app.automation_op(r#"{"op":"run","cmd":"MOVE 0,0 100,0"}"#);

    let deltas = boundary_deltas_drained();
    assert_eq!(deltas.len(), 1, "the MOVE boundary: {deltas:?}");
    assert_eq!(
        deltas[0].1, "DocumentChangedDelta(+0, ~1, -0, 0 records)",
        "modified carries exactly the moved handle's kind: {deltas:?}"
    );
}

#[test]
fn document_delta_built_in_erase_dispatches_deleted() {
    let mut app = app();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line"}"#)["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    let _ = dispatch_log();

    // The real built-in ERASE over the selection.
    app.automation_op(r#"{"op":"run","cmd":"ERASE"}"#);

    let deltas = boundary_deltas_drained();
    assert_eq!(deltas.len(), 1, "the ERASE boundary: {deltas:?}");
    assert_eq!(
        deltas[0].1, "DocumentChangedDelta(+0, ~0, -1, 0 records)",
        "deleted carries the erased handle: {deltas:?}"
    );
}

// ── 4: multi-entity op, complete exact sets ─────────────────────────────────

#[test]
fn document_delta_multi_entity_add_reports_the_complete_exact_set() {
    let mut app = app();
    let i = tab(&app);

    // The scene-level batch add is the shared primitive the plugin batch API
    // routes through: one op, three entities, one geometry bump — the delta
    // must name all three and nothing else.
    let mut expected = Vec::new();
    let handles = app.tabs[i].scene.add_entities(vec![
        point([1.0, 1.0, 0.0]),
        line([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
        point([9.0, 9.0, 0.0]),
    ]);
    for &handle in &handles {
        assert!(!handle.is_null());
        expected.push(handle);
    }
    expected.sort_unstable();

    let boundary = app.tabs[i].take_document_boundary(None, false);
    assert_eq!(boundary.added, expected, "exactly the three new handles");
    assert!(boundary.modified.is_empty());
    assert!(boundary.deleted.is_empty());
    assert!(boundary.records.is_empty());
    assert!(
        !boundary.full_refresh_required,
        "a well-formed batch is provable, not degraded"
    );
}

#[test]
fn document_delta_multi_entity_add_dispatches_plus_three() {
    let mut app = app();
    let i = tab(&app);
    let _ = dispatch_log();

    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    let handles = host.add_entities(vec![
        point([1.0, 1.0, 0.0]),
        point([2.0, 2.0, 0.0]),
        point([3.0, 3.0, 0.0]),
    ]);
    drop(host);
    assert_eq!(handles.len(), 3);
    assert!(handles.iter().all(|handle| !handle.is_null()));

    let deltas = boundary_deltas_drained();
    assert_eq!(deltas.len(), 1, "the batch publishes ONE delta: {deltas:?}");
    assert_eq!(
        deltas[0].1, "DocumentChangedDelta(+3, ~0, -0, 0 records)",
        "one op, three additions: {deltas:?}"
    );
}

// ── 5: undo / redo ──────────────────────────────────────────────────────────

#[test]
fn document_delta_undo_redo_flips_a_created_entity_between_deleted_and_added() {
    let mut app = app();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let _ = dispatch_log();

    // The real undo message path: undo of the creation = the handle leaves.
    let _ = app.update(Message::Undo);
    let deltas = boundary_deltas_drained();
    assert_eq!(
        deltas
            .iter()
            .map(|(_, delta)| delta.as_str())
            .collect::<Vec<_>>(),
        vec!["DocumentChangedDelta(+0, ~0, -1, 0 records)"],
        "undo of the creation reports the handle as deleted: {deltas:?}"
    );

    // Redo re-creates it: the same handle rides back as added.
    let _ = app.update(Message::Redo);
    let deltas = boundary_deltas_drained();
    assert_eq!(
        deltas
            .iter()
            .map(|(_, delta)| delta.as_str())
            .collect::<Vec<_>>(),
        vec!["DocumentChangedDelta(+1, ~0, -0, 0 records)"],
        "redo of the creation reports the handle as added again: {deltas:?}"
    );
}

#[test]
fn document_delta_undo_of_deletion_reports_added() {
    let mut app = app();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line"}"#)["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    app.automation_op(r#"{"op":"run","cmd":"ERASE"}"#);
    let i = tab(&app);
    let handle = Handle::from(u64::from_str_radix(&handle, 16).expect("hex handle"));
    let _ = dispatch_log();

    // The real quiet undo machinery the U command drives (no publisher of
    // its own — the boundary below is taken directly, so the exact handle
    // set is observable).
    app.undo_steps_reported(1, false);
    let boundary = app.tabs[i].take_document_boundary(None, false);
    assert_eq!(
        boundary.added,
        vec![handle],
        "undo of the deletion re-adds the handle"
    );
    assert!(boundary.modified.is_empty());
    assert!(boundary.deleted.is_empty());
    assert!(!boundary.full_refresh_required);
}

#[test]
fn document_delta_undo_of_modification_reports_modified() {
    let mut app = app();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line"}"#)["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    app.automation_op(r#"{"op":"run","cmd":"MOVE 0,0 100,0"}"#);
    let i = tab(&app);
    let handle = Handle::from(u64::from_str_radix(&handle, 16).expect("hex handle"));
    let _ = dispatch_log();

    app.undo_steps_reported(1, false);
    let boundary = app.tabs[i].take_document_boundary(None, false);
    assert_eq!(
        boundary.modified,
        vec![handle],
        "undo of the modification restores the entity in place"
    );
    assert!(boundary.added.is_empty());
    assert!(boundary.deleted.is_empty());
    assert!(!boundary.full_refresh_required);
}

// ── 6: the plugin-write path ────────────────────────────────────────────────

#[test]
fn document_delta_plugin_writes_report_equivalent_deltas() {
    let mut app = app();
    let i = tab(&app);
    let _ = dispatch_log();

    // add_entity → +1.
    let handle = add(&mut app, line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
    let (tab_id, delta) = boundary_deltas_drained().pop().expect("add boundary");
    assert_eq!(tab_id, app.tabs[i].id);
    assert_eq!(
        delta, "DocumentChangedDelta(+1, ~0, -0, 0 records)",
        "the plugin write boundary classifies like the built-in one"
    );

    // update_entity → ~1 (the same handle, modified).
    let Some(EntityType::Line(mut moved)) = app.tabs[i].scene.document.get_entity(handle).cloned()
    else {
        panic!("the plugin line exists");
    };
    moved.end = Vector3::new(20.0, 0.0, 0.0);
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    assert!(host.update_entity(EntityType::Line(moved)));
    drop(host);
    let (tab_id, delta) = boundary_deltas_drained().pop().expect("update boundary");
    assert_eq!(tab_id, app.tabs[i].id);
    assert_eq!(delta, "DocumentChangedDelta(+0, ~1, -0, 0 records)");

    // remove_entity → -1.
    let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
    assert!(host.remove_entity(handle));
    drop(host);
    let (tab_id, delta) = boundary_deltas_drained().pop().expect("remove boundary");
    assert_eq!(tab_id, app.tabs[i].id);
    assert_eq!(delta, "DocumentChangedDelta(+0, ~0, -1, 0 records)");
}

#[test]
fn document_delta_plugin_entity_kinds_match_the_journal_replay_exactly() {
    // Exact handle-level counterpart for the plugin path: the plugin write
    // boundary classifies the SAME journal replay the scene journaled; the
    // anchor snapshot taken before the writes, replayed after, must equal
    // the kinds the delta set carried.
    let mut app = app();
    let i = tab(&app);
    let added_anchor = app.tabs[i].plugin_dirty_epoch;
    let handle = add(&mut app, line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
    let replay = app.tabs[i]
        .scene
        .replay_since(added_anchor)
        .expect("the journal covers a settled write");
    let mut replay_kinds: Vec<(document::Handle, crate::scene::ChangeKind)> = replay;
    replay_kinds.sort_by_key(|(handle, _)| *handle);
    assert_eq!(
        replay_kinds,
        vec![(handle, crate::scene::ChangeKind::Added)],
        "the plugin add journaled exactly the new handle"
    );
}

// ── 7: two tabs ─────────────────────────────────────────────────────────────

#[test]
fn document_delta_interleaved_tab_edits_stay_scoped_to_their_tab() {
    let mut app = app();
    let i = tab(&app);
    let _ = app.update(Message::TabNew);
    let other = tab(&app);
    assert_ne!(
        app.tabs[i].id, app.tabs[other].id,
        "the test addresses two distinct document tabs"
    );

    // Startup settles first: the `new` op and the fresh tab each replaced a
    // whole document, so their boundaries force the full-refresh flag.
    let startup = dispatch_log();
    assert!(
        parse_boundary_deltas(&startup)
            .iter()
            .all(|(_, delta)| delta.ends_with("FULL REFRESH REQUIRED)")),
        "whole-drawing replacements degrade honestly: {startup:?}"
    );

    // TabNew made the fresh tab active. Switch back to tab A: the switch
    // settles A's exact-empty boundary; then A's edit publishes its own +1;
    // the same round-trip for tab B.
    let _ = app.update(Message::TabSwitch(i));
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let _ = app.update(Message::TabSwitch(other));
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 2,0"}"#);

    let log = dispatch_log();
    let deltas = parse_boundary_deltas(&log);
    let tab_a = app.tabs[i].id;
    let tab_b = app.tabs[other].id;

    // The exact interleaved stream: each tab's switch boundary carries
    // nothing (an exact claim, not "unknown"), and each edit delta carries
    // ONLY that tab's one line under that tab's id. A cross-tab leak would
    // show as a +2 delta, a non-empty switch boundary, or a foreign tab id.
    assert_eq!(
        deltas,
        vec![
            (
                tab_a,
                "DocumentChangedDelta(+0, ~0, -0, 0 records)".to_string()
            ),
            (
                tab_a,
                "DocumentChangedDelta(+1, ~0, -0, 0 records)".to_string()
            ),
            (
                tab_b,
                "DocumentChangedDelta(+0, ~0, -0, 0 records)".to_string()
            ),
            (
                tab_b,
                "DocumentChangedDelta(+1, ~0, -0, 0 records)".to_string()
            ),
        ],
        "each boundary carries only its own tab's changes: {log:?}"
    );

    // Exact-set counterpart: an edit journaled on the INACTIVE tab B settles
    // in B's boundary with exactly B's handle, while A's boundary stays
    // empty — the same scoping the log entries above show, handle-exact.
    let b_handle = app.tabs[other]
        .scene
        .add_entity(line([7.0, 0.0, 0.0], [9.0, 0.0, 0.0]));
    let b_boundary = app.tabs[other].take_document_boundary(None, false);
    assert_eq!(b_boundary.added, vec![b_handle], "B's boundary, B's handle");
    assert!(b_boundary.modified.is_empty());
    assert!(b_boundary.deleted.is_empty());
    assert!(!b_boundary.full_refresh_required);
    let a_boundary = app.tabs[i].take_document_boundary(None, false);
    assert!(
        a_boundary.added.is_empty() && a_boundary.modified.is_empty(),
        "the inactive A boundary never saw B's edit"
    );
    assert!(!a_boundary.full_refresh_required);
}

// ── 8: journal gap / overflow — the honesty flag ────────────────────────────

#[test]
fn document_delta_whole_document_replacement_reports_full_refresh() {
    let mut app = app();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let i = tab(&app);
    let _ = dispatch_log();

    // The documented lifecycle case: a whole-document replacement (the
    // automation `new` op = File → New on this tab) journals a `full` marker
    // no delta can express, so the boundary must force the honesty flag
    // with empty lists.
    app.automation_op(r#"{"op":"new"}"#);

    let deltas = boundary_deltas_drained();
    assert_eq!(
        deltas
            .iter()
            .map(|(_, delta)| delta.as_str())
            .collect::<Vec<_>>(),
        vec!["DocumentChangedDelta(+0, ~0, -0, 0 records, FULL REFRESH REQUIRED)"],
        "the replacement boundary forces exactly one full-refresh delta: {deltas:?}"
    );
    assert!(
        !app.tabs[i].is_start,
        "the same tab was replaced, not closed"
    );
}

#[test]
fn document_delta_journal_overflow_reports_full_refresh_with_empty_lists() {
    let mut app = app();
    let i = tab(&app);
    let anchor = app.tabs[i].plugin_dirty_epoch;
    let _ = dispatch_log();

    // Drive the journal past its 256-entry cap within ONE boundary window
    // through the real mutation primitive (`Scene::add_entity`, the path
    // every command routes through), then settle the real boundary: the
    // anchor has been evicted, so only a full-refresh delta is honest.
    for n in 0..257 {
        let handle = app.tabs[i].scene.add_entity(point([n as f64, 0.0, 0.0]));
        assert!(!handle.is_null());
    }
    assert_eq!(
        app.tabs[i].scene.replay_since(anchor),
        None,
        "257 bumps evicted the anchor: the journal has a gap"
    );

    // The REAL boundary the publishers run, settled through the real
    // publisher call.
    app.notify_plugins_document_changed();
    let deltas = boundary_deltas_drained();
    assert!(
        deltas.iter().any(|(_, delta)| delta
            == "DocumentChangedDelta(+0, ~0, -0, 0 records, FULL REFRESH REQUIRED)"),
        "an evicted journal forces the full-refresh boundary: {deltas:?}"
    );
    // And it is the ONLY delta that boundary produced — never a partial set
    // presented as complete (257 additions must never leak as +257).
    assert!(
        !deltas
            .iter()
            .any(|(_, delta)| delta.starts_with("DocumentChangedDelta(+257")
                || delta.starts_with("DocumentChangedDelta(+1")),
        "the degraded boundary never claims partial lists: {deltas:?}"
    );
}

// ── 9: snapshot correlation ─────────────────────────────────────────────────

#[test]
fn document_delta_snapshot_correlation_matches_the_document_changed_v4_version() {
    let mut app = app();
    let i = tab(&app);
    let tab_id = app.tabs[i].id;

    // No V4 store open anywhere: the boundary's snapshot field is None and
    // the log never claims a DocumentChangedV4 for the boundary.
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let log = dispatch_log();
    assert!(
        !log.iter()
            .any(|entry| entry.starts_with("DocumentChangedV4")),
        "without a V4 consumer nothing claimed a snapshot: {log:?}"
    );

    // The real consumer path: a plugin opens the tab's V4 store.
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        assert!(
            host.document_view_v4(tab_id).is_some(),
            "the V4 store opens"
        );
    }
    let _ = dispatch_log();

    // The next real boundary now correlates: DocumentChangedV4 and the delta
    // ride the SAME synchronous boundary (the delta sits between the V4
    // notification and the next boundary's marker).
    app.automation_op(r#"{"op":"run","cmd":"LINE 10,0 20,0"}"#);
    let log = dispatch_log();
    let v4_version = log
        .iter()
        .filter_map(|entry| entry.strip_prefix(&format!("DocumentChangedV4({tab_id}, ")))
        .filter_map(|rest| rest.strip_suffix(')'))
        .filter_map(|version| version.parse::<u64>().ok())
        .next()
        .expect("the boundary published its V4 snapshot");
    assert!(
        log.iter()
            .any(|entry| entry == "DocumentChangedDelta(+1, ~0, -0, 0 records)"),
        "the same boundary carried the delta: {log:?}"
    );

    // Exact correlation equality through the real publishing sequence the
    // boundary runs (publish_document_view_v4 → take_document_boundary):
    // the delta cites exactly the (version, generation) whose publication
    // logged the DocumentChangedV4 for this boundary.
    let handle = app.tabs[i]
        .scene
        .add_entity(line([0.0, 5.0, 0.0], [4.0, 5.0, 0.0]));
    assert!(!handle.is_null());
    let doc = &app.tabs[i].scene.document;
    let correlation: DocumentDeltaSnapshot =
        crate::plugin::v4_support::publish_document_view_v4(tab_id, doc)
            .expect("the V4 publish succeeds")
            .expect("an open store publishes");
    assert!(
        correlation.version > v4_version,
        "each publication advances the store version ({} → {})",
        v4_version,
        correlation.version
    );
    assert!(
        dispatch_log().iter().any(
            |entry| entry == &format!("DocumentChangedV4({}, {})", tab_id, correlation.version)
        ),
        "the publication logged its V4 notification: {:?}",
        dispatch_log()
    );

    let boundary = app.tabs[i].take_document_boundary(Some(correlation.clone()), false);
    assert_eq!(
        boundary.snapshot,
        Some(correlation),
        "the delta cites exactly the published (version, generation)"
    );
    assert_eq!(
        boundary.added,
        vec![handle],
        "the classification input was the real journal"
    );
    assert!(!boundary.full_refresh_required);

    crate::plugin::v4_support::close_document_view_v4(tab_id);
}

// ── 10: records ride the delta ──────────────────────────────────────────────

#[test]
fn document_delta_plugin_record_writes_reach_the_delta() {
    let mut app = app();
    let handle = add(&mut app, line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]));
    let i = tab(&app);
    let _ = dispatch_log();

    // The real write path: the SAME (entity, app) pair stays one tuple in
    // the delta, even written repeatedly.
    let record = |value: &str| document::xdata::ExtendedDataRecord {
        application_name: "PLUGIN".to_string(),
        values: vec![document::xdata::XDataValue::String(value.to_string())],
    };
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        assert!(host.write_record(handle, record("first")));
        drop(host);
    }
    let deltas = boundary_deltas_drained();
    assert_eq!(deltas.len(), 1, "the write_record boundary: {deltas:?}");
    // The shipped shape: write_record's closing plain `bump_geometry`
    // journals a whole-walk marker the journal cannot attribute, so the
    // boundary degrades honestly (the flag) while the (entity, app) pair
    // still rides — the delta's records are the drained legacy set, taken
    // regardless of the replay verdict.
    assert_eq!(
        deltas[0].1, "DocumentChangedDelta(+0, ~0, -0, 1 records, FULL REFRESH REQUIRED)",
        "the (entity, app) pair rides the delta: {deltas:?}"
    );
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        assert!(host.write_record(handle, record("second")));
        drop(host);
    }
    let deltas = boundary_deltas_drained();
    assert_eq!(deltas.len(), 1, "the repeated boundary: {deltas:?}");
    assert_eq!(
        deltas[0].1, "DocumentChangedDelta(+0, ~0, -0, 1 records, FULL REFRESH REQUIRED)",
        "the repeated write of the same pair stays exactly one tuple: {deltas:?}"
    );

    // Exact tuple counterpart through the real boundary take: the records
    // the boundary drained are the (entity, app) pairs the write marked.
    let epoch = app.tabs[i].scene.geometry_epoch;
    app.tabs[i].mark_plugin_record_dirty(handle, "PLUGIN".to_string(), epoch);
    let boundary = app.tabs[i].take_document_boundary(None, false);
    assert_eq!(
        boundary.records,
        vec![(handle, "PLUGIN".to_string())],
        "the drained record set carries the written pair"
    );
}
