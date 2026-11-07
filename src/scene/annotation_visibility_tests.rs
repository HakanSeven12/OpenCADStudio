//! Which annotative objects a layout shows when the current scale is not one of
//! theirs: Model shows them all unless the drawing says otherwise, a viewport on
//! a paper layout filters them unless that layout says otherwise, and an object
//! drawn directly on a sheet is never filtered.

use document::entities::Text;
use document::objects::{ObjectType, Scale};
use document::types::Vector3;
use document::{Document, EntityType, Handle};

use crate::scene::annotative::{
    annotative_offscale_for, create_annotation_context, ensure_scale_object,
};
use crate::scene::Scene;

fn paper_layout_name(document: &Document) -> String {
    document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Layout(layout) if !layout.name.eq_ignore_ascii_case("Model") => {
                Some(layout.name.clone())
            }
            _ => None,
        })
        .expect("a new drawing has a paper layout")
}

fn scene_on(layout: Option<&str>) -> Scene {
    let mut scene = Scene::new();
    scene.document = Document::new();
    scene.current_layout = match layout {
        Some(name) => name.to_string(),
        None => paper_layout_name(&scene.document),
    };
    scene
}

#[test]
fn model_shows_every_annotative_object_unless_the_drawing_says_otherwise() {
    let mut scene = scene_on(Some("Model"));
    assert!(scene.annotation_all_visible());
    assert!(scene.document.set_drawing_variable("ANNOALLVISIBLE", "0"));
    assert!(!scene.annotation_all_visible());
}

#[test]
fn a_paper_layout_filters_annotative_objects_unless_it_says_otherwise() {
    let mut scene = scene_on(None);
    assert!(!scene.annotation_all_visible());
    // The drawing's variable belongs to Model.
    assert!(scene.document.set_drawing_variable("ANNOALLVISIBLE", "1"));
    assert!(!scene.annotation_all_visible());
}

#[test]
fn a_paper_layout_reads_its_value_from_its_xdata() {
    use document::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

    let mut scene = scene_on(None);
    let name = scene.current_layout.clone();
    let layout = scene
        .document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Layout(layout) if layout.name == name => Some(*handle),
            _ => None,
        })
        .unwrap();
    let mut record = ExtendedDataRecord::new("AcadAnnoAV");
    record.add_value(XDataValue::Integer16(1));
    let mut xdata = ExtendedData::new();
    xdata.add_record(record);
    scene.document.object_xdata.insert(layout, xdata);
    assert!(scene.annotation_all_visible());
}

#[test]
fn setting_the_value_on_a_layout_survives_a_dxf_save() {
    let mut scene = scene_on(None);
    let name = scene.current_layout.clone();
    scene.set_annotation_all_visible(true);
    assert!(scene.annotation_all_visible());

    let bytes = crate::io::save_to_bytes(&scene.document, "dxf", document::DxfVersion::AC1027)
        .expect("save");
    let reopened = crate::io::load_bytes("drawing.dxf", bytes).expect("open");
    let layout = reopened
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Layout(layout) if layout.name == name => Some(*handle),
            _ => None,
        })
        .unwrap();
    assert!(reopened
        .object_xdata
        .get(&layout)
        .and_then(|xdata| xdata.get_record("AcadAnnoAV"))
        .is_some());

    scene.set_annotation_all_visible(false);
    assert!(!scene.annotation_all_visible());
}

fn annotative_text(document: &mut Document, layout: Option<&str>, scale: Handle) -> Handle {
    let mut text = Text::new();
    text.value = "SO 1:50".to_string();
    text.insertion_point = Vector3::new(0.0, 0.0, 0.0);
    let entity = EntityType::Text(text);
    let handle = match layout {
        Some(layout) => document.add_entity_to_layout(entity, layout).unwrap(),
        None => document.add_entity(entity).unwrap(),
    };
    assert!(create_annotation_context(document, handle, scale));
    handle
}

#[test]
fn an_object_on_a_sheet_stays_when_the_layout_filters_the_model() {
    let mut document = Document::new();
    let layout = paper_layout_name(&document);
    let at_50 = ensure_scale_object(&mut document, &Scale::new("1:50", 1.0, 50.0));
    let at_1 = ensure_scale_object(&mut document, &Scale::new("1:1", 1.0, 1.0));
    let on_sheet = annotative_text(&mut document, Some(&layout), at_50);
    let in_model = annotative_text(&mut document, None, at_50);

    let offscale = |handle: Handle| {
        let common = document.get_entity(handle).unwrap().common();
        annotative_offscale_for(&document, common, Some(at_1), false)
    };
    assert!(!offscale(on_sheet));
    assert!(offscale(in_model));
}

