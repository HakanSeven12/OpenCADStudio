use super::*;
use crate::scene::model::object::{
    BlockParameterComponent as Component, BlockParameterKey, PropValue,
};
use crate::scene::{
    block_actions as actions, block_distance_parameter as distance,
    block_properties_table as tables,
};

fn table_fixture(testing: bool) -> (OpenCADStudio, usize, Handle, Handle, Handle, Handle) {
    table_fixture_with_limits(testing, false)
}

fn table_fixture_with_limits(
    testing: bool,
    limits: bool,
) -> (OpenCADStudio, usize, Handle, Handle, Handle, Handle) {
    let (mut app, reference) = fixture();
    let source = app.active_tab;
    let other = app.tabs[source]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("EDIT_ME", Vector3::UNIT_Y)))
        .unwrap();
    begin(&mut app, reference);
    let def = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let target = members(&app)[0];
    for end in [Vector3::new(10., 0., 0.), Vector3::new(0., 20., 0.)] {
        let parameter = distance::create(
            &mut app.tabs[source].scene.document,
            def,
            distance::Kind::Linear,
            Vector3::ZERO,
            end,
            Vector3::UNIT_Y,
        )
        .unwrap();
        actions::create(
            &mut app.tabs[source].scene.document,
            def,
            actions::Kind::Move,
            parameter,
            &[target],
            glam::DVec3::ZERO,
        )
        .unwrap();
    }
    let columns = tables::columns(&app.tabs[source].scene.document, def);
    assert_eq!(columns.len(), 2);
    let handle = tables::apply(
        &mut app.tabs[source].scene.document,
        def,
        &columns,
        &[vec![10., 20.], vec![15., 30.], vec![15., 30.]],
        false,
    )
    .unwrap();
    // Properties remains usable when the table's display grip is disabled.
    let codec::objects::ObjectType::DynamicBlock(object) = app.tabs[source]
        .scene
        .document
        .objects
        .get_mut(&handle)
        .unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PropertiesTable(table) = &mut object.data else {
        panic!()
    };
    table.parameter.property_count = 0;
    if limits {
        let codec::objects::ObjectType::DynamicBlock(object) = app.tabs[source]
            .scene
            .document
            .objects
            .get_mut(&columns[0].handle)
            .unwrap()
        else {
            panic!()
        };
        let codec::objects::DynamicBlockData::LinearParameter(parameter) = &mut object.data else {
            panic!()
        };
        parameter.value_set.flags = 8;
        parameter.value_set.values = vec![10., 20.];
    }
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
    (app, source, def, insert, other, handle)
}

