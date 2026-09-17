// #702 repro / regression: a simple 1-page A4 drawing whose only heavy
// content is Unicode (Vietnamese) TEXT exploded into a ~128 MB PDF.
//
// Text rides the wire model as SDF glyph quads (`WireModel::text_verts`);
// `pdf_export::emit_text` re-emits each quad's atlas glyph as vector path
// ops, so the exported file stays sharp. This test measures what that costs
// in serialized content-stream bytes and fails if a change regresses the
// size class of a text-only page.
#![cfg(not(target_arch = "wasm32"))]

use OpenCADStudio::io::pdf_export::{
    render_pdf_bytes, PdfPageInput, PdfPlotOptions, PlotContent, PlotWire,
};
use OpenCADStudio::io::plot_style::{LW_TABLE, PlotStyleTable};
use OpenCADStudio::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
use OpenCADStudio::scene::pipeline::text_gpu::push_glyph_vertices;
use OpenCADStudio::scene::text::glyph_quads::layout_glyph_quads;
use OpenCADStudio::scene::text::sdf_atlas;
use OpenCADStudio::scene::WireModel;

/// Vietnamese sample with the full diacritic inventory (tonal marks,
/// vowel diacritics) — the glyphs an LFF stroke font lacks, so the atlas
/// falls back to a filled TrueType face exactly like the reporter's drawing.
const VIETNAMESE: &str =
    "Tiếng Việt: trường hợp Thanh, Hòa, rẻ, mượn, subway — Nguyễn Văn An ỗ ễ ợ ử ữ ắ ằ ẳ ẵ ặ";

fn text_wire(font: &str, text: &str, origin: [f64; 3], height: f32) -> PlotWire {
    let quads = {
        let mut atlas = sdf_atlas::text_atlas().lock().unwrap();
        layout_glyph_quads(&mut atlas, height, 0.0, 1.0, 0.0, 1.0, font, false, text)
    };
    assert!(!quads.is_empty(), "no glyph quads laid out for {text:?}");
    let mut verts = Vec::new();
    push_glyph_vertices(&mut verts, &quads, origin, 1.0, [1.0, 0.0, 0.0, 1.0], 0.0);
    PlotWire {
        wire: WireModel {
            text_verts: verts,
            ..WireModel::solid("t".into(), Vec::new(), WireModel::WHITE, false)
        },
        draw_depth: 0.0,
    }
}

fn a4_page(wires: Vec<PlotWire>, hatches: Vec<HatchModel>) -> PdfPageInput {
    PdfPageInput {
        content: PlotContent {
            wires: std::sync::Arc::new(wires),
            hatches,
            ..Default::default()
        },
        paper_w: 210.0,
        paper_h: 297.0,
        offset_x: 0.0,
        offset_y: 0.0,
        rotation_deg: 0,
        scale: 1.0,
        clip: None,
        options: PdfPlotOptions::default(),
        plot_style: None,
    }
}

/// 200 short Vietnamese annotations on one A4 sheet — a "simple" text-heavy
/// page like the one in #702.
fn vietnamese_page() -> PdfPageInput {
    let wires = (0..200)
        .map(|i| {
            let x = 10.0 + (i % 5) as f64 * 40.0;
            let y = 10.0 + (i / 5) as f64 * 7.0;
            text_wire("txt", VIETNAMESE, [x, y, 0.0], 2.5)
        })
        .collect();
    a4_page(wires, Vec::new())
}

