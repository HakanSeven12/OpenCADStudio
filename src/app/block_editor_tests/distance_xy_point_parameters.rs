use super::*;

#[test]
fn distance_parameter_batch_three_picks_local_undo_redo_and_saved_parent_undo() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for (index, kind) in ["LINEAR", "POLAR", "XY"].iter().enumerate() {
        let before = app.tabs[i].scene.document.clone();
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        feed(&mut app, "2,3");
        feed(&mut app, "6,7");
        assert_eq!(
            app.tabs[i].scene.document, before,
            "base and endpoint are preview only"
        );
        if *kind == "XY" {
            feed(&mut app, "1");
        } else {
            feed(&mut app, "1,8");
        }
        if *kind == "LINEAR" {
            feed(&mut app, "");
        }
        assert_eq!(distance_parameter_count(&app), index + 1);
        assert_eq!(app.tabs[i].history.undo_stack.len(), index + 1);
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        app.undo_steps(1);
        assert_eq!(distance_parameter_count(&app), index);
        app.redo_steps(1);
        assert_eq!(distance_parameter_count(&app), index + 1);
    }
    let _ = app.run_command_line("BPARAMETER POINT 0,0 1,1");
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let editor = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    assert!(app.tabs[app.active_tab]
        .active_block_edit_session()
        .is_none());
    assert_eq!(
        app.tabs[app.active_tab]
            .scene
            .document
            .entities_in_block("EDIT_ME")
            .count(),
        1,
        "parameters and their editor graphics add no drawing geometry"
    );
    assert_eq!(distance_parameter_count(&app), 3);
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.active_tab, i);
    assert_eq!(app.tabs[i].scene.document, editor);
    let _ = app.run_command_line("UCS ORIGIN 1,2,0");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    for p in
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
    {
        assert_eq!(p.native.definition_base_point, Vector3::new(1.0, 1.0, 0.0));
        assert_eq!(p.native.definition_end_point, Vector3::new(5.0, 5.0, 0.0));
    }
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(distance_parameter_count(&app), 3);
    app.undo_steps(1);
    assert_eq!(distance_parameter_count(&app), 0);
    app.redo_steps(1);
    assert_eq!(distance_parameter_count(&app), 3);
}

#[test]
fn xy_parameter_prompts_for_grip_count_and_places_an_automatic_label() {
    use crate::scene::block_grips::config;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameters = |app: &OpenCADStudio| -> Vec<Handle> {
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|parameter| parameter.handle)
            .collect()
    };
    let _ = app.dispatch_command("BPARAMETER XY");
    feed(&mut app, "0,0");
    feed(&mut app, "6,2");
    let prompt = app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .to_string();
    assert!(
        prompt.contains("Specify number of grips [0/1/2/4] <1>:"),
        "{prompt}"
    );
    let before = app.tabs[i].scene.document.clone();
    for (invalid, reason) in [
        ("3", "Enter the number of grips: 0, 1, 2 or 4."),
        ("-1", "Enter the number of grips: 0, 1, 2 or 4."),
        ("five", "Requires an integer value."),
        ("1,8", "Requires an integer value."),
        ("2.5", "Requires an integer value."),
    ] {
        app.command_line.last_error = None;
        feed(&mut app, invalid);
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(reason),
            "{invalid}"
        );
        assert!(app.tabs[i].active_cmd.is_some());
    }
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(distance_parameter_count(&app), 1);
    let default = parameters(&app)[0];
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, default),
        Some(1)
    );
    let placed =
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .next()
            .unwrap();
    assert_eq!(placed.offsets, [2.0, 6.0], "the label follows the endpoint");
    let mut seen = vec![default];
    for count in ["4", "0", "2"] {
        let _ = app.dispatch_command("BPARAMETER XY");
        feed(&mut app, "0,0");
        feed(&mut app, "6,2");
        feed(&mut app, count);
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the grip count commits the parameter"
        );
        let placed = *parameters(&app)
            .iter()
            .find(|handle| !seen.contains(handle))
            .unwrap();
        seen.push(placed);
        assert_eq!(
            config::count(&app.tabs[i].scene.document, definition, placed),
            Some(count.parse().unwrap())
        );
    }
}

