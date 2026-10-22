//! Styled placement previews must show the text and geometry the command
//! commits at the same stage. Viewport measurement cases live in
//! `viewport_dimension_tests`; these cover the remaining creation commands
//! in model space, style inheritance, batches, and degenerate stages.

use super::viewport_dimension_tests::preview_text as text;
use super::OpenCADStudio;
use crate::command::StepInput;
use codec::entities::{Arc, Circle, Line};
use codec::types::{Handle, Vector3};
use codec::EntityType;
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
    // A new drawing is metric and current on ISO-25, whose decimal separator
    // is a comma and whose trailing zeros are suppressed.
    assert_eq!(text(&app, &entity), "12,5");
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
    let mut style = codec::tables::DimStyle::new("Inherit");
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

/// The vertical extent of a dimension's rendered text.
fn rendered_text_height(app: &OpenCADStudio, handle: Handle) -> f32 {
    let name = handle.value().to_string();
    let wires = app.tabs[app.active_tab].scene.entity_wires();
    let (low, high) = wires
        .iter()
        .filter(|wire| wire.name == name)
        .flat_map(|wire| &wire.text_verts)
        .fold((f32::MAX, f32::MIN), |(low, high), vertex| {
            (low.min(vertex.pos[1]), high.max(vertex.pos[1]))
        });
    high - low
}

#[test]
fn annotative_dimension_text_follows_the_scale_once() {
    // #1939: a dimension holding a representation for the displayed scale
    // drew its text at the square of that scale.
    let (mut app, i) = app();
    let _ = app.dispatch_command("CANNOSCALE 1:1");
    app.dimstyle_selected = app.tabs[i]
        .scene
        .document
        .header
        .current_dimstyle_name
        .clone();
    app.load_dimstyle_bufs(i);
    app.apply_ds_toggle(crate::app::DsField::Annotative);
    app.apply_dimstyle_bufs(i);
    let dimension = place_linear(
        &mut app,
        DVec3::ZERO,
        DVec3::new(100.0, 0.0, 0.0),
        DVec3::new(50.0, 20.0, 0.0),
    );
    let at_one = rendered_text_height(&app, dimension);
    app.annotation_auto_scale = 4;
    let _ = app.dispatch_command("CANNOSCALE 1:2");
    let scales: Vec<_> =
        crate::scene::annotative::object_scale_memberships(&app.tabs[i].scene.document, dimension)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
    assert_eq!(scales, ["1:1", "1:2"]);
    let ratio = rendered_text_height(&app, dimension) / at_one;
    assert!((ratio - 2.0).abs() < 1.0e-3, "text grew {ratio}x at 1:2");
}