#[test]
fn vietnamese_text_page_size_repro() {
    let page = vietnamese_page();
    let glyph_quads: usize = page
        .content
        .wires
        .iter()
        .map(|w| w.wire.text_verts.len() / 6)
        .sum();
    let bytes = render_pdf_bytes(&[page], None).expect("export");
    println!(
        "vietnamese 200-entity A4 page: {} bytes for {} glyph quads ({:.0} bytes/glyph)",
        bytes.len(),
        glyph_quads,
        bytes.len() as f64 / glyph_quads.max(1) as f64
    );
    assert!(bytes.starts_with(b"%PDF"), "not a PDF");
    // Regression fence: with per-glyph Form XObject reuse this page measures
    // ~1.2 MB (95 bytes/glyph) where the per-character outline re-emission of
    // #702 measured 51.5 MB (3,964 bytes/glyph) — 8 MB splits the two with
    // headroom for platform font differences (which only change the ~65 baked
    // unique-glyph forms, not the constant per-instance `q/cm/Do/Q` cost).
    assert!(
        bytes.len() < 8_000_000,
        "Vietnamese text page regressed to {} bytes ({} bytes/glyph) — glyph \
         outlines are being re-emitted per character again (#702)",
        bytes.len(),
        bytes.len() / glyph_quads.max(1)
    );
    if let Ok(dir) = std::env::var("OCS_PDF_DUMP") {
        let path = std::path::Path::new(&dir).join("vietnamese.pdf");
        std::fs::write(&path, &bytes).expect("dump pdf");
        println!("dumped to {}", path.display());
    }
}

/// The same glyph quads rendered with the embedded "txt" stroke font only
/// (ASCII, no system-font fallback) — deterministic on every platform, so
/// this is the leg that carries the regression bound.
#[test]
fn stroke_text_page_size_is_bounded() {
    let wires = (0..200)
        .map(|i| {
            let x = 10.0 + (i % 5) as f64 * 40.0;
            let y = 10.0 + (i / 5) as f64 * 7.0;
            text_wire(
                "txt",
                "DIMENSION TEXT 12345.6789 - TEXT ANNOTATION LINE",
                [x, y, 0.0],
                2.5,
            )
        })
        .collect();
    let page = a4_page(wires, Vec::new());
    let glyph_quads: usize = page
        .content
        .wires
        .iter()
        .map(|w| w.wire.text_verts.len() / 6)
        .sum();
    let bytes = render_pdf_bytes(&[page], None).expect("export");
    println!(
        "stroke-text 200-entity A4 page: {} bytes for {} glyph quads ({:.0} bytes/glyph)",
        bytes.len(),
        glyph_quads,
        bytes.len() as f64 / glyph_quads.max(1) as f64
    );
    assert!(bytes.starts_with(b"%PDF"), "not a PDF");
    // Regression bound: ~90 chars × 200 rows of vector stroke text must stay
    // in the hundreds-of-KB class, not tens of MB. The bound is generous
    // (~2.6x headroom over the post-fix measurement) so font/tessellation
    // updates on different platforms cannot flip it, while a return of the
    // full-precision-per-coordinate explosion (10x+) fails loudly.
    assert!(
        bytes.len() < 2_000_000,
        "text-only A4 page regressed to {} bytes ({} bytes/glyph)",
        bytes.len(),
        bytes.len() / glyph_quads.max(1)
    );
}

