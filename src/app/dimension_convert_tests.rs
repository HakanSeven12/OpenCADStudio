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
