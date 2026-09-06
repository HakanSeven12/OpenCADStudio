//! Example 2 — Construct two solids and intersect them (the canonical boolean).
//!
//! Run: `cargo run --example e2_intersect_two_solids --features host`
//!
//! Shows: `intersect` / `union` / `subtract` on `Solid`, `erase_sources`
//! semantics (the result lives at the first input's id, the second is erased),
//! and `OpGroup` cleanup on failure.

mod common;

use ocs_doc_api::{DocApi, HasId, OpGroup};

fn main() -> ocs_doc_api::ApiResult<()> {
    let api = DocApi::in_process(common::MockBackend::default(), 0);
    let doc = api.document(api.active_tab());
    let mut grp = OpGroup::new();

    // Two overlapping boxes: [0,10]^3 and [5,15]^3.
    let a = grp.track(doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3]))?;
    let b = grp.track(doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0; 3]))?;

    // intersect -> SolidBoolean{Intersection, erase_sources:true}. The result is
    // returned AT `a`'s id; `b` is erased. [5,10]^3 = volume 125.
    let lens = match a.intersect(&b) {
        Ok(l) => {
            grp.commit(); // keep everything
            l
        }
        Err(e) => {
            grp.compensate(&doc)?; // best-effort: delete the two boxes
            return Err(e);
        }
    };

    assert_eq!(lens.id(), a.id(), "result lives at the first input's id");
    let vol = lens.volume()?;
    let bb = lens.bounds()?;
    println!("intersection volume = {vol:.2} (expected 125)");
    println!("intersection bounds = {bb:?}");

    // union / subtract follow the same shape. Fresh inputs (b was erased above).
    let c = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3])?;
    let d = doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0; 3])?;
    let uni = c.union(&d)?;
    // union volume = 1000 + 1000 - 125 (overlap) = 1875.
    println!("union volume = {:.2} (expected 1875)", uni.volume()?);

    let e = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3])?;
    let f = doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0; 3])?;
    let diff = e.subtract(&f)?;
    println!("difference volume = {:.2} (expected 875)", diff.volume()?);
    Ok(())
}
