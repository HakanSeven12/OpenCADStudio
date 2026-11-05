#[cfg(test)]
#[path = "trim_hatch_tests.rs"]
mod trim_hatch_tests;

#[cfg(test)]
mod command_replacement_tests {
    use super::super::*;
    use codec::entities::{Circle, Line};
    use codec::types::Vector3;

    #[test]
    fn one_to_one_edits_keep_identity_but_type_changes_do_not() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let owner = app.tabs[tab]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .owner_handle;

        let kept = app.replace_command_entity(
            tab,
            handle,
            vec![codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(3.0, 0.0, 0.0),
            ))],
        );
        assert_eq!(kept, vec![handle]);
        let edited = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(edited.common().owner_handle, owner);
        assert!(matches!(edited, codec::EntityType::Line(line) if line.end.x == 3.0));

        let allocated = app.replace_command_entity(
            tab,
            handle,
            vec![codec::EntityType::Circle(Circle::from_coords(
                0.0, 0.0, 0.0, 2.0,
            ))],
        );
        assert_eq!(allocated.len(), 1);
        assert_ne!(allocated[0], handle);
        assert!(app.tabs[tab].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn live_geometry_updates_keep_document_identity_and_display_properties() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let expected = {
            let entity = app.tabs[tab].scene.document.get_entity_mut(handle).unwrap();
            entity.common_mut().layer = "LIVE".to_string();
            entity.common_mut().invisible = true;
            entity.common_mut().linetype_scale = 2.5;
            entity.common().clone()
        };

        let _ = app.apply_cmd_result(CmdResult::UpdateLiveEntity {
            handle,
            entity: codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(4.0, 0.0, 0.0),
            )),
            finish: false,
        });

        let updated = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(updated.common().handle, expected.handle);
        assert_eq!(updated.common().owner_handle, expected.owner_handle);
        assert_eq!(updated.common().layer, expected.layer);
        assert_eq!(updated.common().invisible, expected.invisible);
        assert_eq!(updated.common().linetype_scale, expected.linetype_scale);
        assert!(matches!(updated, codec::EntityType::Line(line) if line.end.x == 4.0));
    }
}

#[cfg(test)]
mod trim_extend_history_tests {
    use super::super::*;
    use crate::modules::draw::modify::trim::{ExtendCommand, TrimCommand};
    use codec::entities::Line;
    use codec::types::Vector3;

    fn add_line(app: &mut OpenCADStudio, a: [f64; 2], b: [f64; 2]) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::new(a[0], a[1], 0.0),
                Vector3::new(b[0], b[1], 0.0),
            )))
    }

    fn assert_line_exists(app: &OpenCADStudio, a: [f64; 2], b: [f64; 2]) {
        assert!(
            app.tabs[app.active_tab]
                .scene
                .document
                .entities()
                .any(|entity| {
                    matches!(entity, codec::EntityType::Line(line)
                if (line.start.x - a[0]).abs() < 1e-9
                    && (line.start.y - a[1]).abs() < 1e-9
                    && (line.end.x - b[0]).abs() < 1e-9
                    && (line.end.y - b[1]).abs() < 1e-9)
                }),
            "line from {a:?} to {b:?} should exist"
        );
    }

    #[test]
    fn trim_command_dispatch_includes_fill_only_hatch_targets() {
        use codec::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
        use codec::types::Vector2;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|p| Vector2::new(p[0], p[1]))
                .collect(),
            true,
        )));
        hatch.paths.push(path);
        let i = app.active_tab;
        let hatch_handle = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Hatch(Box::new(hatch)));
        let boundary = add_line(&mut app, [5.0, -2.0], [5.0, 12.0]);
        // Pick-first supplies a deterministic Standard-mode cutting edge,
        // regardless of the global TRIMMODE setting from other tests.
        app.tabs[i].scene.select_entity(boundary, false);
        assert!(app.tabs[i]
            .scene
            .visible_hatch_handles_for_edit()
            .contains(&hatch_handle));

        // This exercises command dispatch, not just TrimCommand::new().
        // A filled hatch does not appear in entity_wires(): previously the
        // command snapshot omitted it, making every inside click a no-op.
        let _ = app.dispatch_command("TRIM");
        let cmd = app.tabs[i].active_cmd.as_mut().expect("TRIM command active");
        assert_eq!(cmd.name(), "TRIM");
        let result = cmd.on_entity_pick(hatch_handle, glam::DVec3::new(2.0, 5.0, 0.0));
        assert!(matches!(result, CmdResult::ReplaceEntity(h, edited)
            if h == hatch_handle && matches!(edited.first(), Some(codec::EntityType::Hatch(_)))));
    }

    #[test]
    fn trim_replacement_survives_undo_and_redo() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let target = add_line(&mut app, [0.0, 0.0], [10.0, 0.0]);
        let _boundary = add_line(&mut app, [6.0, -2.0], [6.0, 2.0]);
        let all: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .cloned()
            .collect();
        app.tabs[app.active_tab].active_cmd = Some(Box::new(TrimCommand::new(all)));
        let result = app.tabs[app.active_tab]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::DVec3::new(8.0, 0.0, 0.0));
        let _ = app.apply_cmd_result(result);
        assert_line_exists(&app, [0.0, 0.0], [6.0, 0.0]);

        app.undo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [10.0, 0.0]);
        app.redo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [6.0, 0.0]);
    }

    #[test]
    fn extend_replacement_survives_undo_and_redo() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let target = add_line(&mut app, [0.0, 0.0], [4.0, 0.0]);
        let _boundary = add_line(&mut app, [8.0, -2.0], [8.0, 2.0]);
        let all: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .cloned()
            .collect();
        app.tabs[app.active_tab].active_cmd = Some(Box::new(ExtendCommand::new(all)));
        let result = app.tabs[app.active_tab]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::DVec3::new(3.5, 0.0, 0.0));
        let _ = app.apply_cmd_result(result);
        assert_line_exists(&app, [0.0, 0.0], [8.0, 0.0]);

        app.undo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [4.0, 0.0]);
        app.redo_active_tab();
        assert_line_exists(&app, [0.0, 0.0], [8.0, 0.0]);
    }
}

#[cfg(test)]
mod parametric_constraint_undo_tests {
    use super::super::*;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::new(x1, y1, 0.0),
                codec::types::Vector3::new(x2, y2, 0.0),
            )))
    }

    fn add_circle(app: &mut OpenCADStudio, cx: f64, cy: f64, radius: f64) -> Handle {
        let mut circle = codec::entities::Circle::new();
        circle.center = codec::types::Vector3::new(cx, cy, 0.0);
        circle.radius = radius;
        app.tabs[app.active_tab]
            .scene
            .add_entity(codec::EntityType::Circle(circle))
    }

    fn circle_center_radius(app: &OpenCADStudio, handle: Handle) -> ([f64; 2], f64) {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Circle(circle)) => {
                ([circle.center.x, circle.center.y], circle.radius)
            }
            other => panic!("expected a Circle, got {other:?}"),
        }
    }

    fn line_angle_deg(app: &OpenCADStudio, handle: Handle) -> f64 {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Line(l)) => (l.end.y - l.start.y)
                .atan2(l.end.x - l.start.x)
                .to_degrees(),
            other => panic!("expected a Line, got {other:?}"),
        }
    }

    fn line_segment(app: &OpenCADStudio, handle: Handle) -> ([f64; 2], [f64; 2]) {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Line(l)) => ([l.start.x, l.start.y], [l.end.x, l.end.y]),
            other => panic!("expected a Line, got {other:?}"),
        }
    }

    fn assert_segment_unmoved(
        app: &OpenCADStudio,
        handle: Handle,
        start: [f64; 2],
        end: [f64; 2],
    ) {
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
            refs: vec![
                ParametricRef::whole(datum),
                ParametricRef::whole(reversed),
            ],
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
            center[0].abs() < 1e-9 && (center[1] - 20.0).abs() < 1e-9 && (radius - 20.0).abs() < 1e-9,
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
            Some(codec::EntityType::Line(line)) => line.clone(),
            other => panic!("expected a Line, got {other:?}"),
        };
        let fixed = line(first);
        let moving = line(second);
        assert_eq!(fixed.start, codec::types::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(fixed.end, codec::types::Vector3::new(10.0, 0.0, 0.0));
        assert!((moving.start - codec::types::Vector3::new(20.0, 0.0, 0.0)).length() < 1.0e-9);
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
        let mut polyline = codec::entities::LwPolyline::new();
        polyline.vertices = [(20.0, 0.0), (30.0, 1.0), (30.5, 6.0), (20.0, 6.0)]
            .into_iter()
            .map(|(x, y)| codec::entities::LwVertex::from_coords(x, y))
            .collect();
        polyline.is_closed = true;
        let i = app.active_tab;
        let handle = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::LwPolyline(polyline));
        let corner = |app: &OpenCADStudio, k: usize| {
            let Some(codec::EntityType::LwPolyline(p)) =
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
                Some(codec::EntityType::Line(line)) => (line.start, line.end, line.length()),
                other => panic!("expected a Line, got {other:?}"),
            };
        let direction = codec::types::Vector3::new(3.0, 4.0, 0.0).normalize();

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::whole(handle),
            ),
            direction,
            label: "Horizontal constraint",
        });

        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Line(line)) => line,
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
            Some(codec::EntityType::Line(line)) => line,
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
            Some(codec::EntityType::Line(line)) => line,
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
            direction: codec::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let line = |handle| match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Line(line)) => line,
            other => panic!("expected a Line, got {other:?}"),
        };
        assert_eq!(line(first).start, codec::types::Vector3::ZERO);
        assert!(line(second).start.y.abs() < 1.0e-8);
    }

    #[test]
    fn horizontal_minor_axis_rotates_an_ellipse_around_its_center() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut ellipse = codec::entities::Ellipse::default();
        ellipse.center = codec::types::Vector3::new(3.0, 7.0, 0.0);
        ellipse.major_axis = codec::types::Vector3::new(3.0, 4.0, 0.0);
        ellipse.minor_axis_ratio = 0.4;
        let handle = app.tabs[app.active_tab]
            .scene
            .add_entity(codec::EntityType::Ellipse(ellipse));

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::ellipse_minor_axis(handle),
            ),
            direction: codec::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let ellipse = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Ellipse(ellipse)) => ellipse,
            other => panic!("expected an Ellipse, got {other:?}"),
        };
        assert!((ellipse.center - codec::types::Vector3::new(3.0, 7.0, 0.0)).length() < 1.0e-9);
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

        let new_line = codec::EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::new(5.0, 0.0, 0.0), // exactly on `existing`'s end point
            codec::types::Vector3::new(10.0, 0.0, 0.0),
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
            Some(codec::EntityType::Line(l)) => (l.start, l.end),
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
            Some(codec::EntityType::Line(l)) => (l.start, l.end),
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
            Some(codec::EntityType::Line(line)) => line.length(),
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
        assert_eq!(app.show_constraint_values, start, "second press turns it back");
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
            Some(codec::EntityType::Line(l)) => (l.start, l.end),
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
        let Some(codec::EntityType::Line(first)) =
            app.tabs[app.active_tab].scene.document.get_entity(a)
        else {
            panic!("expected first line");
        };
        let Some(codec::EntityType::Line(second)) =
            app.tabs[app.active_tab].scene.document.get_entity(b)
        else {
            panic!("expected second line");
        };
        assert!((first.end - codec::types::Vector3::new(5.0, 0.0, 0.0)).length() < 1.0e-7);
        assert!((second.start - first.end).length() < 1.0e-7);
        assert!((second.length() - 5.0).abs() < 1.0e-7);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod delobj_tests {
    use super::super::*;
    use codec::types::Vector3;

    fn sweep_source_presence(value: i16, mode: crate::command::ExtrudeMode) -> (bool, bool) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut circle = codec::Circle::new();
        circle.radius = 2.0;
        let profile = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Circle(circle));
        let path = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(codec::Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 5.0),
            )));
        app.delete_objects = value;

        let _ = app.apply_cmd_result(CmdResult::SweepEntities {
            handles: vec![profile],
            path_handle: path,
            mode,
            options: crate::command::SweepOptions::default(),
            color: [1.0; 4],
            expressions: [None, None],
        });

        let created = app.tabs[i]
            .scene
            .document
            .entities()
            .any(|entity| match mode {
                crate::command::ExtrudeMode::Solid => {
                    matches!(entity, codec::EntityType::Solid3D(_))
                }
                crate::command::ExtrudeMode::Surface => {
                    matches!(entity, codec::EntityType::Surface(_))
                }
            });
        assert!(created, "SWEEP must produce the requested result");
        (
            app.tabs[i].scene.document.get_entity(profile).is_some(),
            app.tabs[i].scene.document.get_entity(path).is_some(),
        )
    }

    #[test]
    fn sweep_applies_all_four_source_deletion_modes() {
        let solid_expectations = [(true, true), (false, true), (false, false), (false, false)];
        for (value, expected) in solid_expectations.into_iter().enumerate() {
            assert_eq!(
                sweep_source_presence(value as i16, crate::command::ExtrudeMode::Solid),
                expected,
            );
        }
        // A swept surface is associative by default (SURFACEASSOCIATIVITY 1)
        // and keeps the profile and path it follows, whatever DELOBJ says;
        // DELOBJ reaches surfaces only with associativity off, a profile-wide
        // switch this parallel test leaves alone.
        assert!(crate::scene::model::sweep_model::surface_associativity());
        for value in [2, 3] {
            assert_eq!(
                sweep_source_presence(value, crate::command::ExtrudeMode::Surface),
                (true, true),
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod thicken_tests {
    use super::super::*;
    use codec::entities::{Surface, SurfaceKind};
    use kernel::geom2d::{Circle, Curve, NurbsCurve};
    use kernel::space::{Parameterization, Plane};

    #[test]
    fn thicken_preserves_sources_and_round_trips_results_with_one_undo() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let body = kernel::brep::planar_region(
            Plane::XY,
            &[vec![Curve::Circle(Circle {
                centre: [0.0; 2],
                radius: 3.0,
            })]],
        )
        .unwrap();
        let source = app.add_surface_model(
            codec::EntityType::Surface(Box::new(Surface::new(SurfaceKind::Plane))),
            body,
        );
        assert!(!source.is_null());
        let i = app.active_tab;
        let invalid = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Surface(Box::new(Surface::new(
                SurfaceKind::Generic,
            ))));
        let original =
            serde_json::to_value(app.tabs[i].scene.document.get_entity(source).unwrap()).unwrap();
        let before = app.tabs[i].history.undo_stack.len();
        let _ = app.apply_cmd_result(CmdResult::ThickenEntities {
            handles: vec![source],
            distance: 0.0,
        });
        assert_eq!(app.tabs[i].history.undo_stack.len(), before);
        let _ = app.apply_cmd_result(CmdResult::ThickenEntities {
            handles: vec![invalid, source],
            distance: 2.0,
        });
        assert_eq!(app.tabs[i].history.undo_stack.len(), before + 1);
        let result = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| {
                matches!(entity, codec::EntityType::Solid3D(_)).then_some(entity.common().handle)
            })
            .unwrap();
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(result)
            .is_some());
        assert_eq!(
            serde_json::to_value(app.tabs[i].scene.document.get_entity(source).unwrap()).unwrap(),
            original
        );
        let expected =
            kernel::brep::analytic_mass_properties(&app.tabs[i].scene.solid_models[&result])
                .unwrap()
                .volume;
        let bytes = crate::io::save_to_bytes(
            &app.tabs[i].scene.document,
            "dwg",
            app.tabs[i].scene.document.version,
        )
        .unwrap();
        let document = crate::io::load_bytes("thicken.dwg", bytes).unwrap();
        let mut restored = crate::scene::Scene::new();
        restored.document = document;
        restored.restore_solid_models(&[result]);
        let actual = kernel::brep::analytic_mass_properties(&restored.solid_models[&result])
            .unwrap()
            .volume;
        assert!((expected - actual).abs() < 1e-8);
        app.undo_active_tab();
        assert!(app.tabs[i].scene.document.get_entity(result).is_none());
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(result)
            .is_none());
        assert!(app.tabs[i].scene.document.get_entity(source).is_some());
        assert!(app.tabs[i].scene.document.get_entity(invalid).is_some());
        app.redo_active_tab();
        assert!(app.tabs[i].scene.document.get_entity(result).is_some());
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(result)
            .is_some());
        assert_eq!(
            serde_json::to_value(app.tabs[i].scene.document.get_entity(source).unwrap()).unwrap(),
            original
        );
    }

    #[test]
    fn thicken_creates_a_solid_from_a_free_form_surface() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let profile = NurbsCurve::interpolate(
            &[[0.0, 0.0], [0.7, 0.15], [1.3, -0.1], [2.0, 0.0]],
            None,
            None,
            Parameterization::Chord,
        )
        .unwrap();
        let body =
            kernel::brep::extrude_surface(Plane::XY, &[Curve::Nurbs(profile)], [0.0, 0.0, 2.0])
                .unwrap();
        let source = app.add_surface_model(
            codec::EntityType::Surface(Box::new(Surface::new(SurfaceKind::Generic))),
            body,
        );
        let i = app.active_tab;

        let _ = app.apply_cmd_result(CmdResult::ThickenEntities {
            handles: vec![source],
            distance: 0.15,
        });

        let solids = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                matches!(entity, codec::EntityType::Solid3D(_)).then_some(entity.common().handle)
            })
            .collect::<Vec<_>>();
        assert_eq!(solids.len(), 1);
        let solid = &app.tabs[i].scene.solid_models[&solids[0]];
        assert!(solid.validate().is_empty());
        assert!(solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2));
        assert!(app.tabs[i]
            .scene
            .document
            .solid_history_operation(solids[0])
            .is_some());
        assert!(app.tabs[i].scene.document.get_entity(source).is_some());
    }
}

