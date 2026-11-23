use super::*;

#[test]
fn orientation_parameter_batch_numeric_rotation_local_history_save_and_parent_undo() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for (index, (kind, inputs)) in [
        ("ROTATION", vec!["0,0", "2", "90", "5,5", ""]),
        ("ALIGNMENT", vec!["0,0", "0,2"]),
        ("FLIP", vec!["0,0", "0,2", "3,3"]),
    ]
    .into_iter()
    .enumerate()
    {
        let before = app.tabs[i].scene.document.clone();
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        for text in &inputs[..inputs.len() - 1] {
            feed(&mut app, text);
            assert_eq!(app.tabs[i].scene.document, before, "incomplete placement");
        }
        feed(&mut app, inputs.last().unwrap());
        assert_eq!(orientation_parameter_count(&app), index + 1, "{kind}");
        assert_eq!(app.tabs[i].history.undo_stack.len(), index + 1);
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        app.undo_steps(1);
        assert_eq!(orientation_parameter_count(&app), index);
        app.redo_steps(1);
        assert_eq!(orientation_parameter_count(&app), index + 1);
    }
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = &crate::scene::block_orientation_parameter::parameters(
        &app.tabs[i].scene.document,
        definition,
    )[0];
    assert!((p.native.definition_end_point - Vector3::new(0.0, 2.0, 0.0)).length() < 1e-10);
    assert_eq!(p.reference, Vector3::new(2.0, 0.0, 0.0));
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
        1
    );
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.tabs[i].scene.document, editor);
    let _ = app.run_command_line("UCS ORIGIN 1,2,0");
    for p in crate::scene::block_orientation_parameter::parameters(
        &app.tabs[i].scene.document,
        definition,
    ) {
        assert_eq!(
            p.native.definition_base_point,
            Vector3::new(-1.0, -2.0, 0.0)
        );
    }
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(orientation_parameter_count(&app), 3);
    app.undo_steps(1);
    assert_eq!(orientation_parameter_count(&app), 0);
    app.redo_steps(1);
    assert_eq!(orientation_parameter_count(&app), 3);
}

#[test]
fn orientation_parameter_chooser_cancels_invalid_picks_and_rotation_default_angle() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    for (kind, maximum) in [("R", 2), ("A", 1), ("F", 2)] {
        for picks in 0..=maximum {
            let _ = app.dispatch_command("BPARAMETER");
            feed(&mut app, kind);
            if picks > 0 {
                feed(&mut app, "0,0");
                feed(&mut app, "0,0");
                assert_eq!(app.tabs[i].scene.document, before);
            }
            if picks > 1 {
                feed(&mut app, "0,2");
            }
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert!(app.tabs[i].history.undo_stack.is_empty());
        }
    }
    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    feed(&mut app, "-2");
    assert_eq!(app.tabs[i].scene.document, before);
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "5,5");
    feed(&mut app, "");
    assert_eq!(orientation_parameter_count(&app), 1);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(orientation_parameter_count(&app), 0);
    let _ = app.dispatch_command("LINE");
    for kind in ["ROTATION", "ALIGNMENT", "FLIP"] {
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
}

#[test]
fn rotation_parameter_accepts_a_base_angle_option() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let prompt = |app: &OpenCADStudio| {
        app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("command active")
            .prompt()
            .to_string()
    };
    let near = |value: Vector3, x: f64, y: f64| (value - Vector3::new(x, y, 0.0)).length() < 1e-9;
    let parameter = |app: &OpenCADStudio, index: usize| {
        let mut all = crate::scene::block_orientation_parameter::parameters(
            &app.tabs[i].scene.document,
            definition,
        );
        let placed = all.swap_remove(index);
        (
            placed.native.definition_base_point,
            placed.reference,
            placed.native.definition_end_point,
        )
    };

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    assert!(
        prompt(&app).contains("Specify radius of parameter:"),
        "{}",
        prompt(&app)
    );
    feed(&mut app, "2");
    let default_prompt = "Specify default rotation angle or [Base angle] <0>:";
    assert!(prompt(&app).contains(default_prompt), "{}", prompt(&app));
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .options()
        .iter()
        .any(|option| option.keyword == "B"));
    feed(&mut app, "B");
    assert!(
        prompt(&app).contains("Specify base angle <0>:"),
        "{}",
        prompt(&app)
    );
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .options()
        .is_empty());
    let before = app.tabs[i].scene.document.clone();
    for (invalid, error) in [
        ("inf", "Specify a finite base angle."),
        ("0,0", "Specify a finite nonzero base angle direction."),
    ] {
        app.command_line.last_error = None;
        feed(&mut app, invalid);
        assert_eq!(app.tabs[i].scene.document, before, "{invalid}");
        assert_eq!(app.command_line.last_error.as_deref(), Some(error));
        assert!(
            prompt(&app).contains("Specify base angle <0>:"),
            "{}",
            prompt(&app)
        );
    }
    feed(&mut app, "30");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "the base angle alone does not place the parameter"
    );
    assert!(
        prompt(&app).contains(default_prompt),
        "the base angle returns to the default angle: {}",
        prompt(&app)
    );
    feed(&mut app, "");
    assert!(
        prompt(&app).contains("Specify label location:"),
        "{}",
        prompt(&app)
    );
    feed(&mut app, "1,1");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let (_, reference_angle, end) = parameter(&app, 0);
    assert!(near(reference_angle, 2.0 * 30f64.to_radians().cos(), 1.0));
    assert!(near(end, 2.0 * 30f64.to_radians().cos(), 1.0));

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    feed(&mut app, "B");
    feed(&mut app, "30");
    feed(&mut app, "60");
    assert!(
        prompt(&app).contains("Specify label location:"),
        "{}",
        prompt(&app)
    );
    feed(&mut app, "3,0");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let (_, reference_angle, end) = parameter(&app, 1);
    assert!(near(reference_angle, 2.0 * 30f64.to_radians().cos(), 1.0));
    assert!(near(end, 0.0, 2.0));

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "5,0");
    feed(&mut app, "2");
    feed(&mut app, "B");
    feed(&mut app, "5,3");
    assert!(
        prompt(&app).contains(default_prompt),
        "a base angle pick returns to the default angle: {}",
        prompt(&app)
    );
    feed(&mut app, "");
    feed(&mut app, "7,0");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let (base, reference_angle, end) = parameter(&app, 2);
    assert!(near(base, 5.0, 0.0));
    assert!(near(reference_angle, 5.0, 2.0));
    assert!(near(end, 5.0, 2.0));

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "10,0");
    feed(&mut app, "3");
    feed(&mut app, "BASE");
    assert!(
        prompt(&app).contains("Specify base angle <0>:"),
        "{}",
        prompt(&app)
    );
    feed(&mut app, "");
    assert!(
        prompt(&app).contains(default_prompt),
        "Enter takes the zero base angle: {}",
        prompt(&app)
    );
    feed(&mut app, "45");
    assert!(
        prompt(&app).contains("Specify label location:"),
        "{}",
        prompt(&app)
    );
    feed(&mut app, "13,1");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let (_, reference_angle, end) = parameter(&app, 3);
    assert!(near(reference_angle, 13.0, 0.0));
    assert!(near(
        end,
        10.0 + 3.0 * 45f64.to_radians().cos(),
        3.0 * 45f64.to_radians().sin()
    ));
    assert_eq!(orientation_parameter_count(&app), 4);
}

