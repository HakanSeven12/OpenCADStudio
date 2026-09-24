#[cfg(test)]
mod command_replacement_tests {
    use super::super::*;
    use acadrust::entities::{Circle, Line};
    use acadrust::types::Vector3;

    #[test]
    fn one_to_one_edits_keep_identity_but_type_changes_do_not() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(acadrust::EntityType::Line(Line::from_points(
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
            vec![acadrust::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(3.0, 0.0, 0.0),
            ))],
        );
        assert_eq!(kept, vec![handle]);
        let edited = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(edited.common().owner_handle, owner);
        assert!(matches!(edited, acadrust::EntityType::Line(line) if line.end.x == 3.0));

        let allocated = app.replace_command_entity(
            tab,
            handle,
            vec![acadrust::EntityType::Circle(Circle::from_coords(
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
            .add_entity(acadrust::EntityType::Line(Line::from_points(
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
            entity: acadrust::EntityType::Line(Line::from_points(
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
        assert!(matches!(updated, acadrust::EntityType::Line(line) if line.end.x == 4.0));
    }
}

#[cfg(test)]
mod parametric_constraint_undo_tests {
    use super::super::*;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        app.tabs[app.active_tab]
            .scene
            .add_entity(acadrust::EntityType::Line(
                acadrust::entities::Line::from_points(
                    acadrust::types::Vector3::new(x1, y1, 0.0),
                    acadrust::types::Vector3::new(x2, y2, 0.0),
                ),
            ))
    }

    fn line_angle_deg(app: &OpenCADStudio, handle: Handle) -> f64 {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(l)) => (l.end.y - l.start.y)
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
            Some(acadrust::EntityType::Line(line)) => line.clone(),
            other => panic!("expected a Line, got {other:?}"),
        };
        let fixed = line(first);
        let moving = line(second);
        assert_eq!(fixed.start, acadrust::types::Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(fixed.end, acadrust::types::Vector3::new(10.0, 0.0, 0.0));
        assert!((moving.start - acadrust::types::Vector3::new(20.0, 0.0, 0.0)).length() < 1.0e-9);
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
        let (original_start, original_end, original_length) = match app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
        {
            Some(acadrust::EntityType::Line(line)) => (line.start, line.end, line.length()),
            other => panic!("expected a Line, got {other:?}"),
        };
        let direction = acadrust::types::Vector3::new(3.0, 4.0, 0.0).normalize();

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::whole(handle),
            ),
            direction,
            label: "Horizontal constraint",
        });

        let line = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line,
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
            Some(acadrust::EntityType::Line(line)) => line,
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
            Some(acadrust::EntityType::Line(line)) => line,
            other => panic!("expected a Line after redo, got {other:?}"),
        };
        assert!((line.end - line.start).normalize().cross(&direction).length() < 1.0e-7);
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
            direction: acadrust::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let line = |handle| match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Line(line)) => line,
            other => panic!("expected a Line, got {other:?}"),
        };
        assert_eq!(line(first).start, acadrust::types::Vector3::ZERO);
        assert!(line(second).start.y.abs() < 1.0e-8);
    }

    #[test]
    fn horizontal_minor_axis_rotates_an_ellipse_around_its_center() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut ellipse = acadrust::entities::Ellipse::default();
        ellipse.center = acadrust::types::Vector3::new(3.0, 7.0, 0.0);
        ellipse.major_axis = acadrust::types::Vector3::new(3.0, 4.0, 0.0);
        ellipse.minor_axis_ratio = 0.4;
        let handle = app.tabs[app.active_tab]
            .scene
            .add_entity(acadrust::EntityType::Ellipse(ellipse));

