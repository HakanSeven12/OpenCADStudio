//! Handle order inside a block definition: one handle per record, in file order.

use codec::entities::{
    CenterMarkAssociation, CenterMarkSource, CenterMarkSourceKind, Line,
};
use codec::types::{Handle, Vector3};
use codec::EntityType;

use crate::scene::{CreateBlockOptions, Scene};
use crate::ui::window::block_definition::BlockObjectMode;

fn line(x: f64) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::ZERO,
        Vector3::new(x, 0.0, 0.0),
    ))
}

fn assert_ends_after_body(document: &codec::Document, name: &str) {
    let record = document
        .block_records
        .get(name)
        .expect("the record is added");
    let marker = record.block_entity_handle.value();
    let end = record.block_end_handle.value();
    let body: Vec<_> = record
        .entity_handles
        .iter()
        .map(|handle| handle.value())
        .collect();
    assert!(
        body.iter().all(|handle| *handle > marker),
        "body handles follow the BLOCK marker: {body:?}"
    );
    let last = body.iter().max().copied().unwrap_or(marker);
    assert!(
        end > last,
        "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
    );
    assert!(
        matches!(document.get_entity(Handle::new(end)), Some(EntityType::BlockEnd(_))),
        "the record's end handle names an ENDBLK entity"
    );
}

#[test]
fn the_end_block_handle_follows_the_definition_body() {
    let mut scene = Scene::new();
    let body = scene
        .define_block_from_owned_entities(vec![line(1.0), line(2.0)], "ORDERED", glam::DVec3::ZERO)
        .expect("the definition is created");
    let record = scene
        .document
        .block_records
        .get("ORDERED")
        .expect("the record is added");
    let marker = record.block_entity_handle.value();
    let end = record.block_end_handle.value();
    let body: Vec<_> = body.iter().map(|handle| handle.value()).collect();
    assert!(
        body.iter().all(|handle| *handle > marker),
        "body handles follow the BLOCK marker: {body:?}"
    );
    assert!(
        end > *body.iter().max().expect("the body is not empty"),
        "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
    );
}

#[test]
fn the_block_command_path_also_ends_after_its_body() {
    let mut scene = Scene::new();
    let source = scene
        .document
        .add_entity(line(1.0))
        .expect("the source line is added");
    scene
        .create_block_from_entities(
            &[source],
            "COMMAND",
            &codec::types::Transform::identity(),
            &codec::types::Transform::identity(),
        )
        .expect("the definition is created");
    let record = scene
        .document
        .block_records
        .get("COMMAND")
        .expect("the record is added");
    let marker = record.block_entity_handle.value();
    let end = record.block_end_handle.value();
    let body: Vec<_> = record
        .entity_handles
        .iter()
        .map(|handle| handle.value())
        .collect();
    assert!(
        body.iter().all(|handle| *handle > marker),
        "body handles follow the BLOCK marker: {body:?}"
    );
    assert!(
        end > *body.iter().max().expect("the body is not empty"),
        "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
    );
}

#[test]
fn a_copied_definition_also_ends_after_its_body() {
    let mut scene = Scene::new();
    let name = scene
        .define_transformed_block(
            &[line(1.0), line(2.0)],
            &crate::command::EntityTransform::Translate(glam::DVec3::ZERO),
        )
        .expect("the copy's definition is created");
    assert_ends_after_body(&scene.document, &name);
}

#[test]
fn a_redefined_definition_ends_after_its_new_body() {
    let mut scene = Scene::new();
    let options = |handles: Vec<Handle>, redefine: bool| CreateBlockOptions {
        name: "REDEFINED".to_string(),
        handles,
        base_point: glam::DVec3::ZERO,
        world_to_block: codec::types::Transform::identity(),
        block_to_world: codec::types::Transform::identity(),
        mode: BlockObjectMode::Retain,
        annotative: false,
        match_orientation: false,
        scale_uniformly: false,
        allow_exploding: true,
        unit: 0,
        description: String::new(),
        hyperlink_url: String::new(),
        hyperlink_desc: String::new(),
        redefine,
    };
    let first = scene.document.add_entity(line(1.0)).expect("the first line");
    scene
        .create_block_with_options(options(vec![first], false))
        .expect("the definition is created");
    let old_end = scene
        .document
        .block_records
        .get("REDEFINED")
        .expect("the record is added")
        .block_end_handle;
    let second = scene.document.add_entity(line(2.0)).expect("the second line");
    scene
        .create_block_with_options(options(vec![second], true))
        .expect("the definition is redefined");
    assert_ends_after_body(&scene.document, "REDEFINED");
    assert!(
        scene.document.get_entity(old_end).is_none(),
        "the old ENDBLK is replaced, not left behind"
    );
}

#[test]
fn a_definition_rejected_for_its_body_still_ends_with_an_end_block() {
    let mut scene = Scene::new();
    let result = scene.create_block_with_options(CreateBlockOptions {
        name: "REJECTED".to_string(),
        handles: vec![Handle::new(9_999_999)],
        base_point: glam::DVec3::ZERO,
        world_to_block: codec::types::Transform::identity(),
        block_to_world: codec::types::Transform::identity(),
        mode: BlockObjectMode::Retain,
        annotative: false,
        match_orientation: false,
        scale_uniformly: false,
        allow_exploding: true,
        unit: 0,
        description: String::new(),
        hyperlink_url: String::new(),
        hyperlink_desc: String::new(),
        redefine: false,
    });
    assert!(result.is_err(), "no resolvable source is rejected");
    assert_ends_after_body(&scene.document, "REJECTED");
}

