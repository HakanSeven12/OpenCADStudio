//! A complex linetype walks the whole entity, each run end-aligned like
//! the "A" alignment: it begins and ends with a dash, its glyphs sit
//! symmetrically, and a run shorter than one pattern is drawn solid.

use std::collections::HashSet;

use codec::entities::{Line, LwPolyline, LwVertex};
use codec::tables::linetype::{LineTypeComplexContent, LineTypeComplexData, LineTypeElement};
use codec::tables::{LineType, TextStyle};
use codec::types::{Vector2, Vector3};
use codec::{Document, EntityType, Handle};

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
    let entity = document.get_entity(handle).expect("entity").clone();
    crate::scene::tessellate_entity(
        document,
        &HashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
        None,
        &entity,
        None,
        None,
        None,
        false,
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
    let (_, align_end, align_total) = crate::scene::pipeline::wire_gpu::wire_distances(&wire);
    assert!((align_total - 2.3).abs() < 1e-4, "{align_total}");
    assert!((align_end - 0.275).abs() < 1e-4, "{align_end}");
}
