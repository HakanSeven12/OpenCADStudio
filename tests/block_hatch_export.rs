// Regression: hatch fills that live *inside* a block INSERT must reach the
// plot / PDF-export hatch set, not just the on-screen viewport.
//
// The export path (`paper_canvas_hatches`) used to collect only hatches owned
// directly by the layout block, so a hatch nested in a block was dropped and a
// plot printed the block as bare monochrome outlines. `synced_hatch_models`
// (the viewport) explodes visible INSERTs and materializes their fills; both
// paths now share `exploded_insert_hatch_models`.
use acadrust::entities::hatch::{
    BoundaryEdge, BoundaryPath, BoundaryPathFlags, HatchPatternLine, LineEdge,
};
use acadrust::entities::Hatch;
use acadrust::types::{Color as AcadColor, Vector2};
use acadrust::EntityType;
use OpenCADStudio::scene::model::hatch_model::HatchPattern;
use OpenCADStudio::scene::Scene;

fn is_blue(c: &[f32; 4]) -> bool {
    c[2] > 0.85 && c[0] < 0.20 && c[1] < 0.20
}

/// Build a 10x10 square solid hatch of the given ACI colour, offset by (cx, cy).
fn square_hatch_at(aci: u8, cx: f64, cy: f64) -> Hatch {
    let mut path = BoundaryPath::new();
    for (s, e) in [
        ((0.0, 0.0), (10.0, 0.0)),
        ((10.0, 0.0), (10.0, 10.0)),
        ((10.0, 10.0), (0.0, 10.0)),
        ((0.0, 10.0), (0.0, 0.0)),
    ] {
        path.edges.push(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(s.0 + cx, s.1 + cy),
            end: Vector2::new(e.0 + cx, e.1 + cy),
        }));
    }
    let mut hatch = Hatch::new();
    hatch.paths.push(path);
    hatch.common.color = AcadColor::Index(aci); // 5 = blue
    hatch
}

fn square_hatch(aci: u8) -> Hatch {
    square_hatch_at(aci, 0.0, 0.0)
}

// A pattern (non-solid) hatch that carries its OWN resolved pattern line —
// a 45° family whose world-unit perpendicular spacing is `spacing` — plus a
// large `pattern_scale` that the (wrong) catalog path would multiply in.
fn ansi31_stored(spacing: f64, pattern_scale: f64) -> Hatch {
    ansi31_stored_at(spacing, pattern_scale, 0.0, 0.0)
}

fn ansi31_stored_at(spacing: f64, pattern_scale: f64, cx: f64, cy: f64) -> Hatch {
    let mut hatch = square_hatch_at(5, cx, cy);
    hatch.is_solid = false;
    hatch.pattern.name = "ANSI31".into();
    // offset = perpendicular step to the next line at 45°: (-s/√2, s/√2).
    let d = spacing / std::f64::consts::SQRT_2;
    // Set the field directly — `set_pattern_scale()` would recompute and
    // clobber the stored lines we are deliberately testing.
    hatch.pattern_scale = pattern_scale;
    hatch.pattern.lines = vec![HatchPatternLine {
        angle: std::f64::consts::FRAC_PI_4,
        base_point: Vector2::new(0.0, 0.0),
        offset: Vector2::new(-d, d),
        dash_lengths: vec![],
    }];
    hatch
}

// Self-contained guard that runs everywhere (no external asset needed).
#[test]
fn block_internal_hatch_reaches_export() {
    let mut scene = Scene::new();

    // A blue hatch, wrapped into a block and inserted in model space — the
    // minimal shape of "coloured fill nested in a block".
    let h = scene.add_entity(EntityType::Hatch(square_hatch(5)));
    let identity = acadrust::types::Transform::identity();
    scene
        .create_block_from_entities(&[h], "LOGO", &identity, &identity)
        .expect("wrap hatch into a block + insert");
    scene.populate_hatches_from_document();

    let hatches = scene.paper_plot_hatches();
    let blue = hatches.iter().filter(|m| is_blue(&m.color)).count();
    assert!(
        blue > 0,
        "block-internal hatch dropped from the export set (len={}) — a plot \
         would print the block without its colours",
        hatches.len()
    );
}

