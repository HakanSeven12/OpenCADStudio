use super::*;

#[test]
fn action_type_list_appears_when_only_a_lookup_parameter_is_placed() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.run_command_line("BPARAMETER LOOKUP 1,1");
    let _ = app.dispatch_command("BACTIONTOOL");
    let prompt = app.command_line.history.last().unwrap().text.clone();
    assert!(prompt.contains("BACTION  Select parameter:"), "{prompt}");
    feed(&mut app, "Lookup1");
    assert!(app.active_modal.is_none(), "the parameter comes first");
    assert_eq!(
        app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt(),
        "BACTION  Enter action type [Lookup]:"
    );
    feed(&mut app, "Lookup");
    assert_eq!(app.active_modal, Some(ModalKind::BlockLookup));
    assert_eq!(
        app.block_lookup.as_ref().unwrap().drafts[0]
            .original_parameter
            .lookup_name,
        "Lookup1"
    );
    let _ = app.update(Message::CloseModal);
}

#[test]
fn lookup_parameter_asks_for_a_grip_count_after_its_location() {
    use crate::scene::block_grips::config;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let placed = |app: &OpenCADStudio| {
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|(handle, parameter)| (handle, parameter.parameter.definition_point))
            .collect::<Vec<_>>()
    };
    let prompt = |app: &OpenCADStudio| {
        app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("command active")
            .prompt()
            .to_string()
    };

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "1,1");
    assert!(
        prompt(&app).contains("Enter number of grips [0/1] <1>:"),
        "{}",
        prompt(&app)
    );
    assert!(
        placed(&app).is_empty(),
        "the parameter waits for the grip count"
    );
    let before = app.tabs[i].scene.document.clone();
    for invalid in ["2", "-1", "four", "5,5"] {
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
    feed(&mut app, "0");
    assert!(app.tabs[i].active_cmd.is_none());
    let (first, location) = placed(&app)[0];
    assert_eq!(location, Vector3::new(1.0, 1.0, 0.0));
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, first),
        Some(0)
    );

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "3,3");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let (second, location) = placed(&app)[1];
    assert_eq!(location, Vector3::new(3.0, 3.0, 0.0));
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, second),
        Some(1)
    );

    let _ = app.dispatch_command("BPARAMETER BASEPOINT");
    feed(&mut app, "2,3");
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "a basepoint is one location"
    );
}

#[test]
fn alignment_visibility_and_lookup_name_their_labels_without_asking() {
    use crate::scene::block_orientation_parameter::{self as orientation, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER ALIGNMENT");
    feed(&mut app, "0,0");
    feed(&mut app, "0,2");
    assert!(app.tabs[i].active_cmd.is_none());

    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "4,4");
    assert!(app.tabs[i].active_cmd.is_none());

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "6,6");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter number of grips [0/1] <1>:",
        "the location is followed by the grip count"
    );
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let alignment = orientation::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == Kind::Alignment)
        .expect("alignment parameter");
    assert_eq!(alignment.label, "Alignment1");
    assert_eq!(
        orientation::graphic(&alignment).labels.first(),
        Some(&(Vector3::new(0.0, 2.0, 0.0), "Alignment1".to_string()))
    );
    assert_eq!(
        crate::scene::block_visibility::parameter(doc, definition).map(|p| p.name.as_str()),
        Some("Visibility1")
    );
    let lookups = crate::scene::block_lookup::parameters(doc, definition);
    assert_eq!(lookups.len(), 1);
    assert_eq!(lookups[0].1.lookup_name, "Lookup1");
    assert_eq!(
        lookups[0].1.parameter.definition_point,
        Vector3::new(6.0, 6.0, 0.0)
    );
}

#[test]
fn a_lookup_takes_its_name_description_and_palette_switch_at_the_placement_prompt() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Lookup1>:",
        "the name question offers the next free name"
    );
    feed(&mut app, "Grip9");
    feed(&mut app, "D");
    feed(&mut app, "shows the grip");
    feed(&mut app, "P");
    feed(&mut app, "No");
    feed(&mut app, "6,6");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let lookups = crate::scene::block_lookup::parameters(doc, definition);
    assert_eq!(lookups.len(), 1);
    assert_eq!(
        lookups[0].1.parameter.parameter.element.name, "Grip9",
        "the name answer names the parameter itself"
    );
    assert_eq!(
        lookups[0].1.lookup_name, "Lookup1",
        "and leaves the lookup property label at its default"
    );
    assert_eq!(lookups[0].1.lookup_description, "shows the grip");
    assert!(!lookups[0].1.parameter.parameter.show_properties);
    assert_eq!(
        lookups[0].1.parameter.definition_point,
        Vector3::new(6.0, 6.0, 0.0)
    );
}

