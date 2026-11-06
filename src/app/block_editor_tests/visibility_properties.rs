use super::*;
use crate::scene::model::object::{BlockParameterComponent, PropValue};
use crate::scene::{block_authoring::metadata, block_visibility};

#[test]
fn batch_visibility_properties_isolate_copies_and_reject_stale_states() {
    let (mut app, first) = fixture();
    begin(&mut app, first);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let member = members(&app)[0];
    let doc = &mut app.tabs[i].scene.document;
    let parameter = block_visibility::create(doc, definition, Vector3::ZERO).unwrap();
    block_visibility::add_state(doc, parameter, "Hidden", 0).unwrap();
    block_visibility::set_visible(doc, parameter, 1, &[member], false);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let scene = &mut app.tabs[i].scene;
    let second = scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::UNIT_X * 5.,
        )))
        .unwrap();
    let shared = scene.visibility_representation(second).unwrap();
    let other = scene.copy_entities(
        &[second],
        &crate::command::EntityTransform::Translate(glam::DVec3::X * 5.),
    )[0];
    let EntityType::Insert(copy) = scene.document.get_entity(other).unwrap() else {
        panic!()
    };
    assert_eq!(copy.block_name, shared);
    assert_eq!(scene.document.dynamic_definition_for_insert(other), Some(definition));
    app.apply_visibility_state(first, 1);
    app.finish_pending_history(i);
    app.tabs[i].scene.select_entity(first, true);
    app.tabs[i].scene.select_entity(second, false);
    app.refresh_properties();
    let row = |app: &OpenCADStudio| {
        app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .find_map(|row| {
                if let PropValue::BlockChoice { key, selected, .. } = &row.value {
                    if matches!(key.component, BlockParameterComponent::Visibility(_)) {
                        return Some((*key, *selected));
                    }
                }
                None
            })
            .unwrap()
    };
    let read = |doc: &codec::Document, insert| {
        let row = OpenCADStudio::visibility_property(doc, insert)
            .unwrap_or_else(|| panic!("missing Visibility property for {insert}"));
        let PropValue::ChoiceWithTooltip { selected, .. } = row.value else {
            panic!()
        };
        selected
    };
    let choose = |app: &mut OpenCADStudio, key, index| {
        let _ = app.update(Message::PropBlockChoice { key, index });
    };
    let (key, selected) = row(&app);
    assert_eq!(selected, crate::ui::properties::MIXED_BLOCK_CHOICE);
    assert!(key.batch_generation.is_some());
    let depth = app.tabs[i].history.undo_stack.len();
    choose(&mut app, key, 1);
    let doc = &app.tabs[i].scene.document;
    assert_eq!(read(doc, first), "Hidden");
    assert_eq!(read(doc, second), "Hidden");
    assert_ne!(read(doc, other), "Hidden");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    choose(&mut app, key, 1);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(row(&app).1, crate::ui::properties::MIXED_BLOCK_CHOICE);
    app.redo_steps(1);
    assert_eq!(row(&app).1, 1);
    for extension in ["dwg", "dxf"] {
        let doc = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
        let loaded =
            crate::io::load_bytes(&format!("batch-visibility.{extension}"), bytes).unwrap();
        assert_eq!(read(&loaded, first), "Hidden");
        assert_eq!(read(&loaded, second), "Hidden");
        assert_ne!(read(&loaded, other), "Hidden");
    }
    assert!(block_visibility::rename_state(
        &mut app.tabs[i].scene.document,
        parameter,
        1,
        "Closed",
    ));
    let before = app.tabs[i].scene.document.clone();
    choose(&mut app, key, 0);
    assert!(app.tabs[i].scene.document == before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.refresh_properties();
    let (current, _) = row(&app);
    assert_ne!(current, key);
    choose(&mut app, current, 0);
    assert_eq!(row(&app).1, 0);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
}

#[test]
fn block_visibility_reference_description_survives_state_history_and_drawing_reload() {
    const DESCRIPTION: &str =
        "  Display façade\nUse the alternative state for 1 + 2 / literal text.  ";
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        let source = app.active_tab;
        begin(&mut app, reference);
        let def = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let target = members(&app)[0];
        let doc = &mut app.tabs[source].scene.document;
        let parameter = block_visibility::create(doc, def, Vector3::ZERO).unwrap();
        metadata::set_text(
            doc,
            def,
            parameter,
            metadata::Field::Description,
            DESCRIPTION,
        )
        .unwrap();
        block_visibility::add_state(doc, parameter, "Alternative", 0).unwrap();
        block_visibility::set_visible(doc, parameter, 1, &[target], false);
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let source_before = app.tabs[source].scene.document.clone();
        let row = OpenCADStudio::visibility_property(&app.tabs[i].scene.document, insert).unwrap();
        let PropValue::ChoiceWithTooltip {
            selected,
            options,
            tooltip,
        } = row.value
        else {
            panic!()
        };
        assert_eq!(tooltip, DESCRIPTION);
        assert!(options.contains(&"Alternative".to_owned()));
        let depth = app.tabs[i].history.undo_stack.len();
        let _ = app.update(Message::PropGeomChoiceChanged {
            field: "dyn_visibility",
            value: "Alternative".into(),
        });
        app.finish_pending_history(i);
        app.refresh_properties();
        let read = |app: &OpenCADStudio| {
            let row =
                OpenCADStudio::visibility_property(&app.tabs[i].scene.document, insert).unwrap();
            let PropValue::ChoiceWithTooltip {
                selected, tooltip, ..
            } = row.value
            else {
                panic!()
            };
            assert_eq!(tooltip, DESCRIPTION);
            selected
        };
        assert_eq!(read(&app), "Alternative");
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        assert_eq!(read(&app), selected);
        app.redo_steps(1);
        assert_eq!(read(&app), "Alternative");
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
        for extension in ["dwg", "dxf"] {
            let doc = &app.tabs[i].scene.document;
            let bytes = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
            let loaded =
                crate::io::load_bytes(&format!("visibility-description.{extension}"), bytes)
                    .unwrap();
            let row = OpenCADStudio::visibility_property(&loaded, insert).unwrap();
            // The existing DXF codec retains paragraph escapes in plain metadata.
            let expected = if extension == "dxf" {
                DESCRIPTION.replace('\n', "\\P")
            } else {
                DESCRIPTION.to_owned()
            };
            assert!(
                matches!(&row.value, PropValue::ChoiceWithTooltip { selected, tooltip, .. }
                if selected == "Alternative" && tooltip == &expected),
                "{extension}, testing={testing}: {:?}", row.value
            );
        }
        if !testing {
            app.tabs[i]
                .scene
                .document
                .layers
                .get_mut("0")
                .unwrap()
                .lock();
            app.refresh_properties();
            assert!(app.tabs[i]
                .properties
                .sections
                .iter()
                .flat_map(|s| &s.props)
                .any(
                    |row| matches!(&row.value, PropValue::ReadOnlyWithTooltip { value, tooltip }
                    if value == "Alternative" && tooltip == DESCRIPTION)
                ));
            app.tabs[i]
                .scene
                .document
                .layers
                .get_mut("0")
                .unwrap()
                .unlock();
        }
        metadata::set_text(
            &mut app.tabs[i].scene.document,
            def,
            parameter,
            metadata::Field::Description,
            "",
        )
        .unwrap();
        assert!(
            matches!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, insert).unwrap().value,
            PropValue::ChoiceWithTooltip { tooltip, .. } if tooltip.is_empty())
        );
        metadata::set_visible(&mut app.tabs[i].scene.document, def, parameter, false).unwrap();
        assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, insert).is_none());
    }
}