#[test]
fn rotation_parameter_takes_a_label_location_and_a_grip_count() {
    use crate::scene::block_grips::config;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let prompt = |app: &OpenCADStudio| {
        app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("command active")
            .prompt()
            .to_string()
    };
    let placed = |app: &OpenCADStudio| {
        crate::scene::block_orientation_parameter::parameters(
            &app.tabs[i].scene.document,
            definition,
        )
        .into_iter()
        .map(|parameter| {
            let label = crate::scene::block_orientation_parameter::graphic(&parameter)
                .labels
                .first()
                .map(|(point, _)| *point)
                .expect("rotation parameter label");
            (parameter.handle, parameter.offset, label)
        })
        .collect::<Vec<_>>()
    };

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    let before = app.tabs[i].scene.document.clone();
    feed(&mut app, "");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "the default angle asks for the label"
    );
    assert!(
        prompt(&app).contains("Specify label location:"),
        "{}",
        prompt(&app)
    );
    feed(&mut app, "-4,0");
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(
        prompt(&app).contains("Enter number of grips [0/1] <1>:"),
        "{}",
        prompt(&app)
    );
    for invalid in ["2", "-1", "four", "1,8"] {
        app.command_line.last_error = None;
        feed(&mut app, invalid);
        assert_eq!(app.tabs[i].scene.document, before, "{invalid}");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Enter the number of grips: 0 or 1."),
            "{invalid}"
        );
        assert!(app.tabs[i].active_cmd.is_some(), "{invalid}");
    }
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(orientation_parameter_count(&app), 1);
    let first = placed(&app)[0];
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, first.0),
        Some(1)
    );
    assert!(
        (first.1 - 2.0).abs() < 1e-9,
        "the label keeps its picked distance from the arc: {}",
        first.1
    );
    assert!(
        (first.2 - Vector3::new(-4.0, 0.0, 0.0)).length() < 1e-9,
        "the label sits where it was picked: {}",
        first.2
    );

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "5,0");
    feed(&mut app, "2");
    feed(&mut app, "90");
    feed(&mut app, "8.535534,3.535534");
    feed(&mut app, "0");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(orientation_parameter_count(&app), 2);
    let second = placed(&app)[1];
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, second.0),
        Some(0)
    );
    assert!(
        (second.1 - 3.0).abs() < 1e-4,
        "the label keeps its picked distance from the arc: {}",
        second.1
    );
    assert!(
        (second.2 - Vector3::new(8.535534, 3.535534, 0.0)).length() < 1e-4,
        "the label sits where it was picked: {}",
        second.2
    );
}

#[test]
fn orientation_parameter_stale_context_and_invalid_result_do_not_start_history() {
    use crate::scene::block_orientation_parameter::Kind;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let id = app.tabs[i].id;
    let before = app.tabs[i].scene.document.clone();
    for (document_id, end) in [
        (id + 1, glam::DVec3::Y),
        (id, glam::DVec3::ZERO),
        (id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_orientation_parameter(
            crate::command::CmdResult::PlaceBlockOrientationParameter {
                document_id,
                definition,
                kind: Kind::Flip,
                perpendicular: false,
                base: glam::DVec3::ZERO,
                end,
                reference: glam::DVec3::X,
                label_offset: None,
                grips: None,
                options: Default::default(),
            },
        );
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
    }
}
