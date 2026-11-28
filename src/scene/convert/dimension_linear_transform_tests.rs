// Tests of how a transformed linear dimension is drawn
// (`entities::dimension::linear_transform`).
use document::entities::{Dimension, DimensionAligned, DimensionLinear};
use document::types::Vector3;
use document::{Document, Handle};
use glam::DVec3;

use crate::entities::dimension::*;
use crate::entities::transform::EntityTransform;
use crate::scene::convert::dimension_tess::DimensionTess;

use std::f64::consts::FRAC_PI_2;

/// A horizontal linear dimension measuring 10 units along X.
fn horizontal() -> Dimension {
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 5.0, 0.0);
    Dimension::Linear(d)
}

/// Every stroke point the dimension draws, with its low residual added.
fn stroke_points(dim: &Dimension) -> Vec<DVec3> {
    let document = Document::new();
    let wires = dim.tessellate(
        &document,
        Handle::new(0x40),
        false,
        [1.0; 4],
        1.0,
        1.0,
        &rustc_hash::FxHashSet::default(),
        None,
        [0.0, 0.0, 0.0, 1.0],
        None,
        None,
        &rustc_hash::FxHashSet::default(),
    );
    let mut points = Vec::new();
    for wire in &wires {
        for (index, high) in wire.points.iter().enumerate() {
            if !high[0].is_finite() {
                continue;
            }
            let low = wire.points_low.get(index).unwrap_or([0.0; 3]);
            points.push(DVec3::new(
                high[0] as f64 + low[0] as f64,
                high[1] as f64 + low[1] as f64,
                high[2] as f64 + low[2] as f64,
            ));
        }
    }
    points
}

/// Turned 90° about X into the XZ plane, the dimension has to draw the
/// same strokes turned with it.
fn assert_draws_turned_with_its_plane(flat: Dimension) {
    let turn = glam::DQuat::from_axis_angle(DVec3::X, FRAC_PI_2);
    let expected: Vec<DVec3> = stroke_points(&flat).into_iter().map(|p| turn * p).collect();
    assert!(!expected.is_empty(), "the dimension draws no strokes");
    let mut turned = flat;
    apply_transform(
        &mut turned,
        &EntityTransform::Rotate {
            center: DVec3::ZERO,
            axis: DVec3::X,
            angle_rad: FRAC_PI_2,
        },
    );
    let drawn = stroke_points(&turned);
    assert_eq!(drawn.len(), expected.len(), "stroke point count");
    for (index, (got, want)) in drawn.iter().zip(&expected).enumerate() {
        assert!(
            got.abs_diff_eq(*want, 1e-3),
            "point {index}: expected {want:?}, got {got:?}"
        );
    }
}

/// A linear dimension in a working plane off world Z draws in that plane.
/// Its offset used to be read in world XY only, so once it ran along Z the
/// dimension line fell onto the measured points and the extension lines
/// stuck out along world Y.
#[test]
fn a_linear_dimension_off_world_z_draws_in_its_own_plane() {
    assert_draws_turned_with_its_plane(horizontal());
}

/// An aligned dimension draws through the same builder and plane.
#[test]
fn an_aligned_dimension_off_world_z_draws_in_its_own_plane() {
    let mut d = DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(6.0, 8.0, 0.0));
    d.definition_point = Vector3::new(2.0, 11.0, 0.0);
    assert_draws_turned_with_its_plane(Dimension::Aligned(d));
}
