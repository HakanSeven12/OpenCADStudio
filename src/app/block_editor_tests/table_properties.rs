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
    let document::objects::ObjectType::DynamicBlock(object) = app.tabs[source]
        .scene
        .document
        .objects
        .get_mut(&handle)
        .unwrap()
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::PropertiesTable(table) = &mut object.data else {
        panic!()
    };
    table.parameter.property_count = 0;
    if limits {
        let document::objects::ObjectType::DynamicBlock(object) = app.tabs[source]
            .scene
            .document
            .objects
            .get_mut(&columns[0].handle)
            .unwrap()
        else {
            panic!()
        };
        let document::objects::DynamicBlockData::LinearParameter(parameter) = &mut object.data else {
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
    let document::objects::ObjectType::DynamicBlock(object) =
        app.tabs[i].scene.document.objects.get_mut(&handle).unwrap()
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::PropertiesTable(table) = &mut object.data else {
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
            PropValue::BlockChoice { key, .. }
                if matches!(key.component, Component::TableRow(_)) =>
            {
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
fn block_table_properties_reject_shared_scratch_references_and_edit_nested_rows() {
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

    let (mut app, _, _, other, _, _) = table_fixture(false);
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
    let (key, selected, _) = row(&app);
    assert_eq!(key.insert, nested);
    assert!(key.editor_session.is_some());
    assert_eq!(selected, 0);
    let _ = app.update(Message::PropBlockChoice { key, index: 2 });
    assert_eq!(row(&app).1, 2);
    assert_eq!(
        crate::scene::block_parameter_properties::current_table_row(
            &app.tabs[i].scene.document, None, other,
        ),
        Some(0),
    );
    app.undo_steps(1);
    assert_eq!(row(&app).1, 0);
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
    app.open_table_popup();
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

/// The other way a list loses its grip: the refresh stops drawing it while the
/// rows it shows stay recorded, because the table no longer binds a display grip.
/// The list is placed from the grip, so that state cannot leave a value list on
/// screen — recorded rows with nothing to hang them from paint nothing.
#[test]
fn a_value_list_is_not_painted_when_its_grip_is_gone() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let i = app.active_tab;
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    let bounds = iced::Rectangle {
        x: 0.,
        y: 0.,
        width: 1600.,
        height: 900.,
    };
    assert!(
        app.table_popup_anchor(i, bounds).is_some(),
        "the open list hangs from the grip while the grip is drawn"
    );
    let doc = &mut app.tabs[i].scene.document;
    let Some(document::objects::ObjectType::DynamicBlock(native)) = doc.objects.get_mut(&table) else {
        panic!("the fixture's table is a dynamic block object");
    };
    let document::objects::DynamicBlockData::PropertiesTable(rows) = &mut native.data else {
        panic!("the table object holds the properties table");
    };
    rows.parameter.property_count = 0;
    app.refresh_properties();
    assert!(
        app.tabs[i].table_grip.is_none(),
        "the grip is gone with its binding"
    );
    assert!(
        app.table_popup.is_some(),
        "and the rows the list shows are not deleted while the drawing stops showing the grip"
    );
    assert!(
        app.table_popup_anchor(i, bounds).is_none(),
        "nothing paints a list whose grip is not drawn"
    );
}

/// Opening the editor for the reference a value list is open on closes that
/// list: the editing context draws no grip for a placed reference, so a list
/// left over from before entry would hang over a drawing whose selection no
/// longer means the same thing.
#[test]
fn a_value_list_is_not_painted_while_its_reference_is_being_edited() {
    let (mut app, _, def, insert, _, table) = table_fixture(false);
    let i = app.active_tab;
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    let bounds = iced::Rectangle {
        x: 0.,
        y: 0.,
        width: 1600.,
        height: 900.,
    };
    assert!(
        app.table_popup_anchor(i, bounds).is_some(),
        "the list hangs from the grip while the drawing shows it"
    );
    begin(&mut app, insert);
    let i = app.active_tab;
    app.refresh_properties();
    assert!(
        app.tabs[i].table_grip.is_none(),
        "the editing context takes the grip down"
    );
    assert!(
        app.table_popup.is_none(),
        "and the entry closes the list rather than leaving it open behind the editor"
    );
}

#[test]
fn the_value_list_follows_its_grip_when_the_view_moves() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let i = app.active_tab;
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    assert!(
        app.table_popup.is_some(),
        "the list opens for the lone selected reference"
    );
    let bounds = iced::Rectangle {
        x: 120.,
        y: 60.,
        width: 1600.,
        height: 900.,
    };
    let before = app
        .table_popup_anchor(i, bounds)
        .expect("the grip projects into the view the list was opened in");
    let world = app.tabs[i].table_grip.as_ref().unwrap().world;
    let projected = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(world, bounds)
        .expect("the same grip point projects");
    assert_eq!(
        before,
        iced::Point::new(bounds.x + projected.x, bounds.y + projected.y),
        "the list hangs from its grip, not from the screen point it was opened at"
    );
    app.tabs[i].scene.camera.borrow_mut().target += glam::DVec3::new(40., 25., 0.);
    let after = app
        .table_popup_anchor(i, bounds)
        .expect("the same grip still projects after the view changes");
    let repanned = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(world, bounds)
        .expect("the same grip point projects");
    assert_eq!(
        after,
        iced::Point::new(bounds.x + repanned.x, bounds.y + repanned.y),
        "the list is placed on the grip the view shows now, not where it was opened"
    );
    assert!(
        before.distance(after) > 1.,
        "a panned view moves the grip, so the list moves with it"
    );
}

/// Inside a floating viewport the grips are drawn through that viewport's own
/// camera and rectangle, so the list that hangs from one is placed there too,
/// not through the sheet camera over the whole canvas.
#[test]
fn the_value_list_hangs_from_the_grip_a_floating_viewport_draws() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let i = app.active_tab;
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    assert!(
        app.table_popup.is_some(),
        "the list opens for the lone selected reference"
    );

    let world = app.tabs[i]
        .table_grip
        .as_ref()
        .expect("the selected reference still draws its grip")
        .world;

    let scene = &mut app.tabs[i].scene;
    scene.document.add_layout("Sheet").unwrap();
    scene.set_current_layout("Sheet".into());
    let mut viewport = document::entities::Viewport::new();
    viewport.id = 2;
    viewport.center = Vector3::new(50., 50., 0.);
    viewport.width = 100.;
    viewport.height = 100.;
    viewport.view_height = 1000.;
    viewport.view_target = Vector3::new(world.x, world.y, world.z);
    viewport.view_direction = Vector3::UNIT_Z;
    viewport.turn_on();
    let viewport = scene.add_entity(EntityType::Viewport(Box::new(viewport)));
    scene.active_viewport = Some(viewport);

    let canvas = iced::Rectangle {
        x: 0.,
        y: 0.,
        width: 1600.,
        height: 900.,
    };
    let (camera, rect) = app.tabs[i]
        .scene
        .viewport_edit_frame((canvas.width, canvas.height))
        .expect("the active floating viewport has an editing frame");
    let projected = camera
        .project(world, rect)
        .expect("the grip projects through the viewport");
    let expected = iced::Point::new(rect.x + projected.x, rect.y + projected.y);
    assert!(
        rect.contains(expected),
        "the viewport draws the grip inside its own rectangle: {expected:?} in {rect:?}"
    );
    let sheet = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(world, canvas)
        .expect("the grip projects through the sheet camera too");
    assert!(
        iced::Point::new(canvas.x + sheet.x, canvas.y + sheet.y).distance(expected) > 1.0,
        "the viewport and the sheet place the grip apart, so the choice matters"
    );
    assert_eq!(
        app.table_popup_anchor(i, canvas)
            .expect("the open list has an anchor"),
        expected,
        "the list hangs from the grip where the viewport draws it"
    );
}

#[test]
fn the_value_list_closes_when_its_drawing_tab_is_left() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let home = app.active_tab;
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    let other = app.tabs.len() - 1;
    crate::scene::block_grips::config::set(&mut app.tabs[home].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    assert!(
        app.table_popup.is_some(),
        "the list opens on the drawing that owns the reference"
    );
    let _ = app.update(Message::TabSwitch(other));
    assert!(
        app.table_popup.is_none(),
        "the list names an insert of one drawing, so leaving that drawing drops it"
    );
}

#[test]
fn the_value_list_closes_when_its_drawing_tab_is_closed() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let home = app.active_tab;
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    crate::scene::block_grips::config::set(&mut app.tabs[home].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    assert!(
        app.table_popup.is_some(),
        "the list opens on the drawing that owns the reference"
    );
    let _ = app.on_tab_close(home);
    assert!(
        app.table_popup.is_none(),
        "the list names an insert of that drawing, so closing it takes the list with it"
    );
}

#[test]
fn an_open_value_list_marks_the_row_selected_after_it_opened() {
    let (mut app, _, def, _, _, table) = table_fixture(false);
    let i = app.active_tab;
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, def, table, Some(1))
        .unwrap();
    app.refresh_properties();
    app.open_table_popup();
    assert!(
        app.table_popup.is_some(),
        "the list opens on the lone selected reference"
    );
    let key = row(&app).0;
    let _ = app.update(Message::PropBlockChoice { key, index: 2 });
    assert!(
        app.table_popup.is_some(),
        "choosing a row on the panel does not close the list belonging to that reference"
    );
    let (_, selected, options) = row(&app);
    assert_eq!(
        app.tabs[i].table_grip.as_ref().unwrap().current,
        Some(2),
        "the grip knows the row chosen after the list opened"
    );
    assert_eq!(
        app.table_popup.as_ref().unwrap().current,
        Some(2),
        "and the open list marks that row, not the one chosen when it opened"
    );
    assert_eq!(
        app.table_popup.as_ref().unwrap().rows.len(),
        options.len(),
        "the reprint still shows every row of the table"
    );
    assert_eq!(
        app.table_popup.as_ref().unwrap().current,
        Some(selected),
        "and it marks the row the panel has selected"
    );
    assert!(
        !app.table_popup.as_ref().unwrap().header.is_empty(),
        "the reprinted list keeps its heading"
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
        app.open_table_popup();
        assert_eq!(app.table_popup.as_ref().unwrap().current, Some(2));
        let _ = app.update(Message::TableRowPick(1));
        assert_eq!(row(&app).1, 1);
        assert_eq!(app.tabs[i].table_grip.as_ref().unwrap().current, Some(1));
    }
}

#[test]
fn block_table_row_history_preserves_transported_linear_frames() {
    use glam::DVec3;
    for testing in [false, true] {
        for chained in [false, true] {
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
            let line = members(&app)[0];
            let doc = &mut app.tabs[source].scene.document;
            let parent = distance::create(
                doc,
                def,
                distance::Kind::Linear,
                Vector3::ZERO,
                Vector3::new(10., 0., 0.),
                Vector3::UNIT_Y,
            )
            .unwrap();
            let child = distance::create(
                doc,
                def,
                distance::Kind::Linear,
                Vector3::new(0., 5., 0.),
                Vector3::new(10., 5., 0.),
                Vector3::new(5., 7., 0.),
            )
            .unwrap();
            let document::objects::ObjectType::DynamicBlock(object) =
                doc.objects.get_mut(&child).unwrap()
            else {
                panic!()
            };
            let document::objects::DynamicBlockData::LinearParameter(parameter) = &mut object.data
            else {
                panic!()
            };
            parameter.parameter.parameter.chain_actions = chained;
            actions::create(doc, def, actions::Kind::Move, parent, &[child], DVec3::ZERO).unwrap();
            actions::create(doc, def, actions::Kind::Move, child, &[line], DVec3::ZERO).unwrap();
            let columns: Vec<_> = tables::columns(doc, def)
                .into_iter()
                .filter(|c| c.handle == parent)
                .collect();
            assert_eq!(columns.len(), 1);
            tables::apply(doc, def, &columns, &[vec![10.], vec![20.]], false).unwrap();
            let target = actions::all_parameters(doc, def)
                .into_iter()
                .find(|p| p.handle == child)
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
            let source_before = app.tabs[source].scene.document.clone();
            let input = actions::Value::Point(target.end + DVec3::X * 2.);
            let doc = &app.tabs[i].scene.document;
            let changed = if testing {
                actions::instance::evaluate_test_value(
                    doc,
                    &app.tabs[i].block_test.as_ref().unwrap().baseline,
                    def,
                    insert,
                    child,
                    input,
                )
            } else {
                actions::instance::evaluate_value(doc, insert, child, input)
            }
            .unwrap()
            .unwrap();
            app.tabs[i].scene.document = changed;
            app.tabs[i].scene.bump_geometry();
            app.tabs[i].scene.select_entity(insert, true);
            app.tabs[i].selected_handle = Some(insert);
            app.refresh_properties();
            let (key, selected, _) = row(&app);
            assert_eq!(selected, 0);
            let depth = app.tabs[i].history.undo_stack.len();
            let check = |app: &OpenCADStudio, selected: usize, offset: f64| {
                let doc = &app.tabs[i].scene.document;
                assert_eq!(row(app).1, selected);
                let frame = actions::instance::frame(doc, insert, &target).unwrap();
                assert_eq!(frame.base, target.base + DVec3::X * offset);
                assert_eq!(frame.end, target.end + DVec3::X * (offset + 2.));
                let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
                    panic!()
                };
                let line = doc
                    .entities_in_block(&reference.block_name)
                    .find_map(|e| {
                        if let EntityType::Line(line) = e {
                            Some(line)
                        } else {
                            None
                        }
                    })
                    .unwrap();
                assert_eq!(
                    line.start,
                    Vector3::new(2. + if chained { offset } else { 0. }, 0., 0.)
                );
                if testing {
                    assert_eq!(app.tabs[source].scene.document, source_before);
                } else {
                    let other_frame = actions::instance::frame(doc, other, &target).unwrap();
                    assert_eq!(other_frame.base, target.base);
                    assert_eq!(other_frame.end, target.end);
                }
            };
            check(&app, 0, 0.);
            let _ = app.update(Message::PropBlockChoice { key, index: 1 });
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
            check(&app, 1, 10.);
            app.undo_steps(1);
            check(&app, 0, 0.);
            app.redo_steps(1);
            check(&app, 1, 10.);
        }
    }
}