#[cfg(test)]
mod dispatched_command_task_tests {
    use super::super::*;
    use crate::command::CmdResult;
    use codec::entities::ViewportRenderMode as Mode;

    /// `CmdResult::Dispatch` and `CmdResult::Relaunch` run the assembled line
    /// through `dispatch_command`, and the Task it returns must reach the
    /// runtime: the render mode a VSCURRENT keyword picker chooses is applied
    /// by a message that Task carries.
    #[test]
    fn dispatch_and_relaunch_keep_the_task_the_command_returns() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        app.tabs[i].render_mode = Mode::Wireframe2D;
        let task = app.apply_cmd_result(CmdResult::Dispatch("VSCURRENT GOURAUDSHADED".into()));
        app.drive_headless_task(task).unwrap();
        assert_eq!(
            app.tabs[i].render_mode,
            Mode::GouraudShaded,
            "Dispatch dropped the dispatched command's task"
        );

        app.tabs[i].render_mode = Mode::Wireframe2D;
        let task = app.apply_cmd_result(CmdResult::Relaunch(
            "VSCURRENT GOURAUDSHADED".into(),
            Vec::new(),
        ));
        app.drive_headless_task(task).unwrap();
        assert_eq!(
            app.tabs[i].render_mode,
            Mode::GouraudShaded,
            "Relaunch dropped the dispatched command's task"
        );
    }

    #[test]
    fn test_dispatch_move_with_handle_enter_and_points() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let handle = app.tabs[i].scene.add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::new(10.0, 20.0, 0.0),
            codec::types::Vector3::new(30.0, 40.0, 0.0),
        )));

        // Unselected entity - AutoLISP (command "_MOVE" ss "" "_NONE" pt "_NONE" '(0 0 0)) produces:
        assert!(app.tabs[i].scene.selected_entities().is_empty());
        let cmd = format!("_MOVE {:X} ENTER _NONE 10,20,0 _NONE 0,0,0", handle.value());
        let task = app.apply_cmd_result(CmdResult::Script(cmd));
        let _ = app.drive_headless_task(task);

        let moved = app.tabs[i].scene.document.get_entity(handle).unwrap();
        if let codec::EntityType::Line(l) = moved {
            assert!((l.start.x - 0.0).abs() < 1e-4, "Expected start.x == 0, got {}", l.start.x);
            assert!((l.start.y - 0.0).abs() < 1e-4, "Expected start.y == 0, got {}", l.start.y);
            assert!((l.end.x - 20.0).abs() < 1e-4, "Expected end.x == 20, got {}", l.end.x);
            assert!((l.end.y - 20.0).abs() < 1e-4, "Expected end.y == 20, got {}", l.end.y);
        } else {
            panic!("Expected line entity");
        }
    }

    #[test]
    fn test_preselection_move_with_base_and_dest_points() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let handle = app.tabs[i].scene.add_entity(codec::EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::new(10.0, 20.0, 0.0),
            codec::types::Vector3::new(30.0, 40.0, 0.0),
        )));

        // User pre-selects the entity in the GUI
        app.tabs[i].scene.select_entity(handle, true);
        assert_eq!(app.tabs[i].scene.selected_entities().len(), 1);

        // Pre-selection MOVE command only takes base and destination points
        let cmd = "_MOVE _NONE 10,20,0 _NONE 0,0,0";
        let task = app.apply_cmd_result(CmdResult::Script(cmd.to_string()));
        let _ = app.drive_headless_task(task);

        let moved = app.tabs[i].scene.document.get_entity(handle).unwrap();
        if let codec::EntityType::Line(l) = moved {
            assert!((l.start.x - 0.0).abs() < 1e-4, "Expected start.x == 0, got {}", l.start.x);
            assert!((l.start.y - 0.0).abs() < 1e-4, "Expected start.y == 0, got {}", l.start.y);
            assert!((l.end.x - 20.0).abs() < 1e-4, "Expected end.x == 20, got {}", l.end.x);
            assert!((l.end.y - 20.0).abs() < 1e-4, "Expected end.y == 20, got {}", l.end.y);
        } else {
            panic!("Expected line entity");
        }
    }
}

/// PW-1 BEDIT characterization tests.
///
/// Two groups:
/// - `locked_*`: behavior Phase 1 must preserve (save keeps edits, discard
///   restores the definition, refusals stay refused).
/// - `documenting_*`: today's questionable behavior, pinned so the refactor
///   cannot change it silently. Each one is expected to flip in Phase 1.
#[cfg(test)]
mod bedit_characterization_tests {
    use super::super::*;
    use codec::entities::{Circle, Insert, Line};
    use codec::types::Vector3;
    use codec::EntityType;

    /// Fresh document with one block definition `BLK` (one line) and one
    /// model-space INSERT referencing it. Returns the INSERT handle and the
    /// definition's member handles.
    fn setup_block_and_insert(app: &mut OpenCADStudio) -> (Handle, Vec<Handle>) {
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        app.tabs[tab].scene.define_block_raw(
            "BLK",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let insert = app.tabs[tab]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "BLK",
                Vector3::new(100.0, 0.0, 0.0),
            )))
            .expect("insert");
        let members = block_members(app, "BLK");
        (insert, members)
    }

    fn block_members(app: &OpenCADStudio, name: &str) -> Vec<Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get(name)
            .map(|br| br.entity_handles.clone())
            .unwrap_or_default()
    }

    /// Definition state as (handle, Debug form) in member order. The Debug
    /// form stands in for PartialEq, which EntityType does not provide.
    fn definition_state(app: &OpenCADStudio, name: &str) -> Vec<(Handle, String)> {
        let tab = app.active_tab;
        block_members(app, name)
            .into_iter()
            .map(|h| {
                let e = app.tabs[tab].scene.document.get_entity(h);
                (h, format!("{e:?}"))
            })
            .collect()
    }

    fn undo_depth(app: &OpenCADStudio) -> usize {
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    fn begin_bedit(app: &mut OpenCADStudio, insert: Handle) {
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[app.active_tab].active_block_edit.is_some(),
            "setup: BEDIT_BEGIN should open a block-edit session"
        );
    }

    fn add_circle_in_bedit(app: &mut OpenCADStudio) -> Handle {
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .add_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)))
    }

    // ---- locked: must survive Phase 1 ----

    /// Locked: save keeps the entity added inside the session.
    #[test]
    fn locked_save_keeps_entity_added_in_session() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        begin_bedit(&mut app, insert);
        add_circle_in_bedit(&mut app);
        let _ = app.dispatch_command("BEDIT_SAVE");

        assert!(app.tabs[app.active_tab].active_block_edit.is_none());
        assert_eq!(
            block_members(&app, "BLK").len(),
            2,
            "save keeps the new entity"
        );
    }

    /// Locked: discard restores the definition to its entry state exactly
    /// (same handles, same geometry, same order).
    #[test]
    fn locked_discard_restores_definition_to_entry_state() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let entry = definition_state(&app, "BLK");
        begin_bedit(&mut app, insert);
        add_circle_in_bedit(&mut app);
        let _ = app.dispatch_command("BEDIT_DISCARD");

        assert!(app.tabs[app.active_tab].active_block_edit.is_none());
        assert_eq!(definition_state(&app, "BLK"), entry);
    }

    /// Locked: refusals. A non-INSERT is rejected and no session opens.
    #[test]
    fn locked_bedit_refuses_non_insert_object() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let line = app.tabs[tab]
            .scene
            .document
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )))
            .expect("line");
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", line.value()));
        assert!(app.tabs[tab].active_block_edit.is_none());
        assert!(app.tabs[tab].block_edits.is_empty());
    }

    /// Locked: refusal. The `BEDIT` picker command is rejected while a
    /// REFEDIT session is active and no BEDIT session opens.
    #[test]
    fn locked_bedit_picker_refused_while_refedit_active() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[tab].refedit_session.is_some(),
            "setup: REFEDIT_BEGIN should open a REFEDIT session"
        );
        let _ = app.dispatch_command("BEDIT");
        assert!(app.tabs[tab].active_block_edit.is_none());
        assert!(app.tabs[tab].block_edits.is_empty());
    }

    /// Every entry point must refuse while REFEDIT is active.
    #[test]
    fn locked_bedit_begin_refuses_active_refedit() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", insert.value()));
        assert!(app.tabs[tab].refedit_session.is_some());
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[tab].active_block_edit.is_none(),
            "begin must refuse while REFEDIT is active"
        );
    }

    /// Locked: refusal. An xref definition cannot be edited.
    #[test]
    fn locked_bedit_refuses_xref_definition() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .document
            .block_records
            .get_mut("BLK")
            .expect("block record")
            .flags
            .is_xref = true;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(app.tabs[tab].active_block_edit.is_none());
        assert!(app.tabs[tab].block_edits.is_empty());
    }

    /// V7 (reference check): while a block editor session is open this
    /// command set is refused outright with "not allowed in block editor";
    /// the refusal pushes no undo entry and starts no command.
    #[test]
    fn locked_deny_list_refuses_while_session_open() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(
            app.tabs[tab].active_block_edit.is_some(),
            "setup: BEDIT_BEGIN should open a session"
        );
        let undo_depth = app.tabs[tab].history.undo_stack.len();
        for verb in [
            "LAYOUT",
            "VPORTS",
            "MVIEW",
            "WBLOCK",
            "PURGE",
            "RENAME",
            "REFEDIT",
            "REFEDIT_BEGIN",
            "PLOT",
        ] {
            let revision = app.command_line.error_revision;
            let _ = app.dispatch_command(verb);
            assert_eq!(
                app.command_line.error_revision,
                revision.wrapping_add(1),
                "{verb} must refuse with a command-line error"
            );
            assert!(
                app.command_line
                    .last_error
                    .as_deref()
                    .unwrap_or_default()
                    .contains("command not allowed in block editor"),
                "{verb}: got {:?}",
                app.command_line.last_error
            );
            assert!(
                app.tabs[tab].active_block_edit.is_some(),
                "{verb} must keep the session open"
            );
            assert!(
                app.tabs[tab].active_cmd.is_none(),
                "{verb} must not install a command"
            );
            assert_eq!(
                app.tabs[tab].history.undo_stack.len(),
                undo_depth,
                "{verb} must push no undo entry"
            );
        }
    }

    /// Spec 8: a rejected command must not cancel a running command — the
    /// deny hook sits before the running-command teardown.
    #[test]
    fn locked_deny_list_does_not_cancel_a_running_command() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        assert!(app.tabs[tab].active_block_edit.is_some());
        let _ = app.dispatch_command("LINE");
        assert_eq!(
            app.tabs[tab].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE"),
            "setup: LINE should be running inside the session"
        );
        let _ = app.dispatch_command("LAYOUT");
        assert!(app
            .command_line
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("command not allowed in block editor"));
        assert_eq!(
            app.tabs[tab].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE"),
            "the refused command must leave the running LINE untouched"
        );
    }

    /// V7: the reference keeps XREF, NEW, OPEN, SAVEAS and BEDIT available
    /// inside the editor — the deny list must not touch them.
    #[test]
    fn locked_deny_list_leaves_v7_allowed_commands_alone() {
        for verb in ["XREF", "NEW", "OPEN", "SAVEAS", "BEDIT"] {
            let mut app = OpenCADStudio::new_for_test();
            let (insert, _) = setup_block_and_insert(&mut app);
            let tab = app.active_tab;
            let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
            assert!(app.tabs[tab].active_block_edit.is_some());
            let _ = app.dispatch_command(verb);
            assert!(
                !app.command_line
                    .last_error
                    .as_deref()
                    .unwrap_or_default()
                    .contains("not allowed in block editor"),
                "{verb} is available in the reference session and must not be denied: {:?}",
                app.command_line.last_error
            );
        }
    }

    // ---- documenting: today's behavior, expected to flip in Phase 1 ----

    /// Discard removes local before-images before drawing undo resumes.
    #[test]
    fn locked_modify_discard_then_undo_does_not_restore_local_edits() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, members) = setup_block_and_insert(&mut app);
        let line = members[0];
        let entry = definition_state(&app, "BLK");
        begin_bedit(&mut app, insert);

        let tab = app.active_tab;
        // Record the undo step before the mutation so the recorder captures it.
        app.push_undo_snapshot(tab, "modify inside BEDIT");
        app.tabs[tab]
            .scene
            .document
            .get_entity_mut(line)
            .map(|e| {
                if let EntityType::Line(l) = e {
                    l.end = Vector3::new(99.0, 0.0, 0.0);
                }
            })
            .expect("line in definition");
        app.finish_pending_history(tab);

        let _ = app.dispatch_command("BEDIT_DISCARD");
        app.finish_pending_history(tab);
        assert_eq!(
            definition_state(&app, "BLK"),
            entry,
            "discard restores entry"
        );

        app.undo_steps(1);
        // Pinned: the state after one undo, observed on current code.
        assert_eq!(
            definition_state(&app, "BLK"),
            entry,
            "documenting: undo after discard restores the entry state (ISS-02 not reproduced here)"
        );
    }

    /// Local undo restores both the entity store and block membership.
    #[test]
    fn locked_session_undo_restores_membership_without_crossing_entry() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        begin_bedit(&mut app, insert);
        let tab = app.active_tab;
        app.push_undo_snapshot(tab, "add circle in BEDIT");
        add_circle_in_bedit(&mut app);
        app.finish_pending_history(tab);
        assert_eq!(block_members(&app, "BLK").len(), 2);

        app.undo_steps(1);
        let members = block_members(&app, "BLK");
        assert_eq!(
            members.len(),
            1,
            "block membership must match the undone entity store"
        );
        let dangling = members
            .iter()
            .filter(|h| app.tabs[tab].scene.document.get_entity(**h).is_none())
            .count();
        assert_eq!(dangling, 0, "every member must have an entity");
        assert!(
            app.tabs[tab].active_block_edit.is_some(),
            "documenting: undo does not close the session"
        );
    }

    /// Opening starts an empty local history; undo cannot cross entry.
    #[test]
    fn locked_session_entry_has_no_undo_step() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        begin_bedit(&mut app, insert);
        app.finish_pending_history(tab);
        let depth_after_begin = undo_depth(&app);
        assert_eq!(depth_after_begin, 0, "opening must not record an undo step");

        app.undo_steps(1);
        assert!(
            app.tabs[tab].active_block_edit.is_some(),
            "documenting: undoing BEDIT_BEGIN does not close the open session"
        );
    }

    /// Switching a clean block replaces the single session.
    #[test]
    fn locked_clean_switch_replaces_the_session() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        let tab = app.active_tab;
        app.tabs[tab].scene.define_block_raw(
            "OTHER",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(5.0, 0.0, 0.0),
            ))],
        );
        let other_insert = app.tabs[tab]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "OTHER",
                Vector3::new(200.0, 0.0, 0.0),
            )))
            .expect("insert");
        begin_bedit(&mut app, insert);
        begin_bedit(&mut app, other_insert);
        assert_eq!(
            app.tabs[tab].block_edits.len(),
            1,
            "a drawing has only one block-editor session"
        );
    }

    /// No-change Save and Discard leave the drawing history untouched.
    #[test]
    fn locked_clean_save_and_discard_leave_history_unchanged() {
        let mut app = OpenCADStudio::new_for_test();
        let (insert, _) = setup_block_and_insert(&mut app);
        begin_bedit(&mut app, insert);
        app.finish_pending_history(app.active_tab);
        let after_begin = undo_depth(&app);

        let _ = app.dispatch_command("BEDIT_SAVE");
        app.finish_pending_history(app.active_tab);
        assert_eq!(undo_depth(&app), after_begin, "save pushes no undo entry");

        begin_bedit(&mut app, insert);
        app.finish_pending_history(app.active_tab);
        let before_discard = undo_depth(&app);
        let _ = app.dispatch_command("BEDIT_DISCARD");
        app.finish_pending_history(app.active_tab);
        assert_eq!(
            undo_depth(&app),
            before_discard,
            "discard pushes no undo entry (not undoable)"
        );
    }

    #[test]
    fn test_locked_layer_selection_rejection() {
        use crate::command::{CadCommand, CmdResult};

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;

        // Create line1 on layer "0" (unlocked)
        let h_unlocked = app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));

        // Create line2 on layer "LOCKED_LAYER" and lock it
        let mut line2 = Line::from_points(
            Vector3::new(0.0, 10.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        );
        line2.common.layer = "LOCKED_LAYER".to_string();
        let h_locked = app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(line2));

        app.tabs[tab]
            .scene
            .document
            .layers
            .get_mut("LOCKED_LAYER")
            .expect("LOCKED_LAYER must exist")
            .flags
            .locked = true;

        assert!(!app.tabs[tab].scene.is_layer_locked(h_unlocked));
        assert!(app.tabs[tab].scene.is_layer_locked(h_locked));

        struct MockCmd {
            rejects: bool,
        }

        impl CadCommand for MockCmd {
            fn name(&self) -> &'static str {
                "MOCK_REJECT"
            }
            fn prompt(&self) -> String {
                "Select objects: ".into()
            }
            fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
                CmdResult::Done
            }
            fn on_enter(&mut self) -> CmdResult {
                CmdResult::Done
            }
            fn selection_rejects_locked_layers(&self) -> bool {
                self.rejects
            }
        }

        // Test 1: Command with rejects = true drops locked handles and reports count
        app.tabs[tab].active_cmd = Some(Box::new(MockCmd { rejects: true }));

        let mut handles = vec![h_unlocked, h_locked];
        app.filter_handles_reject_locked(tab, &mut handles);
        assert_eq!(handles, vec![h_unlocked]);
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("1 was on a locked layer."));

        // Test 2: Command with rejects = false keeps all handles
        app.tabs[tab].active_cmd = Some(Box::new(MockCmd { rejects: false }));

        let mut handles2 = vec![h_unlocked, h_locked];
        app.filter_handles_reject_locked(tab, &mut handles2);
        assert_eq!(handles2, vec![h_unlocked, h_locked]);

        // Test 3: filter_locked_layer_selection drops locked handles from SelectionComplete
        app.tabs[tab].active_cmd = Some(Box::new(MockCmd { rejects: true }));
        let input = StepInput::SelectionComplete(vec![h_unlocked, h_locked]);
        let filtered = app.filter_locked_layer_selection(input);
        match filtered {
            StepInput::SelectionComplete(res) => assert_eq!(res, vec![h_unlocked]),
            _ => panic!("Expected SelectionComplete"),
        }
    }
}

