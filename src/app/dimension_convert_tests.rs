//! `DCCONVERT` turns a dimension drawn from live geometry into a dimensional
//! constraint: the dimension itself stays, becomes the constraint's label and
//! is driven by a new parameter holding the span it measured.
use super::OpenCADStudio;
use crate::command::StepInput;
use crate::scene::parametric_constraints::{
    ConstraintKind, ParametricScope, DYNAMIC_DIMENSION_LAYER,
};
use codec::{EntityType, Handle};

/// A line and the associative dimension across it.
fn fixture() -> (OpenCADStudio, usize, Handle, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 40,0"}"#);
    let i = app.active_tab;
    // Pinned rather than left at the default, because every label below is
    // printed through it; the drawing is new, so `d1` is the first free name.
    app.tabs[i].scene.constraint_name_format = 2;
    let line = app.tabs[i]
        .scene
        .document
        .entities()
        .find(|entity| matches!(entity, EntityType::Line(_)))
        .map(|entity| entity.common().handle)
        .expect("the drawn line");
    let dimension = app.tabs[i].scene.add_entity(horizontal(40.0));
    app.tabs[i]
        .scene
        .attach_dimension_association(dimension, vec![Some(line), Some(line)]);
    (app, i, line, dimension)
}

/// A horizontal dimension from the origin to `length` along X.
fn horizontal(length: f64) -> EntityType {
    crate::modules::annotate::linear_dim::linear_dimension_entity(
        glam::DVec3::ZERO,
        glam::DVec3::new(length, 0.0, 0.0),
        glam::DVec3::new(length * 0.5, 10.0, 0.0),
        glam::DVec3::X,
        None,
    )
}

fn printed(app: &OpenCADStudio, tab: usize, handle: Handle) -> String {
    let entity = app.tabs[tab]
        .scene
        .document
        .get_entity(handle)
        .expect("there");
    crate::entities::dimension::as_dimension(entity)
        .expect("the entity is a dimension")
        .base()
        .user_text
        .clone()
        .unwrap_or_default()
}

fn layer(app: &OpenCADStudio, tab: usize, handle: Handle) -> String {
    app.tabs[tab]
        .scene
        .document
        .get_entity(handle)
        .expect("there")
        .common()
        .layer
        .clone()
}

/// The converted dimension carries the constraint's own record: the parameter
/// named for the constraint it drives, the dynamic layer and grey, and the
/// label printing that parameter.
#[test]
fn converting_an_associative_dimension_makes_it_the_constraints_dimension() {
    let (mut app, i, line, dimension) = fixture();
    let drawn_layer = layer(&app, i, dimension);
    let _ = app.dispatch_command("DCCONVERT");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "the command gathers its dimensions first"
    );
    let _ = app.feed_command(StepInput::SelectionComplete(vec![dimension]));
    let _ = app.feed_command(StepInput::Enter);
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the gather hands over to the conversion"
    );

    let scene = &app.tabs[i].scene;
    let set = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the conversion makes a constraint");
    let constraint = set
        .constraints
        .iter()
        .find(|constraint| constraint.kind == ConstraintKind::DistanceX)
        .expect("a horizontal dimension becomes a horizontal constraint");
    assert_eq!(
        set.dimensions.get(&constraint.id),
        Some(&dimension),
        "the dimension is the constraint's own"
    );
    assert_eq!(
        constraint.driving_param,
        Some(crate::scene::named_parameters::DrivingValue::Named(
            "d1".to_string()
        )),
        "the constraint is driven by a new parameter"
    );
    assert_eq!(
        scene
            .named_parameters()
            .get("d1")
            .map(|p| p.source.as_str()),
        Some("40"),
        "the parameter holds the span the dimension measured"
    );
    assert_eq!(constraint.refs.len(), 2, "both ends of the dimension");
    for reference in &constraint.refs {
        assert_eq!(reference.entity, line, "the line is the source");
    }

    assert_eq!(
        layer(&app, i, dimension),
        DYNAMIC_DIMENSION_LAYER.to_string(),
        "the dynamic form lives on the constraints layer"
    );
    assert_ne!(
        drawn_layer,
        DYNAMIC_DIMENSION_LAYER.to_string(),
        "and it was not there before"
    );
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .get_entity(dimension)
            .expect("there")
            .common()
            .color,
        // The same grey the dynamic form is written with everywhere else:
        // the conversion and `set_dimensional_constraint_form`.
        codec::types::Color::Rgb {
            r: 103,
            g: 109,
            b: 118
        },
        "in the constraint grey"
    );
    assert_eq!(
        printed(&app, i, dimension),
        "d1=40",
        "the label prints the parameter, not a bare number"
    );
}

