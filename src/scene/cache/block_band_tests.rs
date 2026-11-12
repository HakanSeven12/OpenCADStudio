//! A wide polyline inside a block is drawn as the insert's image of its band:
//! a scale on one axis only stretches the band along that axis, so each
//! segment's width is the scale across it, not a mean of the two.

use std::collections::HashSet;

use document::entities::{Insert, LwPolyline, LwVertex};
use document::tables::BlockRecord;
use document::types::{Vector2, Vector3};
use document::{Document, EntityType};

use crate::scene::model::wire_model::WireModel;

/// A document whose block "B" holds one polyline of `width` through `points`.
fn block_with_band(points: &[(f64, f64)], width: f64, closed: bool, plinegen: bool) -> Document {
    let mut document = Document::new();
    let mut block = BlockRecord::new("B");
    block.handle = document.allocate_handle();
    let owner = block.handle;
    document.block_records.add(block).unwrap();
    let mut polyline = LwPolyline::new();
    polyline.vertices = points
        .iter()
        .map(|&(x, y)| LwVertex::new(Vector2::new(x, y)))
        .collect();
    polyline.constant_width = width;
    polyline.is_closed = closed;
    polyline.plinegen = plinegen;
    polyline.common.owner_handle = owner;
    document
        .add_entity(EntityType::LwPolyline(polyline))
        .unwrap();
    document
}

fn insert(x_scale: f64, y_scale: f64) -> Insert {
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.set_x_scale(x_scale);
    insert.set_y_scale(y_scale);
    insert
}

fn wires(document: &Document, insert: Insert) -> Vec<WireModel> {
    crate::scene::tessellate_entity(
        document,
        &HashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
        None,
        &EntityType::Insert(insert),
        None,
        None,
        None,
        None,
        &HashSet::default(),
    )
}

fn point(wire: &WireModel, index: usize) -> Option<[f64; 2]> {
    let high = wire.points[index];
    let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
    high[0]
        .is_finite()
        .then(|| [high[0] as f64 + low[0] as f64, high[1] as f64 + low[1] as f64])
}

fn width(wire: &WireModel, index: usize) -> f64 {
    wire.taper_widths
        .get(index)
        .copied()
        .unwrap_or(wire.world_width) as f64
}

/// Every drawn band segment: its two ends and its full width at each.
fn band_segments(wires: &[WireModel]) -> Vec<([f64; 2], [f64; 2], f64, f64)> {
    let mut segments = Vec::new();
    let bands = wires
        .iter()
        .filter(|wire| wire.display_visible && !wire.snap_only && wire.world_width > 0.0);
    for wire in bands {
        for index in 0..wire.points.len().saturating_sub(1) {
            if let (Some(a), Some(b)) = (point(wire, index), point(wire, index + 1)) {
                segments.push((a, b, width(wire, index), width(wire, index + 1)));
            }
        }
    }
    segments
}

/// Whether the drawn band (segment quads and fills) covers `p`.
fn covered(wires: &[WireModel], p: [f64; 2]) -> bool {
    let on_segment = band_segments(wires).into_iter().any(|(a, b, wa, wb)| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = (dx * dx + dy * dy).sqrt();
        if length < 1e-12 {
            return false;
        }
        let along = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length;
        let across = ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / length;
        let t = along / length;
        (0.0..=1.0).contains(&t) && across <= 0.5 * (wa + (wb - wa) * t)
    });
    let in_fill = wires.iter().filter(|wire| wire.display_visible).any(|wire| {
        wire.fill_tris.chunks_exact(3).enumerate().any(|(triangle, corners)| {
            let at = |k: usize| {
                let low = wire
                    .fill_tris_low
                    .get(triangle * 3 + k)
                    .copied()
                    .unwrap_or([0.0; 3]);
                [
                    corners[k][0] as f64 + low[0] as f64,
                    corners[k][1] as f64 + low[1] as f64,
                ]
            };
            let (a, b, c) = (at(0), at(1), at(2));
            let side = |u: [f64; 2], v: [f64; 2]| {
                (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
            };
            let (s0, s1, s2) = (side(a, b), side(b, c), side(c, a));
            (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0)
        })
    });
    on_segment || in_fill
}

#[test]
fn a_wide_line_keeps_its_width_when_the_insert_stretches_it_lengthwise() {
    // The block of #2878: a 1-long line 0.5 wide, inserted 100.6 times longer.
    let document = block_with_band(&[(0.0, 0.0), (1.0, 0.0)], 0.5, false, false);
    let segments = band_segments(&wires(&document, insert(100.603407, 1.0)));
    assert!(!segments.is_empty());
    for (a, b, wa, wb) in segments {
        assert!((b[0] - a[0] - 100.603407).abs() < 1e-6, "{a:?} {b:?}");
        assert!((wa - 0.5).abs() < 1e-5 && (wb - 0.5).abs() < 1e-5, "{wa} {wb}");
    }
}

#[test]
fn a_wide_line_takes_the_scale_across_it() {
    let document = block_with_band(&[(0.0, 0.0), (1.0, 0.0)], 0.5, false, false);
    let segments = band_segments(&wires(&document, insert(1.0, 4.0)));
    assert!(!segments.is_empty());
    for (_, _, wa, wb) in segments {
        assert!((wa - 2.0).abs() < 1e-5 && (wb - 2.0).abs() < 1e-5, "{wa} {wb}");
    }
}

#[test]
fn a_uniform_insert_scales_the_band_by_its_scale() {
    let document = block_with_band(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], 0.5, false, true);
    let segments = band_segments(&wires(&document, insert(3.0, 3.0)));
    assert!(!segments.is_empty());
    for (_, _, wa, wb) in segments {
        assert!((wa - 1.5).abs() < 1e-5 && (wb - 1.5).abs() < 1e-5, "{wa} {wb}");
    }
}

#[test]
fn a_wide_frame_stretched_on_one_axis_is_the_image_of_its_band() {
    // A 10 × 10 frame 1 wide, three times wider in X: its band runs from
    // (-1.5, -0.5) to (31.5, 10.5) outside and (1.5, 0.5) to (28.5, 9.5) inside.
    let frame = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let inside = [
        [15.0, 0.4],
        [15.0, -0.4],
        [15.0, 10.4],
        [-1.4, 5.0],
        [1.4, 5.0],
        [31.4, 5.0],
        // The corners are closed out to the corners of the image.
        [-1.4, -0.4],
        [31.4, -0.4],
        [31.4, 10.4],
        [-1.4, 10.4],
    ];
    let outside = [
        [15.0, 0.6],
        [15.0, -0.6],
        [-1.6, 5.0],
        [1.6, 5.0],
        [-1.6, -0.4],
        [-1.4, -0.6],
        [31.6, 10.4],
        [31.4, 10.6],
    ];
    for plinegen in [false, true] {
        let document = block_with_band(&frame, 1.0, true, plinegen);
        let drawn = wires(&document, insert(3.0, 1.0));
        for p in inside {
            assert!(covered(&drawn, p), "PLINEGEN {plinegen}: {p:?} left open");
        }
        for p in outside {
            assert!(!covered(&drawn, p), "PLINEGEN {plinegen}: {p:?} drawn");
        }
    }
}
