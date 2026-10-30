use super::{Message, OpenCADStudio};
use crate::scene::model::object::{PropValue, Property};
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use codec::{Document, EntityType};
use std::io::Cursor;

fn fixture() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "PROPERTIES",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("PROPERTIES", Vector3::ZERO)))
        .unwrap();
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    app.refresh_properties();
    app
}

fn row(app: &OpenCADStudio, field: &str) -> Property {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == field)
        .unwrap()
        .clone()
}

fn edit(app: &mut OpenCADStudio, field: &str, value: &str) {
    let property = row(app, field);
    let _ = app
        .set_property_value(property, &serde_json::json!(value))
        .unwrap();
    app.finish_pending_history(app.active_tab);
}

#[test]
fn block_properties_extend_the_existing_panel_only_for_empty_editor_selection() {
    let mut app = fixture();
    let i = app.active_tab;
    let sections = &app.tabs[i].properties.sections;
    assert_eq!(app.tabs[i].properties.title, "No selection");
    let block = sections
        .iter()
        .position(|section| section.title == "Block")
        .unwrap();
    assert_eq!(sections[block - 1].title, "Misc");
    assert_eq!(
        sections[block]
            .props
            .iter()
            .map(|property| property.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Block name",
            "Annotative",
            "Match orientation to layout",
            "Scale uniformly",
            "Allow exploding",
            "Units",
            "Description",
            "Hyperlink"
        ]
    );
    assert_eq!(
        row(&app, "bedit_block_name").value,
        PropValue::ReadOnly("PROPERTIES".into())
    );
    let handle = app.tabs[i]
        .scene
        .document
        .entities_in_block("PROPERTIES")
        .find(|entity| matches!(entity, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle;
    app.tabs[i].scene.selected.insert(handle);
    app.refresh_properties();
    assert!(
        !app.tabs[i].properties.sections.iter().any(|section| section
            .props
            .iter()
            .any(|property| property.field.starts_with("bedit_")))
    );
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged {
        field: "bedit_allow_exploding",
        value: "No".into(),
    });
    assert_eq!(app.tabs[i].scene.document, before);
    app.tabs[i].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT_DISCARD");
    app.refresh_properties();
    assert!(!app.tabs[i]
        .properties
        .sections
        .iter()
        .any(|section| section.title == "Block"));
}

#[test]
fn block_properties_edits_have_local_undo_redo_and_discard() {
    let mut app = fixture();
    let i = app.active_tab;
    let original = app.tabs[i].scene.document.clone();
    edit(&mut app, "bedit_units", "Meters");
    edit(&mut app, "bedit_scale_uniformly", "Yes");
    edit(&mut app, "bedit_allow_exploding", "No");
    edit(&mut app, "bedit_description", "Door = 2 + 3");
    edit(&mut app, "bedit_hyperlink", "https://example.com/door");
    let edited = app.tabs[i].scene.document.clone();
    let record = edited.block_records.get("PROPERTIES").unwrap();
    assert_eq!(record.units, 6);
    assert!(record.scale_uniformly);
    assert!(!record.explodable);
    assert_eq!(record.description, "Door = 2 + 3");
    assert_eq!(
        crate::scene::block_metadata::hyperlink(&edited, "PROPERTIES"),
        ("https://example.com/door".into(), String::new())
    );
    assert!(app.block_edit_dirty(i));
    app.undo_steps(5);
    assert_eq!(
        app.tabs[i].scene.document.block_records,
        original.block_records
    );
    assert!(!app.block_edit_dirty(i));
    app.redo_steps(5);
    assert_eq!(app.tabs[i].scene.document, edited);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(app.tabs[i].scene.document, original);
}

#[test]
fn block_properties_annotation_gates_orientation_and_preserves_unrelated_xdata() {
    let mut app = fixture();
    let i = app.active_tab;
    let definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("PROPERTIES")
        .unwrap()
        .handle;
    let mut data = codec::xdata::ExtendedDataRecord::new("EXTRA_TAG");
    data.values = vec![codec::xdata::XDataValue::String("retained".into())];
    app.tabs[i]
        .scene
        .document
        .set_object_xdata_record(definition, data.clone());
    assert!(matches!(
        row(&app, "bedit_match_orientation").value,
        PropValue::ReadOnlyWithTooltip { .. }
    ));
    let before = app.tabs[i].scene.document.clone();
    let _ = app.set_block_property("bedit_match_orientation", "Yes");
    assert_eq!(app.tabs[i].scene.document, before);
    edit(&mut app, "bedit_annotative", "Yes");
    edit(&mut app, "bedit_match_orientation", "Yes");
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&app.tabs[i].scene.document, "PROPERTIES"),
        (true, true)
    );
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .object_xdata(definition)
            .unwrap()
            .get_record("EXTRA_TAG"),
        Some(&data)
    );
    edit(&mut app, "bedit_annotative", "No");
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&app.tabs[i].scene.document, "PROPERTIES"),
        (false, false)
    );
    app.undo_steps(1);
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&app.tabs[i].scene.document, "PROPERTIES"),
        (true, true)
    );
    assert!(matches!(
        row(&app, "bedit_match_orientation").value,
        PropValue::Choice { .. }
    ));
}

