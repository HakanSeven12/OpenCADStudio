//! A wide polyline seen through a paper-space viewport must carry its band
//! width in PAPER units once projected for plotting. `viewport_content_wires`
//! scales the points and the linetype stations by the viewport scale but used
//! to leave `world_width` in model units, so the PDF / print exporter stroked a
//! 15-unit-wide bus bar at 15 mm of paper (a 1:100 viewport should give
//! 0.15 mm): every single-line diagram plotted as black blobs.
#![cfg(not(target_arch = "wasm32"))]

use acadrust::entities::{LwPolyline, LwVertex, Viewport};
use acadrust::types::{Vector2, Vector3};
use acadrust::EntityType;
use OpenCADStudio::scene::Scene;

#[test]
fn viewport_projection_scales_wide_polyline_band_width() {
    // A fresh `Scene::new()` has no paper layout block yet; reuse the small
    // fixture the image-export test plots through, which defines `Layout1`.
    let mut scene = Scene::new();
    scene.document = OpenCADStudio::io::load_bytes(
        "embedded-image-print.dxf",
        include_bytes!("fixtures/embedded-image-print.dxf").to_vec(),
    )
    .expect("fixture DXF with a Layout1");
    scene.set_current_layout("Model".into());

    // Model space: a 15-unit-wide bus bar 100 units long.
    let mut bar = LwPolyline::new();
    bar.vertices = vec![
        LwVertex::new(Vector2::new(0.0, 0.0)),
        LwVertex::new(Vector2::new(100.0, 0.0)),
    ];
    bar.constant_width = 15.0;
    scene.add_entity(EntityType::LwPolyline(bar));

    // Paper space: a 20 x 20 mm viewport showing 200 model units of height,
    // i.e. a 1:10 view (0.1 paper units per model unit).
    scene.set_current_layout("Layout1".into());
    let mut viewport = Viewport::new();
    viewport.center = Vector3::new(50.0, 50.0, 0.0);
    viewport.width = 20.0;
    viewport.height = 20.0;
    viewport.view_height = 200.0;
    viewport.view_center = Vector3::new(50.0, 0.0, 0.0);
    viewport.id = 2;
    viewport.status.is_on = true;
    scene.add_entity(EntityType::Viewport(viewport));

    let (_paper, model) = scene.plot_wire_groups(None);
    let band = model
        .iter()
        .find(|w| w.world_width > 0.0)
        .expect("the wide polyline must reach the plot wire set with a band width");

    // 15 model units * 0.1 = 1.5 paper units. Before the fix this stayed 15.0.
    assert!(
        (band.world_width - 1.5).abs() < 1e-3,
        "band width must be scaled into paper units: got {}",
        band.world_width
    );
}
