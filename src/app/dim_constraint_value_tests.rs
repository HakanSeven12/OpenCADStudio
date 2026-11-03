//! The value prompt that closes an Angular, Radius or Diameter dimensional
//! constraint: after the sides and the dimension line location, the host
//! offers the corner or span it measured and takes the typed number — which
//! is what actually moves the geometry.
use super::OpenCADStudio;
use crate::command::StepInput;
use crate::scene::parametric_constraints::{
    ConstraintKind, ParametricScope, DYNAMIC_DIMENSION_LAYER,
};
use codec::{EntityType, Handle};
use glam::DVec3;

/// Whole degrees for the corner's own label: the current dimension style's
/// angular precision is what a dynamic angle prints through, so the fixtures
/// pin it instead of trusting the default.
fn pin_whole_degrees(app: &mut OpenCADStudio, tab: usize) {
    let current = app.tabs[tab]
        .scene
        .document
        .header
        .current_dimstyle_name
        .clone();
    let style = app.tabs[tab]
        .scene
        .document
        .dim_styles
        .iter_mut()
        .find(|style| style.name.eq_ignore_ascii_case(current.trim()))
        .expect("the drawing's current dimension style");
    style.dimadec = 0;
    assert_eq!(
        crate::scene::parametric_constraints::angle_decimals(&app.tabs[tab].scene.document, None),
        0,
        "the pin took the style the label prints through"
    );
}

/// Two lines out of the origin, the second at 45° to the first.
fn two_lines() -> (OpenCADStudio, usize, Handle, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 50,0"}"#);
    let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 30,30"}"#);
    let i = app.active_tab;
    // The label format and the style's angular precision are pinned, not left
    // to the drawing's defaults, so every label prints the number it says.
    // The name the prompt offers is free in another sense: a new drawing has
    // no parameters, so the first `angN` is `ang1`.
    app.tabs[i].scene.constraint_name_format = 2;
    pin_whole_degrees(&mut app, i);
    let drawn = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Line(_)))
        .map(|entity| entity.common().handle)
        .collect::<Vec<_>>();
    (app, i, drawn[0], drawn[1])
}

/// Circles at the given centres, each with the given radius.
fn circles(at: &[&str], radius: &str) -> (OpenCADStudio, usize, Vec<Handle>) {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    for centre in at {
        let _ = app.automation_op(&format!(
            r#"{{"op":"run","cmd":"CIRCLE {centre} {radius}"}}"#
        ));
    }
    let i = app.active_tab;
    // Pinned rather than left at the default, because the label below is
    // printed through it; a new drawing has no parameters, so the first
    // `radN` is `rad1`.
    app.tabs[i].scene.constraint_name_format = 2;
    let drawn = app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Circle(_)))
        .map(|entity| entity.common().handle)
        .collect::<Vec<_>>();
    (app, i, drawn)
}

/// One circle at `centre` with `radius`.
fn circle_at(centre: &str, radius: &str) -> (OpenCADStudio, usize, Handle) {
    let (app, i, drawn) = circles(&[centre], radius);
    (app, i, drawn[0])
}

fn prompt(app: &OpenCADStudio, tab: usize) -> String {
    app.tabs[tab]
        .active_cmd
        .as_ref()
        .map(|command| command.prompt())
        .expect("the command is still asking")
}

fn line_ends(app: &OpenCADStudio, tab: usize, handle: Handle) -> (DVec3, DVec3) {
    match app.tabs[tab].scene.document.get_entity(handle) {
        Some(EntityType::Line(line)) => (
            DVec3::new(line.start.x, line.start.y, line.start.z),
            DVec3::new(line.end.x, line.end.y, line.end.z),
        ),
        other => panic!("the constrained entity is a line, found {other:?}"),
    }
}

fn circle_state(app: &OpenCADStudio, tab: usize, handle: Handle) -> (DVec3, f64) {
    match app.tabs[tab].scene.document.get_entity(handle) {
        Some(EntityType::Circle(circle)) => (
            DVec3::new(circle.center.x, circle.center.y, circle.center.z),
            circle.radius,
        ),
        other => panic!("the constrained entity is a circle, found {other:?}"),
    }
}

/// A solved size carries the solver's tolerance; the typed value is what the
/// label and the parameter hold.
fn solved(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1.0e-6
}

/// The same figure for a solved position.
fn sits_at(actual: DVec3, expected: DVec3) -> bool {
    (actual - expected).length() < 1.0e-6
}

/// A corner carries the looser figure: the solver answers in distances, and a
/// distance residual over a short arm is a bigger angle.
fn corner_is(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1.0e-4
}