#[test]
fn block_properties_noop_invalid_and_readonly_edits_do_not_create_history() {
    let mut app = fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    edit(&mut app, "bedit_units", "Unitless");
    let _ = app.set_block_property("bedit_units", "Bogus units");
    assert!(app
        .set_property_value(row(&app, "bedit_block_name"), &serde_json::json!("RENAMED"))
        .is_err());
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i]
        .scene
        .document
        .block_records
        .get_mut("PROPERTIES")
        .unwrap()
        .units = 99;
    app.refresh_properties();
    edit(&mut app, "bedit_units", "Unknown");
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("PROPERTIES")
            .unwrap()
            .units,
        99
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn block_properties_save_checkpoint_and_main_undo_restore_definition_metadata() {
    let mut app = fixture();
    let i = app.active_tab;
    let original = app.tabs[i].scene.document.clone();
    edit(&mut app, "bedit_units", "Millimeters");
    edit(&mut app, "bedit_annotative", "Yes");
    edit(&mut app, "bedit_match_orientation", "Yes");
    let _ = app.dispatch_command("BSAVE");
    assert!(!app.block_edit_dirty(i));
    let saved = app.tabs[i].scene.document.clone();
    edit(&mut app, "bedit_description", "discard this edit");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(
        app.tabs[i].scene.document.block_records,
        saved.block_records
    );
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&app.tabs[i].scene.document, "PROPERTIES"),
        (true, true)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.block_records,
        original.block_records
    );
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&app.tabs[i].scene.document, "PROPERTIES"),
        (false, false)
    );
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.block_records,
        saved.block_records
    );
}

#[test]
fn block_properties_hyperlink_editor_targets_definition_without_selecting_it() {
    let mut app = fixture();
    let i = app.active_tab;
    let definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("PROPERTIES")
        .unwrap()
        .handle;
    let _ = app.update(Message::PropHyperlinkOpen);
    assert!(app.hyperlink_editor_handles.is_empty());
    assert_eq!(
        app.hyperlink_editor_block,
        Some((app.tabs[i].id, definition))
    );
    let _ = app.update(Message::HyperlinkUrlChanged(
        "https://example.com/link".into(),
    ));
    let _ = app.update(Message::HyperlinkDescriptionChanged(
        "Definition link".into(),
    ));
    let _ = app.update(Message::HyperlinkApply);
    app.finish_pending_history(i);
    assert!(app.tabs[i].scene.selected_entities().is_empty());
    assert_eq!(
        crate::scene::block_metadata::hyperlink(&app.tabs[i].scene.document, "PROPERTIES"),
        ("https://example.com/link".into(), "Definition link".into())
    );
    assert!(app.hyperlink_editor_block.is_none());
    let _ = app.update(Message::PropHyperlinkOpen);
    let _ = app.update(Message::HyperlinkRemove);
    app.finish_pending_history(i);
    assert_eq!(
        crate::scene::block_metadata::hyperlink(&app.tabs[i].scene.document, "PROPERTIES"),
        (String::new(), String::new())
    );
    app.undo_steps(1);
    assert!(
        matches!(row(&app, "bedit_hyperlink").value, PropValue::Hyperlink(ref value) if value == "https://example.com/link")
    );
}

fn check_round_trip(doc: &Document) {
    let record = doc.block_records.get("PROPERTIES").unwrap();
    assert_eq!(record.units, 4);
    assert!(record.scale_uniformly);
    assert!(!record.explodable);
    assert_eq!(record.description, "stored description");
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(doc, "PROPERTIES"),
        (true, true)
    );
    assert_eq!(
        crate::scene::block_metadata::hyperlink(doc, "PROPERTIES"),
        ("https://example.com/stored".into(), String::new())
    );
}

