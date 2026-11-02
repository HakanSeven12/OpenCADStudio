use super::*;
use crate::scene::block_lookup as lookup;
use crate::scene::model::object::{BlockParameterKey, PropValue};
use crate::ui::window::block_lookup::Action as A;

#[test]
fn lookup_editor_shared_outputs_apply_reopen_remove_and_undo() {
    let (mut app, def, first) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
    let (second, name) = lookup::parameters(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|(h, _)| *h != first)
        .map(|(h, p)| (h, p.lookup_name.clone()))
        .unwrap();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let state = app.block_lookup.as_ref().unwrap();
    let output = state
        .properties
        .iter()
        .position(|p| p.label == name)
        .unwrap();
    let _ = app.update(Message::BlockLookup(A::Property(output)));
    let _ = app.update(Message::BlockLookup(A::AddProperty));
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Alternate".into())));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    let shared = app.tabs[i].scene.document.clone();
    assert_eq!(
        lookup::table(&shared, def, first).unwrap(),
        lookup::table(&shared, def, second).unwrap()
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, shared);
    let _ = app.dispatch_command(&format!("BLOOKUPTABLE {name}"));
    assert_eq!(app.block_lookup.as_ref().unwrap().drafts.len(), 1);
    let _ = app.update(Message::BlockLookup(A::RemoveProperty(2)));
    assert_eq!(
        app.block_lookup.as_ref().unwrap().drafts[0]
            .edited
            .columns
            .len(),
        3,
        "primary protected"
    );
    let _ = app.update(Message::BlockLookup(A::RemoveProperty(1)));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(lookup::table(&app.tabs[i].scene.document, def, second)
        .unwrap()
        .is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, shared);
}

fn choices(app: &OpenCADStudio) -> Vec<(BlockParameterKey, usize)> {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .filter_map(|p| match &p.value {
            PropValue::BlockChoice { key, selected, .. } if p.field == "dyn_lookup" => {
                Some((*key, *selected))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn block_lookup_output_properties_use_indexed_callbacks_and_history_in_both_contexts() {
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
        let input = crate::scene::block_distance_parameter::create(
            doc,
            def,
            crate::scene::block_distance_parameter::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(2., 0., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        crate::scene::block_actions::create(
            doc,
            def,
            crate::scene::block_actions::Kind::Move,
            input,
            &[target],
            glam::DVec3::ZERO,
        )
        .unwrap();
        let first = lookup::create(doc, def, Vector3::ZERO).unwrap();
        let second = lookup::create(doc, def, Vector3::UNIT_Y).unwrap();
        let mut table = lookup::empty_table(
            lookup::parameters(doc, def)
                .into_iter()
                .find(|(h, _)| *h == first)
                .unwrap()
                .1,
        );
        table
            .columns
            .insert(0, lookup::properties(doc, def)[0].column.clone());
        table.columns.push(
            lookup::empty_table(
                lookup::parameters(doc, def)
                    .into_iter()
                    .find(|(h, _)| *h == second)
                    .unwrap()
                    .1,
            )
            .columns
            .remove(0),
        );
        table.column_count = 3;
        table.row_count = 2;
        table.expressions = ["2", "Small", "Large", "4", "Large", "Small"]
            .map(str::to_owned)
            .to_vec();
        lookup::apply_table(doc, def, first, table).unwrap();
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
            .map_or(reference, |t| t.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let source_before = app.tabs[source].scene.document.clone();
        let before = app.tabs[i].scene.document.clone();
        let keys = choices(&app);
        assert_eq!(keys.len(), 2);
        let depth = app.tabs[i].history.undo_stack.len();
        for (key, index) in [
            (keys[1].0, 0),
            (keys[1].0, 99),
            (
                BlockParameterKey {
                    document: keys[1].0.document + 1,
                    ..keys[1].0
                },
                1,
            ),
        ] {
            let _ = app.update(Message::PropBlockChoice { key, index });
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
        let _ = app.update(Message::PropBlockChoice {
            key: keys[1].0,
            index: 1,
        });
        assert!(choices(&app).iter().all(|p| p.1 == 1));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let changed = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropBlockChoice {
            key: keys[0].0,
            index: 1,
        });
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        // Undo restores drawing content while keeping allocated handles monotonic.
        let undone = &app.tabs[i].scene.document;
        assert_eq!(
            undone.entities().collect::<Vec<_>>(),
            before.entities().collect::<Vec<_>>()
        );
        assert_eq!(undone.objects, before.objects);
        assert_eq!(undone.block_records, before.block_records);
        assert!(choices(&app).iter().all(|p| p.1 == 0));
        app.redo_steps(1);
        assert_eq!(app.tabs[i].scene.document, changed);
        assert!(choices(&app).iter().all(|p| p.1 == 1));
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

#[test]
fn blocks_reports_unmatched_lookup_rows_as_present_and_unmatched() {
    let (mut app, reference) = fixture();
    let source = app.active_tab;
    begin(&mut app, reference);
    let def = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let target = members(&app)[0];
    let doc = &mut app.tabs[source].scene.document;
    let input = crate::scene::block_distance_parameter::create(
        doc,
        def,
        crate::scene::block_distance_parameter::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(2., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    crate::scene::block_actions::create(
        doc,
        def,
        crate::scene::block_actions::Kind::Move,
        input,
        &[target],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let first = lookup::create(doc, def, Vector3::ZERO).unwrap();
    let second = lookup::create(doc, def, Vector3::UNIT_Y).unwrap();
    let mut table = lookup::empty_table(
        lookup::parameters(doc, def)
            .into_iter()
            .find(|(h, _)| *h == first)
            .unwrap()
            .1,
    );
    table
        .columns
        .insert(0, lookup::properties(doc, def)[0].column.clone());
    table.columns.push(
        lookup::empty_table(
            lookup::parameters(doc, def)
                .into_iter()
                .find(|(h, _)| *h == second)
                .unwrap()
                .1,
        )
        .columns
        .remove(0),
    );
    table.column_count = 3;
    table.row_count = 2;
    table.expressions = ["2", "Small", "Large", "4", "Large", "Small"]
        .map(str::to_owned)
        .to_vec();
    lookup::apply_table(doc, def, first, table).unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    let insert = reference;
    app.tabs[i].scene.select_entity(insert, true);
    app.tabs[i].selected_handle = Some(insert);
    app.refresh_properties();
    let labels = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .filter(|property| property.field == "dyn_lookup")
        .map(|property| property.label.clone())
        .collect::<Vec<_>>();
    assert_eq!(labels.len(), 2);
    assert!(choices(&app).iter().all(|p| p.1 == 0));
    // Distance 3 falls between the authored rows of 2 and 4.
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
        value: "3".into(),
    });
    let _ = app.update(Message::PropBlockCommit(field));
    assert!(choices(&app).iter().all(|p| p.1 == usize::MAX));
    let (res, _) = app.control_request(serde_json::json!({
        "op": "blocks",
        "names": ["*"],
        "extract_dynamic_properties": true
    }));
    let block = res["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["handle"] == format!("{:X}", insert.value()).as_str())
        .unwrap();
    let properties = block["dynamic_properties"].as_object().unwrap();
    for label in labels {
        assert!(
            properties.contains_key(&label),
            "the row stays present even though no value applies to {label}: {res}"
        );
        assert_eq!(properties[&label], serde_json::Value::Null, "{res}");
    }
}
