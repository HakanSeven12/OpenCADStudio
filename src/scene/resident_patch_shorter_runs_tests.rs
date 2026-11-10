//! A layer switched off inside a block shortens the wire runs of the
//! block's references. The resident wire set patches those runs where they
//! are, instead of assembling the whole set again.

use document::entities::{Circle, Insert, Line, Text};
use document::tables::Layer;
use document::types::Vector3;
use document::EntityType;
use glam::{DVec3, Mat4};
use iced::{Point, Rectangle, Size};

use super::*;
use crate::scene::pick::hit_test;

fn on_layer(mut entity: EntityType, layer: &str) -> EntityType {
    entity.common_mut().layer = layer.to_string();
    entity
}

fn circle(radius: f64) -> EntityType {
    EntityType::Circle(Circle::from_center_radius(Vector3::ZERO, radius))
}

fn text(value: &str, y: f64) -> EntityType {
    let mut text = Text::new();
    text.value = value.to_string();
    text.height = 0.3;
    text.insertion_point = Vector3::new(0.0, y, 0.0);
    EntityType::Text(text)
}

/// Block `AB` holds a circle and a text on layer A and the same on layer B.
/// Two references to it come before a line, so neither of their runs is the
/// last one in the set.
fn two_references() -> (Scene, Handle) {
    let mut scene = Scene::new();
    for name in ["A", "B"] {
        scene.document.layers.add(Layer::new(name)).unwrap();
    }
    scene
        .define_block_from_owned_entities(
            vec![
                on_layer(circle(1.0), "A"),
                on_layer(text("a", -1.0), "A"),
                on_layer(circle(2.0), "B"),
                on_layer(text("b", -2.0), "B"),
            ],
            "AB",
            glam::DVec3::ZERO,
        )
        .expect("the definition is created");
    let first = scene.add_entity(EntityType::Insert(Insert::new(
        "AB",
        Vector3::new(50.0, 50.0, 0.0),
    )));
    scene.add_entity(EntityType::Insert(Insert::new(
        "AB",
        Vector3::new(150.0, 50.0, 0.0),
    )));
    scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(300.0, 50.0, 0.0),
        Vector3::new(310.0, 50.0, 0.0),
    )));
    (scene, first)
}

fn switch(scene: &mut Scene, layer: &str, on: bool) {
    let entry = scene
        .document
        .layers
        .get_mut(layer)
        .expect("the layer exists");
    if on {
        entry.turn_on();
    } else {
        entry.turn_off();
    }
    scene.invalidate_layer_visibility(&[layer.to_string()]);
}

/// What the model-space set draws or snaps to, wire by wire in set order:
/// (entity, points, snap points, curves, text vertices). Slots that hold
/// nothing are left out.
type Drawn = Vec<(String, usize, usize, usize, usize)>;

fn drawn(scene: &Scene) -> Drawn {
    let wires = scene.model_tile_wires_arc(0, &Camera::default(), 1.0, 1.0);
    let out = wires
        .iter()
        .filter(|wire| Scene::handle_from_wire_name(&wire.name).is_some())
        .map(|wire| {
            (
                wire.name.clone(),
                wire.points.len(),
                wire.snap_pts.len(),
                wire.tangent_geoms.len(),
                wire.text_verts.len(),
            )
        })
        .filter(|&(_, points, snaps, curves, text)| points + snaps + curves + text > 0)
        .collect();
    // Released, so the next patch can take the wires.
    drop(wires);
    out
}

/// The entities a window from (40, 40) to (60, 60) selects, in a view whose
/// world coordinates are its screen coordinates. It holds the first
/// reference whole.
fn window_around_first(scene: &Scene) -> Vec<Handle> {
    let extent = 400.0_f32;
    let view_rot = Mat4::from_scale(glam::Vec3::new(2.0 / extent, -2.0 / extent, 1.0));
    let eye = DVec3::new(f64::from(extent) / 2.0, f64::from(extent) / 2.0, 0.0);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(extent, extent));
    let wires = scene.model_tile_wires_arc(0, &Camera::default(), 1.0, 1.0);
    let hits = hit_test::box_hit(
        Point::new(40.0, 40.0),
        Point::new(60.0, 60.0),
        false,
        wires.as_slice(),
        view_rot,
        eye,
        bounds,
    )
    .into_iter()
    .filter_map(Scene::handle_from_wire_name)
    .collect();
    drop(wires);
    hits
}

