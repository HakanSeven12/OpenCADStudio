use super::*;
use crate::scene::model::object::{BlockParameterKey, PropValue};
use crate::scene::{block_actions as actions, block_lookup as lookup};
use crate::ui::window::block_lookup::Action as A;

fn choice(app: &OpenCADStudio) -> (BlockParameterKey, usize) {
    app.tabs[app.active_tab]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .find_map(|p| match &p.value {
            PropValue::BlockChoice { key, selected, .. } if p.field == "dyn_lookup" => {
                Some((*key, *selected))
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn block_lookup_expression_authoring_audit_history_and_literal_persistence() {
    let (mut app, def, parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let literal = " =sqrt(9) + 1 ";
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, literal.into())));
    let _ = app.update(Message::BlockLookup(A::Audit));
    assert!(app.block_lookup.as_ref().unwrap().error.is_none());
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let authored = app.tabs[i].scene.document.clone();
    assert_eq!(
        lookup::table(&authored, def, parameter)
            .unwrap()
            .unwrap()
            .1
            .expressions[0],
        literal
    );
    app.undo_steps(1);
    assert!(app.tabs[i].scene.document == before);
    app.redo_steps(1);
    assert!(app.tabs[i].scene.document == authored);
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&authored, extension, authored.version).unwrap();
        let loaded =
            crate::io::load_bytes(&format!("lookup-expression.{extension}"), bytes).unwrap();
        assert_eq!(
            lookup::table(&loaded, def, parameter)
                .unwrap()
                .unwrap()
                .1
                .expressions[0],
            literal
        );
    }
    let _ = app.dispatch_command("BLOOKUPTABLE");
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "Distance1+1".into())));
    let _ = app.update(Message::BlockLookup(A::Audit));
    assert!(app.block_lookup.as_ref().unwrap().error.is_some());
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_some());
    assert!(app.tabs[i].scene.document == authored);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::CloseModal);
}

#[test]
fn block_lookup_range_and_formula_properties_evaluate_in_drawing_and_test_block() {
    for testing in [false, true] {
        let (mut app, def, lookup) = lookup_editor_fixture();
        let source = app.active_tab;
        let input = actions::all_parameters(&app.tabs[source].scene.document, def)[0].handle;
        let member = members(&app)[0];
        actions::create(
            &mut app.tabs[source].scene.document,
            def,
            actions::Kind::Move,
            input,
            &[member],
            glam::DVec3::ZERO,
        )
        .unwrap();
        let reference = app.tabs[source]
            .scene
            .document
            .entities()
            .find_map(|e| match e {
                EntityType::Insert(i) if i.block_name == "EDIT_ME" => Some(i.common.handle),
                _ => None,
            })
            .unwrap();
        let _ = app.dispatch_command("BLOOKUPTABLE");
        edit_lookup(&mut app);
        let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "[2,6)".into())));
        let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Range".into())));
        let _ = app.update(Message::BlockLookup(A::AddRow));
        let _ = app.update(Message::BlockLookup(A::Cell(1, 0, "=pow(2,3)".into())));
        let _ = app.update(Message::BlockLookup(A::Cell(1, 1, "Formula".into())));
        let _ = app.update(Message::BlockLookup(A::Apply));
        assert!(app.block_lookup.is_none());
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
        assert_eq!(choice(&app).1, 0);
        let source_before = app.tabs[source].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let (key, _) = choice(&app);
        let _ = app.update(Message::PropBlockChoice { key, index: 1 });
        assert_eq!(choice(&app).1, 1);
        let (key, _) = choice(&app);
        let _ = app.update(Message::PropBlockChoice { key, index: 0 });
        assert_eq!(choice(&app).1, 0);
        let p = actions::all_parameters(&app.tabs[i].scene.document, def)
            .into_iter()
            .find(|p| p.handle == input)
            .unwrap();
        let frame = if testing {
            actions::instance::test_frame(&app.tabs[i].scene.document, insert, &p)
        } else {
            actions::instance::frame(&app.tabs[i].scene.document, insert, &p)
        }
        .unwrap();
        assert!(frame.end.distance(frame.base) < 6. && frame.end.distance(frame.base) > 5.9);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        let (key, _) = choice(&app);
        let _ = app.update(Message::PropBlockChoice { key, index: 0 });
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        app.undo_steps(1);
        assert_eq!(choice(&app).1, 1);
        app.redo_steps(1);
        assert_eq!(choice(&app).1, 0);
        let table = lookup::table(&app.tabs[i].scene.document, def, lookup)
            .unwrap()
            .unwrap()
            .1;
        assert_eq!(
            table.expressions,
            ["[2,6)", "Range", "=pow(2,3)", "Formula"]
        );
        if testing {
            assert!(app.tabs[source].scene.document == source_before);
        }
    }
}

#[test]
fn block_lookup_input_rename_properties_rewrite_and_undo_the_native_table_together() {
    let (mut app, def, lookup) = lookup_editor_fixture();
    let i = app.active_tab;
    let input = actions::all_parameters(&app.tabs[i].scene.document, def)[0].handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 3,0 0,1");
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let second = app
        .block_lookup
        .as_ref()
        .unwrap()
        .properties
        .iter()
        .position(|p| p.label == "Distance2")
        .unwrap();
    let _ = app.update(Message::BlockLookup(A::Property(second)));
    let _ = app.update(Message::BlockLookup(A::AddProperty));
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Distance1 * 2".into())));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    app.tabs[i].selected_authoring = Some(input);
    app.refresh_properties();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropGeomInput {
        field: "authoring_name",
        value: "Panel width".into(),
    });
    let _ = app.update(Message::PropGeomCommit("authoring_name"));
    let changed = app.tabs[i].scene.document.clone();
    assert_eq!(
        lookup::table(&changed, def, lookup)
            .unwrap()
            .unwrap()
            .1
            .expressions[1],
        "`Panel width` * 2"
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert!(app.tabs[i].scene.document == before);
    app.redo_steps(1);
    assert!(app.tabs[i].scene.document == changed);
}
