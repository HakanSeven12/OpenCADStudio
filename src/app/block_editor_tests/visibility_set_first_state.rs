use super::*;

/// A first `Visibility Set` row authors the parameter and its initial state.
/// That state has to keep the definition's objects on screen: a state with no
/// objects listed is not "everything visible", it is nothing visible, and the
/// block goes blank the moment the row finishes.
#[test]
fn visibility_set_row_keeps_every_object_visible_in_the_first_state() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i]
        .active_block_edit_session()
        .expect("an editor session is open")
        .br_handle;

    let _ = app.dispatch_command("BPARAMETERSET Visibility Set");
    feed(&mut app, "0,0");
    feed(&mut app, "");

    let parameter = crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, definition)
        .expect("the row authors the visibility parameter");
    assert!(
        !parameter.all_blocks.is_empty(),
        "the state set has to carry the definition's objects before it can show them"
    );
    let state = parameter
        .states
        .iter()
        .find(|state| state.name == "VisibilityState0")
        .expect("the first placement creates its initial state");
    assert_eq!(
        state.visible_blocks.len(),
        parameter.all_blocks.len(),
        "the initial state keeps every object visible"
    );
}

/// A member drawn after the definition already carries an evaluation graph is
/// new to that graph, and it still travels into the evaluated visibility copy.
/// There it has to be identified by a tag naming the copy itself: a tag carried
/// over from the definition points back at an object the copy does not contain.
#[test]
fn a_member_drawn_after_the_graph_reaches_the_visibility_copy_tagged() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER POINT 2,3 4,5");

    let _ = app.run_command_line("LINE 20,20 24,20");
    let _ = app.run_command_line("BPARAMETER VISIBILITY 8,8");
    let _ = app.dispatch_command("BTESTBLOCK");
    let i = app.active_tab;
    let insert = app.tabs[i].block_test.as_ref().unwrap().insert;
    app.apply_visibility_state(insert, 0);
    app.finish_pending_history(i);

    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        unreachable!();
    };
    let record = app.tabs[i]
        .scene
        .document
        .block_records
        .iter()
        .find(|record| record.name == ins.block_name)
        .expect("the state is read through its own copy of the definition");
    assert!(
        record.flags.anonymous,
        "and the copy is the evaluated one, not the definition the line was drawn in"
    );
    let late = record
        .entity_handles
        .iter()
        .copied()
        .find(|handle| {
            app.tabs[i]
                .scene
                .document
                .get_entity(*handle)
                .is_some_and(|entity| {
                    matches!(entity,EntityType::Line(line)
                        if (line.start.x,line.start.y,line.end.x,line.end.y) == (20.,20.,24.,20.))
                })
        })
        .expect("the member drawn after the graph is in the copy");
    let EntityType::Line(late) = app.tabs[i].scene.document.get_entity(late).unwrap() else {
        unreachable!();
    };
    let tag = late
        .common
        .extended_data
        .get_record("AcDbBlockRepETag")
        .expect("the member drawn after the graph carries an element tag in the copy");
    assert!(
        tag.values
            .iter()
            .any(|value| matches!(value, document::xdata::XDataValue::Handle(handle) if *handle == late.common.handle)),
        "and the tag names the object it sits on: {tag:?}"
    );
}

/// A definition that already carries an evaluation graph still has its members
/// tagged when a visibility parameter joins it: preparing tags only a new
/// graph, so a member drawn after the graph carries no tag until the
/// visibility parameter adds one, and would otherwise enter the state set
/// untagged.
#[test]
fn a_visibility_parameter_tags_a_member_the_existing_graph_has_not_tagged() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER POINT 2,3 4,5");
    let _ = app.run_command_line("LINE 20,20 24,20");
    let i = app.active_tab;
    let late = app.tabs[i]
        .scene
        .document
        .entities()
        .find(|entity| {
            matches!(entity, EntityType::Line(line)
                if (line.start.x, line.start.y, line.end.x, line.end.y) == (20., 20., 24., 20.))
        })
        .map(|entity| entity.common().handle)
        .expect("the member drawn after the graph is in the definition");
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(late)
            .unwrap()
            .common()
            .extended_data
            .get_record("AcDbBlockRepETag")
            .is_none(),
        "the point parameter's graph leaves the later member untagged"
    );
    let _ = app.run_command_line("BPARAMETER VISIBILITY 8,8");
    let tag = app.tabs[i]
        .scene
        .document
        .get_entity(late)
        .unwrap()
        .common()
        .extended_data
        .get_record("AcDbBlockRepETag")
        .expect("the visibility parameter tags the members the existing graph covers");
    assert!(
        tag.values.iter().any(
            |value| matches!(value, document::xdata::XDataValue::Handle(handle) if *handle == late)
        ),
        "and the tag names the object it sits on: {tag:?}"
    );
}
