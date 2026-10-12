//! RED-TEAM: the Model-branch sheet floor in `layout_plot_page_for`
//! (src/app/update/file.rs:3765-3800, `MIN_MODEL_SHEET_MM = 10.0`) attacked
//! through its REAL input: `Scene::model_space_extents`
//! (src/scene/mod.rs:10925 `compute_model_space_extents`).
//!
//! The App method is `pub(super)` and unconstructible outside the app, so the
//! floor arithmetic is replicated verbatim below and fed the REAL extents of
//! hostile scenes (NaN / ±inf / f64-overflow / reversed-header / huge /
//! exactly-10mm / 9.99mm drawings). The question: can a NaN or non-finite
//! page ever reach `pdf_export`, where `width < MIN` silently compares false
//! for NaN and the "normal" branch would emit a NaN-sized `Mm(paper_w)`?
#![cfg(not(target_arch = "wasm32"))]

use codec::entities::Line;
use codec::types::Vector3;
use codec::EntityType;
use OpenCADStudio::scene::Scene;

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> EntityType {
    let mut l = Line::new();
    l.start = Vector3::new(x1, y1, 0.0);
    l.end = Vector3::new(x2, y2, 0.0);
    EntityType::Line(l)
}

/// Verbatim copy of the Model-branch sheet arithmetic
/// (src/app/update/file.rs:3765-3800), reduced to its inputs:
/// `model_space_extents()` + the dialog sheet (default A4 landscape 297x210).
const MIN_MODEL_SHEET_MM: f64 = 10.0;
fn model_sheet(extents: Option<(f64, f64, f64, f64, f64, f64)>) -> (f64, f64, f64, f64) {
    let selected_sheet: (f64, f64) = (297.0, 210.0);
    let standard_sheet = {
        let (w, h) = selected_sheet;
        (w.min(h) >= MIN_MODEL_SHEET_MM).then_some((w, h))
    };
    if let Some((min_x, min_y, _, max_x, max_y, _)) = extents {
        let margin = 1.05_f64;
        let width = ((max_x - min_x) as f64 * margin).max(1.0);
        let height = ((max_y - min_y) as f64 * margin).max(1.0);
        if width < MIN_MODEL_SHEET_MM || height < MIN_MODEL_SHEET_MM {
            let (w, h) = standard_sheet.unwrap_or((297.0, 210.0));
            let page_w = width.max(w);
            let page_h = height.max(h);
            (
                page_w,
                page_h,
                (page_w - width) * 0.5 - min_x as f64,
                (page_h - height) * 0.5 - min_y as f64,
            )
        } else {
            let pad_x = (width - (max_x - min_x) as f64) * 0.5;
            let pad_y = (height - (max_y - min_y) as f64) * 0.5;
            (width, height, -(min_x as f64) + pad_x, -(min_y as f64) + pad_y)
        }
    } else {
        (297.0, 210.0, 0.0, 0.0)
    }
}

fn flat_extents(scene: &Scene) -> Option<(f64, f64, f64, f64, f64, f64)> {
    scene
        .model_space_extents()
        .map(|(min, max)| (min.x as f64, min.y as f64, min.z as f64, max.x as f64, max.y as f64, max.z as f64))
}

/// Entities with NaN coordinates must not poison the sheet: the extents pass
/// in `compute_model_space_extents` drops non-finite vertices
/// (src/scene/mod.rs:10945 `(x as f32).is_finite()`), so NaN alone yields
/// `None` (default 297x210 page) and NaN mixed with real geometry yields the
/// finite bounds only. The `width < MIN` NaN trap in the floor branch is
/// unreachable through this input.
#[test]
fn nan_entities_cannot_produce_a_nan_or_floored_wrongly_page() {
    // Only-NaN drawing: no usable extents at all.
    let mut scene = Scene::new();
    scene.add_entity(line(f64::NAN, f64::NAN, f64::NAN, f64::NAN));
    scene.add_entity(line(0.0, f64::NAN, 10.0, f64::NAN));
    let extents = flat_extents(&scene);
    assert!(
        extents.is_none(),
        "NaN-only geometry must yield None, got {extents:?}"
    );
    let (w, h, ox, oy) = model_sheet(extents);
    assert_eq!((w, h), (297.0, 210.0));
    assert_eq!((ox, oy), (0.0, 0.0));

    // NaN contaminating an otherwise finite drawing.
    let mut scene = Scene::new();
    scene.add_entity(line(0.0, 0.0, 10.0, 10.0));
    scene.add_entity(line(f64::NAN, 5.0, f64::NAN, 5.0));
    let (w, h, ox, oy) = model_sheet(flat_extents(&scene));
    assert_eq!((w, h), (10.5, 10.5), "10mm * 1.05 margin, NaN entity ignored");
    assert!(w.is_finite() && h.is_finite() && ox.is_finite() && oy.is_finite());

    // Every scene fed to the floor keeps the invariant the branch promises:
    // a real, finite page of at least MIN on both axes.
    let mut hostile = vec![
        Scene::new(), // empty drawing
    ];
    let mut inf_scene = Scene::new();
    inf_scene.add_entity(line(f64::NEG_INFINITY, 0.0, f64::INFINITY, f64::INFINITY));
    hostile.push(inf_scene);
    let mut overflow_scene = Scene::new(); // finite f64, overflows f32
    overflow_scene.add_entity(line(-1e300, -1e300, 1e300, 1e300));
    hostile.push(overflow_scene);
    let mut huge_f32_scene = Scene::new(); // finite even after the f32 cast
    huge_f32_scene.add_entity(line(-3.0e38, -3.0e38, 3.0e38, 3.0e38));
    hostile.push(huge_f32_scene);
    for scene in &hostile {
        let extents = flat_extents(scene);
        if let Some((mnx, mny, _, mxx, mxy, _)) = extents {
            assert!(
                mnx.is_finite() && mny.is_finite() && mxx.is_finite() && mxy.is_finite(),
                "extents must be finite: {extents:?}"
            );
            assert!(mnx <= mxx && mny <= mxy, "min/max must be ordered: {extents:?}");
        }
        let (w, h, ox, oy) = model_sheet(extents);
        assert!(w.is_finite() && h.is_finite() && ox.is_finite() && oy.is_finite());
        assert!(w >= MIN_MODEL_SHEET_MM && h >= MIN_MODEL_SHEET_MM, "({w}, {h})");
    }
}

