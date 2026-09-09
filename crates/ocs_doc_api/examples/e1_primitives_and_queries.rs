//! Example 1 — Create solid primitives and query their geometry.
//!
//! Run: `cargo run --example e1_primitives_and_queries --features host`
//!
//! Shows: `doc.solids().create_*` for the supported primitives, and the read
//! queries `bounds()` / `volume()` / `centroid()` on a `Solid`.

mod common;

use ocs_doc_api::DocApi;

fn main() -> ocs_doc_api::ApiResult<()> {
    // In-process document over the mock backend (no host binary needed).
    let api = DocApi::in_process(common::MockBackend::default(), 0);
    let doc = api.document(api.active_tab());

    // The supported solid primitives.
    let block = doc.solids().create_cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])?;
    let ball = doc.solids().create_sphere([5.0, 5.0, 5.0], 6.0)?;
    let cyl = doc.solids().create_cylinder([0.0, 0.0, 0.0], 4.0, 12.0)?;

    // Geometric queries are entity methods — one host call each.
    let bb = block.bounds()?;
    let vol = ball.volume()?;
    let cg = cyl.centroid()?;

    println!("cuboid  bounds : {bb:?}");
    println!("sphere  volume : {vol:.2}  (analytic ~904.78)");
    println!("cylinder centroid: {cg:?}");

    // Read-only batching: same queries, ONE round-trip.
    let res = doc.query_batch(|q| {
        q.bounds(&block);
        q.volume(&ball);
        q.centroid(&cyl);
    })?;
    let (bb2, vol2, cg2) = (res.bounds(0)?, res.volume(1)?, res.centroid(2)?);
    assert_eq!(bb, bb2);
    assert!((vol - vol2).abs() < 1e-9);
    assert_eq!(cg, cg2);
    println!("query_batch matches the per-call results");
    Ok(())
}
