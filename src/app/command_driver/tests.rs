#[cfg(test)]
#[path = "trim_hatch_tests.rs"]
mod trim_hatch_tests;

#[cfg(test)]
mod command_replacement_tests {
    use super::super::*;
    use document::entities::{Circle, Line};
    use document::types::Vector3;

    #[test]
    fn one_to_one_edits_keep_identity_but_type_changes_do_not() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
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
            vec![document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(3.0, 0.0, 0.0),
            ))],
        );
        assert_eq!(kept, vec![handle]);
        let edited = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(edited.common().owner_handle, owner);
        assert!(matches!(edited, document::EntityType::Line(line) if line.end.x == 3.0));

        let allocated = app.replace_command_entity(
            tab,
            handle,
            vec![document::EntityType::Circle(Circle::from_coords(
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
            .add_entity(document::EntityType::Line(Line::from_points(
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
            entity: document::EntityType::Line(Line::from_points(
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
        assert!(matches!(updated, document::EntityType::Line(line) if line.end.x == 4.0));
    }
}

#[cfg(test)]
mod trim_extend_history_tests {
    use super::super::*;
    use crate::modules::draw::modify::trim::{ExtendCommand, TrimCommand};
    use document::entities::Line;
    use document::types::Vector3;

    fn add_line(app: &mut OpenCADStudio, a: [f64; 2], b: [f64; 2]) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
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
                    matches!(entity, document::EntityType::Line(line)
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
        use document::entities::{BoundaryEdge, BoundaryPath, Hatch, PolylineEdge};
        use document::types::Vector2;
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
            .add_entity(document::EntityType::Hatch(Box::new(hatch)));
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
        let cmd = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("TRIM command active");
        assert_eq!(cmd.name(), "TRIM");
        let result = cmd.on_entity_pick(hatch_handle, glam::DVec3::new(2.0, 5.0, 0.0));
        assert!(matches!(result, CmdResult::ReplaceEntity(h, edited)
            if h == hatch_handle && matches!(edited.first(), Some(document::EntityType::Hatch(_)))));
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod delobj_tests {
    use super::super::*;
    use document::types::Vector3;

    fn sweep_source_presence(value: i16, mode: crate::command::ExtrudeMode) -> (bool, bool) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut circle = document::Circle::new();
        circle.radius = 2.0;
        let profile = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Circle(circle));
        let path =
            app.tabs[i]
                .scene
                .add_entity(document::EntityType::Line(document::Line::from_points(
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
                    matches!(entity, document::EntityType::Solid3D(_))
                }
                crate::command::ExtrudeMode::Surface => {
                    matches!(entity, document::EntityType::Surface(_))
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
    use document::entities::{Surface, SurfaceKind};
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
            document::EntityType::Surface(Box::new(Surface::new(SurfaceKind::Plane))),
            body,
        );
        assert!(!source.is_null());
        let i = app.active_tab;
        let invalid = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Surface(Box::new(Surface::new(
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
                matches!(entity, document::EntityType::Solid3D(_)).then_some(entity.common().handle)
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
            document::EntityType::Surface(Box::new(Surface::new(SurfaceKind::Generic))),
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
                matches!(entity, document::EntityType::Solid3D(_)).then_some(entity.common().handle)
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
    use document::entities::ViewportRenderMode as Mode;

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

        let handle = app.tabs[i].scene.add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::new(10.0, 20.0, 0.0),
                document::types::Vector3::new(30.0, 40.0, 0.0),
            ),
        ));

        // Unselected entity - AutoLISP (command "_MOVE" ss "" "_NONE" pt "_NONE" '(0 0 0)) produces:
        assert!(app.tabs[i].scene.selected_entities().is_empty());
        let cmd = format!("_MOVE {:X} ENTER _NONE 10,20,0 _NONE 0,0,0", handle.value());
        let task = app.apply_cmd_result(CmdResult::Script(cmd));
        let _ = app.drive_headless_task(task);

        let moved = app.tabs[i].scene.document.get_entity(handle).unwrap();
        if let document::EntityType::Line(l) = moved {
            assert!(
                (l.start.x - 0.0).abs() < 1e-4,
                "Expected start.x == 0, got {}",
                l.start.x
            );
            assert!(
                (l.start.y - 0.0).abs() < 1e-4,
                "Expected start.y == 0, got {}",
                l.start.y
            );
            assert!(
                (l.end.x - 20.0).abs() < 1e-4,
                "Expected end.x == 20, got {}",
                l.end.x
            );
            assert!(
                (l.end.y - 20.0).abs() < 1e-4,
                "Expected end.y == 20, got {}",
                l.end.y
            );
        } else {
            panic!("Expected line entity");
        }
    }

    #[test]
    fn test_preselection_move_with_base_and_dest_points() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let handle = app.tabs[i].scene.add_entity(document::EntityType::Line(
            document::entities::Line::from_points(
                document::types::Vector3::new(10.0, 20.0, 0.0),
                document::types::Vector3::new(30.0, 40.0, 0.0),
            ),
        ));

        // User pre-selects the entity in the GUI
        app.tabs[i].scene.select_entity(handle, true);
        assert_eq!(app.tabs[i].scene.selected_entities().len(), 1);

        // Pre-selection MOVE command only takes base and destination points
        let cmd = "_MOVE _NONE 10,20,0 _NONE 0,0,0";
        let task = app.apply_cmd_result(CmdResult::Script(cmd.to_string()));
        let _ = app.drive_headless_task(task);

        let moved = app.tabs[i].scene.document.get_entity(handle).unwrap();
        if let document::EntityType::Line(l) = moved {
            assert!(
                (l.start.x - 0.0).abs() < 1e-4,
                "Expected start.x == 0, got {}",
                l.start.x
            );
            assert!(
                (l.start.y - 0.0).abs() < 1e-4,
                "Expected start.y == 0, got {}",
                l.start.y
            );
            assert!(
                (l.end.x - 20.0).abs() < 1e-4,
                "Expected end.x == 20, got {}",
                l.end.x
            );
            assert!(
                (l.end.y - 20.0).abs() < 1e-4,
                "Expected end.y == 20, got {}",
                l.end.y
            );
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
    use document::entities::{Circle, Insert, Line};
    use document::types::Vector3;
    use document::EntityType;

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
            .add_entity(document::EntityType::Line(Line::from_points(
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
        let h_unlocked =
            app.tabs[tab]
                .scene
                .add_entity(document::EntityType::Line(Line::from_points(
                    Vector3::ZERO,
                    Vector3::new(10.0, 0.0, 0.0),
                )));

        // Create line2 on layer "LOCKED_LAYER" and lock it
        let mut line2 =
            Line::from_points(Vector3::new(0.0, 10.0, 0.0), Vector3::new(10.0, 10.0, 0.0));
        line2.common.layer = "LOCKED_LAYER".to_string();
        let h_locked = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(line2));

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
    use document::entities::AttributeDefinition;
    use document::types::Vector3;
    use document::EntityType;

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
                EntityType::Line(document::entities::Line::from_points(
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
                EntityType::Line(document::entities::Line::from_points(
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
                EntityType::Line(document::entities::Line::from_points(
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
            vec![EntityType::Line(document::entities::Line::from_points(
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
    fn block_and_instance_with_base(app: &mut OpenCADStudio, name: &str) -> document::Handle {
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            name,
            Vector3::new(10.0, 5.0, 0.0),
            vec![EntityType::Line(document::entities::Line::from_points(
                Vector3::new(10.0, 5.0, 0.0),
                Vector3::new(30.0, 5.0, 0.0),
            ))],
        );
        let mut insert =
            document::entities::Insert::new(name.to_string(), Vector3::new(100.0, 100.0, 0.0));
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
        let temp: Vec<document::Handle> = app.tabs[i]
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
        let body: Vec<_> = record.entity_handles.iter().map(|h| h.value()).collect();
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
                app.tabs[i]
                    .scene
                    .document
                    .get_entity(document::Handle::new(end)),
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
        let mut param = document::objects::BlockVisibilityParameter::default();
        param.owner = def_block;
        param.def_point = Vector3::new(12.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .document
            .block_visibility_params
            .insert(document::Handle::new(0x70), param);
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
        let mut param = document::objects::BlockVisibilityParameter::default();
        param.owner = def_block;
        param.def_point = Vector3::new(12.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .document
            .block_visibility_params
            .insert(document::Handle::new(0x70), param);
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
    use document::entities::{LwPolyline, LwVertex};
    use document::types::Vector2;

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
            hexagon
                .vertices
                .push(LwVertex::new(Vector2::new(5.0 * a.cos(), 5.0 * a.sin())));
        }
        hexagon.is_closed = true;
        let profile = app.tabs[i]
            .scene
            .add_entity(document::EntityType::LwPolyline(hexagon));
        let mut path = LwPolyline::new();
        path.vertices.push(LwVertex::new(Vector2::new(20.0, 0.0)));
        path.vertices.push(LwVertex::with_bulge(
            Vector2::new(60.0, 0.0),
            22.5f64.to_radians().tan(),
        ));
        path.vertices.push(LwVertex::new(Vector2::new(80.0, 20.0)));
        path.vertices.push(LwVertex::new(Vector2::new(80.0, 60.0)));
        let path = app.tabs[i]
            .scene
            .add_entity(document::EntityType::LwPolyline(path));
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
            .any(|entity| matches!(entity, document::EntityType::Solid3D(_)));
        assert!(swept, "the sweep from the picked end makes a solid");
    }
}

#[cfg(test)]
mod copytolayer_tests {
    use super::super::*;
    use document::entities::{Circle, Line};
    use document::types::Vector3;
    use glam::DVec3;

    fn fixture() -> (OpenCADStudio, Handle, Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let scene = &mut app.tabs[app.active_tab].scene;
        scene
            .document
            .layers
            .add(document::tables::Layer::new("RED"))
            .unwrap();
        scene
            .document
            .layers
            .add(document::tables::Layer::new("BLUE"))
            .unwrap();
        let mut circle = Circle::from_coords(1.0, 2.0, 0.0, 2.0);
        circle.common.layer = "RED".into();
        let source = scene.add_entity(document::EntityType::Circle(circle));
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
        line.common.layer = "BLUE".into();
        let dest = scene.add_entity(document::EntityType::Line(line));
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
        let document::EntityType::Circle(c) = e else {
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
                    scene.add_entity(document::EntityType::Circle(Circle::from_coords(
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
    use crate::app::OpenCADStudio;
    use crate::command::StepInput;
    use document::entities::Line;
    use document::types::Vector3;

    fn three_lines() -> (OpenCADStudio, Vec<document::Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handles = (0..3)
            .map(|n| {
                app.tabs[i]
                    .scene
                    .add_entity(document::EntityType::Line(Line::from_points(
                        Vector3::new(n as f64 * 10.0, 0.0, 0.0),
                        Vector3::new(n as f64 * 10.0 + 5.0, 0.0, 0.0),
                    )))
            })
            .collect();
        (app, handles)
    }

    fn selected(app: &OpenCADStudio) -> Vec<document::Handle> {
        app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect()
    }

    fn pick(app: &mut OpenCADStudio, handle: document::Handle) {
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
    use crate::app::OpenCADStudio;

    fn app_with_layer() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("-LAYER NEW WALLS");
        app
    }

    fn layer(app: &OpenCADStudio) -> &document::tables::Layer {
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
            document::types::Transparency::from_percent(0.5)
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
    use crate::app::OpenCADStudio;
    use document::entities::{AttributeEntity, Insert};
    use document::types::Vector3;

    fn inserts_with_title() -> (OpenCADStudio, Vec<document::Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handles = (0..3)
            .map(|n| {
                let mut insert = Insert::new("TITLEBLOCK", Vector3::new(n as f64 * 50.0, 0.0, 0.0));
                insert
                    .attributes
                    .push(AttributeEntity::new("TITLE".into(), "old".into()));
                app.tabs[i]
                    .scene
                    .add_entity(document::EntityType::Insert(insert))
            })
            .collect();
        (app, handles)
    }

    fn attribute(app: &OpenCADStudio, handle: document::Handle) -> (String, String) {
        let entity = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("insert");
        let document::EntityType::Insert(insert) = entity else {
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
    fn eattedit_opens_the_attribute_editor_on_a_selected_reference() {
        let (mut app, handles) = inserts_with_title();
        let i = app.active_tab;
        app.tabs[i].scene.select_entities(&[handles[1]]);

        let _ = app.run_command_line("EATTEDIT");

        assert_eq!(
            app.attr_editor_rows.len(),
            1,
            "the editor opens on the selected reference's one attribute"
        );
    }

    #[test]
    fn dash_attedit_asks_whether_to_edit_one_at_a_time() {
        let (mut app, _) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT");

        let i = app.active_tab;
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("ATTEDIT must ask its first question");
        assert_eq!(
            command.prompt(),
            "Edit attributes one at a time? [Yes/No] <Y>:"
        );
    }

    #[test]
    fn dash_attedit_global_branch_swaps_a_string_after_its_prompts() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT");
        let _ = app.feed_script_line("N");
        let _ = app.feed_script_line("N");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("old");
        let _ = app.feed_script_line("new");

        for handle in handles {
            assert_eq!(attribute(&app, handle).1, "new");
        }
    }

    #[test]
    fn dash_attedit_renames_a_tag_across_every_reference() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT RENAME TITLE SHEET");

        for handle in handles {
            assert_eq!(attribute(&app, handle), ("SHEET".into(), "old".into()));
        }
    }

    /// A definition change reaches its references through `ATTSYNC`: each
    /// reference takes the definition's attributes in their order, a tag it
    /// already carried keeps the value it held, and a newly defined tag
    /// arrives with its default.
    #[test]
    fn attsync_rebuilds_a_reference_from_its_changed_definition() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let tag_one = || {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new("TAG1".into(), "Tag one".into(), String::new())
            })
        };
        app.tabs[i]
            .scene
            .define_block_raw("SYNCED", Vector3::new(0.0, 0.0, 0.0), vec![tag_one()]);
        let mut insert = Insert::new("SYNCED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TAG1".into(), "kept".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        assert!(!reference.is_null(), "the reference is placed");
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "SYNCED",
                Vector3::new(0.0, 0.0, 0.0),
                vec![
                    tag_one(),
                    EntityType::AttributeDefinition(AttributeDefinition {
                        insertion_point: Vector3::new(2.0, 5.0, 0.0),
                        height: 1.0,
                        ..AttributeDefinition::new("TAG2".into(), "Tag two".into(), "two".into())
                    }),
                ],
            )
            .expect("the definition takes the new tag");

        let _ = app.run_command_line("ATTSYNC SYNCED");

        let doc = &app.tabs[i].scene.document;
        let defs: usize = doc
            .block_records
            .get("SYNCED")
            .expect("definition")
            .entity_handles
            .iter()
            .filter_map(|h| doc.get_entity(*h))
            .filter(|e| matches!(e, EntityType::AttributeDefinition(_)))
            .count();
        assert_eq!(defs, 2, "the definition carries both tags");
        let table = crate::modules::insert::block_cmdline::BlockTable::of(doc);
        assert!(
            table.attributed.iter().any(|b| b == "SYNCED"),
            "the definition is listed as attributed: {:?}",
            table.attributed
        );
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the command ran without waiting for a prompt"
        );
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("TAG1".into(), "kept".into()),
                ("TAG2".into(), "two".into())
            ],
            "the reference follows the definition without losing the value it carried"
        );
    }

    /// A tag the definition renames is a new tag with the definition's default:
    /// the value the old tag carried is dropped rather than moved onto it.
    #[test]
    fn attsync_drops_the_value_of_a_tag_the_definition_renamed() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let definition = |tag: &str, default: &str| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), default.into())
            })
        };
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "RENAMED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TITLE", "")],
        );
        let mut insert = Insert::new("RENAMED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TITLE".into(), "kept".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        assert!(!reference.is_null(), "the reference is placed");
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "RENAMED",
                Vector3::new(0.0, 0.0, 0.0),
                vec![definition("SHEET", "fresh")],
            )
            .expect("the definition renames its tag");

        let _ = app.run_command_line("ATTSYNC RENAMED");

        let document = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = document.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![("SHEET".into(), "fresh".into())],
            "the renamed tag arrives with its default and the old tag leaves"
        );
    }

    /// BATTMAN opens on the drawing's attributed block, counts its references
    /// and lists the block's definitions in their own order.
    #[test]
    fn battman_lists_the_attributed_block_and_its_definitions() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let definition = |tag: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TAG1", 2.0), definition("TAG2", 5.0)],
        );
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "SYNCED",
            Vector3::new(100.0, 100.0, 0.0),
        )));
        assert!(!reference.is_null(), "the reference is placed");

        let _ = app.dispatch_command("BATTMAN");

        let state = app.attman.as_ref().expect("the manager opens");
        let blocks: Vec<(&str, usize)> = state
            .blocks
            .iter()
            .map(|b| (b.name.as_str(), b.found))
            .collect();
        assert_eq!(
            blocks,
            vec![("SYNCED", 1)],
            "the manager shows the attributed block with its one reference"
        );
        let tags: Vec<&str> = state.rows.iter().map(|r| r.original_tag.as_str()).collect();
        assert_eq!(
            tags,
            vec!["TAG1", "TAG2"],
            "the definitions are listed in the block's own order"
        );
    }

    /// Renaming a tag in the manager and syncing writes the new tag to the
    /// definition and to the references that already carry it.
    #[test]
    fn battman_sync_renames_the_definition_tag_and_its_references() {
        use crate::ui::window::attman_dialog::AttManMsg;
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new("TAG1".into(), "Tag one".into(), String::new())
            })],
        );
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "SYNCED",
            Vector3::new(100.0, 100.0, 0.0),
        )));
        let _ = app.dispatch_command("BATTMAN");

        let _ = app.on_attman(AttManMsg::Tag("SHEET".into()));
        let _ = app.on_attman(AttManMsg::Sync);

        let doc = &app.tabs[i].scene.document;
        let definition_tag = doc
            .block_records
            .get("SYNCED")
            .expect("definition")
            .entity_handles
            .iter()
            .find_map(|h| match doc.get_entity(*h) {
                Some(EntityType::AttributeDefinition(def)) => Some(def.tag.clone()),
                _ => None,
            });
        assert_eq!(
            definition_tag.as_deref(),
            Some("SHEET"),
            "the definition takes the renamed tag"
        );
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        assert_eq!(
            insert.attributes[0].tag, "SHEET",
            "and the reference is rebuilt against the renamed definition"
        );
    }

    /// Removing a row from the manager and syncing drops the definition from
    /// the block and the attribute it produced from every reference.
    #[test]
    fn battman_sync_drops_the_removed_definition_and_its_attributes() {
        use crate::ui::window::attman_dialog::AttManMsg;
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let definition = |tag: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TAG1", 2.0), definition("TAG2", 5.0)],
        );
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "SYNCED",
            Vector3::new(100.0, 100.0, 0.0),
        )));
        let _ = app.dispatch_command("BATTMAN");

        let _ = app.on_attman(AttManMsg::Select(1));
        let _ = app.on_attman(AttManMsg::Remove);
        let _ = app.on_attman(AttManMsg::Sync);

        let doc = &app.tabs[i].scene.document;
        let definition_tags: Vec<String> = doc
            .block_records
            .get("SYNCED")
            .expect("definition")
            .entity_handles
            .iter()
            .filter_map(|h| match doc.get_entity(*h) {
                Some(EntityType::AttributeDefinition(def)) => Some(def.tag.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            definition_tags,
            vec!["TAG1".to_string()],
            "the block keeps only the definition left in the list"
        );
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let reference_tags: Vec<&str> = insert.attributes.iter().map(|a| a.tag.as_str()).collect();
        assert_eq!(
            reference_tags,
            vec!["TAG1"],
            "and the reference no longer carries the removed attribute"
        );
    }

    /// A definition dropped from the block takes the attribute it produced off
    /// every reference, while the tags that remain keep their values.
    #[test]
    fn attsync_drops_the_attribute_whose_definition_is_gone() {
        use document::entities::AttributeDefinition;
        use document::EntityType;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let definition = |tag: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::new(0.0, 0.0, 0.0),
            vec![definition("TAG1", 2.0), definition("TAG2", 5.0)],
        );
        let mut insert = Insert::new("SYNCED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TAG1".into(), "kept".into()));
        insert
            .attributes
            .push(AttributeEntity::new("TAG2".into(), "old".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "SYNCED",
                Vector3::new(0.0, 0.0, 0.0),
                vec![definition("TAG1", 2.0)],
            )
            .expect("the definition loses its second tag");

        let _ = app.run_command_line("ATTSYNC SYNCED");

        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = doc.get_entity(reference).expect("reference") else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|a| (a.tag.clone(), a.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![("TAG1".into(), "kept".into())],
            "the reference follows the definition it has now, keeping the value that remains"
        );
    }

    fn set_values(app: &mut OpenCADStudio, handles: &[document::Handle], values: &[&str]) {
        let i = app.active_tab;
        for (handle, value) in handles.iter().zip(values) {
            let entity = app.tabs[i]
                .scene
                .document
                .entities_mut()
                .find(|entity| entity.common().handle == *handle)
                .expect("insert");
            let document::EntityType::Insert(insert) = entity else {
                panic!("not an insert");
            };
            insert.attributes[0].set_value(*value);
        }
    }

    #[test]
    fn dash_attedit_replaces_a_string_inside_every_attribute_value() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT REPLACE old new");

        for handle in handles {
            assert_eq!(attribute(&app, handle), ("TITLE".into(), "new".into()));
        }
    }

    #[test]
    fn dash_attedit_replacement_keeps_the_rest_of_each_value() {
        let (mut app, handles) = inserts_with_title();
        set_values(&mut app, &handles, &["old north", "old south", "keep"]);
        let _ = app.run_command_line("-ATTEDIT REPLACE old new");

        assert_eq!(attribute(&app, handles[0]).1, "new north");
        assert_eq!(attribute(&app, handles[1]).1, "new south");
        assert_eq!(attribute(&app, handles[2]).1, "keep");
    }

    #[test]
    fn dash_attedit_replacement_takes_a_handle_before_the_verb() {
        let (mut app, handles) = inserts_with_title();
        let line = format!("-ATTEDIT {:X} REPLACE old new", handles[1].value());
        let _ = app.run_command_line(&line);

        assert_eq!(attribute(&app, handles[1]).1, "new");
        assert_eq!(attribute(&app, handles[0]).1, "old");
        assert_eq!(attribute(&app, handles[2]).1, "old");
    }

    #[test]
    fn dash_attedit_replacement_reports_a_string_that_matches_nothing() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT REPLACE zzz new");

        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or("")
                .contains("zzz"),
            "error was {:?}",
            app.command_line.last_error
        );
        for handle in handles {
            assert_eq!(attribute(&app, handle).1, "old");
        }
    }

    fn add_note_attributes(app: &mut OpenCADStudio, handles: &[document::Handle]) {
        let i = app.active_tab;
        for handle in handles {
            let entity = app.tabs[i]
                .scene
                .document
                .entities_mut()
                .find(|entity| entity.common().handle == *handle)
                .expect("insert");
            let document::EntityType::Insert(insert) = entity else {
                panic!("not an insert");
            };
            insert
                .attributes
                .push(AttributeEntity::new("NOTE".into(), "old".into()));
        }
    }

    fn attribute_value(app: &OpenCADStudio, handle: document::Handle, tag: &str) -> String {
        let entity = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("insert");
        let document::EntityType::Insert(insert) = entity else {
            panic!("not an insert");
        };
        insert
            .attributes
            .iter()
            .find(|attr| attr.tag == tag)
            .map(|attr| attr.get_value().to_string())
            .unwrap_or_default()
    }

    #[test]
    fn dash_attedit_replacement_filters_by_tag() {
        let (mut app, handles) = inserts_with_title();
        add_note_attributes(&mut app, &handles);
        let _ = app.run_command_line("-ATTEDIT REPLACE old new * TITLE");

        assert_eq!(attribute_value(&app, handles[0], "TITLE"), "new");
        assert_eq!(attribute_value(&app, handles[0], "NOTE"), "old");
    }

    #[test]
    fn dash_attedit_replacement_filters_by_value() {
        let (mut app, handles) = inserts_with_title();
        set_values(&mut app, &handles, &["old north", "north old", "keep"]);
        let _ = app.run_command_line("-ATTEDIT REPLACE old new * * old*");

        assert_eq!(attribute(&app, handles[0]).1, "new north");
        assert_eq!(attribute(&app, handles[1]).1, "north old");
        assert_eq!(attribute(&app, handles[2]).1, "keep");
    }

    #[test]
    fn dash_attedit_replacement_filters_by_block_name() {
        let (mut app, handles) = inserts_with_title();
        let _ = app.run_command_line("-ATTEDIT REPLACE old new OTHERBLOCK");
        for handle in &handles {
            assert_eq!(attribute(&app, *handle).1, "old");
        }

        let _ = app.run_command_line("-ATTEDIT REPLACE old new TITLEBLOCK");
        for handle in &handles {
            assert_eq!(attribute(&app, *handle).1, "new");
        }
    }
}

