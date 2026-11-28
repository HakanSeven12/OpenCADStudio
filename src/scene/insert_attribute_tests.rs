// Tests of how the scene places, moves and draws the attributes of an
// insert (`entities::insert`).
use document::entities::{AttributeEntity, Insert};
use document::tables::Layer;
use document::types::Vector3;
use document::{Document, EntityType, Handle};

use crate::entities::insert::*;
use crate::entities::object::GripApply;
use crate::entities::transform::EntityTransform;
use crate::scene::cache::block_cache;
use crate::scene::view::render;

fn attribute(tag: &str, x: f64, y: f64) -> AttributeEntity {
    let mut att = AttributeEntity::new(tag.to_string(), format!("{tag}-value"));
    att.insertion_point = Vector3::new(x, y, 0.0);
    att.alignment_point = Vector3::new(x, y, 0.0);
    att
}

fn one_character_attribute(tag: &str, x: f64) -> AttributeEntity {
    let mut att = attribute(tag, x, 0.0);
    att.value = "X".into();
    att
}

fn block_with(attributes: Vec<AttributeEntity>) -> Insert {
    let mut ins = Insert::new("BLOCK", Vector3::new(0.0, 0.0, 0.0));
    ins.attributes = attributes;
    ins
}

fn document_with_layers(layers: Vec<Layer>) -> Document {
    let mut document = Document::new();
    for layer in layers {
        document.layers.add_or_replace(layer);
    }
    document
}

/// A constant attribute is drawn by the definition it belongs to, which is
/// why it gets no grip; the reference must not draw the same text a second
/// time under it.
/// An attribute saved at 1:50 with its own placement at 1:100. Its scale
/// data lives under the attribute's own extension dictionary: an ATTRIB is
/// part of its INSERT, not a document entity.
fn annotative_attribute() -> (Document, Insert, Handle, Handle, Handle) {
    use crate::scene::annotative::{create_annotation_context, ensure_scale_object};
    use document::objects::{ObjectContextKind, ObjectType, Scale};
    use document::types::Vector2;

    let mut document = Document::new();
    let at_50 = ensure_scale_object(&mut document, &Scale::new("1:50", 1.0, 50.0));
    let at_100 = ensure_scale_object(&mut document, &Scale::new("1:100", 1.0, 100.0));
    let other = ensure_scale_object(&mut document, &Scale::new("1:25", 1.0, 25.0));
    let mut source = attribute("CIRCUITO", 10.2, 10.0);
    source.height = 0.125;
    let handle = document
        .add_entity(EntityType::AttributeEntity(source))
        .unwrap();
    assert!(create_annotation_context(&mut document, handle, at_50));
    assert!(create_annotation_context(&mut document, handle, at_100));
    for object in document.objects.values_mut() {
        if let ObjectType::ObjectContextData(leaf) = object {
            if let (true, ObjectContextKind::MTextAttribute(value)) =
                (leaf.scale == at_100, &mut leaf.kind)
            {
                value.insertion = Vector2::new(12.4, 10.0);
                value.alignment = Vector2::new(12.4, 10.0);
            }
        }
    }
    let Some(EntityType::AttributeEntity(mut placed)) = document.get_entity(handle).cloned() else {
        panic!("the attribute went missing");
    };
    // Its handle resolves to no document entity, as an INSERT's own ATTRIB.
    placed.common.handle = document.allocate_handle();
    (document, block_with(vec![placed]), at_50, at_100, other)
}

fn painted_attribute(document: &Document, insert: &Insert, scale: Handle) -> (f64, f64) {
    let painted = insert_attribute_entities(
        document,
        insert,
        1.0,
        Some(scale),
        BlockScalePolicy::FromInsert,
    );
    let [EntityType::AttributeEntity(attribute)] = painted.as_slice() else {
        panic!("expected one attribute, got {painted:?}");
    };
    (attribute.insertion_point.x, attribute.height)
}

