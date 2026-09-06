//! Example 4 — 2D entities and bulk creation.
//!
//! Run: `cargo run --example e4_2d_entities_and_bulk --features host`
//!
//! Shows: the minimal-2D constructors (`create_line`/`create_circle`/
//! `create_polyline`/`create_point`), `bounds()` on 2D entities, and the bulk op
//! `create_points` (one op, all-or-nothing, one undo step).

mod common;

use ocs_doc_api::{DocApi, HasId};

fn main() -> ocs_doc_api::ApiResult<()> {
    let api = DocApi::in_process(common::MockBackend::default(), 0);
    let doc = api.document(api.active_tab());

    // Individual 2D constructors.
    let line = doc
        .curves()
        .create_line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0])?;
    let circle = doc.curves().create_circle([5.0, 5.0, 0.0], 3.0)?;
    let poly = doc.curves().create_polyline(
        &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        true,
    )?;

    println!("line   bounds: {:?}", line.bounds()?); // [0,10] in X
    println!("circle bounds: {:?}", circle.bounds()?); // centre ± radius
    println!("poly   bounds: {:?}", poly.bounds()?); // unit square

    // Bulk: 1000 points in ONE op (all-or-nothing, one undo step — not 1000).
    let coords: Vec<[f64; 3]> = (0..1000)
        .map(|i| [i as f64, (i % 10) as f64, 0.0])
        .collect();
    let pts = doc.curves().create_points(&coords)?;
    println!("created {} points in one bulk op", pts.len());
    assert_eq!(pts.len(), 1000);

    // Generic lookup: any id -> Entity -> kind.
    let e = doc.entities().get(line.id())?;
    println!("line entity kind: {}", e.view()?.kind);

    Ok(())
}