fn row(app: &OpenCADStudio) -> (BlockParameterKey, usize, Vec<String>) {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find_map(|property| match &property.value {
            PropValue::BlockChoice {
                key,
                selected,
                options,
                ..
            } if matches!(key.component, Component::TableRow(_)) => {
                Some((*key, *selected, options.clone()))
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn block_table_properties_rows_are_independent_undoable_and_persistent() {
    for testing in [false, true] {
        let (mut app, source, def, insert, other, _) = table_fixture(testing);
        let i = app.active_tab;
        let source_before = app.tabs[source].scene.document.clone();
        let (key, selected, labels) = row(&app);
        assert_eq!(selected, 0);
        assert!(labels[1].starts_with("2:"));
        assert!(labels[2].starts_with("3:"));
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let _ = app.update(Message::PropBlockChoice { key, index: 0 });
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        let _ = app.update(Message::PropBlockChoice { key, index: 1 });
        assert_eq!(row(&app).1, 1);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let changed = app.tabs[i].scene.document.clone();
        let EntityType::Insert(reference) = changed.get_entity(insert).unwrap() else {
            panic!()
        };
        let line = changed
            .entities_in_block(&reference.block_name)
            .find_map(|e| {
                if let EntityType::Line(line) = e {
                    Some(line)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(line.start, Vector3::new(5., 10., 0.));
        assert_eq!(line.end, Vector3::new(6., 10., 0.));
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        } else {
            assert_eq!(changed.get_entity(other), before.get_entity(other));
            assert_eq!(
                changed
                    .entities_in_block("EDIT_ME")
                    .cloned()
                    .collect::<Vec<_>>(),
                before
                    .entities_in_block("EDIT_ME")
                    .cloned()
                    .collect::<Vec<_>>()
            );
        }
        let _ = app.update(Message::PropBlockChoice { key, index: 1 });
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        // Equal values in a distinct authored row retain their row identity.
        let _ = app.update(Message::PropBlockChoice { key, index: 2 });
        assert_eq!(row(&app).1, 2);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        app.undo_steps(1);
        assert_eq!(row(&app).1, 1);
        app.redo_steps(1);
        assert_eq!(row(&app).1, 2);
        for ext in ["dwg", "dxf"] {
            let doc = &app.tabs[i].scene.document;
            let bytes = crate::io::save_to_bytes(doc, ext, doc.version).unwrap();
            let saved = crate::io::load_bytes(&format!("table-properties.{ext}"), bytes).unwrap();
            let property = crate::scene::block_parameter_properties::properties(
                &saved,
                None,
                app.tabs[i].id,
                insert,
            )
            .into_iter()
            .find(|property| property.field == "dyn_table")
            .unwrap();
            assert!(
                matches!(property.value, PropValue::BlockChoice { selected: 2, .. }),
                "{ext}"
            );
            assert_eq!(actions::instance::definition(&saved, insert), Some(def));
        }
    }
}

#[test]
fn block_table_properties_reject_stale_hidden_locked_and_invalid_choices() {
    let (mut app, _, def, insert, other, handle) = table_fixture(false);
    let i = app.active_tab;
    let (key, _, _) = row(&app);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for (key, index) in [
        (key, 99),
        (
            BlockParameterKey {
                document: key.document + 1,
                ..key
            },
            1,
        ),
        (
            BlockParameterKey {
                insert: other,
                ..key
            },
            1,
        ),
    ] {
        let _ = app.update(Message::PropBlockChoice { key, index });
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .lock();
    let locked = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBlockChoice { key, index: 1 });
    assert_eq!(app.tabs[i].scene.document, locked);
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .unlock();
    let view = tables::table(&app.tabs[i].scene.document, def)
        .unwrap()
        .unwrap();
    tables::apply(
        &mut app.tabs[i].scene.document,
        def,
        &view.columns,
        &[vec![10., 20.], vec![40., 50.]],
        false,
    )
    .unwrap();
    let edited = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBlockChoice { key, index: 1 });
    assert_eq!(app.tabs[i].scene.document, edited);
    app.refresh_properties();
    let key = row(&app).0;
    let codec::objects::ObjectType::DynamicBlock(object) =
        app.tabs[i].scene.document.objects.get_mut(&handle).unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PropertiesTable(table) = &mut object.data else {
        panic!()
    };
    table.parameter.parameter.show_properties = false;
    let hidden = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBlockChoice { key, index: 1 });
    assert_eq!(app.tabs[i].scene.document, hidden);
    app.refresh_properties();
    assert!(!app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .any(|property| property.field == "dyn_table"));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[i].selected_handle, Some(insert));
}

#[test]
fn block_table_properties_show_custom_after_an_independent_parameter_edit() {
    let (mut app, _, _, _, _, _) = table_fixture(false);
    let i = app.active_tab;
    let key = row(&app).0;
    let _ = app.update(Message::PropBlockChoice { key, index: 2 });
    assert_eq!(row(&app).1, 2);
    let field = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find_map(|property| {
            if let PropValue::BlockNumber { key, .. } = property.value {
                Some(key)
            } else {
                None
            }
        })
        .unwrap();
    let _ = app.update(Message::PropBlockInput {
        key: field,
        value: "12".into(),
    });
    let _ = app.update(Message::PropBlockCommit(field));
    assert_eq!(row(&app).1, usize::MAX);
    let key = row(&app).0;
    let _ = app.update(Message::PropBlockChoice { key, index: 1 });
    assert_eq!(row(&app).1, 1);
}

#[test]
fn blocks_reports_an_unmatched_table_row_as_present_and_unmatched() {
    let (mut app, _, _, insert, _, _) = table_fixture(false);
    let i = app.active_tab;
    let handle = format!("{:X}", insert.value());
    let reference = |response: &serde_json::Value| {
        response["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|block| block["handle"] == handle.as_str())
            .unwrap()
            .clone()
    };
    let label = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find_map(|property| match &property.value {
            PropValue::BlockChoice { key, .. } if matches!(key.component, Component::TableRow(_)) => {
                Some(property.label.clone())
            }
            _ => None,
        })
        .unwrap();
    let (matched, _) = app.control_request(serde_json::json!({
        "op": "blocks",
        "names": ["*"],
        "extract_dynamic_properties": true
    }));
    assert_eq!(
        reference(&matched)["dynamic_properties"][&label],
        serde_json::json!("1: 10, 20"),
        "{matched}"
    );
    let field = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find_map(|property| match property.value {
            PropValue::BlockNumber { key, .. } => Some(key),
            _ => None,
        })
        .unwrap();
    let _ = app.update(Message::PropBlockInput {
        key: field,
        value: "12".into(),
    });
    let _ = app.update(Message::PropBlockCommit(field));
    assert_eq!(row(&app).1, usize::MAX);
    let (unmatched, _) = app.control_request(serde_json::json!({
        "op": "blocks",
        "names": ["*"],
        "extract_dynamic_properties": true
    }));
    let block = reference(&unmatched);
    let properties = block["dynamic_properties"].as_object().unwrap();
    assert!(
        properties.contains_key(&label),
        "the row stays present even though no value applies to it: {unmatched}"
    );
    assert_eq!(properties[&label], serde_json::Value::Null, "{unmatched}");
}

#[test]
fn block_table_properties_wrap_angles_and_display_degrees_without_actions() {
    let (mut app, _, def, insert, _, _) = table_fixture(false);
    let i = app.active_tab;
    let parameter = distance::create(
        &mut app.tabs[i].scene.document,
        def,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(0., 10., 0.),
    )
    .unwrap();
    let columns: Vec<_> = tables::columns(&app.tabs[i].scene.document, def)
        .into_iter()
        .filter(|column| column.handle == parameter && column.connection_name == "UpdatedAngle")
        .collect();
    assert_eq!(columns.len(), 1);
    let tau = std::f64::consts::TAU;
    tables::apply(
        &mut app.tabs[i].scene.document,
        def,
        &columns,
        &[vec![tau], vec![tau + std::f64::consts::FRAC_PI_2]],
        false,
    )
    .unwrap();
    app.refresh_properties();
    let (key, selected, labels) = row(&app);
    assert_eq!(selected, 0);
    let displayed: f64 = labels[1]
        .split(": ")
        .nth(1)
        .unwrap()
        .trim_end_matches('°')
        .parse()
        .unwrap();
    assert!((displayed - 450.).abs() < 1e-9);
    let _ = app.update(Message::PropBlockChoice { key, index: 1 });
    assert_eq!(row(&app).1, 1);
    let p = actions::all_parameters(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|p| p.handle == parameter)
        .unwrap();
    let frame = actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
    let delta = frame.end - frame.base;
    assert!(delta.x.abs() < 1e-8 && (delta.y - 10.).abs() < 1e-8);
}

#[test]
fn block_table_properties_reject_shared_scratch_references_and_nested_editor_edits() {
    let (mut app, _, _, insert, _, _) = table_fixture(true);
    let i = app.active_tab;
    let key = row(&app).0;
    let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap()
    else {
        panic!()
    };
    let name = reference.block_name.clone();
    app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(name, Vector3::UNIT_Y)))
        .unwrap();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropBlockChoice { key, index: 1 });
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);

    let (mut app, _, _, _, _, _) = table_fixture(false);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "OUTER",
        Vector3::ZERO,
        vec![EntityType::Insert(Insert::new("EDIT_ME", Vector3::ZERO))],
    );
    let outer = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("OUTER", Vector3::ZERO)))
        .unwrap();
    let nested = app.tabs[i]
        .scene
        .document
        .block_records
        .get("OUTER")
        .unwrap()
        .entity_handles[0];
    begin(&mut app, outer);
    app.tabs[i].scene.deselect_all();
    app.tabs[i].scene.select_entity(nested, true);
    app.tabs[i].selected_handle = Some(nested);
    app.refresh_properties();
    let property = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == "dyn_table")
        .unwrap();
    assert!(
        matches!(&property.value,PropValue::ReadOnlyWithTooltip { value,.. } if value.starts_with("1:"))
    );
}

