//! Styled placement previews must show the text and geometry the command
//! commits at the same stage. Viewport measurement cases live in
//! `viewport_dimension_tests`; these cover the remaining creation commands
//! in model space, style inheritance, batches, and degenerate stages.

use super::viewport_dimension_tests::preview_text as text;
use super::OpenCADStudio;
use crate::command::StepInput;
use acadrust::entities::{Arc, Circle, Line};
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use glam::DVec3;

fn app() -> (OpenCADStudio, usize) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    (app, i)
}

fn style_name(entity: &EntityType) -> String {
    let EntityType::Dimension(dimension) = entity else {
        panic!("expected dimension")
    };
    dimension.base().style_name.clone()
}

fn committed(app: &OpenCADStudio) -> Vec<EntityType> {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Dimension(_)))
        .cloned()
        .collect()
}

/// Styled preview entities at `cursor`, asserting they also tessellate.
fn preview(app: &OpenCADStudio, i: usize, cursor: DVec3) -> Vec<EntityType> {
    let entities = app.dimension_preview_entities(i, cursor).unwrap();
    assert!(!entities.is_empty(), "no styled preview at {cursor}");
    assert!(!app.dimension_preview_wires(i, cursor).unwrap().is_empty());
    entities
}

fn commit_and_compare(app: &mut OpenCADStudio, i: usize, cursor: DVec3) -> EntityType {
    let previewed = preview(app, i, cursor);
    assert_eq!(previewed.len(), 1);
    let before = committed(app).len();
    let _ = app.feed_command(StepInput::Point(cursor));
    let after = committed(app);
    assert_eq!(after.len(), before + 1, "commit did not add one dimension");
    let entity = after.last().unwrap().clone();
    assert_eq!(text(app, &entity), text(app, &previewed[0]));
    assert_eq!(style_name(&entity), style_name(&previewed[0]));
    entity
}

#[test]
fn ordinate_preview_matches_commit() {
    let (mut app, i) = app();
    let _ = app.dispatch_command("DIMORDINATE");
    assert!(app.dimension_preview_entities(i, DVec3::ZERO).is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(12.5, 40.0, 0.0)));
    let entity = commit_and_compare(&mut app, i, DVec3::new(12.5, 90.0, 0.0));
    assert_eq!(text(&app, &entity), "12.5");
}

#[test]
fn arc_length_preview_matches_commit_for_full_and_partial_arcs() {
    let (mut app, i) = app();
    let mut arc = Arc::new();
    arc.center = Vector3::ZERO;
    arc.radius = 50.0;
    arc.start_angle = 0.0;
    arc.end_angle = std::f64::consts::FRAC_PI_2;
    let handle = app.tabs[i].scene.add_entity(EntityType::Arc(arc));
    let on_arc = |degrees: f64| {
        let radians = degrees.to_radians();
        DVec3::new(50.0 * radians.cos(), 50.0 * radians.sin(), 0.0)
    };

    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(handle, on_arc(45.0)));
    let full = commit_and_compare(&mut app, i, DVec3::new(60.0, 60.0, 0.0));

    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(handle, on_arc(45.0)));
    let _ = app.feed_command(StepInput::Text("P".into()));
    let _ = app.feed_command(StepInput::Point(on_arc(10.0)));
    // While the second partial point is acquired the command has no
    // committable dimension; the plain arc preview stays in charge.
    assert!(app.dimension_preview_entities(i, on_arc(80.0)).is_none());
    let _ = app.feed_command(StepInput::Point(on_arc(80.0)));
    let partial = commit_and_compare(&mut app, i, DVec3::new(60.0, 60.0, 0.0));
    assert_ne!(text(&app, &full), text(&app, &partial));
}

#[test]
fn jogged_radius_preview_matches_commit_at_text_and_jog_stages() {
    let (mut app, i) = app();
    let mut circle = Circle::new();
    circle.center = Vector3::ZERO;
    circle.radius = 100.0;
    let handle = app.tabs[i].scene.add_entity(EntityType::Circle(circle));
    let _ = app.dispatch_command("DIMJOGGED");
    let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::new(100.0, 0.0, 0.0)));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(30.0, 0.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(30.0, 0.0, 0.0)));
    let text_position = DVec3::new(150.0, 40.0, 0.0);
    let staged = preview(&app, i, text_position);
    assert_eq!(staged.len(), 1);
    let _ = app.feed_command(StepInput::Point(text_position));
    let entity = commit_and_compare(&mut app, i, DVec3::new(90.0, 20.0, 0.0));
    assert_eq!(text(&app, &entity), text(&app, &staged[0]));
    assert_eq!(text(&app, &entity), "R100");
}