/// The drawn wires without the entity each belongs to, sorted: the same for
/// two sets whichever reference to a block carries its full run.
fn shape(set: &Drawn) -> Vec<(usize, usize, usize, usize)> {
    let mut out: Vec<_> = set
        .iter()
        .map(|&(_, points, snaps, curves, text)| (points, snaps, curves, text))
        .collect();
    out.sort_unstable();
    out
}

fn line_at(layer: &str, y: f64) -> EntityType {
    on_layer(
        EntityType::Line(Line::from_points(
            Vector3::new(-2.0, y, 0.0),
            Vector3::new(2.0, y, 0.0),
        )),
        layer,
    )
}

/// Block `LINES` holds two lines and a circle on layer A and the same on
/// layer B. All its references but the first carry the lines as one stub of
/// the first one's wires. Six references to it come before a line.
fn six_references() -> Scene {
    let mut scene = Scene::new();
    for name in ["A", "B"] {
        scene.document.layers.add(Layer::new(name)).unwrap();
    }
    scene
        .define_block_from_owned_entities(
            vec![
                line_at("A", 0.5),
                line_at("A", 1.0),
                on_layer(circle(1.0), "A"),
                line_at("B", -0.5),
                line_at("B", -1.0),
                on_layer(circle(2.0), "B"),
            ],
            "LINES",
            glam::DVec3::ZERO,
        )
        .expect("the definition is created");
    for k in 0..6 {
        scene.add_entity(EntityType::Insert(Insert::new(
            "LINES",
            Vector3::new(50.0 + 100.0 * k as f64, 50.0, 0.0),
        )));
    }
    scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(900.0, 50.0, 0.0),
        Vector3::new(910.0, 50.0, 0.0),
    )));
    scene
}

#[test]
fn references_drawn_as_stubs_are_patched_when_a_layer_inside_switches() {
    let mut scene = six_references();
    let before = drawn(&scene);

    let hits = scene.resident_patch_hits.get();
    switch(&mut scene, "A", false);
    let off = drawn(&scene);
    assert_eq!(
        scene.resident_patch_hits.get(),
        hits + 1,
        "the reference that carried the full run must get it back, so no \
         reference grows past its slots and the set is patched"
    );
    let mut fresh = six_references();
    switch(&mut fresh, "A", false);
    assert_eq!(
        shape(&off),
        shape(&drawn(&fresh)),
        "the patched set draws what a new one does"
    );

    let hits = scene.resident_patch_hits.get();
    switch(&mut scene, "A", true);
    let on = drawn(&scene);
    assert_eq!(
        scene.resident_patch_hits.get(),
        hits + 1,
        "switching A back on patches the set again"
    );
    assert_eq!(
        shape(&on),
        shape(&before),
        "A back on draws what it drew before"
    );
}

#[test]
fn a_layer_switched_off_inside_a_block_patches_the_references_in_place() {
    let (mut scene, first) = two_references();
    let before = drawn(&scene);
    assert_eq!(
        window_around_first(&scene),
        vec![first],
        "the window holds the first reference, or this proves nothing"
    );
    let name = first.value().to_string();
    let count = |set: &Drawn| set.iter().filter(|wire| wire.0 == name).count();

    let hits = scene.resident_patch_hits.get();
    switch(&mut scene, "A", false);
    let off = drawn(&scene);
    assert!(
        count(&off) > 0 && count(&off) < count(&before),
        "switching A off must shorten the first reference's run, or this \
         proves nothing: {} -> {} wires",
        count(&before),
        count(&off)
    );
    assert_eq!(
        scene.resident_patch_hits.get(),
        hits + 1,
        "the shortened runs are not the last ones in the set: they must be \
         patched where they are, not make the whole set be assembled again"
    );
    let (mut fresh, _) = two_references();
    switch(&mut fresh, "A", false);
    assert_eq!(
        off,
        drawn(&fresh),
        "the patched set draws what a new one does"
    );
    assert_eq!(
        window_around_first(&scene),
        vec![first],
        "the slots a shortened run keeps must not stop a window from \
         selecting its entity"
    );

    let hits = scene.resident_patch_hits.get();
    switch(&mut scene, "A", true);
    let on = drawn(&scene);
    assert_eq!(
        scene.resident_patch_hits.get(),
        hits + 1,
        "switching A back on refills the slots the runs kept"
    );
    assert_eq!(on, before, "A back on draws what it drew before");
}