/// A dimension that measures nothing live is not a candidate: the conversion
/// refuses it and no constraint is made from it.
#[test]
fn only_dimensions_drawn_from_geometry_are_converted() {
    let (mut app, i, _line, associative) = fixture();
    let plain = app.tabs[i].scene.add_entity(horizontal(15.0));
    assert!(
        !crate::scene::dimension_assoc::dimension_is_associative(
            &app.tabs[i].scene.document,
            plain
        ),
        "the second dimension is drawn, not associated"
    );
    let plain_layer = layer(&app, i, plain);

    let _ = app.dispatch_command("DCCONVERT");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![associative, plain]));
    let _ = app.feed_command(StepInput::Enter);

    let scene = &app.tabs[i].scene;
    let converted = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .map(|set| set.constraints.len())
        .unwrap_or_default();
    assert_eq!(converted, 1, "only the associative dimension converts");
    assert_eq!(
        layer(&app, i, plain),
        plain_layer,
        "the refused dimension keeps the layer it was drawn on"
    );
    assert_eq!(
        printed(&app, i, plain),
        "",
        "and prints no constraint parameter"
    );
}

/// The annotational form of the same conversion: the dimension stays where it
/// was drawn, and because it is not on the dynamic layer the label reads as
/// annotational — the parameter with four decimals.
#[test]
fn an_annotational_conversion_leaves_the_dimension_where_it_is() {
    let (mut app, i, _line, dimension) = fixture();
    app.constraint_form_annotational = true;
    let drawn_layer = layer(&app, i, dimension);
    let drawn_color = app.tabs[i]
        .scene
        .document
        .get_entity(dimension)
        .expect("there")
        .common()
        .color;

    let _ = app.dispatch_command("DCCONVERT");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![dimension]));
    let _ = app.feed_command(StepInput::Enter);

    let scene = &app.tabs[i].scene;
    let set = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the conversion makes a constraint");
    assert_eq!(
        set.dimensions.values().copied().collect::<Vec<_>>(),
        vec![dimension],
        "the dimension is the constraint's own"
    );
    assert_eq!(
        layer(&app, i, dimension),
        drawn_layer,
        "an annotational dimension stays on the layer it was drawn on"
    );
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .get_entity(dimension)
            .expect("there")
            .common()
            .color,
        drawn_color,
        "and keeps its own colour instead of the constraint grey"
    );
    assert_eq!(
        printed(&app, i, dimension),
        "d1=40.0000",
        "the annotational label keeps four decimals"
    );

    // A later refresh must not rewrite the label.
    app.tabs[i].scene.refresh_dynamic_dimension_texts();
    assert_eq!(printed(&app, i, dimension), "d1=40.0000");
}

