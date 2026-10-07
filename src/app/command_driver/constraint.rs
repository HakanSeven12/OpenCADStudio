use super::*;

impl OpenCADStudio {
    pub(super) fn handle_open_auto_constrain_settings(&mut self) {
        self.auto_constrain_saved = Some(self.auto_constrain_settings.clone());
        self.auto_constrain_selected_row = 0;
        self.auto_constrain_distance_input =
            format!("{}", self.auto_constrain_settings.distance_tolerance);
        self.auto_constrain_angle_input =
            format!("{}", self.auto_constrain_settings.angle_tolerance_deg);
        self.active_modal = Some(super::super::ModalKind::AutoConstrainSettings);
    }

    pub(super) fn handle_add_parametric_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddParametricConstraint {
            kind,
            refs,
            driving_param,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::scene::parametric_constraints::ConstraintKind;
        // GCSMOOTH and FXCONSTRAINT re-prompt after a rejected pick.
        let keep_command = self.tabs[i].active_cmd.as_ref().is_some_and(|command| {
            (kind == ConstraintKind::Smooth && command.name() == "GCSMOOTH")
                || (kind == ConstraintKind::Fixed && command.name() == "FXCONSTRAINT")
        });
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            kind,
            &refs,
            driving_param.as_ref(),
        ) {
            if !keep_command {
                self.tabs[i].active_cmd = None;
            }
            self.tabs[i].snap_result = None;
            self.command_line.push_error(message);
            if keep_command {
                if let Some(prompt) =
                    self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
            }
            return Some(Task::none());
        }
        let scope = self.tabs[i].current_parametric_scope();
        if kind == ConstraintKind::Fixed
            && self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .is_some_and(|set| {
                    set.constraints.iter().any(|existing| {
                        existing.enabled && existing.kind == kind && existing.refs == refs
                    })
                })
        {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line
                .push_error("The constraint already exists on the selected objects.");
            return Some(Task::none());
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let touched: Vec<Handle> = refs.iter().map(|r| r.entity).collect();
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        let retain_size = self.constraint_solve_mode && driving_param.is_none();
        let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
            kind,
            refs,
            driving_param,
        );
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities_with_parametric_policy(
            &changes,
            &[],
            retain_size,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line.push_output("Constraint applied.");
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_equal_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddEqualConstraint {
            first,
            others,
            multiple,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::modules::parametric::EqualConstraintCommand;
        use crate::scene::parametric_constraints::{
            equal_size, equal_size_follower, ConstraintKind, EqualSize, ParametricRef,
        };

        let scope = self.tabs[i].current_parametric_scope();
        // Enter ends a Multiple flow with the reference's summary line.
        let finishing = multiple && others.is_empty();
        let mut followers: Vec<ParametricRef> = Vec::new();
        for other in others {
            let refs = [first, other];
            if other == first
                || self
                    .tabs[i]
                    .scene
                    .validate_parametric_constraint(ConstraintKind::Equal, &refs, None)
                    .is_err()
                || equal_size_follower(&self.tabs[i].scene.document, first, other)
                    .is_none()
            {
                // The reference names the kind the first object
                // asks for.
                let message = match equal_size(&self.tabs[i].scene.document, first) {
                    Some(EqualSize::Radius(_)) => EqualConstraintCommand::INVALID_RADIUS_OBJECT,
                    _ => EqualConstraintCommand::INVALID_LENGTH_OBJECT,
                };
                self.command_line.push_error(message);
                continue;
            }
            let exists = self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .is_some_and(|set| {
                    set.constraints.iter().any(|existing| {
                        existing.enabled
                            && existing.kind == ConstraintKind::Equal
                            && (existing.refs == refs || existing.refs == [other, first])
                    })
                });
            if exists {
                self.command_line
                    .push_error("The constraint already exists on the selected objects.");
                continue;
            }
            followers.push(other);
        }
        // A refused second object is asked for again, as in the
        // reference; Multiple keeps asking anyway.
        let keep = (multiple && !finishing) || (!multiple && followers.is_empty());
        if keep {
            if let Some(prompt) =
                self.tabs[i].active_cmd.as_ref().map(|command| command.prompt())
            {
                self.command_line.push_info(&prompt);
            }
        } else {
            self.tabs[i].active_cmd = None;
        }
        self.tabs[i].snap_result = None;
        if finishing {
            let summary = match equal_size(&self.tabs[i].scene.document, first) {
                Some(EqualSize::Radius(_)) => "Radius of objects made equal",
                _ => "Length of objects made equal",
            };
            self.command_line.push_output(summary);
        }
        if followers.is_empty() {
            return Some(Task::none());
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let mut touched = vec![first.entity];
        for follower in &followers {
            if !touched.contains(&follower.entity) {
                touched.push(follower.entity);
            }
        }
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        // The reference resizes the follower in place — its start (a
        // circle its center) and direction stay, only its length or
        // radius takes the first object's — so do that first and let
        // the relation then hold what already fits.
        for follower in &followers {
            if let Some(resized) =
                equal_size_follower(&self.tabs[i].scene.document, first, *follower)
            {
                self.tabs[i].scene.update_entity(resized);
            }
            let id = self.tabs[i]
                .scene
                .parametric_constraint_set_mut(scope)
                .add(ConstraintKind::Equal, vec![first, *follower], None);
            self.tabs[i].scene.note_parametric_constraint_applied(
                scope,
                id,
                self.constraint_bar_display,
            );
        }
        let changes = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect::<Vec<_>>();
        self.tabs[i].scene.bump_entities_with_parametric_policy(
            &changes,
            &[],
            self.constraint_solve_mode,
        );
        self.tabs[i].dirty = true;
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_check_constraint_point(
        &mut self,
        pick: crate::command::CoincidentPick,
    ) -> Task<Message> {
        let i = self.active_tab;
        use crate::scene::parametric_constraints::{
            nearest_parametric_point, nearest_parametric_point_on_entity, resolve_point,
        };
        let scope = self.tabs[i].current_parametric_scope();
        let resolved = {
            let document = &self.tabs[i].scene.document;
            let world =
                codec::types::Vector3::new(pick.point.x, pick.point.y, pick.point.z);
            let reference = match pick.handle {
                Some(handle) => {
                    nearest_parametric_point_on_entity(document, scope, handle, world)
                }
                None => nearest_parametric_point(document, scope, world, None),
            };
            reference.map(|reference| {
                let point = document
                    .get_entity(reference.entity)
                    .zip(reference.marker)
                    .and_then(|(entity, marker)| resolve_point(entity, marker))
                    .map_or(pick.point, |p| glam::DVec3::new(p.x, p.y, p.z));
                (reference, point)
            })
        };
        let Some((reference, point)) = resolved else {
            return self.apply_cmd_result(CmdResult::ReportError(
                crate::modules::parametric::DimConstraintCommand::NO_POINT.to_string(),
            ));
        };
        let result = self.tabs[i]
            .active_cmd
            .as_mut()
            .map(|command| command.accept_constraint_point(reference, point));
        match result {
            Some(result) => self.apply_cmd_result(result),
            None => Task::none(),
        }
    }

    pub(super) fn handle_add_dimensional_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddDimensionalConstraint {
            kind,
            first,
            second,
            first_point,
            second_point,
            location,
            axis,
            direction,
            name,
            expression,
            renamed,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::modules::annotate::{aligned_dim, linear_dim};
        use crate::scene::named_parameters::{is_valid_name, DrivingValue};
        use crate::scene::parametric_constraints::{
            distance_direction_type, dynamic_dimension_text, ConstraintKind,
        };

        let scope = self.tabs[i].current_parametric_scope();
        let mut refs = vec![first, second];
        refs.extend(direction);
        // The same measurement between the same references is
        // refused after the value, and the command ends.
        let duplicate = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| {
                set.constraints.iter().any(|c| {
                    c.enabled
                        && c.kind == kind
                        && c.refs.len() == refs.len()
                        && c.refs[..2].contains(&first)
                        && c.refs[..2].contains(&second)
                        && c.refs.get(2) == refs.get(2)
                })
            });
        if duplicate {
            self.command_line
                .push_output("The constraint already exists on the selected objects.");
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            return Some(Task::none());
        }
        // A refused value keeps the prompt open, as the reference's
        // editor does. A directed distance validates as the plain
        // two-point distance it also is.
        let (validate_kind, validate_refs) = if kind == ConstraintKind::DistanceDirected {
            (ConstraintKind::Distance, &refs[..2])
        } else {
            (kind, &refs[..])
        };
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            validate_kind,
            validate_refs,
            Some(&DrivingValue::Literal(1.0)),
        ) {
            self.reprompt_active_command(i, message);
            return Some(Task::none());
        }
        let anchors = crate::scene::parametric_constraints::dimensional_anchor_refs(
            &self.tabs[i].scene.document,
            &refs,
        );
        let name = name.trim().to_string();
        if !is_valid_name(&name) {
            self.command_line.push_error(
                "Only alphanumeric names, starting with alpha characters, are allowed.",
            );
            self.reprompt_active_command(i, &format!("Invalid parameter name: {name}."));
            return Some(Task::none());
        }
        if renamed && self.tabs[i].scene.named_parameters().contains(&name) {
            self.reprompt_active_command(i, "Parameter with this name already exists.");
            return Some(Task::none());
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(&name, &expression) {
            self.reprompt_active_command(i, &error.to_string());
            return Some(Task::none());
        }
        let value = match table.resolve(&name) {
            Ok(value) if value.is_finite() => value,
            _ => {
                self.command_line.push_error("Invalid expression.");
                self.reprompt_active_command(
                    i,
                    "The parameter is used in an expression which results in an invalid value for a dimensional constraint.",
                );
                return Some(Task::none());
            }
        };
        // DCFORM Annotational: an ordinary plotted dimension on the
        // current layer that shows the value at dimension precision.
        let annotational = self.constraint_form_annotational;
        let text = dynamic_dimension_text(
            &name,
            value,
            self.tabs[i].scene.constraint_name_format,
            false,
            Some(&expression),
            annotational,
            None,
        );
        let mut entity = if kind == ConstraintKind::Distance {
            aligned_dim::aligned_dimension_entity(
                first_point,
                second_point,
                location,
                Some(text),
            )
        } else {
            linear_dim::linear_dimension_entity(
                first_point,
                second_point,
                location,
                axis,
                Some(text),
            )
        };
        crate::scene::creation_style::apply_current_creation_styles(
            &self.tabs[i].scene.document,
            &mut entity,
        );
        if annotational {
            entity
                .as_entity_mut()
                .set_layer(self.tabs[i].active_layer.clone());
        } else {
            // A dynamic dimension lives on the reference's constraints
            // layer, hidden from the layer lists and never plotted, and
            // draws in the constraint gray, not the current color.
            self.tabs[i].scene.ensure_dynamic_dimension_layer();
            entity.as_entity_mut().set_layer(
                crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER.to_string(),
            );
            entity.common_mut().color = codec::types::Color::Rgb {
                r: 103,
                g: 109,
                b: 118,
            };
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let mut touched = vec![first.entity];
        for reference in [Some(second), direction].into_iter().flatten() {
            if !touched.contains(&reference.entity) {
                touched.push(reference.entity);
            }
        }
        let pending = self.begin_undo(i, label, touched.len() + 1, false);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        let dimension = self.tabs[i].scene.add_entity(entity);
        let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
        let id = set.add(kind, refs, Some(DrivingValue::Named(name)));
        if direction.is_some() {
            if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
                constraint.distance_direction_type =
                    distance_direction_type::PERPENDICULAR_TO_LINE;
            }
        }
        set.dimensions.insert(id, dimension);
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        self.tabs[i].scene.attach_dimension_association(
            dimension,
            vec![Some(first.entity), Some(second.entity)],
        );
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        // The first point (and a line it belongs to that the distance
        // runs perpendicular to) stays; a value other than the
        // measured one moves the second, as in the reference. Not
        // `retain_size`: the typed value is the new length.
        self.tabs[i]
            .scene
            .bump_entities_with_parametric_policy(&changes, &anchors, false);
        // DYNCONSTRAINTDISPLAY 0 also keeps a new dynamic dimension off screen.
        self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
        self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_radial_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddRadialConstraint {
            circle,
            center,
            radius,
            location,
            diameter,
            name,
            expression,
            renamed,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::modules::annotate::{diameter_dim, radius_dim};
        use crate::scene::named_parameters::{is_valid_name, DrivingValue};
        use crate::scene::parametric_constraints::{
            dynamic_dimension_text, ConstraintKind, ParametricRef,
        };

        let scope = self.tabs[i].current_parametric_scope();
        let kind = if diameter {
            ConstraintKind::Diameter
        } else {
            ConstraintKind::Radius
        };
        let refs = vec![circle];
        // A circle already carrying a radius or diameter constraint
        // is refused after the value, as the reference does.
        let duplicate = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| {
                set.constraints.iter().any(|c| {
                    c.enabled
                        && matches!(
                            c.kind,
                            ConstraintKind::Radius | ConstraintKind::Diameter
                        )
                        && c.refs.iter().any(|r| r.entity == circle.entity)
                })
            });
        if duplicate {
            self.command_line
                .push_output("The constraint already exists on the selected objects.");
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            return Some(Task::none());
        }
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            kind,
            &refs,
            Some(&DrivingValue::Literal(1.0)),
        ) {
            self.reprompt_active_command(i, message);
            return Some(Task::none());
        }
        let name = name.trim().to_string();
        if !is_valid_name(&name) {
            self.command_line.push_error(
                "Only alphanumeric names, starting with alpha characters, are allowed.",
            );
            self.reprompt_active_command(i, &format!("Invalid parameter name: {name}."));
            return Some(Task::none());
        }
        if renamed && self.tabs[i].scene.named_parameters().contains(&name) {
            self.reprompt_active_command(i, "Parameter with this name already exists.");
            return Some(Task::none());
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(&name, &expression) {
            self.reprompt_active_command(i, &error.to_string());
            return Some(Task::none());
        }
        // A circle has no zero or negative size: the value is
        // refused and the prompt comes back.
        let value = match table.resolve(&name) {
            Ok(value) if value.is_finite() && value > 0.0 => value,
            _ => {
                self.command_line.push_error("Invalid expression.");
                self.reprompt_active_command(
                    i,
                    "The parameter is used in an expression which results in an invalid value for a dimensional constraint.",
                );
                return Some(Task::none());
            }
        };
        let annotational = self.constraint_form_annotational;
        let text = dynamic_dimension_text(
            &name,
            value,
            self.tabs[i].scene.constraint_name_format,
            false,
            Some(&expression),
            annotational,
            None,
        );
        let entity = if diameter {
            diameter_dim::diameter_constraint_entity(center, radius, location, Some(text))
        } else {
            radius_dim::radius_constraint_entity(center, radius, location, Some(text))
        };
        let Some(mut entity) = entity else {
            self.reprompt_active_command(i, "No valid constraint point found.");
            return Some(Task::none());
        };
        crate::scene::creation_style::apply_current_creation_styles(
            &self.tabs[i].scene.document,
            &mut entity,
        );
        if annotational {
            entity
                .as_entity_mut()
                .set_layer(self.tabs[i].active_layer.clone());
        } else {
            self.tabs[i].scene.ensure_dynamic_dimension_layer();
            entity.as_entity_mut().set_layer(
                crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER.to_string(),
            );
            entity.common_mut().color = codec::types::Color::Rgb {
                r: 103,
                g: 109,
                b: 118,
            };
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        // The centre stays where it is; the value moves the rim.
        let anchors = vec![ParametricRef::center(circle.entity)];
        let pending = self.begin_undo(
            i,
            if diameter {
                "Diameter constraint"
            } else {
                "Radius constraint"
            },
            2,
            false,
        );
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        let dimension = self.tabs[i].scene.add_entity(entity);
        let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
        let id = set.add(kind, refs, Some(DrivingValue::Named(name)));
        set.dimensions.insert(id, dimension);
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        self.tabs[i]
            .scene
            .attach_dimension_association(dimension, vec![Some(circle.entity)]);
        let changes = vec![(circle.entity, crate::scene::ChangeKind::Modified)];
        self.tabs[i]
            .scene
            .bump_entities_with_parametric_policy(&changes, &anchors, false);
        self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
        self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_angular_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddAngularConstraint {
            refs,
            points,
            location,
            sector,
            name,
            expression,
            renamed,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::modules::annotate::angular_dim;
        use crate::scene::named_parameters::{is_valid_name, DrivingValue};
        use crate::scene::parametric_constraints::{
            angle_decimals, dynamic_dimension_text, ConstraintKind, ParametricRef,
        };

        let scope = self.tabs[i].current_parametric_scope();
        let kind = if refs.len() == 2 {
            ConstraintKind::Angle
        } else {
            ConstraintKind::Angle3Point
        };
        // The same angle between the same sides is refused after the
        // value (whichever sector), and the command ends.
        let duplicate = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| {
                set.constraints.iter().any(|c| {
                    c.enabled
                        && c.kind == kind
                        && c.refs.len() == refs.len()
                        && if kind == ConstraintKind::Angle {
                            c.refs.contains(&refs[0]) && c.refs.contains(&refs[1])
                        } else {
                            c.refs == refs
                        }
                })
            });
        if duplicate {
            self.command_line
                .push_output("The constraint already exists on the selected objects.");
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            return Some(Task::none());
        }
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            kind,
            &refs,
            Some(&DrivingValue::Literal(1.0)),
        ) {
            self.reprompt_active_command(i, message);
            return Some(Task::none());
        }
        let name = name.trim().to_string();
        if !is_valid_name(&name) {
            self.command_line.push_error(
                "Only alphanumeric names, starting with alpha characters, are allowed.",
            );
            self.reprompt_active_command(i, &format!("Invalid parameter name: {name}."));
            return Some(Task::none());
        }
        if renamed && self.tabs[i].scene.named_parameters().contains(&name) {
            self.reprompt_active_command(i, "Parameter with this name already exists.");
            return Some(Task::none());
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(&name, &expression) {
            self.reprompt_active_command(i, &error.to_string());
            return Some(Task::none());
        }
        let value = match table.resolve(&name) {
            Ok(value) if value.is_finite() => value,
            _ => {
                self.command_line.push_error("Invalid expression.");
                self.reprompt_active_command(
                    i,
                    "The parameter is used in an expression which results in an invalid value for a dimensional constraint.",
                );
                return Some(Task::none());
            }
        };
        let annotational = self.constraint_form_annotational;
        let decimals = angle_decimals(&self.tabs[i].scene.document, None);
        let text = dynamic_dimension_text(
            &name,
            value,
            self.tabs[i].scene.constraint_name_format,
            false,
            Some(&expression),
            annotational,
            Some(decimals),
        );
        let entity = if kind == ConstraintKind::Angle {
            angular_dim::angular_two_line_entity(
                points[0], points[1], points[2], points[3], location, Some(text),
            )
        } else {
            angular_dim::angular_three_point_entity(
                points[0], points[1], points[2], location, Some(text),
            )
        };
        let Some(mut entity) = entity else {
            self.reprompt_active_command(i, "No valid constraint point found.");
            return Some(Task::none());
        };
        crate::scene::creation_style::apply_current_creation_styles(
            &self.tabs[i].scene.document,
            &mut entity,
        );
        if annotational {
            entity
                .as_entity_mut()
                .set_layer(self.tabs[i].active_layer.clone());
        } else {
            self.tabs[i].scene.ensure_dynamic_dimension_layer();
            entity.as_entity_mut().set_layer(
                crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER.to_string(),
            );
            entity.common_mut().color = codec::types::Color::Rgb {
                r: 103,
                g: 109,
                b: 118,
            };
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let mut touched: Vec<Handle> = Vec::new();
        for reference in &refs {
            if !touched.contains(&reference.entity) {
                touched.push(reference.entity);
            }
        }
        // The first side (or the vertex and first point) stays; a
        // value other than the measured one turns the other side.
        let anchors: Vec<ParametricRef> =
            crate::scene::parametric_constraints::dimensional_anchor_refs(
                &self.tabs[i].scene.document,
                &refs,
            );
        let association = if kind == ConstraintKind::Angle {
            vec![
                Some(refs[0].entity),
                Some(refs[0].entity),
                Some(refs[1].entity),
                Some(refs[1].entity),
            ]
        } else {
            vec![
                Some(refs[1].entity),
                Some(refs[0].entity),
                Some(refs[2].entity),
            ]
        };
        let pending = self.begin_undo(i, "Angular constraint", touched.len() + 1, false);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        let dimension = self.tabs[i].scene.add_entity(entity);
        let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
        let id = set.add(kind, refs, Some(DrivingValue::Named(name)));
        if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
            constraint.angle_sector = sector;
        }
        set.dimensions.insert(id, dimension);
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        self.tabs[i]
            .scene
            .attach_dimension_association(dimension, association);
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i]
            .scene
            .bump_entities_with_parametric_policy(&changes, &anchors, false);
        self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
        self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_make_parallel(&mut self, result: CmdResult) -> Task<Message> {
        let i = self.active_tab;
        let CmdResult::MakeParallel {
            first_line,
            first_ends,
            first_pick,
            second_line,
            second_ends,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::scene::parametric_constraints::{
            resolve_point, two_lines_placed_entity, ConstraintKind,
        };
        let scope = self.tabs[i].current_parametric_scope();
        let refs = [first_line, second_line];
        if let Err(message) =
            self.tabs[i]
                .scene
                .validate_parametric_constraint(ConstraintKind::Parallel, &refs, None)
        {
            return self.apply_cmd_result(CmdResult::ReportError(message.to_string()));
        }
        // The reference's placement of the second line, written
        // before the Parallel constraint holds it there. Built out of
        // line and boxed: this arm's locals would otherwise sit in
        // `apply_cmd_result`'s frame, which is on every click's stack.
        let placed_entity = two_lines_placed_entity(
            &self.tabs[i].scene.document,
            first_ends,
            first_pick,
            second_line,
            second_ends,
        );
        let already = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| {
                set.constraints.iter().any(|c| {
                    c.enabled
                        && c.kind == ConstraintKind::Parallel
                        && c.refs.contains(&first_line)
                        && c.refs.contains(&second_line)
                })
            });
        if !already || placed_entity.is_some() {
            let constraints_before = self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .cloned()
                .unwrap_or_else(|| {
                    crate::scene::parametric_constraints::ParametricConstraintSet::new(
                        scope,
                    )
                });
            let pending = self.begin_undo(i, "Parallel constraint", 1, true);
            if let Some(entity) = placed_entity {
                self.tabs[i].scene.replace_entity_recorded(entity);
            }
            self.tabs[i]
                .scene
                .record_undo_parametric_constraints_before(scope, constraints_before);
            if !already {
                let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                let id = set.add(ConstraintKind::Parallel, refs.to_vec(), None);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
            }
            // Both lines' ends hold the placement; the solve only
            // settles whatever else is tied to them.
            let mut pins = first_ends.to_vec();
            pins.extend(second_ends);
            self.tabs[i].scene.bump_entities_with_parametric_policy(
                &[(second_line.entity, crate::scene::ChangeKind::Modified)],
                &pins,
                self.constraint_solve_mode,
            );
            self.tabs[i].dirty = true;
            if let Some(pd) = pending {
                self.commit_undo_delta(i, pd);
            }
        }
        let ends = {
            let document = &self.tabs[i].scene.document;
            document.get_entity(second_line.entity).and_then(|entity| {
                let world = |reference: crate::scene::parametric_constraints::ParametricRef| {
                    let point = resolve_point(entity, reference.marker?)?;
                    Some((reference, glam::DVec3::new(point.x, point.y, point.z)))
                };
                Some([world(second_ends[0])?, world(second_ends[1])?])
            })
        };
        let Some(ends) = ends else {
            return self.apply_cmd_result(CmdResult::ReportError(
                crate::modules::parametric::DimConstraintCommand::NO_OBJECT.to_string(),
            ));
        };
        let result = self.tabs[i]
            .active_cmd
            .as_mut()
            .map(|command| command.accept_parallel_line(ends));
        match result {
            Some(result) => self.apply_cmd_result(result),
            None => Task::none(),
        }
    }

    pub(super) fn handle_add_fixed_constraint(
        &mut self,
        pick: crate::command::CoincidentPick,
    ) -> Task<Message> {
        let i = self.active_tab;
        use crate::modules::parametric::FixConstraintCommand;
        use crate::scene::parametric_constraints::{
            nearest_parametric_point, nearest_parametric_point_on_entity,
            parametric_curve_ref_for_pick, ConstraintKind,
        };

        let scope = self.tabs[i].current_parametric_scope();
        let document = &self.tabs[i].scene.document;
        let world =
            codec::types::Vector3::new(pick.point.x, pick.point.y, pick.point.z);
        let resolved = match (pick.whole_curve, pick.handle) {
            (true, Some(handle)) => {
                parametric_curve_ref_for_pick(document, scope, handle, world)
                    .ok_or(FixConstraintCommand::INVALID_OBJECT)
            }
            (true, None) => Err(FixConstraintCommand::NO_OBJECT),
            (false, Some(handle)) => {
                nearest_parametric_point_on_entity(document, scope, handle, world)
                    .ok_or(FixConstraintCommand::INVALID_OBJECT)
            }
            (false, None) => nearest_parametric_point(document, scope, world, None)
                .ok_or(FixConstraintCommand::NO_POINT),
        };
        // An off-plane or 3D curve is "not a valid object" to the
        // reference, not a solver limitation.
        let resolved = resolved.and_then(|reference| {
            let supported = !pick.whole_curve
                || self.tabs[i]
                    .scene
                    .validate_parametric_constraint(
                        ConstraintKind::Fixed,
                        &[reference],
                        None,
                    )
                    .is_ok();
            supported
                .then_some(reference)
                .ok_or(FixConstraintCommand::INVALID_OBJECT)
        });
        match resolved {
            Ok(reference) => self.apply_cmd_result(CmdResult::AddParametricConstraint {
                kind: ConstraintKind::Fixed,
                refs: vec![reference],
                driving_param: None,
                label: "Fixed constraint",
            }),
            // A miss re-prompts: `ReportError` keeps the command.
            Err(message) => {
                self.apply_cmd_result(CmdResult::ReportError(message.to_string()))
            }
        }
    }

    pub(super) fn handle_check_horizontal_point(
        &mut self,
        kind: crate::scene::parametric_constraints::ConstraintKind,
        pick: crate::command::CoincidentPick,
    ) -> Task<Message> {
        let i = self.active_tab;
        use crate::modules::parametric::HorizontalConstraintCommand;
        use crate::scene::parametric_constraints::nearest_parametric_point;

        // The reference rejects a missed first point right away and
        // asks for it again; a hit moves on to the second point.
        let scope = self.tabs[i].current_parametric_scope();
        let found = nearest_parametric_point(
            &self.tabs[i].scene.document,
            scope,
            codec::types::Vector3::new(pick.point.x, pick.point.y, pick.point.z),
            None,
        )
        .is_some();
        if !found {
            self.command_line
                .push_error("No valid constraint point found.");
        }
        let command = HorizontalConstraintCommand::resume(kind, found.then_some(pick));
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.tabs[i].snap_result = None;
        Task::none()
    }

    pub(super) fn handle_add_horizontal_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddHorizontalConstraint {
            kind,
            selection,
            direction,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::command::{EntityTransform, HorizontalConstraintSelection};
        use crate::modules::parametric::HorizontalConstraintCommand;
        use crate::scene::parametric_constraints::{
            nearest_parametric_point, nearest_parametric_point_on_entity, resolve_point,
            ConstraintKind, DirectionalAxis, ParametricRef,
        };

        let vertical = kind == ConstraintKind::Vertical;
        let axis = if vertical { "Vertical" } else { "Horizontal" };
        let scope = self.tabs[i].current_parametric_scope();
        let to_world = |point: glam::DVec3| {
            codec::types::Vector3::new(point.x, point.y, point.z)
        };
        let (refs, initial_fixed) = match selection {
            HorizontalConstraintSelection::Reference(reference) => {
                // The reference turns the object about its first
                // vertex: a line's start, the picked polyline
                // segment's first vertex, a text's insertion point,
                // an ellipse's center.
                let initial_fixed = match reference.directional_axis() {
                    Some(DirectionalAxis::EllipseMajor | DirectionalAxis::EllipseMinor) => {
                        vec![ParametricRef::center(reference.entity)]
                    }
                    Some(DirectionalAxis::TextBaseline) => {
                        vec![ParametricRef::point(reference.entity, 0)]
                    }
                    None => vec![ParametricRef::point(
                        reference.entity,
                        reference.segment_index().map_or(0, |index| index as i32),
                    )],
                };
                (vec![reference], initial_fixed)
            }
            HorizontalConstraintSelection::Points(first, second) => {
                let resolve = |pick: crate::command::CoincidentPick| {
                    if let Some(handle) = pick.handle {
                        nearest_parametric_point_on_entity(
                            &self.tabs[i].scene.document,
                            scope,
                            handle,
                            to_world(pick.point),
                        )
                    } else {
                        nearest_parametric_point(
                            &self.tabs[i].scene.document,
                            scope,
                            to_world(pick.point),
                            None,
                        )
                    }
                };
                // A miss asks for that point again, keeping a good
                // first pick; the same point twice asks for another
                // second point — the reference's own re-prompts.
                let (first_ref, second_ref) = match (resolve(first), resolve(second)) {
                    (Some(first_ref), Some(second_ref)) if first_ref != second_ref => {
                        (first_ref, second_ref)
                    }
                    (first_ref, second_ref) => {
                        let (message, keep_first) = if first_ref.is_none() {
                            ("No valid constraint point found.", None)
                        } else if second_ref.is_none() {
                            ("No valid constraint point found.", Some(first))
                        } else {
                            (
                                "The object or point is already selected.  Select a different object or constraint point.",
                                Some(first),
                            )
                        };
                        let command = HorizontalConstraintCommand::resume(kind, keep_first);
                        self.command_line.push_error(message);
                        self.command_line
                            .push_info(&crate::command::CadCommand::prompt(&command));
                        self.tabs[i].active_cmd = Some(Box::new(command));
                        self.tabs[i].snap_result = None;
                        return Some(Task::none());
                    }
                };
                (vec![first_ref, second_ref], vec![first_ref])
            }
        };
        let axis_length = direction.x.hypot(direction.y);
        if axis_length <= 1.0e-12 {
            self.command_line.push_error(&format!(
                "{axis}: the current UCS {} axis is not supported.",
                if vertical { "Y" } else { "X" }
            ));
            return Some(Task::none());
        }
        let direction = codec::types::Vector3::new(
            direction.x / axis_length,
            direction.y / axis_length,
            0.0,
        );
        if let Err(message) =
            self.tabs[i].scene.validate_parametric_constraint(kind, &refs, None)
        {
            self.command_line.push_error(message);
            return Some(Task::none());
        }
        if self
            .tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| set.contains_axis_constraint(kind, &refs, direction))
        {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line
                .push_error("The constraint already exists on the selected objects.");
            return Some(Task::none());
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let mut touched = Vec::new();
        for reference in &refs {
            if !touched.contains(&reference.entity) {
                touched.push(reference.entity);
            }
        }
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        // Two points on different objects: the reference slides the
        // second object rigidly onto the axis through the first
        // point instead of re-solving its shape, so move it first
        // and let the constraint then hold what already fits.
        if let [first_ref, second_ref] = refs.as_slice() {
            if first_ref.entity != second_ref.entity {
                let position = |reference: &ParametricRef| {
                    self.tabs[i]
                        .scene
                        .document
                        .get_entity(reference.entity)
                        .zip(reference.marker)
                        .and_then(|(entity, marker)| resolve_point(entity, marker))
                };
                if let (Some(first_point), Some(second_point)) =
                    (position(first_ref), position(second_ref))
                {
                    let normal = (-direction.y, direction.x);
                    let offset = (second_point.x - first_point.x) * normal.0
                        + (second_point.y - first_point.y) * normal.1;
                    if offset.abs() > 1.0e-9 {
                        self.tabs[i].scene.transform_entities(
                            &[second_ref.entity],
                            &EntityTransform::Translate(glam::DVec3::new(
                                -offset * normal.0,
                                -offset * normal.1,
                                0.0,
                            )),
                        );
                    }
                }
            }
        }
        // The solver cannot start from an axis lying exactly across
        // the datum (its equations are singular there), so turn the
        // object onto the axis about its anchor first — the
        // reference turns it about that anchor too — and let the
        // constraint hold what already fits.
        if let Some((handle, anchor, end, vertex)) =
            crate::scene::parametric_constraints::axis_alignment_target(
                &self.tabs[i].scene.document,
                &refs,
            )
        {
            let axis = (end.x - anchor.x, end.y - anchor.y);
            let length = axis.0.hypot(axis.1);
            let target = if axis.0 * direction.x + axis.1 * direction.y >= 0.0 {
                (direction.x, direction.y)
            } else {
                (-direction.x, -direction.y)
            };
            let angle = (axis.0 * target.1 - axis.1 * target.0)
                .atan2(axis.0 * target.0 + axis.1 * target.1);
            if length > 1.0e-9 && angle.abs() > 1.0e-9 {
                match vertex {
                    Some(index) => {
                        let moved =
                            self.tabs[i].scene.document.get_entity(handle).cloned();
                        if let Some(mut entity) = moved {
                            if crate::scene::parametric_constraints::set_polyline_vertex(
                                &mut entity,
                                index,
                                anchor.x + length * target.0,
                                anchor.y + length * target.1,
                            ) {
                                self.tabs[i].scene.update_entity(entity);
                            }
                        }
                    }
                    None => self.tabs[i].scene.transform_entities(
                        &[handle],
                        &EntityTransform::Rotate {
                            center: glam::DVec3::new(anchor.x, anchor.y, anchor.z),
                            axis: glam::DVec3::Z,
                            angle_rad: angle,
                        },
                    ),
                }
            }
        }
        let id = self.tabs[i]
            .scene
            .parametric_constraint_set_mut(scope)
            .add_axis_constraint(kind, refs, direction);
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect::<Vec<_>>();
        self.tabs[i].scene.bump_entities_with_parametric_policy(
            &changes,
            &initial_fixed,
            self.constraint_solve_mode,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line
            .push_output(&format!("{axis} constraint applied."));
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_symmetric_constraint(
        &mut self,
        selection: crate::command::SymmetricConstraintSelection,
        axis: crate::scene::parametric_constraints::ParametricRef,
        label: &'static str,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        use crate::command::SymmetricConstraintSelection;
        use crate::scene::parametric_constraints::ConstraintKind;

        let scope = self.tabs[i].current_parametric_scope();
        let to_world = |point: glam::DVec3| {
            codec::types::Vector3::new(point.x, point.y, point.z)
        };
        let resolve_point = |pick: crate::command::CoincidentPick| {
            if let Some(handle) = pick.handle {
                crate::scene::parametric_constraints::nearest_parametric_point_on_entity(
                    &self.tabs[i].scene.document,
                    scope,
                    handle,
                    to_world(pick.point),
                )
            } else {
                crate::scene::parametric_constraints::nearest_parametric_point(
                    &self.tabs[i].scene.document,
                    scope,
                    to_world(pick.point),
                    None,
                )
            }
        };
        let (first, second) = match selection {
            SymmetricConstraintSelection::Objects(first, second) => (first, second),
            SymmetricConstraintSelection::Points(first, second) => {
                let (Some(first), Some(second)) =
                    (resolve_point(first), resolve_point(second))
                else {
                    self.command_line.push_error(
                        "Symmetric: select supported endpoints, centers, midpoints, or vertices.",
                    );
                    return Some(Task::none());
                };
                (first, second)
            }
        };
        if first == second
            || axis.entity == first.entity
            || axis.entity == second.entity
        {
            self.command_line
                .push_error("Symmetric: select two different references and a separate line axis.");
            return Some(Task::none());
        }
        let refs = vec![first, second, axis];
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            ConstraintKind::Symmetric,
            &refs,
            None,
        ) {
            self.command_line.push_error(message);
            return Some(Task::none());
        }
        let duplicate = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| {
                set.constraints.iter().any(|constraint| {
                    constraint.enabled
                        && constraint.kind == ConstraintKind::Symmetric
                        && matches!(constraint.refs.as_slice(), [a, b, m]
                            if *m == axis
                                && ((*a == first && *b == second)
                                    || (*a == second && *b == first)))
                })
            });
        if duplicate {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line
                .push_error("The constraint already exists on the selected objects.");
            return Some(Task::none());
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let mut touched = Vec::new();
        for reference in &refs {
            if !touched.contains(&reference.entity) {
                touched.push(reference.entity);
            }
        }
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Symmetric,
            refs,
            None,
        );
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect::<Vec<_>>();
        self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
            &changes,
            &[first, axis],
            false,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line
            .push_output("Symmetric constraint applied.");
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_perpendicular_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddPerpendicularConstraint {
            first,
            second,
            first_fixed,
            second_start,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::scene::parametric_constraints::ConstraintKind;

        let refs = vec![first, second];
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            ConstraintKind::Perpendicular,
            &refs,
            None,
        ) {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line.push_error(message);
            return Some(Task::none());
        }
        let scope = self.tabs[i].current_parametric_scope();
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let touched = if first.entity == second.entity {
            vec![first.entity]
        } else {
            vec![first.entity, second.entity]
        };
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Perpendicular,
            refs,
            None,
        );
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect::<Vec<_>>();
        self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
            &changes,
            &[first_fixed, second_start],
            true,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line.push_output("Constraint applied.");
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_tangent_constraint(
        &mut self,
        first: crate::scene::parametric_constraints::ParametricRef,
        second: crate::scene::parametric_constraints::ParametricRef,
        label: &'static str,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        use crate::scene::parametric_constraints::ConstraintKind;

        let refs = vec![first, second];
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            ConstraintKind::Tangent,
            &refs,
            None,
        ) {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line.push_error(message);
            return Some(Task::none());
        }
        let scope = self.tabs[i].current_parametric_scope();
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let touched = if first.entity == second.entity {
            vec![first.entity]
        } else {
            vec![first.entity, second.entity]
        };
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Tangent,
            refs,
            None,
        );
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect::<Vec<_>>();
        self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
            &changes,
            &[first],
            true,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line.push_output("Constraint applied.");
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_concentric_constraint(
        &mut self,
        first: crate::scene::parametric_constraints::ParametricRef,
        second: crate::scene::parametric_constraints::ParametricRef,
        label: &'static str,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        use crate::scene::parametric_constraints::ConstraintKind;

        let refs = vec![first, second];
        if let Err(message) = self.tabs[i].scene.validate_parametric_constraint(
            ConstraintKind::Concentric,
            &refs,
            None,
        ) {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line.push_error(message);
            return Some(Task::none());
        }
        let scope = self.tabs[i].current_parametric_scope();
        let duplicate = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .is_some_and(|set| {
                set.constraints.iter().any(|constraint| {
                    constraint.enabled
                        && constraint.kind == ConstraintKind::Concentric
                        && matches!(constraint.refs.as_slice(), [a, b]
                            if (*a == first && *b == second)
                                || (*a == second && *b == first))
                })
            });
        if duplicate {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line
                .push_output("The Concentric constraint already exists.");
            return Some(Task::none());
        }
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let touched = if first.entity == second.entity {
            vec![first.entity]
        } else {
            vec![first.entity, second.entity]
        };
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        let id = self.tabs[i].scene.parametric_constraint_set_mut(scope).add(
            ConstraintKind::Concentric,
            refs,
            None,
        );
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect::<Vec<_>>();
        self.tabs[i].scene.bump_entities_with_initial_parametric_policy(
            &changes,
            &[first],
            true,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line.push_output("Constraint applied.");
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_coincident_constraint(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddCoincidentConstraint {
            first,
            second,
            multiple,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        let scope = self.tabs[i].current_parametric_scope();
        let to_world = |p: glam::DVec3| codec::types::Vector3::new(p.x, p.y, p.z);
        let resolve = |pick: crate::command::CoincidentPick,
                       exclude: Option<Handle>| {
            if pick.whole_curve {
                pick.handle.and_then(|handle| {
                    crate::scene::parametric_constraints::parametric_curve_ref_for_pick(
                        &self.tabs[i].scene.document,
                        scope,
                        handle,
                        to_world(pick.point),
                    )
                })
            } else if let Some(handle) = pick.handle {
                crate::scene::parametric_constraints::nearest_parametric_point_on_entity(
                    &self.tabs[i].scene.document,
                    scope,
                    handle,
                    to_world(pick.point),
                )
            } else {
                crate::scene::parametric_constraints::nearest_parametric_point(
                    &self.tabs[i].scene.document,
                    scope,
                    to_world(pick.point),
                    exclude,
                )
            }
        };
        let resolved_first = resolve(first, None);
        let resolved_second = resolved_first
            .and_then(|reference| resolve(second, Some(reference.entity)));
        let (Some(first_ref), Some(second_ref)) = (resolved_first, resolved_second) else {
            self.command_line.push_error(
                "Coincident: select a supported endpoint, center, midpoint, vertex, or curve.",
            );
            return Some(Task::none());
        };
        let (kind, refs) = match (first.whole_curve, second.whole_curve) {
            (false, false) if first_ref != second_ref => (
                crate::scene::parametric_constraints::ConstraintKind::Coincident,
                vec![first_ref, second_ref],
            ),
            (true, false) => (
                crate::scene::parametric_constraints::ConstraintKind::PointOnCurve,
                vec![second_ref, first_ref],
            ),
            (false, true) => (
                crate::scene::parametric_constraints::ConstraintKind::PointOnCurve,
                vec![first_ref, second_ref],
            ),
            _ => {
                self.command_line
                    .push_error("Coincident: select one point and one curve, or two points.");
                return Some(Task::none());
            }
        };
        if let Err(error) = self.tabs[i]
            .scene
            .validate_parametric_constraint(kind, &refs, None)
        {
            self.command_line.push_error(error);
            return Some(Task::none());
        }
        let touched: Vec<_> = refs.iter().map(|reference| reference.entity).collect();
        let constraints_before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        let pending = self.begin_undo(i, label, touched.len(), true);
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        let id = self.tabs[i]
            .scene
            .parametric_constraint_set_mut(scope)
            .add(kind, refs, None);
        self.tabs[i].scene.note_parametric_constraint_applied(
            scope,
            id,
            self.constraint_bar_display,
        );
        let changes: Vec<_> = touched
            .iter()
            .copied()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities_with_parametric_policy(
            &changes,
            &[first_ref],
            self.constraint_solve_mode,
        );
        self.tabs[i].dirty = true;
        self.tabs[i].snap_result = None;
        if !multiple {
            self.tabs[i].active_cmd = None;
        }
        self.command_line.push_output("Coincident constraint applied.");
        if multiple {
            if let Some(prompt) = self.tabs[i].active_cmd.as_ref().map(|cmd| cmd.prompt()) {
                self.command_line.push_info(&prompt);
            }
            self.command_line.set_step_options(
                self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|cmd| cmd.options())
                    .unwrap_or_default(),
            );
        }
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_add_auto_coincident_constraints(&mut self, handles: Vec<Handle>) {
        let i = self.active_tab;
        use crate::scene::parametric_constraints::ConstraintKind;
        let scope = self.tabs[i].current_parametric_scope();
        let inferred = self.tabs[i]
            .scene
            .inferred_coincident_constraints(scope, &handles);
        let count = inferred.len();
        if count > 0 {
            let constraints_before = self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .cloned()
                .unwrap_or_else(|| {
                    crate::scene::parametric_constraints::ParametricConstraintSet::new(
                        scope,
                    )
                });
            let pending = self.begin_undo(i, "Coincident auto constrain", handles.len(), true);
            self.tabs[i]
                .scene
                .record_undo_parametric_constraints_before(scope, constraints_before);
            for refs in inferred {
                let id = self.tabs[i]
                    .scene
                    .parametric_constraint_set_mut(scope)
                    .add(ConstraintKind::Coincident, refs, None);
                self.tabs[i].scene.note_parametric_constraint_applied(
                    scope,
                    id,
                    self.constraint_bar_display,
                );
            }
            let changes: Vec<_> = handles
                .iter()
                .copied()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities(&changes);
            self.tabs[i].dirty = true;
            self.refresh_properties();
            if let Some(pd) = pending {
                self.commit_undo_delta(i, pd);
            }
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.command_line
            .push_output(format!("{} coincident constraint(s) applied.", count).as_str());
    }

    pub(super) fn handle_add_point_on_entity_constraint(
        &mut self,
        result: CmdResult,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::AddPointOnEntityConstraint {
            point,
            target,
            kind,
            label,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        let scope = self.tabs[i].current_parametric_scope();
        let to_world = |p: glam::DVec3| codec::types::Vector3::new(p.x, p.y, p.z);
        let resolved = crate::scene::parametric_constraints::nearest_parametric_point(
            &self.tabs[i].scene.document,
            scope,
            to_world(point),
            Some(target),
        );
        match resolved {
            Some(point_ref) => {
                use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef};
                // `CenterPoint` addresses its whole-entity side via
                // the center marker (same solve as Coincident /
                // Concentric — `ConstraintKind::CenterPoint`'s own
                // doc comment); `Midpoint`/`PointOnCurve` address it
                // as a whole entity.
                let target_ref = if kind == ConstraintKind::CenterPoint {
                    ParametricRef::center(target)
                } else {
                    ParametricRef::whole(target)
                };
                return Some(self.apply_cmd_result(CmdResult::AddParametricConstraint {
                    kind,
                    refs: vec![point_ref, target_ref],
                    driving_param: None,
                    label,
                }));
            }
            None => {
                self.command_line.push_error(
                    "Pick didn't land on a point (endpoint, center, or vertex) — enable an Endpoint/Center object snap and try again.",
                );
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
            }
        }
        None
    }

    pub(super) fn handle_add_equal_distance_constraint(
        &mut self,
        points: [glam::DVec3; 4],
        label: &'static str,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let scope = self.tabs[i].current_parametric_scope();
        let to_world = |p: glam::DVec3| codec::types::Vector3::new(p.x, p.y, p.z);
        let mut refs = Vec::with_capacity(4);
        for p in points {
            let Some(r) = crate::scene::parametric_constraints::nearest_parametric_point(
                &self.tabs[i].scene.document,
                scope,
                to_world(p),
                None,
            ) else {
                refs.clear();
                break;
            };
            refs.push(r);
        }
        if refs.len() == 4 {
            return Some(self.apply_cmd_result(CmdResult::AddParametricConstraint {
                kind: crate::scene::parametric_constraints::ConstraintKind::EqualDistance,
                refs,
                driving_param: None,
                label,
            }));
        }
        self.command_line.push_error(
            "Equal Distance: a pick didn't land on a point (endpoint, center, or vertex) — enable an Endpoint/Center object snap and try again.",
        );
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        None
    }
}