#[test]
fn block_table_properties_reject_rows_that_conflict_with_parameter_value_sets() {
    for testing in [false, true] {
        let (mut app, _, _, _, _, _) = table_fixture_with_limits(testing, true);
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let key = row(&app).0;
        let _ = app.update(Message::PropBlockChoice { key, index: 1 });
        assert!(
            app.tabs[i].scene.document == before,
            "testing={testing}: {:?}",
            app.command_line.last_error
        );
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }
}

#[test]
fn block_table_properties_preserve_both_polar_components_in_either_column_order() {
    for testing in [false, true] {
        for reversed in [false, true] {
            let (mut app, reference) = fixture();
            let source = app.active_tab;
            begin(&mut app, reference);
            let def = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let member = members(&app)[0];
            let parameter = distance::create(
                &mut app.tabs[source].scene.document,
                def,
                distance::Kind::Polar,
                Vector3::ZERO,
                Vector3::new(10., 0., 0.),
                Vector3::UNIT_Y,
            )
            .unwrap();
            actions::create(
                &mut app.tabs[source].scene.document,
                def,
                actions::Kind::Move,
                parameter,
                &[member],
                glam::DVec3::ZERO,
            )
            .unwrap();
            let mut columns = tables::columns(&app.tabs[source].scene.document, def);
            assert_eq!(columns.len(), 2);
            if reversed {
                columns.reverse();
            }
            let make_row = |distance: f64, angle: f64| {
                columns
                    .iter()
                    .map(|column| {
                        if column.connection_name.eq_ignore_ascii_case("UpdatedAngle") {
                            angle
                        } else {
                            distance
                        }
                    })
                    .collect::<Vec<_>>()
            };
            tables::apply(
                &mut app.tabs[source].scene.document,
                def,
                &columns,
                &[
                    make_row(10., 0.),
                    make_row(15., std::f64::consts::FRAC_PI_2),
                ],
                false,
            )
            .unwrap();
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
            let key = row(&app).0;
            let _ = app.update(Message::PropBlockChoice { key, index: 1 });
            assert_eq!(
                row(&app).1,
                1,
                "testing={testing} reversed={reversed}: {:?}",
                app.command_line.last_error
            );
            let p = actions::all_parameters(&app.tabs[i].scene.document, def)
                .into_iter()
                .find(|p| p.handle == parameter)
                .unwrap();
            let frame = if testing {
                actions::instance::test_frame(&app.tabs[i].scene.document, insert, &p)
            } else {
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p)
            }
            .unwrap();
            assert!(frame.end.distance(glam::DVec3::new(0., 15., 0.)) < 1e-8);
        }
    }
}