/// Heavy-pen (CTB lineweight override) leg: an absolute pen is arbitrary pt and
/// can exceed the glyph Form XObject /BBox's 1.5-unit SDF spread margin
/// (2.5 mm text + 1.0 mm pen → 1.79-unit half-width), squaring round caps off
/// at the tile edge. Heavy-pen entities must bypass the Forms and stroke
/// directly in page space, while the default-pen export of the same page keeps
/// its Form replay (and size class).
#[test]
fn heavy_pen_text_bypasses_forms_and_light_pen_sizes_hold() {
    let heavy_ctb = {
        let mut ctb = PlotStyleTable::identity("heavy-pen.ctb");
        let one_mm = LW_TABLE.iter().position(|&mm| mm == 1.0).unwrap() as u8;
        let entry = &mut ctb.aci_entries[1];
        entry.lineweight = one_mm;
        entry.color = Some([0, 0, 0]);
        ctb
    };
    // The same 200-entity stroke-text page as the bounded leg above, with the
    // wires on ACI 1 so the CTB's 1.0 mm pen applies.
    let wires: Vec<PlotWire> = (0..200)
        .map(|i| {
            let x = 10.0 + (i % 5) as f64 * 40.0;
            let y = 10.0 + (i / 5) as f64 * 7.0;
            let mut wire = text_wire(
                "txt",
                "DIMENSION TEXT 12345.6789 - TEXT ANNOTATION LINE",
                [x, y, 0.0],
                2.5,
            );
            wire.wire.aci = 1;
            wire
        })
        .collect();
    let glyph_quads: usize = wires.iter().map(|w| w.wire.text_verts.len() / 6).sum();
    let page = a4_page(wires, Vec::new());

    let heavy =
        render_pdf_bytes(std::slice::from_ref(&page), Some(&heavy_ctb)).expect("heavy export");
    println!(
        "heavy-pen (1.0 mm CTB) stroke-text 200-entity A4 page: {} bytes for {} glyph quads",
        heavy.len(),
        glyph_quads
    );
    assert!(heavy.starts_with(b"%PDF"), "not a PDF");
    assert!(
        heavy.len() > 100_000,
        "heavy-pen text page suspiciously small: {}",
        heavy.len()
    );
    // The bypass engaged: zero glyph Form `/Do` replays in the heavy render.
    // (Page content streams are uncompressed, so the operator is greppable.)
    let do_count = |bytes: &[u8]| bytes.windows(3).filter(|w| w == b" Do").count();
    assert_eq!(
        do_count(&heavy),
        0,
        "heavy-pen glyphs still replay clipped Form XObjects"
    );

    // Light-pen regression fence: the same page without a plot style keeps
    // every glyph on the Form path (one `/Do` per quad) and its size class.
    let light = render_pdf_bytes(std::slice::from_ref(&page), None).expect("light export");
    println!(
        "same page without CTB: {} bytes ({} Form /Do replays)",
        light.len(),
        do_count(&light)
    );
    assert!(
        do_count(&light) >= glyph_quads,
        "Form glyph replay vanished from the light path"
    );
    assert!(
        light.len() < 2_000_000,
        "light-pen text page regressed to {} bytes",
        light.len()
    );
}

/// Pattern hatch + dense polyline comparison leg: identifies which content
/// type dominates a mixed page. ANSI31-style 45° hatch over an A4-sized rect.
#[test]
fn hatch_and_polyline_page_sizes() {
    // 45° lines, 2 mm spacing → ~210 lines across the 297 mm height.
    let hatch = HatchModel {
        pattern_origin: None,
        render_instance: None,
        world_origin: [0.0, 0.0],
        boundary: std::sync::Arc::new(
            vec![
                [10.0, 10.0],
                [200.0, 10.0],
                [200.0, 287.0],
                [10.0, 287.0],
                [10.0, 10.0],
            ],
        ),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: acadrust::entities::HatchStyleType::Normal,
        pattern: HatchPattern::Pattern(vec![PatFamily {
            angle_deg: 45.0,
            x0: 0.0,
            y0: 0.0,
            dx: 2.0,
            dy: 2.0,
            dashes: vec![],
        }]),
        name: "ANSI31".to_string(),
        color: [0.0, 0.0, 0.0, 1.0],
        aci: 7,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    };
    let hatch_bytes = render_pdf_bytes(&[a4_page(Vec::new(), vec![hatch])], None).expect("export");
    println!("one ANSI31 A4 hatch page: {} bytes", hatch_bytes.len());

    // Dense survey-style polyline: 20 polylines × 2000 vertices.
    let wires = (0..20)
        .map(|row| {
            let points = (0..2000)
                .map(|i| {
                    [
                        (10.0 + i as f64 * 0.09) as f32,
                        (20.0 + row as f64 * 10.0 + (i as f64 * 0.37).sin() * 4.0) as f32,
                        0.0,
                    ]
                })
                .collect();
            PlotWire {
                wire: WireModel::solid("p".into(), points, WireModel::WHITE, false),
                draw_depth: 0.0,
            }
        })
        .collect();
    let wire_bytes = render_pdf_bytes(&[a4_page(wires, Vec::new())], None).expect("export");
    println!("dense polyline page (40k verts): {} bytes", wire_bytes.len());
}
