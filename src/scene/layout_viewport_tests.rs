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