/// An evaluated reference can retain scale contexts after its named dynamic
/// definition was made non-annotative.
fn block_with_retained_context(annotative: Option<bool>) -> (Scene, Handle, Handle, Handle) {
    use document::entities::{Insert, Line, Viewport};
    use document::objects::ObjectContextKind;
    use document::xdata::{ExtendedDataRecord, XDataValue};

    let mut scene = scene_on(None);
    for name in ["CONDUCTOR", "*U1"] {
        scene.define_block_raw(
            name,
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_coords(0., 0., 0., 2., 0., 0.))],
        );
    }
    let definition = scene
        .document
        .block_records
        .get("CONDUCTOR")
        .unwrap()
        .handle;
    if let Some(value) = annotative {
        super::block_metadata::set_annotation_flags(&mut scene.document, definition, value, false);
    }
    let representation = scene.document.block_records.get("*U1").unwrap().handle;
    let mut app = document::tables::AppId::new("AcDbBlockRepBTag");
    app.handle = scene.document.allocate_handle();
    scene.document.app_ids.add(app).unwrap();
    let mut tag = ExtendedDataRecord::new("AcDbBlockRepBTag");
    tag.add_value(XDataValue::Integer16(1));
    tag.add_value(XDataValue::Handle(definition));
    scene.document.set_object_xdata_record(representation, tag);

    let stored = ensure_scale_object(&mut scene.document, &Scale::new("1:50", 1., 50.));
    let displayed = ensure_scale_object(&mut scene.document, &Scale::new("1:25", 1., 25.));
    let insert = scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "*U1",
            Vector3::new(10., 10., 0.),
        )))
        .unwrap();
    assert!(create_annotation_context(
        &mut scene.document,
        insert,
        stored
    ));
    // Imported references can have context dictionaries without an entity-level
    // flag. The effective definition carries the explicit annotation state.
    super::view::dispatch::set_entity_xdata(&mut scene.document, insert, "AcadAnnotative", None);
    for object in scene.document.objects.values_mut() {
        if let ObjectType::ObjectContextData(context) = object {
            if let ObjectContextKind::BlkRef { insertion, .. } = &mut context.kind {
                *insertion = Vector3::new(80., 80., 0.);
            }
        }
    }

    let mut viewport = Viewport::new();
    viewport.id = 2;
    viewport.width = 100.;
    viewport.height = 100.;
    viewport.view_height = 100.;
    viewport.view_center = Vector3::new(50., 50., 0.);
    let viewport = scene
        .document
        .add_entity_to_layout(
            EntityType::Viewport(Box::new(viewport)),
            &scene.current_layout,
        )
        .unwrap();
    scene
        .document
        .set_viewport_annotation_scale(viewport, displayed);
    (scene, insert, viewport, displayed)
}

#[test]
fn a_non_annotative_dynamic_block_stays_visible_in_a_paper_viewport() {
    let (scene, insert, viewport, _) = block_with_retained_context(Some(false));
    assert!(!scene.annotation_all_visible());
    let wires = scene.model_wires_for_viewport_arc(viewport, 0.);
    assert!(wires
        .iter()
        .any(|wire| Scene::handle_from_wire_name(&wire.name) == Some(insert)));
}

#[test]
fn a_non_annotative_dynamic_block_keeps_its_stored_placement() {
    let (scene, insert, _, displayed) = block_with_retained_context(Some(false));
    let entity = scene.document.get_entity(insert).unwrap();
    assert!(!super::annotative::is_annotative(&scene.document, entity));
    let placed =
        super::annotative::entity_for_annotation_context(&scene.document, entity, Some(displayed));
    let EntityType::Insert(placed) = placed.as_ref() else {
        panic!("expected INSERT")
    };
    assert_eq!(placed.insert_point, Vector3::new(10., 10., 0.));
}