#[test]
fn the_value_list_closes_with_the_reference_it_belongs_to() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let i = app.active_tab;
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup(iced::Point::new(100., 100.));
    assert!(
        app.table_popup.is_some(),
        "the list opens for the lone selected reference"
    );
    let _ = app.automation_op(r#"{"op":"select","clear":true}"#);
    app.refresh_properties();
    assert!(
        app.tabs[i].table_grip.is_none(),
        "the grip is gone with the selection"
    );
    assert!(
        app.table_popup.is_none(),
        "the list has nothing to hang from once that reference is not selected"
    );
}

#[test]
fn block_table_properties_and_grip_popup_refresh_the_same_indexed_row() {
    for testing in [false, true] {
        let (mut app, _, def, _, _, table) = table_fixture(testing);
        let i = app.active_tab;
        crate::scene::block_grips::config::set(
            &mut app.tabs[i].scene.document,
            def,
            table,
            Some(1),
        )
        .unwrap();
        app.refresh_properties();
        let key = row(&app).0;
        let _ = app.update(Message::PropBlockChoice { key, index: 2 });
        assert_eq!(row(&app).1, 2);
        assert_eq!(app.tabs[i].table_grip.as_ref().unwrap().current, Some(2));
        app.open_table_popup(iced::Point::new(100., 100.));
        assert_eq!(app.table_popup.as_ref().unwrap().current, Some(2));
        let _ = app.update(Message::TableRowPick(1));
        assert_eq!(row(&app).1, 1);
        assert_eq!(app.tabs[i].table_grip.as_ref().unwrap().current, Some(1));
    }
}