        let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
            kind: crate::scene::parametric_constraints::ConstraintKind::Horizontal,
            selection: crate::command::HorizontalConstraintSelection::Reference(
                ParametricRef::ellipse_minor_axis(handle),
            ),
            direction: acadrust::types::Vector3::UNIT_X,
            label: "Horizontal constraint",
        });

        let ellipse = match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(acadrust::EntityType::Ellipse(ellipse)) => ellipse,
            other => panic!("expected an Ellipse, got {other:?}"),
        };
        assert!((ellipse.center - acadrust::types::Vector3::new(3.0, 7.0, 0.0)).length() < 1.0e-9);
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

        let new_line = acadrust::EntityType::Line(acadrust::entities::Line::from_points(
            acadrust::types::Vector3::new(5.0, 0.0, 0.0), // exactly on `existing`'s end point
            acadrust::types::Vector3::new(10.0, 0.0, 0.0),
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
            Some(acadrust::EntityType::Line(l)) => (l.start, l.end),
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
            Some(acadrust::EntityType::Line(l)) => (l.start, l.end),
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
            Some(acadrust::EntityType::Line(line)) => line.length(),
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
            Some(acadrust::EntityType::Line(l)) => (l.start, l.end),
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
        let Some(acadrust::EntityType::Line(first)) =
            app.tabs[app.active_tab].scene.document.get_entity(a)
        else {
            panic!("expected first line");
        };
        let Some(acadrust::EntityType::Line(second)) =
            app.tabs[app.active_tab].scene.document.get_entity(b)
        else {
            panic!("expected second line");
        };
        assert!(
            (first.end - acadrust::types::Vector3::new(5.0, 0.0, 0.0)).length() < 1.0e-7
        );
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
    use acadrust::types::Vector3;

    fn sweep_source_presence(value: i16, mode: crate::command::ExtrudeMode) -> (bool, bool) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut circle = acadrust::Circle::new();
        circle.radius = 2.0;
        let profile = app.tabs[i]
            .scene
            .add_entity(acadrust::EntityType::Circle(circle));
        let path =
            app.tabs[i]
                .scene
                .add_entity(acadrust::EntityType::Line(acadrust::Line::from_points(
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
        });

        let created = app.tabs[i]
            .scene
            .document
            .entities()
            .any(|entity| match mode {
                crate::command::ExtrudeMode::Solid => {
                    matches!(entity, acadrust::EntityType::Solid3D(_))
                }
                crate::command::ExtrudeMode::Surface => {
                    matches!(entity, acadrust::EntityType::Surface(_))
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
        assert_eq!(
            sweep_source_presence(2, crate::command::ExtrudeMode::Surface),
            (false, false),
        );
        assert_eq!(
            sweep_source_presence(3, crate::command::ExtrudeMode::Surface),
            (false, true),
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod thicken_tests {
    use super::super::*;
    use acadrust::entities::{Surface, SurfaceKind};
    use cadkernel::geom2d::{Circle, Curve, NurbsCurve};
    use cadkernel::space::{Parameterization, Plane};

    #[test]
    fn thicken_preserves_sources_and_round_trips_results_with_one_undo() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let body = cadkernel::brep::planar_region(
            Plane::XY,
            &[vec![Curve::Circle(Circle {
                centre: [0.0; 2],
                radius: 3.0,
            })]],
        )
        .unwrap();
        let source = app.add_surface_model(
            acadrust::EntityType::Surface(Surface::new(SurfaceKind::Plane)),
            body,
        );
        assert!(!source.is_null());
        let i = app.active_tab;
        let invalid = app.tabs[i]
            .scene
            .add_entity(acadrust::EntityType::Surface(Surface::new(
                SurfaceKind::Generic,
            )));
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
                matches!(entity, acadrust::EntityType::Solid3D(_)).then_some(entity.common().handle)
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
            cadkernel::brep::analytic_mass_properties(&app.tabs[i].scene.solid_models[&result])
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
        let actual = cadkernel::brep::analytic_mass_properties(&restored.solid_models[&result])
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
            cadkernel::brep::extrude_surface(Plane::XY, &[Curve::Nurbs(profile)], [0.0, 0.0, 2.0])
                .unwrap();
        let source = app.add_surface_model(
            acadrust::EntityType::Surface(Surface::new(SurfaceKind::Generic)),
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
                matches!(entity, acadrust::EntityType::Solid3D(_)).then_some(entity.common().handle)
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
    use acadrust::entities::ViewportRenderMode as Mode;

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
}
