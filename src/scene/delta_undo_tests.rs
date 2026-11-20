use super::*;

use crate::entities::transform::EntityTransform;
use document::entities::Line;
use document::types::Vector3;
use glam::DVec3;

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(x1, y1, 0.0),
        Vector3::new(x2, y2, 0.0),
    ))
}

/// Build the delta the way `commit_undo_delta` does: pair each recorded
/// before-image with the entity's current (after) state.
fn build_delta(
    scene: &Scene,
    rec: UndoRecording,
) -> Vec<(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)> {
    rec.into_before_images()
        .into_iter()
        .map(|(h, before)| {
            let after = scene.document.get_entity_arc(h);
            (h, before, after)
        })
        .collect()
}

/// Occurrences of a handle in the *Model_Space block record's entity list.
fn ms_occurrences(scene: &Scene, handle: Handle) -> usize {
    scene
        .document
        .block_records
        .get("*Model_Space")
        .map(|br| br.entity_handles.iter().filter(|&&x| x == handle).count())
        .unwrap_or(0)
}

#[test]
fn transform_delta_round_trips() {
    let mut scene = Scene::new();
    let h = scene.add_entity(line(0.0, 0.0, 10.0, 0.0));
    let orig = scene.document.get_entity(h).cloned().unwrap();

    scene.begin_undo_recording();
    scene.transform_entities(&[h], &EntityTransform::Translate(DVec3::new(5.0, 3.0, 0.0)));
    let rec = scene.take_undo_recording().unwrap();
    assert!(!rec.is_poisoned(), "a plain move must not poison");
    let delta = build_delta(&scene, rec);
    let moved = scene.document.get_entity(h).cloned().unwrap();
    assert_ne!(orig, moved, "the move must actually change the entity");

    // Undo restores the pre-move image exactly, in place (same handle).
    scene.apply_entity_delta(&delta, true);
    assert_eq!(scene.document.get_entity(h).cloned().unwrap(), orig);
    assert_eq!(ms_occurrences(&scene, h), 1);

    // Redo re-applies the post-move image.
    scene.apply_entity_delta(&delta, false);
    assert_eq!(scene.document.get_entity(h).cloned().unwrap(), moved);
    assert_eq!(ms_occurrences(&scene, h), 1);
}

#[test]
fn erase_delta_round_trips_with_handle_and_no_dup() {
    let mut scene = Scene::new();
    let h1 = scene.add_entity(line(0.0, 0.0, 1.0, 0.0));
    let h2 = scene.add_entity(line(2.0, 2.0, 3.0, 3.0));
    let orig1 = scene.document.get_entity(h1).cloned().unwrap();
    let orig2 = scene.document.get_entity(h2).cloned().unwrap();
    let count = scene.document.entities().count();
    scene.select_entity(h2, false);
    scene.set_hover_highlight(Some(h2));
    let selection_generation = scene.selection_generation;

    scene.begin_undo_recording();
    scene.erase_entities(&[h2]);
    let rec = scene.take_undo_recording().unwrap();
    assert!(
        !rec.is_poisoned(),
        "erasing an ungrouped entity must not poison"
    );
    let delta = build_delta(&scene, rec);
    assert!(scene.document.get_entity(h2).is_none(), "h2 must be erased");
    assert!(!scene.selected.contains(&h2));
    assert_eq!(scene.hover_highlight, None);
    assert_ne!(
        scene.selection_generation, selection_generation,
        "erase must invalidate the stale selection overlay"
    );

    // Undo re-inserts h2 with its ORIGINAL handle and image, exactly once in
    // the block record (the strip prevents a duplicated dangling entry),
    // leaving h1 untouched.
    scene.apply_entity_delta(&delta, true);
    assert_eq!(scene.document.get_entity(h2).cloned().unwrap(), orig2);
    assert_eq!(scene.document.get_entity(h1).cloned().unwrap(), orig1);
    assert_eq!(scene.document.entities().count(), count);
    assert_eq!(
        ms_occurrences(&scene, h2),
        1,
        "h2 must appear once, not duplicated"
    );

    // Redo erases h2 again. (remove_entity leaves the handle dangling in
    // the block record — harmless, a lookup miss is skipped on render/save
    // — so we assert absence from the document, not from entity_handles.)
    scene.apply_entity_delta(&delta, false);
    assert!(scene.document.get_entity(h2).is_none());
}