/// A drawing whose only coordinates exceed f32 range (e.g. 1e300) is silently
/// dropped from the extents (the `(x as f32).is_finite()` filter) — the plot
/// falls back to the default sheet, so the drawing PLOTS EMPTY rather than
/// erroring. Behavior note, not a crash.
#[test]
fn beyond_f32_range_geometry_is_silently_ignored_by_extents() {
    let mut scene = Scene::new();
    scene.add_entity(line(1e300, 1e300, 2e300, 2e300));
    assert!(
        flat_extents(&scene).is_none(),
        "1e300 coordinates overflow the f32 wire cache and vanish from extents"
    );
    // While geometry at f32::MAX-ish scale stays (absurd page, but finite).
    let mut scene = Scene::new();
    scene.add_entity(line(-3.0e38, 0.0, 3.0e38, 3.0e38));
    let (w, h, _, _) = model_sheet(flat_extents(&scene));
    assert!(w > 1.0e38 && h > 1.0e38, "({w}, {h})");
}

/// The reversed-extents case (min > max) cannot come from
/// `compute_model_space_extents` (min starts at +inf and only decreases) and
/// the EXTMIN/EXTMAX header fallback rejects it with a strict `<`
/// (src/scene/mod.rs:11005). A fresh document's header is reversed by
/// DEFAULT (1e20 / -1e20 sentinels) and its model-space block record exists,
/// so an empty drawing exercises the header path immediately.
#[test]
fn reversed_extents_are_unreachable_even_from_a_hostile_header() {
    // Real parsed document (has a *Model_Space block), all entities removed:
    // the header fallback is the only extents source left. One fresh scene
    // per case — `model_space_extents` caches by geometry_epoch, and header
    // mutation alone does not bump the epoch.
    let empty_scene = || {
        let mut scene = Scene::new();
        scene.document = OpenCADStudio::io::load_bytes(
            "embedded-image-print.dxf",
            include_bytes!("fixtures/embedded-image-print.dxf").to_vec(),
        )
        .expect("fixture");
        let handles: Vec<_> = scene.document.entities().map(|e| e.common().handle).collect();
        for handle in handles {
            scene.document.remove_entity(handle);
        }
        scene
    };
    // Reversed header: rejected.
    let mut scene = empty_scene();
    scene.document.header.model_space_extents_min = Vector3::new(100.0, 100.0, 0.0);
    scene.document.header.model_space_extents_max = Vector3::new(-100.0, -100.0, 0.0);
    assert!(
        flat_extents(&scene).is_none(),
        "reversed EXTMIN/EXTMAX must be rejected, not negated into a page"
    );
    // NaN header: also rejected by the strict `<` (NaN comparisons are false).
    let mut scene = empty_scene();
    scene.document.header.model_space_extents_min = Vector3::new(f64::NAN, f64::NAN, 0.0);
    scene.document.header.model_space_extents_max = Vector3::new(10.0, 10.0, 0.0);
    assert!(flat_extents(&scene).is_none(), "NaN EXTMIN must be rejected");
    // Ordered, sane header alone (no drawable geometry) feeds the plot
    // through the fallback path.
    let mut scene = empty_scene();
    scene.document.header.model_space_extents_min = Vector3::new(-100.0, -100.0, 0.0);
    scene.document.header.model_space_extents_max = Vector3::new(100.0, 100.0, 0.0);
    let extents = flat_extents(&scene)
        .expect("ordered sane header feeds the fallback");
    assert_eq!((extents.0, extents.1, extents.3, extents.4), (-100.0, -100.0, 100.0, 100.0));
    let (w, h, ox, oy) = model_sheet(Some(extents));
    // 200mm extents * 1.05 = 210mm page (normal branch, above the floor);
    // offset = -min + (210 - 200)/2 margin padding = 100 + 5.
    assert_eq!((w, h), (210.0, 210.0));
    assert!((ox - 105.0).abs() < 1e-9 && (oy - 105.0).abs() < 1e-9);
}