#[test]
fn region_radial_dimensions_exact_radius_and_diameter_preview_matches_commit() {
    use kernel::geom2d::{Circle as KernelCircle, Curve};
    let (mut app, i) = app();
    let region = crate::scene::model::presspull_model::region_from_loops(
        &[
            vec![Curve::Circle(KernelCircle {
                centre: [0.0, 0.0],
                radius: 10.0,
            })],
            vec![Curve::Circle(KernelCircle {
                centre: [0.0, 0.0],
                radius: 2.0,
            })],
        ],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(region);

    // DIMRADIUS outer edge (radius 10)
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::new(10.0, 0.0, 0.0)));
    let dim_r_outer = commit_and_compare(&mut app, i, DVec3::new(15.0, 10.0, 0.0));
    assert_eq!(text(&app, &dim_r_outer), "R10");

    // DIMRADIUS inner edge (hole, radius 2)
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::new(2.0, 0.0, 0.0)));
    let dim_r_inner = commit_and_compare(&mut app, i, DVec3::new(5.0, 5.0, 0.0));
    assert_eq!(text(&app, &dim_r_inner), "R2");

    // DIMDIAMETER outer edge (diameter 20)
    let _ = app.dispatch_command("DIMDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::new(0.0, 10.0, 0.0)));
    let dim_d_outer = commit_and_compare(&mut app, i, DVec3::new(10.0, 15.0, 0.0));
    assert_eq!(text(&app, &dim_d_outer), "∅20");

    // DIMDIAMETER inner edge (hole, diameter 4)
    let _ = app.dispatch_command("DIMDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::new(0.0, 2.0, 0.0)));
    let dim_d_inner = commit_and_compare(&mut app, i, DVec3::new(4.0, 4.0, 0.0));
    assert_eq!(text(&app, &dim_d_inner), "∅4");
}

#[test]
fn region_arc_length_preview_and_commit_for_circular_arc_edges() {
    use kernel::geom2d::{Arc as KernelArc, Circle as KernelCircle, Curve, Line as KernelLine};
    let (mut app, i) = app();
    let arc_loop = vec![
        Curve::Arc(KernelArc {
            centre: [0.0, 0.0],
            radius: 20.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        }),
        Curve::Line(KernelLine {
            start: [0.0, 20.0],
            end: [0.0, 0.0],
        }),
        Curve::Line(KernelLine {
            start: [0.0, 0.0],
            end: [20.0, 0.0],
        }),
    ];
    let region = crate::scene::model::presspull_model::region_from_loops(
        &[arc_loop],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(region);

    // DIMARC on the circular arc edge (radius 20, sweep pi/2 -> arc length ~31.4159)
    let mid_angle = std::f64::consts::FRAC_PI_4;
    let on_arc = DVec3::new(20.0 * mid_angle.cos(), 20.0 * mid_angle.sin(), 0.0);
    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(handle, on_arc));
    let committed_dim = commit_and_compare(&mut app, i, DVec3::new(25.0, 25.0, 0.0));
    let measured_text = text(&app, &committed_dim);
    assert!(
        measured_text.contains("31,4"),
        "expected arc length ~31.4, got {measured_text}"
    );

    // Closed circular edge should be rejected by DIMARC
    let circle_region = crate::scene::model::presspull_model::region_from_loops(
        &[vec![Curve::Circle(KernelCircle {
            centre: [0.0, 0.0],
            radius: 10.0,
        })]],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    let circle_handle = app.tabs[i].scene.add_entity(circle_region);
    let before_count = committed(&app).len();
    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(
        circle_handle,
        DVec3::new(10.0, 0.0, 0.0),
    ));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(15.0, 0.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(15.0, 0.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);
}

#[test]
fn region_dimension_association_refreshes_after_same_topology_change() {
    use kernel::geom2d::{Arc as KernelArc, Circle as KernelCircle, Curve, Line as KernelLine};
    let (mut app, i) = app();
    app.tabs[i].scene.document.header.dimension_associativity = 2;
    let initial_region = crate::scene::model::presspull_model::region_from_loops(
        &[
            vec![
                Curve::Arc(KernelArc {
                    centre: [0.0, 0.0],
                    radius: 20.0,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::FRAC_PI_2,
                }),
                Curve::Line(KernelLine {
                    start: [0.0, 20.0],
                    end: [0.0, 0.0],
                }),
                Curve::Line(KernelLine {
                    start: [0.0, 0.0],
                    end: [20.0, 0.0],
                }),
            ],
            vec![Curve::Circle(KernelCircle {
                centre: [5.0, 5.0],
                radius: 2.0,
            })],
        ],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(initial_region);

    // Commit DIMRADIUS on inner circle
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::new(7.0, 5.0, 0.0)));
    let rad_dim = commit_and_compare(&mut app, i, DVec3::new(10.0, 10.0, 0.0));
    assert_eq!(text(&app, &rad_dim), "R2");

    // Commit DIMARC on outer arc
    let mid_angle = std::f64::consts::FRAC_PI_4;
    let on_arc = DVec3::new(20.0 * mid_angle.cos(), 20.0 * mid_angle.sin(), 0.0);
    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(handle, on_arc));
    let _ = commit_and_compare(&mut app, i, DVec3::new(25.0, 25.0, 0.0));

    // Update the region with modified radii (same topology: outer radius 30.0, hole radius 4.0)
    let mut updated_region = crate::scene::model::presspull_model::region_from_loops(
        &[
            vec![
                Curve::Arc(KernelArc {
                    centre: [0.0, 0.0],
                    radius: 30.0,
                    start_angle: 0.0,
                    end_angle: std::f64::consts::FRAC_PI_2,
                }),
                Curve::Line(KernelLine {
                    start: [0.0, 30.0],
                    end: [0.0, 0.0],
                }),
                Curve::Line(KernelLine {
                    start: [0.0, 0.0],
                    end: [30.0, 0.0],
                }),
            ],
            vec![Curve::Circle(KernelCircle {
                centre: [5.0, 5.0],
                radius: 4.0,
            })],
        ],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    match &mut updated_region {
        EntityType::Region(r) => r.common.handle = handle,
        _ => unreachable!(),
    }
    assert!(app.tabs[i].scene.update_entity(updated_region));

    // Verify refreshed dimension values
    let refreshed_dims = committed(&app);
    let refreshed_rad = refreshed_dims.iter().find(|d| matches!(d, EntityType::Dimension(dim) if matches!(**dim, codec::entities::Dimension::Radius(_)))).unwrap();
    assert_eq!(text(&app, refreshed_rad), "R4");

    let refreshed_arc = refreshed_dims.iter().find(|d| matches!(d, EntityType::Dimension(dim) if matches!(**dim, codec::entities::Dimension::Arc(_)))).unwrap();
    let new_arc_text = text(&app, refreshed_arc);
    assert!(
        new_arc_text.contains("47,1"),
        "expected ~47.1, got {new_arc_text}"
    );
}

#[test]
fn region_dimension_rejection_for_straight_and_noncircular_edges() {
    use kernel::geom2d::{Arc as KernelArc, Curve, Line as KernelLine};
    let (mut app, i) = app();
    let loop_pieces = vec![
        Curve::Arc(KernelArc {
            centre: [0.0, 0.0],
            radius: 20.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        }),
        Curve::Line(KernelLine {
            start: [0.0, 20.0],
            end: [0.0, 0.0],
        }),
        Curve::Line(KernelLine {
            start: [0.0, 0.0],
            end: [20.0, 0.0],
        }),
    ];
    let region = crate::scene::model::presspull_model::region_from_loops(
        &[loop_pieces],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(region);

    let before_count = committed(&app).len();
    let straight_pt = DVec3::new(0.0, 10.0, 0.0);

    // DIMRADIUS on straight edge
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, straight_pt));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(10.0, 10.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(10.0, 10.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);

    // DIMDIAMETER on straight edge
    let _ = app.dispatch_command("DIMDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(handle, straight_pt));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(10.0, 10.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(10.0, 10.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);

    // DIMARC on straight edge
    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(handle, straight_pt));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(10.0, 10.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(10.0, 10.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);
}

#[test]
fn region_inclined_plane_radial_dimension_reports_world_radius() {
    use kernel::geom2d::{Circle as KernelCircle, Curve};
    let (mut app, i) = app();
    let inclined_plane = crate::command::WorkingPlane::new(
        DVec3::new(100.0, 100.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.0, 1.0, 1.0).normalize(),
    );
    let region = crate::scene::model::presspull_model::region_from_loops(
        &[vec![Curve::Circle(KernelCircle {
            centre: [0.0, 0.0],
            radius: 15.0,
        })]],
        inclined_plane,
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(region);

    let pick_pt = inclined_plane.to_world(DVec3::new(15.0, 0.0, 0.0));
    let place_pt = inclined_plane.to_world(DVec3::new(25.0, 10.0, 0.0));

    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, pick_pt));
    let dim = commit_and_compare(&mut app, i, place_pt);
    assert_eq!(text(&app, &dim), "R15");
}

#[test]
fn baseline_circle_and_arc_dimensions_preserved() {
    let (mut app, i) = app();
    let mut circle = Circle::new();
    circle.center = Vector3::ZERO;
    circle.radius = 25.0;
    let circle_h = app.tabs[i].scene.add_entity(EntityType::Circle(circle));

    let mut arc = Arc::new();
    arc.center = Vector3::ZERO;
    arc.radius = 25.0;
    arc.start_angle = 0.0;
    arc.end_angle = std::f64::consts::PI;
    let arc_h = app.tabs[i].scene.add_entity(EntityType::Arc(arc));

    // Circle DIMRADIUS
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(circle_h, DVec3::new(25.0, 0.0, 0.0)));
    let r_dim = commit_and_compare(&mut app, i, DVec3::new(35.0, 10.0, 0.0));
    assert_eq!(text(&app, &r_dim), "R25");

    // Circle DIMDIAMETER
    let _ = app.dispatch_command("DIMDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(circle_h, DVec3::new(0.0, 25.0, 0.0)));
    let d_dim = commit_and_compare(&mut app, i, DVec3::new(10.0, 35.0, 0.0));
    assert_eq!(text(&app, &d_dim), "∅50");

    // Arc DIMARC
    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(arc_h, DVec3::new(0.0, 25.0, 0.0)));
    let a_dim = commit_and_compare(&mut app, i, DVec3::new(0.0, 35.0, 0.0));
    assert!(text(&app, &a_dim).contains("78,5"));
}