#[cfg(test)]
mod block_base_point_tests {
    use super::super::*;
    use crate::command::{CadCommand, CmdResult};
    use crate::modules::insert::insert_block::InsertBlockCommand;
    use codec::entities::AttributeDefinition;
    use codec::types::Vector3;
    use codec::EntityType;

    /// base-point fix (A), host side: `AttreqNeeded` is where the host resolves the
    /// block's ATTDEFs, so it also injects the definition's base point. Without
    /// that the committed attribute lands on the codec transform's position.
    #[test]
    fn locked_attreq_injects_the_block_base_point_for_attribute_placement() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        // These tests follow the prompt chain, so the insertion form that
        // answers it in one go has to stay out of the way.
        app.tabs[i].scene.document.header.attribute_dialog = false;
        // Block "ATTREQ_BP": base (10,5,0); LINE (10,5)-(30,5); ATTDEF TAG1 at
        // (12,8). Rendering convention (see scene::block_base_point_tests): the
        // ATTDEF lands at (100,100) + Rot(30)*Scale(2)*((12,8) - (10,5))
        //   = (100,100) + 2*(1.732051 - 1.5, 1 + 2.598076) = (100.464102, 107.196152).
        app.tabs[i].scene.define_block_raw(
            "ATTREQ_BP",
            Vector3::new(10.0, 5.0, 0.0),
            vec![
                EntityType::Line(codec::entities::Line::from_points(
                    Vector3::new(10.0, 5.0, 0.0),
                    Vector3::new(30.0, 5.0, 0.0),
                )),
                EntityType::AttributeDefinition(AttributeDefinition {
                    insertion_point: Vector3::new(12.0, 8.0, 0.0),
                    height: 1.0,
                    ..AttributeDefinition::new(
                        "TAG1".to_string(),
                        "Tag one".to_string(),
                        String::new(),
                    )
                }),
            ],
        );

        let mut cmd = InsertBlockCommand::classic_for_block(
            "ATTREQ_BP".to_string(),
            Vec::new(),
            String::new(),
        );
        let _ = cmd.on_point(glam::DVec3::new(100.0, 100.0, 0.0));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
        assert!(matches!(
            cmd.on_text_input("30"),
            Some(CmdResult::AttreqNeeded { .. })
        ));
        app.tabs[i].active_cmd = Some(Box::new(cmd));

        let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
            block_name: "ATTREQ_BP".to_string(),
        });
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("the INSERT command waits for the attribute value")
            .on_text_input("VALUE1")
            .expect("the value finishes the INSERT");
        let _ = app.apply_cmd_result(result);

        let attribute = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(ins) if ins.block_name == "ATTREQ_BP" => {
                    ins.attributes.first().cloned()
                }
                _ => None,
            })
            .expect("the committed INSERT's attribute");
        assert_eq!(attribute.tag, "TAG1");
        let point = attribute.insertion_point;
        assert!(
            (point.x - 100.464102).abs() < 1e-6
                && (point.y - 107.196152).abs() < 1e-6
                && point.z.abs() < 1e-9,
            "attribute at ({:.6},{:.6},{:.6}), expected (100.464102,107.196152,0)",
            point.x,
            point.y,
            point.z
        );
    }

    /// An attribute placed through the command line is an object in its own
    /// right: a handle of its own, the reference as its owner and a terminator,
    /// so that the saved file carries it and the reference stays attributed.
    #[test]
    fn locked_attreq_attributes_are_written_as_objects_of_their_own() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.header.attribute_dialog = false;
        app.tabs[i].scene.define_block_raw(
            "ATTREQ_ID",
            Vector3::ZERO,
            vec![
                EntityType::Line(codec::entities::Line::from_points(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(20.0, 0.0, 0.0),
                )),
                EntityType::AttributeDefinition(AttributeDefinition {
                    insertion_point: Vector3::new(1.0, 2.0, 0.0),
                    height: 1.0,
                    ..AttributeDefinition::new(
                        "TAG1".to_string(),
                        "Tag one".to_string(),
                        "100".to_string(),
                    )
                }),
            ],
        );

        let mut cmd = InsertBlockCommand::classic_for_block(
            "ATTREQ_ID".to_string(),
            Vec::new(),
            String::new(),
        );
        let _ = cmd.on_point(glam::DVec3::new(0.0, 0.0, 0.0));
        let _ = cmd.on_text_input("2");
        let _ = cmd.on_text_input("2");
        assert!(matches!(
            cmd.on_text_input("0"),
            Some(CmdResult::AttreqNeeded { .. })
        ));
        app.tabs[i].active_cmd = Some(Box::new(cmd));
        let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
            block_name: "ATTREQ_ID".to_string(),
        });
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("the INSERT command waits for the attribute value")
            .on_text_input("VALUE1")
            .expect("the value finishes the INSERT");
        let _ = app.apply_cmd_result(result);

        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(candidate) if candidate.block_name == "ATTREQ_ID" => {
                    Some(candidate.clone())
                }
                _ => None,
            })
            .expect("the placed reference");
        assert_eq!(insert.attributes.len(), 1);
        let attribute = &insert.attributes[0];
        assert!(
            !attribute.common.handle.is_null(),
            "the attribute is an object of its own, so it needs a handle"
        );
        assert_eq!(
            attribute.common.owner_handle, insert.common.handle,
            "the attribute belongs to the reference it was placed on"
        );
        assert!(
            insert.seqend_handle.is_some(),
            "the attributes need a terminator"
        );

        for ext in ["dxf", "dwg"] {
            let bytes = crate::io::save_to_bytes(
                &app.tabs[i].scene.document,
                ext,
                app.tabs[i].scene.document.version,
            )
            .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let read = crate::io::load_bytes(&format!("attreq.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("load {ext}: {error}"));
            let reloaded = read
                .entities()
                .find_map(|entity| match entity {
                    EntityType::Insert(candidate) if candidate.block_name == "ATTREQ_ID" => {
                        Some(candidate.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{ext}: the reference was not written"));
            assert_eq!(
                reloaded.attributes.len(),
                1,
                "{ext}: the reference was written without its attribute"
            );
            assert_eq!(reloaded.attributes[0].tag, "TAG1", "{ext}: the tag");
            assert_eq!(
                reloaded.attributes[0].value, "VALUE1",
                "{ext}: the value entered at placement"
            );
        }
    }

    /// A constant attribute is never asked for, but it still belongs to the
    /// reference: the value the definition carries is the value the reference
    /// shows.
    #[test]
    fn locked_attreq_places_a_constant_attribute_without_asking() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut attdef =
            AttributeDefinition::new("TAG1".to_string(), "Tag one".to_string(), "42".to_string());
        attdef.flags.constant = true;
        app.tabs[i].scene.define_block_raw(
            "ATTREQ_CONST",
            Vector3::ZERO,
            vec![
                EntityType::Line(codec::entities::Line::from_points(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(20.0, 0.0, 0.0),
                )),
                EntityType::AttributeDefinition(attdef),
            ],
        );

        let mut cmd = InsertBlockCommand::classic_for_block(
            "ATTREQ_CONST".to_string(),
            Vec::new(),
            String::new(),
        );
        let _ = cmd.on_point(glam::DVec3::new(0.0, 0.0, 0.0));
        let _ = cmd.on_text_input("2");
        let _ = cmd.on_text_input("2");
        assert!(matches!(
            cmd.on_text_input("0"),
            Some(CmdResult::AttreqNeeded { .. })
        ));
        app.tabs[i].active_cmd = Some(Box::new(cmd));
        let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
            block_name: "ATTREQ_CONST".to_string(),
        });
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "a constant attribute asks nothing, so the placement is finished"
        );

        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(candidate) if candidate.block_name == "ATTREQ_CONST" => {
                    Some(candidate.clone())
                }
                _ => None,
            })
            .expect("the placed reference");
        assert_eq!(
            insert.attributes.len(),
            1,
            "the constant attribute belongs to the reference even though nothing was asked"
        );
        assert_eq!(insert.attributes[0].value, "42");
    }

    /// base-point fix (D), host side: picking an existing block in the Block
    /// Definition dialog prefills the base point from that definition.
    #[test]
    fn locked_block_def_name_select_prefills_the_definition_base_point() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "DEF_BASE",
            Vector3::new(10.0, 5.0, 0.0),
            vec![EntityType::Line(codec::entities::Line::from_points(
                Vector3::new(10.0, 5.0, 0.0),
                Vector3::new(30.0, 5.0, 0.0),
            ))],
        );
        app.block_definition = Some(
            crate::ui::window::block_definition::BlockDefinitionState::new(
                vec!["DEF_BASE".to_string()],
                Vec::new(),
                0,
                4,
            ),
        );
        assert_eq!(
            app.block_definition
                .as_ref()
                .map(|s| s.base_point_y.as_str()),
            Some("0.0000"),
            "a new block starts at 0"
        );

        let _ = app.update(Message::BlockDefNameSelect("DEF_BASE".to_string()));

        let state = app.block_definition.as_ref().expect("the dialog is open");
        assert_eq!(state.name, "DEF_BASE");
        assert_eq!(state.base_point_x, "10.0000");
        assert_eq!(state.base_point_y, "5.0000");
        assert_eq!(state.base_point_z, "0.0000");
        assert_eq!(state.parse_base_point(), glam::DVec3::new(10.0, 5.0, 0.0));
    }

    /// A block with base (10,5,0) and LINE (10,5)-(30,5), referenced by an
    /// INSERT at (100,100) with 30° rotation and uniform scale 2. Under the
    /// convention the LINE renders at (100,100)-(134.641016,120).
    fn block_and_instance_with_base(app: &mut OpenCADStudio, name: &str) -> codec::Handle {
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            name,
            Vector3::new(10.0, 5.0, 0.0),
            vec![EntityType::Line(codec::entities::Line::from_points(
                Vector3::new(10.0, 5.0, 0.0),
                Vector3::new(30.0, 5.0, 0.0),
            ))],
        );
        let mut insert =
            codec::entities::Insert::new(name.to_string(), Vector3::new(100.0, 100.0, 0.0));
        insert.set_x_scale(2.0);
        insert.set_y_scale(2.0);
        insert.set_z_scale(2.0);
        insert.rotation = std::f64::consts::PI / 6.0;
        app.tabs[i].scene.add_entity(EntityType::Insert(insert))
    }

    /// base-point follow-up fix: the REFEDIT working copy is placed through the
    /// base-point-aware INSERT transform, so it sits exactly on the rendered
    /// reference, and saving folds it back to the same block-local geometry.
    #[test]
    fn locked_refedit_working_copy_sits_on_the_rendered_reference() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "REFEDIT_BP");
        let i = app.active_tab;

        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", handle.value()));
        assert!(
            app.tabs[i].refedit_session.is_some(),
            "setup: REFEDIT_BEGIN opens a session"
        );

        // The working copy is the LINE the session moved into model space.
        let temp: Vec<codec::Handle> = app.tabs[i]
            .refedit_session
            .as_ref()
            .expect("the session")
            .temp_handles
            .clone();
        let line = temp
            .iter()
            .find_map(|h| match app.tabs[i].scene.document.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line.clone()),
                _ => None,
            })
            .expect("the working-copy LINE");
        // (100,100) + Rot(30)*2*((10,5)-(10,5)) and ...*((30,5)-(10,5)).
        assert!(
            (line.start.x - 100.0).abs() < 1e-6
                && (line.start.y - 100.0).abs() < 1e-9
                && (line.end.x - 134.641016).abs() < 1e-6
                && (line.end.y - 120.0).abs() < 1e-6,
            "working copy at ({:.6},{:.6})-({:.6},{:.6}), expected (100,100)-(134.641016,120)",
            line.start.x,
            line.start.y,
            line.end.x,
            line.end.y
        );

        // Saving without editing restores the definition verbatim: the
        // inverse is the exact inverse of the forward transform.
        let _ = app.dispatch_command("REFCLOSE_SAVE");
        assert!(app.tabs[i].refedit_session.is_none());
        let stored = app.tabs[i]
            .scene
            .document
            .block_records
            .get("REFEDIT_BP")
            .expect("the block record")
            .entity_handles
            .iter()
            .find_map(|h| match app.tabs[i].scene.document.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line.clone()),
                _ => None,
            })
            .expect("the stored LINE");
        assert!(
            (stored.start.x - 10.0).abs() < 1e-9
                && (stored.start.y - 5.0).abs() < 1e-9
                && (stored.end.x - 30.0).abs() < 1e-9
                && (stored.end.y - 5.0).abs() < 1e-9,
            "stored LINE at ({:.9},{:.9})-({:.9},{:.9}), expected (10,5)-(30,5)",
            stored.start.x,
            stored.start.y,
            stored.end.x,
            stored.end.y
        );
        assert_eq!(
            crate::scene::render_graph::block_base_point(&app.tabs[i].scene.document, "REFEDIT_BP"),
            Vector3::new(10.0, 5.0, 0.0),
            "the base point survives the round trip"
        );
    }

    /// The definition body is replaced on save, so the terminator has to move
    /// after the new body.
    #[test]
    fn a_refedit_save_ends_the_definition_after_its_body() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "REFEDIT_ORDER");
        let i = app.active_tab;
        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", handle.value()));
        assert!(
            app.tabs[i].refedit_session.is_some(),
            "setup: REFEDIT_BEGIN opens a session"
        );
        let _ = app.dispatch_command("REFCLOSE_SAVE");
        assert!(app.tabs[i].refedit_session.is_none());
        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .get("REFEDIT_ORDER")
            .expect("the block record");
        let marker = record.block_entity_handle.value();
        let end = record.block_end_handle.value();
        let body: Vec<_> = record
            .entity_handles
            .iter()
            .map(|h| h.value())
            .collect();
        assert!(
            body.iter().all(|h| *h > marker),
            "body handles follow the BLOCK marker: {body:?}"
        );
        assert!(
            end > *body.iter().max().expect("the body is not empty"),
            "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
        );
        assert!(
            matches!(
                app.tabs[i].scene.document.get_entity(codec::Handle::new(end)),
                Some(EntityType::BlockEnd(_))
            ),
            "the record's end handle names an ENDBLK entity"
        );
    }

    /// base-point follow-up fix: the dynamic-block visibility grip is a
    /// block-definition coordinate, so it follows the base-point convention
    /// too: (100,100) + Rot(30)*2*((12,3) - (10,5)) = (105.464102, 98.535898).
    #[test]
    fn locked_visibility_grip_follows_the_base_point_convention() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "VIS_BP");
        let i = app.active_tab;

        let def_block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("VIS_BP")
            .expect("the block record")
            .handle;
        let mut param = codec::objects::BlockVisibilityParameter::default();
        param.owner = def_block;
        param.def_point = Vector3::new(12.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .document
            .block_visibility_params
            .insert(codec::Handle::new(0x70), param);
        app.tabs[i].selected_handle = Some(handle);

        app.refresh_visibility_grip([0.0, 0.0, 0.0]);

        let grip = app.tabs[i]
            .selected_grips
            .last()
            .expect("the visibility lookup grip");
        assert!(
            (grip.world.x - 105.464102).abs() < 1e-6 && (grip.world.y - 98.535898).abs() < 1e-6,
            "grip at ({:.6},{:.6}), expected (105.464102,98.535898)",
            grip.world.x,
            grip.world.y
        );
        assert!(
            app.tabs[i].visibility_grip.is_some(),
            "the selection is a dynamic-block reference"
        );
    }

    /// The visibility grip opens a list, so its triangle points down like a
    /// list arrow, even on a rotated reference (the fixture turns it 30 deg).
    #[test]
    fn visibility_grip_points_down() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "VIS_DOWN");
        let i = app.active_tab;
        let def_block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("VIS_DOWN")
            .expect("the block record")
            .handle;
        let mut param = codec::objects::BlockVisibilityParameter::default();
        param.owner = def_block;
        param.def_point = Vector3::new(12.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .document
            .block_visibility_params
            .insert(codec::Handle::new(0x70), param);
        app.tabs[i].selected_handle = Some(handle);

        app.refresh_visibility_grip([0.0, 0.0, 0.0]);

        let grip = app.tabs[i]
            .selected_grips
            .last()
            .expect("the visibility lookup grip");
        assert_eq!(grip.dir, Some(glam::DVec3::NEG_Y));
    }
}