#[test]
fn an_annotative_attribute_takes_the_place_and_height_of_the_current_scale() {
    let (document, insert, at_50, at_100, _) = annotative_attribute();
    let (x, height) = painted_attribute(&document, &insert, at_100);
    assert!((x - 12.4).abs() < 1e-9, "x = {x}");
    assert!((height - 0.25).abs() < 1e-9, "height = {height}");

    let (x, height) = painted_attribute(&document, &insert, at_50);
    assert!((x - 10.2).abs() < 1e-9, "x = {x}");
    assert!((height - 0.125).abs() < 1e-9, "height = {height}");
}

#[test]
fn annotative_attribute_grip_moves_the_painted_text_during_drag() {
    let (document, insert, at_50, at_100, _) = annotative_attribute();
    let grips = visible_attribute_grips(&document, &insert, 1.0, Some(at_100));
    assert!((grips[1].world.x - 12.4).abs() < 1e-9);

    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    scene.document.header.current_annotation_scale = "1:100".into();
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    let preview_text_min_x = |scene: &crate::scene::Scene| {
        scene
            .grip_wire_models_for(&[handle])
            .iter()
            .flat_map(|wire| wire.text_verts.iter())
            .map(|vertex| f64::from(vertex.pos[0]) + f64::from(vertex.pos_low[0]))
            .reduce(f64::min)
            .expect("attribute text must be present in the grip preview")
    };
    let original_text_x = preview_text_min_x(&scene);
    scene.apply_grip(
        handle,
        grips[1].id,
        GripApply::Absolute(glam::DVec3::new(20.4, 10.0, 0.0)),
    );

    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((insert.attributes[0].insertion_point.x - 18.2).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 20.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_50).0 - 18.2).abs() < 1e-9);
    let grips = visible_attribute_grips(&scene.document, insert, 1.0, Some(at_100));
    assert!((grips[1].world.x - 20.4).abs() < 1e-9);
    assert!((preview_text_min_x(&scene) - original_text_x - 8.0).abs() < 1e-4);
}

#[test]
fn insert_grip_moves_annotative_attribute_text_with_the_block() {
    let (document, insert, _, at_100, _) = annotative_attribute();
    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    scene.document.header.current_annotation_scale = "1:100".into();
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    scene.apply_grip(
        handle,
        0,
        GripApply::Translate(glam::DVec3::new(8.0, 0.0, 0.0)),
    );

    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 20.4).abs() < 1e-9);
}

#[test]
fn block_move_records_attribute_contexts_for_delta_undo() {
    let (document, insert, at_50, at_100, _) = annotative_attribute();
    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();

    scene.begin_undo_recording();
    scene.transform_entities(
        &[handle],
        &EntityTransform::Translate(glam::DVec3::new(8.0, 3.0, 0.0)),
    );
    let recording = scene.take_undo_recording().unwrap();
    assert!(!recording.is_poisoned());
    let (_, objects, _, _, _) = recording.into_recorded_images();
    assert!(
        objects.len() >= 2,
        "each scale context needs a before-image"
    );
    assert!(objects.iter().all(|(object_handle, before)| {
        before.as_ref() != scene.document.objects.get(object_handle)
    }));

    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 20.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_50).0 - 18.2).abs() < 1e-9);
}

#[test]
fn block_move_and_rotate_carry_each_attribute_scale_context() {
    let (document, insert, at_50, at_100, _) = annotative_attribute();
    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();

    scene.transform_entities(
        &[handle],
        &EntityTransform::Translate(glam::DVec3::new(8.0, 3.0, 0.0)),
    );
    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 20.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_50).0 - 18.2).abs() < 1e-9);

    scene.transform_entities(
        &[handle],
        &EntityTransform::Rotate {
            center: glam::DVec3::ZERO,
            axis: glam::DVec3::Z,
            angle_rad: std::f64::consts::FRAC_PI_2,
        },
    );
    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    let painted = |scale| {
        let entities = insert_attribute_entities(
            &scene.document,
            insert,
            1.0,
            Some(scale),
            BlockScalePolicy::FromInsert,
        );
        let EntityType::AttributeEntity(attribute) = &entities[0] else {
            panic!("expected ATTRIB")
        };
        attribute.insertion_point
    };
    let at_100_position = painted(at_100);
    let at_50_position = painted(at_50);
    assert!((at_100_position.x + 13.0).abs() < 1e-9);
    assert!((at_100_position.y - 20.4).abs() < 1e-9);
    assert!((at_50_position.x + 13.0).abs() < 1e-9);
    assert!((at_50_position.y - 18.2).abs() < 1e-9);
}

