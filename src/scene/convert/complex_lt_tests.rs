//! A complex linetype walks the whole entity, each run end-aligned like
//! the "A" alignment: it begins and ends with a dash, its glyphs sit
//! symmetrically, and a run shorter than one pattern is drawn solid.

use std::collections::HashSet;

use document::entities::{Circle, Insert, Line, LwPolyline, LwVertex, MLine};
use document::tables::linetype::{LineTypeComplexContent, LineTypeComplexData, LineTypeElement};
use document::tables::{BlockRecord, Layer, LineType, TextStyle};
use document::types::{Vector2, Vector3};
use document::{Document, EntityType, Handle};

use crate::scene::model::wire_model::WireModel;

/// A fence linetype: a 1.5 dash, then an "X" centred in a 0.02 gap, for a
/// 1.52 period (a site plan's property-line linetype has this shape).
fn fence_document() -> Document {
    let mut document = Document::new();
    let mut style = TextStyle::new("FENCE-TEXT");
    style.handle = document.allocate_handle();
    style.font_file = "txt.shx".to_string();
    let style_handle = style.handle;
    document.text_styles.add(style).unwrap();
    let mut fence = LineType::new("FENCE");
    fence.handle = document.allocate_handle();
    fence.add_element(LineTypeElement::dash(1.5));
    fence.add_element(LineTypeElement {
        length: -0.01,
        complex: Some(LineTypeComplexData {
            content: LineTypeComplexContent::Text { text: "X".into() },
            style_handle,
            scale: 0.25,
            offset: [-0.1, -0.125],
            source_flags: 2,
            ..Default::default()
        }),
    });
    fence.add_element(LineTypeElement::space(0.01));
    fence.pattern_length = 1.52;
    document.line_types.add(fence).unwrap();
    document
}

fn add(document: &mut Document, mut entity: EntityType) -> Handle {
    entity.common_mut().linetype = "FENCE".to_string();
    document.add_entity(entity).unwrap()
}

fn wires(document: &Document, handle: Handle) -> Vec<WireModel> {
    wires_at(document, handle, 1.0, false)
}

fn wires_at(document: &Document, handle: Handle, anno: f32, paper_space: bool) -> Vec<WireModel> {
    let entity = document.get_entity(handle).expect("entity").clone();
    crate::scene::tessellate_entity(
        document,
        &HashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        anno,
        None,
        &entity,
        None,
        None,
        None,
        paper_space.then_some(1.0),
        &HashSet::default(),
    )
}

/// The dashes, as absolute XY points.
fn strokes(wires: &[WireModel]) -> Vec<Vec<[f64; 2]>> {
    wires
        .iter()
        .filter(|wire| wire.points.len() >= 2)
        .map(|wire| {
            wire.points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let low = wire.points_low.get(i).copied().unwrap_or([0.0; 3]);
                    [p[0] as f64 + low[0] as f64, p[1] as f64 + low[1] as f64]
                })
                .collect()
        })
        .collect()
}

/// The centre of each "X", from the glyph quads (glyphs sit at least a dash
/// apart, so vertices within half a unit belong to one glyph).
fn glyph_centres(wires: &[WireModel]) -> Vec<[f64; 2]> {
    let mut glyphs: Vec<Vec<[f64; 2]>> = Vec::new();
    for v in wires.iter().flat_map(|wire| &wire.text_verts) {
        let p = [
            v.pos[0] as f64 + v.pos_low[0] as f64,
            v.pos[1] as f64 + v.pos_low[1] as f64,
        ];
        match glyphs.iter_mut().find(|g| dist(g[0], p) < 0.5) {
            Some(glyph) => glyph.push(p),
            None => glyphs.push(vec![p]),
        }
    }
    glyphs
        .iter()
        .map(|g| {
            let n = g.len() as f64;
            [
                g.iter().map(|p| p[0]).sum::<f64>() / n,
                g.iter().map(|p| p[1]).sum::<f64>() / n,
            ]
        })
        .collect()
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn assert_near(points: &[[f64; 2]], expected: &[[f64; 2]], tol: f64) {
    assert_eq!(points.len(), expected.len(), "{points:?} vs {expected:?}");
    for want in expected {
        assert!(
            points.iter().any(|p| dist(*p, *want) < tol),
            "nothing near {want:?} in {points:?}"
        );
    }
}

/// Both ends of every dash.
fn dash_ends(wires: &[WireModel]) -> Vec<[f64; 2]> {
    strokes(wires)
        .iter()
        .flat_map(|s| [s[0], s[s.len() - 1]])
        .collect()
}

fn assert_dash_ends_at(wires: &[WireModel], points: &[[f64; 2]]) {
    let ends = dash_ends(wires);
    for point in points {
        assert!(
            ends.iter().any(|p| dist(*p, *point) < 1e-4),
            "no dash ends at {point:?}"
        );
    }
}

/// (0,0) → (2,0), an arc bulging to (2.5,1) → (2,2), then (2,3) and (5.5,3).
const WITH_ARC: [(f64, f64, f64); 5] = [
    (0.0, 0.0, 0.0),
    (2.0, 0.0, 0.5),
    (2.0, 2.0, 0.0),
    (2.0, 3.0, 0.0),
    (5.5, 3.0, 0.0),
];

/// (x, y, bulge) vertices.
fn polyline(vertices: &[(f64, f64, f64)], plinegen: bool) -> EntityType {
    let mut polyline = LwPolyline::new();
    for &(x, y, bulge) in vertices {
        let mut vertex = LwVertex::new(Vector2::new(x, y));
        vertex.bulge = bulge;
        polyline.add_vertex(vertex);
    }
    polyline.plinegen = plinegen;
    EntityType::LwPolyline(polyline)
}

#[test]
fn complex_linetype_walks_every_segment_of_a_polyline_with_an_arc() {
    // PLINEGEN off: each segment carries its own end-aligned pattern. The
    // 2 line and the 2.32 arc fit one "X" each, at their middle; the 1.0 line
    // is shorter than a pattern and stays solid; the 3.5 line holds two
    // periods, so two more.
    // The arc used to be the only part drawn: the polyline tessellates to
    // one wire per arc plus one of its straight segments, and only the first
    // wire was walked.
    let mut document = fence_document();
    let handle = add(&mut document, polyline(&WITH_ARC, false));
    let wires = wires(&document, handle);
    assert_near(
        &glyph_centres(&wires),
        &[[1.0, 0.0], [2.5, 1.0], [2.99, 3.0], [4.51, 3.0]],
        0.15,
    );
    assert_dash_ends_at(
        &wires,
        &[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [2.0, 3.0], [5.5, 3.0]],
    );
    assert!(
        strokes(&wires)
            .iter()
            .any(|s| dist(s[0], [2.0, 2.0]) < 1e-4 && dist(s[s.len() - 1], [2.0, 3.0]) < 1e-4),
        "the short segment is one solid dash"
    );
}

#[test]
fn complex_linetype_restarts_at_each_vertex_of_a_straight_polyline() {
    // PLINEGEN off without arcs: the pattern used to run on through the
    // vertices from the start point. Each segment now begins and ends with a
    // dash, its "X" placed symmetrically: one on the 2 and 3 segments, two on
    // the 3.5 one.
    let mut document = fence_document();
    let straight = [
        (0.0, 0.0, 0.0),
        (2.0, 0.0, 0.0),
        (2.0, 3.0, 0.0),
        (5.5, 3.0, 0.0),
    ];
    let handle = add(&mut document, polyline(&straight, false));
    let wires = wires(&document, handle);
    assert_near(
        &glyph_centres(&wires),
        &[[1.0, 0.0], [2.0, 1.5], [2.99, 3.0], [4.51, 3.0]],
        0.15,
    );
    assert_dash_ends_at(&wires, &[[0.0, 0.0], [2.0, 0.0], [2.0, 3.0], [5.5, 3.0]]);
}

#[test]
fn complex_linetype_runs_on_through_the_vertices_with_plinegen() {
    // PLINEGEN on: one pattern along the whole 8.82 path — five "X" 1.52
    // apart, the first 1.369 from the start.
    let mut document = fence_document();
    let handle = add(&mut document, polyline(&WITH_ARC, true));
    let wires = wires(&document, handle);
    let glyphs = glyph_centres(&wires);
    assert_eq!(glyphs.len(), 5, "{glyphs:?}");
    assert!(
        glyphs.iter().any(|p| dist(*p, [1.369, 0.0]) < 0.15),
        "{glyphs:?}"
    );
    assert!(
        glyphs.iter().any(|p| dist(*p, [4.13, 3.0]) < 0.15),
        "{glyphs:?}"
    );
    assert_dash_ends_at(&wires, &[[0.0, 0.0], [5.5, 3.0]]);
}

#[test]
fn complex_linetype_line_begins_and_ends_with_equal_dashes() {
    // 5.0 holds three whole 1.52 periods: 0.97 end dashes, three "X" placed
    // symmetrically.
    let mut document = fence_document();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0));
    let handle = add(&mut document, EntityType::Line(line));
    let wires = wires(&document, handle);
    let strokes = strokes(&wires);
    let mut spans: Vec<[f64; 2]> = strokes
        .iter()
        .map(|s| {
            [
                s[0][0].min(s[s.len() - 1][0]),
                s[0][0].max(s[s.len() - 1][0]),
            ]
        })
        .collect();
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(spans.len(), 4, "{spans:?}");
    let first = spans[0][1] - spans[0][0];
    let last = spans[3][1] - spans[3][0];
    assert!(spans[0][0].abs() < 1e-4 && (spans[3][1] - 5.0).abs() < 1e-4);
    assert!(
        (first - 0.97).abs() < 1e-3 && (last - 0.97).abs() < 1e-3,
        "{spans:?}"
    );
    assert_eq!(glyph_centres(&wires).len(), 3);
}