#[cfg(test)]
mod sweep_path_end_tests {
    use crate::app::OpenCADStudio;
    use crate::command::CmdResult;
    use codec::entities::{LwPolyline, LwVertex};
    use codec::types::Vector2;

    /// Picked near its far end, a path is swept from that end. A path whose
    /// start lies farther from the world origin than its end used to fail.
    #[test]
    fn sweep_from_the_picked_end_of_a_bent_path() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut hexagon = LwPolyline::new();
        for k in 0..6 {
            let a = (60.0 * k as f64).to_radians();
            hexagon.vertices.push(LwVertex::new(Vector2::new(5.0 * a.cos(), 5.0 * a.sin())));
        }
        hexagon.is_closed = true;
        let profile = app.tabs[i].scene.add_entity(codec::EntityType::LwPolyline(hexagon));
        let mut path = LwPolyline::new();
        path.vertices.push(LwVertex::new(Vector2::new(20.0, 0.0)));
        path.vertices.push(LwVertex::with_bulge(
            Vector2::new(60.0, 0.0),
            22.5f64.to_radians().tan(),
        ));
        path.vertices.push(LwVertex::new(Vector2::new(80.0, 20.0)));
        path.vertices.push(LwVertex::new(Vector2::new(80.0, 60.0)));
        let path = app.tabs[i].scene.add_entity(codec::EntityType::LwPolyline(path));
        let options = crate::command::SweepOptions {
            path_pick: Some(glam::DVec3::new(80.0, 40.0, 0.0)),
            ..crate::command::SweepOptions::default()
        };
        let _ = app.apply_cmd_result(CmdResult::SweepEntities {
            handles: vec![profile],
            path_handle: path,
            mode: crate::command::ExtrudeMode::Solid,
            options,
            color: [1.0; 4],
            expressions: [None, None],
        });
        let swept = app.tabs[i]
            .scene
            .document
            .entities()
            .any(|entity| matches!(entity, codec::EntityType::Solid3D(_)));
        assert!(swept, "the sweep from the picked end makes a solid");
    }
}

#[cfg(test)]
mod copytolayer_tests {
    use super::super::*;
    use codec::entities::{Circle, Line};
    use codec::types::Vector3;
    use glam::DVec3;

    fn fixture() -> (OpenCADStudio, Handle, Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let scene = &mut app.tabs[app.active_tab].scene;
        scene
            .document
            .layers
            .add(codec::tables::Layer::new("RED"))
            .unwrap();
        scene
            .document
            .layers
            .add(codec::tables::Layer::new("BLUE"))
            .unwrap();
        let mut circle = Circle::from_coords(1.0, 2.0, 0.0, 2.0);
        circle.common.layer = "RED".into();
        let source = scene.add_entity(codec::EntityType::Circle(circle));
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
        line.common.layer = "BLUE".into();
        let dest = scene.add_entity(codec::EntityType::Line(line));
        (app, source, dest)
    }

    // Like viewport picks / boxes: selection accumulates, and the command receives
    // the WHOLE scene selection, never just the handle picked this time.
    fn pick(app: &mut OpenCADStudio, handles: &[Handle]) {
        let scene = &mut app.tabs[app.active_tab].scene;
        for h in handles {
            scene.select_entity(*h, false);
        }
        selection_changed(app);
    }

    fn selection_changed(app: &mut OpenCADStudio) {
        let handles = app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        feed(app, StepInput::SelectionComplete(handles));
    }

    fn feed(app: &mut OpenCADStudio, input: StepInput) {
        let _ = app.feed_command(input);
        // Match update(): close history at every GUI message boundary unless
        // the running command groups it across the copy / placement steps.
        app.finish_all_pending_history();
    }

    fn type_token(app: &mut OpenCADStudio, text: &str) {
        let _ = app.feed_active_cmd(text);
        app.finish_all_pending_history();
    }

    fn copies(app: &OpenCADStudio, originals: &[Handle]) -> Vec<Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .map(|e| e.common().handle)
            .filter(|h| !originals.contains(h))
            .collect()
    }

    fn assert_circle(app: &OpenCADStudio, h: Handle, layer: &str, offset: DVec3) {
        let e = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(h)
            .unwrap();
        assert_eq!(
            e.common().layer,
            layer,
            "destination layer must come from the phase-2 pick"
        );
        let codec::EntityType::Circle(c) = e else {
            panic!("expected circle")
        };
        assert_eq!(
            (c.center.x, c.center.y, c.center.z),
            (1.0 + offset.x, 2.0 + offset.y, offset.z)
        );
    }

    fn ready() -> (OpenCADStudio, Handle, Handle, Handle) {
        let (mut app, src, dest) = fixture();
        pick(&mut app, &[src]);
        let _ = app.dispatch_command("COPYTOLAYER");
        assert!(app.tabs[app.active_tab].scene.selected.is_empty());
        pick(&mut app, &[dest]);
        let copied = copies(&app, &[src, dest]);
        assert_eq!(copied.len(), 1);
        assert_circle(&app, copied[0], "BLUE", DVec3::ZERO);
        assert!(
            app.tabs[app.active_tab].active_cmd.is_some(),
            "copies must exist at the base point prompt"
        );
        (app, src, dest, copied[0])
    }

    #[test]
    fn copytolayer_destination_after_accumulated_gui_selection() {
        let (mut app, src, dest) = fixture();
        let _ = app.dispatch_command("COPYTOLAYER");
        pick(&mut app, &[src]);
        feed(&mut app, StepInput::Enter);
        pick(&mut app, &[dest]);
        let copied = copies(&app, &[src, dest]);
        assert_eq!(copied.len(), 1);
        assert_circle(&app, copied[0], "BLUE", DVec3::ZERO);
        assert_circle(&app, src, "RED", DVec3::ZERO);
    }

    #[test]
    fn laymatch_destination_after_accumulated_gui_selection() {
        let (mut app, src, dest) = fixture();
        let _ = app.dispatch_command("LAYMATCH");
        pick(&mut app, &[src]);
        feed(&mut app, StepInput::Enter);
        pick(&mut app, &[dest]);
        assert_circle(&app, src, "BLUE", DVec3::ZERO);
        assert!(copies(&app, &[src, dest]).is_empty());
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn copytolayer_and_laymatch_gather_picks_and_box_until_enter() {
        for name in ["COPYTOLAYER", "LAYMATCH"] {
            let (mut app, src, dest) = fixture();
            let scene = &mut app.tabs[app.active_tab].scene;
            let extra: Vec<_> = (0..3)
                .map(|n| {
                    scene.add_entity(codec::EntityType::Circle(Circle::from_coords(
                        20.0 + n as f64,
                        0.0,
                        0.0,
                        1.0,
                    )))
                })
                .collect();
            let _ = app.dispatch_command(name);
            let prompt = app.tabs[app.active_tab]
                .active_cmd
                .as_ref()
                .unwrap()
                .prompt();
            pick(&mut app, &[src]);
            pick(&mut app, &extra[..1]);
            pick(&mut app, &extra[1..]); // A window selecting two more objects.
            assert_eq!(
                app.tabs[app.active_tab]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .prompt(),
                prompt,
                "several picks must stay at the objects prompt until Enter"
            );
            assert_eq!(
                copies(&app, &[src, dest]).len(),
                3,
                "no copies before Enter / destination"
            );
            // A removal must replace the command's cached set, not be unioned into it.
            app.tabs[app.active_tab].scene.deselect_entity(extra[2]);
            selection_changed(&mut app);
            feed(&mut app, StepInput::Enter);
            assert!(app.tabs[app.active_tab].scene.selected.is_empty());
            pick(&mut app, &[dest]);
            let all_originals = [src, dest, extra[0], extra[1], extra[2]];
            if name == "COPYTOLAYER" {
                assert_eq!(copies(&app, &all_originals).len(), 3);
                assert_circle(&app, src, "RED", DVec3::ZERO);
            } else {
                for h in [src, extra[0], extra[1]] {
                    assert_eq!(
                        app.tabs[app.active_tab]
                            .scene
                            .document
                            .get_entity(h)
                            .unwrap()
                            .common()
                            .layer,
                        "BLUE"
                    );
                }
                assert_eq!(
                    app.tabs[app.active_tab]
                        .scene
                        .document
                        .get_entity(extra[2])
                        .unwrap()
                        .common()
                        .layer,
                    "0"
                );
            }
        }
    }

    #[test]
    fn copytolayer_and_laymatch_empty_selection_enter_cancels() {
        for name in ["COPYTOLAYER", "LAYMATCH"] {
            let (mut app, src, _) = fixture();
            let _ = app.dispatch_command(name);
            pick(&mut app, &[src]);
            app.tabs[app.active_tab].scene.deselect_all();
            selection_changed(&mut app);
            feed(&mut app, StepInput::Enter);
            assert!(
                app.tabs[app.active_tab].active_cmd.is_none(),
                "empty selection must cancel"
            );
            assert!(app.tabs[app.active_tab].history.undo_stack.is_empty());
        }
    }

    #[test]
    fn copytolayer_preselection_and_exit_keep_copies_in_place() {
        for exit in ["", "X", "eXit", "Esc"] {
            let (mut app, src, dest, copy) = ready();
            if exit == "Esc" {
                feed(&mut app, StepInput::Escape);
            } else {
                type_token(&mut app, exit);
            }
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
            assert_circle(&app, copy, "BLUE", DVec3::ZERO);
            assert_circle(&app, src, "RED", DVec3::ZERO);
            assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), 1);
            app.undo_active_tab();
            assert!(copies(&app, &[src, dest]).is_empty());
            assert_circle(&app, src, "RED", DVec3::ZERO);
        }
        let (mut app, src, dest) = fixture();
        pick(&mut app, &[src]);
        let _ = app.dispatch_command("LAYMATCH");
        pick(&mut app, &[dest]);
        assert_circle(&app, src, "BLUE", DVec3::ZERO);
    }

    #[test]
    fn copytolayer_base_second_point_preview_move_and_one_undo() {
        let (mut app, src, dest, copy) = ready();
        let i = app.active_tab;
        let originals: Vec<_> = [src, dest]
            .iter()
            .map(|h| {
                serde_json::to_value(app.tabs[i].scene.document.get_entity(*h).unwrap()).unwrap()
            })
            .collect();
        feed(&mut app, StepInput::Point(DVec3::new(2.0, 3.0, 0.0)));
        let cmd = app.tabs[i].active_cmd.as_mut().unwrap();
        assert_eq!(cmd.preview_hidden_handles(), &[copy]);
        let preview = cmd.on_preview_wires(DVec3::new(7.0, 9.0, 0.0));
        assert!(preview.len() > 1, "copied geometry plus COPY rubber band");
        feed(&mut app, StepInput::Point(DVec3::new(7.0, 9.0, 0.0)));
        assert_circle(&app, copy, "BLUE", DVec3::new(5.0, 6.0, 0.0));
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            app.tabs[i].history.undo_stack.len(),
            1,
            "copy + layer + move is one undo"
        );
        for (n, h) in [src, dest].iter().enumerate() {
            assert_eq!(
                serde_json::to_value(app.tabs[i].scene.document.get_entity(*h).unwrap()).unwrap(),
                originals[n]
            );
        }
        app.undo_active_tab();
        assert!(copies(&app, &[src, dest]).is_empty());
        for (n, h) in [src, dest].iter().enumerate() {
            assert_eq!(
                serde_json::to_value(app.tabs[i].scene.document.get_entity(*h).unwrap()).unwrap(),
                originals[n]
            );
        }
        app.redo_active_tab();
        assert_circle(&app, copy, "BLUE", DVec3::new(5.0, 6.0, 0.0));
    }

    #[test]
    fn copytolayer_displacement_coordinates_and_default() {
        for (text, delta) in [
            ("3,4", DVec3::new(3.0, 4.0, 0.0)),
            ("3,4,5", DVec3::new(3.0, 4.0, 5.0)),
            ("", DVec3::ZERO),
        ] {
            let (mut app, src, _, copy) = ready();
            type_token(&mut app, "D");
            type_token(&mut app, text);
            assert!(app.tabs[app.active_tab].active_cmd.is_none());
            assert_circle(&app, copy, "BLUE", delta);
            assert_circle(&app, src, "RED", DVec3::ZERO);
            assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), 1);
        }
    }

    #[test]
    fn copytolayer_second_point_enter_uses_first_point_as_displacement() {
        let (mut app, src, _, copy) = ready();
        feed(&mut app, StepInput::Point(DVec3::new(3.0, 4.0, 5.0)));
        feed(&mut app, StepInput::Enter);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        assert_circle(&app, copy, "BLUE", DVec3::new(3.0, 4.0, 5.0));
        assert_circle(&app, src, "RED", DVec3::ZERO);
        assert_eq!(app.tabs[app.active_tab].history.undo_stack.len(), 1);
    }
}