#[cfg(test)]
mod layer_filter_tests {
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

    /// #2610 / #2611: layer filters match on the shared filter engine, so a
    /// name takes the same patterns as Quick Select and FILTER.
    #[test]
    fn layer_filter_names_take_the_shared_patterns() {
        let mut app = app_with_layers();
        let _ = app.run_command_line("LAYERFILTER NEW SOME DIMS-A,WALLS");
        assert_eq!(matching(&app, "SOME"), ["DIMS-A", "WALLS"]);
        let _ = app.run_command_line("LAYERFILTER NEW OTHERS ~DIMS*");
        assert!(matching(&app, "OTHERS").contains(&"WALLS".to_string()));
        assert!(!matching(&app, "OTHERS")
            .iter()
            .any(|name| name.starts_with("DIMS")));
        let _ = app.run_command_line("-LAYER NEW A1");
        let _ = app.run_command_line("LAYERFILTER NEW NUMBERED A#");
        assert_eq!(matching(&app, "NUMBERED"), ["A1"]);
        let _ = app.run_command_line("LAYERFILTER SET SOME COLOR 1");
        assert_eq!(matching(&app, "SOME"), ["DIMS-A", "WALLS"], "both are red");
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
    use crate::app::OpenCADStudio;
    use crate::entities::dimension::as_dimension;
    use document::entities::Dimension;
    use document::entities::Line;
    use document::types::Vector3;

    /// A line with a linear dimension associated to its two ends.
    fn app_with_associative_dimension() -> (OpenCADStudio, document::Handle, document::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(40.0, 0.0, 0.0),
            )));
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
        if let Some(document::EntityType::Line(line)) =
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
        let mut style = document::tables::DimStyle::new("TALL");
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
        assert!(all_output(&app).contains("CANNOSCALE <\"1:75\">"));

        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 0");
        assert!(!app.tabs[app.active_tab].scene.annotation_all_visible());
        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 1");
        assert!(app.tabs[app.active_tab].scene.annotation_all_visible());
        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE");
        assert!(all_output(&app).contains("ANNOALLVISIBLE <1>"));

        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE 2");
        assert_eq!(app.annotation_auto_scale, 2);
        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE");
        assert!(all_output(&app).contains("ANNOAUTOSCALE <2>"));

        let _ = app.run_command_line("SETVAR HPLAYER");
        assert!(all_output(&app).contains("HPLAYER = ."));
    }

    #[test]
    fn annotation_variables_reject_out_of_range_values_with_a_typed_error() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE 9");
        assert_eq!(app.annotation_auto_scale, -4);
        assert!(all_output(&app).contains("Requires an integer between -4 and 4."));

        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 2");
        assert!(all_output(&app).contains("Requires 0 or 1 only."));
    }
}

