//! Which annotative objects a layout shows when the current scale is not one of
//! theirs: Model shows them all unless the drawing says otherwise, a viewport on
//! a paper layout filters them unless that layout says otherwise, and an object
//! drawn directly on a sheet is never filtered.

use codec::entities::Text;
use codec::objects::{ObjectType, Scale};
use codec::types::Vector3;
use codec::{Document, EntityType, Handle};

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
    use codec::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

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

    let bytes =
        crate::io::save_to_bytes(&scene.document, "dxf", codec::DxfVersion::AC1027).expect("save");
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
