use super::*;

/// `old` as a whole identifier in `source` becomes `new`.
fn replace_identifier(source: &str, old: &str, new: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        if !token.is_empty() {
            out.push_str(if token == old { new } else { token.as_str() });
            token.clear();
        }
    };
    for c in source.chars() {
        if c.is_alphanumeric() || c == '_' {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);
    out
}

impl OpenCADStudio {
    /// Drops what removed dimensional constraints leave behind: their dynamic
    /// dimensions, and their parameters when no other constraint reads them.
    pub(crate) fn purge_dimensional_extras(
        &mut self,
        i: usize,
        dimensions: Vec<Handle>,
        parameters: Vec<String>,
    ) {
        if !dimensions.is_empty() {
            self.tabs[i].scene.erase_entities(&dimensions);
        }
        let unused: Vec<String> = parameters
            .into_iter()
            .filter(|name| {
                !self.tabs[i]
                    .scene
                    .parametric_constraints
                    .iter()
                    .flat_map(|set| set.constraints.iter())
                    .any(|constraint| {
                        matches!(
                            &constraint.driving_param,
                            Some(crate::scene::named_parameters::DrivingValue::Named(used))
                                if used == name
                        )
                    })
            })
            .collect();
        if !unused.is_empty() {
            self.tabs[i].scene.record_undo_named_parameters_before();
            for name in &unused {
                self.tabs[i].scene.named_parameters_mut().remove(name);
            }
        }
    }