#[test]
fn a_lookup_takes_its_property_label_at_the_placement_prompt() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter lookup property label <Lookup1>:",
        "the label question offers the next free label"
    );
    feed(&mut app, "Size");
    feed(&mut app, "6,6");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let lookups = crate::scene::block_lookup::parameters(doc, definition);
    assert_eq!(lookups.len(), 1);
    assert_eq!(lookups[0].1.lookup_name, "Size");
    assert_eq!(
        lookups[0].1.parameter.definition_point,
        Vector3::new(6.0, 6.0, 0.0)
    );
}

#[test]
fn a_point_takes_its_chain_and_palette_answers_at_the_placement_prompt() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER POINT");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER  Specify parameter location or [Name/Label/Chain/Description/Palette]:"
    );
    feed(&mut app, "C");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
    );
    feed(&mut app, "Z");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:",
        "an answer that is neither Yes nor No keeps the question"
    );
    feed(&mut app, "Yes");
    feed(&mut app, "P");
    feed(&mut app, "");
    feed(&mut app, "3,4");
    feed(&mut app, "6,7");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let points = crate::scene::block_point_parameter::parameters(doc, definition);
    assert_eq!(points.len(), 1);
    assert!(points[0].1.parameter.parameter.chain_actions);
    assert!(
        points[0].1.parameter.parameter.show_properties,
        "Enter at the palette question keeps the property visible"
    );
    assert_eq!(
        points[0].1.parameter.definition_point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    assert_eq!(
        points[0].1.definition_label_point,
        Vector3::new(6.0, 7.0, 0.0)
    );
}

#[test]
fn a_rotation_takes_its_chain_description_and_palette_answers_at_the_base_prompt() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Rotation  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "C");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
    );
    feed(&mut app, "Yes");
    feed(&mut app, "D");
    feed(&mut app, "turns the frame");
    feed(&mut app, "P");
    feed(&mut app, "");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "5,5");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let rotation = crate::scene::block_orientation_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| {
            parameter.kind == crate::scene::block_orientation_parameter::Kind::Rotation
        })
        .expect("placed rotation parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&rotation.handle).expect("rotation record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::RotationParameter(native) = &object.data else {
        panic!("expected a rotation parameter");
    };
    assert!(native.parameter.parameter.chain_actions);
    assert_eq!(native.angle_description, "turns the frame");
    assert!(
        native.parameter.parameter.show_properties,
        "Enter at the palette question keeps the property visible"
    );
}

#[test]
fn a_rotation_keeps_the_defaults_shown_at_each_question() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "C");
    feed(&mut app, "");
    feed(&mut app, "P");
    feed(&mut app, "No");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "5,5");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let rotation = crate::scene::block_orientation_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| {
            parameter.kind == crate::scene::block_orientation_parameter::Kind::Rotation
        })
        .expect("placed rotation parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&rotation.handle).expect("rotation record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::RotationParameter(native) = &object.data else {
        panic!("expected a rotation parameter");
    };
    assert!(!native.parameter.parameter.chain_actions);
    assert_eq!(native.angle_description, "");
    assert!(!native.parameter.parameter.show_properties);
    assert_eq!(
        native.value_set.flags,
        1 | 2,
        "a rotation placed without a value-set answer keeps its periodic default"
    );
    assert_eq!(native.value_set.maximum, std::f64::consts::TAU);
}

#[test]
fn a_linear_takes_its_chain_description_and_palette_answers_at_the_base_prompt() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:"
    );
    feed(&mut app, "C");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
    );
    feed(&mut app, "Yes");
    feed(&mut app, "D");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter property description:"
    );
    feed(&mut app, "ramp  length");
    feed(&mut app, "P");
    feed(&mut app, "No");
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "5,2");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let linear = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::Linear)
        .expect("placed linear parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&linear.handle).expect("linear record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::LinearParameter(native) = &object.data else {
        panic!("expected a linear parameter");
    };
    assert!(native.parameter.parameter.chain_actions);
    assert_eq!(native.distance_description, "ramp  length");
    assert!(
        !native.parameter.parameter.show_properties,
        "No at the palette question hides the property"
    );
}