#[test]
fn drawing1_dxf_and_berb_dwg_stretch_grips_evaluate_without_error() {
    let dxf_path = r"C:\Users\Atelier La Medina\AppData\Local\Temp\Rar$DRa40168.24845.rartemp\Drawing1.dxf";
    if std::path::Path::new(dxf_path).exists() {
        let doc = document::DxfReader::from_file(std::path::Path::new(dxf_path))
            .unwrap()
            .read()
            .unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        app.tabs[i].scene.document = doc.clone();

        for e in doc.entities() {
            if let EntityType::Insert(ins) = e {
                if let Some(_def_handle) = actions::instance::definition(&doc, ins.common.handle) {
                    app.tabs[i].scene.select_entity(ins.common.handle, true);
                    app.tabs[i].selected_handle = Some(ins.common.handle);
                    app.refresh_properties();

                    let grips = app.tabs[i].selected_grips.clone();
                    for g in grips {
                        if g.id != 0 {
                            let started = app.start_test_action_grip(ins.common.handle, g.id);
                            if started {
                                if let Some(mut cmd) = app.tabs[i].active_cmd.take() {
                                    let pt = g.world + glam::DVec3::new(5.0, 0.0, 0.0);
                                    let res = cmd.on_point(pt);
                                    let _ = app.apply_cmd_result(res);
                                    assert!(
                                        app.command_line.last_error.is_none(),
                                        "Drawing1.dxf grip evaluation failed: {:?}",
                                        app.command_line.last_error
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let dwg_path = r"C:\Users\Atelier La Medina\Desktop\berb.dwg";
    if std::path::Path::new(dwg_path).exists() {
        let doc = document::DwgReader::from_file(std::path::Path::new(dwg_path))
            .unwrap()
            .read()
            .unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        app.tabs[i].scene.document = doc.clone();

        for e in doc.entities() {
            if let EntityType::Insert(ins) = e {
                if let Some(_def_handle) = actions::instance::definition(&doc, ins.common.handle) {
                    app.tabs[i].scene.select_entity(ins.common.handle, true);
                    app.tabs[i].selected_handle = Some(ins.common.handle);
                    app.refresh_properties();

                    let grips = app.tabs[i].selected_grips.clone();
                    for g in grips {
                        if g.id != 0 {
                            let started = app.start_test_action_grip(ins.common.handle, g.id);
                            if started {
                                if let Some(mut cmd) = app.tabs[i].active_cmd.take() {
                                    let pt = g.world + glam::DVec3::new(5.0, 0.0, 0.0);
                                    let res = cmd.on_point(pt);
                                    let _ = app.apply_cmd_result(res);
                                    assert!(
                                        app.command_line.last_error.is_none(),
                                        "berb.dwg grip evaluation failed: {:?}",
                                        app.command_line.last_error
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