    /// `Dynamic` or `Annotational`, the DCFORM setting.
    pub(crate) fn constraint_form_name(&self) -> &'static str {
        if self.constraint_form_annotational {
            "Annotational"
        } else {
            "Dynamic"
        }
    }

    /// The value prompt a double-clicked constraint dimension opens.
    pub(crate) fn dynamic_dimension_value_command(
        &self,
        i: usize,
        handle: Handle,
    ) -> Option<crate::modules::parametric::DimensionValueCommand> {
        use crate::scene::named_parameters::DrivingValue;
        let scene = &self.tabs[i].scene;
        for set in &scene.parametric_constraints {
            let Some((id, _)) = set
                .dimensions
                .iter()
                .find(|(_, dimension)| **dimension == handle)
            else {
                continue;
            };
            let constraint = set.get(*id)?;
            let Some(DrivingValue::Named(name)) = &constraint.driving_param else {
                return None;
            };
            let table = if set.local_parameters.is_empty() {
                scene.named_parameters()
            } else {
                &set.local_parameters
            };
            let current = table
                .get(name)
                .map(|parameter| parameter.source.trim().to_string())
                .unwrap_or_default();
            return Some(crate::modules::parametric::DimensionValueCommand::new(
                name.clone(),
                current,
            ));
        }
        None
    }

    /// Applies `expression` or `newname=expression` to parameter `name`:
    /// the dimension value prompt and -PARAMETERS Edit.
    pub(crate) fn apply_parameter_input(&mut self, i: usize, name: &str, input: &str) {
        let input = input.trim();
        let (target, expression) = match input.split_once('=') {
            Some((new_name, expression)) => {
                (new_name.trim().to_string(), expression.trim().to_string())
            }
            None => (name.to_string(), input.to_string()),
        };
        if expression.is_empty() {
            return;
        }
        if target != name {
            if let Err(error) = self.rename_parameter(i, name, &target) {
                self.command_line.push_error(&error);
                return;
            }
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(&target, &expression) {
            self.command_line.push_error(&error.to_string());
            return;
        }
        if !table
            .resolve(&target)
            .is_ok_and(|value| value.is_finite())
        {
            self.command_line.push_error("Invalid expression.");
            return;
        }
        let pending = self.begin_undo(i, "Edit parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        self.resolve_named_parameter_edit(i, &target);
        self.tabs[i].scene.refresh_dynamic_dimension_texts();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    /// -PARAMETERS New.
    pub(crate) fn create_parameter(&mut self, i: usize, name: &str, expression: &str) {
        if self.tabs[i].scene.named_parameters().contains(name) {
            self.command_line
                .push_error(&format!("Parameter {name} already exists."));
            return;
        }
        let mut table = self.tabs[i].scene.named_parameters().clone();
        if let Err(error) = table.set(name, expression) {
            self.command_line.push_error(&error.to_string());
            return;
        }
        let pending = self.begin_undo(i, "New parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    /// Renames a parameter everywhere: the table, the expressions that use
    /// it and the constraints it drives.
    pub(crate) fn rename_parameter(&mut self, i: usize, old: &str, new: &str) -> Result<(), String> {
        use crate::scene::named_parameters::{is_reserved_name, is_valid_name, DrivingValue};
        let table = self.tabs[i].scene.named_parameters().clone();
        let Some(source) = table.get(old).map(|parameter| parameter.source.clone()) else {
            return Err(format!("Parameter {old} not found."));
        };
        if !is_valid_name(new) || is_reserved_name(new) || table.contains(new) {
            return Err(format!("Invalid parameter name {new}."));
        }
        let mut renamed = table.clone();
        renamed.set(new, &source).map_err(|error| error.to_string())?;
        for parameter in table.iter() {
            if parameter.name == old {
                continue;
            }
            let rewritten = replace_identifier(&parameter.source, old, new);
            if rewritten != parameter.source {
                renamed
                    .set(&parameter.name, &rewritten)
                    .map_err(|error| error.to_string())?;
            }
        }
        renamed.remove(old);
        let pending = self.begin_undo(i, "Rename parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = renamed;
        let scopes: Vec<_> = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .map(|set| set.scope)
            .collect();
        for scope in scopes {
            let before = self.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .filter(|set| {
                    set.constraints.iter().any(|constraint| {
                        matches!(&constraint.driving_param, Some(DrivingValue::Named(name)) if name == old)
                    })
                })
                .cloned();
            let Some(before) = before else {
                continue;
            };
            self.tabs[i]
                .scene
                .record_undo_parametric_constraints_before(scope, before);
            let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
            for constraint in &mut set.constraints {
                if matches!(&constraint.driving_param, Some(DrivingValue::Named(name)) if name == old) {
                    constraint.driving_param = Some(DrivingValue::Named(new.to_string()));
                }
            }
        }
        self.tabs[i].scene.refresh_dynamic_dimension_texts();
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        Ok(())
    }

    /// -PARAMETERS Delete: a parameter no constraint drives with.
    pub(crate) fn delete_parameter(&mut self, i: usize, name: &str) {
        use crate::scene::named_parameters::DrivingValue;
        if !self.tabs[i].scene.named_parameters().contains(name) {
            self.command_line
                .push_error(&format!("Parameter {name} not found."));
            return;
        }
        let in_use = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .any(|constraint| {
                matches!(&constraint.driving_param, Some(DrivingValue::Named(used)) if used == name)
            });
        if in_use {
            self.command_line
                .push_error(&format!("Parameter {name} is used by a constraint."));
            return;
        }
        let pending = self.begin_undo(i, "Delete parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters_mut().remove(name);
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
    }

    fn remove_parametric_constraint(
        &mut self,
        id: crate::scene::parametric_constraints::ConstraintId,
        label: &'static str,
    ) -> bool {
        let i = self.active_tab;
        let scope = self.tabs[i].current_parametric_scope();
        let Some(set) = self.tabs[i].scene.parametric_constraint_set(scope) else {
            return false;
        };
        let Some(constraint) = set.get(id) else {
            return false;
        };
        let touched: Vec<Handle> = constraint.refs.iter().map(|r| r.entity).collect();
        let dimension = set.dimensions.get(&id).copied();
        let parameter = match &constraint.driving_param {
            Some(crate::scene::named_parameters::DrivingValue::Named(name)) => Some(name.clone()),
            _ => None,
        };
        let constraints_before = set.clone();
        let pending = self.begin_undo(i, label, touched.len(), dimension.is_none());
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, constraints_before);
        self.tabs[i]
            .scene
            .parametric_constraint_set_mut(scope)
            .remove(id);
        self.purge_dimensional_extras(
            i,
            dimension.into_iter().collect(),
            parameter.into_iter().collect(),
        );
        self.tabs[i].scene.bump_constraints_epoch();
        if self.tabs[i].scene.selected_constraint == Some(id) {
            self.tabs[i].scene.selected_constraint = None;
        }
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        true
    }

    /// Removes and re-solves the first conflicting constraint in the active scope.
    pub(in crate::app) fn resolve_one_parametric_conflict(&mut self) {
        let i = self.active_tab;
        let scope = self.tabs[i].current_parametric_scope();
        let Some(id) = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .and_then(|set| set.conflicts.first().map(|(id, _)| *id))
        else {
            return;
        };
        self.remove_parametric_constraint(id, "Remove conflicting constraint");
    }

    /// Removes one user-selected constraint from the active standard graph scope.
    pub(in crate::app) fn delete_parametric_constraint(
        &mut self,
        id: crate::scene::parametric_constraints::ConstraintId,
    ) {
        self.remove_parametric_constraint(id, "Delete constraint");
    }

    /// Rebuilds the active tab's `Scene::named_parameters` from the
    /// editor's working buffer and re-solves dependent constraints.
    pub(in crate::app) fn apply_named_parameter_editor_rows(&mut self) {
        let i = self.active_tab;
        // A name shared by more than one row can't be resolved by picking
        // one arbitrarily (`ParameterTable::set` would just let the last
        // one silently win) -- refuse every row sharing that name up front,
        // same as `named_parameters::preview`'s live check.
        let duplicates = crate::ui::window::named_parameters::duplicate_name_rows(
            &self.named_parameter_editor_rows,
        );
        let mut table = crate::scene::named_parameters::ParameterTable::new();
        let mut failed = 0usize;
        for (idx, row) in self.named_parameter_editor_rows.iter().enumerate() {
            let name = row.name.trim();
            if name.is_empty() || duplicates.contains(&idx) {
                continue;
            }
            if let Err(e) = table.set(name, row.formula.trim()) {
                self.command_line
                    .push_error(crate::tf!("Parameter '{}': {}", name, e).as_ref());
                failed += 1;
            }
        }
        if !duplicates.is_empty() {
            self.command_line.push_error(
                crate::tf!(
                    "{} row(s) skipped: duplicate parameter name.",
                    duplicates.len()
                )
                .as_ref(),
            );
            failed += duplicates.len();
        }
        let param_count = table.len();

        // The whole table just changed, not one isolated value, so there is
        // no cheaper "which handles actually moved" test worth doing here —
        // re-solve every scope with a constraint driven by *any* named
        // parameter (mirrors `solve_scope`'s own "full rebuild, not
        // incremental" philosophy).
        let touched: Vec<Handle> = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|c| {
                c.enabled
                    && matches!(
                        c.driving_param,
                        Some(crate::scene::named_parameters::DrivingValue::Named(_))
                    )
            })
            .flat_map(|c| c.refs.iter().map(|r| r.entity))
            .collect();

        // A changed value moves a dimensional constraint's second point; its
        // first point stays, as in the reference.
        let anchors: Vec<crate::scene::parametric_constraints::ParametricRef> = self.tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|c| {
                c.enabled
                    && matches!(
                        c.driving_param,
                        Some(crate::scene::named_parameters::DrivingValue::Named(_))
                    )
            })
            .flat_map(|c| {
                crate::scene::parametric_constraints::dimensional_anchor_refs(
                    &self.tabs[i].scene.document,
                    &c.refs,
                )
            })
            .collect();
        let pending = self.begin_undo(i, "Apply named parameters", touched.len(), true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters = table;
        self.tabs[i].dirty = true;
        if !touched.is_empty() {
            let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities_with_parametric_policy(
                &changes,
                &anchors,
                self.constraint_solve_mode,
            );
        } else {
            self.tabs[i].scene.sync_native_parametric_graph();
        }
        self.tabs[i].scene.refresh_dynamic_dimension_texts();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        if failed == 0 {
            self.command_line
                .push_info(crate::tf!("{} parameter(s) applied.", param_count).as_ref());
        }
    }

    /// Re-solves every entity touched by an enabled constraint driven by
    /// `name` — the targeted, single-row counterpart to
    /// `apply_named_parameter_editor_rows`'s "re-solve what it affects": an
    /// inline Properties-panel edit changes one parameter, not the whole
    /// table, so unlike that whole-table Apply this can cheaply scope the
    /// re-solve to just the entities that actually reference it.
    fn resolve_named_parameter_edit(&mut self, i: usize, name: &str) {
        let readers: Vec<&crate::scene::parametric_constraints::ParametricConstraint> = self
            .tabs[i]
            .scene
            .parametric_constraints
            .iter()
            .flat_map(|set| set.constraints.iter())
            .filter(|c| {
                c.enabled
                    && matches!(&c.driving_param, Some(crate::scene::named_parameters::DrivingValue::Named(n)) if n == name)
            })
            .collect();
        let touched: Vec<Handle> = readers
            .iter()
            .flat_map(|c| c.refs.iter().map(|r| r.entity))
            .collect();
        // A changed value moves a dimensional constraint's second point; its
        // first point (and the line it measures perpendicular to) stays, as
        // in the reference.
        let anchors: Vec<crate::scene::parametric_constraints::ParametricRef> = readers
            .iter()
            .flat_map(|c| {
                crate::scene::parametric_constraints::dimensional_anchor_refs(
                    &self.tabs[i].scene.document,
                    &c.refs,
                )
            })
            .collect();
        self.tabs[i].dirty = true;
        if touched.is_empty() {
            self.tabs[i].scene.sync_native_parametric_graph();
            return;
        }
        let changes: Vec<(Handle, crate::scene::ChangeKind)> = touched
            .into_iter()
            .map(|h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i]
            .scene
            .bump_entities_with_parametric_policy(&changes, &anchors, false);
    }

    /// Commits one field of one Parameters-section row (Properties panel) —
    /// the per-row, commit-on-submit counterpart to
    /// `apply_named_parameter_editor_rows`'s whole-table Apply. A Formula
    /// edit is a plain redefine (`ParameterTable::set` on the existing
    /// name); a Name edit is a remove-then-set (there's no rename primitive
    /// — `ParameterTable` upserts by name), restoring the old name if the
    /// new one fails to validate so nothing is silently lost. Either way
    /// this only re-solves what actually depends on this one parameter —
    /// no "duplicate name" scan across the whole table is needed the way
    /// the buffered modal Apply needs one, since every row here is always
    /// already a real, distinct table entry.
    pub(in crate::app) fn on_prop_param_commit(
        &mut self,
        index: usize,
        field: crate::ui::window::named_parameters::ParamField,
    ) -> Task<Message> {
        use crate::ui::properties::FieldKey;
        use crate::ui::window::named_parameters::ParamField;
        let i = self.active_tab;
        let key = FieldKey::Param(index, field);
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i].properties.edit_buf.remove(&key) else {
            return Task::none();
        };
        let typed = typed.trim().to_string();
        let Some(current) = self.tabs[i]
            .scene
            .named_parameters()
            .iter()
            .nth(index)
            .cloned()
        else {
            self.refresh_properties();
            return Task::none();
        };

        match field {
            ParamField::Formula if typed.is_empty() || typed == current.source => {
                self.refresh_properties();
                return Task::none();
            }
            ParamField::Name if typed.is_empty() || typed == current.name => {
                self.refresh_properties();
                return Task::none();
            }
            ParamField::Name if self.tabs[i].scene.named_parameters().contains(&typed) => {
                self.command_line
                    .push_error(crate::tf!("Parameter '{}' already exists.", typed).as_ref());
                self.refresh_properties();
                return Task::none();
            }
            _ => {}
        }

        let pending = self.begin_undo(i, "Edit named parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();

        let (result, resolve_name) = match field {
            ParamField::Formula => (
                self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .set(&current.name, &typed),
                current.name.clone(),
            ),
            ParamField::Name => {
                let description = self.tabs[i]
                    .scene
                    .named_parameters()
                    .description(&current.name)
                    .to_string();
                self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .remove(&current.name);
                let outcome = self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .set(&typed, &current.source);
                let kept = if outcome.is_err() {
                    let _ = self.tabs[i]
                        .scene
                        .named_parameters_mut()
                        .set(&current.name, &current.source);
                    current.name.as_str()
                } else {
                    typed.as_str()
                };
                self.tabs[i]
                    .scene
                    .named_parameters_mut()
                    .set_description(kept, &description);
                (outcome, typed.clone())
            }
        };

        if let Err(e) = result {
            self.tabs[i].scene.take_undo_recording();
            self.command_line
                .push_error(crate::tf!("Parameter '{}': {}", current.name, e).as_ref());
            self.refresh_properties();
            return Task::none();
        }

        if field == ParamField::Name {
            let affected: Vec<usize> = self.tabs[i]
                .scene
                .parametric_constraints
                .iter()
                .enumerate()
                .filter_map(|(index, set)| {
                    set.constraints
                        .iter()
                        .any(|constraint| {
                            matches!(&constraint.driving_param, Some(crate::scene::named_parameters::DrivingValue::Named(name)) if name == &current.name)
                        })
                        .then_some(index)
                })
                .collect();
            for set_index in affected {
                let scope = self.tabs[i].scene.parametric_constraints[set_index].scope;
                let before = self.tabs[i].scene.parametric_constraints[set_index].clone();
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, before);
                // A parameter rename rewrites the `Named` driving references
                // pointing at it — the glyph labels change with them.
                let mut renamed = false;
                for constraint in
                    &mut self.tabs[i].scene.parametric_constraints[set_index].constraints
                {
                    if let Some(crate::scene::named_parameters::DrivingValue::Named(name)) =
                        &mut constraint.driving_param
                    {
                        if name == &current.name {
                            *name = typed.clone();
                            renamed = true;
                        }
                    }
                }
                if renamed {
                    self.tabs[i].scene.bump_constraints_epoch();
                }
            }
        }
        self.resolve_named_parameter_edit(i, &resolve_name);
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// The dynamic dimension the Properties panel targets: its handle, scope,
    /// constraint id and parameter name.
    fn dynamic_dimension_target(
        &self,
        i: usize,
    ) -> Option<(
        Handle,
        crate::scene::parametric_constraints::ParametricScope,
        crate::scene::parametric_constraints::ConstraintId,
        String,
    )> {
        use crate::scene::named_parameters::DrivingValue;
        use crate::scene::parametric_constraints::dynamic_dimension_constraint;
        let handle = *self.property_target_handles(i).first()?;
        let (set, constraint) =
            dynamic_dimension_constraint(&self.tabs[i].scene.parametric_constraints, handle)?;
        let Some(DrivingValue::Named(name)) = &constraint.driving_param else {
            return None;
        };
        Some((handle, set.scope, constraint.id, name.clone()))
    }

    /// Commits a dynamic dimension's Description row into its parameter.
    pub(in crate::app) fn on_dynamic_dimension_description_commit(
        &mut self,
        field: &'static str,
    ) -> Task<Message> {
        use crate::ui::properties::FieldKey;
        let i = self.active_tab;
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i]
            .properties
            .edit_buf
            .remove(&FieldKey::Geom(field))
        else {
            return Task::none();
        };
        let Some((_, _, _, name)) = self.dynamic_dimension_target(i) else {
            self.refresh_properties();
            return Task::none();
        };
        let pending = self.begin_undo(i, "Constraint description", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i]
            .scene
            .named_parameters_mut()
            .set_description(&name, &typed);
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// Applies a dynamic dimension's Constraint Form or Reference choice.
    pub(in crate::app) fn on_dynamic_dimension_choice(
        &mut self,
        field: &'static str,
        value: &str,
    ) -> Task<Message> {
        use crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER;
        let i = self.active_tab;
        let Some((handle, scope, id, _)) = self.dynamic_dimension_target(i) else {
            return Task::none();
        };
        match field {
            "dyn_constraint_form" => {
                // Annotational: an ordinary dimension on the current layer
                // that plots; Dynamic: the gray one on the constraints layer.
                let annotational = value.eq_ignore_ascii_case("Annotational");
                let active_layer = self.tabs[i].active_layer.clone();
                if !annotational {
                    self.tabs[i].scene.ensure_dynamic_dimension_layer();
                }
                self.apply_property_op(i, "Constraint form", &[handle], |app, handle| {
                    use crate::entities::dim_override as dov;
                    let Some(mut entity) = app.tabs[i].scene.document.get_entity(handle).cloned()
                    else {
                        return;
                    };
                    if annotational {
                        entity.as_entity_mut().set_layer(active_layer.clone());
                        entity.common_mut().color = codec::types::Color::ByLayer;
                        // The dynamic form's screen-size and horizontal-text
                        // overrides go; the style draws it.
                        for code in [
                            dov::DIMSCALE,
                            dov::DIMGAP,
                            dov::DIMEXO,
                            dov::DIMEXE,
                            dov::DIMASZ,
                            dov::DIMTAD,
                            dov::DIMTIH,
                            dov::DIMTOH,
                        ] {
                            dov::set_on_entity(&mut entity, code, None);
                        }
                    } else {
                        entity
                            .as_entity_mut()
                            .set_layer(DYNAMIC_DIMENSION_LAYER.to_string());
                        entity.common_mut().color = codec::types::Color::Rgb {
                            r: 103,
                            g: 109,
                            b: 118,
                        };
                    }
                    app.tabs[i].scene.update_entity(entity);
                });
                // The dynamic form is rescaled to the screen and shows the
                // trimmed value; the annotational one is never hidden.
                self.tabs[i].scene.refresh_dynamic_dimension_scales(true);
                self.tabs[i].scene.refresh_dynamic_dimension_texts();
                self.tabs[i].scene.refresh_hidden_dynamic_dimensions();
            }
            "dyn_constraint_reference" => {
                // A reference constraint reads the geometry instead of
                // driving it; its text sits in parentheses.
                let reference =
                    value.eq_ignore_ascii_case("Yes") || value == crate::t!("Yes").as_ref();
                let Some(before) = self.tabs[i].scene.parametric_constraint_set(scope).cloned()
                else {
                    return Task::none();
                };
                let pending = self.begin_undo(i, "Constraint reference", 0, true);
                self.tabs[i]
                    .scene
                    .record_undo_parametric_constraints_before(scope, before);
                self.tabs[i].scene.record_undo_named_parameters_before();
                let set = self.tabs[i].scene.parametric_constraint_set_mut(scope);
                if let Some(constraint) = set.constraints.iter_mut().find(|c| c.id == id) {
                    constraint.enabled = !reference;
                }
                // Toggling `enabled` changes the glyph placement set (and the
                // dynamic pills), so the memoised placements must miss next
                // frame — same guarantee `set_constraint_enabled` gives.
                self.tabs[i].scene.bump_constraints_epoch();
                self.tabs[i].scene.refresh_dynamic_dimension_texts();
                self.tabs[i].scene.sync_native_parametric_graph();
                self.tabs[i].dirty = true;
                if let Some(pd) = pending {
                    self.commit_undo_delta(i, pd);
                }
                self.refresh_properties();
            }
            _ => {}
        }
        Task::none()
    }

    /// Commits a dynamic dimension's Name or Expression row: the typed text
    /// goes to the Parameters-section row of the constraint's parameter.
    pub(in crate::app) fn on_dynamic_dimension_field_commit(
        &mut self,
        field: &'static str,
        param_field: crate::ui::window::named_parameters::ParamField,
    ) -> Task<Message> {
        use crate::scene::named_parameters::DrivingValue;
        use crate::scene::parametric_constraints::dynamic_dimension_constraint;
        use crate::ui::properties::FieldKey;
        let i = self.active_tab;
        self.tabs[i].properties.active_field = None;
        let Some(typed) = self.tabs[i]
            .properties
            .edit_buf
            .remove(&FieldKey::Geom(field))
        else {
            return Task::none();
        };
        let handles = self.property_target_handles(i);
        let name = handles.first().and_then(|handle| {
            let (_, constraint) =
                dynamic_dimension_constraint(&self.tabs[i].scene.parametric_constraints, *handle)?;
            match &constraint.driving_param {
                Some(DrivingValue::Named(name)) => Some(name.clone()),
                _ => None,
            }
        });
        let index = name.and_then(|name| {
            self.tabs[i]
                .scene
                .named_parameters()
                .iter()
                .position(|parameter| parameter.name == name)
        });
        let Some(index) = index else {
            self.refresh_properties();
            return Task::none();
        };
        self.tabs[i]
            .properties
            .edit_buf
            .insert(FieldKey::Param(index, param_field), typed);
        self.on_prop_param_commit(index, param_field)
    }

    /// Removes Parameters-section row `index` immediately. Any
    /// constraint that referenced it gets the same "undefined reference"
    /// resolve-failure treatment a formula's own bad reference already gets
    /// (`build_constraint`) — re-solving surfaces that rather than needing
    /// special-case handling here.
    pub(in crate::app) fn on_prop_param_delete(&mut self, index: usize) -> Task<Message> {
        let i = self.active_tab;
        let Some(name) = self.tabs[i]
            .scene
            .named_parameters()
            .iter()
            .nth(index)
            .map(|p| p.name.clone())
        else {
            return Task::none();
        };
        let pending = self.begin_undo(i, "Delete named parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        self.tabs[i].scene.named_parameters_mut().remove(&name);
        self.resolve_named_parameter_edit(i, &name);
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    /// The Parameters section's "+ Add parameter" row: appends a fresh,
    /// uniquely-named parameter (`param1`, `param2`, …) the user then
    /// renames/redefines inline via `on_prop_param_commit`. Defaults to `1`,
    /// not `0`: renaming this onto a name an existing Distance/Radius
    /// constraint already references (unusual, but reachable — define
    /// first, wire up the constraint's reference later) would otherwise
    /// briefly collapse that geometry to a zero-length/zero-radius
    /// degenerate state, which is a genuine numerical singularity for a
    /// point-to-point distance constraint to grow back out of (no defined
    /// direction to separate two exactly-coincident points from). `1` never
    /// hits that.
    pub(in crate::app) fn on_prop_param_add_new(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let mut n = 1usize;
        let name = loop {
            let candidate = format!("param{n}");
            if !self.tabs[i].scene.named_parameters().contains(&candidate) {
                break candidate;
            }
            n += 1;
        };
        let pending = self.begin_undo(i, "Add named parameter", 0, true);
        self.tabs[i].scene.record_undo_named_parameters_before();
        let _ = self.tabs[i].scene.named_parameters_mut().set(&name, "1");
        self.tabs[i].scene.sync_native_parametric_graph();
        self.tabs[i].dirty = true;
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        self.refresh_properties();
        Task::none()
    }

    pub(super) fn apply_continuous_constraints(&mut self, i: usize, new_handles: &[codec::Handle]) {
        let supported_creation = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| {
                matches!(
                    command.name(),
                    "LINE"
                        | "PLINE"
                        | "POLYLINE"
                        | "RECTANG"
                        | "RECTANGLE"
                        | "POLYGON"
                        | "CIRCLE"
                        | "ARC"
                )
            });
        if supported_creation {
            self.apply_inferred_constraints(i, new_handles);
        }
    }

    pub(in crate::app) fn apply_inferred_constraints(
        &mut self,
        i: usize,
        changed_handles: &[codec::Handle],
    ) {
        if !self.constraint_infer || changed_handles.is_empty() {
            return;
        }
        let scope = self.tabs[i].current_parametric_scope();
        let owner = scope.owner_handle(&self.tabs[i].scene.document);
        let candidates = self.tabs[i]
            .scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == owner)
            .map(|record| record.entity_handles.clone())
            .unwrap_or_default();
        let inferred = self.tabs[i].scene.inferred_parametric_constraints(
            scope,
            &candidates,
            &self.auto_constrain_settings,
        );
        let inferred: Vec<_> = inferred
            .into_iter()
            .filter(|(_, refs)| {
                refs.iter()
                    .any(|reference| changed_handles.contains(&reference.entity))
            })
            .collect();
        if inferred.is_empty() {
            return;
        }
        let before = self.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .cloned()
            .unwrap_or_else(|| {
                crate::scene::parametric_constraints::ParametricConstraintSet::new(scope)
            });
        self.tabs[i]
            .scene
            .record_undo_parametric_constraints_before(scope, before);
        let mut touched = Vec::new();
        for (kind, refs) in inferred {
            touched.extend(refs.iter().map(|reference| reference.entity));
            let id = self.tabs[i]
                .scene
                .parametric_constraint_set_mut(scope)
                .add(kind, refs, None);
            self.tabs[i].scene.note_parametric_constraint_applied(
                scope,
                id,
                self.constraint_bar_display,
            );
        }
        touched.sort_unstable_by_key(|handle| handle.value());
        touched.dedup();
        let changes: Vec<_> = touched
            .into_iter()
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities_with_parametric_policy(
            &changes,
            &[],
            self.constraint_solve_mode,
        );
    }
}