#[cfg(test)]
mod selection_history_tests {
    use super::super::*;
    use crate::app::OpenCADStudio;
    use crate::command::StepInput;
    use codec::entities::Line;
    use codec::types::Vector3;

    fn three_lines() -> (OpenCADStudio, Vec<codec::Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handles = (0..3)
            .map(|n| {
                app.tabs[i]
                    .scene
                    .add_entity(codec::EntityType::Line(Line::from_points(
                        Vector3::new(n as f64 * 10.0, 0.0, 0.0),
                        Vector3::new(n as f64 * 10.0 + 5.0, 0.0, 0.0),
                    )))
            })
            .collect();
        (app, handles)
    }

    fn selected(app: &OpenCADStudio) -> Vec<codec::Handle> {
        app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect()
    }

    fn pick(app: &mut OpenCADStudio, handle: codec::Handle) {
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(handle, false);
        let current = selected(app);
        let _ = app.feed_command(StepInput::SelectionComplete(current));
    }

    #[test]
    fn select_command_commits_the_picked_set() {
        let (mut app, h) = three_lines();
        let _ = app.run_command_line("SELECT");
        let i = app.active_tab;
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|c| c.is_selection_gathering()),
            "SELECT must gather objects"
        );
        pick(&mut app, h[1]);
        let _ = app.feed_command(StepInput::Enter);

        assert_eq!(selected(&app), vec![h[1]]);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn select_previous_restores_the_set_before_select() {
        let (mut app, h) = three_lines();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(h[0], false);

        // Objects already selected are taken at once.
        let _ = app.run_command_line("SELECT");
        assert!(app.tabs[i].active_cmd.is_none());
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(h[1], false);
        assert_eq!(selected(&app), vec![h[1]]);

        let _ = app.run_command_line("SELECTPREVIOUS");
        assert_eq!(selected(&app), vec![h[0]]);
    }

    #[test]
    fn select_previous_without_a_previous_set_keeps_the_selection() {
        let (mut app, h) = three_lines();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(h[2], false);
        let _ = app.run_command_line("SELECTPREVIOUS");
        assert_eq!(selected(&app), vec![h[2]]);
    }
}

#[cfg(test)]
mod dash_layer_properties_tests {
    use super::super::*;
    use crate::app::OpenCADStudio;

    fn app_with_layer() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("-LAYER NEW WALLS");
        app
    }

    fn layer(app: &OpenCADStudio) -> &codec::tables::Layer {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get("WALLS")
            .expect("WALLS layer")
    }

    #[test]
    fn dash_layer_sets_transparency_and_description() {
        let mut app = app_with_layer();
        let _ = app.run_command_line("-LAYER TR 50 WALLS");
        let _ = app.run_command_line("-LAYER D walls WALLS");

        assert_eq!(
            layer(&app).transparency,
            codec::types::Transparency::from_percent(0.5)
        );
        assert_eq!(layer(&app).description, "walls");
    }

    #[test]
    fn dash_layer_plot_flag_is_visible_in_the_layer_list() {
        let mut app = app_with_layer();
        let _ = app.run_command_line("-LAYER PLOT NOPLOT WALLS");
        assert!(!layer(&app).is_plottable);

        let listed = app.automation_op(r#"{"op":"layers"}"#);
        let walls = listed["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "WALLS")
            .expect("WALLS listed");
        assert_eq!(walls["plottable"], false);
        assert_eq!(walls["transparency"], 0);
        assert_eq!(walls["description"], "");
    }
}

#[cfg(test)]
mod attedit_dash_tests {
    use super::super::*;
    use crate::app::OpenCADStudio;
    use codec::entities::{AttributeEntity, Insert};
    use codec::types::Vector3;

    fn inserts_with_title() -> (OpenCADStudio, Vec<codec::Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handles = (0..3)
            .map(|n| {
                let mut insert =
                    Insert::new("TITLEBLOCK", Vector3::new(n as f64 * 50.0, 0.0, 0.0));
                insert
                    .attributes
                    .push(AttributeEntity::new("TITLE".into(), "old".into()));
                app.tabs[i]
                    .scene
                    .add_entity(codec::EntityType::Insert(insert))
            })
            .collect();
        (app, handles)
    }

    fn attribute(app: &OpenCADStudio, handle: codec::Handle) -> (String, String) {
        let entity = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("insert");
        let codec::EntityType::Insert(insert) = entity else {
            panic!("not an insert");
        };
        let attr = insert.attributes.first().expect("attribute");
        (attr.tag.clone(), attr.get_value().to_string())
    }

    #[test]
    fn dash_attedit_accepts_a_handle_argument() {
        let (mut app, handles) = inserts_with_title();
        let line = format!("-ATTEDIT {:X} TITLE new", handles[1].value());
        let _ = app.run_command_line(&line);

        assert_eq!(attribute(&app, handles[1]), ("TITLE".into(), "new".into()));
        assert_eq!(attribute(&app, handles[0]), ("TITLE".into(), "old".into()));
    }

    #[test]
    fn dash_attedit_prompts_when_nothing_is_selected() {
        let (mut app, _) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT");

        let i = app.active_tab;
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("ATTEDIT must ask for a reference");
        assert!(
            command.prompt().contains("Select block"),
            "prompt was {}",
            command.prompt()
        );
    }

    #[test]
    fn dash_attedit_renames_a_tag_across_every_reference() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT RENAME TITLE SHEET");

        for handle in handles {
            assert_eq!(attribute(&app, handle), ("SHEET".into(), "old".into()));
        }
    }
}

#[cfg(test)]
mod layer_filter_tests {
    use super::super::*;
    use crate::app::OpenCADStudio;

    fn app_with_layers() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        for (name, color) in [("DIMS-A", 1), ("DIMS-B", 7), ("WALLS", 1)] {
            let _ = app.run_command_line(&format!("-LAYER NEW {name}"));
            let _ = app.run_command_line(&format!("-LAYER COLOR {color} {name}"));
        }
        app
    }

    fn matching(app: &OpenCADStudio, filter: &str) -> Vec<String> {
        let filter = app
            .layer_filters
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(filter))
            .expect("filter");
        let mut names: Vec<String> = app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .iter()
            .filter(|layer| filter.matches(layer))
            .map(|layer| layer.name.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn headless_run_reaches_the_filter_command() {
        let mut app = app_with_layers();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LAYERFILTER NEW DIMS DIMS*"}"#);
        println!("DBG reply={reply}");
        assert_eq!(matching(&app, "DIMS"), ["DIMS-A", "DIMS-B"]);
    }

    #[test]
    fn name_wildcard_filter_selects_the_discipline() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        assert_eq!(matching(&app, "DIMS"), ["DIMS-A", "DIMS-B"]);
    }

    #[test]
    fn colour_predicate_narrows_the_filter_result() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        let _ = app.run_command_line("LAYERFILTER SET DIMS COLOR 1");
        assert_eq!(matching(&app, "DIMS"), ["DIMS-A"]);
    }

    #[test]
    fn apply_turns_off_every_layer_in_the_filter() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        let _ = app.run_command_line("LAYERFILTER APPLY DIMS OFF");

        let layers = &app.tabs[app.active_tab].scene.document.layers;
        assert!(layers.get("DIMS-A").unwrap().is_off());
        assert!(layers.get("DIMS-B").unwrap().is_off());
        assert!(!layers.get("WALLS").unwrap().is_off());
    }

    #[test]
    fn dash_layer_accepts_a_filter_as_its_name_list() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW DIMS DIMS*");
        let _ = app.run_command_line("-LAYER OFF FILTER:DIMS");

        let layers = &app.tabs[app.active_tab].scene.document.layers;
        assert!(layers.get("DIMS-A").unwrap().is_off());
        assert!(layers.get("DIMS-B").unwrap().is_off());
        assert!(!layers.get("WALLS").unwrap().is_off());
    }
}

#[cfg(test)]
mod dim_update_regen_tests {
    use super::super::*;
    use crate::app::OpenCADStudio;
    use crate::entities::dimension::as_dimension;
    use codec::entities::Dimension;
    use codec::entities::Line;
    use codec::types::Vector3;

    /// A line with a linear dimension associated to its two ends.
    fn app_with_associative_dimension() -> (OpenCADStudio, codec::Handle, codec::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(codec::EntityType::Line(
            Line::from_points(Vector3::ZERO, Vector3::new(40.0, 0.0, 0.0)),
        ));
        let dimension = app.tabs[i].scene.add_entity(
            crate::modules::annotate::linear_dim::linear_dimension_entity(
                glam::DVec3::ZERO,
                glam::DVec3::new(40.0, 0.0, 0.0),
                glam::DVec3::new(20.0, 10.0, 0.0),
                glam::DVec3::X,
                None,
            ),
        );
        app.tabs[i]
            .scene
            .attach_dimension_association(dimension, vec![Some(line), Some(line)]);
        (app, line, dimension)
    }

    fn last_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .rev()
            .find(|entry| matches!(entry.kind, crate::ui::command_line::EntryKind::Output))
            .map(|entry| entry.text.clone())
            .unwrap_or_default()
    }

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .filter(|entry| matches!(entry.kind, crate::ui::command_line::EntryKind::Output))
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn dimregen_solves_the_dimension_after_an_external_geometry_change() {
        let (mut app, line, dimension) = app_with_associative_dimension();
        let i = app.active_tab;
        if let Some(codec::EntityType::Line(line)) =
            app.tabs[i].scene.document.get_entity_mut(line)
        {
            line.start = Vector3::new(10.0, 0.0, 0.0);
            line.end = Vector3::new(60.0, 0.0, 0.0);
        }

        let _ = app.run_command_line("DIMREGEN");

        let dimension = as_dimension(
            app.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .expect("dimension"),
        )
        .expect("dimension");
        let Dimension::Linear(linear) = dimension else {
            panic!("linear dimension");
        };
        assert!((linear.first_point.x - 10.0).abs() < 1e-9, "{linear:?}");
        assert!((linear.second_point.x - 60.0).abs() < 1e-9, "{linear:?}");
        assert!((linear.base.actual_measurement - 50.0).abs() < 1e-9);
    }

    #[test]
    fn dimupdate_pushes_the_current_style_onto_the_selection() {
        let (mut app, _, dimension) = app_with_associative_dimension();
        let i = app.active_tab;
        let mut style = codec::tables::DimStyle::new("TALL");
        style.dimtxt = 5.0;
        let _ = app.tabs[i].scene.document.dim_styles.add(style);
        app.tabs[i].scene.document.header.current_dimstyle_name = "TALL".into();

        app.tabs[i].scene.select_entity(dimension, false);
        let _ = app.run_command_line("DIMUPDATE");

        let dimension = as_dimension(
            app.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .expect("dimension"),
        )
        .expect("dimension");
        assert_eq!(dimension.base().style_name, "TALL");
        assert!(
            last_output(&app).contains("applied 'TALL'"),
            "{}",
            last_output(&app)
        );
    }

    #[test]
    fn dimregen_reports_a_broken_association_instead_of_skipping_it() {
        let (mut app, line, _) = app_with_associative_dimension();
        let i = app.active_tab;
        app.tabs[i].scene.document.remove_entity(line);

        let _ = app.run_command_line("DIMREGEN");

        let output = all_output(&app);
        assert!(output.contains("1 broken"), "{output}");
        assert!(
            output.contains("no longer resolves"),
            "a broken reference must be named: {output}"
        );
    }
}

#[cfg(test)]
mod mission_m025_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn annotation_and_hatch_variables_read_and_write_through_setvar() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("SETVAR CANNOSCALE 1:75");
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .current_annotation_scale,
            "1:75"
        );
        let _ = app.run_command_line("SETVAR CANNOSCALE");
        assert!(all_output(&app).contains("CANNOSCALE = 1:75"));

        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 0");
        assert!(!app.tabs[app.active_tab].scene.annotation_all_visible());
        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 1");
        assert!(app.tabs[app.active_tab].scene.annotation_all_visible());
        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE");
        assert!(all_output(&app).contains("ANNOALLVISIBLE = 1"));

        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE 2");
        assert_eq!(app.annotation_auto_scale, 2);
        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE");
        assert!(all_output(&app).contains("ANNOAUTOSCALE = 2"));

        let _ = app.run_command_line("SETVAR HPLAYER");
        assert!(all_output(&app).contains("HPLAYER = ."));
    }

    #[test]
    fn annotation_variables_reject_out_of_range_values_with_a_typed_error() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE 9");
        assert_eq!(app.annotation_auto_scale, -4);
        assert!(all_output(&app).contains("integer from -4 through 4"));

        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 2");
        assert!(all_output(&app).contains("enter 0 or 1"));
    }
}

#[cfg(test)]
mod mission_m027_tests {
    use super::super::*;
    use codec::entities::Line;
    use codec::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn annoupdate_gathers_a_selection_instead_of_refusing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));

        let _ = app.run_command_line("ANNOUPDATE");
        assert!(
            app.tabs[tab].active_cmd.is_some(),
            "ANNOUPDATE must prompt for a selection, not error immediately"
        );
        assert!(!all_output(&app).contains("select annotation objects first"));

        let _ = app.run_command_line("ALL");
        assert!(app.tabs[tab].active_cmd.is_none());
    }

    #[test]
    fn annoupdate_all_explains_when_nothing_is_annotative() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));

        let _ = app.run_command_line("ANNOUPDATE ALL");
        assert!(all_output(&app).contains("no annotative objects"));
    }

    #[test]
    fn annoupdate_on_a_preselected_non_annotative_object_is_a_no_op_with_reason() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let line = app.tabs[tab]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));
        app.tabs[tab].scene.select_entity(line, false);

        let _ = app.run_command_line("ANNOUPDATE");
        let output = all_output(&app);
        assert!(output.contains("not annotative"), "{output}");
    }
}