/// The distance between a line's ends.
fn length(app: &OpenCADStudio, tab: usize, handle: Handle) -> f64 {
    let (start, end) = line_ends(app, tab, handle);
    (end - start).length()
}

/// What a dynamic dimension shows: its text, its layer and its colour.
fn label(
    app: &OpenCADStudio,
    tab: usize,
    handle: Handle,
) -> (Option<String>, String, codec::types::Color) {
    let entity = app.tabs[tab]
        .scene
        .document
        .get_entity(handle)
        .expect("the dimension is in the drawing");
    let dimension = crate::entities::dimension::as_dimension(entity).expect("a dimension");
    (
        dimension.base().user_text.clone(),
        dimension.base().common.layer.clone(),
        dimension.base().common.color,
    )
}

fn constraints(app: &OpenCADStudio, tab: usize) -> Vec<(ConstraintKind, String)> {
    app.tabs[tab]
        .scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .map(|set| {
            set.constraints
                .iter()
                .map(|constraint| {
                    let driven = match &constraint.driving_param {
                        Some(crate::scene::named_parameters::DrivingValue::Named(name)) => {
                            name.clone()
                        }
                        Some(crate::scene::named_parameters::DrivingValue::Literal(value)) => {
                            format!("{value}")
                        }
                        None => "none".to_string(),
                    };
                    (constraint.kind, driven)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The angle, in degrees, from the first side's heading to the second's.
fn signed_angle(first: (DVec3, DVec3), second: (DVec3, DVec3)) -> f64 {
    let heading = |(a, b): (DVec3, DVec3)| (b - a).y.atan2((b - a).x);
    (heading(second) - heading(first))
        .to_degrees()
        .rem_euclid(360.0)
}

/// Both sides and the dimension line location, up to the value prompt.
fn walk_to_the_angular_prompt(app: &mut OpenCADStudio, first: Handle, second: Handle) {
    let _ = app.dispatch_command("DCANGULAR");
    let _ = app.feed_command(StepInput::EntityPick(first, DVec3::new(25.0, 0.0, 0.0)));
    let _ = app.feed_command(StepInput::EntityPick(second, DVec3::new(15.0, 15.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(40.0, 20.0, 0.0)));
}

#[test]
fn the_angular_prompt_offers_the_corner_the_two_sides_make() {
    let (mut app, i, first, second) = two_lines();
    let _ = app.dispatch_command("DCANGULAR");
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Select first line or arc or [3Point] <3Point>:",
        "the angle starts by asking for its sides"
    );
    let _ = app.feed_command(StepInput::EntityPick(first, DVec3::new(25.0, 0.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Select second line:",
        "the first side is taken, the second is asked for"
    );
    let _ = app.feed_command(StepInput::EntityPick(second, DVec3::new(15.0, 15.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Specify dimension line location:",
        "both sides are taken, the location is asked for"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(40.0, 20.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Enter value or name and value <ang1=45>:",
        "the prompt offers the corner the sides make"
    );
    assert!(
        app.command_line
            .history_plain_text()
            .contains("Dimension text = 45"),
        "the measured corner is reported at the drawing's angular precision"
    );
    assert!(
        constraints(&app, i).is_empty(),
        "nothing is applied until the value answers"
    );
}

#[test]
fn typing_a_value_at_the_angular_prompt_turns_the_second_side() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("90".into()));
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the value closes the command"
    );
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Angle, "ang1".to_string())],
        "one angle constraint, driven by the parameter the prompt named"
    );
    assert_eq!(
        app.tabs[i]
            .scene
            .named_parameters()
            .get("ang1")
            .map(|parameter| parameter.source.clone()),
        Some("90".to_string()),
        "the parameter holds the typed value"
    );
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the constraint");
    let dimension = set
        .dimensions
        .values()
        .next()
        .copied()
        .expect("the constraint carries its own dimension");
    assert_eq!(
        label(&app, i, dimension),
        (
            Some("ang1=90".to_string()),
            DYNAMIC_DIMENSION_LAYER.to_string(),
            codec::types::Color::Rgb {
                r: 103,
                g: 109,
                b: 118,
            },
        ),
        "the label shows the parameter, on the dynamic layer and in its grey"
    );
    let kept = line_ends(&app, i, first);
    assert!(
        sits_at(kept.0, DVec3::ZERO) && sits_at(kept.1, DVec3::new(50.0, 0.0, 0.0)),
        "the first side stays where it was drawn, found {kept:?}"
    );
    let turned = signed_angle(kept, line_ends(&app, i, second));
    assert!(
        corner_is(turned, 90.0),
        "the second side turns to the typed corner, found {turned}"
    );
}

#[test]
fn an_angular_value_turns_the_second_side_without_resizing_it() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let drawn = length(&app, i, second);
    assert!(
        solved(drawn, 30.0 * std::f64::consts::SQRT_2),
        "the second side is drawn to 30,30, found {drawn}"
    );
    let _ = app.feed_command(StepInput::Text("90".into()));
    let turned = signed_angle(line_ends(&app, i, first), line_ends(&app, i, second));
    assert!(
        corner_is(turned, 90.0),
        "the typed corner is reached, found {turned}"
    );
    assert!(
        solved(length(&app, i, first), 50.0),
        "the side that stays keeps its length, found {}",
        length(&app, i, first)
    );
    assert!(
        solved(length(&app, i, second), drawn),
        "the turned side keeps the length it was drawn with, found {}",
        length(&app, i, second)
    );
}

#[test]
fn editing_the_corner_turns_the_side_without_resizing_it() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("90".into()));
    let drawn = length(&app, i, second);
    let _ = app.dispatch_command("DCVALUE ang1 45");
    let turned = signed_angle(line_ends(&app, i, first), line_ends(&app, i, second));
    assert!(
        corner_is(turned, 45.0),
        "the edited corner is reached, found {turned}"
    );
    let kept = line_ends(&app, i, first);
    assert!(
        sits_at(kept.0, DVec3::ZERO) && sits_at(kept.1, DVec3::new(50.0, 0.0, 0.0)),
        "the side that stays keeps its place, found {kept:?}"
    );
    assert!(
        solved(length(&app, i, second), drawn),
        "the turned side keeps the length it was drawn with, found {}",
        length(&app, i, second)
    );
}

#[test]
fn a_solve_mode_of_zero_creates_the_corner_by_resizing_it() {
    let (mut app, i, first, second) = two_lines();
    app.constraint_solve_mode = false;
    walk_to_the_angular_prompt(&mut app, first, second);
    let drawn = length(&app, i, second);
    let _ = app.feed_command(StepInput::Text("90".into()));
    let turned = signed_angle(line_ends(&app, i, first), line_ends(&app, i, second));
    assert!(
        corner_is(turned, 90.0),
        "the corner is reached either way, found {turned}"
    );
    let resized = length(&app, i, second);
    assert!(
        !solved(resized, drawn),
        "at zero the corner is reached by resizing the side it turns, found {resized}"
    );
}

#[test]
fn a_solve_mode_of_zero_edits_the_corner_by_resizing_it() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("90".into()));
    let drawn = length(&app, i, second);
    assert!(
        solved(drawn, 30.0 * std::f64::consts::SQRT_2),
        "applying the corner kept the side it turns, found {drawn}"
    );
    app.constraint_solve_mode = false;
    let _ = app.dispatch_command("DCVALUE ang1 45");
    let turned = signed_angle(line_ends(&app, i, first), line_ends(&app, i, second));
    assert!(
        corner_is(turned, 45.0),
        "the edited corner is reached either way, found {turned}"
    );
    let resized = length(&app, i, second);
    assert!(
        !solved(resized, drawn),
        "at zero the edit is reached by resizing the side it turns, found {resized}"
    );
}

