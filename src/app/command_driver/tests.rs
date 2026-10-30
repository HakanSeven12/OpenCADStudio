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

    fn line_angle_deg(app: &OpenCADStudio, handle: Handle) -> f64 {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(codec::EntityType::Line(l)) => (l.end.y - l.start.y)
                .atan2(l.end.x - l.start.x)
                .to_degrees(),
            other => panic!("expected a Line, got {other:?}"),
        }
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
            .add_entity(EntityType::Line(Line::from_points(
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
            ),
        );
        assert_eq!(
            app.block_definition
                .as_ref()
                .map(|s| s.base_point_y.as_str()),
            Some("0"),
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