#[test]
fn complex_linetype_shorter_than_one_pattern_is_solid() {
    // 1.51 is past the first dash but short of the 1.52 period: one solid
    // dash, no "X" squeezed against the end.
    let mut document = fence_document();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.51, 0.0, 0.0));
    let handle = add(&mut document, EntityType::Line(line));
    let wires = wires(&document, handle);
    assert!(glyph_centres(&wires).is_empty());
    let strokes = strokes(&wires);
    assert_eq!(strokes.len(), 1);
    let stroke = &strokes[0];
    assert!(dist(stroke[0], [0.0, 0.0]) < 1e-4);
    assert!(dist(stroke[stroke.len() - 1], [1.51, 0.0]) < 1e-4);
}

/// "X" centres along a run from `start`, measured along it (glyphs at least a
/// dash apart; strokes within `reach` belong to one glyph).
fn glyph_stations(wires: &[WireModel], start: [f64; 2], reach: f64) -> Vec<f64> {
    let mut glyphs: Vec<Vec<[f64; 2]>> = Vec::new();
    for v in wires.iter().flat_map(|wire| &wire.text_verts) {
        let p = [
            v.pos[0] as f64 + v.pos_low[0] as f64,
            v.pos[1] as f64 + v.pos_low[1] as f64,
        ];
        match glyphs.iter_mut().find(|g| dist(g[0], p) < reach) {
            Some(glyph) => glyph.push(p),
            None => glyphs.push(vec![p]),
        }
    }
    let mut stations: Vec<f64> = glyphs
        .iter()
        .map(|g| {
            let n = g.len() as f64;
            let c = [
                g.iter().map(|p| p[0]).sum::<f64>() / n,
                g.iter().map(|p| p[1]).sum::<f64>() / n,
            ];
            dist(c, start)
        })
        .collect();
    stations.sort_by(f64::total_cmp);
    stations
}

#[test]
fn complex_linetype_fits_every_whole_period() {
    // At linetype scale 15 the fence period is 22.8 and its dash 22.5. A run
    // holds floor(L / 22.8) periods, and the leftover goes to the two end
    // dashes, each at least half a dash. 46 used to fit one "X" and 93.37
    // three, with end dashes near a full dash: one "X" short on such runs.
    let cases: [(f64, &[f64]); 10] = [
        (20.0, &[]),
        (30.0, &[15.0]),
        (40.72, &[20.36]),
        (46.0, &[11.6, 34.4]),
        (50.0, &[13.6, 36.4]),
        (57.0, &[17.1, 39.9]),
        (70.0, &[12.2, 35.0, 57.8]),
        (80.0, &[17.2, 40.0, 62.8]),
        (93.37, &[12.49, 35.29, 58.09, 80.89]),
        (100.0, &[15.8, 38.6, 61.4, 84.2]),
    ];
    for (length, want) in cases {
        let mut document = fence_document();
        let mut line =
            Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(length, 0.0, 0.0));
        line.common.linetype_scale = 15.0;
        let handle = add(&mut document, EntityType::Line(line));
        let got = glyph_stations(&wires(&document, handle), [0.0, 0.0], 8.0);
        assert_eq!(got.len(), want.len(), "L = {length}: {got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 0.3, "L = {length}: {got:?} vs {want:?}");
        }
    }
}

/// Model space at annotation scale 1:15 lays the fence out like linetype scale
/// 15 at 1:1: the 22.8 period of `complex_linetype_fits_every_whole_period`.
const FENCE_AT_15: [f64; 4] = [15.8, 38.6, 61.4, 84.2];

fn assert_fence_at_15(got: &[f64]) {
    assert_eq!(got.len(), FENCE_AT_15.len(), "{got:?}");
    for (g, w) in got.iter().zip(FENCE_AT_15) {
        assert!((g - w).abs() < 0.3, "{got:?} vs {FENCE_AT_15:?}");
    }
}

#[test]
fn complex_linetype_scales_by_the_model_space_annotation_scale() {
    let mut document = fence_document();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(100.0, 0.0, 0.0));
    let handle = add(&mut document, EntityType::Line(line));
    let got = glyph_stations(&wires_at(&document, handle, 15.0, false), [0.0, 0.0], 8.0);
    assert_fence_at_15(&got);
}

#[test]
fn complex_linetype_inside_a_viewport_ignores_the_annotation_override() {
    let mut document = fence_document();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(100.0, 0.0, 0.0));
    let handle = add(&mut document, EntityType::Line(line));
    let unscaled = glyph_stations(&wires(&document, handle), [0.0, 0.0], 0.5);
    let viewport = glyph_stations(&wires_at(&document, handle, 15.0, true), [0.0, 0.0], 0.5);
    assert_eq!(unscaled.len(), viewport.len());
}