#[cfg(test)]
mod mission_m029_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn scale_list_reports_ratio_and_in_use_count() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let _ = app.run_command_line("CANNOSCALE 1:75");
        let _ = app.run_command_line("SCALELISTEDIT LIST");
        let output = all_output(&app);
        assert!(output.contains("1:75"), "{output}");
        assert!(output.contains("in use: 0"), "{output}");
        assert!(output.contains("(current)"), "{output}");
    }

    #[test]
    fn deleting_the_current_scale_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let _ = app.run_command_line("CANNOSCALE 1:75");
        let _ = app.run_command_line("SCALELISTEDIT DELETE 1:75");
        let output = all_output(&app);
        assert!(output.contains("Cannot delete the current annotation scale"));
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .current_annotation_scale,
            "1:75"
        );
    }

    #[test]
    fn an_unused_scale_can_be_deleted_and_disappears_from_the_list() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let _ = app.run_command_line("SCALELISTEDIT DELETE 1:75");
        let _ = app.run_command_line("SCALELISTEDIT LIST");
        let output = all_output(&app);
        assert!(output.contains("Removed annotation scale 1:75"), "{output}");
        assert!(!output.contains("1:75  factor"), "{output}");
    }
}

#[cfg(test)]
mod mission_m051_tests {
    use super::super::*;
    use codec::entities::{Line, Text};
    use codec::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn add_note(app: &mut OpenCADStudio) -> (codec::Handle, codec::Handle) {
        let i = app.active_tab;
        let mut text = Text::with_value("NOTE", Vector3::ZERO);
        text.height = 2.0;
        let text = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Text(text));
        let line = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::new(-5.0, 0.5, 0.0),
                Vector3::new(15.0, 0.5, 0.0),
            )));
        (text, line)
    }

    fn masks(app: &OpenCADStudio) -> Vec<codec::Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, codec::EntityType::Wipeout(_)))
            .map(|entity| entity.common().handle)
            .collect()
    }

    fn bounds(app: &OpenCADStudio, handle: codec::Handle) -> ([f32; 3], [f32; 3]) {
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for wire in app.tabs[app.active_tab].scene.wire_models_for(&[handle]) {
            for point in &wire.points {
                for k in 0..3 {
                    min[k] = min[k].min(point[k]);
                    max[k] = max[k].max(point[k]);
                }
            }
        }
        (min, max)
    }

    fn mask_selection(app: &mut OpenCADStudio) {
        let _ = app.run_command_line("TEXTMASK");
        let _ = app.feed_command(crate::command::StepInput::Enter);
    }

    #[test]
    fn masking_text_adds_a_wipeout_and_off_removes_it_again() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (text, _line) = add_note(&mut app);
        let i = app.active_tab;

        assert!(masks(&app).is_empty());
        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        assert_eq!(masks(&app).len(), 1, "{}", all_output(&app));

        app.tabs[i].scene.select_entity(text, false);
        let _ = app.run_command_line("TEXTMASK OFF");
        assert!(masks(&app).is_empty(), "{}", all_output(&app));
        app.tabs[i].scene.select_entity(text, false);
        let record = app.tabs[i]
            .scene
            .document
            .get_entity(text)
            .unwrap()
            .common()
            .extended_data
            .get_record("TEXTMASK");
        assert!(record.is_none(), "the association must be dropped on OFF");
    }

    #[test]
    fn a_larger_offset_grows_the_masked_area() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (text, _line) = add_note(&mut app);
        let i = app.active_tab;

        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        let before = masks(&app);
        assert_eq!(before.len(), 1);
        let (min_before, max_before) = bounds(&app, before[0]);

        let _ = app.run_command_line("TEXTMASK OFFSET 0.5");
        assert!(
            all_output(&app).contains("offset = 0.5"),
            "{}",
            all_output(&app)
        );
        assert_eq!(
            crate::io::drawing_variable(&app.tabs[i].scene.document, "TEXTMASKOFFSET").as_deref(),
            Some("0.5")
        );
        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        let after = masks(&app);
        assert_eq!(after.len(), 1, "the old mask must be replaced, not stacked");
        let (min_after, max_after) = bounds(&app, after[0]);

        assert!(
            (max_after[0] - min_after[0]) > (max_before[0] - min_before[0]) + 0.2,
            "offset 0.5 must grow the mask: before {:?}, after {:?}",
            (min_before, max_before),
            (min_after, max_after)
        );
        assert!((min_after[0] - (min_before[0] - 0.2)).abs() < 1e-3);
    }

    #[test]
    fn off_without_a_selection_clears_every_mask_in_the_drawing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (text, _line) = add_note(&mut app);
        let i = app.active_tab;

        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        app.tabs[i].scene.deselect_all();
        assert_eq!(masks(&app).len(), 1);

        let _ = app.run_command_line("TEXTMASK OFF");
        assert!(masks(&app).is_empty());
        assert!(all_output(&app).contains("removed 1 mask"));
    }
}

#[cfg(test)]
mod mission_m062_tests {
    use super::super::*;
    use codec::entities::Line;
    use codec::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn temp_pdf(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ocs_m062_{tag}_{unique}.pdf"))
    }

    #[test]
    fn exporting_the_current_layout_writes_a_real_pdf_and_reports_it() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(100.0, 100.0, 0.0),
            )));
        let path = temp_pdf("current");

        let _ = app.run_command_line(&format!("EXPORTPDF {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Exported:"), "{output}");
        assert!(output.contains("1 page(s)"), "{output}");
        assert!(output.contains("mm"), "{output}");
        let bytes = std::fs::read(&path).expect("the command must create the file");
        assert!(bytes.len() > 100, "a real PDF, not a stub: {output}");
        assert!(
            bytes.starts_with(b"%PDF"),
            "the file must carry a PDF header"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn exporting_all_layouts_reports_the_page_count() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(50.0, 25.0, 0.0),
            )));
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet1");
        app.tabs[i].scene.add_layout_default_viewport("Sheet1");
        let expected_pages = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .count();
        let path = temp_pdf("all");

        let _ = app.run_command_line(&format!("EXPORTPDF {} ALL", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Exported:"), "{output}");
        assert!(
            output.contains(&format!("{expected_pages} page(s)")),
            "expected {expected_pages} paper layouts: {output}"
        );
        let bytes = std::fs::read(&path).expect("the command must create the file");
        assert!(bytes.starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_output_path_is_refused_with_usage_text() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("EXPORTPDF ");
        assert!(all_output(&app).contains("Usage: EXPORTPDF"));
    }
}

#[cfg(test)]
mod mission_m060_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn temp_pdf(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ocs_m060_{tag}_{unique}.pdf"))
    }

    fn paper_layouts(app: &mut OpenCADStudio) -> Vec<String> {
        let i = app.active_tab;
        let names: Vec<String> = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .collect();
        for name in &names {
            app.tabs[i].scene.initialize_layout_page_setup(name);
        }
        names
    }

    #[test]
    fn publishing_three_layouts_writes_one_pdf_with_three_pages() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.add_layout("Sheet2").unwrap();
        let names = paper_layouts(&mut app);
        assert_eq!(names.len(), 3, "expected three paper layouts: {names:?}");
        let path = temp_pdf("three");

        let _ = app.run_command_line(&format!("PUBLISH {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Publishing 3 sheet(s)"), "{output}");
        assert!(output.contains("Published:") && output.contains("3 page(s)"), "{output}");
        for name in &names {
            assert!(output.contains(&format!("  {name}:")), "{output}");
        }
        let bytes = std::fs::read(&path).expect("publish must create the PDF");
        assert!(bytes.starts_with(b"%PDF"), "a real PDF was written");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn choosing_two_sheets_publishes_two_pages() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.add_layout("Sheet2").unwrap();
        app.tabs[i].scene.add_layout("Sheet3").unwrap();
        let names = paper_layouts(&mut app);
        let chosen = [names[0].clone(), names[1].clone()];
        let path = temp_pdf("two");

        let _ = app.run_command_line(&format!(
            "PUBLISH {} {},{}",
            path.display(),
            chosen[0],
            chosen[1]
        ));

        let output = all_output(&app);
        assert!(output.contains("Publishing 2 sheet(s)"), "{output}");
        assert!(output.contains("2 page(s)"), "{output}");
        assert!(!output.contains(&format!("  {}:", names[2])), "{output}");
        let bytes = std::fs::read(&path).expect("publish must create the PDF");
        assert!(bytes.starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_sheet_name_is_refused_before_writing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        paper_layouts(&mut app);
        let path = temp_pdf("unknown");

        let _ = app.run_command_line(&format!("PUBLISH {} MISSING-SHEET", path.display()));

        let output = all_output(&app);
        assert!(output.contains("no layout named 'MISSING-SHEET'"), "{output}");
        assert!(!path.exists(), "nothing should be written on an unknown sheet");
    }
}

#[cfg(test)]
mod mission_m042_tests {
    use super::super::*;
    use codec::entities::Text;
    use codec::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn add_text(app: &mut OpenCADStudio) -> codec::Handle {
        let i = app.active_tab;
        let text = app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Text(Text::with_value(
                "OLD",
                Vector3::ZERO,
            )));
        text
    }

    fn text_value(app: &OpenCADStudio, handle: codec::Handle) -> String {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Text(text)) => text.value.clone(),
            Some(codec::EntityType::MText(text)) => text.value.clone(),
            _ => panic!("text host disappeared"),
        }
    }

    #[test]
    fn a_filename_field_is_inserted_and_follows_the_drawing_name() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path =
            Some(r"C:\work\TITLEBLOCK.dwg".to_string());
        let text = add_text(&mut app);

        let _ = app.run_command_line(&format!("-FIELD {:X} FILENAME", text.value()));

        let output = all_output(&app);
        assert!(output.contains(r"\AcVar Filename"), "{output}");
        assert_eq!(text_value(&app, text), "TITLEBLOCK.dwg", "{output}");
        let (code, _) = crate::entities::field::whole_text_field(
            &app.tabs[i].scene.document,
            text,
        )
        .expect("the text must carry a field expression");
        assert!(code.contains("Filename"), "{code}");

        app.tabs[i].scene.document.source_path = Some(r"C:\work\PLAN.dwg".to_string());
        let (changed, _) = crate::entities::field::update_fields(
            &mut app.tabs[i].scene.document,
            32,
            Some(&[text]),
        );
        assert_eq!(changed, vec![text]);
        assert_eq!(text_value(&app, text), "PLAN.dwg");
    }

    #[test]
    fn a_date_field_uses_the_requested_format() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let text = add_text(&mut app);

        let _ = app.run_command_line(&format!(
            r#"-FIELD {:X} DATE "yyyy/MM/dd""#,
            text.value()
        ));

        let output = all_output(&app);
        assert!(output.contains("yyyy/MM/dd"), "{output}");
        let value = text_value(&app, text);
        let bytes = value.as_bytes();
        assert_eq!(bytes.len(), 10, "date value: {value}");
        assert_eq!(bytes[4], b'/');
        assert_eq!(bytes[7], b'/');
        assert!(value.chars().filter(char::is_ascii_digit).count() == 8, "{value}");
    }

    #[test]
    fn the_expression_can_be_read_back_without_changing_it() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let text = add_text(&mut app);
        let _ = app.run_command_line(&format!("-FIELD {:X} FILENAME", text.value()));
        let before = text_value(&app, text);

        let _ = app.run_command_line(&format!("-FIELD {:X}", text.value()));

        let output = all_output(&app);
        assert!(output.contains("Field: %<"), "{output}");
        assert!(output.contains(r"\AcVar Filename"), "{output}");
        assert_eq!(text_value(&app, text), before);
    }
}

#[cfg(test)]
mod mission_m055_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn enter_layout(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let name = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.set_current_layout(name);
        app.tabs[i].scene.load_current_layout_state();
    }

    fn viewport_rects(app: &OpenCADStudio) -> Vec<(i64, i64, i64, i64)> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        let mut rects: Vec<(i64, i64, i64, i64)> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                codec::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some((
                        (vp.center.x * 1000.0).round() as i64,
                        (vp.center.z * 1000.0).round() as i64,
                        (vp.width * 1000.0).round() as i64,
                        (vp.height * 1000.0).round() as i64,
                    ))
                }
                _ => None,
            })
            .collect();
        rects.sort();
        rects
    }

    #[test]
    fn four_viewports_can_be_saved_and_restored_after_deleting_two() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);

        let _ = app.run_command_line("VPORTS 4");
        assert_eq!(viewport_rects(&app).len(), 4, "{}", all_output(&app));
        let saved = viewport_rects(&app);

        let _ = app.run_command_line("VPORTS SAVE FOUR");
        assert!(all_output(&app).contains("saved configuration 'FOUR'"));
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        let two: Vec<codec::Handle> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                codec::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .take(2)
            .collect();
        app.tabs[i].scene.erase_entities(&two);
        assert_eq!(viewport_rects(&app).len(), 2);

        let _ = app.run_command_line("VPORTS RESTORE FOUR");
        assert!(all_output(&app).contains("restored 'FOUR' with 4 viewport(s)"));
        assert_eq!(viewport_rects(&app), saved, "restore must recreate the same rectangles");
    }

    #[test]
    fn saved_configurations_are_listed_and_can_be_deleted() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS 2V");
        let _ = app.run_command_line("VPORTS SAVE TWO");

        let _ = app.run_command_line("VPORTS LIST");
        let output = all_output(&app);
        assert!(output.contains("TWO  (2 viewport(s))"), "{output}");

        let _ = app.run_command_line("VPORTS DELETE TWO");
        let output = all_output(&app);
        assert!(output.contains("deleted 'TWO'"), "{output}");
        let before = app.command_line.history.len();
        let _ = app.run_command_line("VPORTS LIST");
        let listed = app.command_line.history[before..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!listed.contains("TWO  (2 viewport(s))"), "{listed}");
        assert!(listed.contains("No saved viewport configurations."), "{listed}");

        let _ = app.run_command_line("VPORTS RESTORE TWO");
        assert!(all_output(&app).contains("no saved configuration named 'TWO'"));
    }

    #[test]
    fn restoring_an_unknown_configuration_names_the_missing_one() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);

        let _ = app.run_command_line("VPORTS RESTORE NOPE");

        assert!(all_output(&app).contains("no saved configuration named 'NOPE'"));
    }
}