// A pattern hatch that stores its own resolved line geometry must render at
// THAT spacing — not the name-matched catalog spacing × pattern_scale. Before
// the fix, ANSI31 was re-derived from the metric catalog (3.175) × scale, so a
// hatch authored at ~0.5-unit spacing collapsed to a near-empty few lines.
#[test]
fn pattern_hatch_uses_stored_line_spacing() {
    let mut scene = Scene::new();
    // Spacing 0.5; a big pattern_scale (10) that the catalog path would apply.
    scene.add_entity(EntityType::Hatch(ansi31_stored(0.5, 10.0)));
    scene.populate_hatches_from_document();

    let hatches = scene.paper_plot_hatches();
    let m = hatches
        .iter()
        .find(|m| matches!(m.pattern, HatchPattern::Pattern(_)))
        .expect("pattern hatch present in export set");

    let HatchPattern::Pattern(fams) = &m.pattern else { unreachable!() };
    // Effective perpendicular spacing = family.dy * model.scale.
    let dy = fams[0].dy.abs();
    let spacing = dy * m.scale;
    assert!(
        (spacing - 0.5).abs() < 0.05,
        "expected ~0.5-unit line spacing from the stored offset, got {spacing} \
         (dy={dy}, scale={}) — catalog×pattern_scale would give ~31.75",
        m.scale
    );
    // Density sanity: a 10x10 boundary at 0.5 spacing -> ~28 lines, not ~1.
    let lines = m.pattern_segments().len();
    assert!(
        lines >= 10,
        "expected a dense fill (~28 lines), got {lines} — pattern too sparse"
    );
}

// A fine-spaced pattern hatch placed far from the pattern origin (0,0) must
// still fill. `pattern_segments` used to clamp the ABSOLUTE line index to
// ±MAX_LINES_PER_FAMILY; a hatch thousands of units away has large-magnitude k
// on both ends, so the clamp inverted the range and emitted nothing — a
// fine-spaced fill far from the origin printed as empty outlines.
#[test]
fn far_from_origin_pattern_hatch_still_fills() {
    let mut scene = Scene::new();
    // The offset must be PERPENDICULAR to the 45° hatch lines to drive k far
    // from 0 — a diagonal offset (e.g. (4000,4000)) lies ALONG the lines and
    // projects to k≈0, so it would not exercise the clamp at all. At (4000,0)
    // the perpendicular index is |k| ≈ 4000·sin45°/0.3 ≈ 9400, well past the
    // 4096 clamp on both ends; the old absolute-index clamp inverts the range
    // (k_lo > k_hi) there and emits nothing.
    scene.add_entity(EntityType::Hatch(ansi31_stored_at(0.3, 1.0, 4000.0, 0.0)));
    scene.populate_hatches_from_document();

    let hatches = scene.paper_plot_hatches();
    let m = hatches
        .iter()
        .find(|m| matches!(m.pattern, HatchPattern::Pattern(_)))
        .expect("pattern hatch present");
    let lines = m.pattern_segments().len();
    assert!(
        lines >= 10,
        "far-from-origin pattern hatch produced {lines} lines — fill was dropped"
    );
}