#[test]
fn block_copy_carries_attribute_contexts_to_the_new_position() {
    let (document, insert, at_50, at_100, _) = annotative_attribute();
    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    let source = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    let copies = scene.copy_entities(
        &[source],
        &EntityTransform::Translate(glam::DVec3::new(8.0, 0.0, 0.0)),
    );
    let [copy] = copies.as_slice() else {
        panic!("expected one copy")
    };
    let EntityType::Insert(original) = scene.document.get_entity(source).unwrap() else {
        panic!("expected source INSERT")
    };
    let EntityType::Insert(copied) = scene.document.get_entity(*copy).unwrap() else {
        panic!("expected copied INSERT")
    };
    assert!((painted_attribute(&scene.document, original, at_100).0 - 12.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, copied, at_100).0 - 20.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, copied, at_50).0 - 18.2).abs() < 1e-9);
    assert_ne!(
        original.attributes[0].common.xdictionary_handle,
        copied.attributes[0].common.xdictionary_handle,
    );
}

#[test]
fn block_position_property_carries_attribute_contexts() {
    let (document, insert, at_50, at_100, _) = annotative_attribute();
    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert.clone()))
        .unwrap();
    crate::entities::dispatch::apply_geom_prop(
        scene.document.get_entity_mut(handle).unwrap(),
        "ins_x",
        "8",
    );
    scene.carry_insert_attribute_contexts_after_position_edit(handle, &insert);

    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 20.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_50).0 - 18.2).abs() < 1e-9);
}

#[test]
fn a_nested_insert_places_its_annotative_attribute_for_the_current_scale() {
    use document::tables::BlockRecord;

    let (mut document, mut nested, at_50, at_100, _) = annotative_attribute();
    nested.block_name = "INNER".to_string();
    for name in ["INNER", "OUTER"] {
        let mut record = BlockRecord::new(name);
        record.handle = document.allocate_handle();
        document.block_records.add(record).unwrap();
    }
    nested.common.owner_handle = document.block_records.get("OUTER").unwrap().handle;
    document.add_entity(EntityType::Insert(nested)).unwrap();
    let outer = Insert::new("OUTER", Vector3::ZERO);
    document
        .add_entity(EntityType::Insert(outer.clone()))
        .unwrap();

    let attribute_x = |scale: Handle| {
        let cache = block_cache::BlockCache::build(
            &document,
            1.0,
            Some(scale),
            true,
            [0.0, 0.0, 0.0, 1.0],
            None,
            &Default::default(),
        );
        let style = render::InheritStyle {
            color: [1.0; 4],
            pat_len: 0.0,
            pat: [0.0; 8],
            lw_px: 1.0,
        };
        let wires = block_cache::expand_insert(
            &document,
            &cache,
            &outer,
            Handle::new(0x40),
            [1.0; 4],
            7,
            0.0,
            [0.0; 8],
            1.0,
            style,
            7,
            true,
            false,
            1.0,
            None,
            None,
            false,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
            BlockScalePolicy::FromInsert,
            false,
            1.0,
        )
        .expect("the block expands");
        wires
            .iter()
            .flat_map(|wire| {
                let strokes = wire.points.iter().map(|p| p[0]);
                strokes.chain(wire.text_verts.iter().map(|v| v.pos[0]))
            })
            .filter(|x| x.is_finite())
            .fold(f32::INFINITY, f32::min)
    };
    assert!((attribute_x(at_100) - 12.4).abs() < 0.1);
    assert!((attribute_x(at_50) - 10.2).abs() < 0.1);
}

#[test]
fn an_annotative_attribute_without_the_current_scale_shows_its_default() {
    let (document, insert, _, _, other) = annotative_attribute();
    let mut attribute = insert.attributes[0].clone();
    assert!(
        crate::entities::annotative::place_attribute_for_annotation_scale(
            &document,
            &mut attribute,
            Some(other),
        )
    );
    let (x, height) = painted_attribute(&document, &insert, other);
    assert!((x - 10.2).abs() < 1e-9, "x = {x}");
    assert!((height - 0.125).abs() < 1e-9, "height = {height}");
}

