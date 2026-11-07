//! A dash inside a block is the linetype's length times LTSCALE, the child's own
//! linetype scale and nothing from the INSERT that places it: neither its scale
//! nor its linetype scale, whichever way the child gets its linetype.

use std::collections::HashSet;

use document::entities::{Insert, Line};
use document::tables::linetype::LineTypeElement;
use document::tables::{BlockRecord, Layer, LineType};
use document::types::Vector3;
use document::{Document, EntityType, Handle};

/// A 2 dash and 2 gap: a period of 4.
fn dashed_document() -> Document {
    let mut document = Document::new();
    let mut dashes = LineType::new("DASHES");
    dashes.handle = document.allocate_handle();
    dashes.add_element(LineTypeElement::dash(2.0));
    dashes.add_element(LineTypeElement::space(2.0));
    dashes.pattern_length = 4.0;
    document.line_types.add(dashes).unwrap();
    document
}

fn add_block(document: &mut Document, name: &str) -> Handle {
    let mut block = BlockRecord::new(name);
    block.handle = document.allocate_handle();
    let handle = block.handle;
    document.block_records.add(block).unwrap();
    handle
}

/// A block of `name` holding one 100-long line.
fn block_with_line(document: &mut Document, name: &str, linetype: &str, lt_scale: f64) {
    let owner = add_block(document, name);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::new(100.0, 0.0, 0.0));
    line.common.owner_handle = owner;
    line.common.linetype = linetype.to_string();
    line.common.linetype_scale = lt_scale;
    document.add_entity(EntityType::Line(line)).unwrap();
}

/// The dash period the INSERT draws its line with.
fn period(document: &Document, insert: Insert) -> f32 {
    let entity = EntityType::Insert(insert);
    let wires = crate::scene::tessellate_entity(
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
        None,
        &HashSet::default(),
    );
    let dashed: Vec<f32> = wires
        .iter()
        .filter(|wire| wire.pattern_length > 0.0)
        .map(|wire| wire.pattern_length)
        .collect();
    assert_eq!(dashed.len(), 1, "{dashed:?}");
    dashed[0]
}

#[test]
fn a_scaled_insert_keeps_the_length_of_its_dashes() {
    let mut document = dashed_document();
    block_with_line(&mut document, "B", "DASHES", 1.0);
    for scale in [0.5, 1.0, 2.0, 13.0] {
        let mut insert = Insert::new("B", Vector3::ZERO);
        insert.set_x_scale(scale);
        insert.set_y_scale(scale);
        assert!(
            (period(&document, insert) - 4.0).abs() < 1e-5,
            "scale {scale}"
        );
    }
}

#[test]
fn a_child_keeps_its_own_linetype_scale_and_ltscale_applies() {
    let mut document = dashed_document();
    block_with_line(&mut document, "B", "DASHES", 2.0);
    document.header.linetype_scale = 3.0;
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.linetype_scale = 5.0;
    assert!((period(&document, insert) - 24.0).abs() < 1e-5);
}

#[test]
fn a_byblock_child_takes_the_linetype_but_not_the_scale_of_its_insert() {
    let mut document = dashed_document();
    block_with_line(&mut document, "B", "BYBLOCK", 2.0);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.linetype = "DASHES".to_string();
    insert.common.linetype_scale = 5.0;
    assert!((period(&document, insert) - 8.0).abs() < 1e-5);
}

#[test]
fn a_layer_zero_child_takes_the_linetype_of_its_insert_layer_at_its_own_scale() {
    let mut document = dashed_document();
    let mut layer = Layer::new("FENCES");
    layer.line_type = "DASHES".to_string();
    document.layers.add(layer).unwrap();
    block_with_line(&mut document, "B", "BYLAYER", 2.0);
    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.layer = "FENCES".to_string();
    insert.common.linetype_scale = 5.0;
    assert!((period(&document, insert) - 8.0).abs() < 1e-5);
}

#[test]
fn a_nested_byblock_chain_ends_at_the_scale_of_the_child() {
    let mut document = dashed_document();
    block_with_line(&mut document, "INNER", "BYBLOCK", 2.0);
    let outer = add_block(&mut document, "OUTER");
    let mut nested = Insert::new("INNER", Vector3::ZERO);
    nested.common.owner_handle = outer;
    nested.common.linetype = "BYBLOCK".to_string();
    nested.common.linetype_scale = 7.0;
    document.add_entity(EntityType::Insert(nested)).unwrap();
    let mut insert = Insert::new("OUTER", Vector3::ZERO);
    insert.common.linetype = "DASHES".to_string();
    insert.common.linetype_scale = 5.0;
    assert!((period(&document, insert) - 8.0).abs() < 1e-5);
}
