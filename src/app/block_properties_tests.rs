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

/// The `Allow exploding` row is a switch, not a label: a definition answered
/// `No` has to survive `EXPLODE` once the editor closes.
#[test]
fn block_properties_allow_exploding_refuses_the_explode_command() {
    let mut app = fixture();
    let i = app.active_tab;
    let insert = app.tabs[i]
        .scene
        .document
        .entities()
        .find(|entity| matches!(entity, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle;
    edit(&mut app, "bedit_allow_exploding", "No");
    let _ = app.dispatch_command("BEDIT_SAVE");
    app.tabs[i].scene.selected.insert(insert);
    let _ = app.dispatch_command("EXPLODE");
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(insert)
            .is_some(),
        "EXPLODE disassembled a definition the row says cannot be exploded"
    );
}

/// The switch has to outlive the drawing session: a file written with `No`
/// reloads as a definition `EXPLODE` still refuses.
#[test]
fn block_properties_allow_exploding_survives_the_saved_file() {
    for ext in ["dxf", "dwg"] {
        let mut app = fixture();
        let i = app.active_tab;
        edit(&mut app, "bedit_allow_exploding", "No");
        let _ = app.dispatch_command("BEDIT_SAVE");
        let bytes = crate::io::save_to_bytes(
            &app.tabs[i].scene.document,
            ext,
            app.tabs[i].scene.document.version,
        )
        .unwrap_or_else(|error| panic!("save {ext}: {error}"));
        let read = crate::io::load_bytes(&format!("properties.{ext}"), bytes)
            .unwrap_or_else(|error| panic!("load {ext}: {error}"));
        assert!(
            !read
                .block_records
                .get("PROPERTIES")
                .unwrap_or_else(|| panic!("{ext}: the definition is gone"))
                .explodable,
            "{ext}: `Allow exploding = No` did not survive the save"
        );
        let reloaded = read
            .entities()
            .find(|entity| matches!(entity, EntityType::Insert(_)))
            .unwrap()
            .common()
            .handle;
        app.tabs[i].scene.document = read;
        app.tabs[i].scene.selected.insert(reloaded);
        let _ = app.dispatch_command("EXPLODE");
        assert!(
            app.tabs[i]
                .scene
                .document
                .get_entity(reloaded)
                .is_some(),
            "{ext}: EXPLODE disassembled a definition reloaded as not explodable"
        );
    }
}

/// A definition that carries a parameter is drawn from a representation copy,
/// so the switch has to reach that record as well or it is inert exactly where
/// the block editor is used.
#[test]
fn block_properties_allow_exploding_refuses_explode_on_a_parametric_block() {
    let mut app = fixture();
    let i = app.active_tab;
    let definition = app.tabs[i]
        .active_block_edit_session()
        .expect("the fixture opens an editor session")
        .br_handle;
    let insert = app.tabs[i]
        .scene
        .document
        .entities()
        .find(|entity| matches!(entity, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,2 3,2");
    assert!(
        app.tabs[i].scene.document.is_dynamic_block(definition),
        "the test block has to be parametric for this to mean anything"
    );
    app.refresh_properties();
    edit(&mut app, "bedit_allow_exploding", "No");
    let _ = app.dispatch_command("BEDIT_SAVE");
    app.tabs[i].scene.selected.insert(insert);
    let _ = app.dispatch_command("EXPLODE");
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(insert)
            .is_some(),
        "EXPLODE disassembled a parametric definition the row says cannot be exploded"
    );
}

/// A reloaded insert can name a record other than the one the row edited, so
/// the refusal has to follow the record the insert actually names — and a
/// record that is empty would refuse for the wrong reason.
#[test]
fn block_properties_allow_exploding_refuses_explode_on_a_saved_parametric_block() {
    for ext in ["dxf", "dwg"] {
        let mut app = fixture();
        let i = app.active_tab;
        let definition = app.tabs[i]
            .active_block_edit_session()
            .expect("the fixture opens an editor session")
            .br_handle;
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,2 3,2");
        assert!(
            app.tabs[i].scene.document.is_dynamic_block(definition),
            "{ext}: the test block has to be parametric for this to mean anything"
        );
        app.refresh_properties();
        edit(&mut app, "bedit_allow_exploding", "No");
        let _ = app.dispatch_command("BEDIT_SAVE");
        let bytes = crate::io::save_to_bytes(
            &app.tabs[i].scene.document,
            ext,
            app.tabs[i].scene.document.version,
        )
        .unwrap_or_else(|error| panic!("save {ext}: {error}"));
        let read = crate::io::load_bytes(&format!("properties.{ext}"), bytes)
            .unwrap_or_else(|error| panic!("load {ext}: {error}"));
        let (reloaded, named, governed) = read
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(insert) => Some((
                    insert.common.handle,
                    insert.block_name.clone(),
                    read.block_records
                        .get(&insert.block_name)
                        .map(|record| record.entity_handles.len())
                        .unwrap_or(usize::MAX),
                )),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{ext}: the reloaded drawing has no insert"));
        assert!(
            governed > 0,
            "{ext}: the record `{named}` holds no objects, so there would be nothing to explode"
        );
        let record = read.block_records.get(&named).unwrap_or_else(|| {
            panic!("{ext}: the reloaded insert names `{named}`, which has no record")
        });
        assert!(
            !record.explodable,
            "{ext}: the reloaded insert names `{named}`, and that record still allows exploding"
        );
        app.tabs[i].scene.document = read;
        app.tabs[i].scene.selected.insert(reloaded);
        let _ = app.dispatch_command("EXPLODE");
        assert!(
            app.tabs[i]
                .scene
                .document
                .get_entity(reloaded)
                .is_some(),
            "{ext}: EXPLODE disassembled a parametric definition reloaded from a file that says it cannot be exploded"
        );
        // The same reference, the same document, only the flag flipped: what
        // stopped the explosion was the flag and nothing else about this file.
        app.tabs[i]
            .scene
            .document
            .block_records
            .get_mut(&named)
            .unwrap()
            .explodable = true;
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.selected.insert(reloaded);
        let _ = app.dispatch_command("EXPLODE");
        assert!(
            app.tabs[i].scene.document.get_entity(reloaded).is_none(),
            "{ext}: the flag was the only difference, yet the reference still refuses to come apart"
        );
    }
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

fn select_editor_line(app: &mut OpenCADStudio) -> codec::types::Handle {
    let i = app.active_tab;
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
    handle
}

fn block_parameter_key() -> crate::scene::model::object::BlockParameterKey {
    use crate::scene::model::object::{BlockParameterComponent, BlockParameterKey};
    BlockParameterKey {
        document: 0,
        definition: codec::Handle::new(1),
        insert: codec::Handle::new(2),
        parameter: codec::Handle::new(3),
        component: BlockParameterComponent::Distance,
    }
}

fn open_dynamic_value_list(
    app: &mut OpenCADStudio,
) -> crate::scene::model::object::BlockParameterKey {
    let key = block_parameter_key();
    let _ = app.update(Message::PropBlockPickerToggle { key, quick: false });
    assert_eq!(
        app.tabs[app.active_tab].properties.open_block_picker,
        Some((key, false))
    );
    key
}

#[test]
fn dynamic_value_list_toggles_open_and_closed() {
    let mut app = fixture();
    let key = open_dynamic_value_list(&mut app);
    let _ = app.update(Message::PropBlockPickerToggle { key, quick: false });
    assert_eq!(app.tabs[app.active_tab].properties.open_block_picker, None);
}

/// The same row exists on the docked and Quick Properties surfaces, and a
/// caret click names the surface it came from: clicking one copy's caret opens
/// that copy's list instead of closing the other copy's.
#[test]
fn each_surface_toggles_its_own_copy_of_the_list() {
    let mut app = fixture();
    let key = open_dynamic_value_list(&mut app);
    let _ = app.update(Message::PropBlockPickerToggle { key, quick: true });
    assert_eq!(
        app.tabs[app.active_tab].properties.open_block_picker,
        Some((key, true))
    );
    let _ = app.update(Message::PropBlockPickerToggle { key, quick: false });
    assert_eq!(
        app.tabs[app.active_tab].properties.open_block_picker,
        Some((key, false))
    );
    let _ = app.update(Message::PropBlockPickerToggle { key, quick: false });
    assert_eq!(app.tabs[app.active_tab].properties.open_block_picker, None);
}

/// Keys the list must keep to itself. Escape closes it; Enter and the arrows
/// would otherwise leave the list open behind a viewport that has already
/// reacted (a cleared selection, a recalled command, a nudged selection).
#[test]
fn open_dynamic_value_list_swallows_escape_enter_and_the_arrows() {
    use super::ArrowKey;

    let mut app = fixture();
    open_dynamic_value_list(&mut app);
    for msg in [
        Message::ArrowKeyPressed {
            direction: ArrowKey::Up,
            shortcut: "UP".into(),
            extend_selection: false,
        },
        Message::CommandLineArrowProbe {
            direction: ArrowKey::Down,
            extend_selection: false,
        },
        Message::CommandFinalize,
        Message::CommandSubmit,
        Message::CommandSpace,
        Message::ShortcutPressed("ENTER".into()),
        Message::ShortcutPressed("SPACE".into()),
    ] {
        assert!(
            app.intercept_block_picker_key(&msg).is_some(),
            "{msg:?} must not reach the command line or the viewport"
        );
    }
    assert!(app.tabs[app.active_tab].properties.open_block_picker.is_some());
    assert!(app
        .intercept_block_picker_key(&Message::PropBlockPickerToggle {
            key: block_parameter_key(),
            quick: false,
        })
        .is_none());
}

#[test]
fn escape_closes_the_dynamic_value_list_and_keeps_the_selection() {
    for escape in [Message::CommandEscape, Message::ShortcutPressed("ESCAPE".into())] {
        let mut app = fixture();
        let handle = select_editor_line(&mut app);
        open_dynamic_value_list(&mut app);
        let _ = app.update(escape);
        let panel = &app.tabs[app.active_tab].properties;
        assert_eq!(panel.open_block_picker, None);
        assert!(app.tabs[app.active_tab].scene.selected.contains(&handle));
    }
}

#[test]
fn enter_does_not_reach_the_command_line_while_the_list_is_open() {
    let mut app = fixture();
    open_dynamic_value_list(&mut app);
    app.command_line.history_open = true;
    app.command_line.input = "LINE".into();
    let _ = app.update(Message::CommandSubmit);
    // Submitting would close the history and run the typed command.
    assert!(app.command_line.history_open);
    assert_eq!(app.command_line.input, "LINE");
}

/// A flag naming a row that is not on the panel is dropped by the rebuild
/// instead of leaving the guard swallowing keys.
#[test]
fn a_rebuild_without_a_dynamic_row_drops_the_list() {
    let mut app = fixture();
    let i = app.active_tab;
    open_dynamic_value_list(&mut app);
    app.refresh_properties();
    assert_eq!(app.tabs[i].properties.open_block_picker, None);
}
