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
    let read = |doc: &document::Document, insert| {
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

#[test]
fn synthetic_dynamic_and_static_references_audit_properties_and_grips() {
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance, block_lookup as lookup,
        block_orientation_parameter as orientation, block_parameter_properties as properties,
        block_point_parameter as point,
    };
    use document::objects::ObjectType;
    use glam::DVec3;

    let (mut app, plain_dynamic) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "STATIC_PART",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "STATIC_PART",
            Vector3::new(-50., 0., 0.),
        )))
        .unwrap();

    begin(&mut app, plain_dynamic);
    circle(&mut app, 4.);
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let targets = members(&app);
    let doc = &mut app.tabs[i].scene.document;

    let pt = point::create(doc, def, Vector3::new(1., 1., 0.), Vector3::new(2., 2., 0.)).unwrap();
    actions::create(doc, def, actions::Kind::Move, pt, &[targets[0]], DVec3::ZERO).unwrap();

    let lin = distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., -2., 0.),
    )
    .unwrap();
    actions::create(doc, def, actions::Kind::Move, lin, &[targets[0]], DVec3::ZERO).unwrap();

    let pol = distance::create(
        doc,
        def,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(0., 8., 0.),
        Vector3::new(2., 4., 0.),
    )
    .unwrap();
    actions::create(doc, def, actions::Kind::Move, pol, &[targets[1]], DVec3::ZERO).unwrap();

    let xy = distance::create(
        doc,
        def,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(6., 8., 0.),
        Vector3::new(3., 9., 0.),
    )
    .unwrap();
    actions::create(doc, def, actions::Kind::Move, xy, &[targets[1]], DVec3::ZERO).unwrap();

    let rot = orientation::create(
        doc,
        def,
        orientation::Kind::Rotation,
        Vector3::ZERO,
        Vector3::new(5., 0., 0.),
        Vector3::new(5., 0., 0.),
    )
    .unwrap();
    actions::create(doc, def, actions::Kind::Rotate, rot, &[targets[0]], DVec3::ZERO).unwrap();

    let flp = orientation::create(
        doc,
        def,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::new(0., 10., 0.),
        Vector3::new(2., 5., 0.),
    )
    .unwrap();
    actions::create(doc, def, actions::Kind::Flip, flp, &[targets[0]], DVec3::ZERO).unwrap();

    let vis = block_visibility::create(doc, def, Vector3::new(-4., 4., 0.)).unwrap();
    block_visibility::add_state(doc, vis, "Alternate", 0).unwrap();
    block_visibility::set_visible(doc, vis, 1, &[targets[1]], false);

    let lkp = lookup::create(doc, def, Vector3::new(-4., -4., 0.)).unwrap();
    let lin_id = actions::all_parameters(doc, def)
        .into_iter()
        .find(|p| p.handle == lin)
        .unwrap()
        .id;
    let input_col = lookup::properties(doc, def)
        .into_iter()
        .find(|prop| prop.column.node_id == lin_id)
        .unwrap()
        .column;
    let mut table = lookup::empty_table(
        lookup::parameters(doc, def)
            .into_iter()
            .find(|(h, _)| *h == lkp)
            .unwrap()
            .1,
    );
    table.columns.insert(0, input_col);
    table.column_count = 2;
    table.row_count = 2;
    table.expressions = ["10", "Compact", "20", "Extended"]
        .map(String::from)
        .to_vec();
    lookup::apply_table(doc, def, lkp, table).unwrap();

    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");

    let mut scaled = Insert::new("EDIT_ME", Vector3::new(150., 50., 0.));
    scaled.set_x_scale(-2.);
    scaled.set_y_scale(2.);
    scaled.set_z_scale(2.);
    app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(scaled))
        .unwrap();

    let doc = app.tabs[i].scene.document.clone();
    let mut failures = Vec::new();
    let mut total = 0;
    let mut static_references = 0;
    let mut controls = 0;

    for entity in doc.entities() {
        let EntityType::Insert(reference) = entity else {
            continue;
        };
        let handle = reference.common.handle;
        assert!(
            doc.block_records.get(&reference.block_name).is_some(),
            "reference {handle:?} has no block record"
        );
        let Some(definition) = doc.dynamic_definition_for_insert(handle) else {
            static_references += 1;
            continue;
        };
        let Some(ObjectType::Dictionary(dictionary)) = doc
            .extension_dictionary_handle(definition)
            .and_then(|h| doc.objects.get(&h))
        else {
            static_references += 1;
            continue;
        };
        if dictionary.get("ACAD_ENHANCEDBLOCK").is_none() {
            static_references += 1;
            continue;
        }
        total += 1;
        let previous_controls = controls;
        let context = format!(
            "{handle:?} ({:?}, scale {:?})",
            reference.block_name,
            reference.x_scale()
        );
        let mut fail = |control: &str, error: &str| {
            failures.push(format!("{context} {control}: {error}"));
        };
        if actions::instance::definition(&doc, handle).is_none() {
            fail("definition", "reference is unavailable");
            continue;
        }
        app.tabs[i].scene.document = doc.clone();
        app.tabs[i].scene.selected.clear();
        app.tabs[i].scene.select_entity(handle, true);
        app.tabs[i].selected_handle = Some(handle);
        app.refresh_properties();
        let grips = app.tabs[i].selected_grips.clone();
        let visibility = OpenCADStudio::indexed_visibility_property(&doc, 0, handle);
        for prop in properties::properties(&doc, None, 0, handle)
            .into_iter()
            .chain(visibility)
        {
            match prop.value {
                PropValue::ReadOnlyWithTooltip { tooltip, .. } => fail(&prop.label, &tooltip),
                PropValue::BlockNumber { key, value, .. } => {
                    controls += 1;
                    if let Some(number) =
                        crate::app::block_parameter_properties::parse_value(&value)
                    {
                        for next in [number + 1., number + 10.] {
                            let result = properties::evaluate(
                                &doc,
                                None,
                                key,
                                properties::Input::Number(next),
                            );
                            match result {
                                Ok(Some(_)) => {},
                                Ok(None) => fail(&prop.label, "different value produced no edit"),
                                Err(err) => { fail(&prop.label, err); break; },
                            }
                        }
                    } else {
                        fail(&prop.label, "failed to parse BlockNumber value");
                    }
                }
                PropValue::BlockChoice {
                    key,
                    options,
                    selected,
                    ..
                } => {
                    for next in (0..options.len()).filter(|&next| next != selected) {
                        controls += 1;
                        let result = if prop.field == "dyn_visibility" {
                            OpenCADStudio::stage_visibility_property(&doc, key, next)
                        } else {
                            properties::evaluate(&doc, None, key, properties::Input::Choice(next))
                        };
                        match result {
                            Ok(Some(_)) => {},
                            Ok(None) => fail(&prop.label, "different choice produced no edit"),
                            Err(err) => fail(&format!("{} -> {next}", prop.label), err),
                        }
                    }
                }
                _ => {}
            }
        }
        for grip in &grips {
            let Some((node, role)) = crate::scene::block_grips::runtime::decode(grip.id) else {
                continue;
            };
            controls += 1;
            app.tabs[i].scene.document = doc.clone();
            app.tabs[i].scene.selected.clear();
            app.tabs[i].scene.select_entity(handle, true);
            app.tabs[i].selected_handle = Some(handle);
            app.refresh_properties();
            app.command_line.last_error = None;
            if !app.start_test_action_grip(handle, grip.id) {
                fail(
                    &format!("grip {node}/{role}"),
                    app.command_line
                        .last_error
                        .as_deref()
                        .unwrap_or("did not start"),
                );
                continue;
            }
            if app.tabs[i].active_cmd.is_some() {
                let direction = grip.dir.unwrap_or(DVec3::new(1., 1., 0.));
                let _ = app.feed_command(crate::command::StepInput::Point(
                    grip.world + direction * 3.,
                ));
                assert!(app.tabs[i].active_cmd.is_none(), "grip input must finish the command");
            }
            if app.tabs[i].scene.document == doc {
                fail(&format!("grip {node}/{role}"), "grip produced no edit");
            }
            if let Some(error) = app.command_line.last_error.as_deref() {
                fail(&format!("grip {node}/{role}"), error);
            }
        }
        if controls == previous_controls {
            fail("controls", "no grip or editable property was exercised");
        }
    }
    assert_eq!(static_references, 1);
    assert_eq!(total, 2);
    assert!(controls >= 20, "exercised {controls} controls");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