#[test]
fn attmode_keeps_entity_and_attribute_visibility_independent() {
    let mut off_layer = Layer::new("OFF");
    off_layer.flags.off = true;
    let mut frozen_layer = Layer::new("FROZEN");
    frozen_layer.flags.frozen = true;
    let mut document = document_with_layers(vec![off_layer, frozen_layer]);

    // One-character labels at spaced X positions let the emitted wire
    // geometry identify which attribute reached the render path.
    let mut common_invisible = one_character_attribute("TAG_COMMON_INVISIBLE", 2_000.0);
    common_invisible.common.invisible = true;
    let mut attribute_invisible = one_character_attribute("TAG_ATTRIBUTE_INVISIBLE", 3_000.0);
    attribute_invisible.flags.invisible = true;
    let mut both_invisible = one_character_attribute("TAG_BOTH_INVISIBLE", 4_000.0);
    both_invisible.common.invisible = true;
    both_invisible.flags.invisible = true;
    let mut off_layer_attribute = one_character_attribute("TAG_OFF_LAYER", 5_000.0);
    off_layer_attribute.common.layer = "OFF".into();
    let mut frozen_layer_attribute = one_character_attribute("TAG_FROZEN_LAYER", 6_000.0);
    frozen_layer_attribute.common.layer = "FROZEN".into();
    let mut constant = one_character_attribute("TAG_CONSTANT", 7_000.0);
    constant.flags.constant = true;

    let insert = block_with(vec![
        one_character_attribute("TAG_NORMAL", 1_000.0),
        common_invisible,
        attribute_invisible,
        both_invisible,
        off_layer_attribute,
        frozen_layer_attribute,
        constant,
    ]);

    for (mode, expected_geometry_indices, expected_grip_indices) in [
        (0, vec![], vec![]),
        (1, vec![0], vec![0]),
        (2, vec![0, 2], vec![0, 2]),
    ] {
        document.header.attribute_visibility = mode;

        let mut wires = Vec::new();
        let insert_color = [1.0; 4];
        crate::scene::convert::insert_tess::append_insert_attribute_wires(
            &mut wires,
            &document,
            &insert,
            Handle::new(0x40),
            false,
            insert_color,
            0.0,
            [0.0; 8],
            1.0,
            render::InheritStyle {
                color: insert_color,
                pat_len: 0.0,
                pat: [0.0; 8],
                lw_px: 1.0,
            },
            true,
            [0.0, 0.0, 0.0, 1.0],
            false,
            1.0,
            1.0,
            None,
        );

        let mut geometry_xs = Vec::new();
        for wire in &wires {
            for (index, point) in wire.points.iter().enumerate() {
                let low = wire.points_low.get(index).unwrap_or([0.0; 3]);
                geometry_xs.push(f64::from(point[0]) + f64::from(low[0]));
            }
            geometry_xs.extend(
                wire.text_verts
                    .iter()
                    .map(|vertex| f64::from(vertex.pos[0]) + f64::from(vertex.pos_low[0])),
            );
        }
        let geometry_indices: Vec<usize> = (0..7)
            .filter(|index| {
                let min_x = (*index as f64 + 1.0) * 1_000.0 - 1.0;
                let max_x = min_x + 100.0;
                geometry_xs.iter().any(|x| *x >= min_x && *x < max_x)
            })
            .collect();
        assert_eq!(
            geometry_indices, expected_geometry_indices,
            "rendered geometry for ATTMODE={mode}"
        );

        let grips = visible_attribute_grips(&document, &insert, 1.0, None);
        let grip_indices: Vec<usize> = grips
            .iter()
            .skip(1)
            .map(|grip| grip.id - ATTRIBUTE_GRIP_BASE)
            .collect();
        assert_eq!(
            grip_indices, expected_grip_indices,
            "attribute grips for ATTMODE={mode}"
        );
    }
}

