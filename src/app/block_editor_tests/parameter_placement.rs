use super::*;

#[test]
fn a_rotation_takes_its_value_set() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "V");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter angle value set type [None/List/Increment] <None>:"
    );
    feed(&mut app, "I");
    feed(&mut app, "5");
    feed(&mut app, "0");
    feed(&mut app, "360");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "5,5");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let first = {
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
        assert_eq!(
            native.value_set.flags,
            1 | 2 | 4,
            "the answered set is stored on the parameter"
        );
        assert_eq!(native.value_set.increment, 5f64.to_radians());
        assert_eq!(native.value_set.minimum, 0.0);
        assert_eq!(native.value_set.maximum, 360f64.to_radians());
        rotation.handle
    };

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "V");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter list of angle values (separated by commas):"
    );
    feed(&mut app, "30,150");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "5,5");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let listed = crate::scene::block_orientation_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| {
            parameter.kind == crate::scene::block_orientation_parameter::Kind::Rotation
                && parameter.handle != first
        })
        .expect("the rotation with the list");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&listed.handle).expect("rotation record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::RotationParameter(native) = &object.data else {
        panic!("expected a rotation parameter");
    };
    assert_eq!(
        native.value_set.flags, 8,
        "a list replaces the increment limits rather than adding to them"
    );
    assert_eq!(
        native.value_set.values,
        vec![30f64.to_radians(), 150f64.to_radians()]
    );
    let second = listed.handle;

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "V");
    feed(&mut app, "");
    feed(&mut app, "0,0");
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "5,5");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let answered_none = crate::scene::block_orientation_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| {
            parameter.kind == crate::scene::block_orientation_parameter::Kind::Rotation
                && parameter.handle != first
                && parameter.handle != second
        })
        .expect("the rotation answered with no set");
    let document::objects::ObjectType::DynamicBlock(object) = doc
        .objects
        .get(&answered_none.handle)
        .expect("rotation record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::RotationParameter(native) = &object.data else {
        panic!("expected a rotation parameter");
    };
    assert_eq!(
        native.value_set.flags, 1,
        "answering None stores the empty set rather than the periodic default"
    );
    assert_eq!(native.value_set.maximum, 0.0);
}

#[test]
fn a_flip_takes_its_name_and_property_label() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER FLIP");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Flip>:"
    );
    feed(&mut app, "Body");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter Flip property label <Flip state1>:"
    );
    feed(&mut app, "Mirror");
    feed(&mut app, "D");
    feed(&mut app, "mirrored");
    feed(&mut app, "P");
    feed(&mut app, "No");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Flip  Specify base point of reflection line or [Name/Label/Description/Palette]:"
    );
    feed(&mut app, "0,0");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Flip  Specify reflection line endpoint:"
    );
    feed(&mut app, "10,0");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Flip  Specify flip parameter label location:"
    );
    feed(&mut app, "-2,2");
    assert!(app.tabs[i].active_cmd.is_none());

    let first = {
        let doc = &app.tabs[i].scene.document;
        crate::scene::block_orientation_parameter::parameters(doc, definition)
            .into_iter()
            .find(|parameter| {
                parameter.kind == crate::scene::block_orientation_parameter::Kind::Flip
            })
            .expect("placed flip parameter")
            .handle
    };
    {
        let doc = &app.tabs[i].scene.document;
        let document::objects::ObjectType::DynamicBlock(object) =
            doc.objects.get(&first).expect("flip record")
        else {
            panic!("expected a dynamic block record");
        };
        let document::objects::DynamicBlockData::FlipParameter(native) = &object.data else {
            panic!("expected a flip parameter");
        };
        assert_eq!(
            native.flip_label, "Mirror",
            "the label collected at the prompt is stored on the parameter"
        );
        assert_eq!(
            native.parameter.parameter.element.name, "Body",
            "the name collected at the prompt is stored on the parameter"
        );
        assert_eq!(
            native.flip_label_description, "mirrored",
            "the description collected at the prompt is stored on the parameter"
        );
        assert!(
            !native.parameter.parameter.show_properties,
            "No at the palette question hides the property"
        );
    }

    // Enter at the label question keeps the shown default; the scene stores the
    // next free Flip state label.
    let _ = app.dispatch_command("BPARAMETER FLIP");
    feed(&mut app, "L");
    feed(&mut app, "");
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "-2,2");
    assert!(app.tabs[i].active_cmd.is_none());
    let doc = &app.tabs[i].scene.document;
    let second = crate::scene::block_orientation_parameter::parameters(doc, definition)
        .into_iter()
        .filter(|parameter| parameter.kind == crate::scene::block_orientation_parameter::Kind::Flip)
        .find(|parameter| parameter.handle != first)
        .expect("second flip parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&second.handle).expect("second flip record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::FlipParameter(native) = &object.data else {
        panic!("expected a flip parameter");
    };
    assert_eq!(native.flip_label, "Flip state1");
}

