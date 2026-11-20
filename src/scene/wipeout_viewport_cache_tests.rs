use super::*;

use document::entities::{Line, Wipeout};
use document::types::{Vector2, Vector3};

fn mask() -> EntityType {
    EntityType::Wipeout(Wipeout::polygonal(
        &[
            Vector2::new(-1.0, -1.0),
            Vector2::new(1.0, -1.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(-1.0, 1.0),
        ],
        0.5,
    ))
}

fn plain_line(x: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(x, 0.0, 0.0),
        Vector3::new(x + 1.0, 1.0, 0.0),
    ))
}

/// A frozen-layer set is what takes the per-viewport path: with a null
/// viewport and no frozen layers the function answers from the model-space
/// cache instead, which is not the one under test.
fn frozen() -> HashSet<Handle> {
    let mut set: HashSet<Handle> = HashSet::default();
    set.insert(Handle::new(41));
    set
}

#[test]
fn moving_plain_geometry_keeps_the_viewport_masks() {
    let mut scene = Scene::new();
    scene.add_entity(mask());
    let frozen = frozen();
    let first = scene.wipeout_models_for_viewport(Handle::NULL, &frozen);
    assert_eq!(first.len(), 1, "the mask belongs in the built set");

    // A line is neither a WIPEOUT nor an INSERT, so it cannot have placed
    // or moved a mask: the set cached before it is still the right answer.
    scene.add_entity(plain_line(10.0));
    let second = scene.wipeout_models_for_viewport(Handle::NULL, &frozen);
    assert!(
        Arc::ptr_eq(&first, &second),
        "a plain-geometry edit rebuilt every mask in the drawing — 96 ms a \
             frame, 153 frames of one measured drag — because this cache \
             compared geometry_epoch outright instead of asking which \
             category changed"
    );
}

#[test]
fn adding_a_mask_still_rebuilds_the_viewport_masks() {
    let mut scene = Scene::new();
    scene.add_entity(mask());
    let frozen = frozen();
    let first = scene.wipeout_models_for_viewport(Handle::NULL, &frozen);

    scene.add_entity(mask());
    let second = scene.wipeout_models_for_viewport(Handle::NULL, &frozen);
    assert!(
        !Arc::ptr_eq(&first, &second),
        "holding the cache through a WIPEOUT edit would draw a stale mask"
    );
    assert_eq!(second.len(), 2, "both masks belong in the rebuilt set");
}