#[test]
fn an_exported_centre_mark_ends_after_its_picture() {
    let mut document = codec::Document::new();
    let association = CenterMarkAssociation {
        source: CenterMarkSource {
            handle: Handle::new(9_999),
            kind: CenterMarkSourceKind::Circle,
            segment_index: -1,
            pick_point: Vector3::ZERO,
        },
        plane_origin: Vector3::ZERO,
        plane_x: Vector3::new(1.0, 0.0, 0.0),
        plane_y: Vector3::new(0.0, 1.0, 0.0),
        center: Vector3::ZERO,
        radius: 2.0,
        cross_size: 3.0,
        cross_gap: 0.5,
        cross_size_relative: false,
        cross_gap_relative: false,
        extension_length: 0.0,
        length_adjustments: [0.0; 4],
        overshoots: [0.0; 4],
        show_extensions: false,
        associated: false,
    };
    let mut carrier = Line::from_points(Vector3::ZERO, Vector3::ZERO);
    association.write(&mut carrier.common.extended_data);
    let handle = document
        .add_entity(EntityType::Line(carrier))
        .expect("the carrier is added");
    crate::scene::center_native::export(&mut document);
    let name = document
        .block_records
        .iter()
        .find(|record| record.insert_handles.contains(&handle))
        .expect("the picture block is created")
        .name
        .clone();
    assert_ends_after_body(&document, &name);
}

#[test]
fn a_pasted_definition_also_ends_after_its_body() {
    let mut scene = Scene::new();
    scene.define_block_raw("PASTED", Vector3::ZERO, vec![line(1.0), line(2.0)]);
    assert_ends_after_body(&scene.document, "PASTED");
}

#[test]
fn a_raw_redefine_also_ends_after_its_new_body() {
    let mut scene = Scene::new();
    scene.define_block_raw("RAW_REDEFINE", Vector3::ZERO, vec![line(1.0)]);
    let old_end = scene
        .document
        .block_records
        .get("RAW_REDEFINE")
        .expect("the record is added")
        .block_end_handle;
    scene
        .redefine_block_raw(
            "RAW_REDEFINE",
            Vector3::ZERO,
            vec![line(2.0), line(3.0)],
        )
        .expect("the definition is redefined");
    assert_ends_after_body(&scene.document, "RAW_REDEFINE");
    assert!(
        scene.document.get_entity(old_end).is_none(),
        "the old ENDBLK is replaced, not left behind"
    );
}

/// A definition whose terminator handle names one of its own body lines.
fn corrupt_definition() -> (codec::Document, Handle) {
    let mut document = codec::Document::new();
    let mut record = codec::tables::BlockRecord::new("CORRUPT");
    record.handle = document.allocate_handle();
    let br = record.handle;
    document
        .block_records
        .add(record)
        .expect("the record is added");
    let mut marker = codec::entities::Block::new("CORRUPT", Vector3::ZERO);
    marker.common.handle = document.allocate_handle();
    marker.common.owner_handle = br;
    let marker_handle = marker.common.handle;
    document.add_entity(EntityType::Block(marker)).unwrap();
    let mut body = Line::from_points(Vector3::ZERO, Vector3::new(3.0, 0.0, 0.0));
    body.common.handle = document.allocate_handle();
    body.common.owner_handle = br;
    let body_handle = body.common.handle;
    document.add_entity(EntityType::Line(body)).unwrap();
    let record = document.block_records.get_mut("CORRUPT").unwrap();
    record.block_entity_handle = marker_handle;
    record.entity_handles = vec![body_handle];
    record.block_end_handle = body_handle;
    (document, body_handle)
}

#[test]
fn a_terminator_naming_body_geometry_is_replaced_but_the_geometry_is_kept() {
    let (mut document, body_handle) = corrupt_definition();

    crate::scene::normalize_block_terminators(&mut document);

    assert!(
        document.get_entity(body_handle).is_some(),
        "repairing the terminator leaves body geometry alone"
    );
    assert_ends_after_body(&document, "CORRUPT");
}

#[test]
fn reterminating_a_definition_keeps_body_geometry_named_as_its_terminator() {
    let mut scene = Scene::new();
    scene.document = corrupt_definition().0;
    let body_handle = scene
        .document
        .block_records
        .get("CORRUPT")
        .unwrap()
        .entity_handles[0];

    scene
        .reterminate_block_definition("CORRUPT")
        .expect("the definition is re-terminated");

    assert!(
        scene.document.get_entity(body_handle).is_some(),
        "repairing the terminator leaves body geometry alone"
    );
    assert_ends_after_body(&scene.document, "CORRUPT");
}

#[test]
fn normalizing_never_invents_a_terminator_for_model_space() {
    let mut scene = Scene::new();
    let model_space = scene.document.header.model_space_block_handle;
    let name = scene
        .document
        .block_records
        .iter()
        .find(|record| record.handle == model_space)
        .map(|record| record.name.clone())
        .expect("the model-space record exists");
    scene
        .document
        .block_records
        .get_mut(&name)
        .unwrap()
        .block_end_handle = Handle::NULL;
    let next_handle = scene.document.next_handle();

    crate::scene::normalize_block_terminators(&mut scene.document);

    assert!(
        scene
            .document
            .block_records
            .get(&name)
            .unwrap()
            .block_end_handle
            .is_null(),
        "model space is written by the document layer, not by definition repair"
    );
    assert_eq!(
        scene.document.next_handle(),
        next_handle,
        "normalizing does not allocate a handle"
    );
}