#[test]
fn editing_a_radius_moves_the_rim() {
    let (mut app, i, shape) = circle_at("10,10", "25");
    let _ = app.dispatch_command("DCRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(shape, DVec3::new(35.0, 10.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(50.0, 20.0, 0.0)));
    let _ = app.feed_command(StepInput::Text("40".into()));
    assert!(
        solved(circle_state(&app, i, shape).1, 40.0),
        "the applied radius is the typed one, found {}",
        circle_state(&app, i, shape).1
    );
    let _ = app.dispatch_command("DCVALUE rad1 55");
    let (centre, radius) = circle_state(&app, i, shape);
    assert!(
        solved(radius, 55.0),
        "editing the radius moves the rim, found {radius}"
    );
    assert!(
        sits_at(centre, DVec3::new(10.0, 10.0, 0.0)),
        "the centre stays where the circle was drawn, found {centre:?}"
    );
}

#[test]
fn naming_the_value_at_the_angular_prompt_names_the_constraint() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("corner=30".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Angle, "corner".to_string())],
        "the typed name drives the constraint"
    );
    assert!(
        !app.tabs[i].scene.named_parameters().contains("ang1"),
        "the reserved default name is not created beside it"
    );
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the constraint");
    let dimension = set.dimensions.values().next().copied().expect("a label");
    assert_eq!(
        label(&app, i, dimension).0,
        Some("corner=30".to_string()),
        "the label carries the name that was typed"
    );
    let turned = signed_angle(line_ends(&app, i, first), line_ends(&app, i, second));
    assert!(
        corner_is(turned, 30.0),
        "the second side holds the corner the name carries, found {turned}"
    );
}