#[test]
fn xy_parameter_stores_endpoint_distances_and_reads_typed_points_absolutely() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let values = |app: &OpenCADStudio| -> ([f64; 2], Vector3, Vector3) {
        let parameters = crate::scene::block_distance_parameter::parameters(
            &app.tabs[i].scene.document,
            definition,
        );
        let placed = parameters.last().unwrap();
        let document::objects::ObjectType::DynamicBlock(object) =
            &app.tabs[i].scene.document.objects[&placed.handle]
        else {
            panic!()
        };
        let document::objects::DynamicBlockData::XYParameter(parameter) = &object.data else {
            panic!()
        };
        (
            [parameter.x_value, parameter.y_value],
            placed.native.definition_base_point,
            placed.native.definition_end_point,
        )
    };

    let _ = app.dispatch_command("BPARAMETER XY");
    feed(&mut app, "10,10");
    feed(&mut app, "60,30");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        values(&app),
        (
            [50., 20.],
            Vector3::new(10., 10., 0.),
            Vector3::new(60., 30., 0.)
        ),
        "x_value/y_value carry the endpoint distances"
    );
    let placed =
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .last()
            .unwrap();
    assert_eq!(
        placed.offsets,
        [20., 50.],
        "the automatic labels still sit on the dimension lines"
    );

    let _ = app.run_command_line("BPARAMETER XY 2,3 22,13");
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the bare Enter commits the default grip count"
    );
    assert_eq!(
        values(&app),
        (
            [20., 10.],
            Vector3::new(2., 3., 0.),
            Vector3::new(22., 13., 0.)
        ),
        "a typed pair is an absolute location on the automation path too"
    );
}

#[test]
fn distance_parameter_cancel_chooser_invalid_endpoint_and_discard_are_safe() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    for kind in ["Linear", "Polar", "XY"] {
        for picks in 0..3 {
            let _ = app.dispatch_command("BPARAMETER");
            feed(&mut app, kind);
            if picks > 0 {
                feed(&mut app, "2,3");
            }
            if picks > 1 {
                feed(&mut app, "6,7");
            }
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert!(app.tabs[i].history.undo_stack.is_empty());
        }
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        feed(&mut app, "2,3");
        feed(&mut app, "2,3");
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "invalid endpoint can be retried"
        );
        feed(&mut app, "6,7");
        if kind == "XY" {
            feed(&mut app, "1");
        } else {
            feed(&mut app, "1,8");
        }
        if kind == "Linear" {
            feed(&mut app, "");
        }
        assert_eq!(distance_parameter_count(&app), 1);
        app.undo_steps(1);
    }
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,1 2,2");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(distance_parameter_count(&app), 0);
    let _ = app.dispatch_command("LINE");
    for kind in ["LINEAR", "POLAR", "XY"] {
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
}

#[test]
fn distance_parameter_stale_context_and_invalid_result_do_not_start_history() {
    use crate::scene::block_distance_parameter::Kind;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let id = app.tabs[i].id;
    let before = app.tabs[i].scene.document.clone();
    for (document_id, end) in [
        (id + 1, glam::DVec3::ONE),
        (id, glam::DVec3::ZERO),
        (id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_distance_parameter(
            crate::command::CmdResult::PlaceBlockDistanceParameter {
                document_id,
                definition,
                kind: Kind::Linear,
                base: glam::DVec3::ZERO,
                end,
                label: glam::DVec3::ZERO,
                grips: None,
                options: Default::default(),
            },
        );
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
    }
}

#[test]
fn point_parameter_placement_two_picks_one_undo_and_repeated_names() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "3,4,0");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "first pick is preview only"
    );
    feed(&mut app, "6,7,0");
    assert_eq!(authored_points(&app)[0].position_name, "Position1");
    assert_eq!(
        authored_points(&app)[0].definition_label_point,
        Vector3::new(6.0, 7.0, 0.0)
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
    assert!(app.block_edit_dirty(i));
    app.undo_steps(1);
    assert!(authored_points(&app).is_empty());
    app.redo_steps(1);
    assert_eq!(authored_points(&app).len(), 1);
    let _ = app.run_command_line("BPARAMETER POINT 8,9 10,11");
    assert_eq!(authored_points(&app)[1].position_name, "Position2");
    assert_eq!(app.tabs[i].history.undo_stack.len(), 2);
}

