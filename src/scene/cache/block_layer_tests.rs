//! Placement-specific layer attribution for cached block expansion.

use std::collections::HashMap;

use document::entities::{Insert, Line};
use document::tables::{BlockRecord, Layer};
use document::types::Vector3;
use document::{Document, EntityType, Handle};

use super::block_cache::{expand_insert, BlockCache};
use crate::scene::model::instance_model::expand_stubs;
use crate::scene::view::render::{layer_render_style_viewport, render_style_for_viewport};

fn add_block(document: &mut Document, name: &str) -> Handle {
    let mut block = BlockRecord::new(name);
    block.handle = document.allocate_handle();
    let handle = block.handle;
    document.block_records.add(block).unwrap();
    handle
}

fn add_owned(document: &mut Document, owner: Handle, mut entity: EntityType) -> Handle {
    entity.common_mut().owner_handle = owner;
    document.add_entity(entity).unwrap()
}

fn add_line(document: &mut Document, owner: Handle, layer: &str, y: f64) -> Handle {
    let mut line = Line::from_points(Vector3::new(0.0, y, 0.0), Vector3::new(10.0, y, 0.0));
    line.common.layer = layer.to_string();
    add_owned(document, owner, EntityType::Line(line))
}

fn add_insert(
    document: &mut Document,
    owner: Option<Handle>,
    block: &str,
    layer: &str,
    x: f64,
) -> (Insert, Handle) {
    let mut insert = Insert::new(block, Vector3::new(x, 0.0, 0.0));
    insert.common.layer = layer.to_string();
    let entity = EntityType::Insert(insert.clone());
    let handle = if let Some(owner) = owner {
        add_owned(document, owner, entity)
    } else {
        document.add_entity(entity).unwrap()
    };
    (insert, handle)
}

fn cache(document: &Document) -> BlockCache {
    BlockCache::build(
        document,
        1.0,
        None,
        true,
        [0.0, 0.0, 0.0, 1.0],
        None,
        &HashMap::default(),
    )
}

fn expanded_layers(
    document: &Document,
    cache: &BlockCache,
    insert: &Insert,
    handle: Handle,
) -> Vec<String> {
    let entity = EntityType::Insert(insert.clone());
    let (color, pat_len, pat, lw_px, aci) = render_style_for_viewport(document, &entity, None);
    let plottable = document
        .layers
        .get(&insert.common.layer)
        .is_none_or(|layer| layer.is_plottable);
    let wires = expand_insert(
        document,
        cache,
        insert,
        handle,
        color,
        aci,
        pat_len,
        pat,
        lw_px,
        layer_render_style_viewport(document, &insert.common.layer, None),
        0,
        plottable,
        false,
        1.0,
        None,
        None,
        false,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
        crate::scene::BlockScalePolicy::FromInsert,
        false,
        1.0,
    )
    .expect("cached block expands");
    let wires = expand_stubs(&wires);
    let mut layers: Vec<String> = wires
        .iter()
        .filter(|wire| {
            !wire.points.is_empty() || !wire.fill_tris.is_empty() || !wire.text_verts.is_empty()
        })
        .map(|wire| {
            wire.effective_layer
                .as_deref()
                .expect("expanded geometry carries its effective layer")
                .to_string()
        })
        .collect();
    layers.sort();
    layers.dedup();
    layers
}

fn layer(document: &mut Document, name: &str) {
    document.layers.add(Layer::new(name)).unwrap();
}