#[cfg(test)]
mod mission_m027_tests {
    use super::super::*;
    use document::entities::Line;
    use document::types::Vector3;

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
            .add_entity(document::EntityType::Line(Line::from_points(
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
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));

        let _ = app.run_command_line("ANNOUPDATE ALL");
        assert!(all_output(&app).contains("1 found 0 were updated"));
    }

    #[test]
    fn annoupdate_on_a_preselected_non_annotative_object_is_a_no_op_with_reason() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let line = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));
        app.tabs[tab].scene.select_entity(line, false);

        let before = format!("{:?}", app.tabs[tab].scene.document.get_entity(line));
        let _ = app.run_command_line("ANNOUPDATE");
        let output = all_output(&app);
        assert!(output.contains("Select objects:"), "{output}");
        assert_eq!(
            format!("{:?}", app.tabs[tab].scene.document.get_entity(line)),
            before
        );
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
    use document::entities::{Line, Text};
    use document::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn add_note(app: &mut OpenCADStudio) -> (document::Handle, document::Handle) {
        let i = app.active_tab;
        let mut text = Text::with_value("NOTE", Vector3::ZERO);
        text.height = 2.0;
        let text = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Text(text));
        let line = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::new(-5.0, 0.5, 0.0),
                Vector3::new(15.0, 0.5, 0.0),
            )));
        (text, line)
    }

    fn masks(app: &OpenCADStudio) -> Vec<document::Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, document::EntityType::Wipeout(_)))
            .map(|entity| entity.common().handle)
            .collect()
    }

    fn bounds(app: &OpenCADStudio, handle: document::Handle) -> ([f32; 3], [f32; 3]) {
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
    use document::entities::Line;
    use document::types::Vector3;

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
            .add_entity(document::EntityType::Line(Line::from_points(
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
            .add_entity(document::EntityType::Line(Line::from_points(
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
        assert!(
            output.contains("Published:") && output.contains("3 page(s)"),
            "{output}"
        );
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
        assert!(
            output.contains("no layout named 'MISSING-SHEET'"),
            "{output}"
        );
        assert!(
            !path.exists(),
            "nothing should be written on an unknown sheet"
        );
    }
}

#[cfg(test)]
mod mission_m042_tests {
    use super::super::*;
    use document::entities::Text;
    use document::types::Vector3;

    fn all_output(app: &OpenCADStudio) -> String {
        app.command_line
            .history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn add_text(app: &mut OpenCADStudio) -> document::Handle {
        let i = app.active_tab;
        let text = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Text(Text::with_value(
                "OLD",
                Vector3::ZERO,
            )));
        text
    }

    fn text_value(app: &OpenCADStudio, handle: document::Handle) -> String {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Text(text)) => text.value.clone(),
            Some(document::EntityType::MText(text)) => text.value.clone(),
            _ => panic!("text host disappeared"),
        }
    }

    #[test]
    fn a_filename_field_is_inserted_and_follows_the_drawing_name() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path = Some(r"C:\work\TITLEBLOCK.dwg".to_string());
        let text = add_text(&mut app);

        let _ = app.run_command_line(&format!("-FIELD {:X} FILENAME", text.value()));

        let output = all_output(&app);
        assert!(output.contains(r"\AcVar Filename"), "{output}");
        assert_eq!(text_value(&app, text), "TITLEBLOCK.dwg", "{output}");
        let (code, _) = crate::entities::field::whole_text_field(&app.tabs[i].scene.document, text)
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

        let _ = app.run_command_line(&format!(r#"-FIELD {:X} DATE "yyyy/MM/dd""#, text.value()));

        let output = all_output(&app);
        assert!(output.contains("yyyy/MM/dd"), "{output}");
        let value = text_value(&app, text);
        let bytes = value.as_bytes();
        assert_eq!(bytes.len(), 10, "date value: {value}");
        assert_eq!(bytes[4], b'/');
        assert_eq!(bytes[7], b'/');
        assert!(
            value.chars().filter(char::is_ascii_digit).count() == 8,
            "{value}"
        );
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
                document::EntityType::Viewport(vp)
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
        let two: Vec<document::Handle> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
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
        assert_eq!(
            viewport_rects(&app),
            saved,
            "restore must recreate the same rectangles"
        );
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
        assert!(
            listed.contains("No saved viewport configurations."),
            "{listed}"
        );

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

    fn viewports(app: &OpenCADStudio) -> Vec<document::Handle> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    fn vp(app: &OpenCADStudio, handle: document::Handle) -> &document::entities::Viewport {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Viewport(vp)) => vp,
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

        let ((x0, y0), (x1, y1)) = app.tabs[i].scene.printable_area_limits().unwrap();
        if let Some(document::EntityType::Viewport(vp)) =
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
        assert!(
            (crate::scene::vp_effective_scale(
                restored.custom_scale,
                restored.view_height,
                restored.height
            ) - 0.02)
                .abs()
                < 1e-9
        );
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
        assert!(
            output.contains("A3-MONO: 420.0×297.0 mm  style=mono.ctb"),
            "{output}"
        );
        assert!(
            output.contains("A1-COLOUR: 841.0×594.0 mm  style=colour.ctb"),
            "{output}"
        );

        let _ = app.run_command_line("-PAGESETUP APPLY A3-MONO");
        let applied = app.tabs[i].scene.effective_plot_settings().unwrap();
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
        assert_eq!(
            app.tabs[i].scene.page_setup_names(),
            vec!["NEW".to_string()]
        );
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
    use document::entities::Line;
    use document::types::Vector3;

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
            .add_entity(document::EntityType::Line(Line::from_points(
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
        ps.rotation = document::objects::PlotRotation::Degrees90;
        app.tabs[i].scene.set_layout_plot_settings(&layout, &ps);
        let path = temp_svg("a3");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("420.0×297.0 mm"), "{output}");
        let svg = std::fs::read_to_string(&path).expect("the preview file must exist");
        let (width, height) = viewbox(&svg).expect("the SVG has a viewBox");
        assert!(
            (width / height - 420.0 / 297.0).abs() < 0.01,
            "{width}x{height}"
        );
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
        let mut source = document::Document::new();
        source
            .layers
            .add(document::tables::Layer::new("WALLS"))
            .unwrap();
        source
            .layers
            .add(document::tables::Layer::new("HIDDEN"))
            .unwrap();
        source
            .text_styles
            .add(document::tables::TextStyle::new("NOTE"))
            .unwrap();
        let mut title = document::tables::BlockRecord::new("TITLE");
        title.handle = source.allocate_handle();
        let title_handle = title.handle;
        source.block_records.add(title).unwrap();
        let mut line = document::entities::Line::new();
        line.common.layer = "WALLS".to_string();
        line.common.owner_handle = title_handle;
        source.add_entity(document::EntityType::Line(line)).unwrap();
        let mut text = document::entities::Text::new();
        text.style = "NOTE".to_string();
        text.common.owner_handle = title_handle;
        source.add_entity(document::EntityType::Text(text)).unwrap();
        let bytes = crate::io::save_to_bytes(&source, "dwg", source.version).unwrap();
        std::fs::write(dir.join("plan.dwg"), bytes).unwrap();
    }

    fn attach(app: &mut OpenCADStudio, dir: &std::path::Path) -> u64 {
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        let mut record = document::tables::BlockRecord::new("PLAN");
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
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("PLAN$0$WALLS")
            .is_some());
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("PLAN$0$HIDDEN")
            .is_none());
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("PLAN")
                .unwrap()
                .flags
                .is_xref
        );

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
        let mut layer = document::tables::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.layers.add(layer).unwrap();
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
            .color = document::types::Color::Index(index);
    }

    fn layer_color(app: &OpenCADStudio, name: &str) -> document::types::Color {
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
        assert!(
            app.active_modal.is_none(),
            "-LAYERSTATE must not open the dialog"
        );
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
        assert_eq!(layer_color(&app, "WALLS"), document::types::Color::Index(5));

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE COLOR");
        assert_eq!(layer_color(&app, "WALLS"), document::types::Color::Index(1));
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

        let _ = app.run_command_line(&format!("-LAYERSTATE EXPORT OLD-NAME {}", path.display()));
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
        let mut layer = document::tables::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.layers.add(layer).unwrap();
    }

    fn add_line(app: &mut OpenCADStudio, layer: &str) {
        let i = app.active_tab;
        let mut line = document::entities::Line::new();
        line.common.layer = layer.to_string();
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(line));
    }

    fn line_layers(app: &OpenCADStudio) -> Vec<String> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(_) => Some(entity.common().layer.clone()),
                _ => None,
            })
            .collect()
    }

    /// A standards drawing holding the office layer names.
    fn write_standards(dir: &std::path::Path, layers: &[&str]) -> std::path::PathBuf {
        let mut document = document::Document::new();
        for name in layers {
            document
                .layers
                .add(document::tables::Layer::new(*name))
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

        let _ = app.run_command_line(&format!("-LAYTRANS {} A-WALL=WALLS", standards.display()));

        let output = all_output(&app);
        assert!(
            output.contains("LAYTRANS: \"A-WALL\" -> \"WALLS\" (1 object(s))."),
            "{output}"
        );
        assert!(
            output.contains("translated 1 layer(s), 1 object(s)"),
            "{output}"
        );
        assert_eq!(
            line_layers(&app),
            vec!["WALLS".to_string(), "0".to_string()]
        );
        assert!(app.tabs[i].scene.document.layers.get("WALLS").is_some());
        assert!(
            app.tabs[i].scene.document.layers.get("A-WALL").is_none(),
            "the empty source layer is purged"
        );
        assert!(
            app.active_modal.is_none(),
            "-LAYTRANS must not open the dialog"
        );
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
        assert!(
            output.contains("translated 2 layer(s), 2 object(s)"),
            "{output}"
        );
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

        let _ = app.run_command_line(&format!("-LAYTRANS {} A-WALL=GHOST", standards.display()));

        let output = all_output(&app);
        assert!(
            output.contains("target layer is not in the loaded set"),
            "{output}"
        );
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

    fn add_mleader(app: &mut OpenCADStudio) -> document::Handle {
        let i = app.active_tab;
        let mut leader = document::entities::MultiLeader::with_text(
            "office note",
            document::types::Vector3::new(20.0, 0.0, 0.0),
            vec![
                document::types::Vector3::ZERO,
                document::types::Vector3::new(5.0, 5.0, 0.0),
            ],
        );
        leader.text_height = 0.18;
        leader.context.text_height = 0.18;
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::MultiLeader(Box::new(leader)))
    }

    fn style(app: &OpenCADStudio, name: &str) -> document::objects::MultiLeaderStyle {
        app.tabs[app.active_tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                document::objects::ObjectType::MultiLeaderStyle(style)
                    if style.name.eq_ignore_ascii_case(name) =>
                {
                    Some(style.clone())
                }
                _ => None,
            })
            .expect("style")
    }

    fn leader(app: &OpenCADStudio, handle: document::Handle) -> document::entities::MultiLeader {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::MultiLeader(leader)) => (**leader).clone(),
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
        assert!(
            output.contains("applied 'NOTE-2.5' to 1 multileader(s)"),
            "{output}"
        );

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
        assert!(
            all_output(&app).contains("unknown property 'width'"),
            "{}",
            all_output(&app)
        );

        let _ = app.run_command_line("MLEADERSTYLE APPLY GHOST ALL");
        assert!(
            all_output(&app).contains("'GHOST' not found"),
            "{}",
            all_output(&app)
        );
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
                document::EntityType::LwPolyline(polyline) => Some(
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
        let lowest: Vec<_> = vertices
            .iter()
            .filter(|(_, y)| {
                (*y - vertices.iter().map(|v| v.1).fold(f64::MAX, f64::min)).abs() < 1e-9
            })
            .collect();
        assert_eq!(lowest.len(), 2, "bottom edge level: {vertices:?}");
    }

    #[test]
    fn circumscribed_takes_the_radius_to_the_edge_midpoints() {
        let vertices = drawn("POLYGON 4 0,0 C 10");
        assert_eq!(vertices.len(), 4);
        for (x, y) in &vertices {
            assert!(
                (x.abs() - 10.0).abs() < 1e-9 && (y.abs() - 10.0).abs() < 1e-9,
                "{vertices:?}"
            );
        }
    }

    #[test]
    fn enter_at_the_option_keeps_the_default_and_a_picked_point_finishes() {
        let vertices = drawn("POLYGON 5 0,0  0,8");
        assert_eq!(vertices.len(), 5);
        assert!(
            vertices
                .iter()
                .any(|(x, y)| x.abs() < 1e-9 && (y - 8.0).abs() < 1e-9),
            "{vertices:?}"
        );
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
        assert!(
            area.contains("Area = 40") && area.contains("Perimeter = 28"),
            "{area}"
        );
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
        let prompt = app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt();
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
        let key = if cfg!(target_os = "macos") {
            "CMD+9"
        } else {
            "CTRL+9"
        }
        .to_string();
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

/// Batch 4: behaviour that still failed against current HEAD.
#[cfg(test)]
mod batch4_tests {
    use crate::app::OpenCADStudio;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn lines(app: &mut OpenCADStudio) -> Vec<(f64, f64, f64, f64)> {
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
        q["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["start"][0].as_f64().unwrap(),
                    e["start"][1].as_f64().unwrap(),
                    e["end"][0].as_f64().unwrap(),
                    e["end"][1].as_f64().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn scale_copy_keeps_the_source_and_adds_a_scaled_duplicate() {
        let mut app = new_app();
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("SELECTALL");
        let _ = app.run_command_line("SCALE 0,0 C 2");
        let mut got = lines(&mut app);
        got.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
        assert_eq!(got.len(), 2, "{got:?}");
        assert!((got[0].2 - 10.0).abs() < 1e-9, "{got:?}");
        assert!((got[1].2 - 20.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn rotate_copy_keeps_the_source_and_adds_a_rotated_duplicate() {
        let mut app = new_app();
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("SELECTALL");
        let _ = app.run_command_line("ROTATE 0,0 C 90");
        let mut got = lines(&mut app);
        got.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            (got[0].2 - 0.0).abs() < 1e-6 && (got[0].3 - 10.0).abs() < 1e-6,
            "{got:?}"
        );
        assert!((got[1].2 - 10.0).abs() < 1e-6, "{got:?}");
    }

    fn out(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
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

    fn layout_viewport_handles(app: &OpenCADStudio) -> Vec<document::Handle> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    fn vp(app: &OpenCADStudio, handle: document::Handle) -> &document::entities::Viewport {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Viewport(vp)) => vp,
            _ => panic!("viewport missing"),
        }
    }

    fn effective_scale(app: &OpenCADStudio, handle: document::Handle) -> f64 {
        let vp = vp(app, handle);
        crate::scene::vp_effective_scale(vp.custom_scale, vp.view_height, vp.height)
    }

    #[test]
    fn vpscale_sets_the_viewport_plot_scale_and_reports_it() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let handles = layout_viewport_handles(&app);
        assert_eq!(handles.len(), 1, "{}", out(&app));

        assert!(app.command_line.last_error.is_none());
        let _ = app.run_command_line("VPSCALE 1:50");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let eff = effective_scale(&app, handles[0]);
        // VPSCALE is a physical sheet scale: metre drawings require
        // 1000 / 50 paper units per model unit, not the raw 1/50.
        let expected = 0.02 * app.tabs[app.active_tab].scene.default_scale_unit_factor();
        assert!(
            (eff - expected).abs() < 1e-9,
            "the viewport is not at 1:50 (got {eff}, expected {expected})"
        );

        let text = out(&app);
        assert!(text.contains("VPSCALE"), "{text}");

        assert!(app.command_line.last_error.is_none());
        let _ = app.run_command_line("VPSCALE ?");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let text = out(&app);
        assert!(text.contains("1:50"), "{text}");
    }

    #[test]
    fn vp_lock_toggles_the_viewport_display_lock() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let handles = layout_viewport_handles(&app);
        assert_eq!(handles.len(), 1, "{}", out(&app));

        let _ = app.run_command_line("VPLOCK ON");
        assert!(!app.command_line.last_error.is_some(), "{}", out(&app));
        assert!(vp(&app, handles[0]).status.locked);

        let _ = app.run_command_line("VPLOCK TOGGLE");
        assert!(!app.command_line.last_error.is_some(), "{}", out(&app));
        assert!(!vp(&app, handles[0]).status.locked);

        let _ = app.run_command_line("VPLOCK OFF");
        assert!(!app.command_line.last_error.is_some(), "{}", out(&app));
        assert!(!vp(&app, handles[0]).status.locked);
    }

    #[test]
    fn vpscale_in_model_space_is_refused() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("VPSCALE 1:50");
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("switch to a paper layout first"),
            "{:?}",
            app.command_line.last_error
        );
    }

    #[test]
    fn objectscale_adds_an_explicit_scale_and_delete_removes_it() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("TEXT 0,0 2 0 hello");
        let _ = app.run_command_line("SELECTALL");
        let i = app.active_tab;
        let handle = app.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .next()
            .expect("the text is selected");

        let _ = app.run_command_line("OBJECTSCALE ADD 1:100");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let after_add =
            crate::scene::annotative::object_scale_memberships(&app.tabs[i].scene.document, handle);
        assert!(
            after_add.iter().any(|(name, _)| name == "1:100"),
            "{after_add:?}"
        );

        let _ = app.run_command_line("OBJECTSCALE DELETE 1:100");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let after_del =
            crate::scene::annotative::object_scale_memberships(&app.tabs[i].scene.document, handle);
        assert!(
            !after_del.iter().any(|(name, _)| name == "1:100"),
            "{after_del:?}"
        );
    }

    #[test]
    fn tablestyle_set_applies_an_existing_style() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("TABLESTYLE NEW MYPAPER");
        let _ = app.run_command_line("TABLESTYLE SET MYPAPER");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let current = app.tabs[app.active_tab]
            .scene
            .document
            .header
            .current_table_style_name
            .clone();
        assert_eq!(current, "MYPAPER");
    }

    fn model_lines(app: &OpenCADStudio) -> Vec<document::Handle> {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => Some(line.common.handle),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn group_rename_add_and_remove_edit_an_existing_group() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 10,5");
        let _ = app.run_command_line("LINE 0,9 10,9");
        let i = app.active_tab;
        let lines = model_lines(&app);
        assert_eq!(lines.len(), 3, "{lines:?}");
        app.tabs[i]
            .scene
            .create_group("MYGROUP".to_string(), vec![lines[0], lines[1]]);

        let _ = app.run_command_line("GROUP RENAME MYGROUP TEAM");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let names: Vec<String> = app.tabs[i].scene.groups().map(|g| g.name.clone()).collect();
        assert_eq!(names, vec!["TEAM".to_string()], "{names:?}");
        let renamed = app.tabs[i]
            .scene
            .groups()
            .find(|g| g.name == "TEAM")
            .cloned()
            .expect("the renamed group");
        assert_eq!(renamed.entities.len(), 2, "{:?}", renamed.entities);
        let keyed = app.tabs[i]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                document::objects::ObjectType::Dictionary(dict) => dict
                    .entries
                    .iter()
                    .find(|(key, _)| key == "TEAM")
                    .map(|_| ()),
                _ => None,
            });
        assert!(keyed.is_some(), "the group dictionary lost the new name");

        let add = format!("GROUP ADD TEAM {:X}", lines[2].value());
        let _ = app.run_command_line(&add);
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let group = app.tabs[i]
            .scene
            .groups()
            .find(|g| g.name == "TEAM")
            .cloned()
            .expect("the group");
        assert!(group.contains(lines[2]), "{:?}", group.entities);

        let remove = format!("GROUP REMOVE TEAM {:X}", lines[0].value());
        let _ = app.run_command_line(&remove);
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        let group = app.tabs[i]
            .scene
            .groups()
            .find(|g| g.name == "TEAM")
            .cloned()
            .expect("the group");
        assert!(!group.contains(lines[0]), "{:?}", group.entities);
        assert!(group.contains(lines[1]), "{:?}", group.entities);

        let _ = app.run_command_line("GROUP RENAME NOSUCH OTHER");
        assert!(
            app.command_line
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("no group named"),
            "{:?}",
            app.command_line.last_error
        );
    }

    /// #2805: GROUPEDIT picks a group by a member, takes members out one
    /// pick at a time, adds a selection and renames, each one undo step.
    #[test]
    fn groupedit_removes_adds_and_renames() {
        use crate::app::Message;
        use crate::command::StepInput;
        use glam::DVec3;
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 10,5");
        let _ = app.run_command_line("LINE 0,9 10,9");
        let lines = model_lines(&app);
        app.tabs[i]
            .scene
            .create_group("G".to_string(), vec![lines[0], lines[1]]);
        let members = |app: &OpenCADStudio| {
            app.tabs[i]
                .scene
                .groups()
                .map(|g| (g.name.clone(), g.entities.clone()))
                .collect::<Vec<_>>()
        };

        let _ = app.dispatch_command("GROUPEDIT");
        let _ = app.feed_command(StepInput::EntityPick(lines[1], DVec3::ZERO));
        let _ = app.feed_command(StepInput::Text("R".into()));
        let _ = app.feed_command(StepInput::EntityPick(lines[0], DVec3::ZERO));
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(members(&app), vec![("G".to_string(), vec![lines[1]])]);

        let _ = app.dispatch_command("GROUPEDIT");
        let _ = app.feed_command(StepInput::Text("N".into()));
        let _ = app.feed_command(StepInput::Text("G".into()));
        let _ = app.feed_command(StepInput::Text("A".into()));
        let _ = app.feed_command(StepInput::SelectionComplete(vec![lines[2]]));
        let _ = app.dispatch_command("GROUPEDIT");
        let _ = app.feed_command(StepInput::Text("N".into()));
        let _ = app.feed_command(StepInput::Text("G".into()));
        let _ = app.feed_command(StepInput::Text("REN".into()));
        let _ = app.feed_command(StepInput::Text("H".into()));
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        assert_eq!(members(&app), vec![("H".to_string(), vec![lines[1], lines[2]])]);

        let _ = app.update(Message::Undo);
        assert_eq!(members(&app), vec![("G".to_string(), vec![lines[1], lines[2]])]);
    }

    #[test]
    fn qselect_command_line_compares_negates_and_matches_wildcards() {
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("LINE 0,5 40,5");
        let _ = app.run_command_line("CIRCLE 100,100 5");
        let count = |app: &OpenCADStudio| app.tabs[app.active_tab].scene.selected_entities().len();

        // Numeric comparison: only the 40-long line.
        let _ = app.run_command_line("QSELECT LENGTH > 20");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));
        assert_eq!(count(&app), 1, "{}", out(&app));

        // Type equality ignores case.
        let _ = app.run_command_line("QSELECT TYPE line");
        assert_eq!(count(&app), 2, "{}", out(&app));

        // Wildcards work on the type name too.
        let _ = app.run_command_line("QSELECT TYPE L*");
        assert_eq!(count(&app), 2, "{}", out(&app));

        // NOT turns the test around: everything that is not a line.
        let _ = app.run_command_line("QSELECT NOT TYPE LINE");
        assert_eq!(count(&app), 1, "{}", out(&app));
        let selected = app.tabs[app.active_tab].scene.selected_entities();
        assert!(
            matches!(selected[0].1, document::EntityType::Circle(_)),
            "the circle should be the only match"
        );

        // A malformed line keeps the selection and states the form.
        let _ = app.run_command_line("QSELECT LENGTH >");
        assert_eq!(count(&app), 1, "{}", out(&app));
        assert!(out(&app).contains("a value is needed"), "{}", out(&app));
    }

    #[test]
    fn burst_explodes_a_block_and_keeps_attribute_values_as_text() {
        use document::entities::{AttributeDefinition, AttributeEntity, Insert};
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;

        let mut record = document::tables::BlockRecord::new("TAGBLOCK");
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();

        let mut line = document::entities::Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.owner_handle = owner;
        let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

        let mut variable =
            AttributeDefinition::new("STATUS".into(), "Status".into(), "DEFAULT".into());
        variable.insertion_point = Vector3::new(1.0, 1.0, 0.0);
        variable.alignment_point = variable.insertion_point;
        variable.height = 2.5;
        variable.common.owner_handle = owner;
        let variable_handle = doc
            .add_entity(document::EntityType::AttributeDefinition(variable.clone()))
            .unwrap();

        let mut constant = AttributeDefinition::new("SHEET".into(), String::new(), "A-101".into());
        constant.insertion_point = Vector3::new(1.0, 4.0, 0.0);
        constant.alignment_point = constant.insertion_point;
        constant.flags.constant = true;
        constant.common.owner_handle = owner;
        let constant_handle = doc
            .add_entity(document::EntityType::AttributeDefinition(constant))
            .unwrap();

        let record = doc.block_records.get_mut("TAGBLOCK").unwrap();
        record.entity_handles = vec![line_handle, variable_handle, constant_handle];

        let mut insert = Insert::new("TAGBLOCK", Vector3::new(100.0, 50.0, 0.0));
        let mut attribute = AttributeEntity::from_definition(&variable, Some("ACTIVE".into()));
        attribute.insertion_point = Vector3::new(101.0, 51.0, 0.0);
        attribute.alignment_point = attribute.insertion_point;
        insert.attributes.push(attribute);
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();

        let _ = app.run_command_line("SELECTALL");
        let _ = app.run_command_line("BURST");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        assert!(
            doc.get_entity(insert_handle).is_none(),
            "the reference itself must be gone"
        );
        let texts: Vec<(String, f64, f64)> = doc
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Text(text) => Some((
                    text.value.clone(),
                    text.insertion_point.x,
                    text.insertion_point.y,
                )),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&("ACTIVE".to_string(), 101.0, 51.0)),
            "the reference's value must survive as text: {texts:?}"
        );
        assert!(
            texts.contains(&("A-101".to_string(), 101.0, 54.0)),
            "a constant attribute's value must survive at its placed point: {texts:?}"
        );
        assert_eq!(texts.len(), 2, "{texts:?}");
        assert!(
            !doc.entities()
                .any(|entity| matches!(entity, document::EntityType::AttributeEntity(_))),
            "no attribute instance may remain"
        );
    }

    #[test]
    fn attredef_takes_the_reference_attributes_as_the_new_definitions() {
        use document::entities::{AttributeDefinition, AttributeEntity, Insert};
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;

        let mut record = document::tables::BlockRecord::new("TAGBLOCK");
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();

        let mut line = document::entities::Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.owner_handle = owner;
        let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

        let mut definition =
            AttributeDefinition::new("STATUS".into(), "Status".into(), "DEFAULT".into());
        definition.insertion_point = Vector3::new(1.0, 1.0, 0.0);
        definition.alignment_point = definition.insertion_point;
        definition.height = 2.5;
        definition.common.owner_handle = owner;
        let definition_handle = doc
            .add_entity(document::EntityType::AttributeDefinition(
                definition.clone(),
            ))
            .unwrap();
        doc.block_records
            .get_mut("TAGBLOCK")
            .unwrap()
            .entity_handles = vec![line_handle, definition_handle];

        let mut insert = Insert::new("TAGBLOCK", Vector3::new(100.0, 50.0, 0.0));
        let mut attribute = AttributeEntity::from_definition(&definition, Some("ACTIVE".into()));
        attribute.insertion_point = Vector3::new(101.0, 51.0, 0.0);
        attribute.alignment_point = attribute.insertion_point;
        insert.attributes.push(attribute);
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();

        // The reference is the authority: its value and placement changed.
        if let Some(document::EntityType::Insert(insert)) = doc.get_entity_mut(insert_handle) {
            insert.attributes[0].value = "CHANGED".into();
            insert.attributes[0].insertion_point = Vector3::new(105.0, 52.0, 0.0);
            insert.attributes[0].alignment_point = Vector3::new(105.0, 52.0, 0.0);
        }

        let _ = app.run_command_line("ATTREDEF TAGBLOCK");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        let definitions: Vec<_> = doc
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::AttributeDefinition(definition) => Some(definition),
                _ => None,
            })
            .collect();
        assert_eq!(definitions.len(), 1, "the old definition must be replaced");
        let definition = definitions[0];
        assert_eq!(definition.tag, "STATUS");
        assert_eq!(
            definition.default_value, "CHANGED",
            "the reference value becomes the new default"
        );
        assert!(
            (definition.insertion_point.x - 5.0).abs() < 1e-9
                && (definition.insertion_point.y - 2.0).abs() < 1e-9,
            "the placement is folded back into block space: {:?}",
            definition.insertion_point
        );

        let Some(document::EntityType::Insert(insert)) = doc.get_entity(insert_handle) else {
            panic!("reference gone");
        };
        assert_eq!(insert.attributes.len(), 1);
        assert_eq!(insert.attributes[0].value, "CHANGED");
        assert!(
            (insert.attributes[0].insertion_point.x - 105.0).abs() < 1e-9
                && (insert.attributes[0].insertion_point.y - 52.0).abs() < 1e-9,
            "the synchronised attribute keeps the reference's placement: {:?}",
            insert.attributes[0].insertion_point
        );
    }

    /// A block with one line and the given attribute definitions.
    fn block_with_attributes(
        app: &mut crate::app::OpenCADStudio,
        name: &str,
        definitions: &[(&str, &str, &str)],
    ) -> (document::Handle, Vec<document::Handle>) {
        use document::entities::{AttributeDefinition, Line};
        use document::types::Vector3;

        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let mut record = document::tables::BlockRecord::new(name);
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();

        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(10.0, 0.0, 0.0);
        line.common.owner_handle = owner;
        let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();

        let mut handles = vec![line_handle];
        for (tag, prompt, default) in definitions {
            let mut definition =
                AttributeDefinition::new((*tag).into(), (*prompt).into(), (*default).into());
            definition.insertion_point = Vector3::new(1.0, 1.0, 0.0);
            definition.alignment_point = definition.insertion_point;
            definition.common.owner_handle = owner;
            let handle = doc
                .add_entity(document::EntityType::AttributeDefinition(definition))
                .unwrap();
            handles.push(handle);
        }
        doc.block_records.get_mut(name).unwrap().entity_handles = handles.clone();
        (owner, handles)
    }

    #[test]
    fn xplode_stamps_the_named_properties_on_the_pieces() {
        use document::entities::Insert;
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        block_with_attributes(&mut app, "XUNIT", &[]);
        let doc = &mut app.tabs[i].scene.document;
        doc.layers
            .add(document::tables::Layer::new("DEST"))
            .unwrap();
        let insert = Insert::new("XUNIT", Vector3::new(5.0, 5.0, 0.0));
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();
        app.tabs[i].scene.selected.insert(insert_handle);

        let _ = app.run_command_line("XPLODE COLOR 1 LAYER DEST LINETYPE DASHED LINEWEIGHT 0.50");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        assert!(
            doc.get_entity(insert_handle).is_none(),
            "the reference itself must be gone"
        );
        let pieces: Vec<_> = doc
            .entities()
            .filter(|entity| {
                matches!(entity, document::EntityType::Line(_)) && entity.common().layer == "DEST"
            })
            .collect();
        assert_eq!(pieces.len(), 1, "one line comes out");
        let common = pieces[0].common();
        assert_eq!(common.color, document::types::Color::Index(1));
        assert_eq!(common.layer, "DEST");
        assert_eq!(common.linetype, "DASHED");
        assert_eq!(
            common.line_weight,
            document::types::LineWeight::Value(50),
            "0.50 mm is 50 hundredths"
        );
    }

    #[test]
    fn blockreplace_retargets_references_and_keeps_values_by_tag() {
        use document::entities::{AttributeEntity, Insert};
        use document::types::Vector3;

        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        block_with_attributes(
            &mut app,
            "OLDBLOCK",
            &[("TAG", "Tag?", "OLD"), ("OLDONLY", "Old?", "X")],
        );
        block_with_attributes(
            &mut app,
            "NEWBLOCK",
            &[("TAG", "Tag?", "NEW"), ("NEWONLY", "New?", "D")],
        );

        let doc = &mut app.tabs[i].scene.document;
        let definition =
            match doc.get_entity(doc.block_records.get("OLDBLOCK").unwrap().entity_handles[1]) {
                Some(document::EntityType::AttributeDefinition(definition)) => definition.clone(),
                _ => panic!("the old definition is there"),
            };
        let mut insert = Insert::new("OLDBLOCK", Vector3::new(20.0, 10.0, 0.0));
        insert.attributes.push(AttributeEntity::from_definition(
            &definition,
            Some("KEPT".into()),
        ));
        let insert_handle = doc
            .add_entity(document::EntityType::Insert(insert))
            .unwrap();

        let _ = app.run_command_line("BLOCKREPLACE");
        let _ = app.feed_active_cmd("OLDBLOCK");
        let _ = app.feed_active_cmd("NEWBLOCK");
        assert!(app.command_line.last_error.is_none(), "{}", out(&app));

        let doc = &app.tabs[i].scene.document;
        let Some(document::EntityType::Insert(insert)) = doc.get_entity(insert_handle) else {
            panic!("the reference is still there");
        };
        assert_eq!(insert.block_name, "NEWBLOCK");
        let mut tags: Vec<_> = insert
            .attributes
            .iter()
            .map(|attribute| attribute.tag.as_str())
            .collect();
        tags.sort_unstable();
        assert_eq!(tags, ["NEWONLY", "TAG"], "old-only tags are dropped");
        let tag = insert
            .attributes
            .iter()
            .find(|attribute| attribute.tag == "TAG")
            .unwrap();
        assert_eq!(tag.value, "KEPT");
        let new_only = insert
            .attributes
            .iter()
            .find(|attribute| attribute.tag == "NEWONLY")
            .unwrap();
        assert_eq!(new_only.value, "D");
    }
}