#[test]
fn point_parameter_options_reach_the_placed_parameter() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let prompt = |app: &OpenCADStudio| app.tabs[i].active_cmd.as_ref().unwrap().prompt();
    let _ = app.dispatch_command("BPARAMETER POINT");
    assert_eq!(
        prompt(&app),
        "BPARAMETER  Specify parameter location or [Name/Label/Chain/Description/Palette]:"
    );
    feed(&mut app, "N");
    assert_eq!(prompt(&app), "Enter parameter name <Point>:");
    feed(&mut app, "Pt9");
    assert_eq!(
        prompt(&app),
        "BPARAMETER  Specify parameter location or [Name/Label/Chain/Description/Palette]:"
    );
    feed(&mut app, "L");
    assert_eq!(prompt(&app), "Enter position property label <Position1>:");
    feed(&mut app, "Pin9");
    feed(&mut app, "D");
    assert_eq!(prompt(&app), "Enter property description:");
    feed(&mut app, "audit note");
    feed(&mut app, "C");
    assert_eq!(
        prompt(&app),
        "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
    );
    feed(&mut app, "Yes");
    feed(&mut app, "P");
    assert_eq!(
        prompt(&app),
        "Display property in Properties palette? [Yes/No] <Yes>:"
    );
    feed(&mut app, "No");
    feed(&mut app, "3,4");
    feed(&mut app, "6,7");
    let point = &authored_points(&app)[0];
    assert_eq!(point.parameter.parameter.element.name, "Pt9");
    assert_eq!(point.position_name, "Pin9");
    assert_eq!(point.position_description, "audit note");
    assert!(point.parameter.parameter.chain_actions);
    assert!(!point.parameter.parameter.show_properties);
}

#[test]
fn point_parameter_defaults_and_duplicate_name_rule() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER POINT 3,4 6,7");
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter position property label <Position2>:"
    );
    feed(&mut app, "");
    feed(&mut app, "8,9");
    feed(&mut app, "10,11");
    assert_eq!(authored_points(&app)[1].position_name, "Position2");
    assert_eq!(
        authored_points(&app)[1].parameter.parameter.element.name,
        "Point"
    );
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "N");
    feed(&mut app, "Pt9");
    feed(&mut app, "12,13");
    feed(&mut app, "14,15");
    assert_eq!(
        authored_points(&app)[2].parameter.parameter.element.name,
        "Pt9"
    );
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "N");
    feed(&mut app, "Pt9");
    feed(&mut app, "16,17");
    feed(&mut app, "18,19");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Parameter names must be unique.")
    );
    let refused = &authored_points(&app)[3];
    assert_eq!(refused.parameter.parameter.element.name, "Point");
    assert_eq!(refused.position_name, "Position4");
}

#[test]
fn point_parameter_placement_cancel_at_both_prompts_is_inert() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    for picked in [false, true] {
        let _ = app.dispatch_command("BPARAMETER POINT");
        if picked {
            feed(&mut app, "3,4");
        }
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
        assert!(!app.block_edit_dirty(i));
    }
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "3,4");
    feed(&mut app, "");
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(app.tabs[i].history.undo_stack.is_empty());
}