fn multiscale_annotative_insert() -> (crate::scene::Scene, Handle, Handle, Handle) {
    use document::objects::{Dictionary, ObjectContextData, ObjectContextKind, ObjectType};

    let (mut document, mut insert, at_50, at_100, _) = annotative_attribute();
    insert.insert_point = Vector3::new(100.0, 50.0, 0.0);
    insert.set_x_scale(50.0);
    insert.set_y_scale(50.0);
    insert.set_z_scale(50.0);
    document.header.current_annotation_scale = "1:100".into();

    let mut scene = crate::scene::Scene::new();
    scene.document = document;
    scene.current_layout = "Model".into();
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();

    let xdict_h = scene.document.allocate_handle();
    let mgr_h = scene.document.allocate_handle();
    let coll_h = scene.document.allocate_handle();
    let leaf_50_h = scene.document.allocate_handle();
    let leaf_100_h = scene.document.allocate_handle();

    let mut xdict = Dictionary::new();
    xdict.handle = xdict_h;
    xdict.owner = handle;
    xdict.hard_owner = true;
    xdict.add_entry("AcDbContextDataManager", mgr_h);

    let mut mgr = Dictionary::new();
    mgr.handle = mgr_h;
    mgr.owner = xdict_h;
    mgr.add_entry("ACDB_ANNOTATIONSCALES", coll_h);

    let mut coll = Dictionary::new();
    coll.handle = coll_h;
    coll.owner = mgr_h;
    coll.add_entry("*A1", leaf_50_h);
    coll.add_entry("*A2", leaf_100_h);

    let mk_blkref = |h: Handle, scale: Handle, is_default: bool, pos: Vector3, s: f64| {
        ObjectType::ObjectContextData(ObjectContextData {
            handle: h,
            owner_handle: coll_h,
            reactors: vec![coll_h],
            xdictionary_handle: None,
            class_version: 4,
            has_file_to_extension_dictionary: true,
            uses_legacy_mleader_context_base: false,
            is_default,
            scale,
            kind: ObjectContextKind::BlkRef {
                rotation: 0.0,
                insertion: pos,
                scale_factor: Vector3::new(s, s, s),
            },
        })
    };

    scene
        .document
        .objects
        .insert(xdict_h, ObjectType::Dictionary(xdict));
    scene
        .document
        .objects
        .insert(mgr_h, ObjectType::Dictionary(mgr));
    scene
        .document
        .objects
        .insert(coll_h, ObjectType::Dictionary(coll));
    scene.document.objects.insert(
        leaf_50_h,
        mk_blkref(leaf_50_h, at_50, true, Vector3::new(100.0, 50.0, 0.0), 50.0),
    );
    scene.document.objects.insert(
        leaf_100_h,
        mk_blkref(
            leaf_100_h,
            at_100,
            false,
            Vector3::new(80.0, 60.0, 0.0),
            100.0,
        ),
    );
    if let Some(entity) = scene.document.get_entity_mut(handle) {
        entity.common_mut().xdictionary_handle = Some(xdict_h);
    }

    (scene, handle, at_50, at_100)
}

fn blkref_context(document: &Document, handle: Handle, scale: Handle) -> (Vector3, Vector3) {
    let entity = document.get_entity(handle).expect("insert exists");
    let placed =
        crate::scene::annotative::entity_for_annotation_context(document, entity, Some(scale));
    let EntityType::Insert(ins) = placed.as_ref() else {
        panic!("expected INSERT")
    };
    (
        ins.insert_point,
        Vector3::new(ins.x_scale(), ins.y_scale(), ins.z_scale()),
    )
}

#[test]
fn annotative_insert_move_preserves_displayed_scale_placement_and_factor() {
    let (mut scene, handle, at_50, at_100) = multiscale_annotative_insert();
    scene.transform_entities(
        &[handle],
        &EntityTransform::Translate(glam::DVec3::new(10.0, 20.0, 0.0)),
    );

    let (pos_100, scale_100) = blkref_context(&scene.document, handle, at_100);
    let (pos_50, scale_50) = blkref_context(&scene.document, handle, at_50);
    assert!((pos_100.x - 90.0).abs() < 1e-9 && (pos_100.y - 80.0).abs() < 1e-9);
    assert!((scale_100.x - 100.0).abs() < 1e-9 && (scale_100.y - 100.0).abs() < 1e-9);
    assert!((pos_50.x - 110.0).abs() < 1e-9 && (pos_50.y - 70.0).abs() < 1e-9);
    assert!((scale_50.x - 50.0).abs() < 1e-9 && (scale_50.y - 50.0).abs() < 1e-9);

    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 22.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_50).0 - 20.2).abs() < 1e-9);
}

