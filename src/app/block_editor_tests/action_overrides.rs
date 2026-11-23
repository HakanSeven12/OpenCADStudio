use super::*;

#[test]
fn block_action_overrides_properties_are_atomic_and_locally_undoable() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
    let targets = members(&app);
    let action = actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.update(Message::BlockActionBarSelect(action));
    let fields = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .map(|p| p.field)
        .collect::<Vec<_>>();
    assert!(fields.contains(&"authoring_action_multiplier"));
    assert!(fields.contains(&"authoring_action_angle"));
    let initial = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for (field, value) in [
        ("authoring_action_multiplier", "1/2"),
        ("authoring_action_angle", "90"),
    ] {
        let _ = app.update(Message::PropGeomInput {
            field,
            value: value.into(),
        });
        let _ = app.update(Message::PropGeomCommit(field));
    }
    let offsets = actions::options::offsets(&app.tabs[i].scene.document, action).unwrap();
    assert_eq!(offsets.distance_multiplier, 0.5);
    assert_eq!(offsets.angle_offset, std::f64::consts::FRAC_PI_2);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    let changed = app.tabs[i].scene.document.clone();
    for value in ["NaN", "inf", "unresolved_name"] {
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_action_multiplier",
            value: value.into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_action_multiplier"));
        assert_eq!(app.tabs[i].scene.document, changed);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    }
    app.undo_steps(2);
    assert_eq!(
        app.tabs[i].scene.document.objects[&action],
        initial.objects[&action]
    );
    app.redo_steps(2);
    assert_eq!(
        app.tabs[i].scene.document.objects[&action],
        changed.objects[&action]
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomCommit("authoring_action_multiplier"));
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn the_chain_actions_row_is_offered_only_where_the_actions_run() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_orientation_parameter::{self as orientation, Kind as OrientationKind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let linear = actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 TYPE P 10,0");
    let alignment = orientation::parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|p| p.kind == OrientationKind::Alignment)
        .unwrap()
        .handle;
    let chained_row = |app: &OpenCADStudio| {
        app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .any(|p| p.field == "authoring_chain_actions")
    };
    let show = |app: &mut OpenCADStudio, handle| {
        app.tabs[i].selected_authoring = Some(handle);
        app.refresh_properties();
    };
    show(&mut app, linear);
    assert!(
        chained_row(&app),
        "a parameter whose actions run can chain them"
    );
    show(&mut app, alignment);
    assert!(
        !chained_row(&app),
        "a parameter whose actions never run has no order to declare"
    );
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBoolToggle("authoring_chain_actions"));
    assert_eq!(
        app.tabs[i].scene.document, before,
        "the same click on that parameter writes nothing"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn the_chain_row_records_the_action_whose_effect_runs_next() {
    use crate::scene::block_actions::{self as actions, chain, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
    let targets = members(&app);
    let member = targets[0];
    let move_action = actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let scale_action = actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Scale,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let name = |doc: &document::Document, action: Handle| {
        actions::actions(doc, definition)
            .into_iter()
            .find(|(handle, ..)| *handle == action)
            .map(|(_, action, _, _)| action.element.name.clone())
            .unwrap()
    };
    let move_name = name(&app.tabs[i].scene.document, move_action);
    let fields = |app: &OpenCADStudio| {
        app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .map(|p| p.field)
            .collect::<Vec<_>>()
    };
    let _ = app.update(Message::BlockActionBarSelect(parameter));
    assert!(
        !fields(&app).contains(&"authoring_action_chain"),
        "a parameter has no chain to name"
    );
    let _ = app.update(Message::BlockActionBarSelect(scale_action));
    assert!(fields(&app).contains(&"authoring_action_chain"));
    assert_eq!(
        chain::labels(&app.tabs[i].scene.document, definition, scale_action),
        Some(vec![])
    );
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    // The parameter runs from ten to twenty units, so the move shifts the member
    // ten along X and the scale doubles it about the origin it starts at.
    let drive = |doc: &document::Document| {
        let baseline = doc.clone();
        let mut driven = doc.clone();
        actions::evaluate(
            &mut driven,
            &baseline,
            definition,
            parameter,
            glam::DVec3::new(20., 0., 0.),
        )
        .unwrap();
        let EntityType::Line(line) = driven.get_entity(member).unwrap() else {
            panic!("the acted-on member is still a line")
        };
        (line.start.x, line.end.x)
    };
    let type_name = |app: &mut OpenCADStudio, value: &str| {
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_action_chain",
            value: value.into(),
        });
        app.update(Message::PropGeomCommit("authoring_action_chain"))
    };
    let _ = type_name(&mut app, "NoSuchAction");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "an unknown name chains nothing"
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = type_name(&mut app, &move_name);
    assert_eq!(
        chain::labels(&app.tabs[i].scene.document, definition, scale_action),
        Some(vec![move_name.clone()])
    );
    let chained = app.tabs[i].scene.document.clone();
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        drive(&chained),
        (10., 12.),
        "the scale runs first and hands its result to the move"
    );
    assert_ne!(
        drive(&before),
        drive(&chained),
        "the chain the row records moves the geometry"
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.objects[&scale_action],
        before.objects[&scale_action]
    );
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.objects[&scale_action],
        chained.objects[&scale_action]
    );
    let _ = type_name(&mut app, " ");
    assert_eq!(
        chain::labels(&app.tabs[i].scene.document, definition, scale_action),
        Some(vec![])
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    assert_eq!(
        drive(&app.tabs[i].scene.document),
        drive(&before),
        "clearing the row hands the order back"
    );

    let doc = &mut app.tabs[i].scene.document;
    let extra = actions::create(
        doc,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let action_list = actions::actions(doc, definition);
    let driver = action_list
        .iter()
        .find(|(handle, ..)| *handle == scale_action)
        .unwrap()
        .3;
    let ids: Vec<_> = [extra, move_action]
        .iter()
        .map(|handle| {
            action_list
                .iter()
                .find(|(candidate, ..)| candidate == handle)
                .unwrap()
                .1
                .element
                .eval
                .node_id
        })
        .collect();
    let names = [name(doc, extra), move_name];
    let list = serde_json::to_string(&names).unwrap();
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get_mut(&scale_action).unwrap()
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::ScaleAction(scale) = &mut object.data else {
        panic!()
    };
    scale.action.action.action_ids = vec![driver];
    let _ = app.update(Message::BlockActionBarSelect(scale_action));
    let branch_depth = app.tabs[i].history.undo_stack.len();
    let _ = type_name(&mut app, &list);
    assert_eq!(app.tabs[i].history.undo_stack.len(), branch_depth + 1);
    assert_eq!(
        chain::labels(&app.tabs[i].scene.document, definition, scale_action),
        Some(names.to_vec())
    );
    assert!(app.tabs[i].properties.sections.iter().flat_map(|s| &s.props).any(|property| {
        property.field == "authoring_action_chain"
            && matches!(&property.value, crate::scene::model::object::PropValue::PlainText(text) if *text == list)
    }));
    let retained = |app: &OpenCADStudio| {
        actions::actions(&app.tabs[i].scene.document, definition)
            .into_iter()
            .find(|(handle, ..)| *handle == scale_action)
            .unwrap()
            .1
            .action_ids
            .clone()
    };
    let expected: Vec<_> = std::iter::once(driver).chain(ids).collect();
    assert_eq!(retained(&app), expected);
    assert_eq!(drive(&app.tabs[i].scene.document), (20., 22.));
    let _ = type_name(&mut app, &list);
    let invalid = serde_json::to_string(&[names[0].as_str(), "NoSuchAction"]).unwrap();
    let _ = type_name(&mut app, &invalid);
    assert_eq!(retained(&app), expected);
    assert_eq!(app.tabs[i].history.undo_stack.len(), branch_depth + 1);
    app.undo_steps(1);
    assert_eq!(retained(&app), [driver]);
    app.redo_steps(1);
    assert_eq!(retained(&app), expected);
    let _ = type_name(&mut app, "[]");
    assert_eq!(retained(&app), [driver]);
    let _ = app.dispatch_command("BEDIT_DISCARD");
}
