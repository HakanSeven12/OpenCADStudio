use super::*;
use crate::scene::model::object::PropValue;
use crate::scene::{block_authoring::metadata, block_visibility};

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