#[test]
fn changing_the_annotation_scale_relays_a_complex_linetype() {
    let mut scene = crate::scene::Scene::new();
    scene.document = fence_document();
    let mut line = EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(100.0, 0.0, 0.0),
    ));
    line.common_mut().linetype = "FENCE".to_string();
    scene.add_entity(line);

    scene.set_annotation_scale_named("1:1").expect("1:1");
    let at_1_1 = glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5).len();
    assert!(at_1_1 > 60, "{at_1_1} glyphs at 1:1");

    scene.set_annotation_scale_named("1:15").expect("1:15");
    assert_fence_at_15(&glyph_stations(&scene.entity_wires(), [0.0, 0.0], 8.0));

    scene.set_annotation_scale_named("1:1").expect("1:1");
    let back = glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5).len();
    assert_eq!(back, at_1_1);
}

#[test]
fn msltscale_off_keeps_a_complex_linetype_at_its_drawing_length() {
    let mut document = fence_document();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(100.0, 0.0, 0.0));
    let handle = add(&mut document, EntityType::Line(line));
    let unscaled = glyph_stations(&wires(&document, handle), [0.0, 0.0], 0.5);

    assert!(document.set_drawing_variable("MSLTSCALE", "0"));
    let off = glyph_stations(&wires_at(&document, handle, 15.0, false), [0.0, 0.0], 0.5);
    assert_eq!(off.len(), unscaled.len());

    assert!(document.set_drawing_variable("MSLTSCALE", "1"));
    let on = glyph_stations(&wires_at(&document, handle, 15.0, false), [0.0, 0.0], 8.0);
    assert_fence_at_15(&on);
}

#[test]
fn msltscale_drives_the_model_space_shader_ratio() {
    use crate::scene::view::render::model_linetype_ratio;
    let mut document = Document::new();
    document.header.annotation_scale_value = 0.02;
    // Not stored: on.
    let ratio = model_linetype_ratio(&document).expect("on by default");
    assert!((ratio - 50.0).abs() < 1e-3, "{ratio}");

    assert!(document.set_drawing_variable("MSLTSCALE", "0"));
    assert_eq!(model_linetype_ratio(&document), None);

    assert!(document.set_drawing_variable("MSLTSCALE", "1"));
    assert!(model_linetype_ratio(&document).is_some());
}

#[test]
fn plotting_model_space_applies_the_annotation_scale_to_plain_dashes() {
    let mut scene = crate::scene::Scene::new();
    let mut dashed = LineType::new("DASHED-T");
    dashed.handle = scene.document.allocate_handle();
    dashed.add_element(LineTypeElement::dash(2.0));
    dashed.add_element(LineTypeElement::space(2.0));
    dashed.pattern_length = 4.0;
    scene.document.line_types.add(dashed).unwrap();
    let mut line = EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(400.0, 0.0, 0.0),
    ));
    line.common_mut().linetype = "DASHED-T".to_string();
    scene.add_entity(line);

    let plotted = |scene: &crate::scene::Scene| {
        let (wires, viewports) = scene.plot_wire_groups(None);
        assert!(viewports.is_empty());
        let wire = wires
            .into_iter()
            .find(|wire| wire.pattern_length > 0.0)
            .expect("a dashed wire");
        (wire.pattern_length, wire.pattern[0])
    };

    scene.set_annotation_scale_named("1:10").expect("1:10");
    let (length, dash) = plotted(&scene);
    assert!((length - 40.0).abs() < 1e-3, "{length}");
    assert!((dash - 20.0).abs() < 1e-3, "{dash}");

    scene.set_model_linetype_scaling(false);
    let (length, dash) = plotted(&scene);
    assert!((length - 4.0).abs() < 1e-3, "{length}");
    assert!((dash - 2.0).abs() < 1e-3, "{dash}");
}

#[test]
fn msltscale_toggle_relays_a_complex_linetype() {
    let mut scene = crate::scene::Scene::new();
    scene.document = fence_document();
    let mut line = EntityType::Line(Line::from_points(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(100.0, 0.0, 0.0),
    ));
    line.common_mut().linetype = "FENCE".to_string();
    scene.add_entity(line);

    scene.set_annotation_scale_named("1:15").expect("1:15");
    assert_fence_at_15(&glyph_stations(&scene.entity_wires(), [0.0, 0.0], 8.0));

    scene.set_model_linetype_scaling(false);
    assert!(!scene.model_linetype_scaling());
    let off = glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5).len();
    assert!(off > 60, "{off} glyphs with MSLTSCALE off");

    scene.set_model_linetype_scaling(true);
    assert_fence_at_15(&glyph_stations(&scene.entity_wires(), [0.0, 0.0], 8.0));
}

#[test]
fn dashed_line_end_dashes_keep_every_whole_period() {
    // The shader path uses the same rule: 2.3 holds three 0.75 periods of a
    // 0.5 dash, so each end dash is (2.3 - 2.25 + 0.5) / 2 = 0.275.
    let mut document = Document::new();
    let mut dashed = LineType::new("DASHED-T");
    dashed.handle = document.allocate_handle();
    dashed.add_element(LineTypeElement::dash(0.5));
    dashed.add_element(LineTypeElement::space(0.25));
    dashed.pattern_length = 0.75;
    document.line_types.add(dashed).unwrap();
    let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(2.3, 0.0, 0.0));
    line.common.linetype = "DASHED-T".to_string();
    let handle = document.add_entity(EntityType::Line(line)).unwrap();
    let wire = wires(&document, handle)
        .into_iter()
        .find(|w| w.pattern_length > 0.0)
        .expect("a dashed wire");
    let dashes = crate::scene::pipeline::wire_gpu::wire_distances(&wire);
    let align_total = dashes.run_totals[0];
    assert!((align_total - 2.3).abs() < 1e-4, "{align_total}");
    let end_dash = crate::scene::pipeline::wire_gpu::aligned_end_dash(
        align_total,
        wire.pattern_length,
        wire.pattern[0],
    );
    assert!((end_dash - 0.275).abs() < 1e-4, "{end_dash}");
}

// ── Inside a block ───────────────────────────────────────────────────────────

/// A second linetype with FENCE's dash pattern and a longer text.
fn add_wide_fence(document: &mut Document) {
    let style_handle = document.text_styles.get("FENCE-TEXT").unwrap().handle;
    let mut fence = LineType::new("WIDE");
    fence.handle = document.allocate_handle();
    fence.add_element(LineTypeElement::dash(1.5));
    fence.add_element(LineTypeElement {
        length: -0.01,
        complex: Some(LineTypeComplexData {
            content: LineTypeComplexContent::Text { text: "XX".into() },
            style_handle,
            scale: 0.25,
            offset: [-0.1, -0.125],
            source_flags: 2,
            ..Default::default()
        }),
    });
    fence.add_element(LineTypeElement::space(0.01));
    fence.pattern_length = 1.52;
    document.line_types.add(fence).unwrap();
}

fn new_block(document: &mut Document, name: &str) -> Handle {
    let mut block = BlockRecord::new(name);
    block.handle = document.allocate_handle();
    let handle = block.handle;
    document.block_records.add(block).unwrap();
    handle
}