#[cfg(test)]
mod array_zoom_extents_tests {
    use crate::app::OpenCADStudio;
    use crate::scene::assoc_array::AssocArraySpec;

    /// Zoom extents (ZOOM E, a double click of the middle button) frames
    /// every item of an associative array of a rectangle, not only the first:
    /// the items after the first render as stubs of one prototype, and those
    /// must count like the wires they stand for.
    #[test]
    fn zoom_extents_frames_every_item_of_a_rectangle_array() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let reply = app.automation_op(r#"{"op":"run","cmd":"RECTANG 0,0 10,5"}"#);
        assert_eq!(reply["status"], "completed", "{reply}");
        let i = app.active_tab;
        let rectangle = app.tabs[i]
            .scene
            .document
            .entities()
            .last()
            .map(|entity| entity.common().handle)
            .unwrap();
        app.handle_assoc_array(
            vec![rectangle],
            AssocArraySpec::Rectangular {
                columns: 4,
                rows: 4,
                column_spacing: 20.0,
                row_spacing: 30.0,
            },
            None,
        );

        app.tabs[i].scene.fit_all();
        let (min, max) = app.tabs[i]
            .scene
            .camera
            .borrow()
            .fitted_model_bounds()
            .expect("zoom extents fitted the drawing");
        assert!(min.abs_diff_eq(glam::DVec3::ZERO, 1e-9), "min {min:?}");
        assert!(
            max.abs_diff_eq(glam::DVec3::new(70.0, 95.0, 0.0), 1e-9),
            "max {max:?}, want the last item's far corner (70, 95)"
        );
    }
}

