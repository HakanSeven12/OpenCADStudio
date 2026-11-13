//! An entity edited after its drawing was opened from DWG is saved as edited.
//!
//! An entity read from DWG keeps the record it was read from, and the writer
//! copies that record instead of encoding the entity again. A command that
//! edits a copy of the entity and writes the copy back must not hand the
//! record of the unedited entity to the writer.

use crate::scene::Scene;
use document::entities::Line;
use document::types::Vector3;
use document::{EntityType, Handle};

/// A scene on a drawing with one line from (0, 0) to (10, 0), saved to DWG
/// and opened again, with the line's handle.
fn scene_opened_from_dwg() -> (Scene, Handle) {
    let mut drawn = Scene::new();
    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(10.0, 0.0, 0.0);
    let handle = drawn.document.add_entity(EntityType::Line(line)).unwrap();
    let mut scene = Scene::new();
    scene.document = reopened_as_dwg(&drawn);
    (scene, handle)
}

/// The scene's drawing saved to DWG and opened the way the application opens
/// a file.
fn reopened_as_dwg(scene: &Scene) -> document::Document {
    let bytes = crate::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
        .expect("save as DWG");
    crate::io::load_bytes_finalized(std::path::Path::new("edited.dwg"), bytes)
        .expect("open the saved DWG")
}

fn line_end(document: &document::Document, handle: Handle) -> Vector3 {
    match document.get_entity(handle) {
        Some(EntityType::Line(line)) => line.end,
        other => panic!("expected the line, got {other:?}"),
    }
}

/// The line as a command holds it: a copy of the stored entity, with its end
/// moved to (25, 0).
fn lengthened_copy(scene: &Scene, handle: Handle) -> EntityType {
    let mut entity = scene.document.get_entity(handle).unwrap().clone();
    let EntityType::Line(line) = &mut entity else {
        panic!("expected the line")
    };
    line.end = Vector3::new(25.0, 0.0, 0.0);
    entity
}

#[test]
fn an_edit_written_back_as_a_copy_is_saved_to_dwg() {
    let (mut scene, handle) = scene_opened_from_dwg();
    assert_eq!(
        line_end(&scene.document, handle),
        Vector3::new(10.0, 0.0, 0.0)
    );

    let edited = lengthened_copy(&scene, handle);
    assert!(scene.update_entity(edited));
    assert_eq!(
        line_end(&scene.document, handle),
        Vector3::new(25.0, 0.0, 0.0)
    );

    let saved = reopened_as_dwg(&scene);
    assert_eq!(
        line_end(&saved, handle),
        Vector3::new(25.0, 0.0, 0.0),
        "the saved DWG holds the line as it was before the edit"
    );
}

#[test]
fn a_batch_of_edits_written_back_as_copies_is_saved_to_dwg() {
    let (mut scene, handle) = scene_opened_from_dwg();

    let edited = lengthened_copy(&scene, handle);
    assert_eq!(scene.update_entities(vec![edited]), 1);

    let saved = reopened_as_dwg(&scene);
    assert_eq!(
        line_end(&saved, handle),
        Vector3::new(25.0, 0.0, 0.0),
        "the saved DWG holds the line as it was before the edit"
    );
}