#[test]
fn a_rotation_takes_a_name_and_one_property_label() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Rotation>:"
    );
    feed(&mut app, "Body");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter rotation property label <Angle>:"
    );
    feed(&mut app, "Turn");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Rotation  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
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
    assert_eq!(
        native.angle_name, "Turn",
        "the label collected at the prompt is stored on the parameter"
    );
    assert_eq!(
        native.parameter.parameter.element.name, "Body",
        "the name collected at the prompt is stored on the parameter"
    );

    // The label question shows the plain word; the parameter takes the name
    // the prompt offered, so Enter still stores a label.
    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Rotation1>:",
        "the name counts the rotation already placed"
    );
    feed(&mut app, "");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter rotation property label <Angle>:"
    );
    feed(&mut app, "");
    feed(&mut app, "10,0");
    feed(&mut app, "2");
    feed(&mut app, "");
    feed(&mut app, "15,5");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let doc = &app.tabs[i].scene.document;
    let labels: Vec<String> =
        crate::scene::block_orientation_parameter::parameters(doc, definition)
            .into_iter()
            .filter(|parameter| {
                parameter.kind == crate::scene::block_orientation_parameter::Kind::Rotation
            })
            .map(|parameter| parameter.label)
            .collect();
    assert!(
        labels.iter().any(|label| label == "Angle1"),
        "Enter takes the next free numbered label: {labels:?}"
    );
}

#[test]
fn a_linear_takes_a_name_and_one_label_and_asks_for_no_angle() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Linear>:",
        "the name question defaults to the name the parameter is created with"
    );
    feed(&mut app, "Body");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter distance property label <Distance1>:"
    );
    feed(&mut app, "Width");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:",
        "a Linear parameter asks one label question, not a distance and an angle"
    );
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
    assert_eq!(
        native.distance_name, "Width",
        "the label collected at the prompt is stored on the parameter"
    );
    assert_eq!(
        native.parameter.parameter.element.name, "Body",
        "the name collected at the prompt is stored on the parameter"
    );

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Linear1>:",
        "the name counts the linear already placed"
    );
}

#[test]
fn a_polar_takes_a_distance_and_then_an_angle_value_set() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER POLAR");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "V");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter distance value set type [None/List/Increment] <None>:"
    );
    feed(&mut app, "I");
    feed(&mut app, "5");
    feed(&mut app, "0");
    feed(&mut app, "100");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter angle value set type [None/List/Increment] <None>:",
        "the angle set is asked once the distance set is answered"
    );
    feed(&mut app, "L");
    feed(&mut app, "0, 45, 90");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:",
        "both answers return to placement"
    );
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "5,5");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let polar = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::Polar)
        .expect("placed polar parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&polar.handle).expect("polar record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::PolarParameter(native) = &object.data else {
        panic!("expected a polar parameter");
    };
    assert_eq!(native.distance_value_set.flags, 1 | 2 | 4);
    assert_eq!(native.distance_value_set.increment, 5.0);
    assert_eq!(native.distance_value_set.minimum, 0.0);
    assert_eq!(native.distance_value_set.maximum, 100.0);
    assert_eq!(native.angle_value_set.flags, 8);
    assert_eq!(
        native.angle_value_set.values,
        vec![
            0.0,
            std::f64::consts::FRAC_PI_4,
            std::f64::consts::FRAC_PI_2
        ],
        "angles are stored in radians"
    );
}

