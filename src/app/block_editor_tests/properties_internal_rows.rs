use super::*;
use crate::scene::model::object::PropValue;
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use codec::{EntityType, Handle};

/// The fixture reference is given its own evaluated `*U` copy, the way a
/// visibility state change isolates an instance, and the definition carries a
/// non-unitless record.
fn anonymous_reference(app: &mut OpenCADStudio) -> Handle {
    let i = app.active_tab;
    let definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let parameter = {
        let doc = &mut app.tabs[i].scene.document;
        if let Some(record) = doc.block_records.get_mut("EDIT_ME") {
            record.units = 1;
        }
        let parameter = crate::scene::block_visibility::create(doc, definition, Vector3::ZERO)
            .expect("visibility parameter");
        crate::scene::block_visibility::add_state(doc, parameter, "Open", 0).unwrap();
        crate::scene::block_visibility::finish_editing(doc, definition);
        parameter
    };
    let _ = parameter;
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(0., 120., 0.),
        )))
        .unwrap();
    let copy = app.tabs[i]
        .scene
        .visibility_representation(reference)
        .expect("evaluated copy");
    assert!(copy.starts_with("*U"), "{copy}");
    reference
}

fn row(app: &OpenCADStudio, field: &str) -> String {
    let row = app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == field)
        .unwrap_or_else(|| panic!("no {field} row"));
    match &row.value {
        PropValue::ReadOnly(text) | PropValue::PlainText(text) | PropValue::EditText(text) => {
            text.clone()
        }
        PropValue::EditChoice { value, .. } => value.clone(),
        other => panic!("{field} is {other:?}"),
    }
}

#[test]
fn block_reference_properties_hide_internal_records_and_name_the_definition() {
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let sections = &app.tabs[i].properties.sections;
    assert!(
        !sections.iter().any(|section| section.title == "Dynamic Block"
            || section.title == "Dynamic Parameters / Actions"),
        "{:?}",
        sections.iter().map(|s| &s.title).collect::<Vec<_>>()
    );
    assert!(
        !sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| property.label.starts_with("Definition ")
                || property.label == "Native records"),
        "internal definition rows leak into the panel"
    );
    assert_eq!(row(&app, "block"), "EDIT_ME");
    assert_eq!(row(&app, "block_unit"), "Inches");
}

/// The evaluated copy carries a generated record name, but the Name row keeps
/// naming its definition and still offers it; re-picking that name asks for no
/// change, so the copy stays evaluated.
#[test]
fn block_reference_name_row_stays_editable_through_its_definition() {
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let property = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == "block")
        .expect("no block row");
    let (value, options) = match &property.value {
        PropValue::EditChoice { value, options } => (value.clone(), options.clone()),
        other => panic!("block row is {other:?}"),
    };
    assert_eq!(value, "EDIT_ME");
    assert!(
        options.iter().any(|option| option == "EDIT_ME"),
        "{options:?}"
    );
    let evaluated = match app.tabs[i].scene.document.get_entity(reference) {
        Some(EntityType::Insert(insert)) => insert.block_name.clone(),
        _ => panic!("the reference vanished"),
    };
    assert!(evaluated.starts_with("*U"), "{evaluated}");
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "block",
        value: "EDIT_ME".into(),
    });
    let doc = &app.tabs[i].scene.document;
    let Some(EntityType::Insert(insert)) = doc.get_entity(reference) else {
        panic!("the reference vanished");
    };
    assert_eq!(insert.block_name, evaluated);
    assert!(
        doc.dynamic_definition_for_insert(reference).is_some(),
        "re-picking the shown name dropped the evaluated link"
    );
}

