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
