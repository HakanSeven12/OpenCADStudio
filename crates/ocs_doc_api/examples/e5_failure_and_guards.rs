//! Example 5 — Failure semantics: atomicity, OpGroup compensation, revision guard.
//!
//! Run: `cargo run --example e5_failure_and_guards --features host`
//!
//! Shows: per-op atomicity (a failed op applies nothing and moves no revision),
//! `OpGroup` best-effort cleanup, `ApiError` handling, and the `assert_revision`
//! read-guard.

mod common;

use ocs_doc_api::{ApiError, DocApi, HasId, OpGroup, ObjectId};

fn main() -> ocs_doc_api::ApiResult<()> {
    let api = DocApi::in_process(common::MockBackend::default(), 0);
    let doc = api.document(api.active_tab());

    // ── Per-op atomicity: a boolean against a deleted id fails cleanly ──
    let a = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3])?;
    let ghost = doc.solids().create_sphere([5.0, 5.0, 5.0], 4.0)?;
    ghost.delete()?; // now a stale handle

    let rev_before = doc.revision()?;
    match a.intersect(&ghost) {
        Err(ApiError::UnknownId(id)) => println!("boolean failed as expected: unknown id {id:?}"),
        Err(e) => println!("boolean failed: {e}"),
        Ok(_) => panic!("expected the boolean to fail"),
    }
    // `a` is still live (failed op mutated nothing) and the revision did not move.
    assert!(a.bounds().is_ok(), "input solid survives a failed boolean");
    assert_eq!(doc.revision()?, rev_before, "failed op records no revision bump");

    // ── OpGroup: clean up a partially-built logical operation on failure ──
    let mut grp = OpGroup::new();
    let p1 = grp.track(doc.solids().create_cuboid([0.0, 0.0, 0.0], [4.0; 3]))?;
    let p2 = grp.track(doc.solids().create_sphere([2.0, 2.0, 2.0], 3.0))?;
    // Simulate a downstream failure: abandon the group and delete what we made.
    grp.compensate(&doc)?;
    for h in [p1.id(), p2.id()] {
        assert!(doc.entities().get(h).is_err(), "compensated entity is gone");
    }
    println!("OpGroup compensated: created entities deleted");

    // ── assert_revision: read-modify-write guard ──
    let rev = doc.revision()?;
    doc.assert_revision(rev)?; // unchanged -> ok
    doc.solids().create_cuboid([0.0, 0.0, 0.0], [1.0; 3])?; // moves the revision
    match doc.assert_revision(rev) {
        Err(ApiError::Validation { reason, .. }) => println!("revision guard caught: {reason}"),
        other => panic!("expected the revision guard to fail, got {other:?}"),
    }

    // A totally unknown id also surfaces a structured error.
    assert!(matches!(
        doc.entities().get(ObjectId::from_u64(0xDEAD)),
        Err(_)
    ));
    println!("failure semantics verified");
    Ok(())
}