#[test]
fn an_xy_takes_a_horizontal_and_then_a_vertical_value_set() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER XY");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "V");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter horizontal value set type [None/List/Increment] <None>:"
    );
    feed(&mut app, "L");
    feed(&mut app, "1, 2, 3");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter vertical value set type [None/List/Increment] <None>:",
        "the vertical set is asked once the horizontal set is answered"
    );
    feed(&mut app, "I");
    feed(&mut app, "2");
    feed(&mut app, "0");
    feed(&mut app, "10");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:",
        "both answers return to placement"
    );
    feed(&mut app, "0,0");
    feed(&mut app, "6,2");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let xy = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::XY)
        .expect("placed xy parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&xy.handle).expect("xy record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::XYParameter(native) = &object.data else {
        panic!("expected an xy parameter");
    };
    assert_eq!(native.x_value_set.flags, 8);
    assert_eq!(native.x_value_set.values, vec![1.0, 2.0, 3.0]);
    assert_eq!(native.y_value_set.flags, 1 | 2 | 4);
    assert_eq!(native.y_value_set.increment, 2.0);
    assert_eq!(native.y_value_set.minimum, 0.0);
    assert_eq!(native.y_value_set.maximum, 10.0);
}

#[test]
fn a_polar_angle_increment_is_asked_and_stored_in_radians() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER POLAR");
    feed(&mut app, "V");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter angle value set type [None/List/Increment] <None>:",
        "an explicit None for the distance set still asks the angle set"
    );
    feed(&mut app, "I");
    feed(&mut app, "45");
    feed(&mut app, "0");
    feed(&mut app, "360");
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "5,5");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let polar = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::Polar)
        .expect("placed polar parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&polar.handle).expect("polar record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::PolarParameter(native) = &object.data else {
        panic!("expected a polar parameter");
    };
    assert_eq!(
        native.distance_value_set.flags, 1,
        "an explicit None stores no set"
    );
    assert_eq!(native.angle_value_set.flags, 1 | 2 | 4);
    assert_eq!(native.angle_value_set.increment, 45f64.to_radians());
    assert_eq!(native.angle_value_set.minimum, 0.0);
    assert_eq!(native.angle_value_set.maximum, 360f64.to_radians());
}

#[test]
fn an_xy_placement_writes_its_name_and_two_labels() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER XY");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <XY>:"
    );
    feed(&mut app, "Frame");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter horizontal distance property label <X Distance1>:"
    );
    feed(&mut app, "Width X");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter vertical distance property label <Y Distance1>:"
    );
    feed(&mut app, "Width Y");
    feed(&mut app, "0,0");
    feed(&mut app, "6,2");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let xy = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::XY)
        .expect("placed xy parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&xy.handle).expect("xy record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::XYParameter(native) = &object.data else {
        panic!("expected an xy parameter");
    };
    assert_eq!(native.parameter.parameter.element.name, "Frame");
    assert_eq!(native.x_label, "Width X");
    assert_eq!(native.y_label, "Width Y");
}

#[test]
fn an_xy_placement_keeps_the_offered_default_name_and_labels() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER XY");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <XY>:"
    );
    feed(&mut app, "");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter horizontal distance property label <X Distance1>:"
    );
    feed(&mut app, "");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter vertical distance property label <Y Distance1>:"
    );
    feed(&mut app, "");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "0,0");
    feed(&mut app, "6,2");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let xy = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .find(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::XY)
        .expect("placed xy parameter");
    let document::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get(&xy.handle).expect("xy record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::XYParameter(native) = &object.data else {
        panic!("expected an xy parameter");
    };
    assert_eq!(native.parameter.parameter.element.name, "XY");
    assert_eq!(native.x_label, "X Distance1");
    assert_eq!(native.y_label, "Y Distance1");
}