#[test]
fn add_delta_round_trips() {
    let mut scene = Scene::new();
    scene.begin_undo_recording();
    let h = scene.add_entity(line(0.0, 0.0, 4.0, 4.0));
    let rec = scene.take_undo_recording().unwrap();
    assert!(
        !rec.is_poisoned(),
        "a plain add on an existing layer must not poison"
    );
    let added = scene.document.get_entity(h).cloned().unwrap();
    let delta = build_delta(&scene, rec);

    // Undo removes the freshly added entity from the document (a dangling
    // block-record entry may remain — see erase test — that's fine).
    scene.apply_entity_delta(&delta, true);
    assert!(scene.document.get_entity(h).is_none());

    // Redo re-adds it with the same handle, exactly once (the strip clears
    // any dangling entry so add_entity doesn't duplicate it).
    scene.apply_entity_delta(&delta, false);
    assert_eq!(scene.document.get_entity(h).cloned().unwrap(), added);
    assert_eq!(ms_occurrences(&scene, h), 1);
}

#[test]
fn raster_add_records_only_its_image_definition_object() {
    use document::entities::RasterImage;
    use document::objects::ObjectType;
    use document::types::Vector3;

    let mut scene = Scene::new();
    let image = RasterImage::with_size(
        "test.png",
        Vector3::new(0.0, 0.0, 0.0),
        16.0,
        16.0,
        10.0,
        10.0,
    );
    scene.begin_undo_recording();
    let handle = scene.add_entity(EntityType::RasterImage(image));
    let rec = scene.take_undo_recording().unwrap();
    assert!(!rec.is_poisoned());
    let (entities, objects, _parametric_constraints, _named_parameters, _designations) =
        rec.into_recorded_images();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].0, handle);
    // The image's own graph: ACAD_IMAGE_DICT, the definition and the image's
    // reactor are new; the named object dictionary gains the entry.
    assert_eq!(objects.len(), 4);
    let root = scene.document.header.named_objects_dict_handle;
    for (h, before) in &objects {
        assert_eq!(before.is_some(), *h == root, "{h:?}");
    }
    assert!(objects.iter().any(|(h, _)| matches!(
        scene.document.objects.get(h),
        Some(ObjectType::ImageDefinition(_))
    )));
}

#[test]
fn grouped_erase_records_group_object_without_poisoning() {
    use document::objects::ObjectType;

    let mut scene = Scene::new();
    let h1 = scene.add_entity(line(0.0, 0.0, 1.0, 0.0));
    let h2 = scene.add_entity(line(2.0, 0.0, 3.0, 0.0));
    let group = scene.create_group("G".to_string(), vec![h1, h2]);

    scene.begin_undo_recording();
    scene.erase_entities(&[h1]);
    let rec = scene.take_undo_recording().unwrap();
    assert!(!rec.is_poisoned());
    let (entities, objects, _parametric_constraints, _named_parameters, _designations) =
        rec.into_recorded_images();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].0, h1);
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].0, group);
    assert!(matches!(objects[0].1, Some(ObjectType::Group(_))));
    assert!(matches!(
        scene.document.objects.get(&group),
        Some(ObjectType::Group(current)) if current.entities == vec![h2]
    ));
}

#[test]
fn copy_delta_round_trips() {
    let mut scene = Scene::new();
    let src = scene.add_entity(line(0.0, 0.0, 1.0, 0.0));

    scene.begin_undo_recording();
    let new_handles = scene.copy_entities(
        &[src],
        &EntityTransform::Translate(DVec3::new(10.0, 0.0, 0.0)),
    );
    let rec = scene.take_undo_recording().unwrap();
    assert!(!rec.is_poisoned(), "copying a plain entity must not poison");
    assert_eq!(new_handles.len(), 1);
    let copy_h = new_handles[0];
    let copy_img = scene.document.get_entity(copy_h).cloned().unwrap();
    let delta = build_delta(&scene, rec);

    // Undo erases the copy from the document, leaving the source intact.
    scene.apply_entity_delta(&delta, true);
    assert!(scene.document.get_entity(copy_h).is_none());
    assert!(scene.document.get_entity(src).is_some());

    // Redo restores the copy with its handle, once.
    scene.apply_entity_delta(&delta, false);
    assert_eq!(
        scene.document.get_entity(copy_h).cloned().unwrap(),
        copy_img
    );
    assert_eq!(ms_occurrences(&scene, copy_h), 1);
}
