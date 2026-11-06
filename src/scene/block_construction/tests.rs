use super::*;
use crate::scene::{render_graph, Scene};
use document::{
    entities::{Circle, Insert, Line},
    types::{Color, Vector3},
};

fn fixture() -> (Scene, Handle, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "CONSTRUCTION",
        Vector3::ZERO,
        vec![
            EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::UNIT_X)),
            EntityType::Circle(Circle::from_center_radius(Vector3::new(5., 0., 0.), 1.)),
        ],
    );
    let record = scene.document.block_records.get("CONSTRUCTION").unwrap();
    let (definition, handles) = (record.handle, record.entity_handles.clone());
    (scene, definition, handles)
}

#[test]
fn block_construction_roundtrip_revert_preserves_geometry_and_unrelated_metadata() {
    let (mut scene, definition, handles) = fixture();
    let doc = &mut scene.document;
    super::super::view::dispatch::set_entity_xdata(
        doc,
        handles[0],
        "OCS_TEST_DATA",
        Some(vec![XDataValue::Integer32(37)]),
    );
    doc.get_entity_mut(handles[1])
        .unwrap()
        .common_mut()
        .invisible = true;
    let original = doc.clone();
    assert_eq!(set(doc, definition, &handles, true, "0").unwrap(), 2);
    let changed = doc.clone();
    assert_eq!(set(doc, definition, &handles, true, "0").unwrap(), 0);
    assert_eq!(*doc, changed);
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
        let mut loaded =
            crate::io::load_bytes(&format!("construction.{extension}"), bytes).unwrap();
        for handle in &handles {
            let entity = loaded.get_entity(*handle).unwrap();
            assert!(is_construction(entity.common()));
            assert!(entity.common().invisible);
        }
        assert_eq!(
            set(&mut loaded, definition, &handles, false, "0").unwrap(),
            2
        );
        for handle in &handles {
            let entity = loaded.get_entity(*handle).unwrap();
            assert!(!is_construction(entity.common()));
            assert_eq!(
                entity.common().invisible,
                original.get_entity(*handle).unwrap().common().invisible
            );
            assert_eq!(
                entity.as_entity().bounding_box(),
                original
                    .get_entity(*handle)
                    .unwrap()
                    .as_entity()
                    .bounding_box()
            );
        }
        assert_eq!(
            loaded
                .get_entity(handles[0])
                .unwrap()
                .common()
                .extended_data
                .get_record("OCS_TEST_DATA")
                .unwrap()
                .values,
            vec![XDataValue::Integer32(37)]
        );
        let bytes = crate::io::save_to_bytes(&loaded, extension, loaded.version).unwrap();
        let reopened = crate::io::load_bytes(&format!("reverted.{extension}"), bytes).unwrap();
        assert!(!is_construction(
            reopened.get_entity(handles[0]).unwrap().common()
        ));
    }
    let mut layer = document::tables::Layer::new("RESTORED");
    layer.handle = doc.allocate_handle();
    doc.layers.add(layer).unwrap();
    assert_eq!(
        set(doc, definition, &handles, false, "RESTORED").unwrap(),
        2
    );
    assert!(handles
        .iter()
        .all(|h| doc.get_entity(*h).unwrap().common().layer == "RESTORED"));
}

#[test]
fn block_construction_is_atomic_and_rejects_foreign_locked_or_unknown_records() {
    let (mut scene, def, handles) = fixture();
    let foreign = scene
        .document
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        )))
        .unwrap();
    let original = scene.document.clone();
    assert!(set(&mut scene.document, def, &[handles[0], foreign], true, "0").is_err());
    assert_eq!(scene.document, original);
    scene.document.layers.get_mut("0").unwrap().flags.locked = true;
    let original = scene.document.clone();
    assert!(set(&mut scene.document, def, &handles, true, "0").is_err());
    assert_eq!(scene.document, original);
    scene.document.layers.get_mut("0").unwrap().flags.locked = false;
    set(&mut scene.document, def, &handles, true, "0").unwrap();
    let before = scene.document.clone();
    assert!(set(&mut scene.document, def, &handles, false, "MISSING").is_err());
    assert_eq!(scene.document, before);
    super::super::view::dispatch::set_entity_xdata(
        &mut scene.document,
        handles[1],
        APP,
        Some(vec![XDataValue::Integer16(2)]),
    );
    let before = scene.document.clone();
    assert!(set(&mut scene.document, def, &handles, false, "0").is_err());
    assert_eq!(scene.document, before);
    assert!(is_construction(
        scene.document.get_entity(handles[1]).unwrap().common()
    ));
}

fn visible(scene: &Scene, root: Handle, editor: bool) -> Vec<Handle> {
    let depths = Default::default();
    let reveal = scene.visibility_reveal_handles();
    let graph = render_graph::RenderSceneGraph::new(&scene.document, None, None, true, &depths)
        .with_visibility_reveal(&reveal);
    let mut handles = vec![];
    graph.walk_root(
        render_graph::SceneRoot::Block(render_graph::BlockRoot {
            record: root,
            role: if editor {
                render_graph::BlockRootRole::DefinitionEdit
            } else {
                render_graph::BlockRootRole::ModelSpace
            },
        }),
        |_, _| true,
        |entity, _| handles.push(entity.common().handle),
    );
    handles
}