#[test]
fn children_keep_nonzero_layers_and_inherit_without_rewriting_sources() {
    let mut document = Document::new();
    for name in ["HOST", "STICKY", "XREF|STICKY", "XREF|0"] {
        layer(&mut document, name);
    }
    let leaf = add_block(&mut document, "LEAF");
    let raw = ["0", "STICKY", "XREF|STICKY", "XREF|0"];
    let handles: Vec<Handle> = raw
        .iter()
        .enumerate()
        .map(|(index, layer)| add_line(&mut document, leaf, layer, index as f64))
        .collect();
    let (host, host_handle) = add_insert(&mut document, None, "LEAF", "HOST", 0.0);
    let (zero, zero_handle) = add_insert(&mut document, None, "LEAF", "0", 100.0);
    let cache = cache(&document);

    assert_eq!(
        expanded_layers(&document, &cache, &host, host_handle),
        ["HOST", "STICKY", "XREF|STICKY"]
    );
    assert_eq!(
        expanded_layers(&document, &cache, &zero, zero_handle),
        ["0", "STICKY", "XREF|0", "XREF|STICKY"]
    );
    for (handle, raw_layer) in handles.into_iter().zip(raw) {
        assert_eq!(
            document.get_entity(handle).unwrap().common().layer,
            raw_layer,
            "expansion must not rewrite the raw block definition"
        );
    }
}

#[test]
fn nested_insert_uses_the_nearest_nonzero_layer() {
    let mut document = Document::new();
    for name in ["HOST", "NESTED", "XREF|NESTED"] {
        layer(&mut document, name);
    }
    let leaf = add_block(&mut document, "LEAF");
    add_line(&mut document, leaf, "0", 0.0);
    let deferred = add_block(&mut document, "DEFERRED");
    add_insert(&mut document, Some(deferred), "LEAF", "0", 0.0);
    let overridden = add_block(&mut document, "OVERRIDDEN");
    add_insert(&mut document, Some(overridden), "LEAF", "NESTED", 0.0);
    let xref = add_block(&mut document, "XREF_OUTER");
    add_insert(&mut document, Some(xref), "LEAF", "XREF|NESTED", 0.0);

    let mut placements = Vec::new();
    for (block, expected) in [
        ("DEFERRED", "HOST"),
        ("OVERRIDDEN", "NESTED"),
        ("XREF_OUTER", "XREF|NESTED"),
    ] {
        let (insert, handle) = add_insert(&mut document, None, block, "HOST", 0.0);
        placements.push((insert, handle, expected));
    }
    let cache = cache(&document);
    for (insert, handle, expected) in placements {
        assert_eq!(
            expanded_layers(&document, &cache, &insert, handle),
            [expected]
        );
    }
}

#[test]
fn repeated_top_level_prototypes_keep_each_placement_layer() {
    let mut document = Document::new();
    for name in ["FIRST", "SECOND"] {
        layer(&mut document, name);
    }
    let leaf = add_block(&mut document, "SHARED");
    add_line(&mut document, leaf, "0", 0.0);
    let (first, first_handle) = add_insert(&mut document, None, "SHARED", "FIRST", 0.0);
    let (second, second_handle) = add_insert(&mut document, None, "SHARED", "SECOND", 100.0);
    let cache = cache(&document);

    assert_eq!(
        expanded_layers(&document, &cache, &first, first_handle),
        ["FIRST"]
    );
    assert_eq!(
        expanded_layers(&document, &cache, &second, second_handle),
        ["SECOND"],
        "a translated prototype must not retain the first placement's layer"
    );
}

#[test]
fn nested_prototypes_and_batches_are_separated_by_effective_layer() {
    let mut document = Document::new();
    for name in ["NESTED_A", "NESTED_B"] {
        layer(&mut document, name);
    }
    let leaf = add_block(&mut document, "LEAF");
    // Exceed the inline-nesting budget so this exercises NestedPrototypeKey.
    for index in 0..130 {
        add_line(&mut document, leaf, "0", index as f64);
    }
    let root = add_block(&mut document, "ROOT");
    add_insert(&mut document, Some(root), "LEAF", "NESTED_A", 0.0);
    add_insert(&mut document, Some(root), "LEAF", "NESTED_B", 1_000.0);
    let (insert, handle) = add_insert(&mut document, None, "ROOT", "0", 0.0);
    let cache = cache(&document);

    assert_eq!(
        expanded_layers(&document, &cache, &insert, handle),
        ["NESTED_A", "NESTED_B"]
    );
}