/// A block of `name` holding one 100-long line on layer `layer`.
fn line_block(document: &mut Document, name: &str, layer: &str, linetype: &str, lt_scale: f64) {
    let owner = new_block(document, name);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(100.0, 0.0, 0.0));
    line.common.owner_handle = owner;
    line.common.layer = layer.to_string();
    line.common.linetype = linetype.to_string();
    line.common.linetype_scale = lt_scale;
    document.add_entity(EntityType::Line(line)).unwrap();
}

fn insert_wires(document: &Document, insert: Insert, anno: f32) -> Vec<WireModel> {
    crate::scene::tessellate_entity(
        document,
        &HashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        anno,
        None,
        &EntityType::Insert(insert),
        None,
        None,
        None,
        None,
        &HashSet::default(),
    )
}

/// Glyph stations of a top-level `length`-long FENCE line.
fn line_stations(length: f64, lt_scale: f64, anno: f32, reach: f64) -> Vec<f64> {
    let mut document = fence_document();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(length, 0.0, 0.0));
    line.common.linetype_scale = lt_scale;
    let handle = add(&mut document, EntityType::Line(line));
    glyph_stations(&wires_at(&document, handle, anno, false), [0.0, 0.0], reach)
}

fn assert_same_stations(got: &[f64], expected: &[f64]) {
    assert_eq!(got.len(), expected.len(), "{got:?} vs {expected:?}");
    assert!(!got.is_empty());
    for (g, e) in got.iter().zip(expected) {
        assert!((g - e).abs() < 1e-2, "{got:?} vs {expected:?}");
    }
}