#[test]
fn xy_parameter_names_count_the_ones_already_placed() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let prompt = |app: &OpenCADStudio| app.tabs[i].active_cmd.as_ref().unwrap().prompt();

    for (end, tag) in [("6,2", "<XY>:"), ("8,3", "<XY1>:"), ("10,4", "<XY2>:")] {
        let _ = app.dispatch_command("BPARAMETER XY");
        feed(&mut app, "N");
        assert_eq!(
            prompt(&app),
            format!("Enter parameter name {tag}"),
            "the name counts the xy parameters already placed"
        );
        feed(&mut app, "");
        feed(&mut app, "0,0");
        feed(&mut app, end);
        feed(&mut app, "");
        assert!(app.tabs[i].active_cmd.is_none());
    }

    let names =
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|parameter| parameter.native.parameter.element.name.clone())
            .collect::<Vec<_>>();
    assert_eq!(names, vec!["XY", "XY1", "XY2"]);
}

#[test]
fn a_linear_takes_its_base_location_and_value_set() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "B");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter base location [Startpoint/Midpoint] <Startpoint>:"
    );
    feed(&mut app, "M");
    feed(&mut app, "V");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter distance value set type [None/List/Increment] <None>:"
    );
    feed(&mut app, "I");
    feed(&mut app, "5");
    feed(&mut app, "0");
    feed(&mut app, "100");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:",
        "both questions return to placement"
    );
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
    assert_eq!(
        native.parameter.parameter_base_location, 1,
        "the midpoint answer is stored as the parameter's base location"
    );
    assert_eq!(native.value_set.flags, 1 | 2 | 4);
    assert_eq!(native.value_set.increment, 5.0);
    assert_eq!(native.value_set.minimum, 0.0);
    assert_eq!(native.value_set.maximum, 100.0);
    let first = linear.handle;

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "V");
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter list of distance values (separated by commas):"
    );
    feed(&mut app, "1, 2.5,3");
    feed(&mut app, "20,0");
    feed(&mut app, "30,0");
    feed(&mut app, "25,2");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let doc = &app.tabs[i].scene.document;
    let second = crate::scene::block_distance_parameter::parameters(doc, definition)
        .into_iter()
        .filter(|parameter| parameter.kind == crate::scene::block_distance_parameter::Kind::Linear)
        .find(|parameter| parameter.handle != first)
        .expect("second linear parameter");
    let document::objects::ObjectType::DynamicBlock(object) = doc
        .objects
        .get(&second.handle)
        .expect("second linear record")
    else {
        panic!("expected a dynamic block record");
    };
    let document::objects::DynamicBlockData::LinearParameter(native) = &object.data else {
        panic!("expected a linear parameter");
    };
    assert_eq!(native.value_set.flags, 8);
    assert_eq!(native.value_set.values, vec![1.0, 2.5, 3.0]);
}

#[test]
fn a_linear_asks_for_its_grip_count_after_the_label() {
    use crate::scene::block_grips::config;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let placed = |app: &OpenCADStudio| {
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|parameter| parameter.handle)
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

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "5,2");
    assert_eq!(
        prompt(&app),
        "BPARAMETER Linear  Enter number of grips [0/1/2] <2>:",
        "the label pick is followed by the grip count"
    );
    assert!(
        placed(&app).is_empty(),
        "the parameter waits for the grip count"
    );
    let before = app.tabs[i].scene.document.clone();
    for unlisted in ["3", "-1", "9"] {
        app.command_line.last_error = None;
        feed(&mut app, unlisted);
        assert_eq!(app.tabs[i].scene.document, before, "{unlisted}");
        assert_eq!(
            app.command_line.last_error, None,
            "{unlisted} is a count the list does not offer, so the question repeats without a message"
        );
        assert!(app.tabs[i].active_cmd.is_some(), "{unlisted}");
    }
    app.command_line.last_error = None;
    feed(&mut app, "four");
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Requires an integer value."),
        "words are no count at all"
    );
    assert!(app.tabs[i].active_cmd.is_some());
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());
    let first = placed(&app)[0];
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, first),
        Some(2),
        "Enter takes the two grips the prompt offers by default"
    );

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "20,0");
    feed(&mut app, "30,0");
    feed(&mut app, "25,2");
    feed(&mut app, "1");
    assert!(app.tabs[i].active_cmd.is_none());
    let second = placed(&app)[1];
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, second),
        Some(1),
        "one grip is the middle of the list"
    );

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "40,0");
    feed(&mut app, "50,0");
    feed(&mut app, "45,2");
    feed(&mut app, "0");
    assert!(app.tabs[i].active_cmd.is_none());
    let third = placed(&app)[2];
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, third),
        Some(0),
        "zero removes the linear's grip"
    );
}