#[test]
fn active_and_unmarked_dynamic_block_contexts_still_filter_by_scale() {
    for flag in [Some(true), None] {
        let (scene, insert, viewport, displayed) = block_with_retained_context(flag);
        let entity = scene.document.get_entity(insert).unwrap();
        assert!(super::annotative::is_annotative(&scene.document, entity));
        assert!(annotative_offscale_for(
            &scene.document,
            entity.common(),
            Some(displayed),
            false
        ));
        assert!(!scene
            .model_wires_for_viewport_arc(viewport, 0.)
            .iter()
            .any(|wire| Scene::handle_from_wire_name(&wire.name) == Some(insert)));
    }
}

#[test]
fn an_explicitly_disabled_object_ignores_retained_scale_contexts() {
    use document::xdata::XDataValue;

    let mut document = Document::new();
    let stored = ensure_scale_object(&mut document, &Scale::new("1:50", 1., 50.));
    let displayed = ensure_scale_object(&mut document, &Scale::new("1:25", 1., 25.));
    let handle = annotative_text(&mut document, None, stored);
    let text = document.get_entity_mut(handle).unwrap();
    let mut record = text
        .common()
        .extended_data
        .get_record("AcadAnnotative")
        .unwrap()
        .clone();
    // Keep the dictionary intact, as files with obsolete contexts do.
    record.values[3] = XDataValue::Integer16(0);
    text.common_mut().extended_data.upsert_record(record);
    let text = document.get_entity(handle).unwrap();
    assert!(!annotative_offscale_for(
        &document,
        text.common(),
        Some(displayed),
        false
    ));
    assert!(
        super::annotative::active_object_context_for_scale(&document, handle, Some(displayed))
            .is_none()
    );
}

#[test]
fn non_annotative_dynamic_block_visibility_survives_dwg_and_dxf_roundtrips() {
    for format in ["dwg", "dxf"] {
        let (mut scene, insert, viewport, displayed) = block_with_retained_context(Some(false));
        let bytes = crate::io::save_to_bytes(&scene.document, format, document::DxfVersion::AC1032)
            .expect("save synthetic drawing");
        scene.document = crate::io::load_bytes(&format!("synthetic.{format}"), bytes)
            .expect("reopen synthetic drawing");
        let entity = scene.document.get_entity(insert).unwrap();
        // The obsolete data is preserved for round-trip fidelity, but has no
        // effect on display or placement while annotation scaling is off.
        assert!(!super::annotative::object_scale_memberships(&scene.document, insert).is_empty());
        assert!(
            !annotative_offscale_for(&scene.document, entity.common(), Some(displayed), false),
            "{format}: definition metadata must survive the round trip"
        );
        assert!(
            scene
                .model_wires_for_viewport_arc(viewport, 0.)
                .iter()
                .any(|wire| Scene::handle_from_wire_name(&wire.name) == Some(insert)),
            "{format}"
        );
    }
}

#[test]
fn attributes_of_a_non_annotative_dynamic_block_ignore_obsolete_placement() {
    use document::entities::AttributeEntity;
    use document::objects::ObjectContextKind;
    use document::types::Vector2;

    let (mut scene, insert, _, displayed) = block_with_retained_context(Some(false));
    let mut attribute = AttributeEntity::new("CIRCUIT".into(), "4".into());
    attribute.insertion_point = Vector3::new(11., 12., 0.);
    attribute.alignment_point = attribute.insertion_point;
    let handle = scene
        .document
        .add_entity(EntityType::AttributeEntity(attribute))
        .unwrap();
    assert!(create_annotation_context(
        &mut scene.document,
        handle,
        displayed
    ));
    super::view::dispatch::set_entity_xdata(&mut scene.document, handle, "AcadAnnotative", None);
    for object in scene.document.objects.values_mut() {
        if let ObjectType::ObjectContextData(context) = object {
            if let ObjectContextKind::MTextAttribute(context) = &mut context.kind {
                context.insertion = Vector2::new(90., 90.);
                context.alignment = context.insertion;
            }
        }
    }
    let EntityType::AttributeEntity(mut attribute) =
        scene.document.get_entity(handle).unwrap().clone()
    else {
        panic!("expected ATTRIB")
    };
    scene.document.remove_entity(handle);
    attribute.common.owner_handle = insert;
    assert!(!super::annotative::place_attribute_for_annotation_scale(
        &scene.document,
        &mut attribute,
        Some(displayed),
    ));
    assert_eq!(attribute.insertion_point, Vector3::new(11., 12., 0.));
}
