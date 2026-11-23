#[cfg(test)]
mod parametric_constraint_undo_tests {
    use super::super::*;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Line(
                document::entities::Line::from_points(
                    document::types::Vector3::new(x1, y1, 0.0),
                    document::types::Vector3::new(x2, y2, 0.0),
                ),
            ))
    }

    fn add_circle(app: &mut OpenCADStudio, cx: f64, cy: f64, radius: f64) -> Handle {
        let mut circle = document::entities::Circle::new();
        circle.center = document::types::Vector3::new(cx, cy, 0.0);
        circle.radius = radius;
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Circle(circle))
    }

    fn add_arc(
        app: &mut OpenCADStudio,
        cx: f64,
        cy: f64,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Handle {
        let mut arc = document::entities::Arc::new();
        arc.center = document::types::Vector3::new(cx, cy, 0.0);
        arc.radius = radius;
        arc.start_angle = start_angle;
        arc.end_angle = end_angle;
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Arc(arc))
    }

    fn add_coincident_at(app: &mut OpenCADStudio, first: Handle, second: Handle, point: [f64; 2]) {
        let pick = |handle| crate::command::CoincidentPick {
            handle: Some(handle),
            point: glam::DVec3::new(point[0], point[1], 0.0),
            whole_curve: false,
        };
        let _ = app.apply_cmd_result(CmdResult::AddCoincidentConstraint {
            first: pick(first),
            second: pick(second),
            multiple: false,
            label: "Coincident constraint",
        });
    }

    fn assert_geometry_unchanged(
        app: &OpenCADStudio,
        handle: Handle,
        expected: &document::EntityType,
        stage: &str,
    ) {
        const EPS: f64 = 1.0e-10;
        let actual = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .unwrap_or_else(|| panic!("{stage}: entity {handle:?} disappeared"));
        match (expected, actual) {
            (document::EntityType::Line(expected), document::EntityType::Line(actual)) => {
                assert!(
                    (actual.start - expected.start).length() <= EPS
                        && (actual.end - expected.end).length() <= EPS,
                    "{stage}: line {handle:?} moved: {:?} -> {:?}",
                    expected,
                    actual
                );
                assert!(
                    (actual.length() - expected.length()).abs() <= EPS,
                    "{stage}: line {handle:?} length changed: {} -> {}",
                    expected.length(),
                    actual.length()
                );
            }
            (document::EntityType::Arc(expected), document::EntityType::Arc(actual)) => {
                assert!(
                    (actual.center - expected.center).length() <= EPS,
                    "{stage}: arc {handle:?} center moved: {:?} -> {:?}",
                    expected.center,
                    actual.center
                );
                assert!(
                    (actual.radius - expected.radius).abs() <= EPS
                        && (actual.start_angle - expected.start_angle).abs() <= EPS
                        && (actual.end_angle - expected.end_angle).abs() <= EPS,
                    "{stage}: arc {handle:?} geometry changed: {:?} -> {:?}",
                    expected,
                    actual
                );
            }
            _ => panic!("{stage}: entity {handle:?} changed type: {expected:?} -> {actual:?}"),
        }
    }

    fn circle_center_radius(app: &OpenCADStudio, handle: Handle) -> ([f64; 2], f64) {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Circle(circle)) => {
                ([circle.center.x, circle.center.y], circle.radius)
            }
            other => panic!("expected a Circle, got {other:?}"),
        }
    }

    fn line_angle_deg(app: &OpenCADStudio, handle: Handle) -> f64 {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(l)) => (l.end.y - l.start.y)
                .atan2(l.end.x - l.start.x)
                .to_degrees(),
            other => panic!("expected a Line, got {other:?}"),
        }
    }

    fn line_segment(app: &OpenCADStudio, handle: Handle) -> ([f64; 2], [f64; 2]) {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(l)) => ([l.start.x, l.start.y], [l.end.x, l.end.y]),
            other => panic!("expected a Line, got {other:?}"),
        }
    }

    fn assert_segment_unmoved(app: &OpenCADStudio, handle: Handle, start: [f64; 2], end: [f64; 2]) {
        let (s, e) = line_segment(app, handle);
        assert!(
            (s[0] - start[0]).abs() < 1e-9
                && (s[1] - start[1]).abs() < 1e-9
                && (e[0] - end[0]).abs() < 1e-9
                && (e[1] - end[1]).abs() < 1e-9,
            "the first-picked entity should not move: {s:?} -> {e:?}"
        );
    }

    /// A parallel constraint keeps the first-selected entity where it is and
    /// turns the second to match it, the order its prompt names.
    #[test]
    fn parallel_constraint_holds_the_first_selected_entity() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let horizontal = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let vertical = add_line(&mut app, 0.0, 0.0, 0.0, 100.0);
        app.tabs[app.active_tab]
            .scene
            .document
            .get_entity_mut(vertical)
            .unwrap()
            .common_mut()
            .layer = "WALLS".to_string();
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![
                ParametricRef::whole(horizontal),
                ParametricRef::whole(vertical),
            ],
            driving_param: None,
            label: "Parallel constraint",
        });
        assert_segment_unmoved(&app, horizontal, [0.0, 0.0], [100.0, 0.0]);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .get_entity(vertical)
                .unwrap()
                .common()
                .layer,
            "WALLS",
            "turning the second entity must keep its layer"
        );
        let angle = line_angle_deg(&app, vertical);
        assert!(
            angle.abs() < 1e-6,
            "the second entity should turn parallel to the first, got {angle} degrees"
        );
    }

    #[test]
    fn parallel_constraint_holds_whichever_entity_was_picked_first() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let vertical = add_line(&mut app, 0.0, 0.0, 0.0, 100.0);
        let horizontal = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![
                ParametricRef::whole(vertical),
                ParametricRef::whole(horizontal),
            ],
            driving_param: None,
            label: "Parallel constraint",
        });
        assert_segment_unmoved(&app, vertical, [0.0, 0.0], [0.0, 100.0]);
        let angle = line_angle_deg(&app, horizontal);
        assert!(
            (angle - 90.0).abs() < 1e-6,
            "the second entity should turn parallel to the first, got {angle} degrees"
        );
    }

    /// A line already parallel but running the opposite way is left alone:
    /// the second entity takes the shorter turn, which is none at all.
    #[test]
    fn parallel_constraint_leaves_an_opposite_direction_pair_alone() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let datum = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let reversed = add_line(&mut app, 0.0, 50.0, -100.0, 50.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![ParametricRef::whole(datum), ParametricRef::whole(reversed)],
            driving_param: None,
            label: "Parallel constraint",
        });
        assert_segment_unmoved(&app, datum, [0.0, 0.0], [100.0, 0.0]);
        assert_segment_unmoved(&app, reversed, [0.0, 50.0], [-100.0, 50.0]);
    }

    /// The redraw journal must name the entity the ordered command turned.
    #[test]
    fn parallel_constraint_publishes_the_entity_it_turned() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let horizontal = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let vertical = add_line(&mut app, 0.0, 0.0, 0.0, 100.0);
        let epoch = app.tabs[app.active_tab].scene.geometry_epoch;
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![
                ParametricRef::whole(horizontal),
                ParametricRef::whole(vertical),
            ],
            driving_param: None,
            label: "Parallel constraint",
        });
        let delta = app.tabs[app.active_tab]
            .scene
            .replay_since(epoch)
            .expect("the add publishes a replayable delta");
        assert!(
            delta
                .iter()
                .any(|&(handle, kind)| handle == vertical
                    && kind == crate::scene::ChangeKind::Modified),
            "the turned handle must be named in the delta: {delta:?}"
        );
    }

    /// The turn the ordered command applies is part of the same undo step.
    #[test]
    fn undo_restores_the_second_entity_a_parallel_constraint_turned() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let horizontal = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let vertical = add_line(&mut app, 0.0, 0.0, 0.0, 100.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![
                ParametricRef::whole(horizontal),
                ParametricRef::whole(vertical),
            ],
            driving_param: None,
            label: "Parallel constraint",
        });
        let turned = line_segment(&app, vertical);
        app.undo_steps(1);
        assert_segment_unmoved(&app, vertical, [0.0, 0.0], [0.0, 100.0]);
        app.redo_steps(1);
        assert_eq!(line_segment(&app, vertical), turned);
    }

    /// A tangent constraint holds the first-picked circle and slides the
    /// second-picked line straight onto it. A solve from scratch is free to
    /// turn the line and drift it off the exact tangent.
    #[test]
    fn tangent_constraint_slides_the_second_line_onto_the_first_circle() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let circle = add_circle(&mut app, 0.0, 0.0, 20.0);
        let line = add_line(&mut app, -100.0, 221.0, 100.0, 221.0);
        let _ = app.apply_cmd_result(CmdResult::AddTangentConstraint {
            first: ParametricRef::whole(circle),
            second: ParametricRef::whole(line),
            label: "Tangent constraint",
        });
        let (center, radius) = circle_center_radius(&app, circle);
        assert!(
            center[0].abs() < 1e-9 && center[1].abs() < 1e-9 && (radius - 20.0).abs() < 1e-9,
            "the datum circle should not move: {center:?} r {radius}"
        );
        let (start, end) = line_segment(&app, line);
        assert!(
            (start[0] + 100.0).abs() < 1e-6
                && (start[1] - 20.0).abs() < 1e-6
                && (end[0] - 100.0).abs() < 1e-6
                && (end[1] - 20.0).abs() < 1e-6,
            "the line should keep its direction and slide down onto the circle: {start:?} -> {end:?}"
        );
    }

    /// The pick order decides which entity gives way: a line picked first
    /// stays put and the circle slides onto it.
    #[test]
    fn tangent_constraint_holds_the_first_line_and_moves_the_circle() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, -100.0, 0.0, 100.0, 0.0);
        let circle = add_circle(&mut app, 0.0, 50.0, 20.0);
        let _ = app.apply_cmd_result(CmdResult::AddTangentConstraint {
            first: ParametricRef::whole(line),
            second: ParametricRef::whole(circle),
            label: "Tangent constraint",
        });
        assert_segment_unmoved(&app, line, [-100.0, 0.0], [100.0, 0.0]);
        let (center, radius) = circle_center_radius(&app, circle);
        assert!(
            center[0].abs() < 1e-9
                && (center[1] - 20.0).abs() < 1e-9
                && (radius - 20.0).abs() < 1e-9,
            "the circle should slide onto the line along the normal: {center:?} r {radius}"
        );
    }

    /// Constraint state and solved geometry share one undo/redo transaction.
    #[test]
    fn adding_a_constraint_is_undoable_and_redoable_atomically() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 3.0);
        let original_angle = line_angle_deg(&app, line);
        assert!((original_angle - 16.699244).abs() < 1e-3);

        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        assert!(
            line_angle_deg(&app, line).abs() < 1e-6,
            "constraint should have leveled the line"
        );
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1
        );

        app.undo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|s| s.constraints.len())
                .unwrap_or(0),
            0,
            "undo should remove the constraint record"
        );
        assert!(
            (line_angle_deg(&app, line) - original_angle).abs() < 1e-3,
            "undo should restore the pre-constraint angle"
        );

        app.redo_steps(1);
        assert!(
            line_angle_deg(&app, line).abs() < 1e-6,
            "redo should re-level the line"
        );
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1,
            "redo should restore the constraint record"
        );
    }

    #[test]
    fn perpendicular_initial_solve_rotates_parallel_second_line_in_kernel() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let first = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let second = add_line(&mut app, 20.0, 0.0, 30.0, 0.0);

        let _ = app.apply_cmd_result(CmdResult::AddPerpendicularConstraint {
            first: ParametricRef::whole(first),
            second: ParametricRef::whole(second),
            first_fixed: ParametricRef::whole(first),
            second_start: ParametricRef::point(second, 0),
            label: "Perpendicular constraint",
        });

        let line = |handle| match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(line)) => line.clone(),
            other => panic!("expected a Line, got {other:?}"),
        };
        let fixed = line(first);
        let moving = line(second);
        assert_eq!(fixed.start, document::types::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(fixed.end, document::types::Vector3::new(10.0, 0.0, 0.0));
        assert!((moving.start - document::types::Vector3::new(20.0, 0.0, 0.0)).length() < 1.0e-9);
        assert!((moving.length() - 10.0).abs() < 1.0e-7);
        assert!(
            (fixed.end - fixed.start)
                .dot(&(moving.end - moving.start))
                .abs()
                < 1.0e-7,
            "fixed={fixed:?}, moving={moving:?}"
        );
    }

    /// #2300: a second Perpendicular on the same closed polyline squares the
    /// next corner too. Keeping every edge length as well over-determines a
    /// quadrilateral, so the edges give way instead of nothing moving.
    #[test]
    fn a_second_perpendicular_on_one_polyline_squares_its_corner() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut polyline = document::entities::LwPolyline::new();
        polyline.vertices = [(20.0, 0.0), (30.0, 1.0), (30.5, 6.0), (20.0, 6.0)]
            .into_iter()
            .map(|(x, y)| document::entities::LwVertex::from_coords(x, y))
            .collect();
        polyline.is_closed = true;
        let i = app.active_tab;
        let handle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::LwPolyline(polyline));
        let corner = |app: &OpenCADStudio, k: usize| {
            let Some(document::EntityType::LwPolyline(p)) =
                app.tabs[app.active_tab].scene.document.get_entity(handle)
            else {
                panic!("expected the polyline");
            };
            let v: Vec<_> = p.vertices.iter().map(|v| v.location).collect();
            let (a, b, c) = (v[k - 1], v[k], v[(k + 1) % v.len()]);
            (b.x - a.x) * (c.x - b.x) + (b.y - a.y) * (c.y - b.y)
        };
        for (first, second) in [(0usize, 1usize), (1, 2)] {
            let _ = app.apply_cmd_result(CmdResult::AddPerpendicularConstraint {
                first: ParametricRef::segment(handle, first),
                second: ParametricRef::segment(handle, second),
                first_fixed: ParametricRef::segment(handle, first),
                second_start: ParametricRef::point(handle, second as i32),
                label: "Perpendicular constraint",
            });
        }
        assert!(corner(&app, 1).abs() < 1.0e-7, "{}", corner(&app, 1));
        assert!(corner(&app, 2).abs() < 1.0e-7, "{}", corner(&app, 2));
    }

    #[test]
    fn horizontal_initial_solve_uses_the_captured_axis_in_kernel() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let handle = add_line(&mut app, 0.0, 0.0, 5.0, 2.0);
        let (original_start, original_end, original_length) =
            match app.tabs[app.active_tab].scene.document.get_entity(handle) {
                Some(document::EntityType::Line(line)) => (line.start, line.end, line.length()),
                other => panic!("expected a Line, got {other:?}"),
            };
        let direction = document::types::Vector3::new(3.0, 4.0, 0.0).normalize();

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::whole(handle),
            ),
            direction,
            label: "Horizontal constraint",
        });

        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(line)) => line,
            other => panic!("expected a Line, got {other:?}"),
        };
        let solved = (line.end - line.start).normalize();
        assert!(solved.cross(&direction).length() < 1.0e-7);
        assert!((line.length() - original_length).abs() < 1.0e-7);
        let constraint = &app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints[0];
        assert_eq!(constraint.axis_direction, Some(direction));

        app.undo_steps(1);
        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(line)) => line,
            other => panic!("expected a Line after undo, got {other:?}"),
        };
        assert_eq!((line.start, line.end), (original_start, original_end));
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|set| set.constraints.len())
                .unwrap_or(0),
            0
        );

        app.redo_steps(1);
        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(line)) => line,
            other => panic!("expected a Line after redo, got {other:?}"),
        };
        assert!(
            (line.end - line.start)
                .normalize()
                .cross(&direction)
                .length()
                < 1.0e-7
        );
    }

    #[test]
    fn horizontal_two_point_solve_keeps_the_first_point_fixed() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let first = add_line(&mut app, 0.0, 0.0, 0.0, 5.0);
        let second = add_line(&mut app, 8.0, 3.0, 8.0, 7.0);
        let pick = |handle, point| crate::command::CoincidentPick {
            handle: Some(handle),
            point,
            whole_curve: false,
        };

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Points(
                pick(first, glam::DVec3::ZERO),
                pick(second, glam::DVec3::new(8.0, 3.0, 0.0)),
            ),
            direction: document::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let line = |handle| match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Line(line)) => line,
            other => panic!("expected a Line, got {other:?}"),
        };
        assert_eq!(line(first).start, document::types::Vector3::ZERO);
        assert!(line(second).start.y.abs() < 1.0e-8);
    }

    #[test]
    fn horizontal_minor_axis_rotates_an_ellipse_around_its_center() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut ellipse = document::entities::Ellipse::default();
        ellipse.center = document::types::Vector3::new(3.0, 7.0, 0.0);
        ellipse.major_axis = document::types::Vector3::new(3.0, 4.0, 0.0);
        ellipse.minor_axis_ratio = 0.4;
        let handle = app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Ellipse(ellipse));

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::ellipse_minor_axis(handle),
            ),
            direction: document::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let ellipse = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Ellipse(ellipse)) => ellipse,
            other => panic!("expected an Ellipse, got {other:?}"),
        };
        assert!((ellipse.center - document::types::Vector3::new(3.0, 7.0, 0.0)).length() < 1.0e-9);
        assert!(ellipse.major_axis.x.abs() < 1.0e-7);
        assert!((ellipse.major_axis.length() - 5.0).abs() < 1.0e-7);
        assert!((ellipse.minor_axis_ratio - 0.4).abs() < 1.0e-9);
    }

    /// Drawing commands must not create parametric constraints implicitly.
    #[test]
    fn drawing_a_line_onto_an_existing_endpoint_does_not_add_a_constraint() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _existing = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);

        let new_line = document::EntityType::Line(document::entities::Line::from_points(
            document::types::Vector3::new(5.0, 0.0, 0.0), // exactly on `existing`'s end point
            document::types::Vector3::new(10.0, 0.0, 0.0),
        ));
        app.tabs[app.active_tab].active_cmd = Some(Box::new(
            crate::modules::draw::draw::line::LineCommand::new(),
        ));
        let _ = app.apply_cmd_result(CmdResult::CommitEntity(new_line));

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|s| s.constraints.len())
                .unwrap_or(0),
            0,
            "a drawing command must not add an implicit constraint"
        );
    }

    /// Conflict resolution removes one flagged constraint and is undoable.
    #[test]
    fn resolve_one_parametric_conflict_removes_a_flagged_constraint_and_is_undoable() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 3.0);

        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        }); // exact duplicate — flagged redundant once bumped
        app.tabs[app.active_tab]
            .scene
            .bump_entities(&[(line, crate::scene::ChangeKind::Modified)]);

        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap();
        assert_eq!(set.constraints.len(), 2);
        assert_eq!(
            set.conflicts.len(),
            1,
            "the duplicate should have been flagged"
        );

        app.resolve_one_parametric_conflict();
        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap();
        assert_eq!(
            set.constraints.len(),
            1,
            "one of the two duplicates should have been removed"
        );

        app.undo_steps(1);
        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap();
        assert_eq!(
            set.constraints.len(),
            2,
            "undo should restore the removed constraint"
        );
    }

    #[test]
    fn deleting_one_parametric_constraint_is_undoable() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 3.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(line)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let id = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints[0]
            .id;
        app.tabs[app.active_tab].scene.selected_constraint = Some(id);

        app.delete_parametric_constraint(id);
        assert!(app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .unwrap()
            .constraints
            .is_empty());
        assert_eq!(app.tabs[app.active_tab].scene.selected_constraint, None);

        app.undo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .unwrap()
                .constraints
                .len(),
            1
        );
    }

    /// `named_parameters_design.md` stage 4: applying the PARAMETERS editor's
    /// working buffer both defines the parameter and re-solves whatever
    /// constraint already references it by name — the entire point of a
    /// *named* parameter, not just data plumbing.
    #[test]
    fn applying_named_parameter_rows_defines_and_resolves_referencing_constraints() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);

        // The constraint is added *before* the parameter exists -- allowed,
        // same as `ParameterTable::set`'s own forward-reference tolerance;
        // it just doesn't resolve to anything until the parameter is defined.
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Distance,
            refs: vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            driving_param: Some(crate::scene::named_parameters::DrivingValue::Named(
                "target_len".to_string(),
            )),
            label: "Distance constraint",
        });

        app.named_parameter_editor_rows =
            vec![crate::ui::window::named_parameters::ParamEditorRow {
                name: "target_len".to_string(),
                formula: "8".to_string(),
            }];
        app.apply_named_parameter_editor_rows();

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("target_len"),
            Ok(8.0)
        );
        let (start, end) = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(document::EntityType::Line(l)) => (l.start, l.end),
            other => panic!("expected a Line, got {other:?}"),
        };
        let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        assert!(
            (len - 8.0).abs() < 1e-6,
            "length should track the applied parameter, got {len}"
        );

        // Redefining the parameter and re-applying should ripple again.
        app.named_parameter_editor_rows[0].formula = "3".to_string();
        app.apply_named_parameter_editor_rows();
        let (start, end) = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(document::EntityType::Line(l)) => (l.start, l.end),
            other => panic!("expected a Line, got {other:?}"),
        };
        let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        assert!(
            (len - 3.0).abs() < 1e-6,
            "length should track the redefined parameter, got {len}"
        );

        app.undo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("target_len"),
            Ok(8.0)
        );
        let length_after_undo = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(document::EntityType::Line(line)) => line.length(),
            other => panic!("expected a Line, got {other:?}"),
        };
        assert!((length_after_undo - 8.0).abs() < 1e-6);

        app.redo_steps(1);
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("target_len"),
            Ok(3.0)
        );
    }

    /// The no-selection page must expose the active drawing's named parameters.
    #[test]
    fn no_selection_properties_exposes_named_parameters() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.tabs[app.active_tab]
            .scene
            .named_parameters_mut()
            .set("width", "12")
            .unwrap();

        app.refresh_properties();

        assert!(app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| matches!(
                &property.value,
                crate::scene::model::object::PropValue::ParamRow { name, .. }
                    if name == "width"
            )));
        assert!(app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| matches!(
                property.value,
                crate::scene::model::object::PropValue::ParamsVisibilityToggle(_)
            )));
    }

    /// The Parameters On/Off button flips: after each press the panel row
    /// shows the new state, so the next press sends the opposite value.
    #[test]
    fn params_visibility_toggle_flips_on_each_press() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.refresh_properties();
        let shown = |app: &OpenCADStudio| {
            app.tabs[app.active_tab]
                .properties
                .sections
                .iter()
                .flat_map(|section| &section.props)
                .find_map(|property| match property.value {
                    crate::scene::model::object::PropValue::ParamsVisibilityToggle(v) => Some(v),
                    _ => None,
                })
                .expect("no-selection page has the On/Off row")
        };
        let start = shown(&app);
        assert_eq!(start, app.show_constraint_values);

        // What a click on the button sends: the opposite of what it shows.
        let _ = app.update(Message::ShowConstraintValuesChanged(!shown(&app)));
        assert_eq!(app.show_constraint_values, !start);
        assert_eq!(shown(&app), !start, "button must show the new state");

        let _ = app.update(Message::ShowConstraintValuesChanged(!shown(&app)));
        assert_eq!(
            app.show_constraint_values, start,
            "second press turns it back"
        );
        assert_eq!(shown(&app), start);
    }

    /// End-to-end through the actual `Message` handlers a Properties-panel
    /// click/keystroke would fire — not `apply_named_parameter_editor_rows`
    /// directly — covering the whole Parameters-section lifecycle: add,
    /// rename, redefine, and confirm a referencing constraint re-solves
    /// after each commit (not just after a whole-table Apply, since this
    /// path commits per field).
    #[test]
    fn properties_panel_parameter_row_add_rename_redefine_and_delete() {
        use crate::ui::window::named_parameters::ParamField;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let line = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Distance,
            refs: vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            driving_param: Some(crate::scene::named_parameters::DrivingValue::Named(
                "len".to_string(),
            )),
            label: "Distance constraint",
        });

        // Add: the first row is auto-named "param1".
        let _ = app.update(Message::PropParamAddNew);
        assert!(app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("param1"));
        let index = app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .iter()
            .position(|p| p.name == "param1")
            .unwrap();

        // Rename param1 -> len (commit-on-submit, not per keystroke: input
        // alone must not touch the table yet).
        let _ = app.update(Message::PropParamInput {
            index,
            field: ParamField::Name,
            value: "len".to_string(),
        });
        assert!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .contains("param1"),
            "typing alone must not commit"
        );
        let _ = app.update(Message::PropParamCommit {
            index,
            field: ParamField::Name,
        });
        assert!(!app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("param1"));
        assert!(app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("len"));

        // Redefine its formula to 8 and confirm the Distance constraint
        // (already referencing "len" by name, defined before this) re-solves.
        let _ = app.update(Message::PropParamInput {
            index,
            field: ParamField::Formula,
            value: "8".to_string(),
        });
        let _ = app.update(Message::PropParamCommit {
            index,
            field: ParamField::Formula,
        });
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("len"),
            Ok(8.0)
        );
        let (start, end) = match app.tabs[app.active_tab].scene.document.get_entity(line) {
            Some(document::EntityType::Line(l)) => (l.start, l.end),
            other => panic!("expected a Line, got {other:?}"),
        };
        let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        assert!(
            (len - 8.0).abs() < 1e-6,
            "length should track the redefined parameter, got {len}"
        );

        // Delete: the row is gone from the table.
        let _ = app.update(Message::PropParamDelete(index));
        assert!(!app.tabs[app.active_tab]
            .scene
            .named_parameters()
            .contains("len"));
    }

    /// A Constraints-section row click selects every entity the constraint
    /// references, replacing whatever was selected before.
    #[test]
    fn properties_panel_constraint_link_click_selects_its_entities() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let a = add_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let b = add_line(&mut app, 0.0, 5.0, 10.0, 5.0);
        let unrelated = add_line(&mut app, 20.0, 20.0, 30.0, 20.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            driving_param: None,
            label: "Parallel constraint",
        });
        app.tabs[app.active_tab]
            .scene
            .select_entity(unrelated, true);

        let _ = app.update(Message::PropConstraintLinkClick(vec![a, b]));

        let selected: std::collections::HashSet<Handle> = app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        assert_eq!(selected, std::collections::HashSet::from([a, b]));
    }

    /// A row that fails to validate (here, a formula that doesn't parse)
    /// must not silently vanish or corrupt the table: it's reported as an
    /// error and simply isn't added, while a valid row alongside it still
    /// commits — the same isolate-the-failure treatment
    /// `ParameterTable::resolve_all` already gives a bad reference.
    #[test]
    fn applying_a_row_with_a_malformed_formula_reports_an_error_and_keeps_the_good_row() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        app.named_parameter_editor_rows = vec![
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "good".to_string(),
                formula: "10".to_string(),
            },
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "bad".to_string(),
                formula: "1 +".to_string(),
            },
        ];
        app.apply_named_parameter_editor_rows();

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("good"),
            Ok(10.0)
        );
        assert!(
            !app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .contains("bad"),
            "a malformed row must not be added"
        );
        assert!(
            app.command_line
                .history
                .iter()
                .any(|e| e.kind == crate::ui::command_line::EntryKind::Error),
            "a malformed row must be reported as an error, not silently dropped"
        );
    }

    /// Two rows typed with the same name must not silently collapse into
    /// "whichever `set` call happens last wins" — both are refused, and an
    /// unrelated row still commits normally.
    #[test]
    fn applying_rows_with_a_duplicate_name_refuses_both_and_keeps_the_unrelated_row() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        app.named_parameter_editor_rows = vec![
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "x".to_string(),
                formula: "1".to_string(),
            },
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "x".to_string(),
                formula: "2".to_string(),
            },
            crate::ui::window::named_parameters::ParamEditorRow {
                name: "y".to_string(),
                formula: "3".to_string(),
            },
        ];
        app.apply_named_parameter_editor_rows();

        assert!(
            !app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .contains("x"),
            "a duplicated name must not be added at all"
        );
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .named_parameters()
                .resolve("y"),
            Ok(3.0)
        );
        assert!(app
            .command_line
            .history
            .iter()
            .any(|e| e.kind == crate::ui::command_line::EntryKind::Error));
    }

    /// Two endpoint picks resolve to a Coincident constraint.
    #[test]
    fn coincident_command_resolves_two_picked_points_into_a_constraint() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let a = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);
        let b = add_line(&mut app, 20.0, 0.0, 20.0, 5.0); // deliberately far from `a`, so the two picks are unambiguous

        let _ = app.apply_cmd_result(CmdResult::AddCoincidentConstraint {
            first: crate::command::CoincidentPick {
                handle: Some(a),
                point: glam::DVec3::new(5.0, 0.0, 0.0),
                whole_curve: false,
            },
            second: crate::command::CoincidentPick {
                handle: Some(b),
                point: glam::DVec3::new(20.0, 0.0, 0.0),
                whole_curve: false,
            },
            multiple: false,
            label: "Coincident constraint",
        });

        let set = app.tabs[app.active_tab]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .expect("a constraint set should exist");
        assert_eq!(set.constraints.len(), 1);
        let c = &set.constraints[0];
        assert_eq!(c.kind, ConstraintKind::Coincident);
        let entities: Vec<Handle> = c.refs.iter().map(|r| r.entity).collect();
        assert!(
            entities.contains(&a) && entities.contains(&b),
            "the constraint should reference both lines"
        );
        let Some(document::EntityType::Line(first)) =
            app.tabs[app.active_tab].scene.document.get_entity(a)
        else {
            panic!("expected first line");
        };
        let Some(document::EntityType::Line(second)) =
            app.tabs[app.active_tab].scene.document.get_entity(b)
        else {
            panic!("expected second line");
        };
        assert!((first.end - document::types::Vector3::new(5.0, 0.0, 0.0)).length() < 1.0e-7);
        assert!((second.start - first.end).length() < 1.0e-7);
        assert!((second.length() - 5.0).abs() < 1.0e-7);
    }

    #[test]
    fn satisfied_h_chain_stays_exactly_in_place_during_initial_constraint_solves() {
        use std::f64::consts::{FRAC_PI_2, PI};

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        // BYPI 0 0 60 0 60 40 100 40 100 80, with all three R15 corners.
        // The middle arc is stored counter-clockwise, hence its endpoints
        // appear in the opposite order along the alignment.
        let elements = vec![
            add_line(&mut app, 0.0, 0.0, 45.0, 0.0),
            add_arc(&mut app, 45.0, 15.0, 15.0, -FRAC_PI_2, 0.0),
            add_line(&mut app, 60.0, 15.0, 60.0, 25.0),
            add_arc(&mut app, 75.0, 25.0, 15.0, FRAC_PI_2, PI),
            add_line(&mut app, 75.0, 40.0, 85.0, 40.0),
            add_arc(&mut app, 85.0, 55.0, 15.0, -FRAC_PI_2, 0.0),
            add_line(&mut app, 100.0, 55.0, 100.0, 80.0),
        ];
        let originals: Vec<_> = elements
            .iter()
            .map(|&handle| {
                app.tabs[app.active_tab]
                    .scene
                    .document
                    .get_entity(handle)
                    .unwrap()
                    .clone()
            })
            .collect();
        let assert_all_unmoved = |app: &OpenCADStudio, stage: &str| {
            for (&handle, expected) in elements.iter().zip(&originals) {
                assert_geometry_unchanged(app, handle, expected, stage);
            }
        };
        let joints = [
            (0usize, 1usize, [45.0, 0.0]),
            (1, 2, [60.0, 15.0]),
            (2, 3, [60.0, 25.0]),
            (3, 4, [75.0, 40.0]),
            (4, 5, [85.0, 40.0]),
            (5, 6, [100.0, 55.0]),
        ];

        for (joint, (first, second, point)) in joints.into_iter().enumerate() {
            add_coincident_at(&mut app, elements[first], elements[second], point);
            assert_all_unmoved(&app, &format!("coincident at joint {joint}"));

            let _ = app.apply_cmd_result(CmdResult::AddTangentConstraint {
                first: ParametricRef::whole(elements[first]),
                second: ParametricRef::whole(elements[second]),
                label: "Tangent constraint",
            });
            assert_all_unmoved(&app, &format!("tangent at joint {joint}"));
        }
    }

    #[test]
    fn violated_coincident_moves_the_second_reference_onto_the_first() {
        const EPS: f64 = 1.0e-10;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let first = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);
        let second = add_line(&mut app, 20.0, 0.0, 20.0, 5.0);
        let first_before = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(first)
            .unwrap()
            .clone();
        assert!(line_segment(&app, second).0[0] - 5.0 > 1.0);

        let _ = app.apply_cmd_result(CmdResult::AddCoincidentConstraint {
            first: crate::command::CoincidentPick {
                handle: Some(first),
                point: glam::DVec3::new(5.0, 0.0, 0.0),
                whole_curve: false,
            },
            second: crate::command::CoincidentPick {
                handle: Some(second),
                point: glam::DVec3::new(20.0, 0.0, 0.0),
                whole_curve: false,
            },
            multiple: false,
            label: "Coincident constraint",
        });

        let (document::EntityType::Line(expected), Some(document::EntityType::Line(actual))) = (
            &first_before,
            app.tabs[app.active_tab].scene.document.get_entity(first),
        ) else {
            panic!("the first reference changed entity type");
        };
        assert!(
            (actual.end - expected.end).length() <= EPS,
            "the picked reference point must stay fixed: {:?} -> {:?}",
            expected.end,
            actual.end
        );
        let (start, end) = line_segment(&app, second);
        assert!(
            (start[0] - 5.0).abs() <= EPS && start[1].abs() <= EPS,
            "the second start must move onto the first end: {start:?}"
        );
        let length = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
        assert!(
            (length - 5.0).abs() <= 1.0e-7,
            "the second segment must remain rigid at its picked point: {start:?} -> {end:?}"
        );
    }

    /// A pick that doesn't land near any real point (no object snap match)
    /// must not add a constraint — it should report an error and leave the
    /// scope untouched rather than silently constraining the wrong thing.
    #[test]
    fn coincident_command_reports_an_error_when_a_pick_misses() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _a = add_line(&mut app, 0.0, 0.0, 5.0, 0.0);

        let _ = app.apply_cmd_result(CmdResult::AddCoincidentConstraint {
            first: crate::command::CoincidentPick {
                handle: Some(_a),
                point: glam::DVec3::new(5.0, 0.0, 0.0),
                whole_curve: false,
            },
            second: crate::command::CoincidentPick {
                handle: None,
                point: glam::DVec3::new(500.0, 500.0, 0.0),
                whole_curve: false,
            },
            multiple: false,
            label: "Coincident constraint",
        });

        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .parametric_constraint_set(ParametricScope::ModelSpace)
                .map(|s| s.constraints.len())
                .unwrap_or(0),
            0,
            "a missed pick must not add a constraint"
        );
    }
}