#[test]
fn complex_linetype_keeps_its_glyphs_on_a_block_child() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let wires = insert_wires(&document, Insert::new("B", Vector3::ZERO), 1.0);
    assert_same_stations(
        &glyph_stations(&wires, [0.0, 0.0], 0.5),
        &line_stations(100.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_in_a_block_follows_the_annotation_scale() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let wires = insert_wires(&document, Insert::new("B", Vector3::ZERO), 15.0);
    assert_fence_at_15(&glyph_stations(&wires, [0.0, 0.0], 8.0));
}

#[test]
fn complex_linetype_in_a_block_stays_at_drawing_length_with_msltscale_off() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    assert!(document.set_drawing_variable("MSLTSCALE", "0"));
    let wires = insert_wires(&document, Insert::new("B", Vector3::ZERO), 15.0);
    assert_same_stations(
        &glyph_stations(&wires, [0.0, 0.0], 0.5),
        &line_stations(100.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_keeps_its_length_in_a_scaled_insert() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    assert_same_stations(
        &glyph_stations(&insert_wires(&document, insert, 1.0), [0.0, 0.0], 0.5),
        &line_stations(200.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_is_not_stretched_by_a_non_uniform_insert() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(2.0);
    assert_same_stations(
        &glyph_stations(&insert_wires(&document, insert, 1.0), [0.0, 0.0], 0.5),
        &line_stations(200.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_uses_the_scale_of_the_block_child() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 2.0);
    let wires = insert_wires(&document, Insert::new("B", Vector3::ZERO), 1.0);
    assert_same_stations(
        &glyph_stations(&wires, [0.0, 0.0], 1.0),
        &line_stations(100.0, 2.0, 1.0, 1.0),
    );
}

#[test]
fn complex_linetype_keeps_its_glyphs_in_a_nested_block() {
    let mut document = fence_document();
    line_block(&mut document, "INNER", "0", "FENCE", 1.0);
    let owner = new_block(&mut document, "OUTER");
    let mut nested = Insert::new("INNER", Vector3::ZERO);
    nested.set_x_scale(0.5);
    nested.set_y_scale(0.5);
    nested.common.owner_handle = owner;
    document.add_entity(EntityType::Insert(nested)).unwrap();

    let mut insert = Insert::new("OUTER", Vector3::ZERO);
    insert.set_x_scale(4.0);
    insert.set_y_scale(4.0);
    assert_same_stations(
        &glyph_stations(&insert_wires(&document, insert, 1.0), [0.0, 0.0], 0.5),
        &line_stations(200.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_reaches_a_byblock_child() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "ByBlock", 1.0);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.linetype = "FENCE".to_string();
    // Neither the INSERT's scale nor its own linetype scale reaches the child.
    insert.common.linetype_scale = 3.0;
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    assert_same_stations(
        &glyph_stations(&insert_wires(&document, insert, 1.0), [0.0, 0.0], 0.5),
        &line_stations(200.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_reaches_a_child_on_layer_0_through_the_insert_layer() {
    let mut document = fence_document();
    let mut layer = Layer::new("FENCES");
    layer.handle = document.allocate_handle();
    layer.line_type = "FENCE".to_string();
    document.layers.add(layer).unwrap();
    line_block(&mut document, "B", "0", "ByLayer", 1.0);

    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.layer = "FENCES".to_string();
    assert_same_stations(
        &glyph_stations(
            &insert_wires(&document, insert.clone(), 1.0),
            [0.0, 0.0],
            0.5,
        ),
        &line_stations(100.0, 1.0, 1.0, 0.5),
    );

    // On layer 0 itself there is nothing to inherit: the child stays plain.
    insert.common.layer = "0".to_string();
    let plain = insert_wires(&document, insert, 1.0);
    assert!(glyph_stations(&plain, [0.0, 0.0], 0.5).is_empty());
    assert!(plain.iter().all(|wire| !wire.snap_only));
}

#[test]
fn complex_linetype_passes_through_a_nested_byblock_insert() {
    let mut document = fence_document();
    line_block(&mut document, "INNER", "0", "ByBlock", 1.0);
    let owner = new_block(&mut document, "OUTER");
    let mut nested = Insert::new("INNER", Vector3::ZERO);
    nested.common.linetype = "ByBlock".to_string();
    nested.common.owner_handle = owner;
    document.add_entity(EntityType::Insert(nested)).unwrap();

    let mut insert = Insert::new("OUTER", Vector3::ZERO);
    insert.common.linetype = "FENCE".to_string();
    assert_same_stations(
        &glyph_stations(&insert_wires(&document, insert, 1.0), [0.0, 0.0], 0.5),
        &line_stations(100.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn complex_linetype_glyphs_take_the_inherited_colour_of_a_block_child() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    document.entities_mut().next().unwrap().common_mut().color = document::types::Color::ByBlock;
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.color = document::types::Color::from_index(1);
    let wires = insert_wires(&document, insert, 1.0);
    let glyph = wires
        .iter()
        .flat_map(|wire| &wire.text_verts)
        .next()
        .unwrap();
    assert!(
        glyph.color[0] > 0.9 && glyph.color[1] < 0.1 && glyph.color[2] < 0.1,
        "{:?}",
        glyph.color
    );
}

#[test]
fn complex_linetype_walks_a_block_polyline_with_an_arc_as_one_path() {
    let vertices = [
        (0.0, 0.0, 0.0),
        (20.0, 0.0, 1.0),
        (20.0, 20.0, 0.0),
        (0.0, 20.0, 0.0),
    ];
    let mut top = fence_document();
    let handle = add(&mut top, polyline(&vertices, true));
    let expected = glyph_stations(&wires(&top, handle), [0.0, 0.0], 0.5);

    let mut document = fence_document();
    let owner = new_block(&mut document, "B");
    let mut child = polyline(&vertices, true);
    child.common_mut().owner_handle = owner;
    child.common_mut().linetype = "FENCE".to_string();
    document.add_entity(child).unwrap();
    let got = glyph_stations(
        &insert_wires(&document, Insert::new("B", Vector3::ZERO), 1.0),
        [0.0, 0.0],
        0.5,
    );
    assert_same_stations(&got, &expected);
}

#[test]
fn a_walked_block_child_keeps_its_snaps_on_hidden_geometry() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let wires = insert_wires(&document, Insert::new("B", Vector3::ZERO), 1.0);
    let hidden: Vec<&WireModel> = wires.iter().filter(|wire| wire.snap_only).collect();
    assert_eq!(hidden.len(), 1);
    assert!(!hidden[0].display_visible && !hidden[0].plot_visible);
    assert!(!hidden[0].snap_pts.is_empty() || !hidden[0].key_vertices.is_empty());
    // The walk draws its dashes as strokes: nothing is left for the shader.
    assert!(wires.iter().all(|wire| wire.pattern_length == 0.0));
}

/// Every INSERT drawn through one cache, as a drawing draws them.
fn cached_stations(document: &Document, inserts: &[Insert], start: [f64; 2]) -> Vec<Vec<f64>> {
    let cache = crate::scene::cache::block_cache::BlockCache::build(
        document,
        1.0,
        None,
        true,
        [0.0, 0.0, 0.0, 1.0],
        None,
        &Default::default(),
    );
    inserts
        .iter()
        .map(|insert| {
            let wires = crate::scene::tessellate_entity(
                document,
                &HashSet::default(),
                None,
                [0.0, 0.0, 0.0, 1.0],
                1.0,
                None,
                &EntityType::Insert(insert.clone()),
                Some(&cache),
                None,
                None,
                None,
                &HashSet::default(),
            );
            let origin = [
                insert.insert_point.x + start[0],
                insert.insert_point.y + start[1],
            ];
            glyph_stations(&wires, origin, 0.5)
        })
        .collect()
}

#[test]
fn every_copy_of_a_block_keeps_its_complex_linetype() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let inserts = [
        Insert::new("B", Vector3::ZERO),
        Insert::new("B", Vector3::new(0.0, 50.0, 0.0)),
        Insert::new("B", Vector3::new(500.0, 50.0, 0.0)),
    ];
    for inserted in &inserts {
        document
            .add_entity(EntityType::Insert(inserted.clone()))
            .unwrap();
    }
    let expected = line_stations(100.0, 1.0, 1.0, 0.5);
    for got in cached_stations(&document, &inserts, [0.0, 0.0]) {
        assert_same_stations(&got, &expected);
    }
}

#[test]
fn copies_inheriting_different_complex_linetypes_keep_them_apart() {
    let mut document = fence_document();
    add_wide_fence(&mut document);
    line_block(&mut document, "B", "0", "ByBlock", 1.0);
    let mut fence = Insert::new("B", Vector3::ZERO);
    fence.common.linetype = "FENCE".to_string();
    let mut wide = Insert::new("B", Vector3::new(0.0, 50.0, 0.0));
    wide.common.linetype = "WIDE".to_string();
    document
        .add_entity(EntityType::Insert(fence.clone()))
        .unwrap();
    document
        .add_entity(EntityType::Insert(wide.clone()))
        .unwrap();

    let cache = crate::scene::cache::block_cache::BlockCache::build(
        &document,
        1.0,
        None,
        true,
        [0.0, 0.0, 0.0, 1.0],
        None,
        &Default::default(),
    );
    let quads = |insert: &Insert| -> usize {
        crate::scene::tessellate_entity(
            &document,
            &HashSet::default(),
            None,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
            None,
            &EntityType::Insert(insert.clone()),
            Some(&cache),
            None,
            None,
            None,
            &HashSet::default(),
        )
        .iter()
        .map(|wire| wire.text_verts.len())
        .sum()
    };
    let (fence_quads, wide_quads) = (quads(&fence), quads(&wide));
    assert!(fence_quads > 0);
    assert_eq!(wide_quads, 2 * fence_quads);
}

#[test]
fn changing_the_annotation_scale_relays_a_complex_linetype_in_a_block() {
    let mut scene = crate::scene::Scene::new();
    scene.document = fence_document();
    line_block(&mut scene.document, "B", "0", "FENCE", 1.0);
    scene.add_entity(EntityType::Insert(Insert::new("B", Vector3::ZERO)));

    scene.set_annotation_scale_named("1:1").expect("1:1");
    let at_1_1 = glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5).len();
    assert!(at_1_1 > 60, "{at_1_1} glyphs at 1:1");

    scene.set_annotation_scale_named("1:15").expect("1:15");
    assert_fence_at_15(&glyph_stations(&scene.entity_wires(), [0.0, 0.0], 8.0));

    scene.set_annotation_scale_named("1:1").expect("1:1");
    let back = glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5).len();
    assert_eq!(back, at_1_1);
}

#[test]
fn a_block_drawn_before_a_complex_linetype_was_loaded_can_inherit_it() {
    let vertices = [(0.0, 0.0, 0.0), (100.0, 0.0, 0.0)];
    let mut scene = crate::scene::Scene::new();
    let owner = new_block(&mut scene.document, "B");
    let mut child = polyline(&vertices, true);
    child.common_mut().owner_handle = owner;
    child.common_mut().linetype = "ByBlock".to_string();
    scene.document.add_entity(child).unwrap();
    scene.add_entity(EntityType::Insert(Insert::new(
        "B",
        Vector3::new(0.0, 50.0, 0.0),
    )));
    assert!(glyph_stations(&scene.entity_wires(), [0.0, 50.0], 0.5).is_empty());

    let source = fence_document();
    let mut style = source.text_styles.get("FENCE-TEXT").unwrap().clone();
    style.handle = scene.document.allocate_handle();
    let style_handle = style.handle;
    scene.document.text_styles.add(style).unwrap();
    let mut fence = source.line_types.get("FENCE").unwrap().clone();
    fence.handle = scene.document.allocate_handle();
    for element in &mut fence.elements {
        if let Some(complex) = &mut element.complex {
            complex.style_handle = style_handle;
        }
    }
    scene.document.line_types.add(fence).unwrap();

    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.linetype = "FENCE".to_string();
    scene.add_entity(EntityType::Insert(insert));
    assert_same_stations(
        &glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5),
        &line_stations(100.0, 1.0, 1.0, 0.5),
    );
}

// ── Around a closed path ─────────────────────────────────────────────────────

/// How many glyphs a FENCE circle of `radius` holds, and the arc length between
/// the first two.
fn circle_glyphs(document: &Document, handle: Handle, radius: f64) -> (usize, f64) {
    let angles = {
        let glyph_wires = wires(document, handle);
        let mut glyphs: Vec<Vec<[f64; 2]>> = Vec::new();
        for v in glyph_wires.iter().flat_map(|wire| &wire.text_verts) {
            let p = [
                v.pos[0] as f64 + v.pos_low[0] as f64,
                v.pos[1] as f64 + v.pos_low[1] as f64,
            ];
            match glyphs.iter_mut().find(|g| dist(g[0], p) < 0.5) {
                Some(glyph) => glyph.push(p),
                None => glyphs.push(vec![p]),
            }
        }
        let mut angles: Vec<f64> = glyphs
            .iter()
            .map(|g| {
                let n = g.len() as f64;
                let x = g.iter().map(|p| p[0]).sum::<f64>() / n;
                let y = g.iter().map(|p| p[1]).sum::<f64>() / n;
                y.atan2(x).rem_euclid(std::f64::consts::TAU)
            })
            .collect();
        angles.sort_by(f64::total_cmp);
        angles
    };
    (angles.len(), (angles[1] - angles[0]) * radius)
}

#[test]
fn a_complex_circle_holds_whole_stretched_periods_and_full_size_glyphs() {
    let mut document = fence_document();
    // L = 12.566, L/P = 8.27: 8 periods, each 1.5708 rather than 1.52.
    let circle = Circle::from_center_radius(Vector3::ZERO, 2.0);
    let handle = add(&mut document, EntityType::Circle(circle));
    let (count, spacing) = circle_glyphs(&document, handle, 2.0);
    assert_eq!(count, 8);
    assert!((spacing - 1.5708).abs() < 0.02, "{spacing}");
}

#[test]
fn a_complex_circle_shorter_than_a_period_is_solid() {
    let mut document = fence_document();
    let circle = Circle::from_center_radius(Vector3::ZERO, 0.2);
    let handle = add(&mut document, EntityType::Circle(circle));
    let drawn = wires(&document, handle);
    assert!(drawn.iter().all(|wire| wire.text_verts.is_empty()));
    assert!(drawn.iter().any(|wire| wire.points.len() > 8));
}

#[test]
fn a_complex_closed_polyline_with_plinegen_starts_at_its_second_vertex() {
    let mut document = fence_document();
    let rectangle = [
        (0.0, 0.0, 0.0),
        (16.0, 0.0, 0.0),
        (16.0, 15.2, 0.0),
        (0.0, 15.2, 0.0),
    ];
    let mut entity = polyline(&rectangle, true);
    if let EntityType::LwPolyline(ref mut p) = entity {
        p.is_closed = true;
    }
    let handle = add(&mut document, entity);
    // The first dash runs on from (16, 0), so a glyph sits one dash past it, 1.51
    // up the second side. Counted from the first vertex one would sit 0.7 up.
    let stations = glyph_stations(&wires(&document, handle), [16.0, 1.51], 0.5);
    assert!(stations[0] < 0.15, "{stations:?}");
}

// ── MLINE elements ───────────────────────────────────────────────────────────

/// An MLINE style whose elements have these offsets and linetypes.
fn add_mline_style(document: &mut Document, name: &str, elements: &[(f64, &str)]) {
    use document::objects::{MLineStyle, MLineStyleElement, ObjectType};
    let mut style = MLineStyle::new(name);
    style.handle = document.allocate_handle();
    for &(offset, linetype) in elements {
        let mut element = MLineStyleElement::new(offset);
        element.linetype = linetype.to_string();
        style.add_element(element);
    }
    document
        .objects
        .insert(style.handle, ObjectType::MLineStyle(style));
}

fn mline(style: &str, points: &[(f64, f64)]) -> MLine {
    let points: Vec<Vector3> = points
        .iter()
        .map(|&(x, y)| Vector3::new(x, y, 0.0))
        .collect();
    let mut mline = MLine::from_points(&points);
    mline.style_name = style.to_string();
    mline.scale_factor = 1.0;
    mline
}

#[test]
fn an_mline_element_walks_its_complex_linetype_like_a_line() {
    let mut document = fence_document();
    add_mline_style(&mut document, "ML", &[(0.0, "FENCE")]);
    let handle = document
        .add_entity(EntityType::MLine(mline("ML", &[(0.0, 0.0), (100.0, 0.0)])))
        .unwrap();
    assert_same_stations(
        &glyph_stations(&wires(&document, handle), [0.0, 0.0], 0.5),
        &line_stations(100.0, 1.0, 1.0, 0.5),
    );
}

#[test]
fn an_mline_element_follows_the_annotation_scale() {
    let mut document = fence_document();
    add_mline_style(&mut document, "ML", &[(0.0, "FENCE")]);
    let handle = document
        .add_entity(EntityType::MLine(mline("ML", &[(0.0, 0.0), (100.0, 0.0)])))
        .unwrap();
    assert_fence_at_15(&glyph_stations(
        &wires_at(&document, handle, 15.0, false),
        [0.0, 0.0],
        8.0,
    ));
}

#[test]
fn changing_the_annotation_scale_relays_a_complex_mline() {
    let mut scene = crate::scene::Scene::new();
    scene.document = fence_document();
    add_mline_style(&mut scene.document, "ML", &[(0.0, "FENCE")]);
    scene.add_entity(EntityType::MLine(mline("ML", &[(0.0, 0.0), (100.0, 0.0)])));

    scene.set_annotation_scale_named("1:1").expect("1:1");
    let at_1_1 = glyph_stations(&scene.entity_wires(), [0.0, 0.0], 0.5).len();
    assert!(at_1_1 > 60, "{at_1_1} glyphs at 1:1");
    scene.set_annotation_scale_named("1:15").expect("1:15");
    assert_fence_at_15(&glyph_stations(&scene.entity_wires(), [0.0, 0.0], 8.0));
}

#[test]
fn an_mline_elements_own_entity_linetype_plays_no_part() {
    let mut document = fence_document();
    add_mline_style(&mut document, "ML", &[(0.0, "Continuous")]);
    let mut entity = mline("ML", &[(0.0, 0.0), (100.0, 0.0)]);
    entity.common.linetype = "FENCE".to_string();
    let handle = document.add_entity(EntityType::MLine(entity)).unwrap();
    assert!(wires(&document, handle)
        .iter()
        .all(|wire| wire.text_verts.is_empty()));
}

#[test]
fn an_mline_element_in_a_block_keeps_its_length_and_its_glyphs() {
    let mut document = fence_document();
    add_mline_style(&mut document, "ML", &[(0.0, "FENCE")]);
    let owner = new_block(&mut document, "B");
    let mut child = mline("ML", &[(0.0, 0.0), (100.0, 0.0)]);
    child.common.owner_handle = owner;
    document.add_entity(EntityType::MLine(child)).unwrap();
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(0.5);
    insert.set_y_scale(0.5);
    assert_same_stations(
        &glyph_stations(&insert_wires(&document, insert, 1.0), [0.0, 0.0], 0.5),
        &line_stations(50.0, 1.0, 1.0, 0.5),
    );
}

/// A paper layout with one viewport showing the model at 1:`ratio`, with
/// PSLTSCALE on.
fn scene_with_viewport(document: Document, ratio: f64) -> (crate::scene::Scene, Handle) {
    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.add_layout("P1").unwrap();
    scene.current_layout = "P1".to_string();
    let mut viewport = document::entities::Viewport::new();
    viewport.width = 100.0;
    viewport.height = 100.0;
    viewport.view_height = 100.0 * ratio;
    viewport.custom_scale = 1.0 / ratio;
    let handle = scene
        .document
        .add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "P1")
        .unwrap();
    scene.document.header.paper_space_linetype_scaling = true;
    scene.bump_geometry();
    (scene, handle)
}

fn fence_line_document() -> Document {
    let mut document = fence_document();
    let line = Line::from_points(Vector3::ZERO, Vector3::new(100.0, 0.0, 0.0));
    add(&mut document, EntityType::Line(line));
    document
}

fn viewport_stations(scene: &crate::scene::Scene, viewport: Handle, reach: f64) -> Vec<f64> {
    glyph_stations(
        &scene.model_wires_for_viewport_arc(viewport, 0.0),
        [0.0, 0.0],
        reach,
    )
}

/// Turn a view that just moved into one that has settled.
fn settle_view(scene: &crate::scene::Scene) {
    let _ = scene.navigating_lod();
    let past = iced::time::Instant::now() - std::time::Duration::from_secs(1);
    scene.nav_changed_at.set(Some(past));
}

#[test]
fn psltscale_keeps_a_complex_linetype_at_its_sheet_length_in_a_viewport() {
    let (scene, viewport) = scene_with_viewport(fence_line_document(), 15.0);
    assert_fence_at_15(&viewport_stations(&scene, viewport, 8.0));
}

#[test]
fn without_psltscale_a_complex_linetype_scales_with_the_viewport() {
    let (mut scene, viewport) = scene_with_viewport(fence_line_document(), 15.0);
    scene.document.header.paper_space_linetype_scaling = false;
    assert!(viewport_stations(&scene, viewport, 0.5).len() > 60);
}

#[test]
fn psltscale_reaches_a_complex_linetype_in_a_block() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let insert = Insert::new("B", Vector3::ZERO);
    document.add_entity(EntityType::Insert(insert)).unwrap();
    let (scene, viewport) = scene_with_viewport(document, 15.0);
    assert_fence_at_15(&viewport_stations(&scene, viewport, 8.0));
}

#[test]
fn psltscale_reaches_a_complex_mline_element() {
    let mut document = fence_document();
    add_mline_style(&mut document, "FENCES", &[(0.0, "FENCE")]);
    document
        .add_entity(EntityType::MLine(mline(
            "FENCES",
            &[(0.0, 0.0), (100.0, 0.0)],
        )))
        .unwrap();
    let (scene, viewport) = scene_with_viewport(document, 15.0);
    assert_fence_at_15(&viewport_stations(&scene, viewport, 8.0));
}

#[test]
fn a_viewport_relays_its_complex_linetype_only_once_the_zoom_settles() {
    let (mut scene, viewport) = scene_with_viewport(fence_line_document(), 15.0);
    scene.active_viewport = Some(viewport);
    assert_fence_at_15(&viewport_stations(&scene, viewport, 8.0));

    // Every tick moves the camera, as the wheel does; the bake stays put.
    for _ in 0..3 {
        scene.zoom_active_viewport(-1.0, None);
        scene.camera_generation += 1;
        assert_fence_at_15(&viewport_stations(&scene, viewport, 8.0));
    }

    settle_view(&scene);
    let factor = scene
        .viewport_psltscale_factor(viewport)
        .expect("PSLTSCALE");
    assert_ne!(factor, 15.0);
    assert_eq!(
        scene.viewport_complex_lt_factor(viewport, false),
        Some(factor),
    );
    assert_same_stations(
        &viewport_stations(&scene, viewport, 8.0),
        &line_stations(100.0, factor as f64, 1.0, 8.0),
    );
}

#[test]
fn a_plot_right_after_a_zoom_takes_the_current_viewport_scale() {
    let (mut scene, viewport) = scene_with_viewport(fence_line_document(), 15.0);
    scene.active_viewport = Some(viewport);
    let _ = viewport_stations(&scene, viewport, 8.0);
    scene.zoom_active_viewport(-1.0, None);
    scene.camera_generation += 1;
    let factor = scene
        .viewport_psltscale_factor(viewport)
        .expect("PSLTSCALE");
    assert_eq!(
        scene.viewport_complex_lt_factor(viewport, false),
        Some(15.0)
    );

    let paper = scene.current_layout_block_handle();
    let _ = scene.viewport_content_wires(paper, Some(viewport), None, true);
    assert_eq!(
        scene.viewport_complex_lt_factor(viewport, false),
        Some(factor),
    );
}

#[test]
fn a_drawing_without_complex_linetypes_keeps_one_viewport_set_across_zooms() {
    let mut document = Document::new();
    document
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(100.0, 0.0, 0.0),
        )))
        .unwrap();
    let (mut scene, viewport) = scene_with_viewport(document, 15.0);
    scene.active_viewport = Some(viewport);
    assert_eq!(scene.viewport_complex_lt_factor(viewport, false), None);
    let _ = scene.model_wires_for_viewport_arc(viewport, 0.0);
    let generation = scene.last_model_wire_gen.get();
    settle_view(&scene);
    scene.document.header.paper_space_linetype_scaling = true;
    if let Some(EntityType::Viewport(vp)) = scene.document.get_entity_mut(viewport) {
        vp.view_height *= 2.0;
        vp.custom_scale = vp.height / vp.view_height;
    }
    let _ = scene.model_wires_for_viewport_arc(viewport, 0.0);
    assert_eq!(scene.last_model_wire_gen.get(), generation);
}

/// A multileader showing block `name` at the origin, with no leaders or text.
fn block_multileader(document: &Document, name: &str) -> document::entities::MultiLeader {
    use document::entities::{BlockContentConnectionType, LeaderContentType, MultiLeader};
    let mut leader = MultiLeader::new();
    leader.content_type = LeaderContentType::Block;
    leader.block_connection_type = BlockContentConnectionType::BasePoint;
    leader.block_content_handle = Some(document.block_records.get(name).unwrap().handle);
    leader.context.has_block_contents = true;
    leader.context.block_content_location = Vector3::ZERO;
    leader.context.block_content_scale = Vector3::new(1.0, 1.0, 1.0);
    leader.block_scale = Vector3::new(1.0, 1.0, 1.0);
    leader
}

#[test]
fn a_multileader_block_follows_the_annotation_scale() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let leader = block_multileader(&document, "B");
    let handle = add(&mut document, EntityType::MultiLeader(Box::new(leader)));
    assert_fence_at_15(&glyph_stations(
        &wires_at(&document, handle, 15.0, false),
        [0.0, 0.0],
        8.0,
    ));
}