#[test]
fn block_properties_metadata_survives_dwg_and_dxf_save_and_reopen() {
    let mut app = fixture();
    for (field, value) in [
        ("bedit_units", "Millimeters"),
        ("bedit_scale_uniformly", "Yes"),
        ("bedit_allow_exploding", "No"),
        ("bedit_description", "stored description"),
        ("bedit_hyperlink", "https://example.com/stored"),
        ("bedit_annotative", "Yes"),
        ("bedit_match_orientation", "Yes"),
    ] {
        edit(&mut app, field, value);
    }
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let doc = &app.tabs[app.active_tab].scene.document;
    let bytes = codec::DwgWriter::write_to_vec(doc).unwrap();
    let read = codec::DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap();
    check_round_trip(&read);
    let mut changed = read.clone();
    let definition = changed.block_records.get("PROPERTIES").unwrap().handle;
    crate::scene::block_metadata::set_annotation_flags(&mut changed, definition, false, false);
    crate::scene::block_metadata::set_hyperlink(
        &mut changed,
        definition,
        "https://example.com/replaced",
        "Replaced",
    );
    let reopened = codec::DwgReader::from_stream(Cursor::new(
        codec::DwgWriter::write_to_vec(&changed).unwrap(),
    ))
    .read()
    .unwrap();
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&reopened, "PROPERTIES"),
        (false, false)
    );
    assert_eq!(
        crate::scene::block_metadata::hyperlink(&reopened, "PROPERTIES"),
        ("https://example.com/replaced".into(), "Replaced".into())
    );
    let bytes = codec::DxfWriter::new(doc).write_to_vec().unwrap();
    let read = codec::DxfReader::from_reader(Cursor::new(bytes))
        .unwrap()
        .read()
        .unwrap();
    check_round_trip(&read);
}

#[test]
fn block_properties_creation_options_persist_orientation_on_the_definition() {
    let mut scene = crate::scene::Scene::new();
    let line = scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::ZERO,
        Vector3::UNIT_X,
    )));
    let mut options = crate::scene::CreateBlockOptions {
        name: "CREATED".into(),
        handles: vec![line],
        base_point: glam::DVec3::ZERO,
        world_to_block: codec::types::Transform::identity(),
        block_to_world: codec::types::Transform::identity(),
        mode: crate::ui::window::block_definition::BlockObjectMode::Retain,
        annotative: true,
        match_orientation: true,
        scale_uniformly: true,
        allow_exploding: true,
        unit: 0,
        description: String::new(),
        hyperlink_url: "https://example.com/created".into(),
        hyperlink_desc: "Created definition".into(),
        redefine: false,
    };
    scene.create_block_with_options(options.clone()).unwrap();
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&scene.document, "CREATED"),
        (true, true)
    );
    let bytes = codec::DwgWriter::write_to_vec(&scene.document).unwrap();
    let read = codec::DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap();
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&read, "CREATED"),
        (true, true)
    );
    assert_eq!(
        crate::scene::block_metadata::hyperlink(&read, "CREATED"),
        (
            "https://example.com/created".into(),
            "Created definition".into()
        )
    );
    options.redefine = true;
    options.annotative = false;
    options.hyperlink_url.clear();
    options.hyperlink_desc.clear();
    scene.create_block_with_options(options).unwrap();
    assert_eq!(
        crate::scene::block_metadata::annotation_flags(&scene.document, "CREATED"),
        (false, false)
    );
    assert_eq!(
        crate::scene::block_metadata::hyperlink(&scene.document, "CREATED"),
        (String::new(), String::new())
    );
}

#[test]
fn block_properties_hyperlink_cancel_and_stale_context_do_not_modify_a_definition() {
    let mut app = fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropHyperlinkOpen);
    let _ = app.update(Message::HyperlinkUrlChanged(
        "https://example.com/cancelled".into(),
    ));
    let _ = app.update(Message::HyperlinkCancel);
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(app.hyperlink_editor_block.is_none());
    let _ = app.update(Message::PropHyperlinkOpen);
    let _ = app.update(Message::HyperlinkUrlChanged(
        "https://example.com/stale".into(),
    ));
    app.hyperlink_editor_block = Some((app.tabs[i].id + 1, codec::Handle::new(1)));
    let _ = app.update(Message::HyperlinkApply);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert!(app.hyperlink_editor_block.is_none());
}

#[test]
fn block_properties_staged_values_do_not_follow_a_different_definition() {
    use crate::ui::properties::FieldKey;
    let mut app = fixture();
    let i = app.active_tab;
    let key = FieldKey::Geom("bedit_description");
    app.tabs[i]
        .properties
        .edit_buf
        .insert(key.clone(), "uncommitted".into());
    app.tabs[i].properties.active_field = Some(key.clone());
    app.refresh_properties();
    assert_eq!(
        app.tabs[i]
            .properties
            .edit_buf
            .get(&key)
            .map(String::as_str),
        Some("uncommitted")
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
    app.tabs[i].scene.define_block_raw(
        "SECOND",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("SECOND", Vector3::ZERO)))
        .unwrap();
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    app.refresh_properties();
    assert!(!app.tabs[i].properties.edit_buf.contains_key(&key));
    assert_eq!(app.tabs[i].properties.active_field, None);
    let _ = app.commit_block_property("bedit_description");
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("SECOND")
            .unwrap()
            .description,
        ""
    );
}