#[test]
fn the_same_corner_between_the_same_sides_is_refused() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("90".into()));
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("60".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Angle, "ang1".to_string())],
        "the second corner on the same sides adds nothing"
    );
    assert!(
        app.command_line
            .history_plain_text()
            .contains("The constraint already exists on the selected objects."),
        "the refusal says why"
    );
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the refusal ends the command"
    );
    let turned = signed_angle(line_ends(&app, i, first), line_ends(&app, i, second));
    assert!(
        corner_is(turned, 90.0),
        "the refused value moves nothing, found {turned}"
    );
}

#[test]
fn a_name_the_parameter_table_refuses_is_reported_at_the_angular_prompt() {
    let (mut app, i, first, second) = two_lines();
    walk_to_the_angular_prompt(&mut app, first, second);
    let _ = app.feed_command(StepInput::Text("2corner=30".into()));
    assert!(
        app.command_line
            .history_plain_text()
            .contains("Only alphanumeric names, starting with alpha characters, are allowed."),
        "the name is refused by name"
    );
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "the prompt comes back for another answer"
    );
    assert!(
        constraints(&app, i).is_empty(),
        "a refused name applies nothing"
    );
    let _ = app.feed_command(StepInput::Text("90".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Angle, "ang1".to_string())],
        "the same prompt takes the next answer"
    );
}