/// Batch 5: the missions that were still open on HEAD.
#[cfg(test)]
mod batch5_tests {
    use crate::app::OpenCADStudio;

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn classic_insert_advertises_its_point_keywords_as_structured_options() {
        use crate::command::CadCommand;
        use crate::modules::insert::insert_block::InsertBlockCommand;

        let command = InsertBlockCommand::classic_for_block(
            "ATTREQ_BP".to_string(),
            Vec::new(),
            String::new(),
        );
        let options = command.options();
        let keywords: Vec<&str> = options.iter().map(|option| option.keyword.as_str()).collect();
        assert_eq!(keywords, ["B", "S", "X", "Y", "Z", "R", "E", "RE"]);
        let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
        assert!(labels.contains(&"Basepoint"));
        assert!(labels.contains(&"Scale"));
        assert!(labels.contains(&"Rotate"));
        assert!(labels.contains(&"Explode"));
        assert!(labels.contains(&"Repeat"));

        let scale_options = InsertBlockCommand::classic_for_block(
            "ATTREQ_BP".to_string(),
            Vec::new(),
            String::new(),
        );
        let step = {
            // Reach the ScaleX step: choose a point, then answer the prompts.
            let mut command = scale_options;
            let _ = command.on_point(glam::DVec3::ZERO);
            command
        };
        assert_eq!(step.options().len(), 2);
    }
    fn out(app: &OpenCADStudio) -> String {
        app.command_line
            .history_plain_text()
            .trim()
            .to_string()
    }