#[test]
fn a_distance_keeps_the_defaults_shown_at_each_question() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LINEAR");
    feed(&mut app, "C");
    feed(&mut app, "maybe");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:",
        "a refused answer leaves the question open"
    );
    feed(&mut app, "");
    feed(&mut app, "P");
    feed(&mut app, "");
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
    assert!(
        !native.parameter.parameter.chain_actions,
        "Enter at the chain question keeps the actions unchained"
    );
    assert!(
        native.parameter.parameter.show_properties,
        "Enter at the palette question keeps the property visible"
    );
}

#[test]
fn a_polar_takes_its_distance_and_angle_labels_at_the_base_prompt() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER POLAR 0,0 5,0 2,2");

    let label_sets = |app: &crate::app::OpenCADStudio| {
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|parameter| parameter.labels)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        label_sets(&app),
        vec![vec!["Distance1".to_string(), "Angle1".to_string()]]
    );

    let _ = app.dispatch_command("BPARAMETER POLAR");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "L");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter distance property label <Distance2>:"
    );
    feed(&mut app, "Dist9");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter angle property label <Angle2>:"
    );
    feed(&mut app, "C");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
    );
    feed(&mut app, "0,0");
    feed(&mut app, "4,0");
    feed(&mut app, "1,1");
    assert!(app.tabs[i].active_cmd.is_none());

    let _ = app.dispatch_command("BPARAMETER POLAR");
    feed(&mut app, "0,0");
    feed(&mut app, "6,0");
    feed(&mut app, "2,2");
    assert!(app.tabs[i].active_cmd.is_none());

    assert_eq!(
        label_sets(&app),
        vec![
            vec!["Distance1".to_string(), "Angle1".to_string()],
            vec!["Dist9".to_string(), "C".to_string()],
            vec!["Distance2".to_string(), "Angle2".to_string()],
        ]
    );

    let names = |app: &crate::app::OpenCADStudio| {
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
            .into_iter()
            .map(|parameter| parameter.native.parameter.element.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&app), vec!["Polar", "Polar1", "Polar2"]);

    let _ = app.dispatch_command("BPARAMETER POLAR");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Polar3>:"
    );
    feed(&mut app, "P9");
    feed(&mut app, "0,0");
    feed(&mut app, "8,0");
    feed(&mut app, "3,3");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(names(&app), vec!["Polar", "Polar1", "Polar2", "P9"]);
    assert_eq!(
        label_sets(&app).last().unwrap(),
        &["Distance3".to_string(), "Angle3".to_string()]
    );

    let _ = app.dispatch_command("BPARAMETER POLAR");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Polar4>:"
    );
    feed(&mut app, "Polar1");
    feed(&mut app, "0,0");
    feed(&mut app, "10,0");
    feed(&mut app, "4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        names(&app),
        vec!["Polar", "Polar1", "Polar2", "P9", "Polar4"]
    );
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Parameter names must be unique.")
    );
}

#[test]
fn a_second_lookup_offers_the_next_free_name_and_a_taken_name_keeps_its_default() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "2,2");
    feed(&mut app, "");

    let _ = app.dispatch_command("BPARAMETER LOOKUP");
    feed(&mut app, "N");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().prompt(),
        "Enter parameter name <Lookup2>:",
        "the second parameter skips the name already placed"
    );
    feed(&mut app, "Lookup1");
    feed(&mut app, "4,4");
    feed(&mut app, "");
    assert!(app.tabs[i].active_cmd.is_none());

    let names = crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .map(|(_, parameter)| parameter.lookup_name.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["Lookup1", "Lookup2"],
        "a name already in use is refused and the default stands"
    );
}