/// Typing a name renames the definition the evaluated copy came from, and the
/// copy keeps its own record.
#[test]
fn block_reference_name_typing_renames_the_authored_definition() {
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    app.tabs[i].properties.edit_buf.insert(
        crate::ui::properties::FieldKey::Geom("block"),
        "RENAMED".into(),
    );
    let _ = app.update(Message::PropGeomCommit("block"));
    let doc = &app.tabs[i].scene.document;
    assert!(
        doc.block_records.get("RENAMED").is_some(),
        "the definition was not renamed"
    );
    let Some(EntityType::Insert(insert)) = doc.get_entity(reference) else {
        panic!("the reference vanished");
    };
    assert!(insert.block_name.starts_with("*U"), "{}", insert.block_name);
    assert_eq!(
        crate::app::control::block_ops::resolve_effective_block(doc, insert).0,
        "RENAMED"
    );
    let doc = doc.clone();
    use std::io::Cursor;
    for saved in [
        codec::DxfReader::from_reader(Cursor::new(
            codec::DxfWriter::new(&doc).write_to_vec().unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap(),
        codec::DwgReader::from_stream(Cursor::new(codec::DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
    ] {
        assert!(
            saved.block_records.get("RENAMED").is_some(),
            "the reopened drawing lost the renamed definition"
        );
        let Some(EntityType::Insert(insert)) = saved.get_entity(reference) else {
            panic!("the reference vanished from the reopened drawing");
        };
        assert_eq!(
            crate::app::control::block_ops::resolve_effective_block(&saved, insert).0,
            "RENAMED"
        );
    }
}

/// Picking another definition drops the evaluated-copy link, so the units and
/// dynamic rows answer for the block the reference now names.
#[test]
fn block_reference_name_choice_drops_the_evaluated_link() {
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "OTHER",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    if let Some(record) = app.tabs[i].scene.document.block_records.get_mut("OTHER") {
        record.units = 6;
    }
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    assert_eq!(row(&app, "block_unit"), "Inches");
    let evaluated = match app.tabs[i].scene.document.get_entity(reference) {
        Some(EntityType::Insert(insert)) => insert.block_name.clone(),
        _ => panic!("the reference vanished"),
    };
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "block",
        value: "OTHER".into(),
    });
    app.refresh_properties();
    let doc = &app.tabs[i].scene.document;
    let Some(EntityType::Insert(insert)) = doc.get_entity(reference) else {
        panic!("the reference vanished");
    };
    assert_eq!(insert.block_name, "OTHER");
    assert_eq!(
        crate::app::control::block_ops::resolve_effective_block(doc, insert).0,
        "OTHER"
    );
    assert!(
        !doc.block_records
            .get(&evaluated)
            .expect("the evaluated record")
            .insert_handles
            .contains(&reference),
        "the copy still claims the reference"
    );
    assert!(
        doc.block_records
            .get("OTHER")
            .expect("the target record")
            .insert_handles
            .contains(&reference),
        "the target does not claim the reference"
    );
    assert_eq!(row(&app, "block"), "OTHER");
    assert_eq!(row(&app, "block_unit"), "Meters");
}

/// Data the panel never touches must survive a re-point: an unrelated XDATA
/// record and an unrelated extension-dictionary entry.
fn add_unrelated_data(app: &mut OpenCADStudio, reference: Handle) -> Handle {
    use codec::objects::{Dictionary, ObjectType};
    use codec::xdata::{ExtendedDataRecord, XDataValue};
    let doc = &mut app.tabs[app.active_tab].scene.document;
    if let Some(EntityType::Insert(insert)) = doc.get_entity_mut(reference) {
        let mut record = ExtendedDataRecord::new("OCS_KEEP");
        record.add_value(XDataValue::String("keep".into()));
        insert.common.extended_data.add_record(record);
    }
    let holder = doc.ensure_extension_dictionary(reference);
    let handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = holder;
    doc.objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(holder)) = doc.objects.get_mut(&holder) {
        holder.add_entry("OCS_KEEP", handle);
    }
    handle
}

/// Typing an existing definition's name takes the same re-point path as the
/// pick, leaving unrelated reference data in place.
#[test]
fn block_reference_name_typing_an_existing_block_repoints_the_reference() {
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "OTHER",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    if let Some(record) = app.tabs[i].scene.document.block_records.get_mut("OTHER") {
        record.units = 6;
    }
    let sibling = add_unrelated_data(&mut app, reference);
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    app.tabs[i].properties.edit_buf.insert(
        crate::ui::properties::FieldKey::Geom("block"),
        "OTHER".into(),
    );
    let _ = app.update(Message::PropGeomCommit("block"));
    let doc = &app.tabs[i].scene.document;
    let Some(EntityType::Insert(insert)) = doc.get_entity(reference) else {
        panic!("the reference vanished");
    };
    assert_eq!(insert.block_name, "OTHER");
    assert_eq!(
        crate::app::control::block_ops::resolve_effective_block(doc, insert).0,
        "OTHER"
    );
    assert_eq!(
        doc.dynamic_definition_for_insert(reference),
        doc.block_records.get("OTHER").map(|record| record.handle),
        "the reference still resolves through the evaluated definition"
    );
    assert!(
        insert.common.extended_data.get_record("OCS_KEEP").is_some(),
        "the re-point dropped unrelated XDATA"
    );
    let holder = doc
        .extension_dictionary_handle(reference)
        .expect("the reference dictionary");
    let Some(codec::objects::ObjectType::Dictionary(holder)) = doc.objects.get(&holder) else {
        panic!("the reference dictionary vanished");
    };
    assert!(
        holder
            .entries
            .iter()
            .any(|(key, handle)| key == "OCS_KEEP" && *handle == sibling),
        "the re-point dropped an unrelated dictionary entry"
    );
    assert!(
        !holder
            .entries
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("AcDbBlockRepresentation")),
        "the evaluation link is still stored on the reference"
    );
    assert_eq!(row(&app, "block"), "OTHER");
    assert_eq!(row(&app, "block_unit"), "Meters");
}

/// The unlink must survive a save and reopen: both readers rebuild the
/// evaluation link from the objects it leaves behind.
#[test]
fn block_reference_name_pick_survives_a_save_and_reopen() {
    use std::io::Cursor;
    let (mut app, _) = fixture();
    let reference = anonymous_reference(&mut app);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "OTHER",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    app.tabs[i].scene.selected.clear();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "block",
        value: "OTHER".into(),
    });
    let live = &app.tabs[i].scene.document;
    assert_eq!(
        live.dynamic_definition_for_insert(reference),
        live.block_records.get("OTHER").map(|record| record.handle),
        "the pick left the reference on the evaluated definition"
    );
    let doc = app.tabs[i].scene.document.clone();
    for saved in [
        codec::DxfReader::from_reader(Cursor::new(
            codec::DxfWriter::new(&doc).write_to_vec().unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap(),
        codec::DwgReader::from_stream(Cursor::new(codec::DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
    ] {
        let Some(EntityType::Insert(insert)) = saved.get_entity(reference) else {
            panic!("the reference vanished from the reopened drawing");
        };
        assert_eq!(insert.block_name, "OTHER");
        assert_eq!(
            crate::app::control::block_ops::resolve_effective_block(&saved, insert).0,
            "OTHER"
        );
        assert_eq!(
            saved.dynamic_definition_for_insert(reference),
            saved.block_records.get("OTHER").map(|record| record.handle),
            "the reopened drawing restored the evaluation link"
        );
    }
}
