// Regression for #385: text and dimension text must reach the PDF / print
// export. From v0.8.2 text renders on-screen only as GPU SDF glyph quads
// (`WireModel::text_verts`); the CPU PDF exporter used to draw only wire
// stroke `points`, so all text — standalone TEXT/MTEXT and dimension text —
// vanished from exported PDFs (present in v0.7.6).
//
// The op-level placement checks live in `pdf_export`'s own unit tests, which can
// see `emit_text`'s ops. What only an integration test can cover is that the
// real entity -> scene.entity_wires() -> export_pdf path carries text at all,
// per entity kind — so that is what this asserts.
use acadrust::entities::{Dimension, DimensionLinear, Text};
use acadrust::types::Vector3;
use acadrust::EntityType;
use OpenCADStudio::io::pdf_export::{export_pdf, PdfPlotOptions, PlotWire};
use OpenCADStudio::scene::Scene;

#[test]
fn text_and_dim_reach_pdf_export() {
    let mut scene = Scene::new();

    let t = Text::with_value("HELLO", Vector3::new(10.0, 10.0, 0.0)).with_height(5.0);
    let text_h = scene.add_entity(EntityType::Text(t));

    // A linear dimension — its measurement value is synthesized as SDF text too,
    // and dimension text is what #385 was actually filed about.
    let dim = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(40.0, 0.0, 0.0));
    let dim_h = scene.add_entity(EntityType::Dimension(Dimension::Linear(dim)));

    let wires = scene.entity_wires();

    // Assert per entity, not "some wire somewhere has text": both asserts below
    // would otherwise be satisfied by the TEXT alone, leaving a dimension-text
    // regression — the actual reported bug — green.
    // Tessellation names each wire after its entity handle (see the
    // `w.name.parse::<u64>()` lookup in the render pipeline).
    let has_text = |h: acadrust::types::Handle| {
        let want = h.value().to_string();
        wires
            .iter()
            .any(|w| w.name == want && !w.text_verts.is_empty())
    };
    assert!(has_text(text_h), "TEXT entity emitted no text glyphs");
    assert!(
        has_text(dim_h),
        "DIMENSION entity emitted no measurement-value text glyphs"
    );

    // End-to-end: the same wire set with and without text. A private temp dir
    // keeps concurrent runs (and other users on a shared build host) from
    // colliding on a fixed name in the shared temp root.
    let dir = std::env::temp_dir().join(format!("ocs385-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let plot_wires: Vec<PlotWire> = wires
        .iter()
        .map(|w| PlotWire {
            wire: w.clone(),
            draw_depth: 0.0,
        })
        .collect();

    let p_text = dir.join("with_text.pdf");
    export_pdf(
        &plot_wires,
        &[],
        &[],
        210.0,
        297.0,
        0.0,
        0.0,
        0,
        1.0,
        None,
        &p_text,
        None,
        PdfPlotOptions::default(),
    )
    .expect("export with text");
    let with_text = std::fs::read(&p_text).expect("read pdf");
    assert!(with_text.starts_with(b"%PDF"), "not a PDF");

    let stripped: Vec<_> = plot_wires
        .into_iter()
        .map(|mut w| {
            w.wire.text_verts.clear();
            w
        })
        .collect();
    let p_bare = dir.join("no_text.pdf");
    export_pdf(
        &stripped,
        &[],
        &[],
        210.0,
        297.0,
        0.0,
        0.0,
        0,
        1.0,
        None,
        &p_bare,
        None,
        PdfPlotOptions::default(),
    )
    .expect("export without text");
    let no_text = std::fs::read(&p_bare).expect("read pdf");

    assert!(
        with_text.len() > no_text.len(),
        "text added no content to the PDF: {} !> {}",
        with_text.len(),
        no_text.len()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