/// The two legs of a corner, the angular dimension across it and the two
/// styles whose angular precisions disagree.
fn angular_fixture() -> (OpenCADStudio, usize, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 50,0"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 30,30"}"#);
    let i = app.active_tab;
    // Pinned rather than left at the default, because the label is printed
    // through it; the drawing is new, so `ang1` is the first free name.
    app.tabs[i].scene.constraint_name_format = 2;
    let mut legs: Vec<Handle> = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Line(_)))
        .map(|entity| entity.common().handle)
        .collect();
    assert_eq!(legs.len(), 2, "the two legs of the corner");
    legs.sort_by_key(|handle| handle.value());
    let (first_leg, second_leg) = (legs[0], legs[1]);

    // The dimension is drawn in a style asking for three decimals while the
    // drawing's current style asks for none.
    {
        let document = &mut app.tabs[i].scene.document;
        let standard = document
            .dim_styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case("Standard"))
            .cloned()
            .expect("the standard dimension style");
        let mut tight = standard.clone();
        tight.name = "ANG3".to_string();
        tight.dimadec = 3;
        let mut loose = standard;
        loose.name = "ANG0".to_string();
        loose.dimadec = 0;
        document.dim_styles.add(tight).unwrap();
        document.dim_styles.add(loose).unwrap();
        document.header.current_dimstyle_name = "ANG0".to_string();
    }
    let document = &app.tabs[i].scene.document;
    assert_eq!(
        crate::scene::parametric_constraints::angle_decimals(document, Some("ANG3")),
        3,
        "the style the dimension is drawn in"
    );
    assert_eq!(
        crate::scene::parametric_constraints::angle_decimals(document, None),
        0,
        "the style the drawing is currently in"
    );

    let mut dimension = crate::modules::annotate::angular_dim::angular_two_line_entity(
        glam::DVec3::ZERO,
        glam::DVec3::new(50.0, 0.0, 0.0),
        glam::DVec3::ZERO,
        glam::DVec3::new(30.0, 30.0, 0.0),
        glam::DVec3::new(40.0, 25.0, 0.0),
        None,
    )
    .expect("the two legs form a corner the dimension can measure");
    crate::entities::dimension::as_dimension_mut(&mut dimension)
        .expect("the entity is a dimension")
        .base_mut()
        .style_name = "ANG3".to_string();
    let dimension = app.tabs[i].scene.add_entity(dimension);
    app.tabs[i]
        .scene
        .attach_dimension_association(dimension, vec![Some(first_leg), Some(second_leg)]);
    (app, i, dimension)
}

/// A converted angular dimension reads at its own style's angular precision.
/// The conversion is not what decides that — the label is restated from the
/// dimension's own style every time the geometry is refreshed, and a later
/// refresh must not rewrite what the command left behind.
#[test]
fn converting_an_angular_dimension_keeps_its_own_styles_precision() {
    let (mut app, i, dimension) = angular_fixture();
    assert!(
        crate::scene::dimension_assoc::dimension_is_associative(
            &app.tabs[i].scene.document,
            dimension
        ),
        "the dimension is drawn from both legs"
    );

    let _ = app.dispatch_command("DCCONVERT");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![dimension]));
    let _ = app.feed_command(StepInput::Enter);

    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the angular dimension converts to an angle constraint");
    let constraint = set
        .constraints
        .iter()
        .find(|constraint| constraint.kind == ConstraintKind::Angle)
        .expect("an angular dimension between two lines becomes a two-line angle constraint");
    assert_eq!(
        set.dimensions.get(&constraint.id),
        Some(&dimension),
        "the dimension is the constraint's own"
    );
    let converted = printed(&app, i, dimension);
    assert_eq!(
        converted, "ang1=45.000",
        "the label reads at the dimension's own style: three decimals of the corner between the two legs, in degrees"
    );

    app.tabs[i].scene.refresh_dynamic_dimension_texts();
    assert_eq!(
        printed(&app, i, dimension),
        converted,
        "the restated label is stable across refreshes"
    );
}

/// The conversion adds a constraint, and that solve covers the whole space: it
/// can level a line the conversion never named. Run on a pre-selection it
/// prints that report itself, naming every such line once.
#[test]
fn converting_a_dimension_names_every_line_the_network_moves() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let (mut app, i, line, dimension) = fixture();
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,50 10,60"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,100 10,110"}"#);
    let mut slanted: Vec<Handle> = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Line(_)))
        .map(|entity| entity.common().handle)
        .filter(|handle| *handle != line)
        .collect();
    assert_eq!(slanted.len(), 2, "the two slanted lines");
    slanted.sort_unstable();
    for handle in &slanted {
        app.tabs[i]
            .scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace)
            .add(
                ConstraintKind::Horizontal,
                vec![ParametricRef::whole(*handle)],
                None,
            );
    }

    app.tabs[i].scene.select_entity(dimension, false);
    let before = app.command_line.history.len();
    let _ = app.dispatch_command("DCCONVERTAPPLY");
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    let handles: Vec<String> = slanted.iter().map(|handle| handle.to_string()).collect();
    let report = format!(
        "The constraint network moved 2 entities the command did not name: {}.",
        handles.join(", ")
    );
    assert_eq!(
        said.iter().filter(|text| **text == report).count(),
        1,
        "the completion names both lines the network moved, once: {said:?}"
    );
}
