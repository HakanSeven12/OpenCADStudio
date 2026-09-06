//! Example 3 — Transform a solid and chain geometric operations.
//!
//! Run: `cargo run --example e3_transform_and_chain --features host`
//!
//! Shows: `transform(PlacementSpec)` (in-place, same ObjectId), how bounds shift
//! exactly by the translation, and chaining intersect -> transform.

mod common;

use ocs_doc_api::{DocApi, PlacementSpec};

fn main() -> ocs_doc_api::ApiResult<()> {
    let api = DocApi::in_process(common::MockBackend::default(), 0);
    let doc = api.document(api.active_tab());

    let a = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0; 3])?;
    let b = doc.solids().create_cuboid([5.0, 5.0, 5.0], [10.0; 3])?;

    // Boolean first: lens = [5,10]^3.
    let lens = a.intersect(&b)?;
    let before = lens.bounds()?;
    println!("lens bounds before transform: {before:?}");

    // Transform is in-place: the SAME ObjectId is preserved; bounds shift by the
    // translation. [5,10]^3 moved +100 in X -> [105,110] x [5,10] x [5,10].
    lens.transform(PlacementSpec::at([100.0, 0.0, 0.0]))?;
    let after = lens.bounds()?;
    println!("lens bounds after +100 X    : {after:?}");
    assert!((after.min[0] - 105.0).abs() < 1e-4);
    assert!((after.max[0] - 110.0).abs() < 1e-4);
    assert_eq!(after.min[1], before.min[1], "Y unchanged");

    // Volume is invariant under a rigid transform.
    let vol = lens.volume()?;
    println!("lens volume after transform = {vol:.2} (still ~125)");
    assert!((vol - 125.0).abs() < 1.0);
    Ok(())
}