#[test]
fn typing_a_value_at_the_radius_prompt_moves_the_rim() {
    let (mut app, i, shape) = circle_at("10,10", "25");
    let _ = app.dispatch_command("DCRADIUS");
    assert_eq!(
        prompt(&app, i),
        "DCRADIUS  Select arc or circle:",
        "the radius starts by asking for the curve"
    );
    let _ = app.feed_command(StepInput::EntityPick(shape, DVec3::new(35.0, 10.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCRADIUS  Specify dimension line location:",
        "the curve is taken, the location is asked for"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(50.0, 20.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCRADIUS  Enter value or name and value <rad1=25>:",
        "the prompt offers the radius the curve has"
    );
    let _ = app.feed_command(StepInput::Text("40".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Radius, "rad1".to_string())],
        "one radius constraint, driven by the parameter the prompt named"
    );
    assert!(
        sits_at(circle_state(&app, i, shape).0, DVec3::new(10.0, 10.0, 0.0)),
        "the centre stays where the circle was drawn"
    );
    assert!(
        solved(circle_state(&app, i, shape).1, 40.0),
        "the rim takes the typed radius, found {}",
        circle_state(&app, i, shape).1
    );
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the constraint");
    let dimension = set.dimensions.values().next().copied().expect("a label");
    assert_eq!(
        label(&app, i, dimension),
        (
            Some("rad1=40".to_string()),
            DYNAMIC_DIMENSION_LAYER.to_string(),
            codec::types::Color::Rgb {
                r: 103,
                g: 109,
                b: 118,
            },
        ),
        "the label shows the parameter with the span it drives, in the grey"
    );
}

#[test]
fn the_diameter_prompt_offers_the_span_and_its_value_moves_the_rim() {
    let (mut app, i, shape) = circle_at("10,10", "25");
    let _ = app.dispatch_command("DCDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(shape, DVec3::new(35.0, 10.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(50.0, 20.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCDIAMETER  Enter value or name and value <dia1=50>:",
        "the prompt offers the span across the circle, not its radius"
    );
    let _ = app.feed_command(StepInput::Text("60".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Diameter, "dia1".to_string())],
        "the diameter constraint is driven by its own parameter"
    );
    assert!(
        solved(circle_state(&app, i, shape).1, 30.0),
        "the typed diameter halves into the radius, found {}",
        circle_state(&app, i, shape).1
    );
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("the constraint");
    let dimension = set.dimensions.values().next().copied().expect("a label");
    assert_eq!(
        label(&app, i, dimension).0,
        Some("dia1=60".to_string()),
        "the label prints the diameter the parameter carries"
    );
}

#[test]
fn a_circle_with_one_radial_constraint_refuses_the_other() {
    let (mut app, i, shape) = circle_at("10,10", "25");
    let _ = app.dispatch_command("DCDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(shape, DVec3::new(35.0, 10.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(50.0, 20.0, 0.0)));
    let _ = app.feed_command(StepInput::Text("60".into()));
    let _ = app.dispatch_command("DCRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(shape, DVec3::new(40.0, 10.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(50.0, 20.0, 0.0)));
    let _ = app.feed_command(StepInput::Text("12".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Diameter, "dia1".to_string())],
        "the circle keeps the one size constraint it has"
    );
    assert!(
        app.command_line
            .history_plain_text()
            .contains("The constraint already exists on the selected objects."),
        "the refusal says why"
    );
    assert!(
        solved(circle_state(&app, i, shape).1, 30.0),
        "the refused radius moves nothing, found {}",
        circle_state(&app, i, shape).1
    );
}

#[test]
fn a_size_that_is_not_positive_is_refused_at_the_radius_prompt() {
    let (mut app, i, shape) = circle_at("0,0", "25");
    let _ = app.dispatch_command("DCRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(shape, DVec3::new(25.0, 0.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(40.0, 0.0, 0.0)));
    let _ = app.feed_command(StepInput::Text("0".into()));
    assert!(
        app.command_line.history_plain_text().contains(
            "The parameter is used in an expression which results in an invalid value for a dimensional constraint."
        ),
        "a circle has no zero size, and the answer says so"
    );
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "the prompt comes back for another answer"
    );
    assert!(
        constraints(&app, i).is_empty(),
        "a refused size applies nothing"
    );
    assert_eq!(
        circle_state(&app, i, shape).1,
        25.0,
        "a refused size leaves the circle alone"
    );
    let _ = app.feed_command(StepInput::Text("30".into()));
    assert!(
        solved(circle_state(&app, i, shape).1, 30.0),
        "the same prompt takes the next answer, found {}",
        circle_state(&app, i, shape).1
    );
}

#[test]
fn a_name_that_is_taken_is_refused_at_the_radius_prompt() {
    let (mut app, i, drawn) = circles(&["0,0", "100,0"], "25");
    let (first, second) = (drawn[0], drawn[1]);
    let _ = app.dispatch_command("DCRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(first, DVec3::new(25.0, 0.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(40.0, 0.0, 0.0)));
    let _ = app.feed_command(StepInput::Text("size=20".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Radius, "size".to_string())],
        "the first constraint takes the typed name"
    );
    let _ = app.dispatch_command("DCRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(second, DVec3::new(125.0, 0.0, 0.0)));
    let _ = app.feed_command(StepInput::Point(DVec3::new(140.0, 0.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCRADIUS  Enter value or name and value <rad1=25>:",
        "the other circle is offered the next free name"
    );
    let _ = app.feed_command(StepInput::Text("size=30".into()));
    assert!(
        app.command_line
            .history_plain_text()
            .contains("Parameter with this name already exists."),
        "the taken name is refused by name"
    );
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "the prompt comes back for another answer"
    );
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Radius, "size".to_string())],
        "a refused name applies nothing to the second circle"
    );
    assert_eq!(
        circle_state(&app, i, second).1,
        25.0,
        "the second circle keeps the radius it was drawn with"
    );
}

#[test]
fn a_three_point_corner_reaches_the_value_it_is_given() {
    let (mut app, i, first, second) = two_lines();
    let _ = app.dispatch_command("DCANGULAR");
    let _ = app.feed_command(StepInput::Enter);
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Specify angle vertex:",
        "the default answer takes the three points"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(0.0, 0.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Specify first angle constraint point:",
        "the vertex is taken, the first point is asked for"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(50.0, 0.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Specify second angle constraint point:",
        "the first point is taken, the second is asked for"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(30.0, 30.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Specify dimension line location:",
        "the corner the three points make is taken, the location is asked for"
    );
    let _ = app.feed_command(StepInput::Point(DVec3::new(40.0, 20.0, 0.0)));
    assert_eq!(
        prompt(&app, i),
        "DCANGULAR  Enter value or name and value <ang1=45>:",
        "the prompt offers the corner the points make"
    );
    let _ = app.feed_command(StepInput::Text("90".into()));
    assert_eq!(
        constraints(&app, i),
        vec![(ConstraintKind::Angle3Point, "ang1".to_string())],
        "three points make a corner of their own kind"
    );
    let kept = line_ends(&app, i, first);
    assert!(
        sits_at(kept.0, DVec3::ZERO) && sits_at(kept.1, DVec3::new(50.0, 0.0, 0.0)),
        "the side through the vertex stays, found {kept:?}"
    );
    let turned = signed_angle(kept, line_ends(&app, i, second));
    assert!(
        corner_is(turned, 90.0),
        "the second point turns to the typed corner, found {turned}"
    );
    assert!(
        sits_at(line_ends(&app, i, second).0, DVec3::ZERO),
        "the vertex stays where it was picked, found {:?}",
        line_ends(&app, i, second).0
    );
}