#[test]
fn angular_three_point_preview_matches_commit() {
    let (mut app, i) = app();
    let _ = app.dispatch_command("DIMANGULAR");
    let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
    let _ = app.feed_command(StepInput::Point(DVec3::new(100.0, 0.0, 0.0)));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(0.0, 100.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(0.0, 100.0, 0.0)));
    let entity = commit_and_compare(&mut app, i, DVec3::new(70.0, 70.0, 0.0));
    assert_eq!(text(&app, &entity), "90°");
}

fn place_linear(app: &mut OpenCADStudio, first: DVec3, second: DVec3, line: DVec3) -> Handle {
    let _ = app.dispatch_command("DIMLINEAR");
    let _ = app.feed_command(StepInput::Point(first));
    let _ = app.feed_command(StepInput::Point(second));
    let _ = app.feed_command(StepInput::Point(line));
    committed(app).last().unwrap().common().handle
}

#[test]
fn continue_and_baseline_previews_follow_style_inheritance() {
    let (mut app, i) = app();
    let default_style = app.tabs[i]
        .scene
        .document
        .header
        .current_dimstyle_name
        .clone();
    let mut style = acadrust::tables::DimStyle::new("Inherit");
    style.dimlfac = 2.0;
    app.tabs[i].scene.document.dim_styles.add(style).unwrap();
    app.tabs[i].scene.document.header.current_dimstyle_name = "Inherit".into();
    let base = place_linear(
        &mut app,
        DVec3::ZERO,
        DVec3::new(100.0, 0.0, 0.0),
        DVec3::new(50.0, -20.0, 0.0),
    );
    assert_eq!(text(&app, committed(&app).last().unwrap()), "200");
    app.tabs[i].scene.document.header.current_dimstyle_name = default_style;

    // DIMCONTINUEMODE=1 keeps the base dimension's style in the preview.
    app.dimension_continue_mode = 1;
    app.tabs[i].scene.last_created_dimension = Some(base);
    let _ = app.dispatch_command("DIMCONTINUE");
    let inherited = commit_and_compare(&mut app, i, DVec3::new(250.0, 0.0, 0.0));
    assert_eq!(style_name(&inherited), "Inherit");
    assert_eq!(text(&app, &inherited), "300");

    // DIMCONTINUEMODE=0 runs the current creation defaults over the built
    // dimension instead; preview and commit must still agree.
    app.dimension_continue_mode = 0;
    app.tabs[i].scene.last_created_dimension = Some(inherited.common().handle);
    let _ = app.dispatch_command("DIMCONTINUE");
    commit_and_compare(&mut app, i, DVec3::new(350.0, 0.0, 0.0));

    app.dimension_continue_mode = 1;
    app.tabs[i].scene.last_created_dimension = Some(base);
    let _ = app.dispatch_command("DIMBASELINE");
    let baseline = commit_and_compare(&mut app, i, DVec3::new(300.0, 0.0, 0.0));
    assert_eq!(style_name(&baseline), "Inherit");
    assert_eq!(text(&app, &baseline), "600");
}

#[test]
fn qdim_preview_matches_the_committed_batch() {
    let (mut app, i) = app();
    let segments = [(0.0, 40.0), (40.0, 100.0), (100.0, 130.0)];
    for (start, end) in segments {
        let line = Line::from_points(Vector3::new(start, 0.0, 0.0), Vector3::new(end, 0.0, 0.0));
        let handle = app.tabs[i].scene.add_entity(EntityType::Line(line));
        app.tabs[i].scene.select_entity(handle, false);
    }
    let _ = app.dispatch_command("QDIM");
    let cursor = DVec3::new(60.0, 40.0, 0.0);
    let previewed = preview(&app, i, cursor);
    assert_eq!(previewed.len(), segments.len());
    let mut preview_texts: Vec<String> = previewed.iter().map(|e| text(&app, e)).collect();
    let _ = app.feed_command(StepInput::Point(cursor));
    let mut committed_texts: Vec<String> = committed(&app).iter().map(|e| text(&app, e)).collect();
    preview_texts.sort();
    committed_texts.sort();
    assert_eq!(committed_texts, preview_texts);
    assert_eq!(committed_texts, ["30", "40", "60"]);
}

#[test]
fn degenerate_linear_preview_defers_to_plain_rubber_band() {
    let (mut app, i) = app();
    let _ = app.dispatch_command("DIMLINEAR");
    assert!(app.dimension_preview_entities(i, DVec3::ZERO).is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
    assert!(app
        .dimension_preview_entities(i, DVec3::ZERO)
        .is_some_and(|entities| entities.is_empty()));
    assert!(app.dimension_preview_wires(i, DVec3::ZERO).is_none());
    assert!(committed(&app).is_empty());
    let cursor = DVec3::new(80.0, 0.0, 0.0);
    let styled = preview(&app, i, cursor);
    assert_eq!(text(&app, &styled[0]), "80");
    assert!(committed(&app).is_empty(), "hover must not commit");
}