    #[test]
    fn dimstyle_variables_accepts_a_write_and_keeps_it() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTXT", "4.5"]"#);
        let out = out(&app);
        assert!(out.contains("DIMTXT = 4.5"), "{out}");
        let doc = &app.tabs[app.active_tab].scene.document;
        let style = doc.dim_styles.get("ISO-25").expect("ISO-25 exists");
        assert_eq!(style.dimtxt, 4.5);
    }

    #[test]
    fn dimstyle_compare_reports_the_differing_variables() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["COMPARE", "ISO-25", "Standard"]"#);
        let out = out(&app);
        assert!(out.contains("DIMTXT"), "{out}");
        assert!(out.contains("DIMASZ"), "{out}");
    }

    #[test]
    fn associative_array_honours_the_base_point() {
        use document::Entity as _;

        let mut app = new_app();
        let before: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .find(|handle| !before.contains(handle))
            .unwrap();
        app.handle_assoc_array(
            vec![line],
            crate::scene::assoc_array::AssocArraySpec::Rectangular {
                columns: 2,
                rows: 2,
                column_spacing: 20.0,
                row_spacing: 20.0,
            },
            Some(glam::DVec3::new(50.0, 50.0, 0.0)),
        );
        let inserts: Vec<document::types::Vector3> = app
            .tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.insert_point),
                _ => None,
            })
            .collect();
        assert!(
            inserts.contains(&document::types::Vector3::new(50.0, 50.0, 0.0)),
            "the array insert is at the named base point: {inserts:?}"
        );

        let before: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .find(|handle| !before.contains(handle))
            .unwrap();
        app.handle_assoc_array(
            vec![line],
            crate::scene::assoc_array::AssocArraySpec::Rectangular {
                columns: 2,
                rows: 2,
                column_spacing: 20.0,
                row_spacing: 20.0,
            },
            None,
        );
        let inserts: Vec<document::types::Vector3> = app
            .tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.insert_point),
                _ => None,
            })
            .collect();
        assert!(
            inserts.contains(&document::types::Vector3::new(0.0, 0.0, 0.0)),
            "the default anchors at the source's bounding corner: {inserts:?}"
        );
    }

    #[test]
    fn setbylayer_reset_is_scriptable() {
        use document::Entity as _;

        let mut app = new_app();
        let reply = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .next()
            .unwrap()
            .common()
            .handle;
        // An empty selection is reported, not silently ignored.
        let empty = app.automation_op(r#"{"op":"run","cmd":"-SETBYLAYER"}"#);
        let text = out(&app);
        assert!(text.contains("SETBYLAYER"), "{empty}: {text}");

        let _ = app.automation_op(r#"{"op":"run","cmd":"SELECTALL"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"CHPROP COLOR 1"}"#);
        let pre = app.tabs[i].scene.document.get_entity(line).unwrap();
        assert_eq!(pre.common().color, document::types::Color::Index(1));

        let _ = app.automation_op(r#"{"op":"run","cmd":"-SETBYLAYER"}"#);
        let post = app.tabs[i].scene.document.get_entity(line).unwrap();
        assert_eq!(post.common().color, document::types::Color::ByLayer);
        let text = out(&app);
        assert!(text.contains("reset 1 entity"), "{text}");
    }
}
/// Batch 6: per-viewport layer overrides and other gaps closed on HEAD.
#[cfg(test)]
mod batch6_tests {
    use super::*;
    use crate::app::OpenCADStudio;

    fn out_text(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn add_layer(app: &mut OpenCADStudio, name: &str, color: document::types::Color) -> document::Handle {
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let mut layer = document::tables::Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.color = color;
        let handle = layer.handle;
        doc.layers.add(layer).unwrap();
        handle
    }

    /// Add a layout (with its page setup) and a viewport to it; call again
    /// with the same name to add another viewport to that layout.
    fn add_viewport(app: &mut OpenCADStudio, layout: &str) -> document::Handle {
        let i = app.active_tab;
        if app.tabs[i].scene.add_layout(layout).is_ok() {
            app.tabs[i].scene.initialize_layout_page_setup(layout);
        }
        let next_id = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Viewport(vp) => Some(vp.id),
                _ => None,
            })
            .max()
            .map(|id| id + 1)
            .unwrap_or(2);
        let mut viewport = document::entities::Viewport::new();
        viewport.center = document::types::Vector3::new(0.0, 0.0, 0.0);
        viewport.width = 200.0;
        viewport.height = 150.0;
        viewport.id = next_id;
        viewport.status = document::entities::ViewportStatusFlags::default_on();
        app.tabs[i]
            .scene
            .document
            .add_entity_to_layout(document::EntityType::Viewport(Box::new(viewport)), layout)
            .expect("the layout accepts a viewport")
    }

    #[test]
    fn vplayer_sets_colour_linetype_lineweight_and_alpha_overrides() {
        use document::objects::KnownXRecordKind as Kind;
        type Channel = crate::command::VpOverrideChannel;

        let mut app = new_app();
        let i = app.active_tab;
        let walls = add_layer(&mut app, "WALLS", document::types::Color::Index(1));
        let viewport = add_viewport(&mut app, "Sheet1");
        let other_viewport = add_viewport(&mut app, "Sheet1");

        // COLOR
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Color, "3");
        let color_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportColorOverride,
        );
        assert_eq!(color_overrides, vec![(viewport, document::objects::XRecordValue::Int32(0xC300_0003u32 as i32))]);
        assert!(
            !color_overrides.iter().any(|(handle, _)| *handle == other_viewport),
            "viewport 2 must not carry the override: {color_overrides:?}"
        );

        // LINETYPE (dashed ships in the default linetypes)
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Linetype, "DASHED");
        let line_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportLinetypeOverride,
        );
        let lt_handle = line_overrides.iter().next().cloned();
        assert!(lt_handle.is_some(), "{line_overrides:?}");

        // LINEWEIGHT
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Lineweight, "0.53");
        let lw_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportLineweightOverride,
        );
        assert!(lw_overrides
            .iter()
            .any(|(handle, value)| *handle == viewport && *value == document::objects::XRecordValue::Int32(53)));

        // ALPHA (77% transparent)
        app.handle_vp_layer_override(viewport, "WALLS", Channel::Alpha, "77");
        let alpha_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            Kind::LayerViewportAlphaOverride,
        );
        assert!(alpha_overrides.iter().any(|(handle, _)| *handle == viewport));
    }

    #[test]
    fn vplayer_rejects_a_bad_value_and_changes_nothing() {
        type Channel = crate::command::VpOverrideChannel;

        let mut app = new_app();
        let i = app.active_tab;
        let walls = add_layer(&mut app, "WALLS", document::types::Color::Index(1));
        let viewport = add_viewport(&mut app, "Sheet2");

        app.handle_vp_layer_override(viewport, "WALLS", Channel::Color, "not-a-color");
        let color_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            document::objects::KnownXRecordKind::LayerViewportColorOverride,
        );
        assert!(color_overrides.is_empty(), "{color_overrides:?}");

        app.handle_vp_layer_override(viewport, "WALLS", Channel::Linetype, "NoSuchLinetype");
        let lt_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            document::objects::KnownXRecordKind::LayerViewportLinetypeOverride,
        );
        assert!(lt_overrides.is_empty(), "{lt_overrides:?}");

        app.handle_vp_layer_override(viewport, "WALLS", Channel::Alpha, "99");
        let alpha_overrides = app.tabs[i].scene.document.layer_viewport_overrides(
            walls,
            document::objects::KnownXRecordKind::LayerViewportAlphaOverride,
        );
        assert!(alpha_overrides.is_empty(), "{alpha_overrides:?}");
    }

    #[test]
    fn vplayer_freeze_stays_in_the_viewport_and_leaves_the_layer_thawed() {
        let mut app = new_app();
        let i = app.active_tab;
        let walls = add_layer(&mut app, "WALLS", document::types::Color::Index(1));
        let viewport = add_viewport(&mut app, "Sheet3");
        let other_viewport = add_viewport(&mut app, "Sheet3");

        app.handle_vp_layer_update(viewport, vec!["WALLS".into()], vec![]);

        // VP Freeze is a separate property: the global layer stays thawed.
        let global = app
            .tabs[i]
            .scene
            .document
            .layers
            .get("WALLS")
            .expect("the layer exists");
        assert!(!global.flags.frozen, "the layer itself must stay thawed");
        let frozen_of = |app: &OpenCADStudio, handle: document::Handle| match app.tabs[i].scene.document.get_entity(handle) {
            Some(document::EntityType::Viewport(vp)) => vp.frozen_layers.clone(),
            other => panic!("expected a viewport, got {other:?}"),
        };
        assert!(
            frozen_of(&app, viewport).contains(&walls),
            "viewport 1 carries the VP freeze"
        );
        assert!(
            !frozen_of(&app, other_viewport).contains(&walls),
            "viewport 2 is unchanged"
        );

        // Thawing takes the freeze out of the viewport again.
        app.handle_vp_layer_update(viewport, vec![], vec!["WALLS".into()]);
        assert!(
            !frozen_of(&app, viewport).contains(&walls),
            "viewport 1 is thawed again"
        );
    }

    #[test]
    fn dimstyle_variables_dumps_the_variable_table() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25"]"#);
        let out = out_text(&app);
        // The history holds 64 entries, so the listing arrives as a suffix of
        // the 79-line dump; the complete content is checked in the module's
        // unit tests. Here the trailing rows must be there, aligned, with
        // current values.
        let rows = crate::app::dim_variables::DIM_VARIABLES
            .iter()
            .filter(|variable| out.contains(variable.name))
            .count();
        assert!(rows >= 60, "expected the tail of the table, got {rows}: {out}");
        let line = out
            .lines()
            .find(|line| line.trim_start().starts_with("DIMTXT"))
            .expect("the DIMTXT row prints");
        assert!(line.contains("2.5"), "{line}");
        let line = out
            .lines()
            .find(|line| line.trim_start().starts_with("DIMASZ"))
            .expect("the DIMASZ row prints");
        assert!(line.contains("2.5"), "{line}");
        assert!(
            out.contains("Dimension text height"),
            "descriptions print: {out}"
        );
    }

    #[test]
    fn dimstyle_create_with_variables_set_current_marks_new_dimensions() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.run_command_line(r#"-DIMSTYLE ["SAVE", "SHEET-1-50"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "SHEET-1-50", "DIMTXT", "2.5"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["RESTORE", "SHEET-1-50"]"#);
        let out = out_text(&app);
        assert!(out.contains("SHEET-1-50"), "{out}");
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .header
                .current_dimstyle_name,
            "SHEET-1-50"
        );

        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let dimension = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Dimension(dimension) => Some(dimension),
                _ => None,
            })
            .expect("the DIMLINEAR command placed a dimension");
        assert_eq!(
            dimension.base().style_name,
            "SHEET-1-50",
            "the new dimension carries the new style"
        );
    }

    #[test]
    fn blocks_palette_menu_renames_a_definition_and_rekeys_its_references() {
        use crate::app::Message;
        use crate::ui::window::block_palette::{BlockPaletteMsg, Item, MenuAction};

        let mut app = new_app();
        let i = app.active_tab;
        {
            let doc = &mut app.tabs[i].scene.document;
            let mut record = document::tables::BlockRecord::new("VALVE");
            record.handle = doc.allocate_handle();
            let owner = record.handle;
            doc.block_records.add(record).unwrap();
            let mut line = document::entities::Line::new();
            line.start = document::types::Vector3::ZERO;
            line.end = document::types::Vector3::new(10.0, 0.0, 0.0);
            line.common.owner_handle = owner;
            let line_handle = doc.add_entity(document::EntityType::Line(line)).unwrap();
            doc.block_records.get_mut("VALVE").unwrap().entity_handles = vec![line_handle];
            let insert = document::entities::Insert::new(
                "VALVE",
                document::types::Vector3::new(5.0, 5.0, 0.0),
            );
            let _ = doc.add_entity(document::EntityType::Insert(insert)).unwrap();
        }

        // Right-click the current-drawing entry and choose Rename Block.
        let _ = app.update(Message::BlockPalette(BlockPaletteMsg::Menu(
            Item::Current("VALVE".into()),
            MenuAction::Rename,
        )));
        let out = out_text(&app);
        assert!(out.contains("Enter new block name"), "{out}");

        // Type the new name; nothing happens until it is submitted.
        let _ = app.feed_active_cmd("GATE-VALVE");
        let out = out_text(&app);
        assert!(out.contains("RENAME"), "{out}");
        let doc = &app.tabs[i].scene.document;
        assert!(doc.block_records.get("VALVE").is_none(), "old name is gone");
        assert!(
            doc.block_records.get("GATE-VALVE").is_some(),
            "the new name exists in the table"
        );
        let insert = doc
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert),
                _ => None,
            })
            .expect("the reference exists");
        assert_eq!(insert.block_name, "GATE-VALVE", "the reference follows");
    }

    #[test]
    fn blocks_palette_rename_cancelled_with_empty_input_changes_nothing() {
        use crate::app::Message;
        use crate::ui::window::block_palette::{BlockPaletteMsg, Item, MenuAction};

        let mut app = new_app();
        let i = app.active_tab;
        {
            let doc = &mut app.tabs[i].scene.document;
            let mut record = document::tables::BlockRecord::new("VALVE");
            record.handle = doc.allocate_handle();
            doc.block_records.add(record).unwrap();
        }

        let _ = app.update(Message::BlockPalette(BlockPaletteMsg::Menu(
            Item::Current("VALVE".into()),
            MenuAction::Rename,
        )));
        assert!(app.tabs[i].active_cmd.is_some(), "the rename command starts");
        let _ = app.feed_active_cmd("");
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .contains("VALVE"),
            "the empty input leaves the definition alone"
        );
    }

    #[test]
    fn dimstyle_compare_reports_all_differing_variables() {
        let mut app = new_app();
        let _ = app.run_command_line(r#"-DIMSTYLE ["SAVE", "SHEET-A"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "SHEET-A", "DIMTXT", "9.0"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["COMPARE", "ISO-25", "SHEET-A"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMTXT"), "{out}");

        let _ = app.run_command_line(r#"-DIMSTYLE ["COMPARE", "ISO-25", "ISO-25"]"#);
        let out = out_text(&app);
        assert!(out.contains("identical"), "{out}");
    }

    #[test]
    fn dimstyle_variables_sets_dimtxt_to_3_and_keeps_it() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTXT", "3.0"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMTXT = 3"), "{out}");
        let style = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .expect("ISO-25 exists");
        assert_eq!(style.dimtxt, 3.0);
    }

    #[test]
    fn dimstyle_variables_rejects_an_out_of_range_value_and_keeps_the_style() {
        let mut app = new_app();
        let i = app.active_tab;
        let before = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .unwrap()
            .dimtxt;
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTXT", "-1"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMSTYLE"), "{out}");
        assert!(out.contains("must be greater than 0"), "{out}");
        let after = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .unwrap()
            .dimtxt;
        assert_eq!(before, after, "the refused value is not written");

        // And a boolean keyword with a wrong kind of value is refused too.
        let _ = app.run_command_line(r#"-DIMSTYLE ["VARIABLES", "ISO-25", "DIMTOL", "banana"]"#);
        let out = out_text(&app);
        assert!(out.contains("DIMSTYLE"), "{out}");
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .dim_styles
                .get("ISO-25")
                .unwrap()
                .dimtol,
            false,
            "the refused value is not written"
        );
    }

    #[test]
    fn dimstyle_rename_and_delete_through_the_command_line() {
        let mut app = new_app();
        let i = app.active_tab;
        // Create a style from the current one, rename it, restyle a
        // dimension to it, and delete it after deleting the dimension.
        let _ = app.run_command_line(r#"-DIMSTYLE ["SAVE", "SHEET-1-50"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["RENAME", "SHEET-1-50", "SHEET_1_50"]"#);
        let out = out_text(&app);
        assert!(out.contains("SHEET_1_50"), "{out}");
        assert!(app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .get("SHEET-1-50")
            .is_none());
        assert!(app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .contains("SHEET_1_50"));

        // Set it current, confirm it is current, then delete it.
        let _ = app.run_command_line(r#"-DIMSTYLE ["RESTORE", "SHEET_1_50"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["DELETE", "SHEET_1_50"]"#);
        let out = out_text(&app);
        assert!(out.contains("current style"), "{out}");
        assert!(app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .contains("SHEET_1_50"), "the current style is not deleted");

        let _ = app.run_command_line(r#"-DIMSTYLE ["RESTORE", "ISO-25"]"#);
        let _ = app.run_command_line(r#"-DIMSTYLE ["DELETE", "SHEET_1_50"]"#);
        let out = out_text(&app);
        assert!(out.contains("deleted"), "{out}");
        assert!(!app
            .tabs[i]
            .scene
            .document
            .dim_styles
            .contains("SHEET_1_50"));
    }
}

/// Batch 7: remaining gaps found in the HEAD audit (evidence-only missions
/// and the four WEAK ones), implemented on HEAD.
#[cfg(test)]
mod batch7_tests {
    use super::*;
    use crate::app::OpenCADStudio;

    fn out_text(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn dimoverride_list_reports_the_override_and_clear_returns_the_style() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let dimension = app
            .tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Dimension(dimension) => Some(dimension.base().common.handle),
                _ => None,
            })
            .expect("DIMLINEAR placed a dimension");
        app.tabs[i].scene.select_entities(&[dimension]);

        let _ = app.run_command_line("DIMOVERRIDE_APPLY DIMTXT=4");
        let out = out_text(&app);
        assert!(out.contains("DIMOVERRIDE: updated"), "{out}");

        let _ = app.run_command_line("DIMOVERRIDELIST");
        let out = out_text(&app);
        assert!(out.contains("DIMTXT = 4"), "{out}");
        assert!(out.contains("Dimension"), "{out}");

        let _ = app.run_command_line("DIMOVERRIDE_APPLY CLEAR");
        let _ = app.run_command_line("DIMOVERRIDELIST");
        let out = out_text(&app);
        assert!(out.contains("No overrides"), "{out}");
        // The style itself never changed.
        let style = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .unwrap();
        assert_eq!(style.dimtxt, 2.5);
    }

    #[test]
    fn mleader_edit_adds_then_removes_a_leader_and_reports_both_vertex_lists() {
        let mut app = new_app();
        let i = app.active_tab;
        let mut leader = document::entities::MultiLeader::with_text(
            "office note",
            document::types::Vector3::new(20.0, 0.0, 0.0),
            vec![
                document::types::Vector3::ZERO,
                document::types::Vector3::new(5.0, 5.0, 0.0),
            ],
        );
        leader.text_height = 0.18;
        leader.context.text_height = 0.18;
        let handle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::MultiLeader(Box::new(leader)));

        // The initial note: one leader, two vertices.
        let q = app.automation_op(&format!(
            r#"{{"op":"query","handle":"{handle:X}","detail":"full"}}"#
        ));
        assert_eq!(q["entities"][0]["leader_count"], 1, "{q}");
        let leaders = q["entities"][0]["leaders"].as_array().unwrap();
        assert_eq!(leaders[0]["points"].as_array().unwrap().len(), 2, "{q}");

        // Add a second leader through the edit command.
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{handle:X}"]}}"#
        ));
        let _ = app.run_command_line("MLEADEREDIT");
        assert!(app.tabs[i].active_cmd.is_some());
        let _ = app.feed_active_cmd("40,10");
        let _ = app.feed_active_cmd("");
        let q = app.automation_op(&format!(
            r#"{{"op":"query","handle":"{handle:X}","detail":"full"}}"#
        ));
        assert_eq!(q["entities"][0]["leader_count"], 2, "{q}");
        let leaders = q["entities"][0]["leaders"].as_array().unwrap();
        assert_eq!(leaders.len(), 2, "{q}");
        let lists: Vec<&Vec<serde_json::Value>> = leaders
            .iter()
            .map(|leader| leader["points"].as_array().unwrap())
            .collect();
        assert_eq!(lists[0].len(), 2, "{q}");
        assert_eq!(lists[1].len(), 1, "{q}");
        assert_ne!(lists[0], lists[1], "{q}");

        // Remove the first leader — one leader remains.
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{handle:X}"]}}"#
        ));
        let _ = app.run_command_line("MLEADEREDIT");
        let _ = app.feed_active_cmd("R");
        let _ = app.feed_active_cmd("0,0");
        let _ = app.feed_active_cmd("");
        let q = app.automation_op(&format!(
            r#"{{"op":"query","handle":"{handle:X}","detail":"full"}}"#
        ));
        assert_eq!(q["entities"][0]["leader_count"], 1, "{q}");
    }

    #[test]
    fn sheet_set_with_a_custom_property_and_a_field() {
        use document::objects::{Dictionary, ObjectType};
        use serde_json::json;

        fn request(app: &mut OpenCADStudio, mut req: serde_json::Value) -> serde_json::Value {
            let state = app.control_state();
            req["protocol"] = json!(1);
            req["document_id"] = state["document_id"].clone();
            req["revision"] = state["revision"].clone();
            req["client_id"] = json!("test");
            if req.get("request_id").is_none() {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                req["request_id"] = json!(format!(
                    "req-{}",
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                ));
            }
            let (r, task) = app.control_request(req.clone());
            app.drive_headless_task(task).unwrap();
            if matches!(r["status"].as_str(), Some("accepted" | "running")) {
                app.control_request(json!({"op":"operation","request_id":req["request_id"]}))
                    .0
            } else {
                r
            }
        }
        let sheet = |app: &mut OpenCADStudio, value: &str| {
            request(app, json!({"op":"action","name":"sheet_set","value":value}))
        };

        let dir = std::env::temp_dir().join(format!("ocs-ss-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let mut app = new_app();

        // Create the set through the wizard.
        let _ = sheet(&mut app, "new");
        let _ = sheet(&mut app, "existing=0");
        let _ = sheet(&mut app, "name=PROJECTSET");
        let _ = sheet(&mut app, &format!("folder={}", dir.display()));
        let _ = sheet(&mut app, "ok");

        // Two subsets.
        let _ = sheet(&mut app, "menu=new_subset:set");
        let _ = sheet(&mut app, "title=Architectural");
        let _ = sheet(&mut app, "ok");
        let _ = sheet(&mut app, "menu=new_subset:set");
        let _ = sheet(&mut app, "title=Structural");
        let _ = sheet(&mut app, "ok");

        // Three sheets.
        for (subset, number, title) in [
            ("Architectural", "A-101", "Plans"),
            ("Architectural", "A-102", "Sections"),
            ("Structural", "S-201", "Framing"),
        ] {
            let _ = sheet(&mut app, &format!("menu=new_sheet:{subset}"));
            let _ = sheet(&mut app, &format!("number={number}"));
            let _ = sheet(&mut app, &format!("title={title}"));
            let _ = sheet(&mut app, &format!("folder={}", dir.display()));
            let _ = sheet(&mut app, "ok");
        }

        // A custom property on the set.
        let _ = sheet(&mut app, "menu=properties:set");
        let _ = sheet(&mut app, "add_custom");
        let _ = sheet(&mut app, "custom_name=PROJECT");
        let _ = sheet(&mut app, "custom_owner=0");
        let _ = sheet(&mut app, "add_custom_ok");
        let _ = sheet(&mut app, "custom=0:The Aragorn Project");
        let _ = sheet(&mut app, "ok");

        // The set round-trips from disk with both subsets, the three sheets
        // and the custom property.
        let dst = dir.join("PROJECTSET.dst");
        assert!(dst.is_file(), "the wizard wrote {}", dst.display());
        let db = crate::app::commands::sheet_set::read_set(&dst.to_string_lossy())
            .expect("the set round-trips from disk");
        let subset_count = db
            .sheet_set()
            .children
            .iter()
            .filter(|child| child.name == "AcSmSubset")
            .count();
        let sheet_count = db
            .sheet_set()
            .children
            .iter()
            .filter(|child| child.name == "AcSmSubset")
            .map(|subset| {
                subset
                    .children
                    .iter()
                    .filter(|child| child.name == "AcSmSheet")
                    .count()
            })
            .sum::<usize>();
        assert_eq!(subset_count, 2, "two subsets");
        assert_eq!(sheet_count, 3, "three sheets across the subsets");
        let custom = document::sheet_set::custom_properties(db.sheet_set());
        assert!(
            custom
                .iter()
                .any(|(name, value, _)| name == "PROJECT" && value == "The Aragorn Project"),
            "{custom:?}"
        );

        // A field on the first sheet's layout reads the custom property.
        let sheet_path = dir.join("A-101 Plans.dwg");
        assert!(sheet_path.is_file());
        let mut doc = crate::io::load_file(&sheet_path).expect("the sheet drawing loads");
        let host = doc
            .add_entity_to_layout(
                document::EntityType::Point(document::entities::Point::new()),
                "A-101 Plans",
            )
            .expect("the layout accepts a point");
        let root_h = doc.allocate_handle();
        let fields_h = doc.allocate_handle();
        let text_h = doc.allocate_handle();
        let child_h = doc.allocate_handle();
        let mut root = Dictionary::new();
        root.handle = root_h;
        root.owner = host;
        root.add_entry("ACAD_FIELD", fields_h);
        let mut fields = Dictionary::new();
        fields.handle = fields_h;
        fields.owner = root_h;
        fields.add_entry("TEXT", text_h);
        doc.objects.insert(root_h, ObjectType::Dictionary(root));
        doc.objects.insert(fields_h, ObjectType::Dictionary(fields));
        doc.fields.insert(
            text_h,
            document::document::FieldDef {
                handle: text_h,
                owner: fields_h,
                evaluator: "_text".to_string(),
                code: r"%<\_FldIdx 0>%".to_string(),
                objects: vec![],
            },
        );
        doc.fields.insert(
            child_h,
            document::document::FieldDef {
                handle: child_h,
                owner: text_h,
                evaluator: "AcSm".to_string(),
                code: r"\AcSm SheetSet.PROJECT".to_string(),
                objects: vec![],
            },
        );

        // The custom property field reads the set through the open sheet.
        let resolved = crate::entities::field::resolve(&doc, host);
        assert_eq!(
            resolved.as_deref(),
            Some("The Aragorn Project"),
            "the field reads the set's custom property on the sheet's layout"
        );

        // And the other sheet's own field shows its sheet number instead.
        doc.fields.insert(
            child_h,
            document::document::FieldDef {
                handle: child_h,
                owner: text_h,
                evaluator: "AcSm".to_string(),
                code: r"\AcSm Sheet.Number".to_string(),
                objects: vec![],
            },
        );
        let resolved = crate::entities::field::resolve(&doc, host);
        assert_eq!(
            resolved.as_deref(),
            Some("A-101"),
            "the sheet field reads the sheet number"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> document::Handle {
        use document::entities::Line;
        use document::types::Vector3;
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let line = Line::from_points(Vector3::new(x1, y1, 0.0), Vector3::new(x2, y2, 0.0));
        doc.add_entity(document::EntityType::Line(line)).unwrap()
    }

    fn reported_lines(app: &OpenCADStudio, before: usize) -> Vec<String> {
        app.command_line.history[before..]
            .iter()
            .map(|entry| entry.text.trim().to_string())
            .collect()
    }

    fn breakdown_sum(said: &[String]) -> usize {
        said.iter()
            .filter_map(|text| {
                let mut parts = text.splitn(2, ' ');
                let (n, rest) = (parts.next()?, parts.next()?);
                n.parse::<usize>()
                    .ok()
                    .filter(|_| !rest.contains("constraint(s) applied"))
            })
            .sum()
    }

    #[test]
    fn autoconstrain_reports_the_breakdown_for_a_rectangle() {
        let mut app = new_app();
        let i = app.active_tab;
        let a = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let b = add_line(&mut app, 100.0, 0.0, 100.0, 50.0);
        let c = add_line(&mut app, 100.0, 50.0, 0.0, 50.0);
        let d = add_line(&mut app, 0.0, 50.0, 0.0, 0.0);
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{a:X}","{b:X}","{c:X}","{d:X}"]}}"#
        ));
        let before = app.command_line.history.len();
        let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
        assert_eq!(inferred["ok"], true, "{inferred}");
        let said = reported_lines(&app, before);
        let out = said.join("\n");
        assert!(out.contains("constraint(s) applied"), "{out}");
        let reported = said
            .iter()
            .find_map(|text| text.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()))
            .expect("the total line prints");
        assert_eq!(breakdown_sum(&said), reported, "{out}");
        assert!(out.contains("Coincident"), "{out}");
        assert!(out.contains("Horizontal"), "{out}");
        assert!(out.contains("Vertical"), "{out}");
    }

    #[test]
    fn autoconstrain_honours_the_enabled_kinds() {
        let mut app = new_app();
        let i = app.active_tab;
        app.auto_constrain_settings
            .enabled
            .retain(|kind| *kind != crate::app::settings::AutoConstraintKind::Coincident);
        let a = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let b = add_line(&mut app, 100.0, 0.0, 100.0, 50.0);
        let c = add_line(&mut app, 100.0, 50.0, 0.0, 50.0);
        let d = add_line(&mut app, 0.0, 50.0, 0.0, 0.0);
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{a:X}","{b:X}","{c:X}","{d:X}"]}}"#
        ));
        let before = app.command_line.history.len();
        let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
        assert_eq!(inferred["ok"], true, "{inferred}");
        let said = reported_lines(&app, before);
        let out = said.join("\n");
        assert!(out.contains("constraint(s) applied"), "{out}");
        assert!(!out.contains("Coincident"), "{out}");
        assert!(out.contains("Horizontal"), "{out}");
        assert!(out.contains("Vertical"), "{out}");
    }

    #[test]
    fn autoconstrain_honours_the_distance_tolerance() {
        // Two lines whose endpoints are 0.02 apart: within the default
        // tolerance they become coincident; with a 1e-6 tolerance they do not.
        for (tolerance, expect_coincident) in [(0.05, true), (1e-6, false)] {
            let mut app = new_app();
            app.auto_constrain_settings.distance_tolerance = tolerance;
            let i = app.active_tab;
            let a = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
            let b = add_line(&mut app, 100.0, 0.02, 200.0, 0.0);
            let _ = app.automation_op(&format!(
                r#"{{"op":"select","handles":["{a:X}","{b:X}"]}}"#
            ));
            let before = app.command_line.history.len();
            let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
            assert_eq!(inferred["ok"], true, "{inferred}");
            let said = reported_lines(&app, before);
            let out = said.join("\n");
            assert_eq!(out.contains("Coincident"), expect_coincident, "{out}");
        }
    }

    fn temp_named(stem: &str, extension: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ocs-{stem}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{stem}.{extension}"))
    }

    /// A fresh tab as a host drawing referencing `source`; returns the tab
    /// index and the reference's block-record handle.
    fn host_with_xref(
        app: &mut OpenCADStudio,
        source: &std::path::Path,
    ) -> (usize, document::Handle) {
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(
                app.tabs.len(),
            ));
        let idx = app.tabs.len() - 1;
        app.active_tab = idx;
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("-XREF");
        let _ = app.feed_active_cmd("ATTACH");
        let _ = app.feed_active_cmd(&source.display().to_string());
        let _ = app.feed_active_cmd("0,0");
        let _ = app.feed_active_cmd("1");
        let _ = app.feed_active_cmd("");
        let _ = app.feed_active_cmd("");
        let record = app.tabs[idx]
            .scene
            .document
            .block_records
            .iter()
            .find(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .map(|br| br.handle)
            .expect("the host carries the reference");
        (idx, record)
    }

    fn referenced_entity_count(
        app: &OpenCADStudio,
        tab: usize,
        record: document::Handle,
    ) -> usize {
        app.tabs[tab]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == record)
            .count()
    }

    #[test]
    fn saving_the_source_reloads_the_host_reference() {
        let source = temp_named("m071-src", "dwg");
        let mut app = new_app();

        // Backslashes would escape out of the JSON string; the resolver
        // normalises forward slashes.
        let source_fwd = source.display().to_string().replace('\\', "/");
        // Tab 0: the source drawing with one line.
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));

        let (host, record) = host_with_xref(&mut app, &source);
        assert_eq!(referenced_entity_count(&app, host, record), 1);

        // A second tab opens the source, gains a line and saves — the
        // host must reload the reference from disk.
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(9));
        let source_tab = app.tabs.len() - 1;
        app.active_tab = source_tab;
        let _ = app.automation_op(&format!(r#"{{"op":"open","path":"{source_fwd}"}}"#));
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 20,0"}"#);
        let _ = app.automation_op(r#"{"op":"save"}"#);

        assert_eq!(
            referenced_entity_count(&app, host, record),
            2,
            "the host reloaded the saved source"
        );
    }

    #[test]
    fn xopen_asks_to_pick_a_reference_and_resolves_it_under_the_point() {
        let source = temp_named("m071-pick", "dwg");
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source_fwd = source.display().to_string().replace('\\', "/");
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));

        let (host, _record) = host_with_xref(&mut app, &source);
        // No selection: XOPEN asks for a pick.
        let _ = app.run_command_line("XOPEN");
        assert!(
            app.tabs[host].active_cmd.is_some(),
            "the pick command starts"
        );
        let out = out_text(&app);
        assert!(out.contains("Select an external reference"), "{out}");
        // A pick on the reference under the point resolves to its source.
        let _ = app.feed_active_cmd("2.5,0");
        let out = out_text(&app);
        assert!(out.contains("XOPEN: opening"), "{out}");
        assert!(out.contains(&source.display().to_string()), "{out}");
    }

    #[test]
    fn xopen_reports_a_missing_source_and_leaves_the_host_alone() {
        let source = temp_named("m071-missing", "dwg");
        let mut app = new_app();

        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source_fwd = source.display().to_string().replace('\\', "/");
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));

        let (host, record) = host_with_xref(&mut app, &source);
        std::fs::remove_file(&source).expect("the source is removed for the test");

        let insert = app.tabs[host]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.common.handle),
                _ => None,
            })
            .expect("the host carries the reference insert");
        let _ = app.automation_op(&format!(r#"{{"op":"select","handles":["{insert:X}"]}}"#));
        let before = referenced_entity_count(&app, host, record);
        let _ = app.run_command_line("XOPEN");
        let out = out_text(&app);
        assert!(out.contains("cannot find"), "{out}");
        assert_eq!(
            referenced_entity_count(&app, host, record),
            before,
            "the missing-file refusal leaves the reference alone"
        );
    }

    use crate::command::CmdResult;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    /// M098: a constrained pair draws its constraint bar. The horizontal and
    /// coincident glyphs of the pair are placed and listed, so showing the bar
    /// surfaces what constrains the geometry.
    #[test]
    fn constraint_bar_lists_the_glyphs_of_a_constrained_pair() {
        let mut app = new_app();
        let i = app.active_tab;
        let first = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let second = add_line(&mut app, 40.0, 0.0, 60.0, 0.0);
        let scope = ParametricScope::ModelSpace;
        let set = app.tabs[i].scene.parametric_constraint_set_mut(scope);
        let horizontal = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(first), ParametricRef::whole(second)],
            None,
        );
        let coincident = set.add(
            ConstraintKind::Coincident,
            vec![ParametricRef::whole(first), ParametricRef::whole(second)],
            None,
        );
        app.tabs[i].scene.note_parametric_constraint_applied(scope, horizontal, 3);
        app.tabs[i].scene.note_parametric_constraint_applied(scope, coincident, 3);
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let placements = app.tabs[i].scene.constraint_glyph_placements_screen(
            scope,
            (800.0, 600.0),
            true,
            3,
            4095,
        );
        let labels: Vec<&str> = placements.iter().map(|p| p.3.as_str()).collect();
        assert!(!placements.is_empty(), "a constrained pair draws a bar");
        assert!(labels.iter().any(|label| *label == "—"), "{labels:?}");
        assert!(labels.iter().any(|label| *label == "≡"), "{labels:?}");
    }

    /// M098: hiding the bar removes its glyphs, and deleting the constraint
    /// the bar named frees it again — the bar's own remove path.
    #[test]
    fn constraint_bar_hide_removes_the_glyphs_and_delete_frees_the_constraint() {
        let mut app = new_app();
        let i = app.active_tab;
        let first = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(first)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let scope = ParametricScope::ModelSpace;
        let id = app.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .unwrap()
            .constraints[0]
            .id;

        let hidden = app.tabs[i].scene.set_parametric_constraint_visibility(
            scope,
            None,
            false,
            false,
        );
        assert_eq!(hidden, 1);
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        assert!(app.tabs[i]
            .scene
            .constraint_glyph_placements_screen(scope, (800.0, 600.0), true, 3, 4095)
            .is_empty());

        app.delete_parametric_constraint(id);
        assert!(app.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .unwrap()
            .constraints
            .is_empty());
        app.undo_steps(1);
        assert_eq!(
            app.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .unwrap()
                .constraints
                .len(),
            1
        );
    }

    /// The live status of one reference as the automation surface reports it.
    fn xref_status(app: &OpenCADStudio, name: &str) -> String {
        app.control_state()["xrefs"]
            .as_array()
            .and_then(|xs| xs.iter().find(|x| x["name"].as_str() == Some(name)))
            .and_then(|x| x["status"].as_str())
            .unwrap_or_default()
            .to_string()
    }

    /// The name of the host's external reference block record.
    fn xref_name(app: &OpenCADStudio, tab: usize) -> String {
        app.tabs[tab]
            .scene
            .document
            .block_records
            .iter()
            .find(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .map(|br| br.name.clone())
            .expect("the host carries a reference")
    }

    /// A saved one-line source drawing and a host that references it, so the
    /// reference loads from disk. Returns the app, the source path, the host
    /// tab, the reference's block record and its name.
    fn m074_host(
        stem: &str,
    ) -> (
        OpenCADStudio,
        std::path::PathBuf,
        usize,
        document::Handle,
        String,
    ) {
        let source = temp_named(stem, "dwg");
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source_fwd = source.display().to_string().replace('\\', "/");
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));
        let (host, record) = host_with_xref(&mut app, &source);
        let name = xref_name(&app, host);
        // Save the host so it has a folder: a relative reference path only
        // resolves (and only reloads) against a saved drawing.
        let host_path = source.with_file_name(format!("{stem}-host.dwg"));
        let host_fwd = host_path.display().to_string().replace('\\', "/");
        app.active_tab = host;
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{host_fwd}"}}"#));
        (app, source, host, record, name)
    }

    /// M074: unloading a reference reports the unloaded status and hides its
    /// merged geometry.
    #[test]
    fn xref_unload_reports_status_and_hides_geometry() {
        let (mut app, _source, host, record, name) = m074_host("m074-unload");
        assert_eq!(referenced_entity_count(&app, host, record), 1);
        assert_eq!(xref_status(&app, &name), "loaded");

        let _ = app.run_command_line(&format!("XREF Unload {name}"));
        let out = out_text(&app);
        assert!(out.contains("unloaded"), "{out}");
        assert_eq!(xref_status(&app, &name), "unloaded");
        assert_eq!(
            referenced_entity_count(&app, host, record),
            0,
            "the unloaded reference's merged geometry is hidden"
        );
    }

    /// M074: reloading clears the unloaded status and restores the geometry.
    #[test]
    fn xref_reload_reports_status_and_restores_geometry() {
        let (mut app, _source, host, record, name) = m074_host("m074-reload");
        let _ = app.run_command_line(&format!("XREF Unload {name}"));
        assert_eq!(xref_status(&app, &name), "unloaded");

        let _ = app.run_command_line(&format!("XREF Reload {name}"));
        let out = out_text(&app);
        assert!(out.contains("Reloaded"), "{out}");
        assert_eq!(xref_status(&app, &name), "loaded");
        assert_eq!(
            referenced_entity_count(&app, host, record),
            1,
            "the reloaded reference's geometry is back"
        );
    }

    /// M074: a source that disappears is reported not-found, on demand.
    #[test]
    fn xref_status_reports_a_missing_source() {
        let (mut app, source, _host, _record, name) = m074_host("m074-missing");
        assert_eq!(xref_status(&app, &name), "loaded");

        std::fs::remove_file(&source).expect("the source file is removed");
        let _ = app.run_command_line("XREF ?");
        let out = out_text(&app);
        assert!(out.contains("Not found"), "{out}");
        assert!(out.contains("not found"), "{out}");
        assert_eq!(xref_status(&app, &name), "not_found");
    }
}
