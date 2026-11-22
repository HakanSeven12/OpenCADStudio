//! Several entities added between two draws (COPY, PASTE, ARRAY) go into the
//! resident wire set by a patch, instead of making the whole set be
//! assembled again.

use document::entities::{Circle, Line};
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;

use super::*;
use crate::entities::transform::EntityTransform;

fn line(i: usize) -> EntityType {
    let x = i as f64 * 10.0;
    EntityType::Line(Line::from_points(
        Vector3::new(x, 0.0, 0.0),
        Vector3::new(x + 5.0, 5.0, 0.0),
    ))
}

fn circle(i: usize) -> EntityType {
    EntityType::Circle(Circle::from_center_radius(
        Vector3::new(i as f64 * 10.0, 20.0, 0.0),
        2.0,
    ))
}

/// What the model-space set draws, wire by wire in set order.
fn drawn(scene: &Scene) -> Vec<(String, usize, usize)> {
    let wires = scene.model_tile_wires_arc(0, &Camera::default(), 1.0, 1.0);
    let out = wires
        .iter()
        .filter(|wire| Scene::handle_from_wire_name(&wire.name).is_some())
        .map(|wire| {
            (
                wire.name.clone(),
                wire.points.len(),
                wire.tangent_geoms.len(),
            )
        })
        .collect();
    // Released, so the next patch can take the wires.
    drop(wires);
    out
}

/// A drawing of `base` lines, drawn one time, then `added` more entities.
/// `draw_first` is false for the comparison: the same drawing with no set to
/// patch, so its first draw assembles the whole set.
fn with_added(base: usize, added: usize, draw_first: bool) -> Scene {
    let mut scene = Scene::new();
    for i in 0..base {
        scene.add_entity(line(i));
    }
    if draw_first {
        assert_eq!(drawn(&scene).len(), base);
    }
    for i in 0..added {
        scene.add_entity(if i % 2 == 0 {
            line(base + i)
        } else {
            circle(base + i)
        });
    }
    scene
}

fn adds_are_patched(base: usize, added: usize) {
    let scene = with_added(base, added, true);
    let hits = scene.resident_patch_hits.get();
    let patched = drawn(&scene);
    assert_eq!(
        scene.resident_patch_hits.get(),
        hits + 1,
        "{added} entities added after a draw are patched into the set"
    );
    assert_eq!(patched.len(), base + added);
    assert_eq!(
        patched,
        drawn(&with_added(base, added, false)),
        "the patched set draws what a newly built set draws, in the same order"
    );
}

#[test]
fn one_added_entity_is_patched_into_the_set() {
    adds_are_patched(20, 1);
}

#[test]
fn two_added_entities_are_patched_into_the_set() {
    adds_are_patched(20, 2);
}

#[test]
fn a_hundred_added_entities_are_patched_into_the_set() {
    adds_are_patched(20, 100);
}

#[test]
fn entities_added_to_an_empty_drawing_are_patched_into_the_set() {
    adds_are_patched(0, 50);
}

/// An added entity that another change then removes was never in the set.
/// The comparison is by content: a newly built set lists the entities in the
/// drawing's storage order, where the last entity takes an erased one's
/// place, and the patched set keeps the order they were added in.
#[test]
fn added_entities_with_one_erased_again_are_patched_into_the_set() {
    let build = |draw_first: bool| {
        let mut scene = with_added(20, 0, draw_first);
        let handles: Vec<_> = (0..30).map(|i| scene.add_entity(line(100 + i))).collect();
        scene.erase_entities(&[handles[7]]);
        scene
    };
    let scene = build(true);
    let hits = scene.resident_patch_hits.get();
    let patched = drawn(&scene);
    assert_eq!(scene.resident_patch_hits.get(), hits + 1);
    assert_eq!(patched.len(), 49);
    let sorted = |mut set: Vec<(String, usize, usize)>| {
        set.sort();
        set
    };
    assert_eq!(sorted(patched), sorted(drawn(&build(false))));
}

/// A copy of entities with one of the old ones moved in the same step: the
/// moved one keeps its place and the new ones go in behind the others.
#[test]
fn added_entities_and_a_moved_one_are_patched_into_the_set() {
    let build = |draw_first: bool| {
        let mut scene = Scene::new();
        let old: Vec<_> = (0..20).map(|i| scene.add_entity(line(i))).collect();
        if draw_first {
            assert_eq!(drawn(&scene).len(), 20);
        }
        for i in 0..40 {
            scene.add_entity(circle(100 + i));
        }
        scene.transform_entities(
            &[old[3]],
            &EntityTransform::Translate(DVec3::new(0.0, 50.0, 0.0)),
        );
        scene
    };
    let scene = build(true);
    let hits = scene.resident_patch_hits.get();
    let patched = drawn(&scene);
    assert_eq!(scene.resident_patch_hits.get(), hits + 1);
    assert_eq!(patched.len(), 60);
    assert_eq!(patched, drawn(&build(false)));
}

/// The patched set stays right over several rounds of adds.
#[test]
fn entities_added_in_several_rounds_are_patched_each_time() {
    let mut scene = with_added(10, 0, true);
    let mut reference = with_added(10, 0, false);
    for round in 0..4 {
        for i in 0..25 {
            scene.add_entity(line(1000 + round * 100 + i));
            reference.add_entity(line(1000 + round * 100 + i));
        }
        let hits = scene.resident_patch_hits.get();
        let patched = drawn(&scene);
        assert_eq!(scene.resident_patch_hits.get(), hits + 1, "round {round}");
        assert_eq!(patched.len(), 10 + (round + 1) * 25);
    }
    assert_eq!(drawn(&scene), drawn(&reference));
}

/// Erasing most of a drawing has the set built again without the erased
/// entities, so undoing the erase brings them back as new ones, between the
/// entities that stayed. New runs only go in at the tail, and the patch
/// declines. It has to tell before it tessellates them all, which the full
/// build then does again: undoing an erase of 100,000 objects spent half a
/// second on that.
#[test]
fn a_patch_that_cannot_place_its_new_entities_declines_before_tessellating_them() {
    let mut scene = Scene::new();
    let handles: Vec<Handle> = (0..400).map(|i| scene.add_entity(line(i))).collect();
    let whole = drawn(&scene);
    assert_eq!(whole.len(), 400);

    // All but every tenth entity go: more than half the set, so it is built
    // again with the forty that stay.
    let erased: Vec<Handle> = handles
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 10 != 0)
        .map(|(_, handle)| *handle)
        .collect();
    let images: Vec<_> = erased
        .iter()
        .map(|handle| scene.document.get_entity_arc(*handle).unwrap())
        .collect();
    scene.erase_entities(&erased);
    assert_eq!(drawn(&scene).len(), 40);

    assert_eq!(scene.restore_erased_entities(images).len(), 360);
    let hits = scene.resident_patch_hits.get();
    let tessellated = scene.resident_patch_tessellated.get();
    let restored = drawn(&scene);
    assert_eq!(
        scene.resident_patch_hits.get(),
        hits,
        "entities that come back between the ones that stayed are not patched in"
    );
    let cost = scene.resident_patch_tessellated.get() - tessellated;
    assert!(
        cost <= 4,
        "the declined patch tessellated {cost} of the 360 entities before the full build"
    );
    // By content: a newly built set lists the entities in the drawing's
    // storage order, which the erase and its undo changed.
    let sorted = |mut set: Vec<(String, usize, usize)>| {
        set.sort();
        set
    };
    assert_eq!(
        sorted(restored),
        sorted(whole),
        "the set draws the whole drawing again"
    );
}