#[cfg(test)]
mod mission_m058_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn enter_layout(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let name = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.set_current_layout(name);
        app.tabs[i].scene.load_current_layout_state();
    }

    fn viewports(app: &OpenCADStudio) -> Vec<codec::Handle> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                codec::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    fn vp(app: &OpenCADStudio, handle: codec::Handle) -> &codec::entities::Viewport {
        match app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
        {
            Some(codec::EntityType::Viewport(vp)) => vp,
            _ => panic!("viewport missing"),
        }
    }

    #[test]
    fn maximizing_keeps_the_scale_and_restoring_puts_the_viewport_back() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS 2H");
        let handles = viewports(&app);
        assert_eq!(handles.len(), 2, "{}", all_output(&app));
        let (target, other) = (handles[0], handles[1]);
        let i = app.active_tab;

        let ((x0, y0), (x1, y1)) = app.tabs[i]
            .scene
            .printable_area_limits()
            .unwrap();
        if let Some(codec::EntityType::Viewport(vp)) =
            app.tabs[i].scene.document.get_entity_mut(target)
        {
            vp.custom_scale = 0.02; // 1:50
            vp.view_height = 0.0;
            vp.status.locked = true;
        }
        app.tabs[i].scene.active_viewport = Some(target);
        let before = vp(&app, target).clone();
        let other_before = vp(&app, other).clone();

        let _ = app.run_command_line("VPMAX");

        let output = all_output(&app);
        assert!(output.contains("scale 1:50"), "{output}");
        let maximized = vp(&app, target);
        assert!((maximized.width - (x1 - x0)).abs() < 1e-6);
        assert!((maximized.height - (y1 - y0)).abs() < 1e-6);
        assert!((maximized.center.x - (x0 + x1) / 2.0).abs() < 1e-6);
        assert!(maximized.status.locked, "the lock state must survive");
        assert!(
            (crate::scene::vp_effective_scale(
                maximized.custom_scale,
                maximized.view_height,
                maximized.height
            ) - 0.02)
                .abs()
                < 1e-9
        );

        let _ = app.run_command_line("VPMIN");

        let restored = vp(&app, target);
        assert!((restored.width - before.width).abs() < 1e-9);
        assert!((restored.height - before.height).abs() < 1e-9);
        assert!((restored.center.x - before.center.x).abs() < 1e-9);
        assert!((restored.center.z - before.center.z).abs() < 1e-9);
        assert!((restored.view_target.x - before.view_target.x).abs() < 1e-9);
        assert!(restored.status.locked);
        assert!((crate::scene::vp_effective_scale(
            restored.custom_scale,
            restored.view_height,
            restored.height
        ) - 0.02)
            .abs()
            < 1e-9);
        assert_eq!(app.tabs[i].scene.active_viewport, Some(target));
        let other_after = vp(&app, other);
        assert!((other_after.width - other_before.width).abs() < 1e-9);
        assert!((other_after.center.x - other_before.center.x).abs() < 1e-9);
    }

    #[test]
    fn restoring_without_maximizing_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");

        let _ = app.run_command_line("VPMIN");

        assert!(all_output(&app).contains("no viewport is maximized"));
    }

    #[test]
    fn maximizing_in_model_space_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("VPMAX");

        assert!(all_output(&app).contains("switch to a paper layout first"));
    }
}

#[cfg(test)]
mod mission_m059_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn open_layout(app: &mut OpenCADStudio, name: &str) {
        let i = app.active_tab;
        app.tabs[i].scene.set_current_layout(name.to_string());
        app.tabs[i].scene.load_current_layout_state();
    }

    fn set_sheet(app: &mut OpenCADStudio, width: f64, height: f64, style: &str) {
        let i = app.active_tab;
        let layout = app.tabs[i].scene.current_layout.clone();
        let mut ps = app.tabs[i]
            .scene
            .effective_plot_settings()
            .expect("the layout has plot settings");
        ps.paper_width = width;
        ps.paper_height = height;
        ps.current_style_sheet = style.to_string();
        ps.paper_size = format!("TEST_{width}x{height}");
        app.tabs[i].scene.set_layout_plot_settings(&layout, &ps);
    }

    #[test]
    fn two_named_setups_can_be_saved_listed_and_applied() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.add_layout("Sheet2").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet2");
        open_layout(&mut app, &first);
        set_sheet(&mut app, 420.0, 297.0, "mono.ctb");
        let _ = app.run_command_line("-PAGESETUP SAVE A3-MONO");

        open_layout(&mut app, "Sheet2");
        set_sheet(&mut app, 841.0, 594.0, "colour.ctb");
        let _ = app.run_command_line("-PAGESETUP SAVE A1-COLOUR");

        let _ = app.run_command_line("-PAGESETUP LIST");
        let output = all_output(&app);
        assert!(output.contains("A3-MONO: 420.0×297.0 mm  style=mono.ctb"), "{output}");
        assert!(output.contains("A1-COLOUR: 841.0×594.0 mm  style=colour.ctb"), "{output}");

        let _ = app.run_command_line("-PAGESETUP APPLY A3-MONO");
        let applied = app.tabs[i]
            .scene
            .effective_plot_settings()
            .unwrap();
        assert!((applied.paper_width - 420.0).abs() < 1e-9, "{applied:?}");
        assert!((applied.paper_height - 297.0).abs() < 1e-9, "{applied:?}");
        assert_eq!(applied.current_style_sheet, "mono.ctb");
        let output = all_output(&app);
        assert!(output.contains("applied 'A3-MONO' to Sheet2"), "{output}");
    }

    #[test]
    fn a_setup_can_be_renamed_and_deleted() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        open_layout(&mut app, &first);
        set_sheet(&mut app, 420.0, 297.0, "mono.ctb");
        let _ = app.run_command_line("-PAGESETUP SAVE OLD");

        let _ = app.run_command_line("-PAGESETUP RENAME OLD NEW");
        assert_eq!(app.tabs[i].scene.page_setup_names(), vec!["NEW".to_string()]);
        assert!(app.tabs[i].scene.page_setup_get("NEW").is_some());

        let _ = app.run_command_line("-PAGESETUP DELETE NEW");
        assert!(app.tabs[i].scene.page_setup_get("NEW").is_none());
        assert!(all_output(&app).contains("deleted 'NEW'"));
    }

    #[test]
    fn applying_an_unknown_setup_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        open_layout(&mut app, &first);

        let _ = app.run_command_line("-PAGESETUP APPLY NOPE");

        assert!(all_output(&app).contains("no setup named 'NOPE'"));
    }
}

#[cfg(test)]
mod mission_m061_tests {
    use super::super::*;
    use codec::entities::Line;
    use codec::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn temp_svg(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ocs_m061_{tag}_{unique}.svg"))
    }

    fn add_diagonal(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(100.0, 60.0, 0.0),
            )));
    }

    fn viewbox(svg: &str) -> Option<(f64, f64)> {
        let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
        let rest = &svg[start..];
        let end = rest.find('"')?;
        let mut numbers = rest[..end].split_whitespace();
        let _x = numbers.next()?.parse::<f64>().ok()?;
        let _y = numbers.next()?.parse::<f64>().ok()?;
        let width = numbers.next()?.parse::<f64>().ok()?;
        let height = numbers.next()?.parse::<f64>().ok()?;
        Some((width, height))
    }

    #[test]
    fn preview_writes_a_real_svg_with_the_layout_paper_aspect() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        add_diagonal(&mut app);
        let path = temp_svg("model");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Preview:"), "{output}");
        assert!(output.contains("297.0×210.0 mm"), "{output}");
        let svg = std::fs::read_to_string(&path).expect("the preview file must exist");
        assert!(svg.contains("<svg"), "a real SVG, not a stub");
        let (width, height) = viewbox(&svg).expect("the SVG has a viewBox");
        assert!((width - 297.0).abs() < 0.5, "viewBox width: {width}");
        assert!((height - 210.0).abs() < 0.5, "viewBox height: {height}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn preview_follows_the_layout_page_setup() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        add_diagonal(&mut app);
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet1");
        app.tabs[i].scene.set_current_layout("Sheet1".into());
        app.tabs[i].scene.load_current_layout_state();
        let layout = app.tabs[i].scene.current_layout.clone();
        let mut ps = app.tabs[i].scene.effective_plot_settings().unwrap();
        ps.paper_size = "ISO_A3_(297.00_x_420.00_MM)".to_string();
        ps.paper_width = 297.0;
        ps.paper_height = 420.0;
        ps.rotation = codec::objects::PlotRotation::Degrees90;
        app.tabs[i].scene.set_layout_plot_settings(&layout, &ps);
        let path = temp_svg("a3");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("420.0×297.0 mm"), "{output}");
        let svg = std::fs::read_to_string(&path).expect("the preview file must exist");
        let (width, height) = viewbox(&svg).expect("the SVG has a viewBox");
        assert!((width / height - 420.0 / 297.0).abs() < 0.01, "{width}x{height}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_preview_of_empty_space_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let path = temp_svg("empty");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        assert!(all_output(&app).contains("Nothing to preview"));
        assert!(!path.exists());
    }
}