/// The floor boundary: the floor compares the MARGIN-INCLUDED size
/// (`extents * 1.05`), not the raw extents, so "exactly 10.0mm" and even
/// 9.99mm drawings take the NORMAL branch (page = 10.5mm and 10.4895mm
/// respectively) — the floor only catches extents below 10/1.05 ≈ 9.524mm.
/// A 9.5mm x 10000mm profile IS floored, per-axis: 297mm wide, 10500mm tall,
/// with the content centered by the offsets.
#[test]
fn floor_boundary_math_per_axis() {
    // Exactly 10.0mm extents: normal branch (10 * 1.05 = 10.5 >= 10).
    let (w, h, _, _) = model_sheet(Some((0.0, 0.0, 0.0, 10.0, 10.0, 0.0)));
    assert_eq!((w, h), (10.5, 10.5));
    // 9.99 x 10000: STILL the normal branch (9.99 * 1.05 = 10.4895 >= 10) —
    // the task's guess "(297, 10000+5%)" is off by the margin.
    let (w, h, _, _) = model_sheet(Some((0.0, 0.0, 0.0, 9.99, 10000.0, 0.0)));
    assert!((w - 10.4895).abs() < 1e-9);
    assert!((h - 10500.0).abs() < 1e-9);
    // 9.5 x 10000: floored per-axis — thin axis lifted to the standard sheet
    // width (297), long axis keeps its margin size, content centered.
    let (w, h, ox, oy) = model_sheet(Some((0.0, 0.0, 0.0, 9.5, 10000.0, 0.0)));
    assert_eq!(w, 297.0);
    assert!((h - 10500.0).abs() < 1e-9);
    // width = 9.5 * 1.05 = 9.975 → offset_x = (297 - 9.975)/2 - 0 = 143.5125
    assert!((ox - (297.0 - 9.975) * 0.5).abs() < 1e-9);
    // height = 10500 exactly fills the page → offset_y = -min_y = 0.
    assert!((oy - 0.0).abs() < 1e-9);
    // A dot (zero extents): width clamped to 1.0 by `.max(1.0)`, floored to
    // the full standard sheet, offsets center the dot's point.
    let (w, h, ox, oy) = model_sheet(Some((5.0, 7.0, 0.0, 5.0, 7.0, 0.0)));
    assert_eq!((w, h), (297.0, 210.0));
    assert!((ox - ((297.0 - 1.0) * 0.5 - 5.0)).abs() < 1e-9);
    assert!((oy - ((210.0 - 1.0) * 0.5 - 7.0)).abs() < 1e-9);
}

/// Defense-in-depth check at the pdf_export layer: if a NaN page size ever
/// DID reach it (future regression, or a non-Model branch), what happens?
/// `PdfPageInput.paper_w = NaN` used to be serialized verbatim into a
/// MediaBox containing the literal string "nan" — a nominally-valid PDF no
/// conforming viewer can display (and which printpdf's own parser even
/// accepts). The paper fields are now validated like the content
/// coordinates (src/io/pdf_export.rs `append_pdf_page`): non-finite or
/// non-positive dimensions are rejected with a plain-English error.
#[test]
fn pdf_export_layer_faced_with_a_nan_page_size() {
    use OpenCADStudio::io::pdf_export::{export_pdf, PdfPageInput, PdfPlotOptions};
    let path = std::env::temp_dir().join(format!(
        "ocs-redteam-nan-page-{}.pdf",
        std::process::id()
    ));
    for (paper_w, paper_h) in [
        (f64::NAN, f64::NAN),
        (f64::NAN, 297.0),
        (210.0, f64::INFINITY),
        (f64::NEG_INFINITY, 297.0),
        (0.0, 297.0),
        (210.0, 0.0),
        (-210.0, 297.0),
    ] {
        let page = PdfPageInput {
            content: Default::default(),
            paper_w,
            paper_h,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0,
            scale: 1.0,
            clip: None,
            options: PdfPlotOptions::default(),
            plot_style: None,
        };
        let error = export_pdf(&page, &path)
            .expect_err("non-finite/zero sheet sizes must be rejected, not plotted");
        assert!(
            error.contains("plot sheet size is not valid"),
            "page ({paper_w}, {paper_h}): {error}"
        );
        assert!(
            !path.exists(),
            "no artifact may be written for a rejected sheet"
        );
    }
    // A sane sheet still exports normally.
    let page = PdfPageInput {
        content: Default::default(),
        paper_w: 210.0,
        paper_h: 297.0,
        offset_x: 0.0,
        offset_y: 0.0,
        rotation_deg: 0,
        scale: 1.0,
        clip: None,
        options: PdfPlotOptions::default(),
        plot_style: None,
    };
    export_pdf(&page, &path).expect("sane sheet exports");
    assert!(path.exists());
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).to_lowercase().contains("nan"),
        "no NaN may leak into a written page"
    );
    let _ = std::fs::remove_file(&path);
}