// A TEXTBOX boundary path (AutoCAD's text-bounding-box, used only for island
// detection) must NOT be filled. Painting its rectangle solid produces a
// phantom bar AutoCAD never shows.
#[test]
fn textbox_boundary_path_is_not_filled() {
    fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BoundaryPath {
        let mut p = BoundaryPath::new();
        for (s, e) in [
            ((x0, y0), (x1, y0)),
            ((x1, y0), (x1, y1)),
            ((x1, y1), (x0, y1)),
            ((x0, y1), (x0, y0)),
        ] {
            p.edges.push(BoundaryEdge::Line(LineEdge {
                start: Vector2::new(s.0, s.1),
                end: Vector2::new(e.0, e.1),
            }));
        }
        p
    }

    let mut scene = Scene::new();
    let mut hatch = Hatch::new();
    hatch.common.color = AcadColor::Index(5);
    // A small real fill region...
    hatch.paths.push(rect_path(0.0, 0.0, 10.0, 10.0));
    // ...plus a large TEXTBOX rectangle that must be ignored.
    let mut tb = rect_path(0.0, 0.0, 200.0, 50.0);
    tb.flags = BoundaryPathFlags::from_bits(8 | 1); // TEXTBOX + EXTERNAL
    hatch.paths.push(tb);
    scene.add_entity(EntityType::Hatch(hatch));
    scene.populate_hatches_from_document();

    let hatches = scene.paper_plot_hatches();
    let m = hatches.first().expect("hatch present");
    // The boundary must not extend into the 200x50 TEXTBOX rectangle.
    let max_x = m
        .boundary
        .iter()
        .filter(|v| v[0].is_finite())
        .map(|v| v[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_x < 50.0,
        "TEXTBOX rectangle leaked into the fill boundary (max_x={max_x}) — it \
         would paint a phantom bar"
    );
}

// A hatch created in-app (HATCH command -> Scene::add_hatch) stores its pattern
// through build_dxf_pattern and is then rebuilt via hatch_model_from_dxf. The
// rebuilt spacing must equal the catalog's own spacing — not a rotated,
// too-dense value. Regression: build_dxf_pattern wrote the pattern line-LOCAL
// step into the world-frame `offset`, so the prebaked reader inverse-rotated it
// and ANSI31 at 45° collapsed its spacing by cos(45°) (3.175 -> 2.245) on both
// the viewport and the PDF/plot export.
#[test]
fn app_created_hatch_roundtrips_catalog_spacing() {
    use std::sync::Arc;

    use OpenCADStudio::scene::model::hatch_model::{HatchModel, PatFamily};
    use OpenCADStudio::scene::model::hatch_patterns;

    // Effective perpendicular spacing of a family, exactly as pattern_segments
    // computes it: rotate the local step out by the angle, project onto the
    // line-perpendicular direction.
    fn perp_spacing(f: &PatFamily, scale: f32) -> f32 {
        let a = f.angle_deg.to_radians();
        let (ca, sa) = (a.cos(), a.sin());
        let step_x = (f.dx * ca - f.dy * sa) * scale;
        let step_y = (f.dx * sa + f.dy * ca) * scale;
        (step_x * -sa + step_y * ca).abs()
    }

    let entry = hatch_patterns::find("ANSI31").expect("ANSI31 in catalog");
    let HatchPattern::Pattern(cat_fams) = &entry.gpu else {
        panic!("ANSI31 is a line pattern")
    };
    let expected = perp_spacing(&cat_fams[0], 1.0);

    // Build the model the way the HATCH command does: catalog family, scale 1.
    let mut scene = Scene::new();
    let boundary: Vec<[f32; 2]> = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let model = HatchModel {
        render_instance: None,
        world_origin: [0.0, 0.0],
        boundary: Arc::new(boundary),
        boundary_wcs: None,
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: acadrust::entities::HatchStyleType::Normal,
        pattern: entry.gpu.clone(),
        name: "ANSI31".into(),
        color: [0.75, 0.75, 0.75, 0.85],
        aci: 0,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    };
    scene.add_hatch(model, None, None);
    scene.populate_hatches_from_document();

    let hatches = scene.paper_plot_hatches();
    let m = hatches
        .iter()
        .find(|m| matches!(m.pattern, HatchPattern::Pattern(_)))
        .expect("pattern hatch present after round-trip");
    let HatchPattern::Pattern(fams) = &m.pattern else { unreachable!() };
    let got = perp_spacing(&fams[0], m.scale);
    assert!(
        (got - expected).abs() < expected * 0.02,
        "app-created ANSI31 round-tripped to spacing {got}, expected ~{expected} \
         — build_dxf_pattern must store the world-frame offset, not the local step"
    );
}

// Regression: a picked "big minus small" hatch must serialize the outer ring
// with the external / outermost flags and each hole ring WITHOUT them. If every
// NaN-separated ring were flagged external, DXF/DWG consumers would treat the
// inner loop as another outer island instead of a hole.
#[test]
fn nested_hatch_serializes_only_outer_as_external() {
    use std::sync::Arc;

    use OpenCADStudio::scene::model::hatch_model::HatchModel;

    let outer: Vec<[f64; 2]> = vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
    let hole: Vec<[f64; 2]> = vec![[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];

    // Outer boundary + hole, NaN-separated, exactly as the HATCH command packs
    // them in `boundary_wcs`.
    let mut wcs: Vec<[f64; 2]> = outer.clone();
    wcs.push([f64::NAN, f64::NAN]);
    wcs.extend(hole.iter().copied());

    let boundary_f32: Vec<[f32; 2]> = wcs.iter().map(|&[x, y]| [x as f32, y as f32]).collect();

    let model = HatchModel {
        render_instance: None,
        world_origin: [0.0, 0.0],
        boundary: Arc::new(boundary_f32),
        boundary_wcs: Some(Arc::new(wcs)),
        fill_plane: None,
        fill_plane_boundary: None,
        boundary_exterior: None,
        boundary_sources: None,
        boundary_paths: None,
        style: acadrust::entities::HatchStyleType::Normal,
        pattern: HatchPattern::Solid,
        name: "SOLID".into(),
        color: [0.45, 0.45, 0.45, 0.60],
        aci: 0,
        line_weight_px: 1.0,
        angle_offset: 0.0,
        scale: 1.0,
        draw_depth: 0.0,
    };

    let mut scene = Scene::new();
    scene.add_hatch(model, None, None);

    let dxf = scene
        .document
        .entities()
        .find_map(|e| if let EntityType::Hatch(h) = e { Some(h) } else { None })
        .expect("nested hatch written to document");

    assert_eq!(dxf.paths.len(), 2, "outer boundary + one hole path");

    let ex = BoundaryPathFlags::EXTERNAL.bits();
    let out = BoundaryPathFlags::OUTERMOST.bits();

    assert!(dxf.paths[0].flags.bits() & ex != 0, "outer path must be flagged external");
    assert!(dxf.paths[0].flags.bits() & out != 0, "outer path must be flagged outermost");
    assert!(dxf.paths[1].flags.bits() & ex == 0, "hole path must NOT be flagged external");
    assert!(dxf.paths[1].flags.bits() & out == 0, "hole path must NOT be flagged outermost");
}