#[cfg(test)]
mod mission_m070_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fixture_dir(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ocs_m070_{tag}_{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_source(dir: &std::path::Path) {
        let mut source = codec::Document::new();
        source
            .layers
            .add(codec::tables::Layer::new("WALLS"))
            .unwrap();
        source
            .layers
            .add(codec::tables::Layer::new("HIDDEN"))
            .unwrap();
        source
            .text_styles
            .add(codec::tables::TextStyle::new("NOTE"))
            .unwrap();
        let mut title = codec::tables::BlockRecord::new("TITLE");
        title.handle = source.allocate_handle();
        let title_handle = title.handle;
        source.block_records.add(title).unwrap();
        let mut line = codec::entities::Line::new();
        line.common.layer = "WALLS".to_string();
        line.common.owner_handle = title_handle;
        source.add_entity(codec::EntityType::Line(line)).unwrap();
        let mut text = codec::entities::Text::new();
        text.style = "NOTE".to_string();
        text.common.owner_handle = title_handle;
        source.add_entity(codec::EntityType::Text(text)).unwrap();
        let bytes = crate::io::save_to_bytes(&source, "dwg", source.version).unwrap();
        std::fs::write(dir.join("plan.dwg"), bytes).unwrap();
    }

    fn attach(app: &mut OpenCADStudio, dir: &std::path::Path) -> u64 {
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        let mut record = codec::tables::BlockRecord::new("PLAN");
        record.flags.is_xref = true;
        record.xref_path = "plan.dwg".to_string();
        record.handle = app.tabs[i].scene.document.allocate_handle();
        let key = record.handle.value();
        app.tabs[i]
            .scene
            .document
            .block_records
            .add(record)
            .unwrap();
        key
    }

    #[test]
    fn a_layer_and_a_block_can_be_bound_from_the_command_line() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let dir = fixture_dir("cmd");
        write_source(&dir);
        let _ = attach(&mut app, &dir);
        let i = app.active_tab;

        let _ = app.run_command_line("XBIND PLAN LAYER WALLS");
        let output = all_output(&app);
        assert!(output.contains("layer WALLS -> PLAN$0$WALLS"), "{output}");
        assert!(app.tabs[i].scene.document.layers.get("PLAN$0$WALLS").is_some());
        assert!(app.tabs[i].scene.document.layers.get("PLAN$0$HIDDEN").is_none());
        assert!(app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap()
            .flags
            .is_xref);

        let _ = app.run_command_line("XBIND PLAN BLOCK TITLE");
        let output = all_output(&app);
        assert!(output.contains("block TITLE -> PLAN$0$TITLE"), "{output}");
        let block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN$0$TITLE")
            .expect("bound block")
            .clone();
        let owned = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == block.handle)
            .count();
        assert_eq!(owned, 2, "{output}");
        assert!(app.tabs[i]
            .scene
            .document
            .text_styles
            .get("PLAN$0$NOTE")
            .is_some());

        let _ = app.run_command_line("XBIND PLAN");
        let output = all_output(&app);
        assert!(output.contains("Bindable symbols in \"PLAN\""), "{output}");
        assert!(output.contains("layer WALLS"), "{output}");
        assert!(output.contains("block TITLE"), "{output}");

        let _ = app.run_command_line("XBIND PLAN LAYER NOPE");
        assert!(all_output(&app).contains("no layer matching 'NOPE'"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unknown_reference_is_refused_and_nothing_is_copied() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let dir = fixture_dir("missingref");
        write_source(&dir);
        let _ = attach(&mut app, &dir);
        let i = app.active_tab;

        let _ = app.run_command_line("XBIND GHOST LAYER WALLS");

        assert!(all_output(&app).contains("no reference matches 'GHOST'"));
        assert!(!app.tabs[i]
            .scene
            .document
            .layers
            .names()
            .any(|name| name.contains("$0$")));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod mission_m076_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A layer state keys each row by layer handle, so a test layer needs a
    /// real one — exactly what the LAYER command does when it creates a row.
    fn add_layer(app: &mut OpenCADStudio, name: &str) {
        let i = app.active_tab;
        let mut layer = codec::tables::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i]
            .scene
            .document
            .layers
            .add(layer)
            .unwrap();
    }

    fn layer_off(app: &OpenCADStudio, name: &str) -> bool {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get(name)
            .expect("layer")
            .flags
            .off
    }

    fn set_off(app: &mut OpenCADStudio, name: &str, off: bool) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut(name)
            .expect("layer")
            .flags
            .off = off;
    }

    fn set_color(app: &mut OpenCADStudio, name: &str, index: u8) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut(name)
            .expect("layer")
            .color = codec::types::Color::Index(index);
    }

    fn layer_color(app: &OpenCADStudio, name: &str) -> codec::types::Color {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get(name)
            .expect("layer")
            .color
    }

    fn temp_las(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ocs_m076_{tag}_{unique}.las"))
    }

    #[test]
    fn three_layers_turned_off_come_back_on() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        for name in ["WALLS", "DOORS", "WINDOWS"] {
            add_layer(&mut app, name);
        }

        let _ = app.run_command_line("-LAYERSTATE SAVE ALL-ON");
        assert!(all_output(&app).contains("saved \"ALL-ON\""));

        for name in ["WALLS", "DOORS", "WINDOWS"] {
            set_off(&mut app, name, true);
        }
        let _ = app.run_command_line("-LAYERSTATE RESTORE ALL-ON");

        for name in ["WALLS", "DOORS", "WINDOWS"] {
            assert!(!layer_off(&app, name), "{name} must come back on");
        }
        let output = all_output(&app);
        assert!(output.contains("restored \"ALL-ON\""), "{output}");
        assert!(app.active_modal.is_none(), "-LAYERSTATE must not open the dialog");
    }

    #[test]
    fn a_restore_filter_touches_only_the_named_properties() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        add_layer(&mut app, "WALLS");
        set_color(&mut app, "WALLS", 1);
        let _ = app.run_command_line("-LAYERSTATE SAVE PALETTE");

        set_color(&mut app, "WALLS", 5);
        set_off(&mut app, "WALLS", true);

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE ON");
        assert!(!layer_off(&app, "WALLS"));
        assert_eq!(layer_color(&app, "WALLS"), codec::types::Color::Index(5));

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE COLOR");
        assert_eq!(layer_color(&app, "WALLS"), codec::types::Color::Index(1));
        assert!(!layer_off(&app, "WALLS"));

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE NONSENSE");
        assert!(all_output(&app).contains("unknown restore property 'NONSENSE'"));
    }

    #[test]
    fn a_state_can_be_renamed_and_round_tripped_through_a_las_file() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        add_layer(&mut app, "WALLS");
        set_off(&mut app, "WALLS", true);
        let _ = app.run_command_line("-LAYERSTATE SAVE OLD-NAME");
        let path = temp_las("roundtrip");

        let _ = app.run_command_line(&format!(
            "-LAYERSTATE EXPORT OLD-NAME {}",
            path.display()
        ));
        assert!(path.exists(), "{}", all_output(&app));
        let _ = app.run_command_line("-LAYERSTATE RENAME OLD-NAME NEW-NAME");
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("NEW-NAME")
            .is_some());
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("OLD-NAME")
            .is_none());

        let _ = app.run_command_line("-LAYERSTATE DELETE NEW-NAME");
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("NEW-NAME")
            .is_none());
        let _ = app.run_command_line(&format!("-LAYERSTATE IMPORT {}", path.display()));
        let imported = app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("OLD-NAME")
            .expect("the exported state returns from the file");
        assert_eq!(imported.layers.len(), 2, "layer 0 and WALLS");
        assert!(imported
            .layers
            .iter()
            .any(|layer| layer.layer_name == "WALLS" && layer.off));

        set_off(&mut app, "WALLS", false);
        let _ = app.run_command_line("-LAYERSTATE RESTORE OLD-NAME");
        assert!(layer_off(&app, "WALLS"), "{}", all_output(&app));
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod mission_m078_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ocs_m078_{tag}_{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A layer a translation can act on needs a real handle.
    fn add_layer(app: &mut OpenCADStudio, name: &str) {
        let i = app.active_tab;
        let mut layer = codec::tables::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.layers.add(layer).unwrap();
    }

    fn add_line(app: &mut OpenCADStudio, layer: &str) {
        let i = app.active_tab;
        let mut line = codec::entities::Line::new();
        line.common.layer = layer.to_string();
        app.tabs[i].scene.add_entity(codec::EntityType::Line(line));
    }

    fn line_layers(app: &OpenCADStudio) -> Vec<String> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                codec::EntityType::Line(_) => Some(entity.common().layer.clone()),
                _ => None,
            })
            .collect()
    }

    /// A standards drawing holding the office layer names.
    fn write_standards(dir: &std::path::Path, layers: &[&str]) -> std::path::PathBuf {
        let mut document = codec::Document::new();
        for name in layers {
            document
                .layers
                .add(codec::tables::Layer::new(*name))
                .unwrap();
        }
        let bytes = crate::io::save_to_bytes(&document, "dwg", document.version).unwrap();
        let path = dir.join("office.dwg");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_mapping_moves_objects_and_purges_the_source() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let dir = temp_dir("apply");
        let standards = write_standards(&dir, &["WALLS"]);
        let i = app.active_tab;
        add_layer(&mut app, "A-WALL");
        add_line(&mut app, "A-WALL");
        add_line(&mut app, "0");

        let _ = app.run_command_line(&format!(
            "-LAYTRANS {} A-WALL=WALLS",
            standards.display()
        ));

        let output = all_output(&app);
        assert!(output.contains("LAYTRANS: \"A-WALL\" -> \"WALLS\" (1 object(s))."), "{output}");
        assert!(output.contains("translated 1 layer(s), 1 object(s)"), "{output}");
        assert_eq!(line_layers(&app), vec!["WALLS".to_string(), "0".to_string()]);
        assert!(app.tabs[i].scene.document.layers.get("WALLS").is_some());
        assert!(
            app.tabs[i].scene.document.layers.get("A-WALL").is_none(),
            "the empty source layer is purged"
        );
        assert!(app.active_modal.is_none(), "-LAYTRANS must not open the dialog");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_saved_mapping_list_can_be_reused_on_another_drawing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let dir = temp_dir("reuse");
        let standards = write_standards(&dir, &["WALLS", "DOORS"]);
        let map_file = dir.join("office.map");

        let _ = app.run_command_line(&format!(
            "-LAYTRANS SAVE {} A-WALL=WALLS A-DOOR=DOORS",
            map_file.display()
        ));
        assert!(map_file.exists(), "{}", all_output(&app));
        let text = std::fs::read_to_string(&map_file).unwrap();
        assert!(text.contains("A-WALL -> WALLS"), "{text}");

        let _ = app.run_command_line(&format!("-LAYTRANS LIST {}", standards.display()));
        let output = all_output(&app);
        assert!(output.contains("target layer(s)"), "{output}");
        assert!(output.contains("  WALLS"), "{output}");
        assert!(output.contains("  DOORS"), "{output}");

        // Another drawing, same mapping list, no dialog in the way.
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        add_layer(&mut app, "A-WALL");
        add_layer(&mut app, "A-DOOR");
        add_line(&mut app, "A-WALL");
        add_line(&mut app, "A-DOOR");

        let _ = app.run_command_line(&format!(
            "-LAYTRANS LOAD {} {}",
            map_file.display(),
            standards.display()
        ));
        let output = all_output(&app);
        assert!(output.contains("translated 2 layer(s), 2 object(s)"), "{output}");
        let mut layers = line_layers(&app);
        layers.sort();
        assert_eq!(layers, vec!["DOORS".to_string(), "WALLS".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_target_the_standards_do_not_define_is_reported_and_nothing_moves() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let dir = temp_dir("missing");
        let standards = write_standards(&dir, &["WALLS"]);
        add_layer(&mut app, "A-WALL");
        add_line(&mut app, "A-WALL");

        let _ = app.run_command_line(&format!(
            "-LAYTRANS {} A-WALL=GHOST",
            standards.display()
        ));

        let output = all_output(&app);
        assert!(output.contains("target layer is not in the loaded set"), "{output}");
        assert_eq!(line_layers(&app), vec!["A-WALL".to_string()]);
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get("A-WALL")
            .is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod mission_m100_tests {
    use super::super::*;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn add_mleader(app: &mut OpenCADStudio) -> codec::Handle {
        let i = app.active_tab;
        let mut leader = codec::entities::MultiLeader::with_text(
            "office note",
            codec::types::Vector3::new(20.0, 0.0, 0.0),
            vec![
                codec::types::Vector3::ZERO,
                codec::types::Vector3::new(5.0, 5.0, 0.0),
            ],
        );
        leader.text_height = 0.18;
        leader.context.text_height = 0.18;
        app.tabs[i]
            .scene
            .add_entity(codec::EntityType::MultiLeader(Box::new(leader)))
    }

    fn style(app: &OpenCADStudio, name: &str) -> codec::objects::MultiLeaderStyle {
        app.tabs[app.active_tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                codec::objects::ObjectType::MultiLeaderStyle(style)
                    if style.name.eq_ignore_ascii_case(name) =>
                {
                    Some(style.clone())
                }
                _ => None,
            })
            .expect("style")
    }

    fn leader(app: &OpenCADStudio, handle: codec::Handle) -> codec::entities::MultiLeader {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::MultiLeader(leader)) => (**leader).clone(),
            _ => panic!("multileader missing"),
        }
    }

    #[test]
    fn a_style_with_a_closed_arrow_and_its_text_height_can_be_applied() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);

        let _ = app.run_command_line("MLEADERSTYLE NEW NOTE-2.5 TEXT 2.5 ARROW CLOSED");
        let output = all_output(&app);
        assert!(output.contains("'NOTE-2.5' created with"), "{output}");
        let created = style(&app, "NOTE-2.5");
        assert_eq!(created.text_height, 2.5);
        let arrow = created.arrowhead_handle.expect("closed arrow block");
        let arrow_name = app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == arrow)
            .map(|record| record.name.clone());
        assert_eq!(arrow_name.as_deref(), Some("_Closed"));

        let handle = add_mleader(&mut app);
        let _ = app.run_command_line("MLEADERSTYLE APPLY NOTE-2.5 ALL");
        let output = all_output(&app);
        assert!(output.contains("applied 'NOTE-2.5' to 1 multileader(s)"), "{output}");

        let styled = leader(&app, handle);
        assert_eq!(styled.style_handle, Some(created.handle));
        assert_eq!(styled.text_height, 2.5);
        assert_eq!(styled.context.text_height, 2.5);
        assert_eq!(styled.arrowhead_handle, Some(arrow));

        let _ = app.run_command_line("MLEADERSTYLE INFO ALL");
        let output = all_output(&app);
        assert!(output.contains("NOTE-2.5"), "{output}");
        assert!(
            output.contains(&format!("MLEADER {:x}", handle.value())),
            "{output}"
        );
    }

    #[test]
    fn changing_landing_distance_updates_existing_geometry() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let _ = app.run_command_line("MLEADERSTYLE NEW NOTE-2.5 TEXT 2.5");
        let handle = add_mleader(&mut app);
        let _ = app.run_command_line("MLEADERSTYLE APPLY NOTE-2.5 ALL");
        let before = leader(&app, handle)
            .context
            .leader_roots
            .first()
            .map(|root| root.landing_distance)
            .unwrap_or_default();

        let _ = app.run_command_line("MLEADERSTYLE SET NOTE-2.5 LANDING 5");
        let output = all_output(&app);
        assert!(
            output.contains("'NOTE-2.5'.LANDING = 5 (1 multileader(s) updated)"),
            "{output}"
        );

        assert_eq!(style(&app, "NOTE-2.5").landing_distance, 5.0);
        let after = leader(&app, handle);
        assert_eq!(after.dogleg_length, 5.0);
        let root = after.context.leader_roots.first().expect("leader root");
        assert_eq!(root.landing_distance, 5.0);
        assert_ne!(root.landing_distance, before);

        let _ = app.run_command_line("MLEADERSTYLE LIST");
        let output = all_output(&app);
        assert!(output.contains("landing:5.00 used:1"), "{output}");
    }

    #[test]
    fn an_unknown_property_or_style_is_reported() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let _ = app.run_command_line("MLEADERSTYLE NEW NOTE-2.5 TEXT 2.5");

        let _ = app.run_command_line("MLEADERSTYLE SET NOTE-2.5 WIDTH 4");
        assert!(all_output(&app).contains("unknown property 'width'"), "{}", all_output(&app));

        let _ = app.run_command_line("MLEADERSTYLE APPLY GHOST ALL");
        assert!(all_output(&app).contains("'GHOST' not found"), "{}", all_output(&app));
    }
}

/// #2588: POLYGON takes the inscribed/circumscribed option after the centre
/// and a typed radius, as the reference does.
#[cfg(test)]
mod polygon_flow_tests {
    use crate::app::OpenCADStudio;

    fn polygon(app: &OpenCADStudio) -> Vec<(f64, f64)> {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                codec::EntityType::LwPolyline(polyline) => Some(
                    polyline
                        .vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect(),
                ),
                _ => None,
            })
            .expect("a polygon was drawn")
    }

    fn drawn(line: &str) -> Vec<(f64, f64)> {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line(line);
        polygon(&app)
    }

    #[test]
    fn a_typed_radius_draws_an_inscribed_polygon_with_its_bottom_edge_level() {
        let vertices = drawn("POLYGON 6 0,0 I 10");
        assert_eq!(vertices.len(), 6);
        for (x, y) in &vertices {
            assert!((x.hypot(*y) - 10.0).abs() < 1e-9, "vertex on the circle");
        }
        let lowest: Vec<_> = vertices.iter().filter(|(_, y)| (*y - vertices.iter().map(|v| v.1).fold(f64::MAX, f64::min)).abs() < 1e-9).collect();
        assert_eq!(lowest.len(), 2, "bottom edge level: {vertices:?}");
    }

    #[test]
    fn circumscribed_takes_the_radius_to_the_edge_midpoints() {
        let vertices = drawn("POLYGON 4 0,0 C 10");
        assert_eq!(vertices.len(), 4);
        for (x, y) in &vertices {
            assert!((x.abs() - 10.0).abs() < 1e-9 && (y.abs() - 10.0).abs() < 1e-9, "{vertices:?}");
        }
    }

    #[test]
    fn enter_at_the_option_keeps_the_default_and_a_picked_point_finishes() {
        let vertices = drawn("POLYGON 5 0,0  0,8");
        assert_eq!(vertices.len(), 5);
        assert!(vertices.iter().any(|(x, y)| x.abs() < 1e-9 && (y - 8.0).abs() < 1e-9), "{vertices:?}");
    }
}

/// #2593: MEASUREGEOM Quick names the edges its lines reach and measures
/// the closed area a click lands in.
#[cfg(test)]
mod measuregeom_quick_tests {
    use crate::app::OpenCADStudio;
    use crate::command::{CadCommand, CmdResult};
    use glam::DVec3;

    #[test]
    fn quick_reads_the_edges_it_reaches_and_a_click_measures_the_area() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("RECTANG 0,0 10,4");
        let scene = &app.tabs[app.active_tab].scene;
        let mut command = crate::modules::draw::inquiry::measuregeom::MeasureGeomCommand::new();
        let _ = command.on_text_input("Q");
        let cursor = DVec3::new(3.0, 1.0, 0.0);
        let wires = command.object_pick_hover_previews(scene, cursor);
        let plain = |s: String| s.replace(['\u{2068}', '\u{2069}'], "");
        let readout = plain(command.measurement_readout(cursor).expect("readout"));
        assert!(readout.contains("Length = 10"), "{readout}");
        assert!(readout.contains("Length = 4"), "{readout}");
        assert!(wires.len() >= 4);
        let CmdResult::ReportMeasurement(area) = command.on_point(cursor) else {
            panic!("expected the area");
        };
        let area = plain(area);
        assert!(area.contains("Area = 40") && area.contains("Perimeter = 28"), "{area}");
    }
}

/// #1912: with Dynamic Input on, Down lists the step's options at the
/// cursor; Enter picks the highlighted one. Ctrl+9 hides the command line.
#[cfg(test)]
mod dynamic_options_tests {
    use crate::app::{ArrowKey, Message, OpenCADStudio};

    fn down() -> Message {
        Message::ArrowKeyPressed {
            direction: ArrowKey::Down,
            shortcut: "DOWN".into(),
            extend_selection: false,
        }
    }

    #[test]
    fn down_lists_the_options_and_enter_picks_one() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.dyn_input = true;
        let _ = app.run_command_line("CIRCLE");
        let _ = app.update(down());
        assert!(app.context_menu_open(), "Down opens the options");
        // CIRCLE's first option is 3P; Enter picks it.
        let _ = app.update(Message::CommandFinalize);
        assert!(!app.context_menu_open());
        let prompt = app.tabs[app.active_tab].active_cmd.as_ref().unwrap().prompt();
        assert!(prompt.to_uppercase().contains("FIRST POINT"), "{prompt}");
    }

    #[test]
    fn down_without_dynamic_input_or_options_does_nothing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.dyn_input = false;
        let _ = app.run_command_line("CIRCLE");
        let _ = app.update(down());
        assert!(!app.context_menu_open());
    }

    #[test]
    fn ctrl_9_hides_and_shows_the_command_line() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let key = if cfg!(target_os = "macos") { "CMD+9" } else { "CTRL+9" }.to_string();
        let _ = app.update(Message::ShortcutPressed(key.clone()));
        assert!(app.command_line_hidden);
        let _ = app.update(Message::ShortcutPressed(key));
        assert!(!app.command_line_hidden);
    }
}

/// #2195: undo and redo bring back the view each step was made in.
#[cfg(test)]
mod undo_view_tests {
    use crate::app::{Message, OpenCADStudio};

    #[test]
    fn undo_returns_to_where_the_change_was_made() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let made_at = app.tabs[i].scene.camera.borrow().target;
        let _ = app.run_command_line("LINE 0,0 10,0 ");
        // Look somewhere far away.
        app.tabs[i].scene.camera.borrow_mut().target = glam::DVec3::new(1.0e6, 1.0e6, 0.0);
        let _ = app.update(Message::Undo);
        assert!(app.tabs[i].scene.camera.borrow().target.distance(made_at) < 1e-9);
        app.tabs[i].scene.camera.borrow_mut().target = glam::DVec3::new(-5.0e5, 0.0, 0.0);
        let _ = app.update(Message::Redo);
        assert!(app.tabs[i].scene.camera.borrow().target.distance(made_at) < 1e-9);
    }
}

/// #2033: Alt+letter opens a menu-bar menu as a keyboard-driven menu, and
/// every row of the idle shortcut menu has a letter.
#[cfg(test)]
mod menu_keyboard_tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::popup::context_menu::{MenuAction, MenuRow};

    #[test]
    fn alt_letter_opens_the_menu_bar_menu() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.show_menu_bar = true;
        let _ = app.update(Message::ShortcutPressed("ALT+F".into()));
        assert!(app.context_menu_open());
        let menu = app.current_context_menu();
        let rows = menu.selectable();
        assert!(rows.len() > 3);
        assert!(rows.iter().all(|row| row.mnemonic.is_some()));
        assert!(rows.iter().any(|row| row.action == MenuAction::MenuLine("NEW".into())
            || matches!(&row.action, MenuAction::MenuLine(line) if line.starts_with("NEW") || line.starts_with("QNEW"))));
        let _ = app.close_context_menu();
        assert!(app.menu_bar_popup.is_none());
    }

    #[test]
    fn the_idle_shortcut_menu_has_letters() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let menu = app.current_context_menu();
        let items = menu.rows.iter().filter_map(|row| match row {
            MenuRow::Item(item) => Some(item),
            _ => None,
        });
        for item in items {
            assert!(item.mnemonic.is_some(), "{}", item.label);
        }
    }
}