#[test]
fn region_dimension_rejection_for_noncircular_and_near_circle_nurbs_edges() {
    use kernel::geom2d::{Curve, Line as KernelLine, NurbsCurve};
    let (mut app, i) = app();
    // Near-circular rational quadratic NURBS:
    // P0 = [10.0, 0.0], P1 = [10.0, 9.9995], P2 = [0.0, 9.9995], w = [1.0, 1/sqrt(2), 1.0].
    // Tangent lengths 9.9995 != 10.0. The 7-sample test would pass because 0.0005 < 1e-4*(1+10),
    // but the analytical certificate must reject this near-circular curve.
    let near_circle_nurbs = NurbsCurve::new(
        2,
        vec![[10.0, 0.0], [10.0, 9.9995], [0.0, 9.9995]],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
    )
    .unwrap();
    let closing_lines = vec![
        Curve::Nurbs(near_circle_nurbs),
        Curve::Line(KernelLine {
            start: [0.0, 9.9995],
            end: [0.0, 0.0],
        }),
        Curve::Line(KernelLine {
            start: [0.0, 0.0],
            end: [10.0, 0.0],
        }),
    ];
    let region = crate::scene::model::presspull_model::region_from_loops(
        &[closing_lines],
        crate::command::WorkingPlane::default(),
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(region);
    let before_count = committed(&app).len();
    let pick_pt = DVec3::new(7.07, 7.07, 0.0);

    // DIMRADIUS on near-circle NURBS must be rejected
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, pick_pt));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(15.0, 15.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(15.0, 15.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);

    // DIMDIAMETER on near-circle NURBS must be rejected
    let _ = app.dispatch_command("DIMDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(handle, pick_pt));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(15.0, 15.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(15.0, 15.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);

    // DIMARC on near-circle NURBS must be rejected
    let _ = app.dispatch_command("DIMARC");
    let _ = app.feed_command(StepInput::EntityPick(handle, pick_pt));
    assert!(app
        .dimension_preview_entities(i, DVec3::new(15.0, 15.0, 0.0))
        .is_none());
    let _ = app.feed_command(StepInput::Point(DVec3::new(15.0, 15.0, 0.0)));
    assert_eq!(committed(&app).len(), before_count);
}

#[test]
fn region_radial_dimensions_large_translated_coordinates_certified() {
    use kernel::geom2d::{Circle as KernelCircle, Curve};
    let (mut app, i) = app();
    let large_plane = crate::command::WorkingPlane::new(
        DVec3::new(1_000_000.0, 1_000_000.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(0.0, 1.0, 0.0),
    );
    let region = crate::scene::model::presspull_model::region_from_loops(
        &[vec![Curve::Circle(KernelCircle {
            centre: [0.0, 0.0],
            radius: 25.0,
        })]],
        large_plane,
    )
    .unwrap();
    let handle = app.tabs[i].scene.add_entity(region);

    let pick_pt = DVec3::new(1_000_025.0, 1_000_000.0, 0.0);
    let place_pt = DVec3::new(1_000_040.0, 1_000_020.0, 0.0);

    // DIMRADIUS
    let _ = app.dispatch_command("DIMRADIUS");
    let _ = app.feed_command(StepInput::EntityPick(handle, pick_pt));
    let dim_r = commit_and_compare(&mut app, i, place_pt);
    assert_eq!(text(&app, &dim_r), "R25");

    // DIMDIAMETER
    let _ = app.dispatch_command("DIMDIAMETER");
    let _ = app.feed_command(StepInput::EntityPick(handle, pick_pt));
    let dim_d = commit_and_compare(&mut app, i, place_pt);
    assert_eq!(text(&app, &dim_d), "∅50");
}