#[test]
fn a_dimension_picture_block_follows_the_annotation_scale() {
    use document::entities::{Dimension, DimensionLinear};
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let mut dimension = DimensionLinear::new(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
    dimension.base.block_name = "B".to_string();
    let handle = add(
        &mut document,
        EntityType::Dimension(Box::new(Dimension::Linear(dimension))),
    );
    assert_fence_at_15(&glyph_stations(
        &wires_at(&document, handle, 15.0, false),
        [0.0, 0.0],
        8.0,
    ));
}

#[test]
fn psltscale_reaches_a_multileader_block() {
    let mut document = fence_document();
    line_block(&mut document, "B", "0", "FENCE", 1.0);
    let leader = block_multileader(&document, "B");
    add(&mut document, EntityType::MultiLeader(Box::new(leader)));
    let (scene, viewport) = scene_with_viewport(document, 15.0);
    assert_fence_at_15(&viewport_stations(&scene, viewport, 8.0));
}

// ── Wide polylines ───────────────────────────────────────────────────────────

fn wide_polyline(vertices: &[(f64, f64, f64, f64)], plinegen: bool) -> LwPolyline {
    let mut polyline = LwPolyline::new();
    polyline.plinegen = plinegen;
    polyline.vertices = vertices
        .iter()
        .map(|&(x, y, start_width, end_width)| {
            let mut vertex = LwVertex::new(Vector2::new(x, y));
            vertex.start_width = start_width;
            vertex.end_width = end_width;
            vertex
        })
        .collect();
    polyline.common.linetype = "FENCE".to_string();
    polyline
}

fn band_polyline(width: f64) -> LwPolyline {
    let mut polyline = wide_polyline(&[(0.0, 0.0, 0.0, 0.0), (100.0, 0.0, 0.0, 0.0)], true);
    polyline.constant_width = width;
    polyline
}

fn dash_wires(wires: &[WireModel]) -> Vec<&WireModel> {
    wires
        .iter()
        .filter(|wire| !wire.snap_only && wire.points.len() >= 2 && wire.text_verts.is_empty())
        .collect()
}

/// The dashes all drawn as a band of `width`, the glyphs as thin as ever.
fn assert_band(wires: &[WireModel], width: f32) {
    // A block batches its dashes into one wire, broken between them.
    let dashes: usize = dash_wires(wires)
        .iter()
        .map(|wire| wire.points.split(|p| p[0].is_nan()).count())
        .sum();
    assert!(dashes > 10, "{dashes} dashes");
    for dash in dash_wires(wires) {
        assert!(
            (dash.world_width - width).abs() < 1e-4,
            "dash band {}",
            dash.world_width
        );
        assert!(dash.taper_widths.is_empty());
    }
    let glyphs: Vec<&WireModel> = wires
        .iter()
        .filter(|wire| !wire.text_verts.is_empty())
        .collect();
    assert!(!glyphs.is_empty());
    assert!(glyphs.iter().all(|wire| wire.world_width == 0.0));
}

#[test]
fn a_wide_polyline_keeps_its_band_on_each_dash() {
    let mut document = fence_document();
    let handle = add(&mut document, EntityType::LwPolyline(band_polyline(3.0)));
    let wide = wires(&document, handle);
    assert_band(&wide, 3.0);

    let thin = add(&mut document, EntityType::LwPolyline(band_polyline(0.0)));
    assert_same_stations(
        &glyph_stations(&wide, [0.0, 0.0], 8.0),
        &glyph_stations(&wires(&document, thin), [0.0, 0.0], 8.0),
    );
}

#[test]
fn a_wide_polyline_still_picks_across_its_band() {
    let mut document = fence_document();
    let handle = add(&mut document, EntityType::LwPolyline(band_polyline(3.0)));
    let wires = wires(&document, handle);
    assert!(wires.iter().any(|wire| !wire.pick_tris.is_empty()));
}

#[test]
fn a_tapered_polyline_narrows_along_its_dashes() {
    let mut document = fence_document();
    let handle = add(
        &mut document,
        EntityType::LwPolyline(wide_polyline(
            &[(0.0, 0.0, 0.0, 6.0), (100.0, 0.0, 0.0, 0.0)],
            true,
        )),
    );
    let wires = wires(&document, handle);
    let dashes = dash_wires(&wires);
    assert!(dashes.len() > 10, "{} dashes", dashes.len());
    let mut widest: f32 = 0.0;
    for dash in dashes {
        assert_eq!(dash.taper_widths.len(), dash.points.len());
        for (point, width) in dash.points.iter().zip(&dash.taper_widths) {
            let low = dash.points_low.first().map_or(0.0, |low| low[0]);
            let x = point[0] + low;
            assert!((width - x * 0.06).abs() < 1e-2, "{width} at x={x}");
            widest = widest.max(*width);
        }
    }
    assert!(widest > 5.5, "widest {widest}");
}

#[test]
fn a_tapered_polyline_restarts_its_pattern_at_each_vertex() {
    let vertices = [
        (0.0, 0.0, 0.0, 6.0),
        (60.0, 0.0, 6.0, 0.0),
        (120.0, 0.0, 0.0, 0.0),
    ];
    let mut document = fence_document();
    let restarts = add(
        &mut document,
        EntityType::LwPolyline(wide_polyline(&vertices, false)),
    );
    let runs_on = add(
        &mut document,
        EntityType::LwPolyline(wide_polyline(&vertices, true)),
    );
    let starts_at_vertex = |wires: &[WireModel]| {
        dash_wires(wires).iter().any(|dash| {
            let low = dash.points_low.first().map_or(0.0, |low| low[0]);
            (dash.points[0][0] + low - 60.0).abs() < 1e-3
        })
    };
    let restarted = wires(&document, restarts);
    assert!(starts_at_vertex(&restarted));
    assert!(!starts_at_vertex(&wires(&document, runs_on)));
    // The band peaks at the vertex on both sides of the restart.
    for dash in dash_wires(&restarted) {
        assert_eq!(dash.taper_widths.len(), dash.points.len());
    }
}

/// A block of `name` holding `polyline` on layer `layer`.
fn polyline_block(
    document: &mut Document,
    name: &str,
    layer: &str,
    linetype: &str,
    mut polyline: LwPolyline,
) {
    let owner = new_block(document, name);
    polyline.common.owner_handle = owner;
    polyline.common.layer = layer.to_string();
    polyline.common.linetype = linetype.to_string();
    document
        .add_entity(EntityType::LwPolyline(polyline))
        .unwrap();
}

#[test]
fn a_wide_polyline_in_a_block_keeps_its_band_and_glyphs() {
    let mut document = fence_document();
    polyline_block(&mut document, "B", "0", "FENCE", band_polyline(3.0));
    let wires = insert_wires(&document, Insert::new("B", Vector3::ZERO), 1.0);
    assert_band(&wires, 3.0);
    assert_same_stations(
        &glyph_stations(&wires, [0.0, 0.0], 8.0),
        &line_stations(100.0, 1.0, 1.0, 8.0),
    );
}

#[test]
fn a_wide_byblock_polyline_keeps_its_band_under_the_inserts_linetype() {
    let mut document = fence_document();
    polyline_block(&mut document, "B", "0", "ByBlock", band_polyline(3.0));
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.linetype = "FENCE".to_string();
    assert_band(&insert_wires(&document, insert, 1.0), 3.0);
}

#[test]
fn a_scaled_insert_scales_the_band_but_not_the_pattern() {
    let mut document = fence_document();
    polyline_block(&mut document, "B", "0", "FENCE", band_polyline(3.0));
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    let wires = insert_wires(&document, insert, 1.0);
    assert_band(&wires, 6.0);
    assert_same_stations(
        &glyph_stations(&wires, [0.0, 0.0], 8.0),
        &line_stations(200.0, 1.0, 1.0, 8.0),
    );
}

#[test]
fn a_tapered_polyline_in_a_block_scales_its_widths() {
    let mut document = fence_document();
    polyline_block(
        &mut document,
        "B",
        "0",
        "FENCE",
        wide_polyline(&[(0.0, 0.0, 0.0, 6.0), (100.0, 0.0, 0.0, 0.0)], true),
    );
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(2.0);
    insert.set_y_scale(2.0);
    let wires = insert_wires(&document, insert, 1.0);
    let dashes = dash_wires(&wires);
    assert!(dashes.len() > 10, "{} dashes", dashes.len());
    let widest = dashes
        .iter()
        .flat_map(|dash| {
            assert_eq!(dash.taper_widths.len(), dash.points.len());
            dash.taper_widths.iter().copied()
        })
        .fold(0.0f32, f32::max);
    assert!(widest > 11.0 && widest <= 12.0 + 1e-3, "widest {widest}");
}