#[test]
fn annotative_insert_copy_preserves_per_scale_contexts_and_disjoint_handles() {
    let (mut scene, handle, at_50, at_100) = multiscale_annotative_insert();
    let src_xdict = scene
        .document
        .get_entity(handle)
        .unwrap()
        .common()
        .xdictionary_handle;
    let copies = scene.copy_entities(
        &[handle],
        &EntityTransform::Translate(glam::DVec3::new(30.0, -10.0, 0.0)),
    );
    assert_eq!(copies.len(), 1);
    let copy = copies[0];
    let copy_xdict = scene
        .document
        .get_entity(copy)
        .unwrap()
        .common()
        .xdictionary_handle;
    assert!(copy_xdict.is_some());
    assert_ne!(copy_xdict, src_xdict);

    // Copy keeps distinct 1:100 and 1:50 placements & scale factors.
    let (pos_100, scale_100) = blkref_context(&scene.document, copy, at_100);
    let (pos_50, scale_50) = blkref_context(&scene.document, copy, at_50);
    assert!((pos_100.x - 110.0).abs() < 1e-9 && (pos_100.y - 50.0).abs() < 1e-9);
    assert!((scale_100.x - 100.0).abs() < 1e-9 && (scale_100.y - 100.0).abs() < 1e-9);
    assert!((pos_50.x - 130.0).abs() < 1e-9 && (pos_50.y - 40.0).abs() < 1e-9);
    assert!((scale_50.x - 50.0).abs() < 1e-9 && (scale_50.y - 50.0).abs() < 1e-9);

    // Source is untouched.
    let (src_100, src_scale_100) = blkref_context(&scene.document, handle, at_100);
    assert!((src_100.x - 80.0).abs() < 1e-9 && (src_100.y - 60.0).abs() < 1e-9);
    assert!((src_scale_100.x - 100.0).abs() < 1e-9);

    // Copy's attribute contexts are disjoint and translated.
    let EntityType::Insert(src_ins) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    let EntityType::Insert(copy_ins) = scene.document.get_entity(copy).unwrap() else {
        panic!("expected INSERT")
    };
    assert_ne!(
        copy_ins.attributes[0].common.xdictionary_handle,
        src_ins.attributes[0].common.xdictionary_handle
    );
    assert!((painted_attribute(&scene.document, copy_ins, at_100).0 - 42.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, src_ins, at_100).0 - 12.4).abs() < 1e-9);
}

#[test]
fn annotative_insert_grip_drag_preserves_displayed_scale_and_moves_all_scales() {
    let (mut scene, handle, at_50, at_100) = multiscale_annotative_insert();
    // Displayed 1:100 grip is at (80, 60); drag it to (90, 80) -> delta = (10, 20).
    scene.apply_grip(
        handle,
        0,
        GripApply::Absolute(glam::DVec3::new(90.0, 80.0, 0.0)),
    );

    let (pos_100, scale_100) = blkref_context(&scene.document, handle, at_100);
    let (pos_50, scale_50) = blkref_context(&scene.document, handle, at_50);
    assert!((pos_100.x - 90.0).abs() < 1e-9 && (pos_100.y - 80.0).abs() < 1e-9);
    assert!((scale_100.x - 100.0).abs() < 1e-9 && (scale_100.y - 100.0).abs() < 1e-9);
    assert!((pos_50.x - 110.0).abs() < 1e-9 && (pos_50.y - 70.0).abs() < 1e-9);
    assert!((scale_50.x - 50.0).abs() < 1e-9 && (scale_50.y - 50.0).abs() < 1e-9);

    let EntityType::Insert(insert) = scene.document.get_entity(handle).unwrap() else {
        panic!("expected INSERT")
    };
    assert!((painted_attribute(&scene.document, insert, at_100).0 - 22.4).abs() < 1e-9);
    assert!((painted_attribute(&scene.document, insert, at_50).0 - 20.2).abs() < 1e-9);
}
