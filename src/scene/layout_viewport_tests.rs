use super::*;
use document::entities::Viewport;

fn viewport_fixture(sheet_id: i16, content_id: i16, listed: bool) -> (Scene, Handle, Handle) {
    let mut scene = Scene::new();
    scene.document.add_layout("Sheet").expect("unique layout");
    scene.set_current_layout("Sheet".to_string());

    let sheet = scene.current_layout_sheet_viewport_handle();
    let Some(EntityType::Viewport(viewport)) = scene.document.get_entity_mut(sheet) else {
        panic!("new layout must have its overall sheet viewport");
    };
    viewport.id = sheet_id;
    let mut content = Viewport::new();
    content.id = content_id;
    let content = scene.add_entity(EntityType::Viewport(Box::new(content)));

    if listed {
        let layout_handle = scene.current_layout_object_handle().expect("layout object");
        let Some(ObjectType::Layout(layout)) = scene.document.objects.get_mut(&layout_handle)
        else {
            panic!("missing layout object");
        };
        layout.viewports = vec![sheet, content];
    }
    (scene, sheet, content)
}

fn assert_sheet_order(scene: &Scene, sheet: Handle, content: Handle) {
    // The fixture mutates the layout's list directly after adding entities;
    // invalidate the derived cache just as the document mutation APIs do.
    scene.paper_viewport_cache.borrow_mut().clear();
    let layout = scene
        .document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Layout(layout) if layout.name == "Sheet" => Some(layout),
            _ => None,
        })
        .expect("paper layout");
    assert_eq!(
        Scene::layout_sheet_viewport_handle(&scene.document, layout),
        sheet
    );
    for (handle, sheet_expected) in [(sheet, true), (content, false)] {
        let Some(EntityType::Viewport(viewport)) = scene.document.get_entity(handle) else {
            panic!("missing viewport");
        };
        assert_eq!(
            Scene::is_sheet_viewport(&scene.document, viewport),
            sheet_expected,
            "viewport {handle:?}"
        );
    }
    let (block, resolved_sheet, content_handles) = scene.paper_viewport_handles();
    assert_eq!(block, layout.block_record);
    assert_eq!(resolved_sheet, sheet, "active layout's resolved sheet");
    assert_eq!(content_handles.as_slice(), &[content]);
}

#[test]
fn ordered_sheet_viewport_wins_over_reversed_ids() {
    // The C.2 and D.2.2 repro layouts store their overall sheet first with
    // id=2, followed by their model-content viewport with id=1.
    for listed in [true, false] {
        let (scene, sheet, content) = viewport_fixture(2, 1, listed);
        assert_sheet_order(&scene, sheet, content);
    }
}

#[test]
fn ordered_sheet_viewport_keeps_normal_ids_unchanged() {
    for listed in [true, false] {
        let (scene, sheet, content) = viewport_fixture(1, 2, listed);
        assert_sheet_order(&scene, sheet, content);
    }
}

#[test]
fn layout_viewport_list_precedes_the_block_record_order() {
    let (mut scene, sheet, content) = viewport_fixture(2, 1, true);
    let block = scene.current_layout_block_handle();
    let record = scene
        .document
        .block_records
        .iter_mut()
        .find(|record| record.handle == block)
        .expect("layout block record");
    let sheet_index = record
        .entity_handles
        .iter()
        .position(|handle| *handle == sheet)
        .expect("sheet in block record");
    let content_index = record
        .entity_handles
        .iter()
        .position(|handle| *handle == content)
        .expect("content in block record");
    record.entity_handles.swap(sheet_index, content_index);

    // ensure_sheet_viewport inserts a repaired sheet at the front of this
    // semantic list; a pre-existing content viewport in the block must not
    // supersede it.
    assert_sheet_order(&scene, sheet, content);
}