#[test]
fn visibility_set_row_edits_the_existing_parameter_instead_of_refusing() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    assert!(
        app.block_visibility.is_none(),
        "placing the parameter does not open its dialog"
    );
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BPARAMETERSET Visibility Set");
    assert!(
        app.block_visibility.is_some(),
        "the row on a definition that already has the parameter opens its states dialog"
    );
    assert_eq!(app.tabs[i].scene.document, before, "nothing is re-authored");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(
        app.tabs[i].pending_parameter_set_follow_up, None,
        "the redirect leaves no follow-up waiting to run against a later placement"
    );
    let _ = app.update(crate::app::Message::CloseModal);
    // A bare placement of a second visibility parameter is still the refusal
    // the model requires; only the row redirects to the dialog.
    let _ = app.run_command_line("BPARAMETER VISIBILITY 4,4");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("This block already has a visibility parameter."),
        "the refusal names the rule"
    );
    assert!(
        app.block_visibility.is_none(),
        "a refused placement opens nothing"
    );
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn point_parameter_placement_chooser_works_with_existing_visibility_and_reframes() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "Point");
    feed(&mut app, "3,4,5");
    feed(&mut app, "6,7,5");
    assert_eq!(authored_points(&app).len(), 1);
    assert!(app.block_visibility_context().is_some());
    let _ = app.run_command_line("UCS ORIGIN 1,2,3");
    assert_eq!(
        authored_points(&app)[0].parameter.definition_point,
        Vector3::new(2.0, 2.0, 2.0)
    );
    assert_eq!(
        authored_points(&app)[0].definition_label_point,
        Vector3::new(5.0, 5.0, 2.0)
    );
    app.undo_steps(1);
    assert_eq!(
        authored_points(&app)[0].parameter.definition_point,
        Vector3::new(3.0, 4.0, 5.0)
    );
    let _ = app.dispatch_command("BSAVE");
    assert!(!app.block_edit_dirty(i));
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(authored_points(&app).len(), 1);
    app.undo_steps(1);
    assert!(
        authored_points(&app).is_empty(),
        "saved block undo restores the parent drawing"
    );
}

#[test]
fn point_parameter_placement_discard_and_stale_context_leave_no_objects() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    begin(&mut app, reference);
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let start = app.tabs[i].scene.document.clone();
    let id = app.tabs[i].id;
    for (document, location) in [
        (id + 1, glam::DVec3::ZERO),
        (id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_point_parameter(
            crate::command::CmdResult::PlaceBlockPointParameter {
                document_id: document,
                definition,
                location,
                label: glam::DVec3::ZERO,
                options: Default::default(),
            },
        );
        assert_eq!(app.tabs[i].scene.document, start);
    }
    let _ = app.run_command_line("BPARAMETER POINT 3,4 6,7");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let mut after = app.tabs[i].scene.document.clone();
    after.vports = before.vports.clone();
    assert_eq!(after, before);
    let closed = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BPARAMETER POINT");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    assert_eq!(app.tabs[i].scene.document, closed);
}

#[test]
fn a_chain_answer_is_recorded_on_the_placed_point_parameter() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "Point");
    feed(&mut app, "C");
    feed(&mut app, "Yes");
    feed(&mut app, "3,4,5");
    feed(&mut app, "6,7,5");
    let placed = authored_points(&app);
    assert_eq!(placed.len(), 1, "the answer still places the parameter");
    assert!(
        placed[0].parameter.parameter.chain_actions,
        "the chained answer is stored on the parameter"
    );
}

#[test]
fn a_point_parameter_left_at_the_chain_default_keeps_actions_unchained() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "Point");
    feed(&mut app, "C");
    feed(&mut app, "");
    feed(&mut app, "3,4,5");
    feed(&mut app, "6,7,5");
    let placed = authored_points(&app);
    assert_eq!(placed.len(), 1);
    assert!(
        !placed[0].parameter.parameter.chain_actions,
        "the offered default leaves the flag clear"
    );
}
