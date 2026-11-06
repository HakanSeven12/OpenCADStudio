//! Pick-side tests for the reference-only designation (API v9): clicks pass
//! through reference-only wires and box selections skip them.

use crate::scene::model::wire_model::WireModel;
use crate::scene::pick::hit_test;
use crate::scene::Scene;
use document::Handle;
use glam::{DVec3, Mat4};
use iced::{Point, Rectangle, Size};

/// A view whose world coordinates map one-to-one onto screen coordinates
/// (the convention of `block_base_point_tests::WorldAsScreen`).
struct WorldAsScreen {
    view_rot: Mat4,
    eye: DVec3,
    bounds: Rectangle,
}

impl WorldAsScreen {
    fn new(extent: f32) -> Self {
        Self {
            view_rot: Mat4::from_scale(glam::Vec3::new(2.0 / extent, -2.0 / extent, 1.0)),
            eye: DVec3::new(extent as f64 / 2.0, extent as f64 / 2.0, 0.0),
            bounds: Rectangle::new(Point::ORIGIN, Size::new(extent, extent)),
        }
    }

    fn at(&self, point: [f64; 3]) -> Point {
        Point::new(point[0] as f32, point[1] as f32)
    }
}

fn segment_wire(name: &str, from: [f64; 3], to: [f64; 3], reference_only: bool) -> WireModel {
    WireModel {
        name: name.to_owned(),
        points: vec![
            [from[0] as f32, from[1] as f32, from[2] as f32],
            [to[0] as f32, to[1] as f32, to[2] as f32],
        ],
        reference_only,
        ..WireModel::default()
    }
}

fn click_at(point: [f64; 3], view: &WorldAsScreen, wires: &[WireModel]) -> Option<Handle> {
    hit_test::click_hit(
        view.at(point),
        wires,
        view.view_rot,
        view.eye,
        view.bounds,
        false,
        8.0,
        &rustc_hash::FxHashMap::default(),
    )
    .and_then(Scene::handle_from_wire_name)
}

#[test]
fn clicks_pass_through_reference_only_wires() {
    let view = WorldAsScreen::new(100.0);
    let editable = segment_wire("1", [25.0, 25.0, 0.0], [75.0, 25.0, 0.0], false);
    let reference = segment_wire("2", [25.0, 30.0, 0.0], [75.0, 30.0, 0.0], true);
    let wires = [editable, reference];

    // A click sitting exactly on the reference-only wire passes through to
    // the editable wire 5 px away (inside the 8 px aperture).
    assert_eq!(
        click_at([50.0, 30.0, 0.0], &view, &wires),
        Some(Handle::new(1)),
        "a click on reference-only geometry must resolve to the entity underneath"
    );
    assert_eq!(
        click_at([50.0, 25.0, 0.0], &view, &wires),
        Some(Handle::new(1))
    );

    // With only the reference-only wire under the cursor the click misses.
    let reference_only = [wires[1].clone()];
    assert_eq!(click_at([50.0, 30.0, 0.0], &view, &reference_only), None);
    assert_eq!(click_at([50.0, 25.0, 0.0], &view, &reference_only), None);
}

#[test]
fn box_selections_skip_reference_only_wires() {
    let view = WorldAsScreen::new(100.0);
    let editable = segment_wire("1", [25.0, 25.0, 0.0], [75.0, 25.0, 0.0], false);
    let reference = segment_wire("2", [25.0, 30.0, 0.0], [75.0, 30.0, 0.0], true);
    let wires = [editable, reference];

    let hits = hit_test::box_hit(
        view.at([10.0, 10.0, 0.0]),
        view.at([90.0, 50.0, 0.0]),
        false,
        &wires,
        view.view_rot,
        view.eye,
        view.bounds,
    );
    let handles: Vec<Handle> = hits
        .into_iter()
        .filter_map(Scene::handle_from_wire_name)
        .collect();
    assert_eq!(
        handles,
        vec![Handle::new(1)],
        "a box selection must not pick reference-only geometry"
    );
}