#[test]
fn orphaned_viewports_keep_the_legacy_id_classification() {
    let document = Document::new();
    for (id, expected) in [(1, true), (2, false), (0, false), (-1, false)] {
        let mut viewport = Viewport::new();
        viewport.id = id;
        assert_eq!(Scene::is_sheet_viewport(&document, &viewport), expected);
    }
}

fn viewport_center(scene: &Scene, handle: Handle) -> (f64, f64) {
    let Some(EntityType::Viewport(viewport)) = scene.document.get_entity(handle) else {
        panic!("missing viewport");
    };
    (viewport.center.x, viewport.center.y)
}

fn viewport_count(scene: &Scene) -> usize {
    scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Viewport(_)))
        .count()
}

#[test]
fn select_all_skips_the_sheet_viewport() {
    let (mut scene, sheet, content) = viewport_fixture(1, 2, true);
    scene.select_all_visible();
    assert!(!scene.selected.contains(&sheet));
    assert!(scene.selected.contains(&content));
}

#[test]
fn erase_keeps_the_sheet_viewport() {
    let (mut scene, sheet, content) = viewport_fixture(1, 2, true);
    scene.erase_entities(&[sheet, content]);
    assert!(scene.document.get_entity(content).is_none());
    assert!(scene.document.get_entity(sheet).is_some());
    assert_eq!(scene.current_layout_sheet_viewport_handle(), sheet);
}

#[test]
fn exact_and_plugin_erase_keep_the_sheet_viewport() {
    let (mut scene, sheet, _) = viewport_fixture(1, 2, true);
    scene.erase_entities_exact(&[sheet]);
    assert!(scene.document.get_entity(sheet).is_some());
    scene.erase_entities_plugin(&[sheet]);
    assert!(scene.document.get_entity(sheet).is_some());
}

#[test]
fn transform_moves_the_content_viewport_not_the_sheet() {
    let (mut scene, sheet, content) = viewport_fixture(1, 2, true);
    let sheet_before = viewport_center(&scene, sheet);
    let content_before = viewport_center(&scene, content);
    scene.transform_entities(
        &[sheet, content],
        &EntityTransform::Translate(glam::DVec3::new(10.0, 20.0, 0.0)),
    );
    assert_eq!(viewport_center(&scene, sheet), sheet_before);
    let moved = viewport_center(&scene, content);
    assert_eq!(moved, (content_before.0 + 10.0, content_before.1 + 20.0));
}

#[test]
fn copy_duplicates_only_the_content_viewport() {
    let (mut scene, sheet, content) = viewport_fixture(1, 2, true);
    let before = viewport_count(&scene);
    let copies = scene.copy_entities(
        &[sheet, content],
        &EntityTransform::Translate(glam::DVec3::new(10.0, 0.0, 0.0)),
    );
    assert_eq!(copies.len(), 1);
    assert_eq!(viewport_count(&scene), before + 1);
}

#[test]
fn select_all_erase_then_new_viewport_keeps_its_view_on_save() {
    let (mut scene, sheet, _) = viewport_fixture(1, 2, true);
    scene.select_all_visible();
    let selected: Vec<Handle> = scene.selected.iter().copied().collect();
    scene.erase_entities(&selected);

    let mut viewport = Viewport::new();
    viewport.id = 3;
    viewport.view_center = document::types::Vector3::new(123.0, 45.0, 0.0);
    viewport.view_height = 77.0;
    let added = scene.add_entity(EntityType::Viewport(Box::new(viewport)));
    scene.paper_viewport_cache.borrow_mut().clear();

    assert_eq!(scene.current_layout_sheet_viewport_handle(), sheet);
    scene.sync_camera_to_document();

    let Some(EntityType::Viewport(viewport)) = scene.document.get_entity(added) else {
        panic!("missing viewport");
    };
    assert!(!Scene::is_sheet_viewport(&scene.document, viewport));
    assert_eq!(viewport.view_center.x, 123.0);
    assert_eq!(viewport.view_center.y, 45.0);
    assert_eq!(viewport.view_height, 77.0);
}