#[test]
fn block_construction_display_is_editor_only_and_properties_are_protected() {
    let (mut scene, def, handles) = fixture();
    let insert = scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "CONSTRUCTION",
            Vector3::ZERO,
        )))
        .unwrap();
    let model = scene
        .document
        .get_entity(insert)
        .unwrap()
        .common()
        .owner_handle;
    set(&mut scene.document, def, &[handles[0]], true, "0").unwrap();
    assert!(!visible(&scene, model, false).contains(&handles[0]));
    scene.block_edit_block = Some(def);
    assert!(visible(&scene, def, true).contains(&handles[0]));
    assert!(
        !visible(&scene, model, false).contains(&handles[0]),
        "editor reveal cannot leak through references"
    );
    let common = scene.document.get_entity(handles[0]).unwrap().common();
    assert!(!scene.display_entity_invisible(common));
    let style =
        super::super::view::render::render_style_for_common_viewport(&scene.document, common, None);
    assert_eq!(style.0, [0.55, 0.55, 0.55, 1.]);
    assert_eq!(style.2[..2], [4., -4.]);
    scene.set_construction_shown(def, false);
    scene.set_block_visibility_reveal(true);
    assert!(
        !visible(&scene, def, true).contains(&handles[0]),
        "visibility mode cannot reveal hidden construction"
    );
    assert!(scene.display_entity_invisible(scene.document.get_entity(handles[0]).unwrap().common()));
    scene.set_construction_shown(def, true);
    assert!(visible(&scene, def, true).contains(&handles[0]));
    let entity = scene.document.get_entity_mut(handles[0]).unwrap();
    let before = entity.clone();
    super::super::view::dispatch::apply_color(entity, Color::Index(2));
    super::super::view::dispatch::apply_named_color(entity, Color::Index(3), Some("test".into()));
    super::super::view::dispatch::apply_common_prop(entity, "layer", "MISSING");
    super::super::view::dispatch::apply_common_prop(entity, "linetype", "DASHED");
    assert_eq!(*entity, before);
}

#[test]
fn block_construction_survives_visibility_actions_refresh_and_explode() {
    use crate::scene::{
        block_actions as actions, block_point_parameter as point, block_visibility as visibility,
    };
    let (mut scene, def, handles) = fixture();
    let insert = scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "CONSTRUCTION",
            Vector3::ZERO,
        )))
        .unwrap();
    let parameter =
        point::create(&mut scene.document, def, Vector3::ZERO, Vector3::UNIT_Y).unwrap();
    actions::create(
        &mut scene.document,
        def,
        actions::Kind::Move,
        parameter,
        &[handles[0]],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let visibility = visibility::create(&mut scene.document, def, Vector3::UNIT_Y).unwrap();
    set(&mut scene.document, def, &[handles[0]], true, "0").unwrap();
    visibility::apply_state(&mut scene.document, visibility, 0);
    assert!(
        scene
            .document
            .get_entity(handles[0])
            .unwrap()
            .common()
            .invisible
    );
    let evaluated = actions::instance::evaluate(
        &scene.document,
        insert,
        parameter,
        glam::DVec3::new(3., 4., 0.),
    )
    .unwrap()
    .unwrap();
    scene.document = actions::instance::refresh(&evaluated, def).unwrap();
    let EntityType::Insert(reference) = scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let representation = scene
        .document
        .block_records
        .get(&reference.block_name)
        .unwrap();
    let member = representation
        .entity_handles
        .iter()
        .filter_map(|h| scene.document.get_entity(*h))
        .find(|e| is_construction(e.common()))
        .expect("construction classification follows evaluated member");
    assert!(member.common().invisible);
    let exploded = crate::modules::draw::modify::explode::explode_entity(
        &EntityType::Insert(reference.clone()),
        &scene.document,
    );
    assert_eq!(exploded.len(), 1);
    assert!(matches!(exploded[0], EntityType::Circle(_)));
    set(&mut scene.document, def, &[handles[0]], false, "0").unwrap();
    scene.document = actions::instance::refresh(&scene.document, def).unwrap();
    let EntityType::Insert(reference) = scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let exploded = crate::modules::draw::modify::explode::explode_entity(
        &EntityType::Insert(reference.clone()),
        &scene.document,
    );
    assert_eq!(exploded.len(), 2);
}

#[test]
fn block_construction_compound_geometry_overrides_child_styles_without_editing_children() {
    let mut scene = Scene::new();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.color = Color::Index(1);
    scene.define_block_raw("INNER", Vector3::ZERO, vec![EntityType::Line(line)]);
    scene.define_block_raw(
        "OUTER",
        Vector3::ZERO,
        vec![EntityType::Insert(Insert::new("INNER", Vector3::ZERO))],
    );
    let record = scene.document.block_records.get("OUTER").unwrap();
    let (definition, handle) = (record.handle, record.entity_handles[0]);
    let child = scene
        .document
        .block_records
        .get("INNER")
        .unwrap()
        .entity_handles[0];
    let before = scene.document.get_entity(child).unwrap().clone();
    set(&mut scene.document, definition, &[handle], true, "0").unwrap();
    scene.block_edit_block = Some(definition);
    let wires = scene.wire_models_for(&[handle]);
    assert!(!wires.is_empty());
    for wire in wires {
        assert_eq!(wire.color, [0.55, 0.55, 0.55, 1.]);
        assert_eq!(wire.pattern[..2], [4., -4.]);
        assert!(wire.stub.is_none());
    }
    assert_eq!(scene.document.get_entity(child).unwrap(), &before);
}
